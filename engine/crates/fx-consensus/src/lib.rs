pub mod chain;
pub mod error;
pub mod scoring;
pub mod types;

pub use chain::{Chain, ChainEntry, ChainStorage, JsonFileChainStorage};
pub use error::{ConsensusError, Result};
pub use scoring::{compute_aggregate_scores, determine_winner};
pub use types::{
    Candidate, ConsensusResult, Decision, Evaluation, Experiment, FitnessCriterion, MetricType,
    ModificationScope, ProposalTier, Signal,
};
