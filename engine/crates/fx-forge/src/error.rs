use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum ForgeError {
    #[error("training failed: {0}")]
    TrainingFailed(String),
    #[error("no backend available for objective: {0}")]
    NoBackendAvailable(String),
    #[error("backend error: {0}")]
    BackendError(String),
    #[error("dataset validation failed: {0}")]
    DatasetInvalid(String),
    #[error("format conversion failed: {0}")]
    ConversionFailed(String),
    #[error("evaluation failed: {0}")]
    EvaluationFailed(String),
    #[error("artifact error: {0}")]
    ArtifactError(String),
    #[error("job not found: {0}")]
    JobNotFound(Uuid),
    #[error("job already running: {0}")]
    JobAlreadyRunning(Uuid),
    #[error("invalid config: {0}")]
    InvalidConfig(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}
