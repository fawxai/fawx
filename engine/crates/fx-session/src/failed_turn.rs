use crate::session::{render_content_blocks_with_options, ContentRenderOptions};
use crate::{MessageRole, SessionContentBlock, SessionMessage};
use fx_core::signals::{ControlPlaneDecisionKind, LoopStep, Signal, SignalKind, SignalSeverity};
use serde::Serialize;
use serde_json::Value;

/// Focused diagnostic payload for the latest failed turn in a session.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FailedTurnDiagnostic {
    pub session_id: String,
    pub failed_turn_timestamp: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_message: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostic_hints: Vec<String>,
    pub tool_chain: Vec<FailedTurnToolChainItem>,
    pub decision_traces: Vec<FailedTurnSignal>,
    pub final_stop: FailedTurnStop,
}

/// Tool-use/result item, preserving the order stored in session history.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FailedTurnToolChainItem {
    ToolUse {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_id: Option<String>,
        name: String,
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        content: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
}

/// Persisted control-plane signal selected for failed-turn diagnosis.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FailedTurnSignal {
    pub id: u64,
    pub step: LoopStep,
    pub kind: SignalKind,
    pub severity: SignalSeverity,
    pub message: String,
    pub metadata: Value,
    pub timestamp_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

/// Terminal loop status for the failed turn.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FailedTurnStop {
    pub result_kind: String,
    pub stop_reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iterations: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recoverable: Option<bool>,
}

#[derive(Debug, Clone, Copy)]
struct TurnBounds {
    start: usize,
    end: usize,
}

/// Build a focused diagnostic for the latest failed turn, if the persisted
/// signal stream contains a failed terminal status for this session.
pub fn latest_failed_turn_diagnostic(
    session_id: &str,
    messages: &[SessionMessage],
    signals: &[Signal],
) -> Option<FailedTurnDiagnostic> {
    let failed_stop_index = signals.iter().rposition(is_failed_turn_stop_signal)?;
    let stop_signal = &signals[failed_stop_index];
    let turn_bounds = turn_bounds_for_stop(messages, signals, failed_stop_index)?;
    let signal_window = signal_window_for_stop(signals, failed_stop_index);
    let turn_messages = &messages[turn_bounds.start..turn_bounds.end];

    Some(FailedTurnDiagnostic {
        session_id: session_id.to_string(),
        failed_turn_timestamp: turn_messages
            .first()
            .map(|message| message.timestamp)
            .unwrap_or_else(|| stop_signal.timestamp_ms / 1000),
        user_message: turn_messages.first().and_then(user_message_text),
        diagnostic_hints: diagnostic_hints_for_turn(&turn_bounds, &signal_window),
        tool_chain: tool_chain_for_turn(turn_messages),
        decision_traces: signal_window
            .signals
            .iter()
            .filter(is_diagnostic_signal)
            .map(FailedTurnSignal::from)
            .collect(),
        final_stop: failed_stop_from_signal(stop_signal),
    })
}

/// Render a compact human-readable diagnostic. JSON remains the canonical
/// typed form; this is for terminal inspection.
pub fn render_failed_turn_diagnostic_text(diagnostic: &FailedTurnDiagnostic) -> String {
    let mut output = format!(
        "Session: {}\nFailed turn: {}\nStop: {} ({})\n",
        diagnostic.session_id,
        diagnostic.failed_turn_timestamp,
        diagnostic.final_stop.stop_reason,
        diagnostic.final_stop.result_kind
    );
    if let Some(user_message) = &diagnostic.user_message {
        output.push_str("User:\n");
        output.push_str(user_message);
        output.push('\n');
    }
    if !diagnostic.diagnostic_hints.is_empty() {
        output.push_str("\nDiagnostic hints:\n");
        for hint in &diagnostic.diagnostic_hints {
            output.push_str("- ");
            output.push_str(hint);
            output.push('\n');
        }
    }

    output.push_str("\nTool chain:\n");
    if diagnostic.tool_chain.is_empty() {
        output.push_str("- none\n");
    } else {
        for item in &diagnostic.tool_chain {
            output.push_str("- ");
            output.push_str(&format_tool_chain_item(item));
            output.push('\n');
        }
    }

    output.push_str("\nControl-plane decisions:\n");
    if diagnostic.decision_traces.is_empty() {
        output.push_str("- none\n");
    } else {
        for trace in &diagnostic.decision_traces {
            output.push_str("- ");
            output.push_str(&format_signal_trace(trace));
            output.push('\n');
        }
    }

    output
}

#[derive(Debug, Clone, Copy)]
struct SignalWindow<'a> {
    signals: &'a [Signal],
    missing_prior_turn_stop: bool,
}

fn signal_window_for_stop(signals: &[Signal], failed_stop_index: usize) -> SignalWindow<'_> {
    let start = signals[..failed_stop_index]
        .iter()
        .rposition(is_turn_stop_signal)
        .map(|index| index + 1)
        .unwrap_or(0);
    SignalWindow {
        signals: &signals[start..=failed_stop_index],
        missing_prior_turn_stop: start == 0,
    }
}

fn diagnostic_hints_for_turn(bounds: &TurnBounds, signal_window: &SignalWindow<'_>) -> Vec<String> {
    let missing_prior_turn_stop = signal_window.missing_prior_turn_stop && bounds.start > 0;
    if missing_prior_turn_stop {
        vec![
            "No earlier turn_stop signal was persisted before this failed turn; decision traces may be incomplete."
                .to_string(),
        ]
    } else {
        Vec::new()
    }
}

fn turn_bounds_for_stop(
    messages: &[SessionMessage],
    signals: &[Signal],
    failed_stop_index: usize,
) -> Option<TurnBounds> {
    let user_turns = user_turn_bounds(messages);
    if user_turns.is_empty() {
        return None;
    }

    let stop_second = signals[failed_stop_index].timestamp_ms / 1000;
    if let Some(bounds) = user_turns
        .iter()
        .copied()
        .rev()
        .find(|bounds| messages[bounds.start].timestamp <= stop_second)
    {
        return Some(bounds);
    }

    // Fallback for legacy or synthetic persisted data whose message timestamps
    // cannot be compared to signal timestamps. The timestamp path above is the
    // source of truth because a turn can emit more than one terminal trace.
    let terminal_ordinal = signals[..=failed_stop_index]
        .iter()
        .filter(|signal| is_turn_stop_signal(signal))
        .count();
    if let Some(bounds) = terminal_ordinal
        .checked_sub(1)
        .and_then(|index| user_turns.get(index))
        .copied()
    {
        return Some(bounds);
    }

    user_turns.last().copied()
}

fn user_turn_bounds(messages: &[SessionMessage]) -> Vec<TurnBounds> {
    let mut starts = messages
        .iter()
        .enumerate()
        .filter_map(|(index, message)| (message.role == MessageRole::User).then_some(index))
        .collect::<Vec<_>>();
    if starts.is_empty() {
        return Vec::new();
    }

    starts.push(messages.len());
    starts
        .windows(2)
        .map(|window| TurnBounds {
            start: window[0],
            end: window[1],
        })
        .collect()
}

fn user_message_text(message: &SessionMessage) -> Option<String> {
    if message.role != MessageRole::User {
        return None;
    }
    let text = render_content_blocks_with_options(
        &message.content,
        ContentRenderOptions {
            include_tool_use_id: true,
        },
    );
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn tool_chain_for_turn(messages: &[SessionMessage]) -> Vec<FailedTurnToolChainItem> {
    messages
        .iter()
        .flat_map(|message| message.content.iter())
        .filter_map(|block| match block {
            SessionContentBlock::ToolUse {
                id,
                provider_id,
                name,
                input,
            } => Some(FailedTurnToolChainItem::ToolUse {
                id: id.clone(),
                provider_id: provider_id.clone(),
                name: name.clone(),
                input: input.clone(),
            }),
            SessionContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => Some(FailedTurnToolChainItem::ToolResult {
                tool_use_id: tool_use_id.clone(),
                content: content.clone(),
                is_error: *is_error,
            }),
            SessionContentBlock::Text { .. }
            | SessionContentBlock::Image { .. }
            | SessionContentBlock::Document { .. } => None,
        })
        .collect()
}

fn is_diagnostic_signal(signal: &&Signal) -> bool {
    is_turn_stop_signal(signal)
        || signal.has_control_plane_decision_kind()
        || signal.metadata.get("failure_class").is_some()
        || matches!(
            signal.kind,
            SignalKind::Retry | SignalKind::Blocked | SignalKind::Timeout
        )
}

fn is_turn_stop_signal(signal: &Signal) -> bool {
    signal.is_control_plane_decision_kind(ControlPlaneDecisionKind::TurnStop)
}

fn is_failed_turn_stop_signal(signal: &Signal) -> bool {
    is_turn_stop_signal(signal)
        && signal
            .metadata
            .get("failed")
            .and_then(Value::as_bool)
            .unwrap_or(false)
}

fn failed_stop_from_signal(signal: &Signal) -> FailedTurnStop {
    FailedTurnStop {
        result_kind: metadata_string(signal, "result_kind", "unknown"),
        stop_reason: metadata_string(signal, "stop_reason", "unknown"),
        iterations: signal
            .metadata
            .get("iterations")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
        recoverable: signal.metadata.get("recoverable").and_then(Value::as_bool),
    }
}

fn metadata_string(signal: &Signal, key: &str, fallback: &str) -> String {
    signal
        .metadata
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or(fallback)
        .to_string()
}

fn format_tool_chain_item(item: &FailedTurnToolChainItem) -> String {
    match item {
        FailedTurnToolChainItem::ToolUse {
            id, name, input, ..
        } => {
            format!("tool_use {name}#{id} input={input}")
        }
        FailedTurnToolChainItem::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => {
            let suffix = match is_error {
                Some(true) => " error",
                Some(false) => " ok",
                None => "",
            };
            format!("tool_result {tool_use_id}{suffix} content={content}")
        }
    }
}

fn format_signal_trace(trace: &FailedTurnSignal) -> String {
    let decision = trace
        .metadata
        .get("decision_kind")
        .and_then(Value::as_str)
        .map(|kind| format!(" [{kind}]"))
        .unwrap_or_default();
    format!(
        "{} {}{}: {}",
        trace.step, trace.kind, decision, trace.message
    )
}

impl From<&Signal> for FailedTurnSignal {
    fn from(signal: &Signal) -> Self {
        Self {
            id: signal.id,
            step: signal.step,
            kind: signal.kind,
            severity: signal.severity,
            message: signal.message.clone(),
            metadata: signal.metadata.clone(),
            timestamp_ms: signal.timestamp_ms,
            cause_id: signal.cause_id,
            duration_ms: signal.duration_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_core::signals::Signal;

    fn user(text: &str, timestamp: u64) -> SessionMessage {
        SessionMessage::text(MessageRole::User, text, timestamp)
    }

    fn assistant_tool_use(id: &str, name: &str, timestamp: u64) -> SessionMessage {
        SessionMessage::structured(
            MessageRole::Assistant,
            vec![SessionContentBlock::ToolUse {
                id: id.to_string(),
                provider_id: Some(format!("provider-{id}")),
                name: name.to_string(),
                input: serde_json::json!({"command": "git status"}),
            }],
            timestamp,
            None,
        )
    }

    fn tool_result(id: &str, content: Value, timestamp: u64) -> SessionMessage {
        SessionMessage::structured(
            MessageRole::Tool,
            vec![SessionContentBlock::ToolResult {
                tool_use_id: id.to_string(),
                content,
                is_error: Some(true),
            }],
            timestamp,
            None,
        )
    }

    fn signal(kind: SignalKind, message: &str, metadata: Value, id: u64) -> Signal {
        Signal::new(LoopStep::Act, kind, message, metadata, 10_000 + id).with_id(id)
    }

    fn signal_at(
        kind: SignalKind,
        message: &str,
        metadata: Value,
        id: u64,
        timestamp_ms: u64,
    ) -> Signal {
        Signal::new(LoopStep::Act, kind, message, metadata, timestamp_ms).with_id(id)
    }

    fn terminal(failed: bool, id: u64) -> Signal {
        terminal_at(failed, id, 10_000 + id)
    }

    fn terminal_at(failed: bool, id: u64, timestamp_ms: u64) -> Signal {
        signal_at(
            SignalKind::Trace,
            "loop turn terminal status",
            serde_json::json!({
                "decision_kind": "turn_stop",
                "decision": if failed { "failed" } else { "completed" },
                "failed": failed,
                "result_kind": if failed { "incomplete" } else { "complete" },
                "stop_reason": if failed { "tool synthesis did not produce a usable final response" } else { "complete" },
                "iterations": 2,
            }),
            id,
            timestamp_ms,
        )
    }

    #[test]
    fn diagnostic_collects_latest_failed_turn_chain_and_decisions() {
        let messages = vec![
            user("successful turn", 1),
            SessionMessage::text(MessageRole::Assistant, "done", 2),
            user("fix the failing turn", 3),
            assistant_tool_use("call-1", "run_command", 4),
            tool_result(
                "call-1",
                serde_json::json!("Tool call failed: [redacted]"),
                4,
            ),
        ];
        let signals = vec![
            terminal(false, 1),
            signal(
                SignalKind::Trace,
                "planned preflight external resource route",
                serde_json::json!({
                    "decision_kind": "preflight_route",
                    "decision": "planned",
                }),
                2,
            ),
            signal(
                SignalKind::Retry,
                "retrying tool 'run_command'",
                serde_json::json!({
                    "decision_kind": "retry_policy",
                    "decision": "retry_allowed",
                    "retry_cause": "prior_failure",
                }),
                3,
            ),
            signal(
                SignalKind::Blocked,
                "tool 'run_command' blocked: previous identical call failed permanently",
                serde_json::json!({
                    "decision_kind": "tool_call_guardrail",
                    "source": "retry_policy",
                    "failure_class": "permanent",
                }),
                4,
            ),
            terminal(true, 5),
        ];

        let diagnostic =
            latest_failed_turn_diagnostic("sess-1", &messages, &signals).expect("diagnostic");

        assert_eq!(diagnostic.session_id, "sess-1");
        assert_eq!(diagnostic.failed_turn_timestamp, 3);
        assert_eq!(
            diagnostic.user_message.as_deref(),
            Some("fix the failing turn")
        );
        assert_eq!(diagnostic.tool_chain.len(), 2);
        assert!(matches!(
            diagnostic.tool_chain[0],
            FailedTurnToolChainItem::ToolUse { ref name, .. } if name == "run_command"
        ));
        assert!(diagnostic
            .decision_traces
            .iter()
            .any(|trace| { trace.metadata["decision_kind"] == "preflight_route" }));
        assert!(diagnostic
            .decision_traces
            .iter()
            .any(|trace| { trace.metadata["decision_kind"] == "retry_policy" }));
        assert!(diagnostic.decision_traces.iter().any(|trace| {
            trace.metadata["source"] == "retry_policy"
                && trace.metadata["failure_class"] == "permanent"
        }));
        assert_eq!(diagnostic.final_stop.result_kind, "incomplete");
    }

    #[test]
    fn diagnostic_includes_normalization_and_mutation_guardrail_decisions() {
        let messages = vec![
            user("open the browser", 1),
            assistant_tool_use("call-1", "run_command", 2),
            tool_result("call-1", serde_json::json!("blocked"), 2),
        ];
        let signals = vec![
            signal(
                SignalKind::Trace,
                "normalized malformed tool-call markup",
                serde_json::json!({
                    "decision_kind": "tool_call_normalization",
                    "decision": "normalized",
                    "outcome": "normalized",
                    "tool_names": ["run_command"],
                }),
                1,
            ),
            signal(
                SignalKind::Friction,
                "mutation serialization: executing first mutation, deferring following call(s): run_command",
                serde_json::json!({
                    "decision_kind": "mutation_guardrail",
                    "decision": "deferred",
                    "guardrail": "mutation_serialization",
                }),
                2,
            ),
            terminal(true, 3),
        ];

        let diagnostic =
            latest_failed_turn_diagnostic("sess-guard", &messages, &signals).expect("diagnostic");

        assert!(diagnostic.decision_traces.iter().any(|trace| {
            trace.metadata["decision_kind"] == "tool_call_normalization"
                && trace.metadata["decision"] == "normalized"
                && trace.metadata["outcome"] == "normalized"
        }));
        assert!(diagnostic.decision_traces.iter().any(|trace| {
            trace.metadata["decision_kind"] == "mutation_guardrail"
                && trace.metadata["guardrail"] == "mutation_serialization"
        }));
    }

    #[test]
    fn successful_turns_do_not_create_failed_diagnostics() {
        let messages = vec![
            user("hello", 1),
            SessionMessage::text(MessageRole::Assistant, "hi", 2),
        ];
        let signals = vec![terminal(false, 1)];

        assert!(latest_failed_turn_diagnostic("sess-ok", &messages, &signals).is_none());
    }

    #[test]
    fn duplicate_stop_signals_use_timestamp_to_select_failed_turn() {
        let messages = vec![
            user("first turn", 1),
            SessionMessage::text(MessageRole::Assistant, "done", 2),
            user("failed second turn", 20),
            assistant_tool_use("call-2", "run_command", 21),
            tool_result("call-2", serde_json::json!("failed"), 22),
            user("later successful turn", 40),
            SessionMessage::text(MessageRole::Assistant, "later done", 41),
        ];
        let signals = vec![
            terminal_at(false, 1, 2_000),
            signal_at(
                SignalKind::Blocked,
                "tool 'run_command' blocked",
                serde_json::json!({
                    "decision_kind": "tool_call_guardrail",
                    "decision": "blocked",
                }),
                2,
                22_000,
            ),
            terminal_at(true, 3, 25_000),
            terminal_at(true, 4, 26_000),
            terminal_at(false, 5, 42_000),
        ];

        let diagnostic = latest_failed_turn_diagnostic("sess-dup-stop", &messages, &signals)
            .expect("diagnostic");

        assert_eq!(
            diagnostic.user_message.as_deref(),
            Some("failed second turn")
        );
        assert_eq!(diagnostic.failed_turn_timestamp, 20);
        assert_eq!(diagnostic.tool_chain.len(), 2);
    }

    #[test]
    fn latest_failed_turn_uses_window_after_previous_failed_stop() {
        let messages = vec![
            user("first failed turn", 1),
            assistant_tool_use("call-1", "read_file", 2),
            tool_result("call-1", serde_json::json!("first failure"), 3),
            user("second failed turn", 20),
            assistant_tool_use("call-2", "run_command", 21),
            tool_result("call-2", serde_json::json!("second failure"), 22),
        ];
        let signals = vec![
            signal_at(
                SignalKind::Blocked,
                "first block",
                serde_json::json!({
                    "decision_kind": "tool_call_guardrail",
                    "decision": "blocked",
                    "tool_call_id": "call-1",
                }),
                1,
                2_000,
            ),
            terminal_at(true, 2, 4_000),
            signal_at(
                SignalKind::Retry,
                "second retry",
                serde_json::json!({
                    "decision_kind": "retry_policy",
                    "decision": "retry_allowed",
                    "tool_call_id": "call-2",
                }),
                3,
                21_000,
            ),
            terminal_at(true, 4, 23_000),
        ];

        let diagnostic = latest_failed_turn_diagnostic("sess-two-failures", &messages, &signals)
            .expect("diagnostic");

        assert_eq!(
            diagnostic.user_message.as_deref(),
            Some("second failed turn")
        );
        assert!(diagnostic
            .decision_traces
            .iter()
            .any(|trace| trace.metadata["tool_call_id"] == "call-2"));
        assert!(!diagnostic
            .decision_traces
            .iter()
            .any(|trace| trace.metadata["tool_call_id"] == "call-1"));
    }
}
