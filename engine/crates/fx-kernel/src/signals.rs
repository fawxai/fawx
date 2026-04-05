pub use fx_core::signals::{LoopStep, Signal, SignalKind, SignalSeverity};

use std::collections::HashMap;
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
        if signal.id == Signal::UNASSIGNED_ID {
            signal.id = self.next_id.fetch_add(1, Ordering::SeqCst);
        } else {
            self.advance_next_id_past(signal.id);
        }

        self.push_signal(signal);
    }

    /// Import signals from another collector, remapping IDs into this collector's namespace.
    ///
    /// Any causal links that point at another imported signal are rewritten to the new IDs.
    /// Cause IDs that do not refer to a signal in the imported batch are preserved as-is.
    pub fn import_signals(&mut self, signals: &[Signal]) {
        let assigned_ids = signals
            .iter()
            .map(|_| self.next_id.fetch_add(1, Ordering::SeqCst))
            .collect::<Vec<_>>();
        let remapped_ids = signals
            .iter()
            .zip(assigned_ids.iter().copied())
            .filter_map(|(signal, new_id)| (signal.id != 0).then_some((signal.id, new_id)))
            .collect::<HashMap<_, _>>();

        for (signal, new_id) in signals.iter().zip(assigned_ids) {
            let mut imported = signal.clone();
            imported.id = new_id;
            if let Some(remapped_cause_id) = signal
                .cause_id
                .and_then(|cause_id| remapped_ids.get(&cause_id).copied())
            {
                imported.cause_id = Some(remapped_cause_id);
            }
            self.push_signal(imported);
        }
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
        self.emit(Signal::new(step, kind, message, metadata, timestamp_ms));
    }

    fn push_signal(&mut self, signal: Signal) {
        if self.signals.len() >= self.max_signals {
            self.drop_signal_for_capacity();
        }
        self.signals.push(signal);
    }

    fn advance_next_id_past(&self, id: u64) {
        let reserved_next = id.saturating_add(1);
        let mut current = self.next_id.load(Ordering::SeqCst);
        while current < reserved_next {
            match self.next_id.compare_exchange(
                current,
                reserved_next,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
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
        Signal::new(
            step,
            kind,
            message.to_string(),
            serde_json::json!({"test": true}),
            timestamp_ms,
        )
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
    fn signal_collector_preserves_existing_id_and_advances_counter() {
        let mut collector = SignalCollector::new(10);

        // Signal with pre-set ID is preserved
        let signal = mk_signal(LoopStep::Act, SignalKind::Success, "preset", 1).with_id(42);
        collector.emit(signal);

        assert_eq!(collector.signals()[0].id, 42);

        // Next signal continues after the highest explicit ID.
        collector.emit(mk_signal(LoopStep::Act, SignalKind::Success, "next", 2));
        assert_eq!(collector.signals()[1].id, 43);
        assert_eq!(collector.next_id(), 44);
    }

    #[test]
    fn import_signals_remaps_ids_and_causal_links() {
        let mut collector = SignalCollector::new(10);
        collector.emit(mk_signal(LoopStep::Act, SignalKind::Success, "parent", 1));

        let child_signals = vec![
            Signal::new(
                LoopStep::Act,
                SignalKind::Trace,
                "child-start",
                serde_json::json!({}),
                2,
            )
            .with_id(1),
            Signal::new(
                LoopStep::Act,
                SignalKind::Success,
                "child-done",
                serde_json::json!({}),
                3,
            )
            .with_id(2)
            .with_cause_id(1),
        ];

        collector.import_signals(&child_signals);

        let signals = collector.signals();
        assert_eq!(
            signals.iter().map(|signal| signal.id).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert_eq!(signals[1].message, "child-start");
        assert_eq!(signals[2].message, "child-done");
        assert_eq!(signals[2].cause_id, Some(2));
        assert_eq!(collector.next_id(), 4);
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
            Signal::new(
                LoopStep::Act,
                SignalKind::Success,
                "s1",
                serde_json::json!({}),
                1,
            )
            .with_id(5),
            Signal::new(
                LoopStep::Act,
                SignalKind::Success,
                "s2",
                serde_json::json!({}),
                2,
            )
            .with_id(10),
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
