use crate::context::Experiment;
use crate::dag::ExecutionDag;
use crate::error::DecomposeError;
use crate::{AggregationStrategy, DecompositionPlan, SubGoal, SubGoalOutcome, SubGoalResult};
use std::sync::Arc;

pub type DecompositionProgressCallback = Arc<dyn Fn(&DecompositionEvent) + Send + Sync>;

#[derive(Debug, Clone)]
pub enum DecompositionEvent {
    PlanGenerated { sub_goal_count: usize },
    SubGoalStarted { index: usize, description: String },
    SubGoalCompleted { index: usize },
    SubGoalFailed { index: usize, error: String },
    AggregationStarted { completed_count: usize },
    AggregationComplete { completion_rate: f64 },
}

#[async_trait::async_trait]
pub trait SubGoalExecutor: Send + Sync {
    /// Execute a single sub-goal.
    ///
    /// For sequential dispatch, `prior_results` contains results from previously
    /// completed sub-goals. For parallel and DAG dispatch, `prior_results` will
    /// be empty since goals execute independently.
    async fn execute(
        &self,
        goal: &SubGoal,
        experiment: &Experiment,
        prior_results: &[SubGoalResult],
    ) -> Result<SubGoalResult, DecomposeError>;
}

#[async_trait::async_trait]
pub trait SubGoalDispatcher: Send + Sync {
    async fn dispatch(
        &self,
        plan: &DecompositionPlan,
        experiment: &Experiment,
        progress: Option<&DecompositionProgressCallback>,
    ) -> Result<Vec<SubGoalResult>, DecomposeError>;
}

/// Executes sub-goals one at a time in order.
pub struct SequentialDispatcher {
    executor: Arc<dyn SubGoalExecutor>,
    fail_fast: bool,
}

impl SequentialDispatcher {
    pub fn new(executor: Arc<dyn SubGoalExecutor>, fail_fast: bool) -> Self {
        Self {
            executor,
            fail_fast,
        }
    }
}

#[async_trait::async_trait]
impl SubGoalDispatcher for SequentialDispatcher {
    async fn dispatch(
        &self,
        plan: &DecompositionPlan,
        experiment: &Experiment,
        progress: Option<&DecompositionProgressCallback>,
    ) -> Result<Vec<SubGoalResult>, DecomposeError> {
        let mut results = Vec::new();
        let mut failed = false;
        for (index, goal) in plan.sub_goals.iter().enumerate() {
            if failed && self.fail_fast {
                results.push(skipped_result(goal));
                continue;
            }

            emit(
                progress,
                DecompositionEvent::SubGoalStarted {
                    index,
                    description: goal.description.clone(),
                },
            );

            match self.executor.execute(goal, experiment, &results).await {
                Ok(result) => {
                    let event = if sub_goal_outcome_is_terminal_failure(&result.outcome) {
                        failed = true;
                        DecompositionEvent::SubGoalFailed {
                            index,
                            error: sub_goal_outcome_error(&result.outcome),
                        }
                    } else {
                        DecompositionEvent::SubGoalCompleted { index }
                    };
                    emit(progress, event);
                    results.push(result);
                }
                Err(error) => {
                    emit(
                        progress,
                        DecompositionEvent::SubGoalFailed {
                            index,
                            error: error.to_string(),
                        },
                    );
                    results.push(failed_result(goal, &error.to_string()));
                    failed = true;
                }
            }
        }
        Ok(results)
    }
}

/// Executes all sub-goals concurrently.
pub struct ParallelDispatcher {
    executor: Arc<dyn SubGoalExecutor>,
}

impl ParallelDispatcher {
    pub fn new(executor: Arc<dyn SubGoalExecutor>) -> Self {
        Self { executor }
    }
}

#[async_trait::async_trait]
impl SubGoalDispatcher for ParallelDispatcher {
    async fn dispatch(
        &self,
        plan: &DecompositionPlan,
        experiment: &Experiment,
        progress: Option<&DecompositionProgressCallback>,
    ) -> Result<Vec<SubGoalResult>, DecomposeError> {
        let mut handles = Vec::new();
        for (index, goal) in plan.sub_goals.iter().enumerate() {
            emit(
                progress,
                DecompositionEvent::SubGoalStarted {
                    index,
                    description: goal.description.clone(),
                },
            );

            let executor = self.executor.clone();
            let goal = goal.clone();
            let experiment = experiment.clone();
            handles.push(tokio::spawn(async move {
                (index, executor.execute(&goal, &experiment, &[]).await)
            }));
        }

        let mut results = vec![None; plan.sub_goals.len()];
        for handle in handles {
            let (index, result) = handle
                .await
                .map_err(|error| DecomposeError::DispatchFailed(error.to_string()))?;
            match result {
                Ok(sub_goal_result) => {
                    emit(progress, progress_event_for_result(index, &sub_goal_result));
                    results[index] = Some(sub_goal_result);
                }
                Err(error) => {
                    emit(
                        progress,
                        DecompositionEvent::SubGoalFailed {
                            index,
                            error: error.to_string(),
                        },
                    );
                    results[index] =
                        Some(failed_result(&plan.sub_goals[index], &error.to_string()));
                }
            }
        }
        Ok(results.into_iter().flatten().collect())
    }
}

/// Executes sub-goals according to a DAG specification.
pub struct DagDispatcher {
    executor: Arc<dyn SubGoalExecutor>,
}

impl DagDispatcher {
    pub fn new(executor: Arc<dyn SubGoalExecutor>) -> Self {
        Self { executor }
    }
}

#[async_trait::async_trait]
impl SubGoalDispatcher for DagDispatcher {
    async fn dispatch(
        &self,
        plan: &DecompositionPlan,
        experiment: &Experiment,
        progress: Option<&DecompositionProgressCallback>,
    ) -> Result<Vec<SubGoalResult>, DecomposeError> {
        let dag_spec = match &plan.strategy {
            AggregationStrategy::Custom(spec) => spec.clone(),
            _ => {
                return Err(DecomposeError::DispatchFailed(
                    "DagDispatcher requires Custom strategy".to_owned(),
                ));
            }
        };

        let dag = ExecutionDag::parse(&dag_spec, plan.sub_goals.len())?;
        let mut all_results = vec![None; plan.sub_goals.len()];

        for level in dag.levels() {
            let mut handles = Vec::new();
            for &index in level {
                emit(
                    progress,
                    DecompositionEvent::SubGoalStarted {
                        index,
                        description: plan.sub_goals[index].description.clone(),
                    },
                );

                let executor = self.executor.clone();
                let goal = plan.sub_goals[index].clone();
                let experiment = experiment.clone();
                handles.push(tokio::spawn(async move {
                    (index, executor.execute(&goal, &experiment, &[]).await)
                }));
            }

            for handle in handles {
                let (index, result) = handle
                    .await
                    .map_err(|error| DecomposeError::DispatchFailed(error.to_string()))?;
                match result {
                    Ok(sub_goal_result) => {
                        emit(progress, progress_event_for_result(index, &sub_goal_result));
                        all_results[index] = Some(sub_goal_result);
                    }
                    Err(error) => {
                        emit(
                            progress,
                            DecompositionEvent::SubGoalFailed {
                                index,
                                error: error.to_string(),
                            },
                        );
                        all_results[index] =
                            Some(failed_result(&plan.sub_goals[index], &error.to_string()));
                    }
                }
            }
        }

        Ok(all_results.into_iter().flatten().collect())
    }
}

/// Mock executor for testing.
#[cfg(any(test, feature = "test-support"))]
pub struct MockSubGoalExecutor {
    outcomes: Vec<SubGoalOutcome>,
}

#[cfg(any(test, feature = "test-support"))]
impl MockSubGoalExecutor {
    pub fn new(outcomes: Vec<SubGoalOutcome>) -> Self {
        Self { outcomes }
    }

    pub fn all_completed(count: usize) -> Self {
        Self {
            outcomes: (0..count)
                .map(|index| SubGoalOutcome::Completed(format!("diff --git goal-{index}")))
                .collect(),
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
#[async_trait::async_trait]
impl SubGoalExecutor for MockSubGoalExecutor {
    async fn execute(
        &self,
        goal: &SubGoal,
        _experiment: &Experiment,
        _prior_results: &[SubGoalResult],
    ) -> Result<SubGoalResult, DecomposeError> {
        let index = goal
            .description
            .chars()
            .filter(|character| character.is_ascii_digit())
            .collect::<String>()
            .parse::<usize>()
            .unwrap_or(0);
        let outcome = self
            .outcomes
            .get(index)
            .cloned()
            .unwrap_or(SubGoalOutcome::Completed("default".to_owned()));
        Ok(SubGoalResult {
            goal: goal.clone(),
            outcome,
            signals: Vec::new(),
        })
    }
}

fn skipped_result(goal: &SubGoal) -> SubGoalResult {
    SubGoalResult {
        goal: goal.clone(),
        outcome: SubGoalOutcome::Skipped,
        signals: Vec::new(),
    }
}

fn failed_result(goal: &SubGoal, error: &str) -> SubGoalResult {
    SubGoalResult {
        goal: goal.clone(),
        outcome: SubGoalOutcome::Failed(error.to_owned()),
        signals: Vec::new(),
    }
}

fn progress_event_for_result(index: usize, result: &SubGoalResult) -> DecompositionEvent {
    if sub_goal_outcome_is_terminal_failure(&result.outcome) {
        DecompositionEvent::SubGoalFailed {
            index,
            error: sub_goal_outcome_error(&result.outcome),
        }
    } else {
        DecompositionEvent::SubGoalCompleted { index }
    }
}

fn sub_goal_outcome_is_terminal_failure(outcome: &SubGoalOutcome) -> bool {
    matches!(
        outcome,
        SubGoalOutcome::Incomplete(_)
            | SubGoalOutcome::Failed(_)
            | SubGoalOutcome::BudgetExhausted { .. }
    )
}

fn sub_goal_outcome_error(outcome: &SubGoalOutcome) -> String {
    match outcome {
        SubGoalOutcome::Incomplete(message) => {
            format!("execution returned incomplete result: {message}")
        }
        SubGoalOutcome::Failed(message) => message.clone(),
        SubGoalOutcome::BudgetExhausted { partial_response } => partial_response
            .clone()
            .unwrap_or_else(|| "budget exhausted".to_owned()),
        SubGoalOutcome::Completed(_) | SubGoalOutcome::Skipped => {
            "sub-goal did not complete successfully".to_owned()
        }
    }
}

fn emit(progress: Option<&DecompositionProgressCallback>, event: DecompositionEvent) {
    if let Some(callback) = progress {
        callback(&event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ComplexityHint, SubGoalContract};

    fn sample_experiment() -> Experiment {
        Experiment {
            hypothesis: "test".to_owned(),
        }
    }

    fn plan(count: usize, strategy: AggregationStrategy) -> DecompositionPlan {
        DecompositionPlan {
            sub_goals: (0..count)
                .map(|index| {
                    SubGoal::new(
                        format!("Goal {index}"),
                        vec![],
                        SubGoalContract::default(),
                        Some(ComplexityHint::Trivial),
                    )
                })
                .collect(),
            strategy,
            truncated_from: None,
        }
    }

    #[tokio::test]
    async fn sequential_executes_in_order() {
        let executor = Arc::new(MockSubGoalExecutor::all_completed(3));
        let dispatcher = SequentialDispatcher::new(executor, false);
        let plan = plan(3, AggregationStrategy::Sequential);

        let results = dispatcher
            .dispatch(&plan, &sample_experiment(), None)
            .await
            .unwrap();

        assert_eq!(results.len(), 3);
        for result in &results {
            assert!(matches!(result.outcome, SubGoalOutcome::Completed(_)));
        }
    }

    #[tokio::test]
    async fn sequential_fail_fast_skips_after_failure() {
        let executor = Arc::new(MockSubGoalExecutor::new(vec![
            SubGoalOutcome::Completed("ok".to_owned()),
            SubGoalOutcome::Failed("boom".to_owned()),
            SubGoalOutcome::Completed("ok".to_owned()),
        ]));
        let dispatcher = SequentialDispatcher::new(executor, true);
        let plan = plan(3, AggregationStrategy::Sequential);

        let results = dispatcher
            .dispatch(&plan, &sample_experiment(), None)
            .await
            .unwrap();

        assert_eq!(results.len(), 3);
        assert!(matches!(results[0].outcome, SubGoalOutcome::Completed(_)));
        assert!(matches!(results[1].outcome, SubGoalOutcome::Failed(_)));
        assert!(matches!(results[2].outcome, SubGoalOutcome::Skipped));
    }

    #[tokio::test]
    async fn sequential_fail_fast_skips_after_budget_exhaustion() {
        let executor = Arc::new(MockSubGoalExecutor::new(vec![
            SubGoalOutcome::Completed("ok".to_owned()),
            SubGoalOutcome::BudgetExhausted {
                partial_response: Some("enough research for implementation".to_owned()),
            },
            SubGoalOutcome::Completed("ok".to_owned()),
        ]));
        let dispatcher = SequentialDispatcher::new(executor, true);
        let plan = plan(3, AggregationStrategy::Sequential);

        let results = dispatcher
            .dispatch(&plan, &sample_experiment(), None)
            .await
            .unwrap();

        assert_eq!(results.len(), 3);
        assert!(matches!(results[0].outcome, SubGoalOutcome::Completed(_)));
        assert!(matches!(
            results[1].outcome,
            SubGoalOutcome::BudgetExhausted { .. }
        ));
        assert!(matches!(results[2].outcome, SubGoalOutcome::Skipped));
    }

    #[tokio::test]
    async fn parallel_executes_all() {
        let executor = Arc::new(MockSubGoalExecutor::all_completed(3));
        let dispatcher = ParallelDispatcher::new(executor);
        let plan = plan(3, AggregationStrategy::Parallel);

        let results = dispatcher
            .dispatch(&plan, &sample_experiment(), None)
            .await
            .unwrap();

        assert_eq!(results.len(), 3);
    }

    #[tokio::test]
    async fn dag_executes_in_levels() {
        let executor = Arc::new(MockSubGoalExecutor::all_completed(4));
        let dispatcher = DagDispatcher::new(executor);
        let plan = plan(4, AggregationStrategy::Custom("0,1->2->3".to_owned()));

        let results = dispatcher
            .dispatch(&plan, &sample_experiment(), None)
            .await
            .unwrap();

        assert_eq!(results.len(), 4);
    }

    #[tokio::test]
    async fn dag_rejects_non_custom_strategy() {
        let executor = Arc::new(MockSubGoalExecutor::all_completed(2));
        let dispatcher = DagDispatcher::new(executor);
        let plan = plan(2, AggregationStrategy::Sequential);

        let error = dispatcher
            .dispatch(&plan, &sample_experiment(), None)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("Custom strategy"));
    }
}
