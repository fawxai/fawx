use super::{
    direct_utility_progress, BoundedLocalPhase, CycleStream, LoopEngine, TurnExecutionProfile,
    DECOMPOSE_TOOL_NAME,
};
use crate::act::{
    ContinuationToolScope, ProceedUnderConstraints, ToolCacheability, ToolCallClassification,
    ToolExecutor, TurnCommitment,
};
use crate::streaming::StreamEvent;
use fx_core::message::{InternalMessage, ProgressKind};
use fx_llm::ToolCall;

#[derive(Clone, Copy)]
pub(super) struct ToolRoundProgressContext<'a> {
    pub commitment: Option<&'a TurnCommitment>,
    pub pending_tool_scope: Option<&'a ContinuationToolScope>,
    pub pending_artifact_write_target: Option<&'a str>,
    pub turn_execution_profile: &'a TurnExecutionProfile,
    pub bounded_local_phase: BoundedLocalPhase,
    pub tool_executor: &'a dyn ToolExecutor,
}

impl LoopEngine {
    pub(super) fn emit_public_progress(
        &mut self,
        kind: ProgressKind,
        message: impl Into<String>,
        stream: CycleStream<'_>,
    ) {
        let message = message.into();
        let next = (kind, message.clone());
        if self.last_emitted_public_progress.as_ref() == Some(&next) {
            return;
        }
        self.last_emitted_public_progress = Some(next);

        if let Some(bus) = self.public_event_bus() {
            let _ = bus.publish(InternalMessage::ProgressUpdate {
                kind,
                message: message.clone(),
            });
        }
        stream.emit(StreamEvent::Progress { kind, message });
    }

    pub(super) fn publish_turn_state_progress(
        &mut self,
        kind: ProgressKind,
        message: impl Into<String>,
        stream: CycleStream<'_>,
    ) {
        let next = (kind, message.into());
        self.last_turn_state_progress = Some(next.clone());
        self.last_activity_progress = None;
        self.emit_public_progress(next.0, next.1, stream);
    }

    pub(super) fn publish_activity_progress(
        &mut self,
        kind: ProgressKind,
        message: impl Into<String>,
        stream: CycleStream<'_>,
    ) {
        let next = (kind, message.into());
        if self.last_activity_progress.as_ref() == Some(&next) {
            return;
        }
        self.last_activity_progress = Some(next.clone());
        self.emit_public_progress(next.0, next.1, stream);
    }

    pub(super) fn expire_activity_progress(&mut self, stream: CycleStream<'_>) {
        if self.last_activity_progress.take().is_none() {
            return;
        }

        let fallback = self
            .last_turn_state_progress
            .clone()
            .unwrap_or_else(|| self.current_turn_state_progress());
        self.last_turn_state_progress = Some(fallback.clone());
        self.emit_public_progress(fallback.0, fallback.1, stream);
    }

    pub(super) fn current_turn_state_progress(&self) -> (ProgressKind, String) {
        progress_for_turn_state_with_profile(
            self.pending_turn_commitment.as_ref(),
            self.pending_tool_scope.as_ref(),
            self.pending_artifact_write_target.as_deref(),
            self.tool_executor.as_ref(),
            &self.turn_execution_profile,
            self.bounded_local_phase,
        )
    }

    pub(super) fn maybe_publish_reason_progress(&mut self, stream: CycleStream<'_>) {
        let (kind, message) = self.current_turn_state_progress();
        self.publish_turn_state_progress(kind, message, stream);
    }

    pub(super) fn maybe_publish_tool_round_progress(
        &mut self,
        _round: usize,
        calls: &[ToolCall],
        stream: CycleStream<'_>,
    ) {
        let context = ToolRoundProgressContext {
            commitment: self.pending_turn_commitment.as_ref(),
            pending_tool_scope: self.pending_tool_scope.as_ref(),
            pending_artifact_write_target: self.pending_artifact_write_target.as_deref(),
            turn_execution_profile: &self.turn_execution_profile,
            bounded_local_phase: self.bounded_local_phase,
            tool_executor: self.tool_executor.as_ref(),
        };
        let Some((kind, message)) = progress_for_tool_round(context, calls) else {
            return;
        };
        self.publish_activity_progress(kind, message, stream);
    }
}

pub(super) fn progress_for_turn_state_with_profile(
    commitment: Option<&TurnCommitment>,
    pending_tool_scope: Option<&ContinuationToolScope>,
    pending_artifact_write_target: Option<&str>,
    tool_executor: &dyn ToolExecutor,
    turn_execution_profile: &TurnExecutionProfile,
    bounded_local_phase: BoundedLocalPhase,
) -> (ProgressKind, String) {
    if let Some(path) = pending_artifact_write_target {
        return (
            ProgressKind::WritingArtifact,
            format!("Writing the requested artifact to {path}..."),
        );
    }

    if let TurnExecutionProfile::DirectUtility(profile) = turn_execution_profile {
        if commitment.is_none() {
            return direct_utility_progress(profile);
        }
    }

    if matches!(turn_execution_profile, TurnExecutionProfile::BoundedLocal) && commitment.is_none()
    {
        return match bounded_local_phase {
            BoundedLocalPhase::Discovery => (
                ProgressKind::Researching,
                "Inspecting the local workspace to identify the issue...".to_string(),
            ),
            BoundedLocalPhase::Mutation => (
                ProgressKind::Implementing,
                "Applying the local code change...".to_string(),
            ),
            BoundedLocalPhase::Recovery => (
                ProgressKind::Implementing,
                "Reading the exact local context needed to retry the edit...".to_string(),
            ),
            BoundedLocalPhase::Verification => (
                ProgressKind::Implementing,
                "Running one focused local verification...".to_string(),
            ),
            BoundedLocalPhase::Terminal => (
                ProgressKind::Implementing,
                "Summarizing the bounded local run...".to_string(),
            ),
        };
    }

    match commitment {
        Some(TurnCommitment::NeedsDirection(commitment)) => (
            ProgressKind::AwaitingDirection,
            format!(
                "Preparing one blocking question about {}",
                compact_progress_subject(&commitment.blocking_choice)
            ),
        ),
        Some(TurnCommitment::ProceedUnderConstraints(commitment)) => {
            if commitment_focuses_on_implementation(commitment, pending_tool_scope, tool_executor) {
                let subject = commitment
                    .success_target
                    .as_deref()
                    .unwrap_or(commitment.goal.as_str());
                (
                    ProgressKind::Implementing,
                    format!(
                        "Implementing the committed plan: {}",
                        compact_progress_subject(subject)
                    ),
                )
            } else {
                (
                    ProgressKind::Researching,
                    format!(
                        "Working through the committed plan: {}",
                        compact_progress_subject(&commitment.goal)
                    ),
                )
            }
        }
        None => (
            ProgressKind::Researching,
            "Researching the request and planning the next step...".to_string(),
        ),
    }
}

pub(super) fn progress_for_tool_round(
    context: ToolRoundProgressContext<'_>,
    calls: &[ToolCall],
) -> Option<(ProgressKind, String)> {
    if calls.is_empty() {
        return None;
    }

    if let Some(path) = context.pending_artifact_write_target {
        return Some((
            ProgressKind::WritingArtifact,
            format!("Writing the requested artifact to {path}..."),
        ));
    }

    if let Some(path) = first_write_path_from_calls(calls) {
        return Some((
            ProgressKind::WritingArtifact,
            format!("Writing changes to {path}..."),
        ));
    }

    if let Some((kind, detail)) =
        progress_for_round_activity(calls, context.commitment, context.tool_executor)
    {
        return Some((kind, detail));
    }

    let (kind, message) = progress_for_turn_state_with_profile(
        context.commitment,
        context.pending_tool_scope,
        context.pending_artifact_write_target,
        context.tool_executor,
        context.turn_execution_profile,
        context.bounded_local_phase,
    );
    Some((kind, message))
}

fn commitment_focuses_on_implementation(
    commitment: &ProceedUnderConstraints,
    pending_tool_scope: Option<&ContinuationToolScope>,
    tool_executor: &dyn ToolExecutor,
) -> bool {
    match commitment.allowed_tools.as_ref().or(pending_tool_scope) {
        Some(ContinuationToolScope::MutationOnly) => true,
        Some(ContinuationToolScope::Only(names)) => names.iter().any(|name| {
            tool_executor.cacheability(name) == ToolCacheability::SideEffect || name == "write_file"
        }),
        Some(ContinuationToolScope::Full) | None => false,
    }
}

fn first_write_path_from_calls(calls: &[ToolCall]) -> Option<&str> {
    calls.iter().find_map(|call| {
        if call.name != "write_file" {
            return None;
        }

        call.arguments
            .get("path")
            .and_then(serde_json::Value::as_str)
            .filter(|path| !path.trim().is_empty())
    })
}

fn compact_progress_subject(subject: &str) -> String {
    const MAX_PROGRESS_SUBJECT_CHARS: usize = 96;

    let normalized = subject
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();
    let mut chars = normalized.chars();
    let compact: String = chars.by_ref().take(MAX_PROGRESS_SUBJECT_CHARS).collect();
    if chars.next().is_some() {
        format!("{compact}...")
    } else if compact.is_empty() {
        "the current task".to_string()
    } else {
        compact
    }
}

fn progress_for_round_activity(
    calls: &[ToolCall],
    commitment: Option<&TurnCommitment>,
    tool_executor: &dyn ToolExecutor,
) -> Option<(ProgressKind, String)> {
    let representative = calls
        .iter()
        .enumerate()
        .filter_map(|(index, call)| {
            round_activity_descriptor(call, tool_executor.classify_call(call)).map(|descriptor| {
                (
                    descriptor.priority,
                    index,
                    descriptor.kind,
                    descriptor.message,
                    descriptor.countable,
                )
            })
        })
        .max_by_key(|(priority, index, ..)| (*priority, usize::MAX - *index))?;

    let (_, _, kind, mut message, countable) = representative;
    if kind == ProgressKind::Implementing {
        if let Some(TurnCommitment::ProceedUnderConstraints(commitment)) = commitment {
            let subject = commitment
                .success_target
                .as_deref()
                .unwrap_or(commitment.goal.as_str());
            if !message.contains("committed plan") {
                message = format!("{} for {}", message, compact_progress_subject(subject));
            }
        }
    }

    if countable {
        let same_kind_calls = calls
            .iter()
            .filter(|call| {
                round_activity_descriptor(call, tool_executor.classify_call(call))
                    .is_some_and(|descriptor| descriptor.kind == kind)
            })
            .count();
        if same_kind_calls > 1 {
            let noun = match kind {
                ProgressKind::Researching => "lookups",
                ProgressKind::Implementing => "actions",
                ProgressKind::WritingArtifact | ProgressKind::AwaitingDirection => "steps",
            };
            message.push_str(&format!(" ({same_kind_calls} {noun})"));
        }
    }

    Some((kind, message))
}

#[derive(Debug, Clone)]
struct RoundActivityDescriptor {
    priority: u8,
    kind: ProgressKind,
    message: String,
    countable: bool,
}

fn round_activity_descriptor(
    call: &ToolCall,
    classification: ToolCallClassification,
) -> Option<RoundActivityDescriptor> {
    match call.name.as_str() {
        "web_fetch" | "fetch_url" => {
            let target = json_string_arg(&call.arguments, &["url"])
                .map(compact_progress_url)
                .unwrap_or_else(|| "live documentation".to_string());
            Some(RoundActivityDescriptor {
                priority: 80,
                kind: ProgressKind::Researching,
                message: format!("Checking live docs from {target}"),
                countable: true,
            })
        }
        "web_search" | "brave_search" => {
            let query = json_string_arg(&call.arguments, &["query", "q"])
                .map(compact_progress_subject)
                .unwrap_or_else(|| "the current docs".to_string());
            Some(RoundActivityDescriptor {
                priority: 75,
                kind: ProgressKind::Researching,
                message: format!("Searching the web for {query}"),
                countable: true,
            })
        }
        "weather" => {
            let location = json_string_arg(&call.arguments, &["location", "query", "q"])
                .map(compact_progress_subject)
                .unwrap_or_else(|| "the requested location".to_string());
            Some(RoundActivityDescriptor {
                priority: 90,
                kind: ProgressKind::Researching,
                message: format!("Checking the weather for {location}"),
                countable: false,
            })
        }
        "read_file" => {
            let target = json_string_arg(&call.arguments, &["path"])
                .map(compact_progress_path)
                .unwrap_or_else(|| "the workspace".to_string());
            Some(RoundActivityDescriptor {
                priority: 65,
                kind: ProgressKind::Researching,
                message: format!("Reading local files in {target}"),
                countable: true,
            })
        }
        "search_text" => {
            let pattern = json_string_arg(&call.arguments, &["pattern"])
                .map(compact_progress_subject)
                .unwrap_or_else(|| "the requested signals".to_string());
            let scope = json_string_arg(&call.arguments, &["path"])
                .map(compact_progress_path)
                .unwrap_or_else(|| "the workspace".to_string());
            Some(RoundActivityDescriptor {
                priority: 60,
                kind: ProgressKind::Researching,
                message: format!("Searching {scope} for {pattern}"),
                countable: true,
            })
        }
        "run_command" => {
            let command = json_string_arg(&call.arguments, &["command"])
                .map(compact_progress_command)
                .unwrap_or_else(|| "the requested command".to_string());
            let working_dir =
                json_string_arg(&call.arguments, &["working_dir"]).map(compact_progress_path);
            match classification {
                ToolCallClassification::Observation => Some(RoundActivityDescriptor {
                    priority: 62,
                    kind: ProgressKind::Researching,
                    message: match working_dir {
                        Some(dir) => format!("Running local checks with `{command}` in {dir}"),
                        None => format!("Running local checks with `{command}`"),
                    },
                    countable: true,
                }),
                ToolCallClassification::Mutation => Some(RoundActivityDescriptor {
                    priority: 85,
                    kind: ProgressKind::Implementing,
                    message: match working_dir {
                        Some(dir) => format!("Running local commands with `{command}` in {dir}"),
                        None => format!("Running local commands with `{command}`"),
                    },
                    countable: true,
                }),
            }
        }
        "list_directory" => {
            let target = json_string_arg(&call.arguments, &["path"])
                .map(compact_progress_path)
                .unwrap_or_else(|| "the workspace".to_string());
            Some(RoundActivityDescriptor {
                priority: 55,
                kind: ProgressKind::Researching,
                message: format!("Inspecting the directory layout in {target}"),
                countable: true,
            })
        }
        "kernel_manifest" => Some(RoundActivityDescriptor {
            priority: 50,
            kind: ProgressKind::Researching,
            message: "Checking the kernel tool surface and runtime context".to_string(),
            countable: false,
        }),
        DECOMPOSE_TOOL_NAME => Some(RoundActivityDescriptor {
            priority: 45,
            kind: ProgressKind::Researching,
            message: "Breaking the task into smaller execution steps".to_string(),
            countable: false,
        }),
        "current_time" => Some(RoundActivityDescriptor {
            priority: 90,
            kind: ProgressKind::Researching,
            message: "Checking the current time".to_string(),
            countable: false,
        }),
        _ if classification == ToolCallClassification::Mutation => Some(RoundActivityDescriptor {
            priority: 70,
            kind: ProgressKind::Implementing,
            message: format!("Applying changes with {}", call.name),
            countable: true,
        }),
        _ => None,
    }
}

pub(super) fn json_string_arg<'a>(
    arguments: &'a serde_json::Value,
    keys: &[&str],
) -> Option<&'a str> {
    keys.iter().find_map(|key| {
        arguments
            .get(*key)
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    })
}

fn compact_progress_path(path: &str) -> String {
    let normalized = path.trim().replace('\\', "/");
    if normalized.is_empty() {
        return "the workspace".to_string();
    }

    if normalized == "." {
        return "the workspace".to_string();
    }

    if normalized.starts_with("~/") {
        return compact_progress_subject(&normalized);
    }

    let components: Vec<&str> = normalized
        .split('/')
        .filter(|component| !component.is_empty() && *component != ".")
        .collect();
    if components.is_empty() {
        return compact_progress_subject(&normalized);
    }

    let keep = if normalized.ends_with('/') { 2 } else { 3 }.min(components.len());
    let tail = components[components.len().saturating_sub(keep)..].join("/");
    compact_progress_subject(&tail)
}

fn compact_progress_url(url: &str) -> String {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return "the requested URL".to_string();
    }

    let without_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(trimmed);
    let without_query = without_scheme
        .split(['?', '#'])
        .next()
        .unwrap_or(without_scheme);
    let mut parts = without_query.split('/').filter(|part| !part.is_empty());
    let Some(host) = parts.next() else {
        return compact_progress_subject(trimmed);
    };
    if let Some(first_path) = parts.next() {
        compact_progress_subject(&format!("{host}/{first_path}"))
    } else {
        compact_progress_subject(host)
    }
}

fn compact_progress_command(command: &str) -> String {
    const MAX_COMMAND_WORDS: usize = 6;
    const MAX_COMMAND_CHARS: usize = 72;

    let normalized = command
        .split_whitespace()
        .take(MAX_COMMAND_WORDS)
        .collect::<Vec<_>>()
        .join(" ");
    let compact = compact_progress_subject(&normalized);
    let mut chars = compact.chars();
    let truncated: String = chars.by_ref().take(MAX_COMMAND_CHARS).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}
