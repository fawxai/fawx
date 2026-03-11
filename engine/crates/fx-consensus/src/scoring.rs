use crate::types::{Candidate, Decision, Evaluation, FitnessCriterion, MetricType};
use std::collections::BTreeMap;
use uuid::Uuid;

pub fn compute_aggregate_scores(
    candidates: &[Candidate],
    evaluations: &[Evaluation],
    criteria: &[FitnessCriterion],
) -> BTreeMap<Uuid, f64> {
    candidates
        .iter()
        .map(|candidate| {
            (
                candidate.id,
                score_candidate(candidate, evaluations, criteria),
            )
        })
        .collect()
}

pub fn determine_winner(
    aggregate_scores: &BTreeMap<Uuid, f64>,
    evaluations: &[Evaluation],
) -> (Decision, Option<Uuid>) {
    let Some((winner, _)) = aggregate_scores
        .iter()
        .max_by(|left, right| left.1.total_cmp(right.1))
    else {
        return (Decision::Inconclusive, None);
    };

    let winner = *winner;
    let candidate_evaluations: Vec<&Evaluation> = evaluations
        .iter()
        .filter(|evaluation| evaluation.candidate_id == winner)
        .collect();
    if candidate_evaluations.is_empty() || !all_safe(&candidate_evaluations) {
        return (Decision::Reject, None);
    }
    if has_signal_majority(&candidate_evaluations) {
        return (Decision::Accept, Some(winner));
    }
    (Decision::Reject, None)
}

fn score_candidate(
    candidate: &Candidate,
    evaluations: &[Evaluation],
    criteria: &[FitnessCriterion],
) -> f64 {
    criteria
        .iter()
        .map(|criterion| score_criterion(candidate, evaluations, criterion) * criterion.weight)
        .sum()
}

fn score_criterion(
    candidate: &Candidate,
    evaluations: &[Evaluation],
    criterion: &FitnessCriterion,
) -> f64 {
    let values: Vec<f64> = evaluations
        .iter()
        .filter(|evaluation| {
            evaluation.candidate_id == candidate.id && evaluation.evaluator_id != candidate.node_id
        })
        .filter_map(|evaluation| evaluation.fitness_scores.get(&criterion.name).copied())
        .collect();
    if values.is_empty() {
        return 0.0;
    }
    normalize_score(
        values.iter().sum::<f64>() / values.len() as f64,
        &criterion.metric_type,
    )
}

fn normalize_score(value: f64, metric_type: &MetricType) -> f64 {
    match metric_type {
        MetricType::Higher | MetricType::Boolean => value,
        MetricType::Lower => -value,
    }
}

fn all_safe(evaluations: &[&Evaluation]) -> bool {
    evaluations.iter().all(|evaluation| evaluation.safety_pass)
}

fn has_signal_majority(evaluations: &[&Evaluation]) -> bool {
    let resolved = evaluations
        .iter()
        .filter(|evaluation| evaluation.signal_resolved && !evaluation.regression_detected)
        .count();
    resolved * 2 > evaluations.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::tests::{sample_candidate, sample_evaluation, sample_experiment};
    use chrono::Utc;
    use std::collections::BTreeMap;

    #[test]
    fn computes_weighted_average_scores() {
        let experiment = sample_experiment();
        let candidate = sample_candidate(experiment.id, "node-a");
        let evaluations = vec![
            sample_evaluation(candidate.id, "node-b", 2.0),
            sample_evaluation(candidate.id, "node-c", 4.0),
        ];
        let criteria = vec![FitnessCriterion {
            name: "latency".into(),
            metric_type: MetricType::Higher,
            weight: 0.5,
        }];

        let scores = compute_aggregate_scores(&[candidate.clone()], &evaluations, &criteria);

        assert_eq!(scores.get(&candidate.id), Some(&1.5));
    }

    #[test]
    fn excludes_self_evaluations() {
        let experiment = sample_experiment();
        let candidate = sample_candidate(experiment.id, "node-a");
        let mut self_eval = sample_evaluation(candidate.id, "node-a", 100.0);
        self_eval.created_at = Utc::now();
        let peer_eval = sample_evaluation(candidate.id, "node-b", 4.0);
        let criteria = experiment.fitness_criteria.clone();

        let scores =
            compute_aggregate_scores(&[candidate.clone()], &[self_eval, peer_eval], &criteria);

        assert_eq!(scores.get(&candidate.id), Some(&-4.0));
    }

    #[test]
    fn rejects_candidate_when_safety_is_not_unanimous() {
        let candidate_id = Uuid::new_v4();
        let mut failing = sample_evaluation(candidate_id, "node-b", 1.0);
        failing.safety_pass = false;
        let passing = sample_evaluation(candidate_id, "node-c", 1.0);
        let scores = BTreeMap::from([(candidate_id, 10.0)]);

        let result = determine_winner(&scores, &[failing, passing]);

        assert!(matches!(result, (Decision::Reject, None)));
    }

    #[test]
    fn accepts_candidate_when_signal_resolution_has_majority() {
        let candidate_id = Uuid::new_v4();
        let yes_a = sample_evaluation(candidate_id, "node-b", 1.0);
        let yes_b = sample_evaluation(candidate_id, "node-c", 1.0);
        let mut no = sample_evaluation(candidate_id, "node-d", 1.0);
        no.signal_resolved = false;
        let scores = BTreeMap::from([(candidate_id, 10.0)]);

        let result = determine_winner(&scores, &[yes_a, yes_b, no]);

        assert!(matches!(result, (Decision::Accept, Some(id)) if id == candidate_id));
    }
}
