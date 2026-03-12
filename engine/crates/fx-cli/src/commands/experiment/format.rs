use super::RunExperimentArgs;
use crate::commands::experiment::placeholders::{display_strategy, strategy_for};
use fx_consensus::{
    evaluation_build_success, note_value, plural_suffix, BuildOutcome, BuildSummary, Chain,
    ChainEntry, Decision, ExperimentReport, GenerationStrategy, ProgressEvent,
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
    eprintln!("{}", format_progress_event(event));
}

fn format_progress_event(event: &ProgressEvent) -> String {
    match event {
        ProgressEvent::RoundStarted {
            round,
            max_rounds,
            signal,
        } => progress_line("▸", *round, *max_rounds, format!("started signal {signal}")),
        ProgressEvent::BaselineCollected {
            round,
            max_rounds,
            node_count,
        } => progress_line(
            "▸",
            *round,
            *max_rounds,
            format!(
                "baseline collected for {node_count} node{}",
                plural_suffix(*node_count as u64)
            ),
        ),
        ProgressEvent::NodeStarted {
            round,
            max_rounds,
            node_id,
            strategy,
        } => progress_line(
            "▸",
            *round,
            *max_rounds,
            format!(
                "{} ({}) generating patch",
                node_id.0,
                display_strategy(strategy)
            ),
        ),
        ProgressEvent::PatchGenerated {
            round,
            max_rounds,
            node_id,
        } => progress_line(
            "✓",
            *round,
            *max_rounds,
            format!("{} patch generated", node_id.0),
        ),
        ProgressEvent::BuildVerifying {
            round,
            max_rounds,
            node_id,
        } => progress_line(
            "▸",
            *round,
            *max_rounds,
            format!("{} verifying build", node_id.0),
        ),
        ProgressEvent::BuildResult {
            round,
            max_rounds,
            node_id,
            passed,
            total,
        } => format_build_result(*round, *max_rounds, node_id, *passed, *total),
        ProgressEvent::EvaluationStarted {
            round,
            max_rounds,
            node_id,
        } => progress_line(
            "▸",
            *round,
            *max_rounds,
            format!("{} recording evaluation", node_id.0),
        ),
        ProgressEvent::EvaluationComplete {
            round,
            max_rounds,
            node_id,
            evaluated,
        } => progress_line(
            "✓",
            *round,
            *max_rounds,
            format!(
                "{} evaluation complete ({} evaluator{})",
                node_id.0,
                evaluated,
                plural_suffix(*evaluated as u64)
            ),
        ),
        ProgressEvent::ScoringComplete {
            round,
            max_rounds,
            decision,
            winner,
        } => format_scoring_complete(*round, *max_rounds, decision, winner.as_ref()),
        ProgressEvent::RoundComplete {
            round,
            max_rounds,
            decision,
            continuing,
        } => format_round_complete(*round, *max_rounds, decision, *continuing),
        ProgressEvent::ChainRecorded {
            round,
            max_rounds,
            entry_index,
        } => progress_line(
            "✓",
            *round,
            *max_rounds,
            format!("chain entry #{entry_index} recorded"),
        ),
    }
}

fn format_build_result(
    round: u32,
    max_rounds: u32,
    node_id: &fx_consensus::NodeId,
    passed: usize,
    total: usize,
) -> String {
    match BuildSummary::from_counts(passed, total).outcome() {
        BuildOutcome::Skipped => progress_line(
            "▸",
            round,
            max_rounds,
            format!("{} build verification skipped", node_id.0),
        ),
        BuildOutcome::Passed => progress_line(
            "✓",
            round,
            max_rounds,
            format!("{} build passed", node_id.0),
        ),
        BuildOutcome::Failed { failed, total } => progress_line(
            "✗",
            round,
            max_rounds,
            format!("{} build failed on {failed}/{total} evaluators", node_id.0),
        ),
    }
}

fn format_scoring_complete(
    round: u32,
    max_rounds: u32,
    decision: &Decision,
    winner: Option<&fx_consensus::NodeId>,
) -> String {
    let winner_suffix = winner
        .map(|node_id| format!(" (winner: {})", node_id.0))
        .unwrap_or_default();
    progress_line(
        decision_symbol(decision, false),
        round,
        max_rounds,
        format!(
            "scoring complete — {}{winner_suffix}",
            decision.uppercase_label()
        ),
    )
}

fn format_round_complete(
    round: u32,
    max_rounds: u32,
    decision: &Decision,
    continuing: bool,
) -> String {
    let status = if continuing {
        format!("{} — continuing", decision.uppercase_label())
    } else {
        decision.uppercase_label().to_owned()
    };
    progress_line(
        decision_symbol(decision, continuing),
        round,
        max_rounds,
        format!("round complete — {status}"),
    )
}

fn progress_line(symbol: &str, round: u32, max_rounds: u32, message: String) -> String {
    let prefix = round_prefix(round, max_rounds);
    format!("{symbol} {prefix}{message}")
}

fn round_prefix(round: u32, max_rounds: u32) -> String {
    if max_rounds > 1 {
        format!("Round {round}/{max_rounds}: ")
    } else {
        String::new()
    }
}

fn decision_symbol(decision: &Decision, continuing: bool) -> &'static str {
    if continuing {
        return "▸";
    }
    match decision {
        Decision::Accept => "✓",
        Decision::Reject => "✗",
        Decision::Inconclusive => "▸",
    }
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
    use super::{format_progress_event, note_denominator, note_flag, note_number, note_value};
    use fx_consensus::{Decision, GenerationStrategy, NodeId, ProgressEvent};

    fn assert_progress_line(event: ProgressEvent, expected: &str) {
        assert_eq!(format_progress_event(&event), expected);
    }

    #[test]
    fn format_progress_event_covers_round_lifecycle_variants() {
        assert_progress_line(
            ProgressEvent::RoundStarted {
                round: 1,
                max_rounds: 3,
                signal: "latency spike".into(),
            },
            "▸ Round 1/3: started signal latency spike",
        );
        assert_progress_line(
            ProgressEvent::BaselineCollected {
                round: 1,
                max_rounds: 3,
                node_count: 2,
            },
            "▸ Round 1/3: baseline collected for 2 nodes",
        );
        assert_progress_line(
            ProgressEvent::ChainRecorded {
                round: 1,
                max_rounds: 3,
                entry_index: 7,
            },
            "✓ Round 1/3: chain entry #7 recorded",
        );
    }

    #[test]
    fn format_progress_event_covers_node_and_build_variants() {
        let node_id = NodeId::from("node-a");
        assert_progress_line(
            ProgressEvent::NodeStarted {
                round: 2,
                max_rounds: 3,
                node_id: node_id.clone(),
                strategy: GenerationStrategy::Creative,
            },
            "▸ Round 2/3: node-a (creative) generating patch",
        );
        assert_progress_line(
            ProgressEvent::PatchGenerated {
                round: 2,
                max_rounds: 3,
                node_id: node_id.clone(),
            },
            "✓ Round 2/3: node-a patch generated",
        );
        assert_progress_line(
            ProgressEvent::BuildVerifying {
                round: 2,
                max_rounds: 3,
                node_id: node_id.clone(),
            },
            "▸ Round 2/3: node-a verifying build",
        );
        assert_progress_line(
            ProgressEvent::BuildResult {
                round: 2,
                max_rounds: 3,
                node_id: node_id.clone(),
                passed: 0,
                total: 0,
            },
            "▸ Round 2/3: node-a build verification skipped",
        );
        assert_progress_line(
            ProgressEvent::BuildResult {
                round: 2,
                max_rounds: 3,
                node_id: node_id.clone(),
                passed: 2,
                total: 2,
            },
            "✓ Round 2/3: node-a build passed",
        );
        assert_progress_line(
            ProgressEvent::BuildResult {
                round: 2,
                max_rounds: 3,
                node_id,
                passed: 1,
                total: 3,
            },
            "✗ Round 2/3: node-a build failed on 2/3 evaluators",
        );
    }

    #[test]
    fn format_progress_event_covers_evaluation_and_scoring_variants() {
        let node_id = NodeId::from("node-b");
        assert_progress_line(
            ProgressEvent::EvaluationStarted {
                round: 3,
                max_rounds: 3,
                node_id: node_id.clone(),
            },
            "▸ Round 3/3: node-b recording evaluation",
        );
        assert_progress_line(
            ProgressEvent::EvaluationComplete {
                round: 3,
                max_rounds: 3,
                node_id: node_id.clone(),
                evaluated: 1,
            },
            "✓ Round 3/3: node-b evaluation complete (1 evaluator)",
        );
        assert_progress_line(
            ProgressEvent::ScoringComplete {
                round: 3,
                max_rounds: 3,
                decision: Decision::Accept,
                winner: Some(node_id.clone()),
            },
            "✓ Round 3/3: scoring complete — ACCEPT (winner: node-b)",
        );
        assert_progress_line(
            ProgressEvent::RoundComplete {
                round: 3,
                max_rounds: 3,
                decision: Decision::Reject,
                continuing: true,
            },
            "▸ Round 3/3: round complete — REJECT — continuing",
        );
        assert_progress_line(
            ProgressEvent::RoundComplete {
                round: 3,
                max_rounds: 3,
                decision: Decision::Inconclusive,
                continuing: false,
            },
            "▸ Round 3/3: round complete — INCONCLUSIVE",
        );
    }

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
