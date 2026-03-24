//! Error types for the improvement pipeline.

use thiserror::Error;

/// Errors produced by the improvement pipeline.
#[derive(Debug, Error)]
pub enum ImprovementError {
    /// Error from the signal analysis engine.
    #[error("analysis error: {0}")]
    Analysis(String),

    /// Error during improvement planning (LLM tool call issues, etc.).
    #[error("planning error: {0}")]
    Planning(String),

    /// The planner model failed to call the required plan-generation tool.
    #[error("planning error: model did not call generate_fix_plan tool")]
    MissingPlanToolCall,

    /// Error writing a proposal to disk.
    #[error("proposal error: {0}")]
    Proposal(#[from] fx_propose::ProposalError),

    /// Error from the LLM completion provider.
    #[error("llm error: {0}")]
    Llm(#[from] fx_llm::ProviderError),

    /// Filesystem I/O error.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// Git operation failed.
    #[error("git error: {0}")]
    Git(String),

    /// Invalid configuration.
    #[error("config error: {0}")]
    Config(String),

    /// Error reading or writing fingerprint history.
    #[error("history error: {0}")]
    History(String),
}

impl From<fx_analysis::AnalysisError> for ImprovementError {
    fn from(e: fx_analysis::AnalysisError) -> Self {
        Self::Analysis(e.to_string())
    }
}
