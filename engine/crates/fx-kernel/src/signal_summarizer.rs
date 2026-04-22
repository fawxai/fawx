use crate::budget::{BudgetState, SignalFeedbackConfig};
use crate::signals::{Signal, SignalKind};
use std::collections::{BTreeSet, HashMap};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ObservationPressure {
    pub rounds_used: u16,
    pub rounds_limit: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SignalSummaryContext {
    pub budget_state: BudgetState,
    pub budget_remaining_percent: Option<u8>,
    pub observation_pressure: Option<ObservationPressure>,
}

impl Default for SignalSummaryContext {
    fn default() -> Self {
        Self {
            budget_state: BudgetState::Normal,
            budget_remaining_percent: None,
            observation_pressure: None,
        }
    }
}

pub(crate) struct SignalSummarizer;

impl SignalSummarizer {
    pub(crate) fn summarize(
        signals: &[Signal],
        context: SignalSummaryContext,
        config: &SignalFeedbackConfig,
    ) -> Option<String> {
        if !config.enabled || config.max_summary_tokens == 0 {
            return None;
        }

        let relevant = signals
            .iter()
            .filter(|signal| signal.severity >= config.min_severity)
            .collect::<Vec<_>>();

        let mut bullets = Vec::new();
        let mut seen_bullets = BTreeSet::new();
        push_unique(
            &mut bullets,
            &mut seen_bullets,
            repeated_failure_guidance(&relevant),
        );
        push_unique(
            &mut bullets,
            &mut seen_bullets,
            repeated_timeout_guidance(&relevant),
        );
        push_unique(
            &mut bullets,
            &mut seen_bullets,
            repeated_retry_guidance(&relevant),
        );
        push_unique(&mut bullets, &mut seen_bullets, budget_guidance(context));
        push_unique(
            &mut bullets,
            &mut seen_bullets,
            observation_guidance(context.observation_pressure),
        );
        push_unique(
            &mut bullets,
            &mut seen_bullets,
            context_overflow_guidance(&relevant),
        );

        if bullets.is_empty() {
            push_unique(
                &mut bullets,
                &mut seen_bullets,
                generic_friction_guidance(&relevant),
            );
        }

        if bullets.is_empty() {
            return None;
        }

        bounded_summary(
            "Recent signal guidance:",
            &bullets,
            config.max_summary_tokens,
        )
    }
}

fn repeated_failure_guidance(signals: &[&Signal]) -> Option<String> {
    let pattern = strongest_tool_pattern(signals, &[SignalKind::Blocked, SignalKind::Friction])?;
    if pattern.count < 2 {
        return None;
    }

    if pattern.permanent_failure {
        Some(format!(
            "Avoid retrying `{}`; it has failed repeatedly with permanent errors.",
            pattern.tool
        ))
    } else {
        Some(format!(
            "Avoid repeating `{}` unchanged; it has been blocked repeatedly. Change the arguments, use a different tool, or surface the blocker.",
            pattern.tool
        ))
    }
}

fn repeated_timeout_guidance(signals: &[&Signal]) -> Option<String> {
    let pattern = strongest_tool_pattern(signals, &[SignalKind::Timeout])?;
    (pattern.count >= 2).then(|| {
        format!(
            "`{}` has timed out repeatedly; avoid slow retries and prefer a narrower or alternative step.",
            pattern.tool
        )
    })
}

fn repeated_retry_guidance(signals: &[&Signal]) -> Option<String> {
    let pattern = strongest_tool_pattern(signals, &[SignalKind::Retry])?;
    (pattern.count >= 2).then(|| {
        format!(
            "Retries keep clustering on `{}`; change the arguments or switch tools instead of repeating the same call.",
            pattern.tool
        )
    })
}

fn budget_guidance(context: SignalSummaryContext) -> Option<String> {
    let remaining = context.budget_remaining_percent?;
    (remaining <= 40).then(|| {
        format!(
            "Budget is getting tight ({}% remaining); prefer direct steps over exploratory research.",
            remaining
        )
    })
}

fn observation_guidance(pressure: Option<ObservationPressure>) -> Option<String> {
    let pressure = pressure?;
    if pressure.rounds_limit == 0
        || pressure.rounds_used == 0
        || pressure.rounds_used.saturating_add(1) < pressure.rounds_limit
    {
        return None;
    }

    Some(format!(
        "Observation-only rounds are nearly exhausted ({}/{}); batch any final reads now or switch to implementation.",
        pressure.rounds_used, pressure.rounds_limit
    ))
}

fn context_overflow_guidance(signals: &[&Signal]) -> Option<String> {
    signals
        .iter()
        .any(|signal| signal.kind == SignalKind::ContextOverflow)
        .then_some(
            "Context compaction already fired; avoid exploratory context growth and reuse the evidence already in hand."
                .to_string(),
        )
}

fn generic_friction_guidance(signals: &[&Signal]) -> Option<String> {
    let friction = signals
        .iter()
        .filter(|signal| matches!(signal.kind, SignalKind::Blocked | SignalKind::Friction))
        .count();

    (friction >= 2).then_some(
        "Recent runs hit repeated blockers; verify the next assumption before trying the same path again."
            .to_string(),
    )
}

struct ToolPattern {
    tool: String,
    count: usize,
    permanent_failure: bool,
}

fn strongest_tool_pattern(signals: &[&Signal], kinds: &[SignalKind]) -> Option<ToolPattern> {
    let mut patterns: HashMap<String, (usize, bool)> = HashMap::new();

    for signal in signals
        .iter()
        .copied()
        .filter(|signal| kinds.contains(&signal.kind))
    {
        let Some(tool) = tool_name(signal) else {
            continue;
        };
        let entry = patterns.entry(tool.to_string()).or_insert((0, false));
        entry.0 += 1;
        entry.1 |= signal_indicates_permanent_failure(signal);
    }

    patterns
        .into_iter()
        .max_by_key(|(_, (count, permanent))| (*count, *permanent))
        .map(|(tool, (count, permanent_failure))| ToolPattern {
            tool,
            count,
            permanent_failure,
        })
}

fn tool_name(signal: &Signal) -> Option<&str> {
    signal
        .metadata
        .get("tool")
        .and_then(serde_json::Value::as_str)
        .or_else(|| extract_tool_name(&signal.message))
}

fn extract_tool_name(message: &str) -> Option<&str> {
    if let Some(rest) = message.strip_prefix("retrying tool '") {
        return rest.split('\'').next();
    }
    if let Some(rest) = message.strip_prefix("tool '") {
        return rest.split('\'').next();
    }
    if let Some(rest) = message.strip_prefix("tool ") {
        return rest
            .split_whitespace()
            .next()
            .map(|name| name.trim_matches(|ch| ch == '\'' || ch == ':'));
    }
    None
}

fn signal_indicates_permanent_failure(signal: &Signal) -> bool {
    signal
        .metadata
        .get("permanent")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
        || signal
            .metadata
            .get("failure_class")
            .and_then(serde_json::Value::as_str)
            == Some("permanent")
}

fn push_unique(lines: &mut Vec<String>, seen: &mut BTreeSet<String>, line: Option<String>) {
    let Some(line) = line else {
        return;
    };
    if seen.insert(line.clone()) {
        lines.push(line);
    }
}

fn bounded_summary(header: &str, bullets: &[String], max_tokens: usize) -> Option<String> {
    let header_tokens = approx_token_count(header);
    if max_tokens <= header_tokens {
        return None;
    }

    let mut lines = vec![header.to_string()];
    let mut used_tokens = header_tokens;
    let mut added_bullet = false;

    for bullet in bullets {
        let bullet_tokens = approx_token_count(bullet).saturating_add(1);
        if used_tokens.saturating_add(bullet_tokens) <= max_tokens {
            lines.push(format!("- {bullet}"));
            used_tokens = used_tokens.saturating_add(bullet_tokens);
            added_bullet = true;
            continue;
        }

        if !added_bullet {
            let remaining = max_tokens.saturating_sub(used_tokens).saturating_sub(1);
            let truncated = truncate_to_approx_tokens(bullet, remaining);
            if truncated.is_empty() {
                break;
            }
            lines.push(format!("- {truncated}"));
            added_bullet = true;
        }
        break;
    }

    added_bullet.then(|| lines.join("\n"))
}

fn approx_token_count(text: &str) -> usize {
    text.split_whitespace().count()
}

fn truncate_to_approx_tokens(text: &str, max_tokens: usize) -> String {
    if max_tokens == 0 {
        return String::new();
    }

    let words = text.split_whitespace().collect::<Vec<_>>();
    if words.len() <= max_tokens {
        return words.join(" ");
    }

    if max_tokens == 1 {
        return "...".to_string();
    }

    let mut truncated = words[..max_tokens - 1].join(" ");
    truncated.push_str(" ...");
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signals::{LoopStep, Signal, SignalSeverity};

    fn signal(kind: SignalKind, message: &str, metadata: serde_json::Value, id: u64) -> Signal {
        Signal::new(LoopStep::Act, kind, message, metadata, id)
            .with_id(id)
            .with_severity(kind.default_severity())
    }

    #[test]
    fn summarize_returns_none_for_unremarkable_signals() {
        let signals = vec![
            signal(
                SignalKind::Trace,
                "LLM call completed",
                serde_json::json!({}),
                1,
            ),
            signal(
                SignalKind::Success,
                "tool read_file",
                serde_json::json!({}),
                2,
            ),
        ];

        let summary =
            SignalSummarizer::summarize(&signals, SignalSummaryContext::default(), &config());

        assert!(summary.is_none());
    }

    #[test]
    fn summarize_returns_actionable_guidance_for_repeated_blockage() {
        let signals = vec![
            signal(
                SignalKind::Blocked,
                "tool 'run_command' blocked",
                serde_json::json!({
                    "tool": "run_command",
                    "failure_class": "permanent",
                    "permanent": true
                }),
                1,
            ),
            signal(
                SignalKind::Blocked,
                "tool 'run_command' blocked",
                serde_json::json!({
                    "tool": "run_command",
                    "failure_class": "permanent",
                    "permanent": true
                }),
                2,
            ),
        ];

        let summary =
            SignalSummarizer::summarize(&signals, SignalSummaryContext::default(), &config())
                .expect("summary");

        assert!(summary.contains("Recent signal guidance:"));
        assert!(summary.contains("Avoid retrying `run_command`"));
    }

    #[test]
    fn summarize_bounds_summary_size() {
        let config = SignalFeedbackConfig {
            max_summary_tokens: 18,
            ..config()
        };
        let signals = vec![
            signal(
                SignalKind::Blocked,
                "tool 'run_command' blocked",
                serde_json::json!({
                    "tool": "run_command",
                    "failure_class": "permanent",
                    "permanent": true
                }),
                1,
            ),
            signal(
                SignalKind::Blocked,
                "tool 'run_command' blocked",
                serde_json::json!({
                    "tool": "run_command",
                    "failure_class": "permanent",
                    "permanent": true
                }),
                2,
            ),
            signal(
                SignalKind::ContextOverflow,
                "conversation context compacted",
                serde_json::json!({}),
                3,
            ),
        ];
        let context = SignalSummaryContext {
            budget_state: BudgetState::Normal,
            budget_remaining_percent: Some(32),
            observation_pressure: Some(ObservationPressure {
                rounds_used: 2,
                rounds_limit: 3,
            }),
        };

        let summary =
            SignalSummarizer::summarize(&signals, context, &config).expect("bounded summary");

        assert!(summary.split_whitespace().count() <= config.max_summary_tokens);
    }

    #[test]
    fn extract_tool_name_supports_retry_and_plain_tool_messages() {
        assert_eq!(
            extract_tool_name("retrying tool 'run_command'"),
            Some("run_command")
        );
        assert_eq!(
            extract_tool_name("tool read_file blocked by policy"),
            Some("read_file")
        );
    }

    fn config() -> SignalFeedbackConfig {
        SignalFeedbackConfig {
            enabled: true,
            lookback_cycles: 3,
            min_severity: SignalSeverity::Medium,
            max_summary_tokens: 80,
        }
    }
}
