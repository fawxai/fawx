pub mod cargo_workspace;
pub mod chain;
pub mod error;
pub mod evaluator;
pub mod generator;
pub mod llm_source;
pub mod orchestrator;
pub mod protocol;
pub(crate) mod response_parser;
pub mod runner;
pub mod scoring;
pub mod subagent_source;
pub mod types;

pub use cargo_workspace::CargoWorkspace;
pub use chain::{Chain, ChainEntry, ChainStorage, JsonFileChainStorage};
pub use error::{ConsensusError, Result};
pub use evaluator::{BuildTestEvaluator, EvaluationWorkspace, TestResult};
pub use generator::{
    aggressive_prompt, conservative_prompt, creative_prompt, GenerationStrategy,
    LlmCandidateGenerator, PatchResponse, PatchSource,
};
pub use orchestrator::{CandidateEvaluator, CandidateGenerator, ExperimentOrchestrator};
pub use protocol::{ConsensusProtocol, ExperimentConfig, LocalConsensusEngine};
pub use runner::{
    CandidateReport, ExperimentReport, ExperimentRunner, NeutralEvaluatorConfig, NodeConfig,
};
pub use scoring::{compute_aggregate_scores, determine_winner};
pub use types::{
    Candidate, ConsensusResult, Decision, Evaluation, Experiment, FitnessCriterion, MetricType,
    ModificationScope, NodeId, PathPattern, ProposalTier, Severity, Signal,
};

pub use llm_source::{build_experiment_prompt, LlmPatchSource};
pub use subagent_source::SubagentPatchSource;
