pub mod mock;

use crate::error::ForgeError;
use crate::format::ModelFormat;
use crate::objective::TrainObjective;
use crate::progress::ArtifactType;
use crate::CostRecord;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendCapabilities {
    pub name: String,
    pub supports_lora: bool,
    pub supports_full_finetune: bool,
    pub supports_pretraining: bool,
    pub max_model_params: Option<u64>,
    pub max_dataset_gb: Option<u64>,
    pub has_gpu: bool,
    pub gpu_vram_gb: Option<u32>,
    pub estimated_cost_per_gpu_hour: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct JobHandle(pub String);

impl std::fmt::Display for JobHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainProgress {
    pub phase: String,
    pub epoch: Option<u32>,
    pub total_epochs: Option<u32>,
    pub step: u64,
    pub total_steps: Option<u64>,
    pub loss: Option<f64>,
    pub learning_rate: Option<f64>,
    pub elapsed_secs: u64,
    pub estimated_remaining_secs: Option<u64>,
    pub tokens_processed: Option<u64>,
    pub gpu_utilization_pct: Option<f32>,
    pub cost_so_far_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainArtifact {
    pub artifact_type: ArtifactType,
    pub format: ModelFormat,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub final_loss: Option<f64>,
    pub training_duration_secs: u64,
    pub examples_or_tokens_processed: u64,
    pub cost: Option<CostRecord>,
    pub metadata: serde_json::Value,
}

#[async_trait::async_trait]
pub trait TrainBackend: Send + Sync {
    fn capabilities(&self) -> &BackendCapabilities;
    fn validate(&self, objective: &TrainObjective) -> Result<(), ForgeError>;
    async fn estimate_cost(&self, objective: &TrainObjective) -> Result<Option<f64>, ForgeError>;
    async fn submit(&self, objective: &TrainObjective) -> Result<JobHandle, ForgeError>;
    async fn progress(&self, handle: &JobHandle) -> Result<TrainProgress, ForgeError>;
    async fn wait(&self, handle: &JobHandle) -> Result<TrainArtifact, ForgeError>;
    async fn logs(&self, handle: &JobHandle, tail: usize) -> Result<Vec<String>, ForgeError>;
    async fn cancel(&self, handle: &JobHandle) -> Result<(), ForgeError>;
    async fn resume(&self, handle: &JobHandle) -> Result<Option<JobHandle>, ForgeError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_handle_display() {
        let handle = JobHandle("job-123".to_owned());
        assert_eq!(handle.to_string(), "job-123");
    }

    #[test]
    fn train_progress_roundtrip() {
        let progress = TrainProgress {
            phase: "training".to_owned(),
            epoch: Some(2),
            total_epochs: Some(3),
            step: 100,
            total_steps: Some(300),
            loss: Some(0.42),
            learning_rate: Some(1e-4),
            elapsed_secs: 120,
            estimated_remaining_secs: Some(240),
            tokens_processed: Some(50_000),
            gpu_utilization_pct: Some(95.0),
            cost_so_far_usd: Some(1.5),
        };
        let json = serde_json::to_string(&progress).unwrap();
        let decoded: TrainProgress = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.step, 100);
    }
}
