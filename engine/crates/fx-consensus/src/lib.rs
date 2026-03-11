pub mod chain;
pub mod error;
pub mod evaluator;
pub mod experiment_cli;
pub mod generator;
pub mod orchestrator;
pub mod protocol;
pub mod scoring;
pub mod types;

pub use chain::{Chain, ChainEntry, ChainStorage, JsonFileChainStorage};
pub use error::{ConsensusError, Result};
pub use evaluator::{BuildTestEvaluator, EvaluationWorkspace, MockEvaluationWorkspace, TestResult};
pub use experiment_cli::{CandidateReport, ExperimentReport, ExperimentRunner, NodeConfig};
pub use generator::{
    aggressive_prompt, conservative_prompt, creative_prompt, GenerationStrategy,
    LlmCandidateGenerator, PatchResponse, PatchSource,
};
pub use orchestrator::{CandidateEvaluator, CandidateGenerator, ExperimentOrchestrator};
pub use protocol::{ConsensusProtocol, ExperimentConfig, LocalConsensusEngine};
pub use scoring::{compute_aggregate_scores, determine_winner};
pub use types::{
    Candidate, ConsensusResult, Decision, Evaluation, Experiment, FitnessCriterion, MetricType,
    ModificationScope, NodeId, PathPattern, ProposalTier, Severity, Signal,
};
