use crate::{current_epoch_secs, HealthSnapshot, HealthVector};
use fx_kernel::{Signal, SignalKind};
use std::collections::VecDeque;

const MILLIS_PER_SECOND: u64 = 1_000;

/// Rolling window of signals across multiple loop cycles.
///
/// Stores signals in insertion order, prunes expired entries on read, and
/// enforces a hard capacity cap to prevent unbounded growth.
pub struct SignalWindow {
    signals: VecDeque<Signal>,
    cycle_timestamps_ms: VecDeque<u64>,
    max_signals: usize,
    max_age_secs: u64,
}

impl SignalWindow {
    pub fn new(max_signals: usize, max_age_secs: u64) -> Self {
        Self {
            signals: VecDeque::new(),
            cycle_timestamps_ms: VecDeque::new(),
            max_signals,
            max_age_secs,
        }
    }

    /// Ingest signals from a completed loop cycle.
    pub fn ingest(&mut self, signals: Vec<Signal>) {
        let cycle_timestamp_ms = signals
            .iter()
            .map(|signal| signal.timestamp_ms)
            .max()
            .unwrap_or_else(now_ms);
        self.cycle_timestamps_ms.push_back(cycle_timestamp_ms);

        for signal in signals {
            self.signals.push_back(signal);
            self.enforce_capacity();
        }
        self.prune();
    }

    /// Get all signals within the window.
    ///
    /// This requires `&mut self` because reads lazily prune expired entries
    /// and make the internal deque contiguous before returning a slice.
    pub fn signals(&mut self) -> &[Signal] {
        self.prune();
        self.signals.make_contiguous()
    }

    pub fn signal_count(&self) -> u64 {
        let cutoff_ms = self.cutoff_ms();
        self.signals
            .iter()
            .filter(|signal| signal.timestamp_ms >= cutoff_ms)
            .count() as u64
    }

    pub fn cycle_count(&self) -> u64 {
        let cutoff_ms = self.cutoff_ms();
        self.cycle_timestamps_ms
            .iter()
            .filter(|timestamp_ms| **timestamp_ms >= cutoff_ms)
            .count() as u64
    }

    /// Compute the multi-dimensional health vector for the current window.
    pub fn compute_health(&mut self) -> HealthVector {
        self.snapshot(self.max_age_secs).health
    }

    /// Capture an inspectable window snapshot for canary baseline or evaluation.
    pub fn snapshot(&mut self, window_seconds: u64) -> HealthSnapshot {
        self.prune();
        HealthSnapshot {
            captured_at: current_epoch_secs(),
            window_seconds,
            total_signals: self.signals.len() as u64,
            cycle_count: self.cycle_timestamps_ms.len() as u64,
            health: compute_health_from_window(
                &self.signals,
                self.cycle_timestamps_ms.len() as u64,
            ),
        }
    }

    /// Prune expired signals.
    fn prune(&mut self) {
        let cutoff_ms = self.cutoff_ms();
        self.signals
            .retain(|signal| signal.timestamp_ms >= cutoff_ms);
        self.cycle_timestamps_ms
            .retain(|timestamp_ms| *timestamp_ms >= cutoff_ms);
        self.enforce_capacity();
    }

    fn enforce_capacity(&mut self) {
        while self.signals.len() > self.max_signals {
            self.signals.pop_front();
        }
    }

    fn cutoff_ms(&self) -> u64 {
        current_epoch_secs()
            .saturating_sub(self.max_age_secs)
            .saturating_mul(MILLIS_PER_SECOND)
    }
}

fn compute_health_from_window(signals: &VecDeque<Signal>, cycle_count: u64) -> HealthVector {
    let mut accumulator = WindowHealthAccumulator::default();
    for signal in signals {
        accumulator.observe(signal);
    }
    accumulator.into_health(cycle_count)
}

#[derive(Default)]
struct WindowHealthAccumulator {
    outcome_count: u64,
    success_count: u64,
    friction_count: u64,
    blocked_count: u32,
    tool_attempt_count: u64,
    retry_count: u64,
    provider_fallback_count: u32,
    latency_samples: Vec<u64>,
    total_cost_cents: f64,
    cost_samples: u64,
}

impl WindowHealthAccumulator {
    fn observe(&mut self, signal: &Signal) {
        match signal.kind {
            SignalKind::Success | SignalKind::Friction => self.observe_outcome(signal),
            SignalKind::Blocked => self.observe_blocked(signal),
            SignalKind::Retry => self.observe_retry(signal),
            SignalKind::ProviderFallback => {
                self.provider_fallback_count = self.provider_fallback_count.saturating_add(1);
            }
            SignalKind::Cost => self.observe_cost(signal),
            _ => {}
        }
    }

    fn observe_outcome(&mut self, signal: &Signal) {
        self.outcome_count = self.outcome_count.saturating_add(1);
        match signal.kind {
            SignalKind::Success => {
                self.success_count = self.success_count.saturating_add(1);
            }
            SignalKind::Friction => {
                self.friction_count = self.friction_count.saturating_add(1);
            }
            _ => {}
        }

        if signal.tool_classification().is_some() {
            self.record_tool_attempt(signal);
        }
    }

    fn observe_blocked(&mut self, signal: &Signal) {
        self.outcome_count = self.outcome_count.saturating_add(1);
        self.blocked_count = self.blocked_count.saturating_add(1);
        if signal.has_tool_name() {
            self.tool_attempt_count = self.tool_attempt_count.saturating_add(1);
        }
    }

    fn observe_retry(&mut self, signal: &Signal) {
        if signal.has_tool_name() {
            self.retry_count = self.retry_count.saturating_add(1);
        }
    }

    fn observe_cost(&mut self, signal: &Signal) {
        if let Some(cost_cents) = signal.cost_cents() {
            self.total_cost_cents += cost_cents;
            self.cost_samples = self.cost_samples.saturating_add(1);
        }
    }

    fn record_tool_attempt(&mut self, signal: &Signal) {
        self.tool_attempt_count = self.tool_attempt_count.saturating_add(1);
        if let Some(duration_ms) = signal.duration_ms {
            self.latency_samples.push(duration_ms);
        }
    }

    fn into_health(mut self, cycle_count: u64) -> HealthVector {
        self.latency_samples.sort_unstable();
        HealthVector {
            success_rate: rate(self.success_count, self.outcome_count),
            friction_rate: rate(self.friction_count, self.outcome_count),
            avg_latency_ms: average_u64(&self.latency_samples),
            p95_latency_ms: p95_latency_ms(&self.latency_samples),
            retry_rate: rate(self.retry_count, self.tool_attempt_count),
            provider_fallback_count: self.provider_fallback_count,
            avg_cost_per_cycle: average_cost_per_cycle(
                self.total_cost_cents,
                self.cost_samples,
                cycle_count,
            ),
            blocked_count: self.blocked_count,
        }
    }
}

fn average_cost_per_cycle(
    total_cost_cents: f64,
    cost_samples: u64,
    cycle_count: u64,
) -> Option<f64> {
    if cost_samples == 0 || cycle_count == 0 {
        return None;
    }
    Some(total_cost_cents / cycle_count as f64)
}

fn rate(numerator: u64, denominator: u64) -> Option<f64> {
    (denominator > 0).then_some(numerator as f64 / denominator as f64)
}

fn average_u64(values: &[u64]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().sum::<u64>() as f64 / values.len() as f64)
}

fn p95_latency_ms(values: &[u64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let index = ((values.len() as f64) * 0.95).ceil() as usize - 1;
    Some(values[index.min(values.len() - 1)] as f64)
}

fn now_ms() -> u64 {
    current_epoch_secs() * MILLIS_PER_SECOND
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_kernel::{LoopStep, SignalSeverity};

    fn mk_signal(message: &str, timestamp_ms: u64) -> Signal {
        Signal::new(
            LoopStep::Act,
            SignalKind::Success,
            message.to_string(),
            serde_json::json!({}),
            timestamp_ms,
        )
    }

    fn now_ms() -> u64 {
        current_epoch_secs() * MILLIS_PER_SECOND
    }

    fn tool_success(duration_ms: u64, timestamp_ms: u64) -> Signal {
        Signal::new(
            LoopStep::Act,
            SignalKind::Success,
            "tool read_file",
            serde_json::json!({ "classification": "observation" }),
            timestamp_ms,
        )
        .with_duration_ms(duration_ms)
    }

    fn tool_friction(duration_ms: u64, timestamp_ms: u64) -> Signal {
        Signal::new(
            LoopStep::Act,
            SignalKind::Friction,
            "tool read_file",
            serde_json::json!({ "classification": "observation" }),
            timestamp_ms,
        )
        .with_duration_ms(duration_ms)
    }

    fn blocked_tool(timestamp_ms: u64) -> Signal {
        Signal::new(
            LoopStep::Act,
            SignalKind::Blocked,
            "tool 'read_file' blocked",
            serde_json::json!({ "tool": "read_file" }),
            timestamp_ms,
        )
    }

    #[test]
    fn keeps_only_newest_signals_at_capacity() {
        let mut window = SignalWindow::new(2, 3_600);
        let now = now_ms();

        window.ingest(vec![
            mk_signal("first", now),
            mk_signal("second", now + 1),
            mk_signal("third", now + 2),
        ]);

        let messages = window
            .signals()
            .iter()
            .map(|signal| signal.message.as_str())
            .collect::<Vec<_>>();
        assert_eq!(messages, vec!["second", "third"]);
    }

    #[test]
    fn prunes_expired_signals_on_read() {
        let mut window = SignalWindow::new(4, 60);
        let now = now_ms();
        let expired = now.saturating_sub(120 * MILLIS_PER_SECOND);

        window.ingest(vec![mk_signal("old", expired), mk_signal("fresh", now)]);

        let messages = window
            .signals()
            .iter()
            .map(|signal| signal.message.as_str())
            .collect::<Vec<_>>();
        assert_eq!(messages, vec!["fresh"]);
    }

    #[test]
    fn empty_window_returns_empty_slice() {
        let mut window = SignalWindow::new(2, 60);
        assert!(window.signals().is_empty());
    }

    #[test]
    fn count_accessors_are_read_only() {
        let mut window = SignalWindow::new(4, 3_600);
        let now = now_ms();
        window.ingest(vec![mk_signal("first", now)]);

        let window_ref = &window;
        assert_eq!(window_ref.signal_count(), 1);
        assert_eq!(window_ref.cycle_count(), 1);
    }

    #[test]
    fn compute_health_uses_operational_dimensions() {
        let mut window = SignalWindow::new(16, 3_600);
        let now = now_ms();

        window.ingest(vec![
            tool_success(100, now),
            tool_success(200, now + 1),
            Signal::new(
                LoopStep::Act,
                SignalKind::Retry,
                "retrying tool",
                serde_json::json!({ "tool": "read_file", "tool_call_id": "a-1" }),
                now + 2,
            ),
            Signal::new(
                LoopStep::Reason,
                SignalKind::Cost,
                "llm usage observed",
                serde_json::json!({ "cost_cents": 1.5 }),
                now + 3,
            ),
        ]);
        window.ingest(vec![
            tool_friction(400, now + 10),
            blocked_tool(now + 11),
            Signal::new(
                LoopStep::Act,
                SignalKind::ProviderFallback,
                "router fell back",
                serde_json::json!({}),
                now + 12,
            ),
            Signal::new(
                LoopStep::Act,
                SignalKind::Trace,
                "noise",
                serde_json::json!({}),
                now + 13,
            ),
        ]);

        let health = window.compute_health();

        assert_eq!(window.cycle_count(), 2);
        assert_eq!(health.success_rate, Some(0.5));
        assert_eq!(health.friction_rate, Some(0.25));
        assert_eq!(health.avg_latency_ms, Some((100.0 + 200.0 + 400.0) / 3.0));
        assert_eq!(health.p95_latency_ms, Some(400.0));
        assert_eq!(health.retry_rate, Some(0.25));
        assert_eq!(health.provider_fallback_count, 1);
        assert_eq!(health.avg_cost_per_cycle, Some(0.75));
        assert_eq!(health.blocked_count, 1);
    }

    #[test]
    fn compute_health_treats_sparse_dimensions_as_unavailable() {
        let mut window = SignalWindow::new(4, 3_600);
        let now = now_ms();

        window.ingest(vec![Signal::new(
            LoopStep::Act,
            SignalKind::Trace,
            "noise",
            serde_json::json!({}),
            now,
        )]);
        window.ingest(Vec::new());

        let health = window.compute_health();

        assert_eq!(window.cycle_count(), 2);
        assert_eq!(health.success_rate, None);
        assert_eq!(health.friction_rate, None);
        assert_eq!(health.avg_latency_ms, None);
        assert_eq!(health.p95_latency_ms, None);
        assert_eq!(health.retry_rate, None);
        assert_eq!(health.avg_cost_per_cycle, None);
        assert_eq!(health.provider_fallback_count, 0);
        assert_eq!(health.blocked_count, 0);
    }

    #[test]
    fn preserves_enriched_signal_fields() {
        let mut window = SignalWindow::new(2, 60);
        let signal = mk_signal("linked", now_ms())
            .with_id(7)
            .with_severity(SignalSeverity::High)
            .with_span_id("span-1")
            .with_cause_id(6)
            .with_duration_ms(120);

        window.ingest(vec![signal]);

        let stored = &window.signals()[0];
        assert_eq!(stored.id, 7);
        assert_eq!(stored.severity, SignalSeverity::High);
        assert_eq!(stored.span_id.as_deref(), Some("span-1"));
        assert_eq!(stored.cause_id, Some(6));
        assert_eq!(stored.duration_ms, Some(120));
    }
}
