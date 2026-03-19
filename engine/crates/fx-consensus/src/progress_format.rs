use crate::{
    plural_suffix, BuildOutcome, BuildSummary, Decision, GenerationStrategy, NodeId, ProgressEvent,
};
use std::fmt;

pub fn format_progress_event(event: &ProgressEvent) -> String {
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
    node_id: &NodeId,
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
    winner: Option<&NodeId>,
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

pub struct StrategyDisplay<'a>(&'a GenerationStrategy);

impl fmt::Display for StrategyDisplay<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self.0 {
            GenerationStrategy::Conservative => "Conservative",
            GenerationStrategy::Aggressive => "Aggressive",
            GenerationStrategy::Creative => "Creative",
        };
        formatter.write_str(label)
    }
}

pub fn display_strategy(strategy: &GenerationStrategy) -> StrategyDisplay<'_> {
    StrategyDisplay(strategy)
}

#[cfg(test)]
mod tests {
    use super::format_progress_event;
    use crate::{Decision, GenerationStrategy, NodeId, ProgressEvent};

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
            "▸ Round 2/3: node-a (Creative) generating patch",
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
}
