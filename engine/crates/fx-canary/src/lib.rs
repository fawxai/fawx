//! fx-canary — Signal quality monitor for the Fawx agentic engine.
//!
//! Captures signal baselines (success/friction/decision ratios), evaluates
//! current signals against baseline, and returns verdicts: Healthy, Warning,
//! or Degraded (with rollback recommendation).
//!
//! Pure computation — no I/O, no file writes, no network.

mod monitor;
mod time;
mod trigger;
mod window;

pub use monitor::CanaryMonitor;
pub use trigger::{RipcordTrigger, RollbackError, RollbackReason, RollbackTrigger};
pub use window::SignalWindow;

pub(crate) use time::current_epoch_secs;

use fx_kernel::{Signal, SignalKind};
use serde::{Deserialize, Serialize};

/// Captured signal ratios at a point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalBaseline {
    pub captured_at: u64,
    pub window_seconds: u64,
    pub total_signals: u64,
    pub success_count: u64,
    pub friction_count: u64,
    pub decision_count: u64,
    pub avg_friction_severity: f64,
    pub success_rate: f64,
    pub friction_rate: f64,
}

/// Degradation verdict emitted by the canary.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// Signal quality is stable or improved.
    Healthy,
    /// Minor degradation detected, within tolerance.
    Warning { message: String },
    /// Significant degradation — recommend rollback.
    Degraded {
        message: String,
        rollback_recommended: bool,
    },
}

/// Canary configuration with sensible defaults.
#[derive(Debug, Clone)]
pub struct CanaryConfig {
    /// Minimum signals needed before baseline is meaningful.
    pub min_signals_for_baseline: u64,
    /// Success rate drop threshold for Warning (e.g., 0.10 = 10% drop).
    pub warning_threshold: f64,
    /// Success rate drop threshold for Degraded (e.g., 0.25 = 25% drop).
    pub degraded_threshold: f64,
    /// Friction rate increase threshold for Degraded.
    pub friction_increase_threshold: f64,
    /// Time window for signal collection (seconds).
    pub window_seconds: u64,
}

impl Default for CanaryConfig {
    fn default() -> Self {
        Self {
            min_signals_for_baseline: 20,
            warning_threshold: 0.10,
            degraded_threshold: 0.25,
            friction_increase_threshold: 0.20,
            window_seconds: 3600,
        }
    }
}

/// The canary monitor.
pub struct Canary {
    config: CanaryConfig,
    baseline: Option<SignalBaseline>,
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

    /// Capture a baseline from a slice of signals.
    pub fn capture_baseline(&mut self, signals: &[Signal], window_seconds: u64) {
        let mut baseline = compute_ratios(signals);
        baseline.window_seconds = window_seconds;
        self.baseline = Some(baseline);
    }

    /// Compare current signals against baseline, return verdict.
    pub fn evaluate(&self, current_signals: &[Signal]) -> Verdict {
        let baseline = match &self.baseline {
            Some(b) => b,
            None => return Verdict::Healthy,
        };

        let current_len = current_signals.len() as u64;
        if current_len < self.config.min_signals_for_baseline {
            return Verdict::Healthy;
        }

        let current = compute_ratios(current_signals);
        classify_degradation(&self.config, baseline, &current)
    }

    /// Get the current baseline (if captured).
    pub fn baseline(&self) -> Option<&SignalBaseline> {
        self.baseline.as_ref()
    }

    /// Check if we have enough signals for a meaningful baseline.
    pub fn has_sufficient_baseline(&self) -> bool {
        self.baseline
            .as_ref()
            .is_some_and(|b| b.total_signals >= self.config.min_signals_for_baseline)
    }
}

/// Classify degradation by comparing current ratios against baseline.
fn classify_degradation(
    config: &CanaryConfig,
    baseline: &SignalBaseline,
    current: &SignalBaseline,
) -> Verdict {
    let success_drop = baseline.success_rate - current.success_rate;
    let friction_increase = current.friction_rate - baseline.friction_rate;

    if success_drop >= config.degraded_threshold {
        return Verdict::Degraded {
            message: format!(
                "success rate dropped {:.1}% (baseline {:.1}% → current {:.1}%)",
                success_drop * 100.0,
                baseline.success_rate * 100.0,
                current.success_rate * 100.0,
            ),
            rollback_recommended: true,
        };
    }

    if friction_increase >= config.friction_increase_threshold {
        return Verdict::Degraded {
            message: format!(
                "friction rate increased {:.1}% (baseline {:.1}% → current {:.1}%)",
                friction_increase * 100.0,
                baseline.friction_rate * 100.0,
                current.friction_rate * 100.0,
            ),
            rollback_recommended: true,
        };
    }

    if success_drop >= config.warning_threshold {
        return Verdict::Warning {
            message: format!(
                "success rate dropped {:.1}% (baseline {:.1}% → current {:.1}%)",
                success_drop * 100.0,
                baseline.success_rate * 100.0,
                current.success_rate * 100.0,
            ),
        };
    }

    Verdict::Healthy
}

/// Compute signal ratios from a slice of signals.
pub fn compute_ratios(signals: &[Signal]) -> SignalBaseline {
    let total = signals.len() as u64;
    let mut success_count: u64 = 0;
    let mut friction_count: u64 = 0;
    let mut decision_count: u64 = 0;
    let mut severity_sum: f64 = 0.0;

    for signal in signals {
        match signal.kind {
            SignalKind::Success => success_count += 1,
            SignalKind::Friction => {
                friction_count += 1;
                severity_sum += extract_severity(&signal.metadata);
            }
            SignalKind::Decision => decision_count += 1,
            _ => {}
        }
    }

    let (success_rate, friction_rate, avg_friction_severity) = if total == 0 {
        (0.0, 0.0, 0.0)
    } else {
        let sr = success_count as f64 / total as f64;
        let fr = friction_count as f64 / total as f64;
        let afs = if friction_count > 0 {
            severity_sum / friction_count as f64
        } else {
            0.0
        };
        (sr, fr, afs)
    };

    SignalBaseline {
        captured_at: 0,
        window_seconds: 0,
        total_signals: total,
        success_count,
        friction_count,
        decision_count,
        avg_friction_severity,
        success_rate,
        friction_rate,
    }
}

/// Extract severity from signal metadata, defaulting to 1.0 if absent.
fn extract_severity(metadata: &serde_json::Value) -> f64 {
    metadata
        .get("severity")
        .and_then(|v| v.as_f64())
        .unwrap_or(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_kernel::LoopStep;

    fn mk(kind: SignalKind) -> Signal {
        Signal {
            step: LoopStep::Act,
            kind,
            message: String::new(),
            metadata: serde_json::json!({}),
            timestamp_ms: 0,
        }
    }

    fn mk_friction(severity: f64) -> Signal {
        Signal {
            step: LoopStep::Act,
            kind: SignalKind::Friction,
            message: String::new(),
            metadata: serde_json::json!({ "severity": severity }),
            timestamp_ms: 0,
        }
    }

    fn mk_signals(success: usize, friction: usize, decision: usize) -> Vec<Signal> {
        let mut signals = Vec::new();
        for _ in 0..success {
            signals.push(mk(SignalKind::Success));
        }
        for _ in 0..friction {
            signals.push(mk_friction(0.5));
        }
        for _ in 0..decision {
            signals.push(mk(SignalKind::Decision));
        }
        signals
    }

    #[test]
    fn compute_ratios_empty_signals() {
        let ratios = compute_ratios(&[]);
        assert_eq!(ratios.total_signals, 0);
        assert_eq!(ratios.success_count, 0);
        assert_eq!(ratios.friction_count, 0);
        assert_eq!(ratios.decision_count, 0);
        assert!((ratios.success_rate - 0.0).abs() < f64::EPSILON);
        assert!((ratios.friction_rate - 0.0).abs() < f64::EPSILON);
        assert!((ratios.avg_friction_severity - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn compute_ratios_all_success() {
        let signals: Vec<Signal> = (0..10).map(|_| mk(SignalKind::Success)).collect();
        let ratios = compute_ratios(&signals);
        assert_eq!(ratios.total_signals, 10);
        assert_eq!(ratios.success_count, 10);
        assert!((ratios.success_rate - 1.0).abs() < f64::EPSILON);
        assert!((ratios.friction_rate - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn compute_ratios_mixed_signals() {
        // 5 success, 3 friction (sev 0.5), 2 decision = 10 total
        let signals = mk_signals(5, 3, 2);
        let ratios = compute_ratios(&signals);
        assert_eq!(ratios.total_signals, 10);
        assert_eq!(ratios.success_count, 5);
        assert_eq!(ratios.friction_count, 3);
        assert_eq!(ratios.decision_count, 2);
        assert!((ratios.success_rate - 0.5).abs() < f64::EPSILON);
        assert!((ratios.friction_rate - 0.3).abs() < f64::EPSILON);
        assert!((ratios.avg_friction_severity - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn evaluate_healthy_when_no_baseline() {
        let canary = Canary::with_defaults();
        let signals = mk_signals(10, 5, 5);
        assert_eq!(canary.evaluate(&signals), Verdict::Healthy);
    }

    #[test]
    fn evaluate_healthy_when_insufficient_data() {
        let mut canary = Canary::with_defaults();
        canary.capture_baseline(&mk_signals(20, 0, 0), 3600);
        // Only 5 current signals — below min_signals_for_baseline (20)
        let current = mk_signals(3, 1, 1);
        assert_eq!(canary.evaluate(&current), Verdict::Healthy);
    }

    #[test]
    fn evaluate_warning_on_moderate_drop() {
        let mut canary = Canary::with_defaults();
        // Baseline: 80% success (16/20)
        canary.capture_baseline(&mk_signals(16, 2, 2), 3600);
        // Current: 65% success (13/20) — drop of 15%, above warning (10%)
        let current = mk_signals(13, 3, 4);
        let verdict = canary.evaluate(&current);
        match verdict {
            Verdict::Warning { .. } => {}
            other => panic!("expected Warning, got {other:?}"),
        }
    }

    #[test]
    fn evaluate_degraded_on_large_drop() {
        let mut canary = Canary::with_defaults();
        // Baseline: 80% success (16/20)
        canary.capture_baseline(&mk_signals(16, 2, 2), 3600);
        // Current: 50% success (10/20) — drop of 30%, above degraded (25%)
        let current = mk_signals(10, 5, 5);
        let verdict = canary.evaluate(&current);
        match verdict {
            Verdict::Degraded {
                rollback_recommended,
                ..
            } => assert!(rollback_recommended),
            other => panic!("expected Degraded, got {other:?}"),
        }
    }

    #[test]
    fn evaluate_degraded_on_friction_spike() {
        let mut canary = Canary::with_defaults();
        // Baseline: 5% friction (1/20)
        canary.capture_baseline(&mk_signals(15, 1, 4), 3600);
        // Current: 30% friction (6/20) — increase of 25%, above threshold (20%)
        let current = mk_signals(10, 6, 4);
        let verdict = canary.evaluate(&current);
        match verdict {
            Verdict::Degraded {
                rollback_recommended,
                ..
            } => assert!(rollback_recommended),
            other => panic!("expected Degraded, got {other:?}"),
        }
    }

    #[test]
    fn capture_baseline_stores_ratios() {
        let mut canary = Canary::with_defaults();
        assert!(canary.baseline().is_none());
        canary.capture_baseline(&mk_signals(8, 1, 1), 7200);
        let baseline = canary.baseline().expect("baseline should be set");
        assert_eq!(baseline.total_signals, 10);
        assert_eq!(baseline.window_seconds, 7200);
        assert!((baseline.success_rate - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn has_sufficient_baseline_checks_min_signals() {
        let mut canary = Canary::with_defaults();
        assert!(!canary.has_sufficient_baseline());

        // 10 signals — below default min of 20
        canary.capture_baseline(&mk_signals(8, 1, 1), 3600);
        assert!(!canary.has_sufficient_baseline());

        // 20 signals — meets minimum
        canary.capture_baseline(&mk_signals(16, 2, 2), 3600);
        assert!(canary.has_sufficient_baseline());
    }
}
