pub mod cargo_workspace;
pub mod chain;
pub mod error;
pub mod evaluator;
pub mod generator;
pub mod llm_source;
pub mod orchestrator;
pub mod progress_format;
pub mod protocol;
pub mod remote_workspace;
pub(crate) mod response_parser;
pub mod runner;
pub mod scoring;
pub mod subagent_source;
#[cfg(any(test, feature = "test-support"))]
pub mod test_fixtures;
pub mod tournament;
pub mod training;
pub mod types;

pub use cargo_workspace::CargoWorkspace;
pub use chain::{Chain, ChainEntry, ChainStorage, JsonFileChainStorage};
pub use error::{ConsensusError, Result};
pub use evaluator::{BuildTestEvaluator, EvaluationWorkspace, TestResult};
pub use generator::{
    aggressive_prompt, conservative_prompt, creative_prompt, GenerationStrategy,
    LlmCandidateGenerator, PatchResponse, PatchSource,
};
pub use orchestrator::{CandidateEvaluator, CandidateGenerator};
pub use progress_format::{display_strategy, format_progress_event, StrategyDisplay};
pub use protocol::{ConsensusProtocol, ExperimentConfig, LocalConsensusEngine};
pub use runner::{
    build_summary, evaluation_build_success, format_auto_chain_result, note_value, plural_suffix,
    validate_auto_chain_rounds, AutoChainResult, BuildOutcome, BuildSummary, CandidateReport,
    ExperimentReport, ExperimentRunner, NeutralEvaluatorConfig, NodeConfig, ProgressCallback,
    ProgressEvent, RoundNodes, RoundNodesBuilder,
};
pub use scoring::{compute_aggregate_scores, determine_winner};
pub use types::{
    Candidate, ConsensusResult, Decision, Evaluation, Experiment, FitnessCriterion, MetricType,
    ModificationScope, NodeId, PathPattern, ProposalTier, Severity, Signal,
};

pub use llm_source::{
    build_experiment_prompt, build_subagent_experiment_prompt, format_chain_history,
    load_chain_history_for_signal, LlmPatchSource, CHAIN_HISTORY_LIMIT,
};
pub use remote_workspace::{RemoteEvalTarget, RemoteEvaluationWorkspace};
pub use subagent_source::SubagentPatchSource;
