//! Event system for skill execution.
//!
//! Skills emit events during execution that are buffered in an [`EventCollector`]
//! and processed after the skill completes.

use serde::{Deserialize, Serialize};

/// Event emitted by a skill during execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillEvent {
    /// Name of the skill that emitted this event
    pub source_skill: String,
    /// Event type: "message", "status", "error", "metric"
    pub event_type: String,
    /// JSON payload
    pub payload: String,
    /// Timestamp in milliseconds since epoch
    pub timestamp_ms: u64,
}

/// Collects events emitted during skill execution.
///
/// Events are buffered here and drained after the skill finishes.
pub struct EventCollector {
    events: Vec<SkillEvent>,
}

impl EventCollector {
    /// Create a new empty collector.
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    /// Buffer an event.
    pub fn emit(&mut self, event: SkillEvent) {
        self.events.push(event);
    }

    /// Drain all collected events, leaving the collector empty.
    pub fn drain(&mut self) -> Vec<SkillEvent> {
        std::mem::take(&mut self.events)
    }

    /// Number of buffered events.
    pub fn count(&self) -> usize {
        self.events.len()
    }
}

impl Default for EventCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_event(source: &str, etype: &str) -> SkillEvent {
        SkillEvent {
            source_skill: source.to_string(),
            event_type: etype.to_string(),
            payload: r#"{"key":"value"}"#.to_string(),
            timestamp_ms: 1_700_000_000_000,
        }
    }

    #[test]
    fn emit_event_collects() {
        let mut c = EventCollector::new();
        assert_eq!(c.count(), 0);
        c.emit(make_event("weather", "status"));
        assert_eq!(c.count(), 1);
        c.emit(make_event("weather", "metric"));
        assert_eq!(c.count(), 2);
    }

    #[test]
    fn event_collector_drain() {
        let mut c = EventCollector::new();
        c.emit(make_event("skill_a", "message"));
        c.emit(make_event("skill_b", "error"));
        let events = c.drain();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].source_skill, "skill_a");
        assert_eq!(events[1].event_type, "error");
        assert_eq!(c.count(), 0);
        assert!(c.drain().is_empty());
    }
}
