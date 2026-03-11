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
    async fn apply_patch(&self, patch: &str) -> Result<(), ConsensusError>;
    async fn build(&self) -> Result<(), ConsensusError>;
    async fn test(&self) -> Result<TestResult, ConsensusError>;
    async fn check_signal(&self, signal: &Signal) -> Result<bool, ConsensusError>;
    async fn reset(&self) -> Result<(), ConsensusError>;
}

pub struct MockEvaluationWorkspace {
    reset_result: Result<(), ConsensusError>,
    patch_result: Result<(), ConsensusError>,
    build_result: Result<(), ConsensusError>,
    test_result: Result<TestResult, ConsensusError>,
    signal_present: bool,
}

impl BuildTestEvaluator {
    pub fn new(node_id: NodeId, workspace: Box<dyn EvaluationWorkspace>) -> Self {
        Self { node_id, workspace }
    }
}

impl MockEvaluationWorkspace {
    pub fn new(
        reset_result: Result<(), ConsensusError>,
        patch_result: Result<(), ConsensusError>,
        build_result: Result<(), ConsensusError>,
        test_result: Result<TestResult, ConsensusError>,
        signal_present: bool,
    ) -> Self {
        Self {
            reset_result,
            patch_result,
            build_result,
            test_result,
            signal_present,
        }
    }
}

#[async_trait]
impl CandidateEvaluator for BuildTestEvaluator {
    async fn evaluate(
        &self,
        experiment: &Experiment,
        candidate: &Candidate,
    ) -> Result<Evaluation, ConsensusError> {
        self.workspace.reset().await?;
        self.workspace.apply_patch(&candidate.patch).await?;
        let build_ok = self.workspace.build().await.is_ok();
        let test_result = collect_test_result(&*self.workspace).await?;
        let signal_present = self.workspace.check_signal(&experiment.trigger).await?;
        Ok(build_evaluation(
            candidate,
            self.node_id.clone(),
            build_ok,
            test_result,
            signal_present,
        ))
    }

    fn node_id(&self) -> &NodeId {
        &self.node_id
    }
}

#[async_trait]
impl EvaluationWorkspace for MockEvaluationWorkspace {
    async fn apply_patch(&self, _patch: &str) -> Result<(), ConsensusError> {
        clone_result(&self.patch_result)
    }

    async fn build(&self) -> Result<(), ConsensusError> {
        clone_result(&self.build_result)
    }

    async fn test(&self) -> Result<TestResult, ConsensusError> {
        clone_result(&self.test_result)
    }

    async fn check_signal(&self, _signal: &Signal) -> Result<bool, ConsensusError> {
        Ok(self.signal_present)
    }

    async fn reset(&self) -> Result<(), ConsensusError> {
        clone_result(&self.reset_result)
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

fn build_evaluation(
    candidate: &Candidate,
    evaluator_id: NodeId,
    build_ok: bool,
    test_result: TestResult,
    signal_present: bool,
) -> Evaluation {
    let signal_resolved = !signal_present;
    let safety_pass = build_ok && test_result.failed == 0;
    Evaluation {
        candidate_id: candidate.id,
        evaluator_id,
        fitness_scores: fitness_scores(build_ok, &test_result, signal_resolved),
        safety_pass,
        signal_resolved,
        regression_detected: false,
        notes: evaluation_notes(build_ok, &test_result, signal_resolved),
        created_at: Utc::now(),
    }
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

fn evaluation_notes(build_ok: bool, test_result: &TestResult, signal_resolved: bool) -> String {
    format!(
        "build_ok={build_ok}; tests={}/{}, failed={}; signal_resolved={signal_resolved}",
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

fn clone_result<T: Clone>(result: &Result<T, ConsensusError>) -> Result<T, ConsensusError> {
    result.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::tests::{sample_candidate, sample_experiment};

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
            false,
        ));

        let evaluation = evaluator
            .evaluate(&experiment, &candidate)
            .await
            .expect("evaluate");

        assert!(evaluation.safety_pass);
        assert!(evaluation.signal_resolved);
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
                passed: 0,
                failed: 0,
                total: 0,
            }),
            true,
        ));

        let evaluation = evaluator
            .evaluate(&experiment, &candidate)
            .await
            .expect("evaluate");

        assert!(!evaluation.safety_pass);
        assert!(!evaluation.signal_resolved);
        assert_eq!(evaluation.fitness_scores.get("build_success"), Some(&0.0));
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
            false,
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
            true,
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

    fn build_evaluator(workspace: MockEvaluationWorkspace) -> BuildTestEvaluator {
        BuildTestEvaluator::new(NodeId::from("node-b"), Box::new(workspace))
    }
}
