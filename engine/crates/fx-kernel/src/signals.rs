pub use fx_core::signals::{
    ControlPlaneDecisionKind, LoopStep, Signal, SignalEvictionPriority, SignalKind, SignalSeverity,
    SignalToolClassification,
};

use std::collections::{HashMap, VecDeque};
use std::ops::Deref;
use std::sync::atomic::{AtomicU64, Ordering};

type SignalLaneIter<'a> = std::iter::Peekable<std::collections::vec_deque::Iter<'a, Signal>>;

#[derive(Debug, Clone, Copy)]
enum SignalLane {
    DropFirst,
    Normal,
    Keep,
    KeepStrong,
}

impl SignalLane {
    const fn from_priority(priority: SignalEvictionPriority) -> Self {
        match priority {
            SignalEvictionPriority::DropFirst => Self::DropFirst,
            SignalEvictionPriority::Normal => Self::Normal,
            SignalEvictionPriority::Keep => Self::Keep,
            SignalEvictionPriority::KeepStrong => Self::KeepStrong,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct SignalPriorityLanes {
    drop_first: VecDeque<Signal>,
    normal: VecDeque<Signal>,
    keep: VecDeque<Signal>,
    keep_strong: VecDeque<Signal>,
}

impl SignalPriorityLanes {
    fn from_signals(signals: Vec<Signal>) -> Self {
        let capacities = signals.iter().fold([0; 4], |mut capacities, signal| {
            capacities[Self::lane_slot(signal.kind.eviction_priority())] += 1;
            capacities
        });
        let mut lanes = Self {
            drop_first: VecDeque::with_capacity(capacities[0]),
            normal: VecDeque::with_capacity(capacities[1]),
            keep: VecDeque::with_capacity(capacities[2]),
            keep_strong: VecDeque::with_capacity(capacities[3]),
        };
        for signal in signals {
            lanes.push(signal);
        }
        lanes
    }

    fn push(&mut self, signal: Signal) {
        self.lane_mut(signal.kind.eviction_priority())
            .push_back(signal);
    }

    fn pop_oldest_evictable(&mut self) -> Option<Signal> {
        self.drop_first
            .pop_front()
            .or_else(|| self.normal.pop_front())
            .or_else(|| self.keep.pop_front())
            .or_else(|| self.keep_strong.pop_front())
    }

    fn drain_by_kind(&mut self, kind: SignalKind) -> Vec<Signal> {
        let mut drained = Vec::new();
        drain_kind_from_lane(&mut self.drop_first, kind, &mut drained);
        drain_kind_from_lane(&mut self.normal, kind, &mut drained);
        drain_kind_from_lane(&mut self.keep, kind, &mut drained);
        drain_kind_from_lane(&mut self.keep_strong, kind, &mut drained);
        sort_signals_by_timestamp(&mut drained);
        drained
    }

    fn drain_all(&mut self) -> Vec<Signal> {
        let mut all = Vec::with_capacity(self.len());
        all.extend(self.drop_first.drain(..));
        all.extend(self.normal.drain(..));
        all.extend(self.keep.drain(..));
        all.extend(self.keep_strong.drain(..));
        sort_signals_by_timestamp(&mut all);
        all
    }

    fn snapshot(&self) -> Vec<Signal> {
        self.iter_in_order().cloned().collect()
    }

    fn clear(&mut self) {
        self.drop_first.clear();
        self.normal.clear();
        self.keep.clear();
        self.keep_strong.clear();
    }

    fn len(&self) -> usize {
        self.drop_first.len() + self.normal.len() + self.keep.len() + self.keep_strong.len()
    }

    fn is_empty(&self) -> bool {
        self.drop_first.is_empty()
            && self.normal.is_empty()
            && self.keep.is_empty()
            && self.keep_strong.is_empty()
    }

    fn iter_in_order(&self) -> OrderedSignalIter<'_> {
        OrderedSignalIter {
            drop_first: self.drop_first.iter().peekable(),
            normal: self.normal.iter().peekable(),
            keep: self.keep.iter().peekable(),
            keep_strong: self.keep_strong.iter().peekable(),
        }
    }

    fn lane_mut(&mut self, priority: SignalEvictionPriority) -> &mut VecDeque<Signal> {
        match SignalLane::from_priority(priority) {
            SignalLane::DropFirst => &mut self.drop_first,
            SignalLane::Normal => &mut self.normal,
            SignalLane::Keep => &mut self.keep,
            SignalLane::KeepStrong => &mut self.keep_strong,
        }
    }

    const fn lane_slot(priority: SignalEvictionPriority) -> usize {
        match SignalLane::from_priority(priority) {
            SignalLane::DropFirst => 0,
            SignalLane::Normal => 1,
            SignalLane::Keep => 2,
            SignalLane::KeepStrong => 3,
        }
    }
}

/// Owned, slice-like snapshot of the collector's current signals.
#[derive(Debug, Clone)]
pub struct SignalSnapshot(Vec<Signal>);

impl SignalSnapshot {
    fn new(signals: Vec<Signal>) -> Self {
        Self(signals)
    }
}

impl Deref for SignalSnapshot {
    type Target = [Signal];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<[Signal]> for SignalSnapshot {
    fn as_ref(&self) -> &[Signal] {
        &self.0
    }
}

impl IntoIterator for SignalSnapshot {
    type Item = Signal;
    type IntoIter = std::vec::IntoIter<Signal>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a> IntoIterator for &'a SignalSnapshot {
    type Item = &'a Signal;
    type IntoIter = std::slice::Iter<'a, Signal>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[derive(Debug)]
struct OrderedSignalIter<'a> {
    drop_first: SignalLaneIter<'a>,
    normal: SignalLaneIter<'a>,
    keep: SignalLaneIter<'a>,
    keep_strong: SignalLaneIter<'a>,
}

impl<'a> OrderedSignalIter<'a> {
    fn peek_lane(&mut self, lane: SignalLane) -> Option<&'a Signal> {
        match lane {
            SignalLane::DropFirst => self.drop_first.peek().copied(),
            SignalLane::Normal => self.normal.peek().copied(),
            SignalLane::Keep => self.keep.peek().copied(),
            SignalLane::KeepStrong => self.keep_strong.peek().copied(),
        }
    }

    fn advance_lane(&mut self, lane: SignalLane) -> Option<&'a Signal> {
        match lane {
            SignalLane::DropFirst => self.drop_first.next(),
            SignalLane::Normal => self.normal.next(),
            SignalLane::Keep => self.keep.next(),
            SignalLane::KeepStrong => self.keep_strong.next(),
        }
    }
}

impl<'a> Iterator for OrderedSignalIter<'a> {
    type Item = &'a Signal;

    fn next(&mut self) -> Option<Self::Item> {
        let lanes = [
            SignalLane::DropFirst,
            SignalLane::Normal,
            SignalLane::Keep,
            SignalLane::KeepStrong,
        ];
        let next_lane = lanes
            .into_iter()
            .filter_map(|lane| self.peek_lane(lane).map(|signal| (lane, signal)))
            .min_by_key(|(_, signal)| signal_order_key(signal))
            .map(|(lane, _)| lane)?;

        self.advance_lane(next_lane)
    }
}

/// Accumulates signals for a single loop cycle.
#[derive(Debug)]
pub struct SignalCollector {
    lanes: SignalPriorityLanes,
    max_signals: usize,
    next_id: AtomicU64,
}

impl Clone for SignalCollector {
    fn clone(&self) -> Self {
        Self {
            lanes: self.lanes.clone(),
            max_signals: self.max_signals,
            next_id: AtomicU64::new(self.next_id.load(Ordering::SeqCst)),
        }
    }
}

impl Default for SignalCollector {
    fn default() -> Self {
        Self {
            lanes: SignalPriorityLanes::default(),
            max_signals: 200,
            next_id: AtomicU64::new(1),
        }
    }
}

impl SignalCollector {
    pub fn new(max_signals: usize) -> Self {
        Self {
            lanes: SignalPriorityLanes::default(),
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
            lanes: SignalPriorityLanes::from_signals(signals),
            next_id: AtomicU64::new(max_id + 1),
        }
    }

    /// Emit a signal. Drops oldest low-priority signals if at capacity.
    /// Assigns a monotonic ID to the signal.
    pub fn emit(&mut self, mut signal: Signal) -> u64 {
        if signal.id == Signal::UNASSIGNED_ID {
            signal.id = self.next_id.fetch_add(1, Ordering::SeqCst);
        } else {
            self.advance_next_id_past(signal.id);
        }

        let assigned_id = signal.id;
        self.push_signal(signal);
        assigned_id
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
    ) -> u64 {
        self.emit(Signal::new(step, kind, message, metadata, timestamp_ms))
    }

    fn push_signal(&mut self, signal: Signal) {
        if self.max_signals == 0 {
            return;
        }
        if self.len() >= self.max_signals {
            self.drop_signal_for_capacity();
        }
        self.lanes.push(signal);
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
        let _ = self.lanes.pop_oldest_evictable();
    }

    /// Drain signals by kind.
    pub fn drain_by_kind(&mut self, kind: SignalKind) -> Vec<Signal> {
        self.lanes.drain_by_kind(kind)
    }

    pub fn drain_all(&mut self) -> Vec<Signal> {
        self.lanes.drain_all()
    }

    /// All signals (read-only).
    pub fn signals(&self) -> SignalSnapshot {
        SignalSnapshot::new(self.lanes.snapshot())
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.lanes.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lanes.is_empty()
    }

    /// Condensed summary (max 5 lines).
    pub fn summary(&self) -> String {
        let mut friction_count = 0;
        let mut success_count = 0;
        let mut tool_count = 0;
        let mut last_friction = None;

        for signal in self.lanes.iter_in_order() {
            if signal.kind == SignalKind::Friction {
                friction_count += 1;
                last_friction = Some(signal.message.as_str());
            }
            if signal.kind == SignalKind::Success {
                success_count += 1;
            }
            if signal.step == LoopStep::Act {
                tool_count += 1;
            }
        }

        let mut lines = vec![format!("{} signals", self.len())];
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
        if let Some(last_friction) = last_friction {
            lines.push(format!("last friction: {last_friction}"));
        }

        lines.into_iter().take(5).collect::<Vec<_>>().join(" · ")
    }

    /// Full debug dump.
    pub fn debug_dump(&self) -> String {
        self.lanes
            .iter_in_order()
            .map(format_signal_debug_line)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Clear buffered signals for a new cycle.
    /// The monotonic ID counter is preserved so IDs are never reused within a
    /// collector lifetime.
    pub fn clear(&mut self) {
        self.lanes.clear();
    }

    /// Get the next ID that will be assigned (for testing/debugging).
    pub fn next_id(&self) -> u64 {
        self.next_id.load(Ordering::SeqCst)
    }
}

fn drain_kind_from_lane(lane: &mut VecDeque<Signal>, kind: SignalKind, drained: &mut Vec<Signal>) {
    lane.retain(|signal| {
        if signal.kind == kind {
            drained.push(signal.clone());
            false
        } else {
            true
        }
    });
}

fn sort_signals_by_timestamp(signals: &mut [Signal]) {
    signals.sort_by_key(|signal| (signal.timestamp_ms, signal.id));
}

fn signal_order_key(signal: &Signal) -> (u64, u64) {
    (signal.timestamp_ms, signal.id)
}

fn format_signal_debug_line(signal: &Signal) -> String {
    format!(
        "[{:?}/{:?}] {} (id={}, ts={})",
        signal.step, signal.kind, signal.message, signal.id, signal.timestamp_ms
    )
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

        let signals = collector.signals();
        let messages = signals
            .iter()
            .map(|signal| signal.message.as_str())
            .collect::<Vec<_>>();
        assert_eq!(messages, vec!["success-1", "friction-1", "friction-2"]);
    }

    #[test]
    fn signal_collector_drops_oldest_when_only_normal_priority_signals_remain() {
        let mut collector = SignalCollector::new(2);
        collector.emit(mk_signal(LoopStep::Act, SignalKind::Friction, "first", 1));
        collector.emit(mk_signal(LoopStep::Act, SignalKind::Friction, "second", 2));

        collector.emit(mk_signal(LoopStep::Act, SignalKind::Friction, "third", 3));

        let signals = collector.signals();
        let messages = signals
            .iter()
            .map(|signal| signal.message.as_str())
            .collect::<Vec<_>>();
        assert_eq!(messages, vec!["second", "third"]);
    }

    #[test]
    fn signal_collector_falls_back_to_normal_when_drop_first_lane_is_empty() {
        let mut collector = SignalCollector::new(3);
        collector.emit(mk_signal(
            LoopStep::Perceive,
            SignalKind::ContextOverflow,
            "overflow",
            1,
        ));
        collector.emit(mk_signal(LoopStep::Act, SignalKind::Timeout, "timeout", 2));
        collector.emit(mk_signal(LoopStep::Act, SignalKind::Retry, "retry-1", 3));

        collector.emit(mk_signal(LoopStep::Act, SignalKind::Retry, "retry-2", 4));

        let kinds = collector
            .signals()
            .iter()
            .map(|signal| signal.kind)
            .collect::<Vec<_>>();
        assert_eq!(
            kinds,
            vec![SignalKind::Timeout, SignalKind::Retry, SignalKind::Retry,]
        );
    }

    #[test]
    fn signal_collector_falls_back_to_keep_before_keep_strong() {
        let mut collector = SignalCollector::new(2);
        collector.emit(mk_signal(LoopStep::Act, SignalKind::Retry, "retry", 1));
        collector.emit(mk_signal(LoopStep::Act, SignalKind::Timeout, "timeout", 2));

        collector.emit(mk_signal(
            LoopStep::Act,
            SignalKind::ProviderFallback,
            "fallback",
            3,
        ));

        let signals = collector.signals();
        let messages = signals
            .iter()
            .map(|signal| signal.message.as_str())
            .collect::<Vec<_>>();
        assert_eq!(messages, vec!["timeout", "fallback"]);
    }

    #[test]
    fn signal_collector_drain_all_returns_timestamp_order_across_lanes() {
        let mut collector = SignalCollector::new(10);
        collector.emit(mk_signal(LoopStep::Act, SignalKind::Timeout, "timeout", 30));
        collector.emit(mk_signal(LoopStep::Act, SignalKind::Cost, "cost", 10));
        collector.emit(mk_signal(LoopStep::Act, SignalKind::Retry, "retry", 20));

        let drained = collector.drain_all();

        let messages = drained
            .iter()
            .map(|signal| signal.message.as_str())
            .collect::<Vec<_>>();
        assert_eq!(messages, vec!["cost", "retry", "timeout"]);
        assert!(collector.is_empty());
    }

    #[test]
    fn signal_collector_len_and_is_empty_track_live_signals() {
        let mut collector = SignalCollector::new(3);
        assert!(collector.is_empty());
        assert_eq!(collector.len(), 0);

        collector.emit(mk_signal(LoopStep::Act, SignalKind::Trace, "trace", 1));
        collector.emit(mk_signal(LoopStep::Act, SignalKind::Success, "success", 2));

        assert_eq!(collector.len(), 2);
        assert!(!collector.is_empty());

        let drained = collector.drain_by_kind(SignalKind::Trace);
        assert_eq!(drained.len(), 1);
        assert_eq!(collector.len(), 1);

        let remaining = collector.drain_all();
        assert_eq!(remaining.len(), 1);
        assert!(collector.is_empty());
        assert_eq!(collector.len(), 0);
    }

    #[test]
    fn signal_collector_preserves_monotonic_ids_under_capacity_eviction() {
        let mut collector = SignalCollector::new(2);

        assert_eq!(
            collector.emit(mk_signal(LoopStep::Act, SignalKind::Trace, "trace", 1)),
            1
        );
        assert_eq!(
            collector.emit(mk_signal(LoopStep::Act, SignalKind::Retry, "retry", 2)),
            2
        );
        assert_eq!(
            collector.emit(mk_signal(LoopStep::Act, SignalKind::Timeout, "timeout", 3)),
            3
        );

        let ids = collector
            .signals()
            .iter()
            .map(|signal| signal.id)
            .collect::<Vec<_>>();
        assert_eq!(ids, vec![2, 3]);
        assert_eq!(collector.next_id(), 4);
    }

    #[test]
    fn signal_collector_handles_high_eviction_volume() {
        let mut collector = SignalCollector::new(500);

        for index in 0..1_000 {
            let kind = if index % 2 == 0 {
                SignalKind::Cost
            } else {
                SignalKind::Timeout
            };
            collector.emit(mk_signal(
                LoopStep::Act,
                kind,
                &format!("signal-{index}"),
                index as u64,
            ));
        }

        assert_eq!(collector.len(), 500);
        assert!(collector
            .signals()
            .iter()
            .all(|signal| signal.kind == SignalKind::Timeout));
    }

    #[test]
    fn signal_collector_with_zero_capacity_drops_all_signals() {
        let mut collector = SignalCollector::new(0);

        collector.emit(mk_signal(LoopStep::Act, SignalKind::Success, "ignored", 1));

        assert!(collector.is_empty());
        assert_eq!(collector.len(), 0);
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
        assert_eq!(collector.len(), 1);
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
    fn signal_snapshot_supports_owned_iteration() {
        let mut collector = SignalCollector::new(2);
        collector.emit(mk_signal(LoopStep::Act, SignalKind::Trace, "first", 1));
        collector.emit(mk_signal(LoopStep::Act, SignalKind::Success, "second", 2));

        let messages = collector
            .signals()
            .into_iter()
            .map(|signal| signal.message)
            .collect::<Vec<_>>();

        assert_eq!(messages, vec!["first", "second"]);
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
    fn clear_resets_signals_without_reusing_ids() {
        let mut collector = SignalCollector::new(10);
        collector.emit(mk_signal(LoopStep::Act, SignalKind::Success, "s1", 1));
        collector.emit(mk_signal(LoopStep::Act, SignalKind::Success, "s2", 2));

        assert_eq!(collector.len(), 2);
        assert_eq!(collector.next_id(), 3);

        collector.clear();

        assert!(collector.is_empty());
        assert_eq!(collector.next_id(), 3);

        collector.emit(mk_signal(LoopStep::Act, SignalKind::Success, "s3", 3));
        assert_eq!(collector.signals()[0].id, 3);
        assert_eq!(collector.next_id(), 4);
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
