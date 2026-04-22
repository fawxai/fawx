//! fx-canary — signal health monitor for the Fawx agentic engine.
//!
//! Captures multi-dimensional health baselines, evaluates current health
//! against those baselines, and returns verdicts that drive warnings or
//! rollback recommendations.
//!
//! Pure computation — no I/O, no file writes, no network.

mod health;
mod monitor;
mod time;
mod trigger;
mod window;

pub use health::{
    summarize_degraded_dimensions, DegradedDimension, HealthDimension, HealthThresholds,
    HealthVector,
};
pub use monitor::CanaryMonitor;
pub use trigger::{RipcordTrigger, RollbackError, RollbackPolicy, RollbackReason, RollbackTrigger};
pub use window::SignalWindow;

pub(crate) use time::current_epoch_secs;

use serde::{Deserialize, Serialize};

/// Baseline or current health captured for a rolling signal window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HealthSnapshot {
    pub captured_at: u64,
    pub window_seconds: u64,
    pub total_signals: u64,
    pub cycle_count: u64,
    pub health: HealthVector,
}

/// Degradation verdict emitted by the canary.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// Signal quality is stable or improved.
    Healthy,
    /// Some degradation is present, but not enough to justify rollback.
    Warning {
        message: String,
        degraded_dimensions: Vec<DegradedDimension>,
    },
    /// Significant degradation — recommend rollback.
    Degraded {
        message: String,
        degraded_dimensions: Vec<DegradedDimension>,
        rollback_recommended: bool,
    },
}

/// Canary configuration with sensible defaults.
#[derive(Debug, Clone)]
pub struct CanaryConfig {
    /// Minimum signals needed before baseline is meaningful.
    pub min_signals_for_baseline: u64,
    /// Per-dimension degradation thresholds.
    pub health_thresholds: HealthThresholds,
    /// Typed rollback policy based on degraded-dimension severity counts.
    pub rollback_policy: RollbackPolicy,
    /// Time window for signal collection (seconds).
    pub window_seconds: u64,
}

impl Default for CanaryConfig {
    fn default() -> Self {
        Self {
            min_signals_for_baseline: 20,
            health_thresholds: HealthThresholds::default(),
            rollback_policy: RollbackPolicy::default(),
            window_seconds: 3600,
        }
    }
}

/// The canary monitor core.
pub struct Canary {
    config: CanaryConfig,
    baseline: Option<HealthSnapshot>,
}

impl Canary {
    /// Create a new canary with configuration.
    pub fn new(config: CanaryConfig) -> Self {
        Self {
            config,
            baseline: None,
        }
    }

    /// Create with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(CanaryConfig::default())
    }

    /// Capture a baseline from a health snapshot.
    pub fn capture_baseline(&mut self, baseline: HealthSnapshot) {
        self.baseline = Some(baseline);
    }

    /// Compare current health against baseline, return verdict.
    pub fn evaluate(&self, current: &HealthSnapshot) -> Verdict {
        let baseline = match &self.baseline {
            Some(baseline) => baseline,
            None => return Verdict::Healthy,
        };

        if current.total_signals < self.config.min_signals_for_baseline {
            return Verdict::Healthy;
        }

        let degraded_dimensions = current
            .health
            .evaluate(&baseline.health, &self.config.health_thresholds);
        if degraded_dimensions.is_empty() {
            return Verdict::Healthy;
        }

        let message = format!(
            "{} degraded dimension{}: {}",
            degraded_dimensions.len(),
            if degraded_dimensions.len() == 1 {
                ""
            } else {
                "s"
            },
            summarize_degraded_dimensions(&degraded_dimensions)
        );

        if self
            .config
            .rollback_policy
            .should_trigger(&degraded_dimensions)
        {
            Verdict::Degraded {
                message,
                degraded_dimensions,
                rollback_recommended: true,
            }
        } else {
            Verdict::Warning {
                message,
                degraded_dimensions,
            }
        }
    }

    /// Get the current baseline (if captured).
    pub fn baseline(&self) -> Option<&HealthSnapshot> {
        self.baseline.as_ref()
    }

    /// Check if we have enough signals for a meaningful baseline.
    pub fn has_sufficient_baseline(&self) -> bool {
        self.baseline
            .as_ref()
            .is_some_and(|baseline| baseline.total_signals >= self.config.min_signals_for_baseline)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(success_rate: f64, retry_rate: Option<f64>, total_signals: u64) -> HealthSnapshot {
        HealthSnapshot {
            captured_at: 1,
            window_seconds: 60,
            total_signals,
            cycle_count: 3,
            health: HealthVector {
                success_rate: Some(success_rate),
                retry_rate,
                ..HealthVector::default()
            },
        }
    }

    #[test]
    fn evaluate_healthy_when_no_baseline() {
        let canary = Canary::with_defaults();
        assert_eq!(
            canary.evaluate(&snapshot(0.8, Some(0.1), 20)),
            Verdict::Healthy
        );
    }

    #[test]
    fn evaluate_healthy_when_insufficient_data() {
        let mut canary = Canary::with_defaults();
        canary.capture_baseline(snapshot(0.9, Some(0.0), 20));

        assert_eq!(
            canary.evaluate(&snapshot(0.3, Some(0.8), 5)),
            Verdict::Healthy
        );
    }

    #[test]
    fn evaluate_returns_warning_for_single_dimension_regression() {
        let mut canary = Canary::with_defaults();
        canary.capture_baseline(snapshot(0.9, Some(0.0), 20));

        let verdict = canary.evaluate(&snapshot(0.9, Some(0.4), 20));

        match verdict {
            Verdict::Warning {
                degraded_dimensions,
                ..
            } => {
                assert_eq!(degraded_dimensions.len(), 1);
                assert_eq!(degraded_dimensions[0].dimension, HealthDimension::RetryRate);
            }
            other => panic!("expected Warning, got {other:?}"),
        }
    }

    #[test]
    fn evaluate_returns_degraded_for_multi_dimension_regression() {
        let mut canary = Canary::with_defaults();
        canary.capture_baseline(HealthSnapshot {
            captured_at: 1,
            window_seconds: 60,
            total_signals: 40,
            cycle_count: 4,
            health: HealthVector {
                success_rate: Some(0.9),
                friction_rate: Some(0.05),
                avg_latency_ms: Some(120.0),
                retry_rate: Some(0.0),
                ..HealthVector::default()
            },
        });

        let verdict = canary.evaluate(&HealthSnapshot {
            captured_at: 2,
            window_seconds: 60,
            total_signals: 40,
            cycle_count: 4,
            health: HealthVector {
                success_rate: Some(0.6),
                friction_rate: Some(0.2),
                avg_latency_ms: Some(240.0),
                retry_rate: Some(0.4),
                ..HealthVector::default()
            },
        });

        match verdict {
            Verdict::Degraded {
                rollback_recommended,
                degraded_dimensions,
                ..
            } => {
                assert!(rollback_recommended);
                assert!(degraded_dimensions.len() >= 3);
            }
            other => panic!("expected Degraded, got {other:?}"),
        }
    }

    #[test]
    fn capture_baseline_stores_health_vector() {
        let mut canary = Canary::with_defaults();
        let baseline = snapshot(0.8, Some(0.1), 20);
        canary.capture_baseline(baseline.clone());

        assert_eq!(canary.baseline(), Some(&baseline));
    }

    #[test]
    fn has_sufficient_baseline_checks_min_signals() {
        let mut canary = Canary::with_defaults();
        assert!(!canary.has_sufficient_baseline());

        canary.capture_baseline(snapshot(0.8, Some(0.1), 10));
        assert!(!canary.has_sufficient_baseline());

        canary.capture_baseline(snapshot(0.8, Some(0.1), 20));
        assert!(canary.has_sufficient_baseline());
    }
}
