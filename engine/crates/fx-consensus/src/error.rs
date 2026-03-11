use thiserror::Error;

pub type Result<T> = std::result::Result<T, ConsensusError>;

#[derive(Debug, Error)]
pub enum ConsensusError {
    #[error("chain integrity failure at index {index}: {message}")]
    ChainIntegrity { index: usize, message: String },
    #[error("storage error: {0}")]
    Storage(String),
    #[error("invalid experiment: {0}")]
    InvalidExperiment(String),
    #[error("no consensus: {0}")]
    NoConsensus(String),
    #[error("safety violation: {0}")]
    SafetyViolation(String),
}
