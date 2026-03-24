use chrono::Utc;
use fx_consensus::{
    compute_aggregate_scores, determine_winner, Candidate, Chain, ConsensusResult, Decision,
    Evaluation, Experiment, FitnessCriterion, MetricType, ModificationScope, NodeId, PathPattern,
    ProposalTier, Severity, Signal,
};
use std::collections::BTreeMap;
use std::time::Duration;
use uuid::Uuid;

#[test]
fn end_to_end_consensus_flow_records_chain_entry() {
    let experiment = sample_experiment();
    let candidate_a = sample_candidate(experiment.id, "node-a");
    let candidate_b = sample_candidate(experiment.id, "node-b");
    let evaluations = vec![
        sample_evaluation(candidate_a.id, "node-b", 8.0, true, false, true),
        sample_evaluation(candidate_a.id, "node-c", 6.0, true, false, true),
        sample_evaluation(candidate_b.id, "node-a", 3.0, true, false, true),
        sample_evaluation(candidate_b.id, "node-c", 4.0, false, false, true),
    ];
    let scores = compute_aggregate_scores(
        &[candidate_a.clone(), candidate_b.clone()],
        &evaluations,
        &experiment.fitness_criteria,
    );
    let (decision, winner) = determine_winner(&scores, &evaluations);
    let result = ConsensusResult {
        experiment_id: experiment.id,
        winner,
        candidates: vec![candidate_a.id, candidate_b.id],
        candidate_nodes: BTreeMap::from([
            (candidate_a.id, NodeId::from("node-a")),
            (candidate_b.id, NodeId::from("node-b")),
        ]),
        candidate_patches: BTreeMap::from([
            (candidate_a.id, candidate_a.patch.clone()),
            (candidate_b.id, candidate_b.patch.clone()),
        ]),
        evaluations,
        aggregate_scores: scores,
        decision,
        timestamp: Utc::now(),
    };
    let mut chain = Chain::new();

    chain
        .append(experiment, result, Some(candidate_a.patch.clone()), None)
        .expect("append succeeds");

    assert_eq!(
        chain.head().map(|entry| entry.result.decision.clone()),
        Some(Decision::Accept)
    );
    assert!(chain.verify().is_ok());
}

#[test]
fn records_reject_when_all_candidates_fail_safety() {
    let experiment = sample_experiment();
    let candidate = sample_candidate(experiment.id, "node-a");
    let evaluations = vec![
        sample_evaluation(candidate.id, "node-b", 5.0, true, false, false),
        sample_evaluation(candidate.id, "node-c", 4.0, true, false, true),
    ];

    let result = build_result(&experiment, vec![candidate.clone()], evaluations);
    let mut chain = Chain::new();
    chain
        .append(experiment.clone(), result, None, None)
        .expect("append succeeds");

    assert_eq!(
        chain.head().map(|entry| entry.result.decision.clone()),
        Some(Decision::Reject)
    );
}

#[test]
fn records_reject_when_signal_is_not_resolved_by_majority() {
    let experiment = sample_experiment();
    let candidate = sample_candidate(experiment.id, "node-a");
    let evaluations = vec![
        sample_evaluation(candidate.id, "node-b", 5.0, false, false, true),
        sample_evaluation(candidate.id, "node-c", 4.0, false, false, true),
        sample_evaluation(candidate.id, "node-d", 6.0, true, false, true),
    ];

    let result = build_result(&experiment, vec![candidate.clone()], evaluations);
    let mut chain = Chain::new();
    chain
        .append(experiment.clone(), result, None, None)
        .expect("append succeeds");

    assert_eq!(
        chain.head().map(|entry| entry.result.decision.clone()),
        Some(Decision::Reject)
    );
}

#[test]
fn records_reject_when_candidates_fail_for_mixed_reasons() {
    let experiment = sample_experiment();
    let candidate_a = sample_candidate(experiment.id, "node-a");
    let candidate_b = sample_candidate(experiment.id, "node-b");
    let evaluations = vec![
        sample_evaluation(candidate_a.id, "node-c", 7.0, true, true, true),
        sample_evaluation(candidate_a.id, "node-d", 6.0, true, true, true),
        sample_evaluation(candidate_b.id, "node-c", 5.0, false, false, true),
        sample_evaluation(candidate_b.id, "node-d", 4.0, false, false, true),
    ];

    let result = build_result(
        &experiment,
        vec![candidate_a.clone(), candidate_b.clone()],
        evaluations,
    );
    let mut chain = Chain::new();
    chain
        .append(experiment.clone(), result, None, None)
        .expect("append succeeds");

    assert_eq!(
        chain.head().map(|entry| entry.result.decision.clone()),
        Some(Decision::Reject)
    );
}

fn build_result(
    experiment: &Experiment,
    candidates: Vec<Candidate>,
    evaluations: Vec<Evaluation>,
) -> ConsensusResult {
    let scores = compute_aggregate_scores(&candidates, &evaluations, &experiment.fitness_criteria);
    let (decision, winner) = determine_winner(&scores, &evaluations);
    ConsensusResult {
        experiment_id: experiment.id,
        winner,
        candidates: candidates.iter().map(|candidate| candidate.id).collect(),
        candidate_nodes: candidates
            .iter()
            .map(|c| (c.id, c.node_id.clone()))
            .collect(),
        candidate_patches: candidates
            .iter()
            .map(|candidate| (candidate.id, candidate.patch.clone()))
            .collect(),
        evaluations,
        aggregate_scores: scores,
        decision,
        timestamp: Utc::now(),
    }
}

fn sample_experiment() -> Experiment {
    Experiment {
        id: Uuid::new_v4(),
        trigger: Signal {
            id: Uuid::new_v4(),
            name: "token_waste".into(),
            description: "Parallelism opportunity".into(),
            severity: Severity::Medium,
        },
        hypothesis: "parallel calls improve fitness".into(),
        fitness_criteria: vec![FitnessCriterion {
            name: "token_reduction".into(),
            metric_type: MetricType::Higher,
            weight: 1.0,
        }],
        scope: ModificationScope {
            allowed_files: vec![PathPattern::from("src/**/*.rs")],
            proposal_tier: ProposalTier::Tier1,
        },
        timeout: Duration::from_secs(60),
        min_candidates: 2,
        created_at: Utc::now(),
    }
}

fn sample_candidate(experiment_id: Uuid, node_id: &str) -> Candidate {
    Candidate {
        id: Uuid::new_v4(),
        experiment_id,
        node_id: NodeId::from(node_id),
        patch: format!("diff --git a/{node_id} b/{node_id}"),
        approach: format!("approach from {node_id}"),
        self_metrics: BTreeMap::new(),
        created_at: Utc::now(),
    }
}

fn sample_evaluation(
    candidate_id: Uuid,
    evaluator_id: &str,
    score: f64,
    resolved: bool,
    regression_detected: bool,
    safety_pass: bool,
) -> Evaluation {
    Evaluation {
        candidate_id,
        evaluator_id: NodeId::from(evaluator_id),
        fitness_scores: BTreeMap::from([("token_reduction".into(), score)]),
        safety_pass,
        signal_resolved: resolved,
        regression_detected,
        notes: "checked".into(),
        created_at: Utc::now(),
    }
}
