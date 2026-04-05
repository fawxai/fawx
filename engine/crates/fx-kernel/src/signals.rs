pub use fx_core::signals::{LoopStep, Signal, SignalKind, SignalSeverity};

use std::sync::atomic::{AtomicU64, Ordering};

/// Accumulates signals for a single loop cycle.
#[derive(Debug)]
pub struct SignalCollector {
    signals: Vec<Signal>,
    max_signals: usize,
    next_id: AtomicU64,
}

impl Clone for SignalCollector {
    fn clone(&self) -> Self {
        Self {
            signals: self.signals.clone(),
            max_signals: self.max_signals,
            next_id: AtomicU64::new(self.next_id.load(Ordering::SeqCst)),
        }
    }
}

impl Default for SignalCollector {
    fn default() -> Self {
        Self {
            signals: Vec::new(),
            max_signals: 200,
            next_id: AtomicU64::new(1),
        }
    }
}

impl SignalCollector {
    pub fn new(max_signals: usize) -> Self {
        Self {
            signals: Vec::new(),
            max_signals,
            next_id: AtomicU64::new(1),
        }
    }

    /// Reconstruct a read-only collector from a signal snapshot.
    /// The capacity is set to the snapshot size (no further emissions expected).
    pub fn from_signals(signals: Vec<Signal>) -> Self {
        let max_id = signals.iter().map(|s| s.id).max().unwrap_or(0);
        Self {
            max_signals: signals.len().max(1),
            signals,
            next_id: AtomicU64::new(max_id + 1),
        }
    }

    /// Emit a signal. Drops oldest low-priority signals if at capacity.
    /// Assigns a monotonic ID to the signal.
    pub fn emit(&mut self, mut signal: Signal) {
        // Assign monotonic ID if not already set (id == 0 means unassigned)
        if signal.id == 0 {
            signal.id = self.next_id.fetch_add(1, Ordering::SeqCst);
        }

        if self.signals.len() >= self.max_signals {
            self.drop_signal_for_capacity();
        }
        self.signals.push(signal);
    }

    /// Emit a signal with the given parameters, assigning a monotonic ID.
    pub fn emit_signal(
        &mut self,
        step: LoopStep,
        kind: SignalKind,
        message: impl Into<String>,
        metadata: serde_json::Value,
        timestamp_ms: u64,
    ) {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let signal = Signal::new(id, step, kind, message, metadata, timestamp_ms);
        self.emit(signal);
    }

    fn drop_signal_for_capacity(&mut self) {
        let low_priority_pos = self.signals.iter().position(is_low_priority_signal);
        if let Some(pos) = low_priority_pos {
            self.signals.remove(pos);
        } else if !self.signals.is_empty() {
            self.signals.remove(0);
        }
    }

    /// Drain signals by kind.
    pub fn drain_by_kind(&mut self, kind: SignalKind) -> Vec<Signal> {
        let mut matching = Vec::new();
        self.signals.retain(|s| {
            if s.kind == kind {
                matching.push(s.clone());
                false
            } else {
                true
            }
        });
        matching
    }

    pub fn drain_all(&mut self) -> Vec<Signal> {
        std::mem::take(&mut self.signals)
    }

    /// All signals (read-only).
    pub fn signals(&self) -> &[Signal] {
        &self.signals
    }

    /// Condensed summary (max 5 lines).
    pub fn summary(&self) -> String {
        let friction_count = count_by_kind(&self.signals, SignalKind::Friction);
        let success_count = count_by_kind(&self.signals, SignalKind::Success);
        let tool_count = self
            .signals
            .iter()
            .filter(|signal| signal.step == LoopStep::Act)
            .count();

        let mut lines = vec![format!("{} signals", self.signals.len())];
        if success_count > 0 {
            lines.push(format!("{success_count} success"));
        }
        if friction_count > 0 {
            lines.push(format!("{friction_count} friction"));
        }
        if tool_count == 1 {
            lines.push("1 tool action".to_string());
        } else if tool_count > 0 {
            lines.push(format!("{tool_count} tool actions"));
        }
        if let Some(last_friction) = self
            .signals
            .iter()
            .rev()
            .find(|signal| signal.kind == SignalKind::Friction)
        {
            lines.push(format!("last friction: {}", last_friction.message));
        }

        lines.into_iter().take(5).collect::<Vec<_>>().join(" · ")
    }

    /// Full debug dump.
    pub fn debug_dump(&self) -> String {
        self.signals
            .iter()
            .map(format_signal_debug_line)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Reset for new cycle.
    pub fn clear(&mut self) {
        self.signals.clear();
        self.next_id.store(1, Ordering::SeqCst);
    }

    /// Get the next ID that will be assigned (for testing/debugging).
    pub fn next_id(&self) -> u64 {
        self.next_id.load(Ordering::SeqCst)
    }
}

fn format_signal_debug_line(signal: &Signal) -> String {
    format!(
        "[{:?}/{:?}] {} (id={}, ts={})",
        signal.step, signal.kind, signal.message, signal.id, signal.timestamp_ms
    )
}

fn is_low_priority_signal(signal: &Signal) -> bool {
    matches!(signal.kind, SignalKind::Trace | SignalKind::Performance)
}

fn count_by_kind(signals: &[Signal], kind: SignalKind) -> usize {
    signals.iter().filter(|signal| signal.kind == kind).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_signal(step: LoopStep, kind: SignalKind, message: &str, timestamp_ms: u64) -> Signal {
        Signal {
            id: 0, // Will be assigned by collector
            span_id: None,
            step,
            kind,
            severity: kind.default_severity(),
            message: message.to_string(),
            metadata: serde_json::json!({"test": true}),
            timestamp_ms,
            cause_id: None,
            duration_ms: None,
        }
    }

    #[test]
    fn signal_collector_emits_and_retrieves() {
        let mut collector = SignalCollector::new(10);
        collector.emit(mk_signal(LoopStep::Perceive, SignalKind::Trace, "p1", 1));
        collector.emit(mk_signal(LoopStep::Reason, SignalKind::Trace, "r1", 2));
        collector.emit(mk_signal(LoopStep::Act, SignalKind::Success, "a1", 3));

        let signals = collector.signals();
        assert_eq!(signals.len(), 3);
        assert_eq!(signals[0].id, 1);
        assert_eq!(signals[1].id, 2);
        assert_eq!(signals[2].id, 3);
        assert_eq!(signals[0].message, "p1");
        assert_eq!(signals[1].message, "r1");
        assert_eq!(signals[2].message, "a1");
    }

    #[test]
    fn signal_collector_assigns_monotonic_ids() {
        let mut collector = SignalCollector::new(10);
        
        // First signal gets id=1
        collector.emit(mk_signal(LoopStep::Act, SignalKind::Success, "first", 1));
        assert_eq!(collector.signals()[0].id, 1);
        
        // Second signal gets id=2
        collector.emit(mk_signal(LoopStep::Act, SignalKind::Success, "second", 2));
        assert_eq!(collector.signals()[1].id, 2);
        
        // Third signal gets id=3
        collector.emit(mk_signal(LoopStep::Act, SignalKind::Success, "third", 3));
        assert_eq!(collector.signals()[2].id, 3);
    }

    #[test]
    fn signal_collector_preserves_existing_id() {
        let mut collector = SignalCollector::new(10);
        
        // Signal with pre-set ID is preserved
        let mut signal = mk_signal(LoopStep::Act, SignalKind::Success, "preset", 1);
        signal.id = 42;
        collector.emit(signal);
        
        assert_eq!(collector.signals()[0].id, 42);
        
        // Next signal gets next available ID (not 1, since we started at 1)
        collector.emit(mk_signal(LoopStep::Act, SignalKind::Success, "next", 2));
        assert_eq!(collector.signals()[1].id, 1); // First assigned ID
    }

    #[test]
    fn signal_collector_drops_low_priority_at_capacity() {
        let mut collector = SignalCollector::new(3);
        collector.emit(mk_signal(
            LoopStep::Perceive,
            SignalKind::Trace,
            "trace-1",
            1,
        ));
        collector.emit(mk_signal(
            LoopStep::Act,
            SignalKind::Success,
            "success-1",
            2,
        ));
        collector.emit(mk_signal(
            LoopStep::Synthesize,
            SignalKind::Friction,
            "friction-1",
            3,
        ));

        collector.emit(mk_signal(
            LoopStep::Act,
            SignalKind::Friction,
            "friction-2",
            4,
        ));

        let messages = collector
            .signals()
            .iter()
            .map(|signal| signal.message.as_str())
            .collect::<Vec<_>>();
        assert_eq!(messages, vec!["success-1", "friction-1", "friction-2"]);
    }

    #[test]
    fn signal_collector_drops_oldest_when_all_high_priority() {
        let mut collector = SignalCollector::new(2);
        collector.emit(mk_signal(LoopStep::Act, SignalKind::Friction, "first", 1));
        collector.emit(mk_signal(LoopStep::Act, SignalKind::Friction, "second", 2));

        collector.emit(mk_signal(LoopStep::Act, SignalKind::Friction, "third", 3));

        let messages = collector
            .signals()
            .iter()
            .map(|signal| signal.message.as_str())
            .collect::<Vec<_>>();
        assert_eq!(messages, vec!["second", "third"]);
    }

    #[test]
    fn drain_by_kind_removes_matching() {
        let mut collector = SignalCollector::new(10);
        collector.emit(mk_signal(
            LoopStep::Act,
            SignalKind::Friction,
            "friction-1",
            1,
        ));
        collector.emit(mk_signal(
            LoopStep::Act,
            SignalKind::Success,
            "success-1",
            2,
        ));
        collector.emit(mk_signal(
            LoopStep::Synthesize,
            SignalKind::Friction,
            "friction-2",
            3,
        ));

        let drained = collector.drain_by_kind(SignalKind::Friction);

        assert_eq!(drained.len(), 2);
        assert_eq!(collector.signals().len(), 1);
        assert_eq!(collector.signals()[0].kind, SignalKind::Success);
    }

    #[test]
    fn summary_format() {
        let mut collector = SignalCollector::new(10);
        collector.emit(mk_signal(LoopStep::Act, SignalKind::Success, "tool ok", 1));
        collector.emit(mk_signal(
            LoopStep::Synthesize,
            SignalKind::Friction,
            "mismatch",
            2,
        ));

        let summary = collector.summary();
        assert!(summary.contains("2 signals"));
        assert!(summary.contains("1 success"));
        assert!(summary.contains("1 friction"));
        assert!(summary.contains("1 tool action"));
    }

    #[test]
    fn debug_dump_format() {
        let mut collector = SignalCollector::new(10);
        collector.emit(mk_signal(
            LoopStep::Reason,
            SignalKind::Trace,
            "llm done",
            42,
        ));

        let dump = collector.debug_dump();
        assert!(dump.contains("[Reason/Trace]"));
        assert!(dump.contains("llm done"));
        assert!(dump.contains("id=1"));
        assert!(dump.contains("ts=42"));
    }

    #[test]
    fn loop_step_to_label_is_stable() {
        assert_eq!(LoopStep::Perceive.to_label(), "perceive");
        assert_eq!(LoopStep::Act.to_label(), "act");
    }

    #[test]
    fn signal_kind_to_label_is_stable() {
        assert_eq!(SignalKind::UserIntervention.to_label(), "user_intervention");
        assert_eq!(SignalKind::Success.to_label(), "success");
    }

    #[test]
    fn from_signals_preserves_ids_and_sets_next_id() {
        let signals = vec![
            Signal::new(5, LoopStep::Act, SignalKind::Success, "s1", serde_json::json!({}), 1),
            Signal::new(10, LoopStep::Act, SignalKind::Success, "s2", serde_json::json!({}), 2),
        ];
        
        let collector = SignalCollector::from_signals(signals);
        
        // IDs preserved
        assert_eq!(collector.signals()[0].id, 5);
        assert_eq!(collector.signals()[1].id, 10);
        
        // Next ID set to max + 1
        assert_eq!(collector.next_id(), 11);
    }

    #[test]
    fn clear_resets_signals_and_id_counter() {
        let mut collector = SignalCollector::new(10);
        collector.emit(mk_signal(LoopStep::Act, SignalKind::Success, "s1", 1));
        collector.emit(mk_signal(LoopStep::Act, SignalKind::Success, "s2", 2));
        
        assert_eq!(collector.signals().len(), 2);
        assert_eq!(collector.next_id(), 3);
        
        collector.clear();
        
        assert!(collector.signals().is_empty());
        assert_eq!(collector.next_id(), 1);
    }

    #[test]
    fn emit_signal_convenience_method() {
        let mut collector = SignalCollector::new(10);
        collector.emit_signal(
            LoopStep::Act,
            SignalKind::Success,
            "convenience",
            serde_json::json!({"key": "value"}),
            1000,
        );
        
        let signals = collector.signals();
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0].id, 1);
        assert_eq!(signals[0].step, LoopStep::Act);
        assert_eq!(signals[0].kind, SignalKind::Success);
        assert_eq!(signals[0].message, "convenience");
        assert_eq!(signals[0].severity, SignalSeverity::Low);
    }
}
