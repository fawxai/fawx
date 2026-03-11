use thiserror::Error;
use uuid::Uuid;

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
    #[error("experiment not found: {0}")]
    ExperimentNotFound(Uuid),
    #[error("experiment already finalized: {0}")]
    ExperimentAlreadyFinalized(Uuid),
    #[error("candidate not found: {0}")]
    CandidateNotFound(Uuid),
    #[error("insufficient candidates: required {required}, received {received}")]
    InsufficientCandidates { required: u32, received: u32 },
    #[error("protocol error: {0}")]
    Protocol(String),
}
