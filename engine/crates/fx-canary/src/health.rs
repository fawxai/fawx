use fx_kernel::SignalSeverity;
use serde::{Deserialize, Serialize};
use std::fmt;

const EPSILON: f64 = 1e-9;

/// Typed canary dimensions evaluated against a baseline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthDimension {
    SuccessRate,
    FrictionRate,
    AvgLatencyMs,
    P95LatencyMs,
    RetryRate,
    ProviderFallbackCount,
    AvgCostPerCycle,
    BlockedCount,
}

impl HealthDimension {
    pub const fn label(self) -> &'static str {
        match self {
            Self::SuccessRate => "success_rate",
            Self::FrictionRate => "friction_rate",
            Self::AvgLatencyMs => "avg_latency_ms",
            Self::P95LatencyMs => "p95_latency_ms",
            Self::RetryRate => "retry_rate",
            Self::ProviderFallbackCount => "provider_fallback_count",
            Self::AvgCostPerCycle => "avg_cost_per_cycle",
            Self::BlockedCount => "blocked_count",
        }
    }

    fn format_value(self, value: f64) -> String {
        match self {
            Self::SuccessRate | Self::FrictionRate | Self::RetryRate => {
                format!("{:.1}%", value * 100.0)
            }
            Self::AvgLatencyMs | Self::P95LatencyMs => format!("{value:.0}ms"),
            Self::AvgCostPerCycle => format!("{value:.2}c/cycle"),
            Self::ProviderFallbackCount | Self::BlockedCount => format!("{value:.0}"),
        }
    }
}

impl fmt::Display for HealthDimension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Multi-dimensional health snapshot for canary evaluation.
///
/// Optional fields are used for dimensions that cannot be computed reliably
/// from the current window. We skip those comparisons instead of treating
/// missing data as "zero" and triggering bogus rollbacks. Rate, latency, and
/// cost dimensions are therefore `Option<f64>`, while raw counts remain `u32`
/// because they are directly observable from the signal stream even when other
/// denominators are sparse.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct HealthVector {
    pub success_rate: Option<f64>,
    pub friction_rate: Option<f64>,
    pub avg_latency_ms: Option<f64>,
    pub p95_latency_ms: Option<f64>,
    pub retry_rate: Option<f64>,
    pub provider_fallback_count: u32,
    pub avg_cost_per_cycle: Option<f64>,
    pub blocked_count: u32,
}

/// Thresholds for per-dimension degradation checks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HealthThresholds {
    pub success_rate_drop: f64,
    pub friction_rate_increase: f64,
    pub avg_latency_increase: f64,
    pub p95_latency_increase: f64,
    pub retry_rate_increase: f64,
    pub fallback_count_threshold: u32,
    pub cost_increase: f64,
    pub blocked_count_threshold: u32,
    #[serde(default = "default_high_severity_multiplier")]
    pub high_severity_multiplier: f64,
    #[serde(default = "default_critical_severity_multiplier")]
    pub critical_severity_multiplier: f64,
}

impl Default for HealthThresholds {
    fn default() -> Self {
        Self {
            success_rate_drop: 0.15,
            friction_rate_increase: 0.20,
            avg_latency_increase: 0.50,
            p95_latency_increase: 1.00,
            retry_rate_increase: 0.25,
            fallback_count_threshold: 3,
            cost_increase: 0.50,
            blocked_count_threshold: 5,
            high_severity_multiplier: default_high_severity_multiplier(),
            critical_severity_multiplier: default_critical_severity_multiplier(),
        }
    }
}

/// One dimension that meaningfully regressed from the baseline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DegradedDimension {
    pub dimension: HealthDimension,
    pub baseline_value: f64,
    pub current_value: f64,
    pub threshold: f64,
    pub severity: SignalSeverity,
}

impl DegradedDimension {
    pub fn describe(&self) -> String {
        format!(
            "{} {} -> {} ({})",
            self.dimension,
            self.dimension.format_value(self.baseline_value),
            self.dimension.format_value(self.current_value),
            self.severity
        )
    }
}

impl HealthVector {
    /// Returns the dimensions that degraded beyond their configured thresholds.
    pub fn evaluate(
        &self,
        baseline: &HealthVector,
        thresholds: &HealthThresholds,
    ) -> Vec<DegradedDimension> {
        let mut degraded = Vec::new();

        compare_relative(
            &mut degraded,
            HealthDimension::SuccessRate,
            baseline.success_rate,
            self.success_rate,
            thresholds.success_rate_drop,
            RelativeDirection::Drop,
            thresholds,
        );
        compare_relative(
            &mut degraded,
            HealthDimension::FrictionRate,
            baseline.friction_rate,
            self.friction_rate,
            thresholds.friction_rate_increase,
            RelativeDirection::Increase,
            thresholds,
        );
        compare_relative(
            &mut degraded,
            HealthDimension::AvgLatencyMs,
            baseline.avg_latency_ms,
            self.avg_latency_ms,
            thresholds.avg_latency_increase,
            RelativeDirection::Increase,
            thresholds,
        );
        compare_relative(
            &mut degraded,
            HealthDimension::P95LatencyMs,
            baseline.p95_latency_ms,
            self.p95_latency_ms,
            thresholds.p95_latency_increase,
            RelativeDirection::Increase,
            thresholds,
        );
        compare_relative(
            &mut degraded,
            HealthDimension::RetryRate,
            baseline.retry_rate,
            self.retry_rate,
            thresholds.retry_rate_increase,
            RelativeDirection::Increase,
            thresholds,
        );
        compare_absolute_count(
            &mut degraded,
            HealthDimension::ProviderFallbackCount,
            baseline.provider_fallback_count,
            self.provider_fallback_count,
            thresholds.fallback_count_threshold,
            thresholds,
        );
        compare_relative(
            &mut degraded,
            HealthDimension::AvgCostPerCycle,
            baseline.avg_cost_per_cycle,
            self.avg_cost_per_cycle,
            thresholds.cost_increase,
            RelativeDirection::Increase,
            thresholds,
        );
        compare_absolute_count(
            &mut degraded,
            HealthDimension::BlockedCount,
            baseline.blocked_count,
            self.blocked_count,
            thresholds.blocked_count_threshold,
            thresholds,
        );

        degraded.sort_by(|left, right| {
            right
                .severity
                .cmp(&left.severity)
                .then_with(|| left.dimension.cmp(&right.dimension))
        });
        degraded
    }
}

pub fn summarize_degraded_dimensions(dimensions: &[DegradedDimension]) -> String {
    dimensions
        .iter()
        .map(DegradedDimension::describe)
        .collect::<Vec<_>>()
        .join(", ")
}

#[derive(Clone, Copy)]
enum RelativeDirection {
    Drop,
    Increase,
}

impl RelativeDirection {
    fn observed_change(self, baseline: f64, current: f64) -> Option<f64> {
        match self {
            Self::Drop => (baseline > EPSILON).then(|| ((baseline - current) / baseline).max(0.0)),
            Self::Increase => Some(if baseline > EPSILON {
                ((current - baseline) / baseline).max(0.0)
            } else {
                current.max(0.0)
            }),
        }
    }
}

fn compare_relative(
    degraded: &mut Vec<DegradedDimension>,
    dimension: HealthDimension,
    baseline: Option<f64>,
    current: Option<f64>,
    threshold: f64,
    direction: RelativeDirection,
    thresholds: &HealthThresholds,
) {
    let (Some(baseline), Some(current)) = (baseline, current) else {
        return;
    };
    let Some(observed) = direction.observed_change(baseline, current) else {
        return;
    };
    if observed + EPSILON < threshold {
        return;
    }

    degraded.push(DegradedDimension {
        dimension,
        baseline_value: baseline,
        current_value: current,
        threshold,
        severity: severity_for_multiplier(observed / threshold.max(EPSILON), thresholds),
    });
}

fn compare_absolute_count(
    degraded: &mut Vec<DegradedDimension>,
    dimension: HealthDimension,
    baseline: u32,
    current: u32,
    threshold: u32,
    thresholds: &HealthThresholds,
) {
    let allowed = baseline.max(threshold);
    if current <= allowed {
        return;
    }

    let threshold = threshold.max(1);
    let excess = current - allowed;
    degraded.push(DegradedDimension {
        dimension,
        baseline_value: f64::from(baseline),
        current_value: f64::from(current),
        threshold: f64::from(threshold),
        severity: severity_for_multiplier(
            1.0 + f64::from(excess) / f64::from(threshold),
            thresholds,
        ),
    });
}

fn severity_for_multiplier(multiplier: f64, thresholds: &HealthThresholds) -> SignalSeverity {
    let high = thresholds.high_severity_multiplier.max(1.0);
    let critical = thresholds.critical_severity_multiplier.max(high);
    if multiplier >= critical {
        SignalSeverity::Critical
    } else if multiplier >= high {
        SignalSeverity::High
    } else {
        SignalSeverity::Medium
    }
}

const fn default_high_severity_multiplier() -> f64 {
    1.5
}

const fn default_critical_severity_multiplier() -> f64 {
    2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluate_skips_unavailable_dimensions() {
        let baseline = HealthVector {
            success_rate: Some(0.9),
            avg_latency_ms: Some(100.0),
            ..HealthVector::default()
        };
        let current = HealthVector {
            success_rate: Some(0.85),
            avg_latency_ms: None,
            ..HealthVector::default()
        };

        let degraded = current.evaluate(&baseline, &HealthThresholds::default());

        assert!(degraded.is_empty());
    }

    #[test]
    fn evaluate_uses_threshold_floor_when_baseline_is_zero() {
        let baseline = HealthVector {
            friction_rate: Some(0.0),
            ..HealthVector::default()
        };
        let below_floor = HealthVector {
            friction_rate: Some(0.15),
            ..HealthVector::default()
        };
        let above_floor = HealthVector {
            friction_rate: Some(0.25),
            ..HealthVector::default()
        };

        assert!(below_floor
            .evaluate(&baseline, &HealthThresholds::default())
            .is_empty());

        let degraded = above_floor.evaluate(&baseline, &HealthThresholds::default());
        assert_eq!(degraded.len(), 1);
        assert_eq!(degraded[0].dimension, HealthDimension::FrictionRate);
    }

    #[test]
    fn evaluate_detects_relative_and_absolute_regressions() {
        let baseline = HealthVector {
            success_rate: Some(0.9),
            friction_rate: Some(0.05),
            avg_latency_ms: Some(100.0),
            p95_latency_ms: Some(160.0),
            retry_rate: Some(0.05),
            provider_fallback_count: 0,
            avg_cost_per_cycle: Some(1.0),
            blocked_count: 0,
        };
        let current = HealthVector {
            success_rate: Some(0.6),
            friction_rate: Some(0.2),
            avg_latency_ms: Some(200.0),
            p95_latency_ms: Some(400.0),
            retry_rate: Some(0.25),
            provider_fallback_count: 4,
            avg_cost_per_cycle: Some(2.0),
            blocked_count: 6,
        };

        let degraded = current.evaluate(&baseline, &HealthThresholds::default());
        let dimensions = degraded
            .iter()
            .map(|dimension| dimension.dimension)
            .collect::<Vec<_>>();

        assert!(dimensions.contains(&HealthDimension::SuccessRate));
        assert!(dimensions.contains(&HealthDimension::FrictionRate));
        assert!(dimensions.contains(&HealthDimension::AvgLatencyMs));
        assert!(dimensions.contains(&HealthDimension::P95LatencyMs));
        assert!(dimensions.contains(&HealthDimension::RetryRate));
        assert!(dimensions.contains(&HealthDimension::ProviderFallbackCount));
        assert!(dimensions.contains(&HealthDimension::AvgCostPerCycle));
        assert!(dimensions.contains(&HealthDimension::BlockedCount));
    }

    #[test]
    fn evaluate_uses_configurable_severity_multipliers() {
        let baseline = HealthVector {
            success_rate: Some(1.0),
            ..HealthVector::default()
        };
        let current = HealthVector {
            success_rate: Some(0.5),
            ..HealthVector::default()
        };
        let thresholds = HealthThresholds {
            success_rate_drop: 0.40,
            high_severity_multiplier: 1.1,
            critical_severity_multiplier: 1.2,
            ..HealthThresholds::default()
        };

        let degraded = current.evaluate(&baseline, &thresholds);
        assert_eq!(degraded.len(), 1);
        assert_eq!(degraded[0].severity, SignalSeverity::Critical);
    }
}
