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
}

impl fmt::Display for SignalKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.to_label())
    }
}

/// A structured observation emitted by a loop step.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Signal {
    pub step: LoopStep,
    pub kind: SignalKind,
    pub message: String,
    pub metadata: serde_json::Value,
    pub timestamp_ms: u64,
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
}
