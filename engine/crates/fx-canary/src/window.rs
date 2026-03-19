use crate::current_epoch_secs;
use fx_kernel::Signal;
use std::collections::VecDeque;

const MILLIS_PER_SECOND: u64 = 1_000;

/// Rolling window of signals across multiple loop cycles.
///
/// Stores signals in insertion order, prunes expired entries on read, and
/// enforces a hard capacity cap to prevent unbounded growth.
pub struct SignalWindow {
    signals: VecDeque<Signal>,
    max_signals: usize,
    max_age_secs: u64,
}

impl SignalWindow {
    pub fn new(max_signals: usize, max_age_secs: u64) -> Self {
        Self {
            signals: VecDeque::new(),
            max_signals,
            max_age_secs,
        }
    }

    /// Ingest signals from a completed loop cycle.
    pub fn ingest(&mut self, signals: Vec<Signal>) {
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

    /// Prune expired signals.
    fn prune(&mut self) {
        let cutoff_ms = current_epoch_secs()
            .saturating_sub(self.max_age_secs)
            .saturating_mul(MILLIS_PER_SECOND);
        self.signals
            .retain(|signal| signal.timestamp_ms >= cutoff_ms);
        self.enforce_capacity();
    }

    fn enforce_capacity(&mut self) {
        while self.signals.len() > self.max_signals {
            self.signals.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_kernel::{LoopStep, SignalKind};

    fn mk_signal(message: &str, timestamp_ms: u64) -> Signal {
        Signal {
            step: LoopStep::Act,
            kind: SignalKind::Success,
            message: message.to_string(),
            metadata: serde_json::json!({}),
            timestamp_ms,
        }
    }

    fn now_ms() -> u64 {
        current_epoch_secs() * MILLIS_PER_SECOND
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
}
