//! Shared signal types used across the engine.
use serde::{Deserialize, Serialize};
use std::fmt;

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
    Low,
    /// Normal friction (file not found, parse error)
    Medium,
    /// Repeated failures, budget pressure, retry exhaustion
    High,
    /// Provider down, budget exhausted, unrecoverable error
    Critical,
}

impl SignalSeverity {
    pub fn to_label(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }
}

impl fmt::Display for SignalSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.to_label())
    }
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
        }
    }

    /// Returns the default severity for this signal kind.
    /// Callers can override when context warrants escalation.
    pub fn default_severity(self) -> SignalSeverity {
        match self {
            // Low severity: informational, trace-level
            Self::Trace
            | Self::Observation
            | Self::Thinking
            | Self::Success
            | Self::Decision
            | Self::UserInput => SignalSeverity::Low,

            // Medium severity: normal friction, performance notes
            Self::Friction | Self::Performance | Self::UserFeedback => SignalSeverity::Medium,

            // High severity: blocked operations, user intervention required
            Self::Blocked | Self::UserIntervention => SignalSeverity::High,
        }
    }
}

impl fmt::Display for SignalKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.to_label())
    }
}

/// A structured observation emitted by a loop step.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Signal {
    /// Monotonic signal ID (unique within a collector instance)
    pub id: u64,
    /// Request/subagent correlation ID for cross-boundary tracing
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
    pub cause_id: Option<u64>,
    /// Wall time for the step that emitted this signal (milliseconds)
    pub duration_ms: Option<u64>,
}

impl Signal {
    /// Create a new signal with the given parameters.
    /// Uses the default severity for the signal kind.
    pub fn new(
        id: u64,
        step: LoopStep,
        kind: SignalKind,
        message: impl Into<String>,
        metadata: serde_json::Value,
        timestamp_ms: u64,
    ) -> Self {
        Self {
            id,
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

    /// Create a signal with explicit severity override.
    pub fn with_severity(
        id: u64,
        step: LoopStep,
        kind: SignalKind,
        severity: SignalSeverity,
        message: impl Into<String>,
        metadata: serde_json::Value,
        timestamp_ms: u64,
    ) -> Self {
        Self {
            id,
            span_id: None,
            step,
            kind,
            severity,
            message: message.into(),
            metadata,
            timestamp_ms,
            cause_id: None,
            duration_ms: None,
        }
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
    fn signal_severity_ordering() {
        assert!(SignalSeverity::Low < SignalSeverity::Medium);
        assert!(SignalSeverity::Medium < SignalSeverity::High);
        assert!(SignalSeverity::High < SignalSeverity::Critical);
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
    fn signal_kind_default_severity() {
        // Low severity
        assert_eq!(SignalKind::Trace.default_severity(), SignalSeverity::Low);
        assert_eq!(SignalKind::Observation.default_severity(), SignalSeverity::Low);
        assert_eq!(SignalKind::Thinking.default_severity(), SignalSeverity::Low);
        assert_eq!(SignalKind::Success.default_severity(), SignalSeverity::Low);
        assert_eq!(SignalKind::Decision.default_severity(), SignalSeverity::Low);
        assert_eq!(SignalKind::UserInput.default_severity(), SignalSeverity::Low);

        // Medium severity
        assert_eq!(SignalKind::Friction.default_severity(), SignalSeverity::Medium);
        assert_eq!(SignalKind::Performance.default_severity(), SignalSeverity::Medium);
        assert_eq!(SignalKind::UserFeedback.default_severity(), SignalSeverity::Medium);

        // High severity
        assert_eq!(SignalKind::Blocked.default_severity(), SignalSeverity::High);
        assert_eq!(SignalKind::UserIntervention.default_severity(), SignalSeverity::High);
    }

    #[test]
    fn signal_new_uses_default_severity() {
        let signal = Signal::new(
            1,
            LoopStep::Act,
            SignalKind::Friction,
            "test message",
            serde_json::json!({}),
            1000,
        );
        assert_eq!(signal.id, 1);
        assert_eq!(signal.severity, SignalSeverity::Medium); // Friction default
        assert_eq!(signal.span_id, None);
        assert_eq!(signal.cause_id, None);
        assert_eq!(signal.duration_ms, None);
    }

    #[test]
    fn signal_with_severity_override() {
        let signal = Signal::with_severity(
            2,
            LoopStep::Act,
            SignalKind::Friction,
            SignalSeverity::Critical,
            "critical failure",
            serde_json::json!({"error": "oom"}),
            2000,
        );
        assert_eq!(signal.id, 2);
        assert_eq!(signal.severity, SignalSeverity::Critical);
    }

    #[test]
    fn signal_builder_methods() {
        let signal = Signal::new(
            3,
            LoopStep::Reason,
            SignalKind::Trace,
            "trace",
            serde_json::json!({}),
            3000,
        )
        .with_span_id("span-123")
        .with_cause_id(2)
        .with_duration_ms(150);

        assert_eq!(signal.span_id, Some("span-123".to_string()));
        assert_eq!(signal.cause_id, Some(2));
        assert_eq!(signal.duration_ms, Some(150));
    }

    #[test]
    fn signal_serialization_roundtrip() {
        let original = Signal::new(
            42,
            LoopStep::Act,
            SignalKind::Success,
            "success",
            serde_json::json!({"tool": "read_file"}),
            1000,
        )
        .with_span_id("test-span")
        .with_cause_id(41)
        .with_duration_ms(250);

        let json = serde_json::to_string(&original).expect("serialize");
        let deserialized: Signal = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(original, deserialized);
    }
}
