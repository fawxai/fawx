//! Shared signal types used across the engine.
use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

/// Which loop step produced a signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoopStep {
    #[serde(alias = "Perceive")]
    Perceive,
    #[serde(alias = "Reason")]
    Reason,
    #[serde(alias = "Decide")]
    Decide,
    #[serde(alias = "Act")]
    Act,
    #[serde(alias = "Synthesize")]
    Synthesize,
}

impl LoopStep {
    pub fn to_label(self) -> &'static str {
        match self {
            Self::Perceive => "perceive",
            Self::Reason => "reason",
            Self::Decide => "decide",
            Self::Act => "act",
            Self::Synthesize => "synthesize",
        }
    }
}

impl fmt::Display for LoopStep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.to_label())
    }
}

/// Severity classification for signals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalSeverity {
    /// Trace-level, informational (memory hit, observation)
    // Accept PascalCase spellings from pre-enrichment signal files.
    #[serde(alias = "Low")]
    Low,
    /// Normal friction (file not found, parse error)
    #[serde(alias = "Medium")]
    Medium,
    /// Repeated failures, budget pressure, retry exhaustion
    #[serde(alias = "High")]
    High,
    /// Provider down, budget exhausted, unrecoverable error.
    ///
    /// `Critical` is always an explicit escalation; no `SignalKind` defaults to it.
    #[serde(alias = "Critical")]
    Critical,
}

impl fmt::Display for SignalSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        })
    }
}

impl SignalSeverity {
    /// Numeric weight used by consumers that aggregate severity.
    ///
    /// This is an ordinal typed scale from 1-4, which intentionally differs
    /// from the arbitrary legacy `metadata["severity"]` float values.
    pub const fn weight(self) -> u8 {
        match self {
            Self::Low => 1,
            Self::Medium => 2,
            Self::High => 3,
            Self::Critical => 4,
        }
    }
}

/// Eviction priority used by in-memory signal collectors when at capacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SignalEvictionPriority {
    /// Evict this signal kind before higher-priority entries.
    DropFirst,
    /// Keep if possible, but not at the expense of higher-priority signals.
    Normal,
    /// Prefer keeping this signal kind over normal traffic.
    Keep,
    /// Strongest keep tier for scarce, high-diagnostic-value signals.
    KeepStrong,
}

/// Signal category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalKind {
    #[serde(alias = "Trace")]
    Trace,
    #[serde(alias = "Thinking")]
    Thinking,
    #[serde(alias = "Friction")]
    Friction,
    #[serde(alias = "Success")]
    Success,
    #[serde(alias = "Blocked")]
    Blocked,
    #[serde(alias = "Performance")]
    Performance,
    #[serde(alias = "UserIntervention")]
    UserIntervention,
    #[serde(alias = "UserInput")]
    UserInput,
    #[serde(alias = "UserFeedback")]
    UserFeedback,
    #[serde(alias = "Decision")]
    Decision,
    #[serde(alias = "Observation")]
    Observation,
    #[serde(alias = "Retry")]
    Retry,
    #[serde(alias = "Timeout")]
    Timeout,
    #[serde(alias = "Cost")]
    Cost,
    #[serde(alias = "ContextOverflow")]
    ContextOverflow,
    #[serde(alias = "MemoryHit")]
    MemoryHit,
    #[serde(alias = "MemoryMiss")]
    MemoryMiss,
    #[serde(alias = "ProviderFallback")]
    ProviderFallback,
}

impl SignalKind {
    pub fn to_label(self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Thinking => "thinking",
            Self::Friction => "friction",
            Self::Success => "success",
            Self::Blocked => "blocked",
            Self::Performance => "performance",
            Self::UserIntervention => "user_intervention",
            Self::UserInput => "user_input",
            Self::UserFeedback => "user_feedback",
            Self::Decision => "decision",
            Self::Observation => "observation",
            Self::Retry => "retry",
            Self::Timeout => "timeout",
            Self::Cost => "cost",
            Self::ContextOverflow => "context_overflow",
            Self::MemoryHit => "memory_hit",
            Self::MemoryMiss => "memory_miss",
            Self::ProviderFallback => "provider_fallback",
        }
    }

    /// Returns the default severity for this signal kind.
    /// Callers can override when context warrants escalation.
    pub const fn default_severity(self) -> SignalSeverity {
        match self {
            // Low severity: informational, trace-level
            Self::Trace
            | Self::Observation
            | Self::Thinking
            | Self::Success
            | Self::Decision
            | Self::UserInput
            | Self::Cost
            | Self::MemoryHit
            | Self::MemoryMiss => SignalSeverity::Low,

            // Medium severity: normal friction, performance notes
            Self::Friction
            | Self::Performance
            | Self::UserFeedback
            | Self::Retry
            | Self::ContextOverflow
            | Self::ProviderFallback => SignalSeverity::Medium,

            // High severity: blocked operations, user intervention required
            Self::Blocked | Self::UserIntervention | Self::Timeout => SignalSeverity::High,
        }
    }

    /// Returns the collector eviction priority for this signal kind.
    pub const fn eviction_priority(self) -> SignalEvictionPriority {
        match self {
            Self::Trace | Self::Performance | Self::Cost | Self::MemoryHit | Self::MemoryMiss => {
                SignalEvictionPriority::DropFirst
            }
            Self::Retry | Self::ProviderFallback => SignalEvictionPriority::Keep,
            Self::Timeout => SignalEvictionPriority::KeepStrong,
            Self::Thinking
            | Self::Friction
            | Self::Success
            | Self::Blocked
            | Self::UserIntervention
            | Self::UserInput
            | Self::UserFeedback
            | Self::Decision
            | Self::Observation => SignalEvictionPriority::Normal,
            Self::ContextOverflow => SignalEvictionPriority::Keep,
        }
    }
}

impl fmt::Display for SignalKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.to_label())
    }
}

/// Typed classification for tool-result signals emitted during `LoopStep::Act`.
///
/// The engine still persists this value inside structured signal metadata for
/// wire compatibility, but downstream consumers should use this enum-backed
/// accessor surface instead of reading raw JSON strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalToolClassification {
    #[serde(alias = "Observation")]
    Observation,
    #[serde(alias = "Mutation")]
    Mutation,
}

impl SignalToolClassification {
    pub const fn to_label(self) -> &'static str {
        match self {
            Self::Observation => "observation",
            Self::Mutation => "mutation",
        }
    }
}

impl fmt::Display for SignalToolClassification {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.to_label())
    }
}

/// Typed control-plane decision families persisted in signal metadata.
///
/// Signals still write the wire field as `metadata["decision_kind"]` for
/// compatibility with session exports, but emitters and readers should share
/// this enum instead of open-coded string labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlPlaneDecisionKind {
    TurnStop,
    ToolFailure,
    RetryPolicy,
    ToolCallGuardrail,
    MutationGuardrail,
    ToolBatchGuardrail,
    ToolRoundGuardrail,
    BudgetGuardrail,
    ToolCallNormalization,
    PreflightRoute,
}

impl ControlPlaneDecisionKind {
    pub const fn to_label(self) -> &'static str {
        match self {
            Self::TurnStop => "turn_stop",
            Self::ToolFailure => "tool_failure",
            Self::RetryPolicy => "retry_policy",
            Self::ToolCallGuardrail => "tool_call_guardrail",
            Self::MutationGuardrail => "mutation_guardrail",
            Self::ToolBatchGuardrail => "tool_batch_guardrail",
            Self::ToolRoundGuardrail => "tool_round_guardrail",
            Self::BudgetGuardrail => "budget_guardrail",
            Self::ToolCallNormalization => "tool_call_normalization",
            Self::PreflightRoute => "preflight_route",
        }
    }
}

impl fmt::Display for ControlPlaneDecisionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.to_label())
    }
}

/// A structured observation emitted by a loop step.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Signal {
    /// Monotonic signal ID (unique within a collector instance)
    pub id: u64,
    /// Request/subagent correlation ID for cross-boundary tracing
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span_id: Option<String>,
    /// Which loop step produced this signal
    pub step: LoopStep,
    /// Signal category
    pub kind: SignalKind,
    /// Severity classification
    pub severity: SignalSeverity,
    /// Human-readable message
    pub message: String,
    /// Structured metadata (tool results, error details, etc.)
    pub metadata: serde_json::Value,
    /// Unix timestamp in milliseconds
    pub timestamp_ms: u64,
    /// ID of the signal that caused this one (causal chain)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause_id: Option<u64>,
    /// Wall time for the step that emitted this signal (milliseconds)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct SignalWire {
    #[serde(default = "default_signal_id")]
    id: u64,
    #[serde(default)]
    span_id: Option<String>,
    step: LoopStep,
    kind: SignalKind,
    #[serde(default)]
    severity: Option<SignalSeverity>,
    message: String,
    #[serde(default = "default_signal_metadata")]
    metadata: serde_json::Value,
    timestamp_ms: u64,
    #[serde(default)]
    cause_id: Option<u64>,
    #[serde(default)]
    duration_ms: Option<u64>,
}

fn default_signal_id() -> u64 {
    Signal::UNASSIGNED_ID
}

fn default_signal_metadata() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

impl Signal {
    /// Sentinel ID for signals that have not yet been collected.
    pub const UNASSIGNED_ID: u64 = 0;

    /// Returns the current Unix timestamp in milliseconds.
    pub fn now_ms() -> u64 {
        let elapsed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
    }

    /// Create a new unregistered signal with the given parameters.
    ///
    /// The signal starts with `id = 0` and receives a real ID when a collector emits it.
    pub fn new(
        step: LoopStep,
        kind: SignalKind,
        message: impl Into<String>,
        metadata: serde_json::Value,
        timestamp_ms: u64,
    ) -> Self {
        Self {
            id: Self::UNASSIGNED_ID,
            span_id: None,
            step,
            kind,
            severity: kind.default_severity(),
            message: message.into(),
            metadata,
            timestamp_ms,
            cause_id: None,
            duration_ms: None,
        }
    }

    /// Override the collector-assigned ID when reconstructing an existing signal.
    pub fn with_id(mut self, id: u64) -> Self {
        self.id = id;
        self
    }

    /// Override the default severity for this signal.
    pub fn with_severity(mut self, severity: SignalSeverity) -> Self {
        self.severity = severity;
        self
    }

    /// Set the span ID for cross-boundary correlation.
    pub fn with_span_id(mut self, span_id: impl Into<String>) -> Self {
        self.span_id = Some(span_id.into());
        self
    }

    /// Set the cause ID for causal chain linking.
    pub fn with_cause_id(mut self, cause_id: u64) -> Self {
        self.cause_id = Some(cause_id);
        self
    }

    /// Set the duration for the step that produced this signal.
    pub fn with_duration_ms(mut self, duration_ms: u64) -> Self {
        self.duration_ms = Some(duration_ms);
        self
    }

    /// Returns the typed tool classification encoded in signal metadata.
    pub fn tool_classification(&self) -> Option<SignalToolClassification> {
        serde_json::from_value(self.metadata.get("classification")?.clone()).ok()
    }

    /// Returns the typed control-plane decision family encoded in signal metadata.
    pub fn control_plane_decision_kind(&self) -> Option<ControlPlaneDecisionKind> {
        serde_json::from_value(self.metadata.get("decision_kind")?.clone()).ok()
    }

    /// Returns whether this signal carries any control-plane decision marker.
    pub fn has_control_plane_decision_kind(&self) -> bool {
        self.metadata.get("decision_kind").is_some()
    }

    /// Returns whether this signal is for the requested control-plane decision family.
    pub fn is_control_plane_decision_kind(&self, kind: ControlPlaneDecisionKind) -> bool {
        self.control_plane_decision_kind() == Some(kind)
    }

    /// Returns whether this signal metadata names a concrete tool.
    pub fn has_tool_name(&self) -> bool {
        self.metadata
            .get("tool")
            .and_then(serde_json::Value::as_str)
            .is_some()
    }

    /// Returns the cost sample recorded in metadata, if present.
    pub fn cost_cents(&self) -> Option<f64> {
        self.metadata
            .get("cost_cents")
            .and_then(serde_json::Value::as_f64)
    }
}

// Manual `Deserialize` keeps migration defaults centralized in `SignalWire`
// without weakening the typed `Signal` contract with serde-only fields.
impl<'de> Deserialize<'de> for Signal {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = SignalWire::deserialize(deserializer)?;
        Ok(Self {
            id: wire.id,
            span_id: wire.span_id,
            step: wire.step,
            kind: wire.kind,
            severity: wire
                .severity
                .unwrap_or_else(|| wire.kind.default_severity()),
            message: wire.message,
            metadata: wire.metadata,
            timestamp_ms: wire.timestamp_ms,
            cause_id: wire.cause_id,
            duration_ms: wire.duration_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loop_step_display_matches_label() {
        assert_eq!(LoopStep::Perceive.to_string(), "perceive");
        assert_eq!(LoopStep::Reason.to_string(), "reason");
        assert_eq!(LoopStep::Act.to_string(), "act");
        assert_eq!(LoopStep::Synthesize.to_string(), "synthesize");
        assert_eq!(LoopStep::Decide.to_string(), "decide");
    }

    #[test]
    fn loop_step_serializes_to_snake_case_label() {
        let encoded = serde_json::to_string(&LoopStep::Synthesize).expect("serialize");
        assert_eq!(encoded, "\"synthesize\"");
    }

    #[test]
    fn loop_step_deserializes_legacy_and_snake_case_labels() {
        let legacy: LoopStep = serde_json::from_str("\"Synthesize\"").expect("legacy");
        let snake_case: LoopStep = serde_json::from_str("\"synthesize\"").expect("snake_case");
        assert_eq!(legacy, LoopStep::Synthesize);
        assert_eq!(snake_case, LoopStep::Synthesize);
    }

    #[test]
    fn signal_severity_display_matches_label() {
        assert_eq!(SignalSeverity::Low.to_string(), "low");
        assert_eq!(SignalSeverity::Medium.to_string(), "medium");
        assert_eq!(SignalSeverity::High.to_string(), "high");
        assert_eq!(SignalSeverity::Critical.to_string(), "critical");
    }

    #[test]
    fn signal_severity_serializes_to_snake_case() {
        let encoded = serde_json::to_string(&SignalSeverity::High).expect("serialize");
        assert_eq!(encoded, "\"high\"");
    }

    #[test]
    fn signal_severity_deserializes_legacy_and_snake_case_labels() {
        let legacy: SignalSeverity = serde_json::from_str("\"Critical\"").expect("legacy");
        let snake_case: SignalSeverity = serde_json::from_str("\"critical\"").expect("snake_case");
        assert_eq!(legacy, SignalSeverity::Critical);
        assert_eq!(snake_case, SignalSeverity::Critical);
    }

    #[test]
    fn signal_severity_ordering() {
        assert!(SignalSeverity::Low < SignalSeverity::Medium);
        assert!(SignalSeverity::Medium < SignalSeverity::High);
        assert!(SignalSeverity::High < SignalSeverity::Critical);
    }

    #[test]
    fn signal_severity_weight_increases_with_severity() {
        assert_eq!(SignalSeverity::Low.weight(), 1);
        assert_eq!(SignalSeverity::Medium.weight(), 2);
        assert_eq!(SignalSeverity::High.weight(), 3);
        assert_eq!(SignalSeverity::Critical.weight(), 4);
    }

    #[test]
    fn signal_kind_display_matches_label() {
        assert_eq!(SignalKind::Trace.to_string(), "trace");
        assert_eq!(SignalKind::Friction.to_string(), "friction");
        assert_eq!(
            SignalKind::UserIntervention.to_string(),
            "user_intervention"
        );
        assert_eq!(SignalKind::Decision.to_string(), "decision");
        assert_eq!(SignalKind::Performance.to_string(), "performance");
        assert_eq!(SignalKind::Thinking.to_string(), "thinking");
        assert_eq!(SignalKind::Success.to_string(), "success");
        assert_eq!(SignalKind::Blocked.to_string(), "blocked");
        assert_eq!(SignalKind::UserInput.to_string(), "user_input");
        assert_eq!(SignalKind::UserFeedback.to_string(), "user_feedback");
        assert_eq!(SignalKind::Observation.to_string(), "observation");
        assert_eq!(SignalKind::Retry.to_string(), "retry");
        assert_eq!(SignalKind::Timeout.to_string(), "timeout");
        assert_eq!(SignalKind::Cost.to_string(), "cost");
        assert_eq!(SignalKind::ContextOverflow.to_string(), "context_overflow");
        assert_eq!(SignalKind::MemoryHit.to_string(), "memory_hit");
        assert_eq!(SignalKind::MemoryMiss.to_string(), "memory_miss");
        assert_eq!(
            SignalKind::ProviderFallback.to_string(),
            "provider_fallback"
        );
    }

    #[test]
    fn signal_kind_serializes_to_snake_case_label() {
        let encoded = serde_json::to_string(&SignalKind::UserIntervention).expect("serialize");
        assert_eq!(encoded, "\"user_intervention\"");
    }

    #[test]
    fn signal_kind_deserializes_legacy_and_snake_case_labels() {
        let legacy: SignalKind = serde_json::from_str("\"UserFeedback\"").expect("legacy");
        let snake_case: SignalKind = serde_json::from_str("\"user_feedback\"").expect("snake_case");
        assert_eq!(legacy, SignalKind::UserFeedback);
        assert_eq!(snake_case, SignalKind::UserFeedback);
    }

    #[test]
    fn new_signal_kinds_roundtrip_legacy_and_snake_case_labels() {
        let cases = [
            (SignalKind::Retry, "\"Retry\"", "\"retry\""),
            (SignalKind::Timeout, "\"Timeout\"", "\"timeout\""),
            (SignalKind::Cost, "\"Cost\"", "\"cost\""),
            (
                SignalKind::ContextOverflow,
                "\"ContextOverflow\"",
                "\"context_overflow\"",
            ),
            (SignalKind::MemoryHit, "\"MemoryHit\"", "\"memory_hit\""),
            (SignalKind::MemoryMiss, "\"MemoryMiss\"", "\"memory_miss\""),
            (
                SignalKind::ProviderFallback,
                "\"ProviderFallback\"",
                "\"provider_fallback\"",
            ),
        ];

        for (kind, legacy, snake_case) in cases {
            let encoded = serde_json::to_string(&kind).expect("serialize");
            assert_eq!(encoded, snake_case);

            let decoded_legacy: SignalKind = serde_json::from_str(legacy).expect("legacy");
            let decoded_snake_case: SignalKind =
                serde_json::from_str(snake_case).expect("snake_case");
            assert_eq!(decoded_legacy, kind);
            assert_eq!(decoded_snake_case, kind);
        }
    }

    #[test]
    fn signal_kind_default_severity() {
        // Low severity
        assert_eq!(SignalKind::Trace.default_severity(), SignalSeverity::Low);
        assert_eq!(
            SignalKind::Observation.default_severity(),
            SignalSeverity::Low
        );
        assert_eq!(SignalKind::Thinking.default_severity(), SignalSeverity::Low);
        assert_eq!(SignalKind::Success.default_severity(), SignalSeverity::Low);
        assert_eq!(SignalKind::Decision.default_severity(), SignalSeverity::Low);
        assert_eq!(
            SignalKind::UserInput.default_severity(),
            SignalSeverity::Low
        );
        assert_eq!(SignalKind::Cost.default_severity(), SignalSeverity::Low);
        assert_eq!(
            SignalKind::MemoryHit.default_severity(),
            SignalSeverity::Low
        );
        assert_eq!(
            SignalKind::MemoryMiss.default_severity(),
            SignalSeverity::Low
        );

        // Medium severity
        assert_eq!(
            SignalKind::Friction.default_severity(),
            SignalSeverity::Medium
        );
        assert_eq!(
            SignalKind::Performance.default_severity(),
            SignalSeverity::Medium
        );
        assert_eq!(
            SignalKind::UserFeedback.default_severity(),
            SignalSeverity::Medium
        );
        assert_eq!(SignalKind::Retry.default_severity(), SignalSeverity::Medium);
        assert_eq!(
            SignalKind::ContextOverflow.default_severity(),
            SignalSeverity::Medium
        );

        // High severity
        assert_eq!(SignalKind::Blocked.default_severity(), SignalSeverity::High);
        assert_eq!(
            SignalKind::UserIntervention.default_severity(),
            SignalSeverity::High
        );
        assert_eq!(SignalKind::Timeout.default_severity(), SignalSeverity::High);
        assert_eq!(
            SignalKind::ProviderFallback.default_severity(),
            SignalSeverity::Medium
        );
    }

    #[test]
    fn signal_tool_classification_roundtrips_labels() {
        let encoded =
            serde_json::to_string(&SignalToolClassification::Observation).expect("serialize");
        assert_eq!(encoded, "\"observation\"");

        let legacy: SignalToolClassification =
            serde_json::from_str("\"Mutation\"").expect("legacy");
        let snake_case: SignalToolClassification =
            serde_json::from_str("\"mutation\"").expect("snake_case");
        assert_eq!(legacy, SignalToolClassification::Mutation);
        assert_eq!(snake_case, SignalToolClassification::Mutation);
    }

    #[test]
    fn control_plane_decision_kind_roundtrips_labels() {
        let encoded =
            serde_json::to_string(&ControlPlaneDecisionKind::PreflightRoute).expect("serialize");
        assert_eq!(encoded, "\"preflight_route\"");

        let decoded: ControlPlaneDecisionKind =
            serde_json::from_str("\"tool_call_guardrail\"").expect("snake_case");
        assert_eq!(decoded, ControlPlaneDecisionKind::ToolCallGuardrail);
        assert_eq!(
            ControlPlaneDecisionKind::ToolCallNormalization.to_string(),
            "tool_call_normalization"
        );
    }

    #[test]
    fn signal_kind_eviction_priority_covers_operational_kinds() {
        assert_eq!(
            SignalKind::Retry.eviction_priority(),
            SignalEvictionPriority::Keep
        );
        assert_eq!(
            SignalKind::Timeout.eviction_priority(),
            SignalEvictionPriority::KeepStrong
        );
        assert_eq!(
            SignalKind::Cost.eviction_priority(),
            SignalEvictionPriority::DropFirst
        );
        assert_eq!(
            SignalKind::ContextOverflow.eviction_priority(),
            SignalEvictionPriority::Keep
        );
        assert_eq!(
            SignalKind::MemoryHit.eviction_priority(),
            SignalEvictionPriority::DropFirst
        );
        assert_eq!(
            SignalKind::MemoryMiss.eviction_priority(),
            SignalEvictionPriority::DropFirst
        );
        assert_eq!(
            SignalKind::ProviderFallback.eviction_priority(),
            SignalEvictionPriority::Keep
        );
    }

    #[test]
    fn signal_kind_eviction_priority_matches_all_retention_tiers() {
        let cases = [
            (SignalKind::Trace, SignalEvictionPriority::DropFirst),
            (SignalKind::Performance, SignalEvictionPriority::DropFirst),
            (SignalKind::Cost, SignalEvictionPriority::DropFirst),
            (SignalKind::MemoryHit, SignalEvictionPriority::DropFirst),
            (SignalKind::MemoryMiss, SignalEvictionPriority::DropFirst),
            (SignalKind::Thinking, SignalEvictionPriority::Normal),
            (SignalKind::Friction, SignalEvictionPriority::Normal),
            (SignalKind::Success, SignalEvictionPriority::Normal),
            (SignalKind::Blocked, SignalEvictionPriority::Normal),
            (SignalKind::UserIntervention, SignalEvictionPriority::Normal),
            (SignalKind::UserInput, SignalEvictionPriority::Normal),
            (SignalKind::UserFeedback, SignalEvictionPriority::Normal),
            (SignalKind::Decision, SignalEvictionPriority::Normal),
            (SignalKind::Observation, SignalEvictionPriority::Normal),
            (SignalKind::Retry, SignalEvictionPriority::Keep),
            (SignalKind::ContextOverflow, SignalEvictionPriority::Keep),
            (SignalKind::ProviderFallback, SignalEvictionPriority::Keep),
            (SignalKind::Timeout, SignalEvictionPriority::KeepStrong),
        ];

        for (kind, expected) in cases {
            assert_eq!(kind.eviction_priority(), expected, "{kind:?}");
        }
    }

    #[test]
    fn signal_new_uses_default_severity() {
        let signal = Signal::new(
            LoopStep::Act,
            SignalKind::Friction,
            "test message",
            serde_json::json!({}),
            1000,
        );
        assert_eq!(signal.id, Signal::UNASSIGNED_ID);
        assert_eq!(signal.severity, SignalSeverity::Medium); // Friction default
        assert_eq!(signal.span_id, None);
        assert_eq!(signal.cause_id, None);
        assert_eq!(signal.duration_ms, None);
    }

    #[test]
    fn signal_with_severity_override() {
        let signal = Signal::new(
            LoopStep::Act,
            SignalKind::Friction,
            "critical failure",
            serde_json::json!({"error": "oom"}),
            2000,
        )
        .with_severity(SignalSeverity::Critical);
        assert_eq!(signal.id, Signal::UNASSIGNED_ID);
        assert_eq!(signal.severity, SignalSeverity::Critical);
    }

    #[test]
    fn signal_builder_methods() {
        let signal = Signal::new(
            LoopStep::Reason,
            SignalKind::Trace,
            "trace",
            serde_json::json!({}),
            3000,
        )
        .with_id(3)
        .with_span_id("span-123")
        .with_cause_id(2)
        .with_duration_ms(150);

        assert_eq!(signal.id, 3);
        assert_eq!(signal.span_id, Some("span-123".to_string()));
        assert_eq!(signal.cause_id, Some(2));
        assert_eq!(signal.duration_ms, Some(150));
    }

    #[test]
    fn signal_exposes_typed_operational_metadata_accessors() {
        let signal = Signal::new(
            LoopStep::Act,
            SignalKind::Success,
            "tool read_file",
            serde_json::json!({
                "classification": "observation",
                "decision_kind": "tool_failure",
                "tool": "read_file",
                "cost_cents": 1.5
            }),
            4000,
        );

        assert_eq!(
            signal.tool_classification(),
            Some(SignalToolClassification::Observation)
        );
        assert_eq!(
            signal.control_plane_decision_kind(),
            Some(ControlPlaneDecisionKind::ToolFailure)
        );
        assert!(signal.has_control_plane_decision_kind());
        assert!(signal.is_control_plane_decision_kind(ControlPlaneDecisionKind::ToolFailure));
        assert!(signal.has_tool_name());
        assert_eq!(signal.cost_cents(), Some(1.5));
    }

    #[test]
    fn signal_serialization_roundtrip() {
        let original = Signal::new(
            LoopStep::Act,
            SignalKind::Success,
            "success",
            serde_json::json!({"tool": "read_file"}),
            1000,
        )
        .with_id(42)
        .with_span_id("test-span")
        .with_cause_id(41)
        .with_duration_ms(250);

        let json = serde_json::to_string(&original).expect("serialize");
        let deserialized: Signal = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(original, deserialized);
    }

    #[test]
    fn signal_deserializes_legacy_shape_with_defaults() {
        let legacy = serde_json::json!({
            "step": "act",
            "kind": "friction",
            "message": "legacy failure",
            "metadata": {"output": "bad"},
            "timestamp_ms": 1234
        });

        let signal: Signal = serde_json::from_value(legacy).expect("deserialize legacy signal");
        assert_eq!(signal.id, Signal::UNASSIGNED_ID);
        assert_eq!(signal.step, LoopStep::Act);
        assert_eq!(signal.kind, SignalKind::Friction);
        assert_eq!(signal.severity, SignalKind::Friction.default_severity());
        assert_eq!(signal.span_id, None);
        assert_eq!(signal.cause_id, None);
        assert_eq!(signal.duration_ms, None);
    }

    #[test]
    fn signal_deserializes_missing_metadata_to_empty_object() {
        let legacy = serde_json::json!({
            "step": "reason",
            "kind": "decision",
            "message": "picked a plan",
            "timestamp_ms": 55
        });

        let signal: Signal = serde_json::from_value(legacy).expect("deserialize legacy signal");
        assert_eq!(signal.metadata, serde_json::json!({}));
        assert_eq!(signal.severity, SignalSeverity::Low);
    }

    #[test]
    fn signal_now_ms_returns_unix_millis() {
        assert!(Signal::now_ms() > 0);
    }
}
