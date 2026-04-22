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
use fx_core::command_text::normalize_http_url_token;
use fx_core::message::ProgressKind;
use fx_llm::{CompletionResponse, ToolCall, ToolDefinition};
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) enum TurnExecutionProfile {
    #[default]
    Standard,
    BoundedLocal,
    DirectInspection(DirectInspectionProfile),
    DirectUtility(DirectUtilityProfile),
    DeterministicLocal(DeterministicLocalPlan),
}

impl TurnExecutionProfile {
    pub(super) fn uses_standard_observation_controls(&self) -> bool {
        matches!(self, Self::Standard)
    }

    pub(super) fn completes_terminally(&self) -> bool {
        matches!(
            self,
            Self::DirectInspection(_) | Self::DirectUtility(_) | Self::DeterministicLocal(_)
        )
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
            Self::DirectInspection(_) | Self::DirectUtility(_) | Self::DeterministicLocal(_) => {
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
            Self::Standard
            | Self::BoundedLocal
            | Self::DirectUtility(_)
            | Self::DeterministicLocal(_) => None,
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

const DETERMINISTIC_LOCAL_BLOCK_REASON: &str =
    "deterministic local intent turns only allow their planned local action";
const DETERMINISTIC_LOCAL_URL_ALLOWED_TOKENS: &[&str] = &[
    "a", "an", "browser", "can", "could", "default", "in", "launch", "link", "me", "open", "page",
    "please", "start", "tab", "the", "this", "url", "webpage", "website", "would", "you",
];
const DETERMINISTIC_LOCAL_BROWSER_ALLOWED_TOKENS: &[&str] = &[
    "a",
    "an",
    "app",
    "application",
    "browser",
    "can",
    "could",
    "for",
    "launch",
    "me",
    "open",
    "please",
    "start",
    "tab",
    "the",
    "would",
    "you",
];
static DETERMINISTIC_LOCAL_TOOL_CALL_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum DeterministicLocalPlan {
    OpenBrowserApplication { browser: BrowserApplication },
    OpenBrowserUrl { url: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
// Keep this variant list aligned with the parallel tool-local enum in
// fx-tools/src/tools/local_actions.rs. The duplication is intentional so the
// kernel can classify bounded intents without depending on the tool crate, but
// the supported browser set is one cross-crate contract.
pub(super) enum BrowserApplication {
    Chrome,
    Safari,
    Firefox,
    Brave,
    Edge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BrowserAliasMatch {
    browser: BrowserApplication,
    start: usize,
    len: usize,
}

impl BrowserApplication {
    const ALL: [Self; 5] = [
        Self::Chrome,
        Self::Safari,
        Self::Firefox,
        Self::Brave,
        Self::Edge,
    ];

    pub(super) fn argument_value(self) -> &'static str {
        match self {
            Self::Chrome => "chrome",
            Self::Safari => "safari",
            Self::Firefox => "firefox",
            Self::Brave => "brave",
            Self::Edge => "edge",
        }
    }

    fn display_name(self) -> &'static str {
        match self {
            Self::Chrome => "Google Chrome",
            Self::Safari => "Safari",
            Self::Firefox => "Firefox",
            Self::Brave => "Brave Browser",
            Self::Edge => "Microsoft Edge",
        }
    }

    fn aliases(self) -> &'static [&'static [&'static str]] {
        match self {
            Self::Chrome => &[&["google", "chrome"], &["chrome"]],
            Self::Safari => &[&["safari"]],
            Self::Firefox => &[&["mozilla", "firefox"], &["firefox"]],
            Self::Brave => &[&["brave", "browser"], &["brave"]],
            Self::Edge => &[&["microsoft", "edge"], &["edge"]],
        }
    }

    fn find_match(self, tokens: &[String]) -> Option<BrowserAliasMatch> {
        self.aliases()
            .iter()
            .filter_map(|alias| {
                find_token_sequence(tokens, alias).map(|start| BrowserAliasMatch {
                    browser: self,
                    start,
                    len: alias.len(),
                })
            })
            .max_by_key(|matched| matched.len)
    }

    pub(super) fn from_candidate(candidate: &str) -> Option<Self> {
        let tokens = tokenize_local_intent(candidate);
        if tokens.is_empty() {
            return None;
        }
        Self::ALL
            .into_iter()
            .filter_map(|browser| browser.find_match(&tokens))
            .max_by_key(|matched| matched.len)
            .map(|matched| matched.browser)
    }
}

impl DeterministicLocalPlan {
    pub(super) fn tool_name(&self) -> &'static str {
        match self {
            Self::OpenBrowserApplication { .. } => "open_browser_application",
            Self::OpenBrowserUrl { .. } => "open_browser_url",
        }
    }

    pub(super) fn completion_response(&self) -> CompletionResponse {
        CompletionResponse {
            content: Vec::new(),
            tool_calls: vec![self.tool_call()],
            usage: None,
            stop_reason: Some("tool_use".to_string()),
        }
    }

    pub(super) fn terminal_response(&self, tool_results: &[ToolResult]) -> String {
        if let Some(result) = latest_successful_result(tool_results) {
            if !result.output.trim().is_empty() {
                return result.output.trim().to_string();
            }
            return self.success_message();
        }

        let prefix = match self {
            Self::OpenBrowserApplication { browser } => {
                format!("I couldn't open {}.", browser.display_name())
            }
            Self::OpenBrowserUrl { url } => {
                format!("I couldn't open {url} in the default browser.")
            }
        };

        if let Some(result) = latest_non_empty_result(tool_results) {
            format!("{prefix} {}", result.output.trim())
        } else {
            prefix
        }
    }

    pub(super) fn progress(&self) -> (ProgressKind, String) {
        (
            ProgressKind::Implementing,
            match self {
                Self::OpenBrowserApplication { browser } => {
                    format!("Opening {}...", browser.display_name())
                }
                Self::OpenBrowserUrl { url } => {
                    format!("Opening {url} in the default browser...")
                }
            },
        )
    }

    pub(super) fn signal_kind(&self) -> &'static str {
        match self {
            Self::OpenBrowserApplication { .. } => "open_browser_application",
            Self::OpenBrowserUrl { .. } => "open_browser_url",
        }
    }

    pub(super) fn signal_metadata(&self) -> serde_json::Value {
        match self {
            Self::OpenBrowserApplication { browser } => serde_json::json!({
                "kind": self.signal_kind(),
                "tool_name": self.tool_name(),
                "browser": browser.argument_value(),
            }),
            Self::OpenBrowserUrl { url } => serde_json::json!({
                "kind": self.signal_kind(),
                "tool_name": self.tool_name(),
                "url": url,
            }),
        }
    }

    fn tool_call(&self) -> ToolCall {
        let sequence = DETERMINISTIC_LOCAL_TOOL_CALL_COUNTER.fetch_add(1, Ordering::Relaxed);
        ToolCall {
            id: format!(
                "deterministic-local-{}-{sequence}",
                self.signal_kind().replace('_', "-")
            ),
            name: self.tool_name().to_string(),
            arguments: match self {
                Self::OpenBrowserApplication { browser } => serde_json::json!({
                    "browser": browser.argument_value(),
                }),
                Self::OpenBrowserUrl { url } => serde_json::json!({
                    "url": url,
                }),
            },
        }
    }

    fn success_message(&self) -> String {
        match self {
            Self::OpenBrowserApplication { browser } => {
                format!("Opened {}.", browser.display_name())
            }
            Self::OpenBrowserUrl { url } => {
                format!("Opened {url} in the default browser.")
            }
        }
    }
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
            TurnExecutionProfile::DeterministicLocal(plan) => {
                Some(vec![plan.tool_name().to_string()])
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
            TurnExecutionProfile::DeterministicLocal(_) => Some(DETERMINISTIC_LOCAL_BLOCK_REASON),
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
            && self.preflight_route_plan.is_none()
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
            TurnExecutionProfile::DeterministicLocal(_) => None,
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
            blocked.push(BlockedToolCall::policy(call.clone(), reason));
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

fn latest_successful_result(tool_results: &[ToolResult]) -> Option<&ToolResult> {
    tool_results
        .iter()
        .rev()
        .find(|result| result.success && !result.output.trim().is_empty())
}

fn latest_non_empty_result(tool_results: &[ToolResult]) -> Option<&ToolResult> {
    tool_results
        .iter()
        .rev()
        .find(|result| !result.output.trim().is_empty())
}

fn detect_deterministic_local_plan(
    user_message: &str,
    available_tools: &[ToolDefinition],
) -> Option<DeterministicLocalPlan> {
    let url = extract_single_http_url(user_message);
    if let Some(url) = url.filter(|_| tool_available("open_browser_url", available_tools)) {
        let tokens = tokenize_without_http_urls(user_message);
        if is_single_open_intent_request(&tokens, DETERMINISTIC_LOCAL_URL_ALLOWED_TOKENS) {
            return Some(DeterministicLocalPlan::OpenBrowserUrl { url });
        }
    }

    if !tool_available("open_browser_application", available_tools) {
        return None;
    }
    let tokens = tokenize_local_intent(user_message);
    let browser = detect_browser_application(&tokens)?;
    let tokens_without_browser = strip_browser_alias_tokens(&tokens, browser)?;
    is_single_open_intent_request(
        &tokens_without_browser,
        DETERMINISTIC_LOCAL_BROWSER_ALLOWED_TOKENS,
    )
    .then_some(DeterministicLocalPlan::OpenBrowserApplication { browser })
}

fn tool_available(tool_name: &str, available_tools: &[ToolDefinition]) -> bool {
    available_tools.iter().any(|tool| tool.name == tool_name)
}

fn is_single_open_intent_request(tokens: &[String], allowed_tokens: &[&str]) -> bool {
    !tokens.is_empty()
        && tokens
            .iter()
            .any(|token| matches!(token.as_str(), "open" | "launch" | "start"))
        && tokens
            .iter()
            .all(|token| allowed_tokens.contains(&token.as_str()))
}

fn extract_single_http_url(user_message: &str) -> Option<String> {
    let mut urls = user_message
        .split_whitespace()
        .filter_map(normalize_http_url_token);
    let first = urls.next()?;
    urls.next().is_none().then_some(first)
}

fn tokenize_without_http_urls(user_message: &str) -> Vec<String> {
    let filtered = user_message
        .split_whitespace()
        .filter(|token| normalize_http_url_token(token).is_none())
        .collect::<Vec<_>>()
        .join(" ");
    tokenize_local_intent(&filtered)
}

fn tokenize_local_intent(user_message: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for ch in user_message.chars() {
        if ch.is_ascii_alphanumeric() {
            current.push(ch.to_ascii_lowercase());
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn detect_browser_application(tokens: &[String]) -> Option<BrowserApplication> {
    let matches = BrowserApplication::ALL
        .into_iter()
        .filter_map(|browser| browser.find_match(tokens))
        .collect::<Vec<_>>();

    let first = *matches.first()?;
    matches
        .iter()
        .all(|matched| matched.browser == first.browser)
        .then_some(first.browser)
}

fn strip_browser_alias_tokens(
    tokens: &[String],
    browser: BrowserApplication,
) -> Option<Vec<String>> {
    let matched = browser.find_match(tokens)?;
    let mut stripped = Vec::with_capacity(tokens.len().saturating_sub(matched.len));
    stripped.extend(tokens[..matched.start].iter().cloned());
    stripped.extend(tokens[matched.start + matched.len..].iter().cloned());
    Some(stripped)
}

fn find_token_sequence(tokens: &[String], needle: &[&str]) -> Option<usize> {
    if needle.is_empty() || needle.len() > tokens.len() {
        return None;
    }

    tokens
        .windows(needle.len())
        .position(|window| window.iter().map(String::as_str).eq(needle.iter().copied()))
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
    // Deterministic local lanes intentionally win over broader direct-utility
    // detection so an obvious one-shot local action stays on the narrowest
    // possible control-plane path.
    if let Some(plan) = detect_deterministic_local_plan(user_message, available_tools) {
        return TurnExecutionProfile::DeterministicLocal(plan);
    }
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
            "Observed during the run: no blocking tool error details were recorded.".to_string()
        });
    let next_step =
        "Next best step: point me to the exact file/function to edit, or give a more specific target for the code change so I can retry with grounded context.";
    format!("{headline}\n\n{access_note}\n\n{tool_summary}\n\n{next_step}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::act::FailureClass;
    use fx_llm::ToolDefinition;

    fn deterministic_local_tools() -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "open_browser_url".to_string(),
                description: String::new(),
                parameters: serde_json::json!({}),
            },
            ToolDefinition {
                name: "open_browser_application".to_string(),
                description: String::new(),
                parameters: serde_json::json!({}),
            },
        ]
    }

    #[test]
    fn bounded_local_policy_blocks_are_classified_permanent() {
        let calls = vec![ToolCall {
            id: "call-1".to_string(),
            name: "list_directory".to_string(),
            arguments: serde_json::json!({"path":"."}),
        }];

        let (_allowed, blocked) = partition_by_bounded_local_phase_semantics(
            &calls,
            BoundedLocalPhase::Verification,
            None,
        );

        assert_eq!(blocked.len(), 1);
        assert_eq!(blocked[0].failure_class, Some(FailureClass::Permanent));
    }

    #[test]
    fn deterministic_local_detector_rejects_multiple_browsers() {
        let tools = deterministic_local_tools();
        assert_eq!(
            detect_deterministic_local_plan("open google chrome and safari", &tools),
            None
        );
    }

    #[test]
    fn deterministic_local_detector_matches_multi_token_browser_alias() {
        let tools = deterministic_local_tools();
        assert_eq!(
            detect_deterministic_local_plan("open brave browser", &tools),
            Some(DeterministicLocalPlan::OpenBrowserApplication {
                browser: BrowserApplication::Brave,
            })
        );
    }

    #[test]
    fn deterministic_local_detector_accepts_url_with_filler_tokens() {
        let tools = deterministic_local_tools();
        assert_eq!(
            detect_deterministic_local_plan("Open https://example.com in a browser", &tools),
            Some(DeterministicLocalPlan::OpenBrowserUrl {
                url: "https://example.com".to_string(),
            })
        );
    }

    #[test]
    fn deterministic_local_detector_rejects_open_without_target() {
        let tools = deterministic_local_tools();
        assert_eq!(detect_deterministic_local_plan("open", &tools), None);
    }

    #[test]
    fn deterministic_local_detector_rejects_multiple_urls() {
        let tools = deterministic_local_tools();
        assert_eq!(
            detect_deterministic_local_plan("open https://foo.com https://bar.com", &tools),
            None
        );
    }

    #[test]
    fn deterministic_local_detector_is_case_insensitive() {
        let tools = deterministic_local_tools();
        assert_eq!(
            detect_deterministic_local_plan("OPEN CHROME", &tools),
            Some(DeterministicLocalPlan::OpenBrowserApplication {
                browser: BrowserApplication::Chrome,
            })
        );
        assert_eq!(
            detect_deterministic_local_plan("Open HTTPS://Example.Com", &tools),
            Some(DeterministicLocalPlan::OpenBrowserUrl {
                url: "HTTPS://Example.Com".to_string(),
            })
        );
    }

    #[test]
    fn browser_application_from_candidate_uses_alias_sequences_not_substrings() {
        assert_eq!(
            BrowserApplication::from_candidate("Google Chrome"),
            Some(BrowserApplication::Chrome)
        );
        assert_eq!(
            BrowserApplication::from_candidate("/Applications/Google Chrome.app"),
            Some(BrowserApplication::Chrome)
        );
        assert_eq!(BrowserApplication::from_candidate("knowledge"), None);
        assert_eq!(BrowserApplication::from_candidate("bravery"), None);
        assert_eq!(BrowserApplication::from_candidate("chromeedge"), None);
    }

    #[test]
    fn normalize_http_url_token_only_trims_one_trailing_sentence_punctuation_mark() {
        assert_eq!(
            normalize_http_url_token("https://example.com."),
            Some("https://example.com".to_string())
        );
        assert_eq!(
            normalize_http_url_token("https://example.com..."),
            Some("https://example.com..".to_string())
        );
    }

    #[test]
    fn deterministic_local_tool_call_ids_are_unique() {
        let plan = DeterministicLocalPlan::OpenBrowserUrl {
            url: "https://example.com".to_string(),
        };
        let first = plan.completion_response().tool_calls[0].id.clone();
        let second = plan.completion_response().tool_calls[0].id.clone();
        assert_ne!(first, second);
    }
}
