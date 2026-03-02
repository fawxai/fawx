use fx_core::signals::SignalKind;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnalysisFinding {
    pub pattern_name: String,
    pub description: String,
    pub confidence: Confidence,
    pub evidence: Vec<SignalEvidence>,
    pub suggested_action: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SignalEvidence {
    pub session_id: String,
    pub signal_kind: SignalKind,
    pub message: String,
    pub timestamp_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analysis_finding_roundtrips_through_json() {
        let finding = AnalysisFinding {
            pattern_name: "Tool timeout loop".to_string(),
            description: "Search calls repeatedly timeout under load.".to_string(),
            confidence: Confidence::High,
            evidence: vec![SignalEvidence {
                session_id: "sess-1".to_string(),
                signal_kind: SignalKind::Friction,
                message: "search timed out".to_string(),
                timestamp_ms: 123,
            }],
            suggested_action: Some("Increase timeout budget and retry policy".to_string()),
        };

        let encoded = serde_json::to_string(&finding).expect("serialize finding");
        let decoded: AnalysisFinding = serde_json::from_str(&encoded).expect("deserialize finding");

        assert_eq!(decoded, finding);
        assert!(encoded.contains("\"confidence\":\"high\""));
    }
}
