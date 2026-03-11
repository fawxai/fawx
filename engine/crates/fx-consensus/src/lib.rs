pub mod chain;
pub mod error;
pub mod orchestrator;
pub mod protocol;
pub mod scoring;
pub mod types;

pub use chain::{Chain, ChainEntry, ChainStorage, JsonFileChainStorage};
pub use error::{ConsensusError, Result};
pub use orchestrator::{CandidateEvaluator, CandidateGenerator, ExperimentOrchestrator};
pub use protocol::{ConsensusProtocol, ExperimentConfig, LocalConsensusEngine};
pub use scoring::{compute_aggregate_scores, determine_winner};
pub use types::{
    Candidate, ConsensusResult, Decision, Evaluation, Experiment, FitnessCriterion, MetricType,
    ModificationScope, NodeId, PathPattern, ProposalTier, Severity, Signal,
};
