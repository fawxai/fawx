use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Experiment {
    pub id: Uuid,
    pub trigger: Signal,
    pub hypothesis: String,
    pub fitness_criteria: Vec<FitnessCriterion>,
    pub scope: ModificationScope,
    #[serde(with = "duration_serde")]
    pub timeout: Duration,
    pub min_candidates: u32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FitnessCriterion {
    pub name: String,
    pub metric_type: MetricType,
    pub weight: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MetricType {
    Lower,
    Higher,
    Boolean,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModificationScope {
    pub allowed_files: Vec<String>,
    pub proposal_tier: ProposalTier,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProposalTier {
    Tier1,
    Tier2,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signal {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub severity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub id: Uuid,
    pub experiment_id: Uuid,
    pub node_id: String,
    pub patch: String,
    pub approach: String,
    pub self_metrics: BTreeMap<String, f64>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evaluation {
    pub candidate_id: Uuid,
    pub evaluator_id: String,
    pub fitness_scores: BTreeMap<String, f64>,
    pub safety_pass: bool,
    pub signal_resolved: bool,
    pub regression_detected: bool,
    pub notes: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusResult {
    pub experiment_id: Uuid,
    pub winner: Option<Uuid>,
    pub candidates: Vec<Uuid>,
    pub evaluations: Vec<Evaluation>,
    pub aggregate_scores: BTreeMap<Uuid, f64>,
    pub decision: Decision,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Decision {
    Accept,
    Reject,
    Inconclusive,
}

mod duration_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    #[derive(Serialize, Deserialize)]
    struct DurationRepr {
        secs: u64,
        nanos: u32,
    }

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        DurationRepr {
            secs: duration.as_secs(),
            nanos: duration.subsec_nanos(),
        }
        .serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let repr = DurationRepr::deserialize(deserializer)?;
        Ok(Duration::new(repr.secs, repr.nanos))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn serializes_and_deserializes_all_types() {
        let experiment = sample_experiment();
        let candidate = sample_candidate(experiment.id, "node-a");
        let evaluation = sample_evaluation(candidate.id, "node-b", 0.8);
        let result = ConsensusResult {
            experiment_id: experiment.id,
            winner: Some(candidate.id),
            candidates: vec![candidate.id],
            evaluations: vec![evaluation],
            aggregate_scores: BTreeMap::from([(candidate.id, 0.9)]),
            decision: Decision::Accept,
            timestamp: Utc::now(),
        };

        round_trip(&experiment);
        round_trip(&candidate);
        round_trip(&result);
    }

    fn round_trip<T>(value: &T)
    where
        T: Serialize + for<'de> Deserialize<'de>,
    {
        let json = serde_json::to_string(value).expect("serialize test value");
        let _: T = serde_json::from_str(&json).expect("deserialize test value");
    }

    pub(crate) fn sample_experiment() -> Experiment {
        Experiment {
            id: Uuid::new_v4(),
            trigger: Signal {
                id: Uuid::new_v4(),
                name: "latency".into(),
                description: "High latency detected".into(),
                severity: "high".into(),
            },
            hypothesis: "parallelism helps".into(),
            fitness_criteria: vec![FitnessCriterion {
                name: "latency".into(),
                metric_type: MetricType::Lower,
                weight: 1.0,
            }],
            scope: ModificationScope {
                allowed_files: vec!["src/**/*.rs".into()],
                proposal_tier: ProposalTier::Tier1,
            },
            timeout: Duration::from_secs(300),
            min_candidates: 2,
            created_at: Utc::now(),
        }
    }

    pub(crate) fn sample_candidate(experiment_id: Uuid, node_id: &str) -> Candidate {
        Candidate {
            id: Uuid::new_v4(),
            experiment_id,
            node_id: node_id.into(),
            patch: "diff --git a/src/lib.rs b/src/lib.rs".into(),
            approach: "Optimize scoring".into(),
            self_metrics: BTreeMap::from([("latency".into(), 123.0)]),
            created_at: Utc::now(),
        }
    }

    pub(crate) fn sample_evaluation(
        candidate_id: Uuid,
        evaluator_id: &str,
        score: f64,
    ) -> Evaluation {
        Evaluation {
            candidate_id,
            evaluator_id: evaluator_id.into(),
            fitness_scores: BTreeMap::from([("latency".into(), score)]),
            safety_pass: true,
            signal_resolved: true,
            regression_detected: false,
            notes: "Looks good".into(),
            created_at: Utc::now(),
        }
    }
}
