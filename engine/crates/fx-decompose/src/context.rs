use crate::error::DecomposeError;
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

/// Minimal experiment representation for decomposition.
/// Uses local types to avoid circular dependency with fx-consensus.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Experiment {
    pub hypothesis: String,
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

impl DecompositionContext {
    /// Validates that the context has sensible values.
    pub fn validate(&self) -> Result<(), DecomposeError> {
        if self.max_sub_goals == 0 {
            return Err(DecomposeError::BudgetExceeded(
                "max_sub_goals must be at least 1".to_owned(),
            ));
        }
        if self.max_complexity_weight == 0 {
            return Err(DecomposeError::BudgetExceeded(
                "max_complexity_weight must be at least 1".to_owned(),
            ));
        }
        Ok(())
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

    #[test]
    fn validate_rejects_zero_max_sub_goals() {
        let context = DecompositionContext {
            max_sub_goals: 0,
            ..DecompositionContext::default()
        };
        let error = context.validate().unwrap_err();
        assert!(error.to_string().contains("max_sub_goals"));
    }

    #[test]
    fn validate_rejects_zero_max_complexity_weight() {
        let context = DecompositionContext {
            max_complexity_weight: 0,
            ..DecompositionContext::default()
        };
        let error = context.validate().unwrap_err();
        assert!(error.to_string().contains("max_complexity_weight"));
    }
}
