use super::retry::BlockedToolCall;
use super::{
    artifact_path_candidates, detect_direct_utility_profile, direct_utility_directive,
    direct_utility_tool_names, json_string_arg, summarize_tool_progress, LoopEngine,
    BOUNDED_LOCAL_DISCOVERY_BLOCK_REASON, BOUNDED_LOCAL_DISCOVERY_PHASE_DIRECTIVE,
    BOUNDED_LOCAL_MUTATION_BLOCK_REASON, BOUNDED_LOCAL_MUTATION_NOOP_BLOCK_REASON,
    BOUNDED_LOCAL_MUTATION_PHASE_DIRECTIVE, BOUNDED_LOCAL_RECOVERY_BLOCK_REASON,
    BOUNDED_LOCAL_RECOVERY_PHASE_DIRECTIVE, BOUNDED_LOCAL_TASK_DIRECTIVE,
    BOUNDED_LOCAL_TERMINAL_PHASE_DIRECTIVE, BOUNDED_LOCAL_VERIFICATION_BLOCK_REASON,
    BOUNDED_LOCAL_VERIFICATION_DISCOVERY_BLOCK_REASON, BOUNDED_LOCAL_VERIFICATION_PHASE_DIRECTIVE,
};
use crate::act::ToolResult;
use crate::budget::TerminationConfig;
use crate::loop_engine::direct_inspection::{
    direct_inspection_block_reason, direct_inspection_directive, direct_inspection_tool_names,
    DirectInspectionOwnership, DirectInspectionProfile,
};
use crate::loop_engine::direct_utility::{direct_utility_block_reason, DirectUtilityProfile};
use crate::signals::{LoopStep, SignalKind};
use fx_llm::{ToolCall, ToolDefinition};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) enum TurnExecutionProfile {
    #[default]
    Standard,
    BoundedLocal,
    DirectInspection(DirectInspectionProfile),
    DirectUtility(DirectUtilityProfile),
}

impl TurnExecutionProfile {
    pub(super) fn uses_standard_observation_controls(&self) -> bool {
        matches!(self, Self::Standard)
    }

    pub(super) fn completes_terminally(&self) -> bool {
        matches!(self, Self::DirectInspection(_) | Self::DirectUtility(_))
    }

    pub(super) fn tightened_termination_config(
        &self,
        base: &TerminationConfig,
    ) -> Option<TerminationConfig> {
        match self {
            Self::Standard => None,
            Self::BoundedLocal => {
                let mut tightened = base.clone();
                tightened.nudge_after_tool_turns =
                    tighten_or_default_threshold(tightened.nudge_after_tool_turns, 3);
                tightened.strip_tools_after_nudge = tightened.strip_tools_after_nudge.min(1);
                tightened.tool_round_nudge_after =
                    tighten_or_default_threshold(tightened.tool_round_nudge_after, 2);
                tightened.tool_round_strip_after_nudge =
                    tightened.tool_round_strip_after_nudge.min(1);
                tightened.observation_only_round_nudge_after = 1;
                tightened.observation_only_round_strip_after_nudge = 0;
                Some(tightened)
            }
            Self::DirectInspection(_) | Self::DirectUtility(_) => {
                Some(tightened_direct_profile_termination(base))
            }
        }
    }

    pub(super) fn allows_synthesis_fallback(&self) -> bool {
        matches!(self, Self::DirectInspection(_))
    }

    pub(super) fn direct_inspection_profile(&self) -> Option<DirectInspectionProfile> {
        match self {
            Self::DirectInspection(profile) => Some(*profile),
            Self::Standard | Self::BoundedLocal | Self::DirectUtility(_) => None,
        }
    }

    pub(super) fn owns_tool_surface(&self) -> bool {
        !matches!(self, Self::Standard)
    }
}

fn tighten_or_default_threshold(current: u16, ceiling: u16) -> u16 {
    if current == 0 {
        ceiling
    } else {
        current.min(ceiling)
    }
}

fn tightened_direct_profile_termination(base: &TerminationConfig) -> TerminationConfig {
    let mut tightened = base.clone();
    tightened.nudge_after_tool_turns =
        tighten_or_default_threshold(tightened.nudge_after_tool_turns, 1);
    tightened.strip_tools_after_nudge = 0;
    tightened.tool_round_nudge_after =
        tighten_or_default_threshold(tightened.tool_round_nudge_after, 1);
    tightened.tool_round_strip_after_nudge = 0;
    tightened.observation_only_round_nudge_after = 0;
    tightened.observation_only_round_strip_after_nudge = 0;
    tightened
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum BoundedLocalPhase {
    #[default]
    Discovery,
    Mutation,
    Recovery,
    Verification,
    Terminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BoundedLocalTerminalReason {
    NeedsGroundedEditAfterRecovery,
    RecoveryStepDidNotProduceTargetedContext,
}

impl LoopEngine {
    pub(super) fn turn_execution_profile_tool_names(&self) -> Option<Vec<String>> {
        match &self.turn_execution_profile {
            TurnExecutionProfile::DirectInspection(profile) => Some(
                direct_inspection_tool_names(profile)
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            ),
            TurnExecutionProfile::BoundedLocal => Some(match self.bounded_local_phase {
                BoundedLocalPhase::Discovery => ["search_text", "read_file", "list_directory"]
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
                BoundedLocalPhase::Mutation => ["write_file", "edit_file"]
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
                BoundedLocalPhase::Recovery => ["read_file", "search_text"]
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
                BoundedLocalPhase::Verification => ["run_command", "read_file"]
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
                BoundedLocalPhase::Terminal => Vec::new(),
            }),
            TurnExecutionProfile::DirectUtility(profile) => {
                Some(direct_utility_tool_names(profile))
            }
            TurnExecutionProfile::Standard => None,
        }
    }

    pub(super) fn turn_execution_profile_block_reason(&self) -> Option<&'static str> {
        match &self.turn_execution_profile {
            TurnExecutionProfile::DirectInspection(profile) => {
                Some(direct_inspection_block_reason(profile))
            }
            TurnExecutionProfile::BoundedLocal => Some(self.bounded_local_phase_block_reason()),
            TurnExecutionProfile::DirectUtility(profile) => {
                Some(direct_utility_block_reason(profile))
            }
            TurnExecutionProfile::Standard => None,
        }
    }

    pub(super) fn apply_turn_execution_profile_tool_surface(
        &self,
        tools: Vec<ToolDefinition>,
    ) -> Vec<ToolDefinition> {
        let Some(allowed) = self.turn_execution_profile_tool_names() else {
            return tools;
        };
        let allowed: HashSet<&str> = allowed.iter().map(String::as_str).collect();
        tools
            .into_iter()
            .filter(|tool| allowed.contains(tool.name.as_str()))
            .collect()
    }

    pub(super) fn effective_decompose_enabled(&self) -> bool {
        self.decompose_enabled
            && matches!(&self.turn_execution_profile, TurnExecutionProfile::Standard)
    }

    pub(super) fn turn_execution_profile_directive(&self) -> Option<String> {
        match &self.turn_execution_profile {
            TurnExecutionProfile::DirectInspection(profile) => {
                Some(direct_inspection_directive(profile))
            }
            TurnExecutionProfile::Standard => None,
            TurnExecutionProfile::BoundedLocal => {
                let phase_directive = match self.bounded_local_phase {
                    BoundedLocalPhase::Discovery => BOUNDED_LOCAL_DISCOVERY_PHASE_DIRECTIVE,
                    BoundedLocalPhase::Mutation => BOUNDED_LOCAL_MUTATION_PHASE_DIRECTIVE,
                    BoundedLocalPhase::Recovery => {
                        return Some(format!(
                            "{BOUNDED_LOCAL_TASK_DIRECTIVE}{}",
                            bounded_local_recovery_phase_directive(
                                &self.bounded_local_recovery_focus
                            )
                        ));
                    }
                    BoundedLocalPhase::Verification => BOUNDED_LOCAL_VERIFICATION_PHASE_DIRECTIVE,
                    BoundedLocalPhase::Terminal => BOUNDED_LOCAL_TERMINAL_PHASE_DIRECTIVE,
                };
                Some(format!("{BOUNDED_LOCAL_TASK_DIRECTIVE}{phase_directive}"))
            }
            TurnExecutionProfile::DirectUtility(profile) => Some(direct_utility_directive(profile)),
        }
    }

    pub(super) fn reasoning_decompose_enabled(&self) -> bool {
        self.effective_decompose_enabled() && self.pending_artifact_write_target.is_none()
    }

    pub(super) fn bounded_local_phase_block_reason(&self) -> &'static str {
        match self.bounded_local_phase {
            BoundedLocalPhase::Discovery => BOUNDED_LOCAL_DISCOVERY_BLOCK_REASON,
            BoundedLocalPhase::Mutation => BOUNDED_LOCAL_MUTATION_BLOCK_REASON,
            BoundedLocalPhase::Recovery => BOUNDED_LOCAL_RECOVERY_BLOCK_REASON,
            BoundedLocalPhase::Verification => BOUNDED_LOCAL_VERIFICATION_BLOCK_REASON,
            BoundedLocalPhase::Terminal => {
                "bounded local terminal phase does not allow further tools"
            }
        }
    }

    pub(super) fn advance_bounded_local_phase_after_tool_round(
        &mut self,
        calls: &[ToolCall],
        results: &[ToolResult],
    ) {
        if !matches!(
            &self.turn_execution_profile,
            TurnExecutionProfile::BoundedLocal
        ) {
            return;
        }

        let previous = self.bounded_local_phase;
        let mut terminal_reason = None;
        let artifact_target = self
            .pending_artifact_write_target
            .as_deref()
            .or(self.requested_artifact_target.as_deref());
        self.bounded_local_phase = match self.bounded_local_phase {
            BoundedLocalPhase::Discovery => {
                self.bounded_local_recovery_focus.clear();
                if bounded_local_discovery_round_completed(calls, results, artifact_target) {
                    BoundedLocalPhase::Mutation
                } else {
                    BoundedLocalPhase::Discovery
                }
            }
            BoundedLocalPhase::Mutation => {
                if bounded_local_mutation_round_completed(calls, results, artifact_target) {
                    self.bounded_local_recovery_focus.clear();
                    BoundedLocalPhase::Verification
                } else if bounded_local_mutation_round_needs_recovery(
                    calls,
                    results,
                    artifact_target,
                ) {
                    if self.bounded_local_recovery_used {
                        self.bounded_local_recovery_focus.clear();
                        terminal_reason =
                            Some(BoundedLocalTerminalReason::NeedsGroundedEditAfterRecovery);
                        BoundedLocalPhase::Terminal
                    } else {
                        self.bounded_local_recovery_used = true;
                        self.bounded_local_recovery_focus =
                            bounded_local_recovery_focus_from_calls(calls);
                        BoundedLocalPhase::Recovery
                    }
                } else {
                    BoundedLocalPhase::Mutation
                }
            }
            BoundedLocalPhase::Recovery => {
                if bounded_local_recovery_round_completed(calls, results) {
                    self.bounded_local_recovery_focus.clear();
                    BoundedLocalPhase::Mutation
                } else {
                    self.bounded_local_recovery_focus.clear();
                    terminal_reason =
                        Some(BoundedLocalTerminalReason::RecoveryStepDidNotProduceTargetedContext);
                    BoundedLocalPhase::Terminal
                }
            }
            BoundedLocalPhase::Verification => {
                self.bounded_local_recovery_focus.clear();
                if bounded_local_verification_round_completed(calls, results) {
                    BoundedLocalPhase::Terminal
                } else {
                    BoundedLocalPhase::Verification
                }
            }
            BoundedLocalPhase::Terminal => {
                self.bounded_local_recovery_focus.clear();
                BoundedLocalPhase::Terminal
            }
        };
        self.bounded_local_terminal_reason = terminal_reason;

        if self.bounded_local_phase != previous {
            self.pending_tool_scope = None;
            self.last_turn_state_progress = Some(self.current_turn_state_progress());
            self.emit_signal(
                LoopStep::Act,
                SignalKind::Trace,
                "advanced bounded local execution phase",
                serde_json::json!({
                    "from": bounded_local_phase_label(previous),
                    "to": bounded_local_phase_label(self.bounded_local_phase),
                }),
            );
        }
    }
}

pub(super) fn partition_by_bounded_local_phase_semantics(
    calls: &[ToolCall],
    phase: BoundedLocalPhase,
    requested_artifact_target: Option<&str>,
) -> (Vec<ToolCall>, Vec<BlockedToolCall>) {
    let mut allowed = Vec::new();
    let mut blocked = Vec::new();
    for call in calls {
        let block_reason = match phase {
            BoundedLocalPhase::Mutation => {
                if bounded_local_mutation_call_is_meaningful(call, requested_artifact_target) {
                    None
                } else {
                    Some(BOUNDED_LOCAL_MUTATION_NOOP_BLOCK_REASON)
                }
            }
            BoundedLocalPhase::Verification => {
                if bounded_local_verification_call_is_focused(call) {
                    None
                } else {
                    Some(BOUNDED_LOCAL_VERIFICATION_DISCOVERY_BLOCK_REASON)
                }
            }
            BoundedLocalPhase::Discovery
            | BoundedLocalPhase::Recovery
            | BoundedLocalPhase::Terminal => None,
        };

        if let Some(reason) = block_reason {
            blocked.push(BlockedToolCall {
                call: call.clone(),
                reason: reason.to_string(),
            });
        } else {
            allowed.push(call.clone());
        }
    }
    (allowed, blocked)
}

fn bounded_local_discovery_round_completed(
    calls: &[ToolCall],
    results: &[ToolResult],
    requested_artifact_target: Option<&str>,
) -> bool {
    if requested_artifact_target.is_some() {
        return calls
            .iter()
            .any(|call| successful_result_for_call(call, results).is_some());
    }

    calls.iter().any(|call| {
        successful_result_for_call(call, results)
            .is_some_and(|_| bounded_local_discovery_call_grounds_edit_target(call))
    })
}

fn bounded_local_discovery_call_grounds_edit_target(call: &ToolCall) -> bool {
    match call.name.as_str() {
        "read_file" => json_string_arg(&call.arguments, &["path"]).is_some(),
        _ => false,
    }
}

fn bounded_local_mutation_round_completed(
    calls: &[ToolCall],
    results: &[ToolResult],
    requested_artifact_target: Option<&str>,
) -> bool {
    calls.iter().any(|call| {
        successful_result_for_call(call, results).is_some_and(|result| {
            bounded_local_mutation_call_is_meaningful(call, requested_artifact_target)
                && bounded_local_mutation_result_confirms_real_change(call, result)
        })
    })
}

fn bounded_local_mutation_round_needs_recovery(
    calls: &[ToolCall],
    results: &[ToolResult],
    requested_artifact_target: Option<&str>,
) -> bool {
    calls.iter().any(|call| {
        result_for_call(call, results).is_some_and(|result| {
            if result
                .output
                .contains(BOUNDED_LOCAL_MUTATION_NOOP_BLOCK_REASON)
            {
                return true;
            }

            bounded_local_mutation_call_is_meaningful(call, requested_artifact_target) && {
                let output_lower = result.output.to_ascii_lowercase();
                !output_lower.contains("proposal created")
                    && !output_lower.contains("was not modified")
                    && !successful_result_for_call(call, results).is_some_and(|success| {
                        bounded_local_mutation_result_confirms_real_change(call, success)
                    })
            }
        })
    })
}

fn bounded_local_recovery_round_completed(calls: &[ToolCall], results: &[ToolResult]) -> bool {
    calls
        .iter()
        .any(|call| successful_result_for_call(call, results).is_some())
}

fn bounded_local_verification_round_completed(calls: &[ToolCall], results: &[ToolResult]) -> bool {
    calls
        .iter()
        .any(|call| successful_result_for_call(call, results).is_some())
}

fn result_for_call<'a>(call: &ToolCall, results: &'a [ToolResult]) -> Option<&'a ToolResult> {
    results.iter().find(|result| result.tool_call_id == call.id)
}

fn successful_result_for_call<'a>(
    call: &ToolCall,
    results: &'a [ToolResult],
) -> Option<&'a ToolResult> {
    result_for_call(call, results).filter(|result| result.success)
}

pub(super) fn bounded_local_mutation_call_is_meaningful(
    call: &ToolCall,
    requested_artifact_target: Option<&str>,
) -> bool {
    match call.name.as_str() {
        "edit_file" => {
            let Some(path) = json_string_arg(&call.arguments, &["path"]) else {
                return false;
            };
            let old_text = call
                .arguments
                .get("old_text")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .unwrap_or("");
            !path_looks_like_bounded_local_scratch(path) && !old_text.is_empty()
        }
        "write_file" => {
            let Some(path) = json_string_arg(&call.arguments, &["path"]) else {
                return false;
            };
            let content = call
                .arguments
                .get("content")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .unwrap_or("");
            if content.is_empty() {
                return false;
            }
            if let Some(target) = requested_artifact_target {
                return bounded_local_path_matches_requested_target(path, target);
            }
            !path_looks_like_bounded_local_scratch(path)
        }
        _ => false,
    }
}

fn bounded_local_mutation_result_confirms_real_change(
    call: &ToolCall,
    result: &ToolResult,
) -> bool {
    let output_lower = result.output.to_ascii_lowercase();
    if output_lower.contains("proposal created") || output_lower.contains("was not modified") {
        return false;
    }

    match call.name.as_str() {
        "edit_file" => result.output.contains("Successfully edited"),
        "write_file" => {
            let content = call
                .arguments
                .get("content")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .unwrap_or("");
            !content.is_empty()
                && result.output.contains("wrote ")
                && !result.output.contains("wrote 0 bytes")
        }
        _ => false,
    }
}

fn bounded_local_path_matches_requested_target(path: &str, target: &str) -> bool {
    let candidates = artifact_path_candidates(target);
    candidates.iter().any(|candidate| candidate == path)
}

fn path_looks_like_bounded_local_scratch(path: &str) -> bool {
    let normalized = path.trim().to_ascii_lowercase();
    let file_name = path
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .trim()
        .to_ascii_lowercase();
    normalized.starts_with("/tmp/")
        || normalized.starts_with("tmp/")
        || file_name.starts_with(".fawx_")
        || file_name.starts_with("tmp")
        || file_name.starts_with("temp")
        || file_name.contains("noop")
        || file_name == "tmp"
        || file_name == "scratch"
}

fn bounded_local_recovery_focus_from_calls(calls: &[ToolCall]) -> Vec<String> {
    let mut focus = Vec::new();
    let mut seen = HashSet::new();
    for call in calls {
        if !matches!(call.name.as_str(), "edit_file" | "write_file") {
            continue;
        }
        let Some(path) = json_string_arg(&call.arguments, &["path"]) else {
            continue;
        };
        if path.trim().is_empty() || !seen.insert(path.to_string()) {
            continue;
        }
        focus.push(path.to_string());
    }
    focus
}

pub(super) fn bounded_local_verification_call_is_focused(call: &ToolCall) -> bool {
    match call.name.as_str() {
        "read_file" => true,
        "run_command" => run_command_looks_like_focused_verification(call),
        _ => false,
    }
}

fn run_command_looks_like_focused_verification(call: &ToolCall) -> bool {
    let Some(command) = json_string_arg(&call.arguments, &["command"]) else {
        return false;
    };
    let words = shell_command_words(command);
    let Some(first) = first_effective_command_word(&words) else {
        return false;
    };

    const DISCOVERY_COMMANDS: &[&str] = &[
        "rg", "grep", "find", "fd", "ls", "tree", "pwd", "which", "whereis", "locate", "cat",
        "sed", "awk", "head", "tail",
    ];
    if DISCOVERY_COMMANDS.contains(&first) {
        return false;
    }

    const VERIFICATION_WORDS: &[&str] = &["test", "check", "build", "lint", "verify"];
    if VERIFICATION_WORDS
        .iter()
        .any(|word| words.iter().any(|token| token == word))
    {
        return true;
    }

    if first == "git" {
        return words
            .iter()
            .any(|token| token == "diff" || token == "status");
    }

    matches!(
        first,
        "pytest"
            | "ctest"
            | "cargo"
            | "swift"
            | "xcodebuild"
            | "just"
            | "make"
            | "cmake"
            | "npm"
            | "pnpm"
            | "yarn"
            | "bun"
            | "uv"
            | "go"
            | "gradle"
            | "./gradlew"
            | "mvn"
            | "ninja"
    ) && words
        .iter()
        .any(|token| token == "test" || token == "check" || token == "build" || token == "lint")
}

fn shell_command_words(command: &str) -> Vec<String> {
    command
        .split_whitespace()
        .map(|token| {
            token
                .trim_matches(|c: char| matches!(c, '"' | '\'' | '`' | ';' | '|' | '&' | '(' | ')'))
                .to_ascii_lowercase()
        })
        .filter(|token| !token.is_empty())
        .collect()
}

fn first_effective_command_word(words: &[String]) -> Option<&str> {
    words.iter().map(String::as_str).find(|word| {
        !matches!(
            *word,
            "sh" | "/bin/sh" | "bash" | "/bin/bash" | "zsh" | "/bin/zsh" | "-lc" | "-c"
        )
    })
}

#[cfg(test)]
pub(super) fn detect_turn_execution_profile(
    user_message: &str,
    available_tools: &[ToolDefinition],
) -> TurnExecutionProfile {
    detect_turn_execution_profile_for_ownership(
        user_message,
        available_tools,
        DirectInspectionOwnership::DetectFromTurn,
    )
}

pub(super) fn detect_turn_execution_profile_for_ownership(
    user_message: &str,
    available_tools: &[ToolDefinition],
    direct_inspection_ownership: DirectInspectionOwnership,
) -> TurnExecutionProfile {
    if let Some(profile) = detect_direct_utility_profile(user_message, available_tools) {
        return TurnExecutionProfile::DirectUtility(profile);
    }
    if let Some(profile) = direct_inspection_ownership.profile_for_turn(user_message) {
        return TurnExecutionProfile::DirectInspection(profile);
    }

    let lower = user_message.to_lowercase();
    let forbids_web_research = [
        "do not use web research",
        "don't use web research",
        "no web research",
        "without web research",
    ]
    .iter()
    .any(|needle| lower.contains(needle));
    if !forbids_web_research {
        return TurnExecutionProfile::Standard;
    }

    let local_scope = [
        "work only inside ",
        "work only in ",
        "use only local tools",
        "using only local tools",
        "local-only",
        "within the working directory",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
        || user_message
            .split_whitespace()
            .any(|token| token.starts_with('/') || token.starts_with("~/"));

    if !local_scope {
        return TurnExecutionProfile::Standard;
    }

    let direct_action_markers = [
        " read ",
        " inspect ",
        " find ",
        " identify ",
        " locate ",
        " make ",
        " change ",
        " edit ",
        " write ",
        " run ",
        " test ",
        " summarize ",
        " end with ",
    ];
    let padded = format!(" {} ", lower);
    let direct_action_count = direct_action_markers
        .iter()
        .filter(|needle| padded.contains(**needle))
        .count();

    if direct_action_count >= 2 {
        TurnExecutionProfile::BoundedLocal
    } else {
        TurnExecutionProfile::Standard
    }
}

fn bounded_local_recovery_phase_directive(focus: &[String]) -> String {
    if focus.is_empty() {
        return BOUNDED_LOCAL_RECOVERY_PHASE_DIRECTIVE.to_string();
    }

    format!(
        "{BOUNDED_LOCAL_RECOVERY_PHASE_DIRECTIVE}\nFocus this recovery step on these failed edit targets if relevant: {}.",
        focus.join(", ")
    )
}

pub(super) fn bounded_local_phase_label(phase: BoundedLocalPhase) -> &'static str {
    match phase {
        BoundedLocalPhase::Discovery => "discovery",
        BoundedLocalPhase::Mutation => "mutation",
        BoundedLocalPhase::Recovery => "recovery",
        BoundedLocalPhase::Verification => "verification",
        BoundedLocalPhase::Terminal => "terminal",
    }
}

pub(super) fn bounded_local_terminal_reason_label(
    reason: BoundedLocalTerminalReason,
) -> &'static str {
    match reason {
        BoundedLocalTerminalReason::NeedsGroundedEditAfterRecovery => {
            "needs_grounded_edit_after_recovery"
        }
        BoundedLocalTerminalReason::RecoveryStepDidNotProduceTargetedContext => {
            "recovery_step_did_not_produce_targeted_context"
        }
    }
}

pub(super) fn bounded_local_terminal_reason_text(
    reason: BoundedLocalTerminalReason,
) -> &'static str {
    match reason {
        BoundedLocalTerminalReason::NeedsGroundedEditAfterRecovery => {
            "bounded local run exhausted its one recovery pass before a grounded edit could be made"
        }
        BoundedLocalTerminalReason::RecoveryStepDidNotProduceTargetedContext => {
            "bounded local recovery did not produce the exact local context needed for a safe retry"
        }
    }
}

pub(super) fn bounded_local_terminal_partial_response(
    reason: BoundedLocalTerminalReason,
    tool_results: &[ToolResult],
) -> String {
    let headline = match reason {
        BoundedLocalTerminalReason::NeedsGroundedEditAfterRecovery => {
            "Blocked: this bounded local run completed discovery and one targeted recovery pass, but it still did not have a grounded enough edit to apply safely."
        }
        BoundedLocalTerminalReason::RecoveryStepDidNotProduceTargetedContext => {
            "Blocked: this bounded local run used its one targeted recovery pass, but that recovery step still did not produce the exact local context needed for a safe edit."
        }
    };
    let access_note =
        "File access was available during the run; it stopped because the bounded-local policy ends after one failed edit, one tiny recovery pass, and one retry.";
    let tool_summary = summarize_tool_progress(tool_results)
        .map(|summary| format!("Observed during the run: {summary}"))
        .unwrap_or_else(|| {
            "Observed during the run: no meaningful tool progress was recorded.".to_string()
        });
    let next_step =
        "Next best step: point me to the exact file/function to edit, or give a more specific target for the code change so I can retry with grounded context.";
    format!("{headline}\n\n{access_note}\n\n{tool_summary}\n\n{next_step}")
}
