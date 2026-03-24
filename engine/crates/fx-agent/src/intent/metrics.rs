//! Metrics and telemetry for intent classification.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Thread-safe metrics for intent classification.
///
/// Tracks classification performance and patterns:
/// - Total number of classifications performed
/// - Average confidence scores
/// - Average classification latency
/// - Fallback count (low-confidence or timeout cases)
#[derive(Debug, Clone)]
pub struct IntentMetrics {
    inner: Arc<MetricsInner>,
}

#[derive(Debug)]
struct MetricsInner {
    /// Total number of classifications
    total_classifications: AtomicUsize,

    /// Sum of all confidence scores (to calculate average)
    /// Stored as (confidence * 1000) to use integer atomics
    sum_confidence_millis: AtomicU64,

    /// Sum of all classification latencies in microseconds
    sum_latency_micros: AtomicU64,

    /// Number of times fallback was triggered (low confidence or timeout)
    fallback_count: AtomicUsize,
}

impl Default for IntentMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl IntentMetrics {
    /// Create a new metrics instance.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(MetricsInner {
                total_classifications: AtomicUsize::new(0),
                sum_confidence_millis: AtomicU64::new(0),
                sum_latency_micros: AtomicU64::new(0),
                fallback_count: AtomicUsize::new(0),
            }),
        }
    }

    /// Record a successful classification.
    ///
    /// # Arguments
    /// * `confidence` - Classification confidence score (0.0-1.0)
    /// * `latency` - Time taken for classification
    /// * `was_fallback` - Whether this was a fallback classification (low confidence or timeout)
    ///
    /// # Memory Ordering
    /// Uses `Ordering::Relaxed` for all atomic operations because:
    /// - Metrics are aggregate statistics, not control flow data
    /// - No inter-thread synchronization is required
    /// - Eventual consistency is acceptable for metrics
    /// - Provides maximum performance (no memory barriers)
    pub fn record_classification(&self, confidence: f32, latency: Duration, was_fallback: bool) {
        // Increment total count
        self.inner
            .total_classifications
            .fetch_add(1, Ordering::Relaxed);

        // Add confidence (stored as millis for atomic operations)
        let confidence_millis = (confidence * 1000.0) as u64;
        self.inner
            .sum_confidence_millis
            .fetch_add(confidence_millis, Ordering::Relaxed);

        // Add latency
        let latency_micros = latency.as_micros() as u64;
        self.inner
            .sum_latency_micros
            .fetch_add(latency_micros, Ordering::Relaxed);

        // Increment fallback count if applicable
        if was_fallback {
            self.inner.fallback_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Get a snapshot of current metrics.
    pub fn get_snapshot(&self) -> MetricsSnapshot {
        let total = self.inner.total_classifications.load(Ordering::Relaxed);
        let sum_confidence_millis = self.inner.sum_confidence_millis.load(Ordering::Relaxed);
        let sum_latency_micros = self.inner.sum_latency_micros.load(Ordering::Relaxed);
        let fallbacks = self.inner.fallback_count.load(Ordering::Relaxed);

        let avg_confidence = if total > 0 {
            (sum_confidence_millis as f64 / total as f64) / 1000.0
        } else {
            0.0
        };

        let avg_latency = if total > 0 {
            Duration::from_micros(sum_latency_micros / total as u64)
        } else {
            Duration::from_secs(0)
        };

        MetricsSnapshot {
            total_classifications: total,
            average_confidence: avg_confidence,
            average_latency: avg_latency,
            fallback_count: fallbacks,
        }
    }

    /// Reset all metrics to zero.
    ///
    /// Useful for testing or periodic metric resets.
    pub fn reset(&self) {
        self.inner.total_classifications.store(0, Ordering::Relaxed);
        self.inner.sum_confidence_millis.store(0, Ordering::Relaxed);
        self.inner.sum_latency_micros.store(0, Ordering::Relaxed);
        self.inner.fallback_count.store(0, Ordering::Relaxed);
    }
}

/// Snapshot of intent classification metrics at a point in time.
///
/// All values are computed from atomic counters and represent
/// aggregate statistics since the last reset.
#[derive(Debug, Clone, PartialEq)]
pub struct MetricsSnapshot {
    /// Total number of classifications performed
    pub total_classifications: usize,

    /// Average confidence score across all classifications (0.0-1.0)
    pub average_confidence: f64,

    /// Average time taken for classification
    pub average_latency: Duration,

    /// Number of fallback classifications (low confidence or timeout)
    pub fallback_count: usize,
}

impl MetricsSnapshot {
    /// Calculate the fallback rate as a percentage.
    ///
    /// Returns 0.0 if no classifications have been performed.
    pub fn fallback_rate(&self) -> f64 {
        if self.total_classifications == 0 {
            0.0
        } else {
            (self.fallback_count as f64 / self.total_classifications as f64) * 100.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_metrics_are_zero() {
        let metrics = IntentMetrics::new();
        let snapshot = metrics.get_snapshot();

        assert_eq!(snapshot.total_classifications, 0);
        assert_eq!(snapshot.average_confidence, 0.0);
        assert_eq!(snapshot.average_latency, Duration::from_secs(0));
        assert_eq!(snapshot.fallback_count, 0);
    }

    #[test]
    fn test_record_single_classification() {
        let metrics = IntentMetrics::new();

        metrics.record_classification(0.85, Duration::from_millis(100), false);

        let snapshot = metrics.get_snapshot();
        assert_eq!(snapshot.total_classifications, 1);
        assert!((snapshot.average_confidence - 0.85).abs() < 0.001);
        assert_eq!(snapshot.average_latency, Duration::from_millis(100));
        assert_eq!(snapshot.fallback_count, 0);
    }

    #[test]
    fn test_record_multiple_classifications() {
        let metrics = IntentMetrics::new();

        metrics.record_classification(0.9, Duration::from_millis(50), false);
        metrics.record_classification(0.8, Duration::from_millis(150), false);
        metrics.record_classification(0.7, Duration::from_millis(100), true);

        let snapshot = metrics.get_snapshot();
        assert_eq!(snapshot.total_classifications, 3);

        // Average confidence: (0.9 + 0.8 + 0.7) / 3 = 0.8
        assert!((snapshot.average_confidence - 0.8).abs() < 0.001);

        // Average latency: (50 + 150 + 100) / 3 = 100ms
        assert_eq!(snapshot.average_latency, Duration::from_millis(100));

        assert_eq!(snapshot.fallback_count, 1);
    }

    #[test]
    fn test_fallback_tracking() {
        let metrics = IntentMetrics::new();

        metrics.record_classification(0.95, Duration::from_millis(50), false);
        metrics.record_classification(0.3, Duration::from_millis(50), true);
        metrics.record_classification(0.4, Duration::from_millis(50), true);

        let snapshot = metrics.get_snapshot();
        assert_eq!(snapshot.total_classifications, 3);
        assert_eq!(snapshot.fallback_count, 2);
    }

    #[test]
    fn test_metrics_reset() {
        let metrics = IntentMetrics::new();

        metrics.record_classification(0.9, Duration::from_millis(100), false);
        metrics.record_classification(0.8, Duration::from_millis(200), true);

        let snapshot_before = metrics.get_snapshot();
        assert_eq!(snapshot_before.total_classifications, 2);
        assert_eq!(snapshot_before.fallback_count, 1);

        metrics.reset();

        let snapshot_after = metrics.get_snapshot();
        assert_eq!(snapshot_after.total_classifications, 0);
        assert_eq!(snapshot_after.average_confidence, 0.0);
        assert_eq!(snapshot_after.average_latency, Duration::from_secs(0));
        assert_eq!(snapshot_after.fallback_count, 0);
    }

    #[test]
    fn test_thread_safety() {
        use std::thread;

        let metrics = IntentMetrics::new();

        let mut handles = vec![];

        // Spawn 10 threads, each recording 100 classifications
        for _ in 0..10 {
            let metrics_clone = metrics.clone();
            handles.push(thread::spawn(move || {
                for i in 0..100 {
                    let confidence = 0.7 + (i as f32 / 1000.0);
                    let fallback = i % 10 == 0;
                    metrics_clone.record_classification(
                        confidence,
                        Duration::from_millis(50),
                        fallback,
                    );
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        let snapshot = metrics.get_snapshot();
        assert_eq!(snapshot.total_classifications, 1000); // 10 threads * 100 classifications
        assert_eq!(snapshot.fallback_count, 100); // 10 threads * 10 fallbacks each
    }

    #[test]
    fn test_fallback_rate_calculation() {
        let metrics = IntentMetrics::new();

        metrics.record_classification(0.9, Duration::from_millis(50), false);
        metrics.record_classification(0.3, Duration::from_millis(50), true);
        metrics.record_classification(0.8, Duration::from_millis(50), false);
        metrics.record_classification(0.2, Duration::from_millis(50), true);

        let snapshot = metrics.get_snapshot();
        assert_eq!(snapshot.total_classifications, 4);
        assert_eq!(snapshot.fallback_count, 2);
        assert!((snapshot.fallback_rate() - 50.0).abs() < 0.01); // 2/4 = 50%
    }

    #[test]
    fn test_fallback_rate_with_no_classifications() {
        let metrics = IntentMetrics::new();
        let snapshot = metrics.get_snapshot();
        assert_eq!(snapshot.fallback_rate(), 0.0);
    }

    #[test]
    fn test_precision_of_confidence_tracking() {
        let metrics = IntentMetrics::new();

        // Test with various confidence values to ensure precision
        metrics.record_classification(0.123, Duration::from_millis(10), false);
        metrics.record_classification(0.456, Duration::from_millis(10), false);
        metrics.record_classification(0.789, Duration::from_millis(10), false);

        let snapshot = metrics.get_snapshot();
        let expected_avg = (0.123 + 0.456 + 0.789) / 3.0;
        assert!((snapshot.average_confidence - expected_avg).abs() < 0.001);
    }

    #[test]
    fn test_latency_precision() {
        let metrics = IntentMetrics::new();

        metrics.record_classification(0.8, Duration::from_micros(1234), false);
        metrics.record_classification(0.8, Duration::from_micros(5678), false);

        let snapshot = metrics.get_snapshot();
        let expected_avg_micros = (1234 + 5678) / 2;
        assert_eq!(
            snapshot.average_latency,
            Duration::from_micros(expected_avg_micros)
        );
    }
}
