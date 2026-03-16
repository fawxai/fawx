mod collector;
mod consent;
mod error;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub use collector::SignalCollector;
pub use consent::TelemetryConsent;
pub use error::TelemetryError;

/// A single telemetry signal emitted by the engine.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TelemetrySignal {
    pub id: Uuid,
    pub category: SignalCategory,
    pub event: String,
    pub value: serde_json::Value,
    pub timestamp: DateTime<Utc>,
    pub session_id: String,
}

/// Categories of telemetry signals.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SignalCategory {
    ToolUsage,
    ProposalGate,
    Experiments,
    Errors,
    ModelUsage,
    Performance,
}

impl SignalCategory {
    pub fn all() -> Vec<Self> {
        vec![
            Self::ToolUsage,
            Self::ProposalGate,
            Self::Experiments,
            Self::Errors,
            Self::ModelUsage,
            Self::Performance,
        ]
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::ToolUsage => "Which tools succeed/fail and how often",
            Self::ProposalGate => "How often the safety gate activates",
            Self::Experiments => "Experiment scores and outcomes (no code content)",
            Self::Errors => "Error rates and categories",
            Self::ModelUsage => "Which models and thinking levels are used",
            Self::Performance => "Response times and token counts",
        }
    }
}

impl std::fmt::Display for SignalCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::ToolUsage => "tool_usage",
                Self::ProposalGate => "proposal_gate",
                Self::Experiments => "experiments",
                Self::Errors => "errors",
                Self::ModelUsage => "model_usage",
                Self::Performance => "performance",
            }
        )
    }
}

/// Hash an error message for telemetry (privacy: no raw message text).
pub fn hash_error_message(message: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(message.as_bytes());
    format!("{:x}", digest).chars().take(16).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_category_all_covers_every_variant() {
        let all = SignalCategory::all();
        assert_eq!(all.len(), 6);
    }

    #[test]
    fn signal_category_descriptions_non_empty() {
        for category in SignalCategory::all() {
            assert!(!category.description().is_empty());
        }
    }

    #[test]
    fn signal_category_display() {
        assert_eq!(SignalCategory::ToolUsage.to_string(), "tool_usage");
        assert_eq!(SignalCategory::ProposalGate.to_string(), "proposal_gate");
    }

    #[test]
    fn hash_error_message_deterministic() {
        let hash1 = hash_error_message("connection refused");
        let hash2 = hash_error_message("connection refused");
        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 16);
    }

    #[test]
    fn hash_error_message_different_for_different_inputs() {
        let hash1 = hash_error_message("error a");
        let hash2 = hash_error_message("error b");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn telemetry_signal_roundtrip_serde() {
        let signal = TelemetrySignal {
            id: Uuid::new_v4(),
            category: SignalCategory::ToolUsage,
            event: "tool_call".to_owned(),
            value: serde_json::json!({"tool": "read_file", "success": true}),
            timestamp: Utc::now(),
            session_id: "session-abc".to_owned(),
        };
        let json = serde_json::to_string(&signal).unwrap();
        let decoded: TelemetrySignal = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.category, signal.category);
        assert_eq!(decoded.event, signal.event);
    }

    /// Compile-time enforcement: if a new SignalCategory variant is added,
    /// this match will fail to compile, reminding the developer to update all().
    #[test]
    fn signal_category_all_is_exhaustive() {
        for category in SignalCategory::all() {
            match category {
                SignalCategory::ToolUsage
                | SignalCategory::ProposalGate
                | SignalCategory::Experiments
                | SignalCategory::Errors
                | SignalCategory::ModelUsage
                | SignalCategory::Performance => {}
            }
        }
        // If you add a variant and this doesn't compile, add it to all() too.
    }
}
