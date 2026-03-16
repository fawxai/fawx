use super::{BackendCapabilities, JobHandle, TrainArtifact, TrainBackend, TrainProgress};
use crate::error::ForgeError;
use crate::format::ModelFormat;
use crate::objective::TrainObjective;
use crate::progress::ArtifactType;
use std::path::PathBuf;

pub struct MockBackend {
    capabilities: BackendCapabilities,
    artifact: TrainArtifact,
}

impl MockBackend {
    pub fn new() -> Self {
        Self {
            capabilities: BackendCapabilities {
                name: "mock".to_owned(),
                supports_lora: true,
                supports_full_finetune: true,
                supports_pretraining: false,
                max_model_params: None,
                max_dataset_gb: None,
                has_gpu: false,
                gpu_vram_gb: None,
                estimated_cost_per_gpu_hour: Some(0.0),
            },
            artifact: TrainArtifact {
                artifact_type: ArtifactType::LoraAdapter,
                format: ModelFormat::Safetensors,
                path: PathBuf::from("mock-adapter"),
                size_bytes: 1024,
                final_loss: Some(0.1),
                training_duration_secs: 10,
                examples_or_tokens_processed: 100,
                cost: None,
                metadata: serde_json::json!({"mock": true}),
            },
        }
    }

    pub fn with_capabilities(mut self, capabilities: BackendCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }
}

impl Default for MockBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl TrainBackend for MockBackend {
    fn capabilities(&self) -> &BackendCapabilities {
        &self.capabilities
    }

    fn validate(&self, objective: &TrainObjective) -> Result<(), ForgeError> {
        match objective {
            TrainObjective::Lora(_) if self.capabilities.supports_lora => Ok(()),
            TrainObjective::FullFinetune(_) if self.capabilities.supports_full_finetune => Ok(()),
            TrainObjective::ContinuedPretrain(_) if self.capabilities.supports_pretraining => {
                Ok(())
            }
            _ => Err(ForgeError::NoBackendAvailable(
                self.capabilities.name.clone(),
            )),
        }
    }

    async fn estimate_cost(&self, _objective: &TrainObjective) -> Result<Option<f64>, ForgeError> {
        Ok(self.capabilities.estimated_cost_per_gpu_hour)
    }

    async fn submit(&self, _objective: &TrainObjective) -> Result<JobHandle, ForgeError> {
        Ok(JobHandle(uuid::Uuid::new_v4().to_string()))
    }

    async fn progress(&self, _handle: &JobHandle) -> Result<TrainProgress, ForgeError> {
        Ok(TrainProgress {
            phase: "complete".to_owned(),
            epoch: Some(3),
            total_epochs: Some(3),
            step: 100,
            total_steps: Some(100),
            loss: Some(0.1),
            learning_rate: Some(1e-4),
            elapsed_secs: 10,
            estimated_remaining_secs: Some(0),
            tokens_processed: Some(100),
            gpu_utilization_pct: None,
            cost_so_far_usd: Some(0.0),
        })
    }

    async fn wait(&self, _handle: &JobHandle) -> Result<TrainArtifact, ForgeError> {
        Ok(self.artifact.clone())
    }

    async fn logs(&self, _handle: &JobHandle, _tail: usize) -> Result<Vec<String>, ForgeError> {
        Ok(vec!["mock training complete".to_owned()])
    }

    async fn cancel(&self, _handle: &JobHandle) -> Result<(), ForgeError> {
        Ok(())
    }

    async fn resume(&self, _handle: &JobHandle) -> Result<Option<JobHandle>, ForgeError> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_backend_full_lifecycle() {
        let backend = MockBackend::new();
        let objective = TrainObjective::Lora(crate::LoraConfig::default());

        backend.validate(&objective).unwrap();
        let cost = backend.estimate_cost(&objective).await.unwrap();
        assert_eq!(cost, Some(0.0));

        let handle = backend.submit(&objective).await.unwrap();
        let progress = backend.progress(&handle).await.unwrap();
        assert_eq!(progress.phase, "complete");

        let artifact = backend.wait(&handle).await.unwrap();
        assert_eq!(artifact.artifact_type, ArtifactType::LoraAdapter);

        let logs = backend.logs(&handle, 10).await.unwrap();
        assert_eq!(logs.len(), 1);

        backend.cancel(&handle).await.unwrap();
        let resumed = backend.resume(&handle).await.unwrap();
        assert!(resumed.is_none());
    }

    #[test]
    fn mock_capabilities() {
        let backend = MockBackend::new();
        assert!(backend.capabilities().supports_lora);
        assert!(!backend.capabilities().supports_pretraining);
    }

    #[test]
    fn validate_rejects_unsupported_objective() {
        let backend = MockBackend::new().with_capabilities(BackendCapabilities {
            name: "mock".to_owned(),
            supports_lora: false,
            supports_full_finetune: false,
            supports_pretraining: false,
            max_model_params: None,
            max_dataset_gb: None,
            has_gpu: false,
            gpu_vram_gb: None,
            estimated_cost_per_gpu_hour: Some(0.0),
        });
        let result = backend.validate(&TrainObjective::Lora(crate::LoraConfig::default()));
        assert!(result.is_err());
    }
}
