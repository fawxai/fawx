#[derive(Debug, thiserror::Error)]
pub enum DecomposeError {
    #[error("decomposition failed: {0}")]
    DecompositionFailed(String),

    #[error("sub-goal dispatch failed: {0}")]
    DispatchFailed(String),

    #[error("patch merge conflict between sub-goals {a} and {b}: {detail}")]
    MergeConflict { a: usize, b: usize, detail: String },

    #[error("aggregation failed: {0}")]
    AggregationFailed(String),

    #[error("budget exceeded: {0}")]
    BudgetExceeded(String),

    #[error("no fleet nodes available: {0}")]
    NoNodesAvailable(String),

    #[error("DAG parse error: {0}")]
    DagParseError(String),
}
