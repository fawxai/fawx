use crate::error::ConsensusError;
use crate::orchestrator::CandidateEvaluator;
use crate::types::{Candidate, Evaluation, Experiment, NodeId, Signal};
use async_trait::async_trait;
use chrono::Utc;
use std::collections::BTreeMap;

pub struct BuildTestEvaluator {
    node_id: NodeId,
    workspace: Box<dyn EvaluationWorkspace>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestResult {
    pub passed: u32,
    pub failed: u32,
    pub total: u32,
}

#[async_trait]
pub trait EvaluationWorkspace: Send + Sync {
    async fn begin_evaluation(&self) -> Result<(), ConsensusError> {
        Ok(())
    }

    async fn apply_patch(&self, patch: &str) -> Result<(), ConsensusError>;
    async fn build(&self) -> Result<(), ConsensusError>;
    async fn test(&self) -> Result<TestResult, ConsensusError>;
    async fn check_signal(&self, signal: &Signal) -> Result<bool, ConsensusError>;
    async fn check_regression(&self, experiment: &Experiment) -> Result<bool, ConsensusError>;
    async fn reset(&self) -> Result<(), ConsensusError>;

    async fn finish_evaluation(&self) -> Result<(), ConsensusError> {
        Ok(())
    }
}

impl BuildTestEvaluator {
    pub fn new(node_id: NodeId, workspace: Box<dyn EvaluationWorkspace>) -> Self {
        Self { node_id, workspace }
    }

    async fn evaluate_candidate(
        &self,
        experiment: &Experiment,
        candidate: &Candidate,
    ) -> Result<Evaluation, ConsensusError> {
        self.workspace.reset().await?;
        if self.workspace.apply_patch(&candidate.patch).await.is_err() {
            return Ok(failed_evaluation(candidate, self.node_id.clone()));
        }
        if self.workspace.build().await.is_err() {
            return Ok(failed_evaluation(candidate, self.node_id.clone()));
        }
        let test_result = collect_test_result(&*self.workspace).await?;
        let signal_resolved = check_signal_resolved(&*self.workspace, experiment).await;
        let regression_detected = check_regression_detected(&*self.workspace, experiment).await;
        Ok(build_evaluation(
            candidate,
            self.node_id.clone(),
            true,
            test_result,
            signal_resolved,
            regression_detected,
        ))
    }
}

#[async_trait]
impl CandidateEvaluator for BuildTestEvaluator {
    async fn evaluate(
        &self,
        experiment: &Experiment,
        candidate: &Candidate,
    ) -> Result<Evaluation, ConsensusError> {
        self.workspace.begin_evaluation().await?;
        let result = self.evaluate_candidate(experiment, candidate).await;
        complete_evaluation(&*self.workspace, result).await
    }

    fn node_id(&self) -> &NodeId {
        &self.node_id
    }
}

async fn complete_evaluation(
    workspace: &dyn EvaluationWorkspace,
    result: Result<Evaluation, ConsensusError>,
) -> Result<Evaluation, ConsensusError> {
    let finish_result = workspace.finish_evaluation().await;
    match (result, finish_result) {
        (Ok(evaluation), Ok(())) => Ok(evaluation),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Err(error), Err(_)) => Err(error),
    }
}

async fn collect_test_result(
    workspace: &dyn EvaluationWorkspace,
) -> Result<TestResult, ConsensusError> {
    match workspace.test().await {
        Ok(result) => Ok(result),
        Err(ConsensusError::TestFailed {
            passed,
            failed,
            total,
        }) => Ok(TestResult {
            passed,
            failed,
            total,
        }),
        Err(error) => Err(error),
    }
}

async fn check_signal_resolved(
    workspace: &dyn EvaluationWorkspace,
    experiment: &Experiment,
) -> bool {
    match workspace.check_signal(&experiment.trigger).await {
        Ok(signal_present) => !signal_present,
        Err(_) => false,
    }
}

async fn check_regression_detected(
    workspace: &dyn EvaluationWorkspace,
    experiment: &Experiment,
) -> bool {
    workspace.check_regression(experiment).await.unwrap_or(true)
}

fn build_evaluation(
    candidate: &Candidate,
    evaluator_id: NodeId,
    build_ok: bool,
    test_result: TestResult,
    signal_resolved: bool,
    regression_detected: bool,
) -> Evaluation {
    let safety_pass = build_ok && test_result.failed == 0 && !regression_detected;
    Evaluation {
        candidate_id: candidate.id,
        evaluator_id,
        fitness_scores: fitness_scores(build_ok, &test_result, signal_resolved),
        safety_pass,
        signal_resolved,
        regression_detected,
        notes: evaluation_notes(build_ok, &test_result, signal_resolved, regression_detected),
        created_at: Utc::now(),
    }
}

fn failed_evaluation(candidate: &Candidate, evaluator_id: NodeId) -> Evaluation {
    build_evaluation(
        candidate,
        evaluator_id,
        false,
        TestResult {
            passed: 0,
            failed: 0,
            total: 0,
        },
        false,
        false,
    )
}

fn fitness_scores(
    build_ok: bool,
    test_result: &TestResult,
    signal_resolved: bool,
) -> BTreeMap<String, f64> {
    BTreeMap::from([
        ("build_success".into(), bool_score(build_ok)),
        ("test_pass_rate".into(), pass_rate(test_result)),
        ("signal_resolution".into(), bool_score(signal_resolved)),
    ])
}

fn evaluation_notes(
    build_ok: bool,
    test_result: &TestResult,
    signal_resolved: bool,
    regression_detected: bool,
) -> String {
    format!(
        "build_ok={build_ok}; tests={}/{}, failed={}; signal_resolved={signal_resolved}; regression_detected={regression_detected}",
        test_result.passed, test_result.total, test_result.failed,
    )
}

fn pass_rate(test_result: &TestResult) -> f64 {
    if test_result.total == 0 {
        return 0.0;
    }
    test_result.passed as f64 / test_result.total as f64
}

fn bool_score(value: bool) -> f64 {
    if value {
        1.0
    } else {
        0.0
    }
}

#[cfg(test)]
mod test_support {
    use super::*;

    pub struct MockEvaluationWorkspace {
        reset_result: Result<(), ConsensusError>,
        patch_result: Result<(), ConsensusError>,
        build_result: Result<(), ConsensusError>,
        test_result: Result<TestResult, ConsensusError>,
        signal_result: Result<bool, ConsensusError>,
        regression_result: Result<bool, ConsensusError>,
    }

    impl MockEvaluationWorkspace {
        pub fn new(
            reset_result: Result<(), ConsensusError>,
            patch_result: Result<(), ConsensusError>,
            build_result: Result<(), ConsensusError>,
            test_result: Result<TestResult, ConsensusError>,
            signal_result: Result<bool, ConsensusError>,
            regression_result: Result<bool, ConsensusError>,
        ) -> Self {
            Self {
                reset_result,
                patch_result,
                build_result,
                test_result,
                signal_result,
                regression_result,
            }
        }
    }

    #[async_trait]
    impl EvaluationWorkspace for MockEvaluationWorkspace {
        async fn apply_patch(&self, _patch: &str) -> Result<(), ConsensusError> {
            self.patch_result.clone()
        }

        async fn build(&self) -> Result<(), ConsensusError> {
            self.build_result.clone()
        }

        async fn test(&self) -> Result<TestResult, ConsensusError> {
            self.test_result.clone()
        }

        async fn check_signal(&self, _signal: &Signal) -> Result<bool, ConsensusError> {
            self.signal_result.clone()
        }

        async fn check_regression(&self, _experiment: &Experiment) -> Result<bool, ConsensusError> {
            self.regression_result.clone()
        }

        async fn reset(&self) -> Result<(), ConsensusError> {
            self.reset_result.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::MockEvaluationWorkspace;
    use super::*;
    use crate::types::tests::{sample_candidate, sample_experiment};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct FinishingWorkspace {
        finish_calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl EvaluationWorkspace for FinishingWorkspace {
        async fn apply_patch(&self, _patch: &str) -> Result<(), ConsensusError> {
            Err(ConsensusError::PatchFailed("patch failed".into()))
        }

        async fn build(&self) -> Result<(), ConsensusError> {
            Ok(())
        }

        async fn test(&self) -> Result<TestResult, ConsensusError> {
            Ok(TestResult {
                passed: 1,
                failed: 0,
                total: 1,
            })
        }

        async fn check_signal(&self, _signal: &Signal) -> Result<bool, ConsensusError> {
            Ok(false)
        }

        async fn check_regression(&self, _experiment: &Experiment) -> Result<bool, ConsensusError> {
            Ok(false)
        }

        async fn reset(&self) -> Result<(), ConsensusError> {
            Ok(())
        }

        async fn finish_evaluation(&self) -> Result<(), ConsensusError> {
            self.finish_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[tokio::test]
    async fn evaluator_reports_high_fitness_when_build_tests_and_signal_all_pass() {
        let experiment = sample_experiment();
        let candidate = sample_candidate(experiment.id, "node-a");
        let evaluator = build_evaluator(MockEvaluationWorkspace::new(
            Ok(()),
            Ok(()),
            Ok(()),
            Ok(TestResult {
                passed: 4,
                failed: 0,
                total: 4,
            }),
            Ok(false),
            Ok(false),
        ));

        let evaluation = evaluator
            .evaluate(&experiment, &candidate)
            .await
            .expect("evaluate");

        assert!(evaluation.safety_pass);
        assert!(evaluation.signal_resolved);
        assert!(!evaluation.regression_detected);
        assert_eq!(evaluation.fitness_scores.get("build_success"), Some(&1.0));
        assert_eq!(evaluation.fitness_scores.get("test_pass_rate"), Some(&1.0));
        assert_eq!(
            evaluation.fitness_scores.get("signal_resolution"),
            Some(&1.0)
        );
    }

    #[tokio::test]
    async fn evaluator_marks_candidate_unsafe_when_build_fails() {
        let experiment = sample_experiment();
        let candidate = sample_candidate(experiment.id, "node-a");
        let evaluator = build_evaluator(MockEvaluationWorkspace::new(
            Ok(()),
            Ok(()),
            Err(ConsensusError::BuildFailed("compile error".into())),
            Ok(TestResult {
                passed: 4,
                failed: 0,
                total: 4,
            }),
            Ok(false),
            Ok(false),
        ));

        let evaluation = evaluator
            .evaluate(&experiment, &candidate)
            .await
            .expect("evaluate");

        assert!(!evaluation.safety_pass);
        assert!(!evaluation.signal_resolved);
        assert!(!evaluation.regression_detected);
        assert_eq!(evaluation.fitness_scores.get("build_success"), Some(&0.0));
        assert_eq!(evaluation.fitness_scores.get("test_pass_rate"), Some(&0.0));
        assert_eq!(
            evaluation.fitness_scores.get("signal_resolution"),
            Some(&0.0)
        );
    }

    #[tokio::test]
    async fn evaluator_marks_candidate_unsafe_when_tests_fail() {
        let experiment = sample_experiment();
        let candidate = sample_candidate(experiment.id, "node-a");
        let evaluator = build_evaluator(MockEvaluationWorkspace::new(
            Ok(()),
            Ok(()),
            Ok(()),
            Err(ConsensusError::TestFailed {
                passed: 2,
                failed: 1,
                total: 3,
            }),
            Ok(false),
            Ok(false),
        ));

        let evaluation = evaluator
            .evaluate(&experiment, &candidate)
            .await
            .expect("evaluate");

        assert!(!evaluation.safety_pass);
        assert_eq!(
            evaluation.fitness_scores.get("test_pass_rate"),
            Some(&(2.0 / 3.0))
        );
    }

    #[tokio::test]
    async fn evaluator_marks_signal_unresolved_when_check_reports_signal_present() {
        let experiment = sample_experiment();
        let candidate = sample_candidate(experiment.id, "node-a");
        let evaluator = build_evaluator(MockEvaluationWorkspace::new(
            Ok(()),
            Ok(()),
            Ok(()),
            Ok(TestResult {
                passed: 3,
                failed: 0,
                total: 3,
            }),
            Ok(true),
            Ok(false),
        ));

        let evaluation = evaluator
            .evaluate(&experiment, &candidate)
            .await
            .expect("evaluate");

        assert!(!evaluation.signal_resolved);
        assert_eq!(
            evaluation.fitness_scores.get("signal_resolution"),
            Some(&0.0)
        );
    }

    #[tokio::test]
    async fn evaluator_returns_unsafe_result_when_patch_application_fails() {
        let experiment = sample_experiment();
        let candidate = sample_candidate(experiment.id, "node-a");
        let evaluator = build_evaluator(MockEvaluationWorkspace::new(
            Ok(()),
            Err(ConsensusError::Protocol("patch failed".into())),
            Ok(()),
            Ok(TestResult {
                passed: 1,
                failed: 0,
                total: 1,
            }),
            Ok(false),
            Ok(false),
        ));

        let evaluation = evaluator
            .evaluate(&experiment, &candidate)
            .await
            .expect("evaluate");

        assert!(!evaluation.safety_pass);
        assert_eq!(evaluation.fitness_scores.get("build_success"), Some(&0.0));
    }

    #[tokio::test]
    async fn evaluator_finishes_workspace_after_patch_failure() {
        let experiment = sample_experiment();
        let candidate = sample_candidate(experiment.id, "node-a");
        let finish_calls = Arc::new(AtomicUsize::new(0));
        let evaluator = BuildTestEvaluator::new(
            NodeId::from("node-b"),
            Box::new(FinishingWorkspace {
                finish_calls: Arc::clone(&finish_calls),
            }),
        );

        let evaluation = evaluator
            .evaluate(&experiment, &candidate)
            .await
            .expect("evaluate");

        assert!(!evaluation.safety_pass);
        assert_eq!(finish_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn evaluator_handles_signal_check_failure_gracefully() {
        let experiment = sample_experiment();
        let candidate = sample_candidate(experiment.id, "node-a");
        let evaluator = build_evaluator(MockEvaluationWorkspace::new(
            Ok(()),
            Ok(()),
            Ok(()),
            Ok(TestResult {
                passed: 2,
                failed: 0,
                total: 2,
            }),
            Err(ConsensusError::Protocol("signal failed".into())),
            Ok(false),
        ));

        let evaluation = evaluator
            .evaluate(&experiment, &candidate)
            .await
            .expect("evaluate");

        assert!(!evaluation.signal_resolved);
        assert_eq!(
            evaluation.fitness_scores.get("signal_resolution"),
            Some(&0.0)
        );
    }

    #[tokio::test]
    async fn evaluator_fails_closed_when_regression_check_errors() {
        let experiment = sample_experiment();
        let candidate = sample_candidate(experiment.id, "node-a");
        let evaluator = build_evaluator(MockEvaluationWorkspace::new(
            Ok(()),
            Ok(()),
            Ok(()),
            Ok(TestResult {
                passed: 2,
                failed: 0,
                total: 2,
            }),
            Ok(false),
            Err(ConsensusError::Protocol("regression failed".into())),
        ));

        let evaluation = evaluator
            .evaluate(&experiment, &candidate)
            .await
            .expect("evaluate");

        assert!(evaluation.regression_detected);
        assert!(!evaluation.safety_pass);
    }

    fn build_evaluator(workspace: MockEvaluationWorkspace) -> BuildTestEvaluator {
        BuildTestEvaluator::new(NodeId::from("node-b"), Box::new(workspace))
    }
}
