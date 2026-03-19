#[derive(Debug, thiserror::Error)]
pub enum TrainingError {
    #[error("extraction failed: {0}")]
    ExtractionFailed(String),
    #[error("filter error: {0}")]
    FilterError(String),
    #[error("dataset I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("chain load error: {0}")]
    Chain(String),
    #[error("no examples match the given criteria")]
    NoExamples,
}
