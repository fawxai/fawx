use super::RunExperimentArgs;
use crate::commands::experiment::placeholders::strategy_for;
use fx_consensus::{
    display_strategy, evaluation_build_success, note_value, Chain, ChainEntry, ExperimentReport,
    GenerationStrategy, ProgressEvent,
};
use uuid::Uuid;

const PATCH_PREVIEW_LINES: usize = 20;

pub(super) fn format_experiment_report(
    args: &RunExperimentArgs,
    report: &ExperimentReport,
) -> String {
    let mut lines = vec![
        "═══ Experiment Complete ═══".to_owned(),
        String::new(),
        format!("  Experiment ID: {}", report.result.experiment_id),
        format!("  Signal:        {}", args.signal),
        format!("  Hypothesis:    {}", args.hypothesis),
        format!("  Decision:      {}", report.result.decision.emoji_label()),
        format!("  Candidates:    {}", report.candidates.len()),
        String::new(),
        "  Candidates:".to_owned(),
    ];
    lines.extend(report.candidates.iter().map(format_candidate_line));
    lines.extend([
        String::new(),
        format!("  Chain entry #{} recorded", report.chain_entry_index),
        "  Verify: fawx experiment verify".to_owned(),
    ]);
    lines.join("\n")
}

pub(super) fn render_progress_event(event: &ProgressEvent) {
    eprintln!("{}", fx_consensus::format_progress_event(event));
}

fn format_candidate_line(candidate: &fx_consensus::CandidateReport) -> String {
    let prefix = if candidate.is_winner {
        "    🏆"
    } else {
        "      "
    };
    let winner_suffix = if candidate.is_winner {
        "  ← WINNER"
    } else {
        ""
    };
    format!(
        "{prefix} {} ({})  score: {:.2}{winner_suffix}",
        candidate.node_id.0,
        display_strategy(&candidate.strategy),
        candidate.aggregate_score,
    )
}

pub(super) fn format_chain_entries(chain: &Chain, limit: usize) -> String {
    let mut lines = vec!["Recent experiments:".to_owned()];
    lines.extend(
        chain
            .entries()
            .iter()
            .rev()
            .take(limit)
            .map(format_chain_summary_line),
    );
    lines.join("\n")
}

fn format_chain_summary_line(entry: &ChainEntry) -> String {
    format!(
        "#{} | {} | {} | winner: {} | {}",
        entry.index,
        entry.experiment.hypothesis,
        entry.result.decision.emoji_label(),
        winner_node_label(entry),
        entry.result.timestamp.to_rfc3339(),
    )
}

fn winner_node_label(entry: &ChainEntry) -> String {
    entry
        .result
        .winner
        .and_then(|winner| entry.result.candidate_nodes.get(&winner))
        .map(|node_id| node_id.0.clone())
        .unwrap_or_else(|| "none".to_owned())
}

pub(super) fn format_chain_entry(entry: &ChainEntry) -> String {
    let winner = winner_node_label(entry);
    [
        format!("Chain entry #{}", entry.index),
        format!("Experiment ID: {}", entry.experiment.id),
        format!("Signal: {}", entry.experiment.trigger.name),
        format!("Hypothesis: {}", entry.experiment.hypothesis),
        format!("Decision: {}", entry.result.decision.emoji_label()),
        format!("Winner: {}", winner),
        format!(
            "Winning patch: {}",
            entry.winning_patch.as_deref().unwrap_or("<none>")
        ),
        "Scores:".to_owned(),
        format_score_lines(entry),
        format!("Evaluations: {} total", entry.result.evaluations.len()),
        format!("Recorded at: {}", entry.result.timestamp.to_rfc3339()),
    ]
    .join("\n")
}

pub(super) fn format_chain_entry_detail(entry: &ChainEntry) -> String {
    let mut lines = vec![
        format!("Chain entry #{}", entry.index),
        format!("Experiment ID: {}", entry.experiment.id),
        format!("Signal: {}", entry.experiment.trigger.name),
        format!("Hypothesis: {}", entry.experiment.hypothesis),
        format!("Scope: {}", format_scope(entry)),
        format!("Timeout: {}s", entry.experiment.timeout.as_secs()),
        format!("Created: {}", entry.experiment.created_at.to_rfc3339()),
        String::new(),
        "Candidates:".to_owned(),
    ];
    lines.extend(format_detail_candidates(entry));
    lines.push(String::new());
    lines.push("Evaluations:".to_owned());
    lines.extend(format_detail_evaluations(entry));
    lines.extend([
        String::new(),
        format!("Decision: {}", entry.result.decision.emoji_label()),
        format!("Winner: {}", winner_node_label(entry)),
        format!("Chain hash: {}", entry.hash),
    ]);
    lines.join("\n")
}

fn format_scope(entry: &ChainEntry) -> String {
    let files = entry
        .experiment
        .scope
        .allowed_files
        .iter()
        .map(|pattern| pattern.0.as_str())
        .collect::<Vec<_>>();
    if files.is_empty() {
        "(none)".to_owned()
    } else {
        files.join(", ")
    }
}

fn format_detail_candidates(entry: &ChainEntry) -> Vec<String> {
    let mut lines = scored_candidates(entry)
        .into_iter()
        .flat_map(|(candidate_id, score)| format_detail_candidate(entry, &candidate_id, score))
        .collect::<Vec<_>>();
    if lines.is_empty() {
        lines.push("  (none)".to_owned());
    }
    lines
}

fn format_detail_candidate(entry: &ChainEntry, candidate_id: &Uuid, score: f64) -> Vec<String> {
    let node_id = entry
        .result
        .candidate_nodes
        .get(candidate_id)
        .map(|node| node.0.as_str())
        .unwrap_or("unknown");
    let strategy = infer_strategy(node_id)
        .map(|value| display_strategy(&value).to_string())
        .unwrap_or_else(|| "Unknown".to_owned());
    vec![
        format!("  {node_id} ({strategy})"),
        format!("    ID: {candidate_id}"),
        format!("    Score: {score:.2}"),
        "    Approach: (not stored in chain entry)".to_owned(),
        "    Patch:".to_owned(),
        format_patch_preview(entry, candidate_id),
    ]
}

fn format_patch_preview(entry: &ChainEntry, candidate_id: &Uuid) -> String {
    if entry.result.winner == Some(*candidate_id) {
        return indented_patch(entry.winning_patch.as_deref().unwrap_or("(empty)"));
    }
    "      (not stored in chain entry)".to_owned()
}

fn indented_patch(patch: &str) -> String {
    let trimmed = patch.trim_end();
    if trimmed.is_empty() {
        return "      (empty)".to_owned();
    }
    let mut lines = trimmed
        .lines()
        .take(PATCH_PREVIEW_LINES)
        .map(|line| format!("      {line}"))
        .collect::<Vec<_>>();
    if trimmed.lines().count() > PATCH_PREVIEW_LINES {
        lines.push(format!(
            "      ... (truncated to {PATCH_PREVIEW_LINES} lines)"
        ));
    }
    lines.join("\n")
}

fn format_detail_evaluations(entry: &ChainEntry) -> Vec<String> {
    let mut lines = entry
        .result
        .evaluations
        .iter()
        .enumerate()
        .flat_map(|(index, evaluation)| format_single_evaluation(entry, index, evaluation))
        .collect::<Vec<_>>();
    if lines.is_empty() {
        lines.push("  (none)".to_owned());
    }
    lines
}

fn format_single_evaluation(
    entry: &ChainEntry,
    index: usize,
    evaluation: &fx_consensus::Evaluation,
) -> Vec<String> {
    let candidate = entry
        .result
        .candidate_nodes
        .get(&evaluation.candidate_id)
        .map(|node| node.0.as_str())
        .unwrap_or("unknown");
    let build_status = if evaluation_build_success(evaluation) {
        "✅ PASSED"
    } else {
        "❌ FAILED"
    };
    let mut block = vec![
        format!("  [{}] Evaluator: {}", index + 1, evaluation.evaluator_id.0),
        format!("      Candidate: {candidate}"),
        format!("      Build: {build_status}"),
        format!(
            "      Tests: {} passed / {} failed / {} total",
            tests_passed(&evaluation.notes),
            tests_failed(&evaluation.notes),
            tests_total(&evaluation.notes),
        ),
        format!(
            "      Signal resolved: {}",
            yes_no(evaluation.signal_resolved)
        ),
        format!(
            "      Regression detected: {}",
            yes_no(evaluation.regression_detected)
        ),
        format!("      Safety pass: {}", yes_no(evaluation.safety_pass)),
        "      Fitness scores:".to_owned(),
    ];
    block.extend(format_fitness_scores(entry, evaluation));
    block.push(format!("      Notes: {}", evaluation.notes));
    block
}

fn format_fitness_scores(entry: &ChainEntry, evaluation: &fx_consensus::Evaluation) -> Vec<String> {
    let mut lines = entry
        .experiment
        .fitness_criteria
        .iter()
        .map(|criterion| {
            let score = evaluation
                .fitness_scores
                .get(&criterion.name)
                .copied()
                .unwrap_or(0.0);
            format!(
                "        {}: {:.2} (weight: {:.2})",
                criterion.name, score, criterion.weight
            )
        })
        .collect::<Vec<_>>();
    for (name, score) in &evaluation.fitness_scores {
        if entry
            .experiment
            .fitness_criteria
            .iter()
            .all(|criterion| criterion.name != *name)
        {
            lines.push(format!("        {name}: {score:.2} (weight: n/a)"));
        }
    }
    lines
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn tests_passed(notes: &str) -> u64 {
    note_number(notes, "tests").unwrap_or(0)
}

fn tests_total(notes: &str) -> u64 {
    note_denominator(notes, "tests").unwrap_or(0)
}

fn tests_failed(notes: &str) -> u64 {
    note_number(notes, "failed").unwrap_or(0)
}

#[cfg(test)]
fn note_flag(notes: &str, key: &str) -> Option<bool> {
    note_value(notes, key).and_then(|value| match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    })
}

fn note_number(notes: &str, key: &str) -> Option<u64> {
    note_value(notes, key)?.parse().ok()
}

fn note_denominator(notes: &str, key: &str) -> Option<u64> {
    let value = note_value(notes, key)?;
    let (_, denominator) = value.split_once('/')?;
    denominator.parse().ok()
}

fn infer_strategy(node_id: &str) -> Option<GenerationStrategy> {
    let index = node_id.strip_prefix("node-")?.parse().ok()?;
    Some(strategy_for(index))
}

fn format_score_lines(entry: &ChainEntry) -> String {
    let mut lines = scored_candidates(entry)
        .into_iter()
        .map(|(candidate_id, score)| format_score_line(entry, &candidate_id, score))
        .collect::<Vec<_>>();
    if lines.is_empty() {
        lines.push("  - none".to_owned());
    }
    lines.join("\n")
}

fn scored_candidates(entry: &ChainEntry) -> Vec<(Uuid, f64)> {
    entry
        .result
        .aggregate_scores
        .iter()
        .map(|(candidate_id, score)| (*candidate_id, *score))
        .collect()
}

fn format_score_line(entry: &ChainEntry, candidate_id: &Uuid, score: f64) -> String {
    let node_label = entry
        .result
        .candidate_nodes
        .get(candidate_id)
        .map(|node_id| node_id.0.as_str())
        .unwrap_or("unknown");
    let winner_suffix = if entry.result.winner == Some(*candidate_id) {
        "  ← WINNER"
    } else {
        ""
    };
    format!("  - {node_label}: {score:.2}{winner_suffix}")
}

#[cfg(test)]
mod tests {
    use super::{note_denominator, note_flag, note_number};
    use fx_consensus::note_value;

    #[test]
    fn note_value_handles_missing_key_and_empty_notes() {
        assert_eq!(note_value("tests=3/4", "failed"), None);
        assert_eq!(note_value("", "tests"), None);
    }

    #[test]
    fn note_value_skips_empty_segments_between_semicolons() {
        assert_eq!(
            note_value("; ; tests=3/4 ;; build_ok=true ;", "tests"),
            Some("3/4")
        );
    }

    #[test]
    fn note_number_returns_none_for_missing_key_empty_notes_and_malformed_values() {
        assert_eq!(note_number("tests=3/4", "failed"), None);
        assert_eq!(note_number("", "tests"), None);
        assert_eq!(note_number("failed=abc", "failed"), None);
    }

    #[test]
    fn note_number_parses_value_with_multiple_semicolons() {
        assert_eq!(
            note_number(";; failed=2 ;; tests=3/4 ;;", "failed"),
            Some(2)
        );
    }

    #[test]
    fn note_denominator_returns_none_for_missing_key_empty_notes_and_malformed_values() {
        assert_eq!(note_denominator("failed=1", "tests"), None);
        assert_eq!(note_denominator("", "tests"), None);
        assert_eq!(note_denominator("tests=3/def", "tests"), None);
        assert_eq!(note_denominator("tests=3", "tests"), None);
    }

    #[test]
    fn note_denominator_parses_value_with_multiple_semicolons() {
        assert_eq!(
            note_denominator(";; tests=3/4 ;; failed=1 ;;", "tests"),
            Some(4)
        );
    }

    #[test]
    fn note_flag_returns_none_for_missing_key_empty_notes_and_invalid_values() {
        assert_eq!(note_flag("tests=3/4", "build_ok"), None);
        assert_eq!(note_flag("", "build_ok"), None);
        assert_eq!(note_flag("build_ok=maybe", "build_ok"), None);
    }

    #[test]
    fn note_flag_parses_value_with_multiple_semicolons() {
        assert_eq!(
            note_flag(";; build_ok=true ;; tests=3/4 ;;", "build_ok"),
            Some(true)
        );
    }
}
