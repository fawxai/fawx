pub mod artifact;
pub mod backend;
pub mod dataset;
pub mod error;
pub mod eval;
pub mod format;
pub mod objective;
pub mod orchestrator;
pub mod progress;
pub mod serving;
mod storage;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub use artifact::{ArtifactFilter, ArtifactInfo, ArtifactManager};
pub use backend::{BackendCapabilities, JobHandle, TrainArtifact, TrainBackend, TrainProgress};
pub use dataset::{DatasetFormat, DatasetRef};
pub use error::ForgeError;
pub use eval::{EvalConfig, EvalPrompt, EvalResults};
pub use format::{ConversionResult, FormatConverter, ModelFormat, QuantizationConfig};
pub use objective::{FinetuneConfig, LoraConfig, PretrainConfig, TrainObjective};
pub use orchestrator::{ForgeEvent, ForgeOrchestrator, ForgeProgressCallback, JobValidation};
pub use progress::ArtifactType;

/// A training job submitted to the forge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeJob {
    pub id: Uuid,
    pub name: String,
    pub status: JobStatus,
    pub objective: TrainObjective,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub result: Option<TrainArtifact>,
    pub error: Option<String>,
    pub backend: String,
    #[serde(default)]
    pub handle: Option<JobHandle>,
    pub cost: Option<CostRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Pending,
    Preparing,
    Uploading,
    Training,
    Converting,
    Evaluating,
    Completed,
    Failed,
    Cancelled,
}

impl std::fmt::Display for JobStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Preparing => write!(f, "preparing"),
            Self::Uploading => write!(f, "uploading"),
            Self::Training => write!(f, "training"),
            Self::Converting => write!(f, "converting"),
            Self::Evaluating => write!(f, "evaluating"),
            Self::Completed => write!(f, "completed"),
            Self::Failed => write!(f, "failed"),
            Self::Cancelled => write!(f, "cancelled"),
        }
    }
}

/// Cost tracking for a training job.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CostRecord {
    pub estimated_usd: Option<f64>,
    pub actual_usd: Option<f64>,
    pub gpu_hours: Option<f64>,
    pub details: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_status_display() {
        assert_eq!(JobStatus::Training.to_string(), "training");
        assert_eq!(JobStatus::Completed.to_string(), "completed");
    }

    #[test]
    fn cost_record_roundtrip() {
        let cost = CostRecord {
            estimated_usd: Some(10.0),
            actual_usd: Some(8.5),
            gpu_hours: Some(2.0),
            details: serde_json::json!({"provider": "runpod"}),
        };
        let json = serde_json::to_string(&cost).unwrap();
        let decoded: CostRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, cost);
    }

    #[test]
    fn forge_job_serializes() {
        let job = ForgeJob {
            id: Uuid::new_v4(),
            name: "test".to_owned(),
            status: JobStatus::Pending,
            objective: TrainObjective::Lora(LoraConfig {
                base_model: "test-model".to_owned(),
                ..LoraConfig::default()
            }),
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            result: None,
            error: None,
            backend: "mock".to_owned(),
            handle: None,
            cost: None,
        };
        let json = serde_json::to_string(&job).unwrap();
        assert!(json.contains("pending"));
    }
}
