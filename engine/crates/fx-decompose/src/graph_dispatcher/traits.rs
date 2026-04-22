use crate::{DecomposeError, ThoughtState};
use async_trait::async_trait;

#[derive(Debug, Clone, PartialEq)]
pub struct ThoughtScore {
    pub value: f64,
    pub llm_calls: usize,
}

impl ThoughtScore {
    pub const fn new(value: f64, llm_calls: usize) -> Self {
        Self { value, llm_calls }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedThoughts {
    pub contents: Vec<String>,
    pub llm_calls: usize,
}

impl GeneratedThoughts {
    pub fn new(contents: Vec<String>, llm_calls: usize) -> Self {
        Self {
            contents,
            llm_calls,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergedThought {
    pub content: String,
    pub llm_calls: usize,
}

impl MergedThought {
    pub fn new(content: String, llm_calls: usize) -> Self {
        Self { content, llm_calls }
    }
}

/// Pluggable scoring for thoughts. Used by Score and Refine operations.
#[async_trait]
pub trait ThoughtScorer: Send + Sync {
    async fn score(
        &self,
        thought: &ThoughtState,
        criteria: &str,
    ) -> Result<ThoughtScore, DecomposeError>;
}

/// Generates new thoughts from a parent thought.
#[async_trait]
pub trait ThoughtGenerator: Send + Sync {
    async fn generate(
        &self,
        parent: &ThoughtState,
        num_branches: usize,
        prompt_override: Option<&str>,
    ) -> Result<GeneratedThoughts, DecomposeError>;
}

/// Merges multiple thoughts into one.
#[async_trait]
pub trait ThoughtMerger: Send + Sync {
    async fn merge(
        &self,
        thoughts: &[&ThoughtState],
        instruction: Option<&str>,
    ) -> Result<MergedThought, DecomposeError>;
}
