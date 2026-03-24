use crate::consent::TelemetryConsent;
use crate::{SignalCategory, TelemetryError, TelemetrySignal};
use chrono::Utc;
use std::path::PathBuf;
use std::sync::RwLock;
use uuid::Uuid;

const DEFAULT_MAX_BUFFER_SIZE: usize = 10_000;

/// Collects telemetry signals in memory, respecting consent.
pub struct SignalCollector {
    consent: RwLock<TelemetryConsent>,
    buffer: RwLock<Vec<TelemetrySignal>>,
    session_id: String,
    max_buffer_size: usize,
    data_dir: Option<PathBuf>,
}

impl SignalCollector {
    pub fn new(consent: TelemetryConsent) -> Self {
        Self::build(consent, None)
    }

    pub fn new_with_persistence(consent: TelemetryConsent, data_dir: PathBuf) -> Self {
        Self::build(consent, Some(data_dir))
    }

    fn build(consent: TelemetryConsent, data_dir: Option<PathBuf>) -> Self {
        Self {
            consent: RwLock::new(consent),
            buffer: RwLock::new(Vec::new()),
            session_id: Uuid::new_v4().to_string(),
            max_buffer_size: DEFAULT_MAX_BUFFER_SIZE,
            data_dir,
        }
    }

    pub fn with_max_buffer(mut self, max: usize) -> Self {
        self.max_buffer_size = max;
        self
    }

    /// Record a signal. Silently dropped if category not consented.
    ///
    /// Holds both consent and buffer locks atomically to prevent TOCTOU
    /// races where consent is revoked between the check and the write.
    pub fn record(&self, category: SignalCategory, event: &str, value: serde_json::Value) {
        let consent = match self.consent.read() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if !consent.is_category_enabled(&category) {
            return;
        }
        // Hold consent lock while acquiring buffer lock to prevent
        // a concurrent update_consent from revoking between check and write.
        let mut buffer = match self.buffer.write() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        // Re-check after acquiring buffer lock (consent may have been
        // revoked if another thread upgraded to write between our read
        // and the buffer write acquisition — belt and suspenders).
        if !consent.is_category_enabled(&category) {
            return;
        }
        drop(consent);
        if buffer.len() < self.max_buffer_size {
            buffer.push(TelemetrySignal {
                id: Uuid::new_v4(),
                category,
                event: event.to_owned(),
                value,
                timestamp: Utc::now(),
                session_id: self.session_id.clone(),
            });
        }
    }

    /// Drain the buffer.
    pub fn drain(&self) -> Vec<TelemetrySignal> {
        let mut buffer = match self.buffer.write() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        std::mem::take(&mut *buffer)
    }

    /// Current buffer size.
    pub fn pending_count(&self) -> usize {
        let buffer = match self.buffer.read() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        buffer.len()
    }

    /// Update consent. Drops buffered signals for newly-disabled categories.
    /// Persists to disk while locks are held to prevent TOCTOU races.
    pub fn update_consent(&self, new_consent: TelemetryConsent) -> Result<(), TelemetryError> {
        let mut buffer = match self.buffer.write() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let mut consent = match self.consent.write() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        // Persist before mutating in-memory state so a failed save
        // doesn't leave memory diverged from disk.
        if let Some(ref data_dir) = self.data_dir {
            new_consent.save(data_dir)?;
        }
        buffer.retain(|signal| new_consent.is_category_enabled(&signal.category));
        *consent = new_consent;
        Ok(())
    }

    /// Get current consent state.
    pub fn consent(&self) -> TelemetryConsent {
        let consent = match self.consent.read() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        consent.clone()
    }

    /// Session ID for this collector instance.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn enabled_consent() -> TelemetryConsent {
        let mut consent = TelemetryConsent::default();
        consent.enable_all();
        consent
    }

    #[test]
    fn record_respects_consent() {
        let collector = SignalCollector::new(TelemetryConsent::default());
        collector.record(
            SignalCategory::ToolUsage,
            "tool_call",
            json!({"tool": "read_file"}),
        );
        assert_eq!(collector.pending_count(), 0);
    }

    #[test]
    fn record_stores_when_enabled() {
        let collector = SignalCollector::new(enabled_consent());
        collector.record(
            SignalCategory::ToolUsage,
            "tool_call",
            json!({"tool": "read_file"}),
        );
        assert_eq!(collector.pending_count(), 1);
    }

    #[test]
    fn drain_clears_buffer() {
        let collector = SignalCollector::new(enabled_consent());
        collector.record(SignalCategory::Errors, "error", json!({"code": 500}));
        collector.record(SignalCategory::Errors, "error", json!({"code": 404}));
        let signals = collector.drain();
        assert_eq!(signals.len(), 2);
        assert_eq!(collector.pending_count(), 0);
    }

    #[test]
    fn max_buffer_enforced() {
        let collector = SignalCollector::new(enabled_consent()).with_max_buffer(2);
        for i in 0..5 {
            collector.record(SignalCategory::Performance, "tick", json!({"i": i}));
        }
        assert_eq!(collector.pending_count(), 2);
    }

    #[test]
    fn update_consent_drops_disabled_signals() {
        let collector = SignalCollector::new(enabled_consent());
        collector.record(SignalCategory::ToolUsage, "a", json!({}));
        collector.record(SignalCategory::Errors, "b", json!({}));

        let mut new_consent = enabled_consent();
        new_consent.disable_category(SignalCategory::ToolUsage);
        collector
            .update_consent(new_consent)
            .expect("consent update should succeed");

        assert_eq!(collector.pending_count(), 1);
        let signals = collector.drain();
        assert_eq!(signals[0].category, SignalCategory::Errors);
    }

    #[test]
    fn session_id_is_stable() {
        let collector = SignalCollector::new(enabled_consent());
        let id1 = collector.session_id().to_owned();
        let id2 = collector.session_id().to_owned();
        assert_eq!(id1, id2);
    }

    #[test]
    fn consent_returns_current_state() {
        let collector = SignalCollector::new(TelemetryConsent::default());
        assert!(!collector.consent().enabled);
        let new_consent = TelemetryConsent {
            enabled: true,
            ..TelemetryConsent::default()
        };
        collector
            .update_consent(new_consent)
            .expect("consent update should succeed");
        assert!(collector.consent().enabled);
    }

    #[test]
    fn category_specific_consent() {
        let mut consent = TelemetryConsent {
            enabled: true,
            ..TelemetryConsent::default()
        };
        consent.enable_category(SignalCategory::Errors);
        let collector = SignalCollector::new(consent);

        collector.record(SignalCategory::Errors, "err", json!({}));
        collector.record(SignalCategory::ToolUsage, "tool", json!({}));

        assert_eq!(collector.pending_count(), 1);
    }
}
