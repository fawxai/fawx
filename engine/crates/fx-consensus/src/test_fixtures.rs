use crate::{
    Chain, ChainStorage, ConsensusResult, Decision, Experiment, FitnessCriterion,
    JsonFileChainStorage, MetricType, ModificationScope, NodeId, PathPattern, ProposalTier,
    Severity, Signal,
};
use chrono::Utc;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::Duration;
use uuid::Uuid;

pub fn write_chain_with_signals<const N: usize>(path: &Path, entries: [(&str, &str, &str); N]) {
    let mut chain = Chain::new();
    for (index, entry) in entries.into_iter().enumerate() {
        append_entry(&mut chain, index, entry);
    }
    save_chain(path, &chain);
}

fn append_entry(chain: &mut Chain, index: usize, entry: (&str, &str, &str)) {
    let (signal_name, hypothesis, notes) = entry;
    let candidate_id = Uuid::from_u128(index as u128 + 1);
    let experiment_id = Uuid::from_u128(index as u128 + 100);
    let timestamp = Utc::now();
    let experiment = build_experiment(signal_name, hypothesis, experiment_id, index, timestamp);
    let result = build_result(candidate_id, experiment_id, notes, timestamp);
    chain
        .append(experiment, result, Some("diff --git".to_owned()), None)
        .expect("append chain entry");
}

fn build_experiment(
    signal_name: &str,
    hypothesis: &str,
    experiment_id: Uuid,
    index: usize,
    timestamp: chrono::DateTime<Utc>,
) -> Experiment {
    Experiment {
        id: experiment_id,
        trigger: Signal {
            id: Uuid::from_u128(index as u128 + 200),
            name: signal_name.to_owned(),
            description: "signal".to_owned(),
            severity: Severity::Medium,
        },
        hypothesis: hypothesis.to_owned(),
        fitness_criteria: default_fitness_criteria(),
        scope: build_scope(),
        timeout: Duration::from_secs(120),
        min_candidates: 1,
        created_at: timestamp,
    }
}

fn build_result(
    candidate_id: Uuid,
    experiment_id: Uuid,
    notes: &str,
    timestamp: chrono::DateTime<Utc>,
) -> ConsensusResult {
    ConsensusResult {
        experiment_id,
        winner: Some(candidate_id),
        candidates: vec![candidate_id],
        candidate_nodes: BTreeMap::from([(candidate_id, NodeId("node-0".to_owned()))]),
        candidate_patches: BTreeMap::new(),
        evaluations: vec![crate::Evaluation {
            candidate_id,
            evaluator_id: NodeId("node-1".to_owned()),
            fitness_scores: BTreeMap::from([("build_success".to_owned(), 1.0)]),
            safety_pass: true,
            signal_resolved: true,
            regression_detected: false,
            notes: notes.to_owned(),
            created_at: timestamp,
        }],
        aggregate_scores: BTreeMap::from([(candidate_id, 8.73)]),
        decision: Decision::Accept,
        timestamp,
    }
}

fn save_chain(path: &Path, chain: &Chain) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create chain dir");
    }
    JsonFileChainStorage::new(path)
        .save(chain)
        .expect("save chain");
}

fn default_fitness_criteria() -> Vec<FitnessCriterion> {
    vec![
        criterion("build_success", MetricType::Higher, 0.2),
        criterion("test_pass_rate", MetricType::Higher, 0.5),
        criterion("signal_resolution", MetricType::Higher, 0.3),
    ]
}

fn criterion(name: &str, metric_type: MetricType, weight: f64) -> FitnessCriterion {
    FitnessCriterion {
        name: name.to_owned(),
        metric_type,
        weight,
    }
}

fn build_scope() -> ModificationScope {
    ModificationScope {
        allowed_files: vec![PathPattern::from("src/**/*.rs")],
        proposal_tier: ProposalTier::Tier1,
    }
}
