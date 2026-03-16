use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PathPattern(pub String);

impl From<&str> for PathPattern {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChainEntry {
    pub summary: String,
}

/// Context provided to a decomposer for informed planning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecompositionContext {
    /// Relevant source files (path → content snippet).
    pub source_files: BTreeMap<String, String>,
    /// Signal scope (allowed modification paths).
    pub scope: Vec<PathPattern>,
    /// Previous experiment results for this signal.
    pub chain_history: Vec<ChainEntry>,
    /// Maximum number of sub-goals.
    pub max_sub_goals: usize,
    /// Maximum total complexity weight.
    pub max_complexity_weight: u32,
}

impl Default for DecompositionContext {
    fn default() -> Self {
        Self {
            source_files: BTreeMap::new(),
            scope: Vec::new(),
            chain_history: Vec::new(),
            max_sub_goals: 8,
            max_complexity_weight: 16,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_context_has_expected_limits() {
        let context = DecompositionContext::default();
        assert_eq!(context.max_sub_goals, 8);
        assert_eq!(context.max_complexity_weight, 16);
        assert!(context.source_files.is_empty());
        assert!(context.scope.is_empty());
        assert!(context.chain_history.is_empty());
    }
}
