use chrono::Utc;
use fx_consensus::{
    compute_aggregate_scores, determine_winner, Candidate, Chain, ConsensusResult, Decision,
    Evaluation, Experiment, FitnessCriterion, MetricType, ModificationScope, ProposalTier, Signal,
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
        sample_evaluation(candidate_a.id, "node-b", 8.0, true),
        sample_evaluation(candidate_a.id, "node-c", 6.0, true),
        sample_evaluation(candidate_b.id, "node-a", 3.0, true),
        sample_evaluation(candidate_b.id, "node-c", 4.0, false),
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
        evaluations,
        aggregate_scores: scores,
        decision,
        timestamp: Utc::now(),
    };
    let mut chain = Chain::new();

    chain
        .append(experiment, result, Some(candidate_a.patch.clone()))
        .expect("append succeeds");

    assert!(matches!(
        chain.head().map(|entry| &entry.result.decision),
        Some(Decision::Accept)
    ));
    assert!(chain.verify().is_ok());
}

fn sample_experiment() -> Experiment {
    Experiment {
        id: Uuid::new_v4(),
        trigger: Signal {
            id: Uuid::new_v4(),
            name: "token_waste".into(),
            description: "Parallelism opportunity".into(),
            severity: "medium".into(),
        },
        hypothesis: "parallel calls improve fitness".into(),
        fitness_criteria: vec![FitnessCriterion {
            name: "token_reduction".into(),
            metric_type: MetricType::Higher,
            weight: 1.0,
        }],
        scope: ModificationScope {
            allowed_files: vec!["src/**/*.rs".into()],
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
        node_id: node_id.into(),
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
) -> Evaluation {
    Evaluation {
        candidate_id,
        evaluator_id: evaluator_id.into(),
        fitness_scores: BTreeMap::from([("token_reduction".into(), score)]),
        safety_pass: true,
        signal_resolved: resolved,
        regression_detected: false,
        notes: "checked".into(),
        created_at: Utc::now(),
    }
}
