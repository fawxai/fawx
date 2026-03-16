use crate::context::Experiment;
use crate::error::DecomposeError;
use crate::{DecompositionPlan, SubGoalOutcome, SubGoalResult};

/// Aggregated result from executing a decomposition plan.
#[derive(Debug, Clone)]
pub struct AggregatedResult {
    pub combined_patch: String,
    pub approach: String,
    pub sub_goal_outcomes: Vec<(String, SubGoalOutcome)>,
    pub completion_rate: f64,
}

#[async_trait::async_trait]
pub trait ResultAggregator: Send + Sync {
    async fn aggregate(
        &self,
        plan: &DecompositionPlan,
        results: &[SubGoalResult],
        experiment: &Experiment,
    ) -> Result<AggregatedResult, DecomposeError>;
}

/// Simple aggregator that concatenates patches from completed sub-goals.
pub struct SimpleAggregator;

#[async_trait::async_trait]
impl ResultAggregator for SimpleAggregator {
    async fn aggregate(
        &self,
        _plan: &DecompositionPlan,
        results: &[SubGoalResult],
        _experiment: &Experiment,
    ) -> Result<AggregatedResult, DecomposeError> {
        let mut patches = Vec::new();
        let mut approaches = Vec::new();
        let mut outcomes = Vec::new();
        let mut completed = 0;

        for result in results {
            let description = result.goal.description.clone();
            if let SubGoalOutcome::Completed(patch) = &result.outcome {
                patches.push(patch.clone());
                approaches.push(format!("{description}: completed"));
                completed += 1;
            }
            outcomes.push((description, result.outcome.clone()));
        }

        let total = results.len();
        let completion_rate = if total == 0 {
            0.0
        } else {
            completed as f64 / total as f64
        };

        Ok(AggregatedResult {
            combined_patch: patches.join("\n"),
            approach: approaches.join("; "),
            sub_goal_outcomes: outcomes,
            completion_rate,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ComplexityHint, SubGoal};

    fn sample_experiment() -> Experiment {
        Experiment {
            hypothesis: "test".to_owned(),
        }
    }

    fn goal(description: &str) -> SubGoal {
        SubGoal {
            description: description.to_owned(),
            required_tools: vec![],
            expected_output: None,
            complexity_hint: Some(ComplexityHint::Trivial),
        }
    }

    fn completed(description: &str, patch: &str) -> SubGoalResult {
        SubGoalResult {
            goal: goal(description),
            outcome: SubGoalOutcome::Completed(patch.to_owned()),
            signals: Vec::new(),
        }
    }

    fn failed_result(description: &str) -> SubGoalResult {
        SubGoalResult {
            goal: goal(description),
            outcome: SubGoalOutcome::Failed("error".to_owned()),
            signals: Vec::new(),
        }
    }

    #[tokio::test]
    async fn aggregates_all_completed() {
        let plan = DecompositionPlan {
            sub_goals: vec![goal("A"), goal("B")],
            strategy: crate::AggregationStrategy::Sequential,
            truncated_from: None,
        };
        let results = vec![completed("A", "diff-a"), completed("B", "diff-b")];
        let aggregator = SimpleAggregator;
        let result = aggregator
            .aggregate(&plan, &results, &sample_experiment())
            .await
            .unwrap();

        assert_eq!(result.completion_rate, 1.0);
        assert!(result.combined_patch.contains("diff-a"));
        assert!(result.combined_patch.contains("diff-b"));
    }

    #[tokio::test]
    async fn partial_failure_reduces_rate() {
        let plan = DecompositionPlan {
            sub_goals: vec![goal("A"), goal("B")],
            strategy: crate::AggregationStrategy::Sequential,
            truncated_from: None,
        };
        let results = vec![completed("A", "diff-a"), failed_result("B")];
        let aggregator = SimpleAggregator;
        let result = aggregator
            .aggregate(&plan, &results, &sample_experiment())
            .await
            .unwrap();

        assert_eq!(result.completion_rate, 0.5);
        assert!(result.combined_patch.contains("diff-a"));
        assert!(!result.combined_patch.contains("diff-b"));
    }

    #[tokio::test]
    async fn empty_results() {
        let plan = DecompositionPlan {
            sub_goals: vec![],
            strategy: crate::AggregationStrategy::Sequential,
            truncated_from: None,
        };
        let aggregator = SimpleAggregator;
        let result = aggregator
            .aggregate(&plan, &[], &sample_experiment())
            .await
            .unwrap();

        assert_eq!(result.completion_rate, 0.0);
        assert!(result.combined_patch.is_empty());
    }
}
