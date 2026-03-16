use crate::error::DecomposeError;
use chrono::{DateTime, Utc};
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
#[derive(Debug, Clone, PartialEq)]
pub struct DecompositionContext {
    /// Relevant source files (path → content snippet).
    pub source_files: BTreeMap<String, String>,
    /// Signal scope (allowed modification paths).
    pub scope: Vec<PathPattern>,
    /// Previous experiment results for this signal.
    pub chain_history: Vec<ChainEntry>,
    /// Historical fitness data from prior decomposition attempts.
    pub fitness: FitnessContext,
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
            fitness: FitnessContext::default(),
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

/// Historical fitness data from prior decomposition attempts.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct FitnessContext {
    /// Prior decomposition attempts for this signal.
    pub prior_attempts: Vec<DecompositionAttempt>,
    /// Aggregate statistics across all attempts.
    pub stats: FitnessStats,
}

/// Outcome of a decomposition attempt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AttemptDecision {
    Accept,
    Reject,
    Inconclusive,
}

impl std::fmt::Display for AttemptDecision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Accept => write!(f, "accept"),
            Self::Reject => write!(f, "reject"),
            Self::Inconclusive => write!(f, "inconclusive"),
        }
    }
}

/// Outcome of an individual sub-goal attempt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubGoalAttemptOutcome {
    Completed,
    Failed,
    Skipped,
    BudgetExhausted,
}

impl std::fmt::Display for SubGoalAttemptOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Completed => write!(f, "completed"),
            Self::Failed => write!(f, "failed"),
            Self::Skipped => write!(f, "skipped"),
            Self::BudgetExhausted => write!(f, "budget_exhausted"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DecompositionAttempt {
    pub timestamp: DateTime<Utc>,
    pub sub_goals: Vec<SubGoalAttempt>,
    pub decision: AttemptDecision,
    pub best_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SubGoalAttempt {
    pub description: String,
    pub outcome: SubGoalAttemptOutcome,
    pub score: Option<f64>,
    pub failure_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct FitnessStats {
    pub total_attempts: usize,
    pub accepts: usize,
    pub rejects: usize,
    pub avg_best_score: f64,
    pub common_failures: Vec<(String, usize)>,
    pub successful_approaches: Vec<String>,
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

    #[test]
    fn fitness_context_default_is_empty() {
        let fitness = FitnessContext::default();
        assert!(fitness.prior_attempts.is_empty());
        assert_eq!(fitness.stats.total_attempts, 0);
    }

    #[test]
    fn fitness_context_roundtrip_serde() {
        let fitness = FitnessContext {
            prior_attempts: vec![DecompositionAttempt {
                timestamp: Utc::now(),
                sub_goals: vec![SubGoalAttempt {
                    description: "fix bug".to_owned(),
                    outcome: SubGoalAttemptOutcome::Completed,
                    score: Some(0.8),
                    failure_reason: None,
                }],
                decision: AttemptDecision::Accept,
                best_score: 0.8,
            }],
            stats: FitnessStats {
                total_attempts: 1,
                accepts: 1,
                rejects: 0,
                avg_best_score: 0.8,
                common_failures: vec![],
                successful_approaches: vec!["fix bug".to_owned()],
            },
        };
        let json = serde_json::to_string(&fitness).unwrap();
        let decoded: FitnessContext = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.stats.total_attempts, 1);
    }

    #[test]
    fn decomposition_context_includes_fitness() {
        let context = DecompositionContext::default();
        assert!(context.fitness.prior_attempts.is_empty());
    }
}
