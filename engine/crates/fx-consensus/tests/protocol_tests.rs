use async_trait::async_trait;
use chrono::Utc;
use fx_consensus::{
    Candidate, CandidateEvaluator, CandidateGenerator, ConsensusError, ConsensusProtocol, Decision,
    Evaluation, Experiment, ExperimentConfig, ExperimentOrchestrator, FitnessCriterion,
    JsonFileChainStorage, LocalConsensusEngine, MetricType, ModificationScope, NodeId, PathPattern,
    ProposalTier, Severity, Signal,
};
use std::collections::BTreeMap;
use std::time::Duration;
use tempfile::tempdir;
use uuid::Uuid;

#[tokio::test]
async fn creates_experiment_and_keeps_it_retrievable() {
    let engine = create_engine();
    let config = sample_config(1);

    let experiment = engine
        .create_experiment(config)
        .await
        .expect("create works");
    let candidates = engine
        .candidates(experiment.id)
        .await
        .expect("candidate lookup works");

    assert!(candidates.is_empty());
}

#[tokio::test]
async fn stores_submitted_candidate_under_experiment() {
    let engine = create_engine();
    let experiment = engine
        .create_experiment(sample_config(1))
        .await
        .expect("create works");
    let candidate = sample_candidate(experiment.id, "node-a", "candidate-a");

    engine
        .submit_candidate(candidate.clone())
        .await
        .expect("submit works");

    let stored = engine
        .candidates(experiment.id)
        .await
        .expect("candidate lookup works");
    assert_eq!(stored, vec![candidate]);
}

#[tokio::test]
async fn stores_submitted_evaluation() {
    let engine = create_engine();
    let experiment = engine
        .create_experiment(sample_config(1))
        .await
        .expect("create works");
    let candidate = sample_candidate(experiment.id, "node-a", "candidate-a");
    let evaluation = sample_evaluation(candidate.id, "node-b", 9.0, true, false, true);

    engine
        .submit_candidate(candidate)
        .await
        .expect("submit works");
    engine
        .submit_evaluation(evaluation.clone())
        .await
        .expect("evaluation works");

    let result = engine
        .finalize(experiment.id)
        .await
        .expect("finalize works");
    assert_eq!(result.evaluations, vec![evaluation]);
}

#[tokio::test]
async fn finalizes_with_accept_and_correct_winner() {
    let engine = create_engine();
    let experiment = engine
        .create_experiment(sample_config(2))
        .await
        .expect("create works");
    let candidate_a = sample_candidate(experiment.id, "node-a", "candidate-a");
    let candidate_b = sample_candidate(experiment.id, "node-b", "candidate-b");
    submit_candidates(&engine, &[candidate_a.clone(), candidate_b.clone()]).await;
    submit_evaluations(
        &engine,
        &[
            sample_evaluation(candidate_a.id, "node-b", 9.0, true, false, true),
            sample_evaluation(candidate_a.id, "node-c", 8.0, true, false, true),
            sample_evaluation(candidate_b.id, "node-a", 4.0, true, false, true),
            sample_evaluation(candidate_b.id, "node-c", 3.0, true, false, true),
        ],
    )
    .await;

    let result = engine
        .finalize(experiment.id)
        .await
        .expect("finalize works");

    assert_eq!(result.decision, Decision::Accept);
    assert_eq!(result.winner, Some(candidate_a.id));
    assert_eq!(engine.chain().len(), 1);
}

#[tokio::test]
async fn finalizes_with_rejection_when_all_candidates_fail_safety() {
    let engine = create_engine();
    let experiment = engine
        .create_experiment(sample_config(2))
        .await
        .expect("create works");
    let candidate_a = sample_candidate(experiment.id, "node-a", "candidate-a");
    let candidate_b = sample_candidate(experiment.id, "node-b", "candidate-b");
    submit_candidates(&engine, &[candidate_a.clone(), candidate_b.clone()]).await;
    submit_evaluations(
        &engine,
        &[
            sample_evaluation(candidate_a.id, "node-c", 8.0, true, false, false),
            sample_evaluation(candidate_b.id, "node-c", 7.0, true, false, false),
        ],
    )
    .await;

    let result = engine
        .finalize(experiment.id)
        .await
        .expect("finalize works");

    assert_eq!(result.decision, Decision::Reject);
    assert_eq!(result.winner, None);
}

#[tokio::test]
async fn finalizing_with_too_few_candidates_returns_error() {
    let engine = create_engine();
    let experiment = engine
        .create_experiment(sample_config(2))
        .await
        .expect("create works");
    let candidate = sample_candidate(experiment.id, "node-a", "candidate-a");

    engine
        .submit_candidate(candidate)
        .await
        .expect("submit works");

    let error = engine
        .finalize(experiment.id)
        .await
        .expect_err("should fail");
    assert!(matches!(
        error,
        ConsensusError::InsufficientCandidates {
            required: 2,
            received: 1
        }
    ));
}

#[tokio::test]
async fn double_finalize_returns_already_finalized_error() {
    let engine = create_engine();
    let experiment = engine
        .create_experiment(sample_config(1))
        .await
        .expect("create works");
    let candidate = sample_candidate(experiment.id, "node-a", "candidate-a");
    engine
        .submit_candidate(candidate.clone())
        .await
        .expect("submit works");
    engine
        .submit_evaluation(sample_evaluation(
            candidate.id,
            "node-b",
            9.0,
            true,
            false,
            true,
        ))
        .await
        .expect("evaluation works");

    engine
        .finalize(experiment.id)
        .await
        .expect("first finalize works");
    let error = engine
        .finalize(experiment.id)
        .await
        .expect_err("should fail");

    assert!(matches!(
        error,
        ConsensusError::ExperimentAlreadyFinalized(id) if id == experiment.id
    ));
}

#[tokio::test]
async fn orchestrator_runs_full_experiment_end_to_end() {
    let engine = create_engine();
    let expected_winner = Uuid::new_v4();
    let orchestrator = ExperimentOrchestrator::new(engine);
    let generators: Vec<Box<dyn CandidateGenerator>> = vec![
        Box::new(MockGenerator::new(
            "node-a",
            expected_winner,
            "winner",
            10.0,
        )),
        Box::new(MockGenerator::new(
            "node-b",
            Uuid::new_v4(),
            "runner-up",
            4.0,
        )),
    ];
    let evaluators: Vec<Box<dyn CandidateEvaluator>> = vec![
        Box::new(MockEvaluator::new("node-a")),
        Box::new(MockEvaluator::new("node-b")),
        Box::new(MockEvaluator::new("node-c")),
    ];

    let result = orchestrator
        .run_experiment(sample_config(2), generators, evaluators)
        .await
        .expect("orchestration works");

    assert_eq!(result.decision, Decision::Accept);
    assert_eq!(result.winner, Some(expected_winner));
    assert_eq!(result.candidates.len(), 2);
}

fn create_engine() -> LocalConsensusEngine {
    let path = tempdir().expect("create tempdir").keep().join("chain.json");
    LocalConsensusEngine::new(Box::new(JsonFileChainStorage::new(path))).expect("engine creates")
}

async fn submit_candidates(engine: &LocalConsensusEngine, candidates: &[Candidate]) {
    for candidate in candidates {
        engine
            .submit_candidate(candidate.clone())
            .await
            .expect("submit works");
    }
}

async fn submit_evaluations(engine: &LocalConsensusEngine, evaluations: &[Evaluation]) {
    for evaluation in evaluations {
        engine
            .submit_evaluation(evaluation.clone())
            .await
            .expect("submit evaluation works");
    }
}

fn sample_config(min_candidates: u32) -> ExperimentConfig {
    ExperimentConfig {
        signal: Signal {
            id: Uuid::new_v4(),
            name: "token_waste".into(),
            description: "Parallel work exists".into(),
            severity: Severity::Medium,
        },
        hypothesis: "parallel candidates improve outcomes".into(),
        fitness_criteria: vec![FitnessCriterion {
            name: "fitness".into(),
            metric_type: MetricType::Higher,
            weight: 1.0,
        }],
        scope: ModificationScope {
            allowed_files: vec![PathPattern::from("src/**/*.rs")],
            proposal_tier: ProposalTier::Tier1,
        },
        timeout: Duration::from_secs(30),
        min_candidates,
    }
}

fn sample_candidate(experiment_id: Uuid, node_id: &str, approach: &str) -> Candidate {
    Candidate {
        id: Uuid::new_v4(),
        experiment_id,
        node_id: NodeId::from(node_id),
        patch: format!("diff --git a/{approach} b/{approach}"),
        approach: approach.into(),
        self_metrics: BTreeMap::new(),
        created_at: Utc::now(),
    }
}

fn sample_evaluation(
    candidate_id: Uuid,
    evaluator_id: &str,
    score: f64,
    resolved: bool,
    regression: bool,
    safety_pass: bool,
) -> Evaluation {
    Evaluation {
        candidate_id,
        evaluator_id: NodeId::from(evaluator_id),
        fitness_scores: BTreeMap::from([("fitness".into(), score)]),
        safety_pass,
        signal_resolved: resolved,
        regression_detected: regression,
        notes: "checked".into(),
        created_at: Utc::now(),
    }
}

struct MockGenerator {
    node_id: NodeId,
    candidate_id: Uuid,
    approach: String,
    score: f64,
}

impl MockGenerator {
    fn new(node_id: &str, candidate_id: Uuid, approach: &str, score: f64) -> Self {
        Self {
            node_id: NodeId::from(node_id),
            candidate_id,
            approach: approach.into(),
            score,
        }
    }
}

#[async_trait]
impl CandidateGenerator for MockGenerator {
    async fn generate(&self, experiment: &Experiment) -> Result<Candidate, ConsensusError> {
        Ok(Candidate {
            id: self.candidate_id,
            experiment_id: experiment.id,
            node_id: self.node_id.clone(),
            patch: format!("diff --git a/{} b/{}", self.approach, self.approach),
            approach: self.approach.clone(),
            self_metrics: BTreeMap::from([("fitness".into(), self.score)]),
            created_at: Utc::now(),
        })
    }

    fn node_id(&self) -> &NodeId {
        &self.node_id
    }
}

struct MockEvaluator {
    node_id: NodeId,
}

impl MockEvaluator {
    fn new(node_id: &str) -> Self {
        Self {
            node_id: NodeId::from(node_id),
        }
    }
}

#[async_trait]
impl CandidateEvaluator for MockEvaluator {
    async fn evaluate(
        &self,
        _experiment: &Experiment,
        candidate: &Candidate,
    ) -> Result<Evaluation, ConsensusError> {
        let score = candidate
            .self_metrics
            .get("fitness")
            .copied()
            .expect("mock candidate has fitness score");
        Ok(Evaluation {
            candidate_id: candidate.id,
            evaluator_id: self.node_id.clone(),
            fitness_scores: BTreeMap::from([("fitness".into(), score)]),
            safety_pass: score >= 5.0,
            signal_resolved: score >= 5.0,
            regression_detected: false,
            notes: "mock eval".into(),
            created_at: Utc::now(),
        })
    }

    fn node_id(&self) -> &NodeId {
        &self.node_id
    }
}
