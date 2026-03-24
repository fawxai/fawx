use crate::types::{Candidate, Decision, Evaluation, FitnessCriterion, MetricType};
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CandidateStatus {
    Accepted,
    NotEvaluated,
    RejectedSafety,
    RejectedSignal,
    RejectedRegression,
}

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
    let candidate_statuses = collect_candidate_statuses(aggregate_scores, evaluations);
    if let Some(winner) = select_accepted_candidate(aggregate_scores, &candidate_statuses) {
        return (Decision::Accept, Some(winner));
    }
    (decision_without_winner(&candidate_statuses), None)
}

fn collect_candidate_statuses(
    aggregate_scores: &BTreeMap<Uuid, f64>,
    evaluations: &[Evaluation],
) -> Vec<(Uuid, CandidateStatus)> {
    aggregate_scores
        .keys()
        .map(|candidate_id| (*candidate_id, candidate_status(*candidate_id, evaluations)))
        .collect()
}

fn select_accepted_candidate(
    aggregate_scores: &BTreeMap<Uuid, f64>,
    candidate_statuses: &[(Uuid, CandidateStatus)],
) -> Option<Uuid> {
    aggregate_scores
        .iter()
        .filter(|(candidate_id, _)| is_accepted(**candidate_id, candidate_statuses))
        .max_by(|left, right| left.1.total_cmp(right.1))
        .map(|(candidate_id, _)| *candidate_id)
}

fn is_accepted(candidate_id: Uuid, candidate_statuses: &[(Uuid, CandidateStatus)]) -> bool {
    candidate_statuses
        .iter()
        .any(|(id, status)| *id == candidate_id && *status == CandidateStatus::Accepted)
}

fn decision_without_winner(candidate_statuses: &[(Uuid, CandidateStatus)]) -> Decision {
    if candidate_statuses.is_empty() || !any_candidate_was_evaluated(candidate_statuses) {
        return Decision::Inconclusive;
    }
    Decision::Reject
}

fn any_candidate_was_evaluated(candidate_statuses: &[(Uuid, CandidateStatus)]) -> bool {
    candidate_statuses
        .iter()
        .any(|(_, status)| *status != CandidateStatus::NotEvaluated)
}

fn candidate_status(candidate_id: Uuid, evaluations: &[Evaluation]) -> CandidateStatus {
    let candidate_evaluations: Vec<&Evaluation> = evaluations
        .iter()
        .filter(|evaluation| evaluation.candidate_id == candidate_id)
        .collect();
    evaluate_candidate(&candidate_evaluations)
}

fn evaluate_candidate(evaluations: &[&Evaluation]) -> CandidateStatus {
    if evaluations.is_empty() {
        return CandidateStatus::NotEvaluated;
    }
    if !all_safe(evaluations) {
        return CandidateStatus::RejectedSafety;
    }
    if !has_signal_majority(evaluations) {
        return CandidateStatus::RejectedSignal;
    }
    if has_regression_majority(evaluations) {
        return CandidateStatus::RejectedRegression;
    }
    CandidateStatus::Accepted
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
    majority_count(
        evaluations
            .iter()
            .filter(|evaluation| evaluation.signal_resolved)
            .count(),
        evaluations.len(),
    )
}

fn has_regression_majority(evaluations: &[&Evaluation]) -> bool {
    majority_count(
        evaluations
            .iter()
            .filter(|evaluation| evaluation.regression_detected)
            .count(),
        evaluations.len(),
    )
}

fn majority_count(matches: usize, total: usize) -> bool {
    matches * 2 > total
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

        let scores =
            compute_aggregate_scores(std::slice::from_ref(&candidate), &evaluations, &criteria);

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

        let scores = compute_aggregate_scores(
            std::slice::from_ref(&candidate),
            &[self_eval, peer_eval],
            &criteria,
        );

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

        assert_eq!(result, (Decision::Reject, None));
    }

    #[test]
    fn accepts_candidate_when_signal_resolution_has_majority_even_with_minor_regression() {
        let candidate_id = Uuid::new_v4();
        let yes_a = sample_evaluation(candidate_id, "node-b", 1.0);
        let mut yes_b = sample_evaluation(candidate_id, "node-c", 1.0);
        yes_b.regression_detected = true;
        let mut no = sample_evaluation(candidate_id, "node-d", 1.0);
        no.signal_resolved = false;
        let scores = BTreeMap::from([(candidate_id, 10.0)]);

        let result = determine_winner(&scores, &[yes_a, yes_b, no]);

        assert_eq!(result, (Decision::Accept, Some(candidate_id)));
    }

    #[test]
    fn falls_back_to_next_highest_candidate_that_meets_consensus_rules() {
        let rejected_candidate = Uuid::new_v4();
        let accepted_candidate = Uuid::new_v4();
        let mut reg_a = sample_evaluation(rejected_candidate, "node-b", 1.0);
        let mut reg_b = sample_evaluation(rejected_candidate, "node-c", 1.0);
        reg_a.regression_detected = true;
        reg_b.regression_detected = true;
        let ok_a = sample_evaluation(accepted_candidate, "node-d", 1.0);
        let ok_b = sample_evaluation(accepted_candidate, "node-e", 1.0);
        let scores = BTreeMap::from([(rejected_candidate, 10.0), (accepted_candidate, 9.0)]);

        let result = determine_winner(&scores, &[reg_a, reg_b, ok_a, ok_b]);

        assert_eq!(result, (Decision::Accept, Some(accepted_candidate)));
    }

    #[test]
    fn rejects_when_candidates_fail_different_gates() {
        let safety_rejected = Uuid::new_v4();
        let signal_rejected = Uuid::new_v4();
        let mut unsafe_eval = sample_evaluation(safety_rejected, "node-b", 1.0);
        unsafe_eval.safety_pass = false;
        let mut unresolved_a = sample_evaluation(signal_rejected, "node-c", 1.0);
        let mut unresolved_b = sample_evaluation(signal_rejected, "node-d", 1.0);
        unresolved_a.signal_resolved = false;
        unresolved_b.signal_resolved = false;
        let scores = BTreeMap::from([(safety_rejected, 10.0), (signal_rejected, 9.0)]);

        let result = determine_winner(&scores, &[unsafe_eval, unresolved_a, unresolved_b]);

        assert_eq!(result, (Decision::Reject, None));
    }

    #[test]
    fn stays_inconclusive_when_candidates_have_no_evaluations() {
        let scores = BTreeMap::from([(Uuid::new_v4(), 10.0)]);

        let result = determine_winner(&scores, &[]);

        assert_eq!(result, (Decision::Inconclusive, None));
    }
}
