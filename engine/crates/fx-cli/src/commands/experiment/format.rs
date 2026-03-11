use super::RunExperimentArgs;
use crate::commands::experiment::placeholders::strategy_label;
use fx_consensus::{Chain, ChainEntry, Decision, ExperimentReport};
use uuid::Uuid;

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
        format!(
            "  Decision:      {}",
            decision_label(&report.result.decision)
        ),
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
        strategy_label(&candidate.strategy),
        candidate.aggregate_score,
    )
}

fn decision_label(decision: &Decision) -> &'static str {
    match decision {
        Decision::Accept => "✅ ACCEPT",
        Decision::Reject => "❌ REJECT",
        Decision::Inconclusive => "⚠️ INCONCLUSIVE",
    }
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
        decision_label(&entry.result.decision),
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
        format!("Decision: {}", decision_label(&entry.result.decision)),
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

fn format_score_lines(entry: &ChainEntry) -> String {
    let mut lines = scored_candidates(entry);
    if lines.is_empty() {
        lines.push("  - none".to_owned());
    }
    lines.join("\n")
}

fn scored_candidates(entry: &ChainEntry) -> Vec<String> {
    entry
        .result
        .aggregate_scores
        .iter()
        .map(|(candidate_id, score)| format_score_line(entry, candidate_id, *score))
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
