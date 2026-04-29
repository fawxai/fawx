use anyhow::Context;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum EvalMode {
    CiLite,
    Full,
}

impl EvalMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CiLite => "ci-lite",
            Self::Full => "full",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Options {
    pub mode: EvalMode,
    pub output: PathBuf,
    pub baseline: Option<PathBuf>,
    pub update_baseline: bool,
    pub fail_on_regression: bool,
}

#[derive(Debug, Clone)]
struct ScenarioCase {
    #[allow(dead_code)] // used in test assertions (duplicate-id check)
    id: &'static str,
    domain: &'static str,
    false_success_claim: bool,
    artifacts_complete: bool,
    deterministic_fallback_correct: bool,
    retries_observed: u8,
    retry_bound: u8,
    mutation_state_pass: bool,
    tool_result_reused: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalMetrics {
    pub false_success_claim_rate: f64,
    pub completion_artifact_pass_rate: f64,
    pub deterministic_fallback_correctness: f64,
    pub retry_bound_adherence: f64,
    pub mutation_state_pass_rate: f64,
    pub tool_result_reuse_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalReport {
    pub report_version: u32,
    pub generated_at: String,
    pub mode: EvalMode,
    pub scenario_count: usize,
    pub domain_counts: DomainCounts,
    pub metrics: EvalMetrics,
    pub trend_vs_baseline: Option<MetricDelta>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DomainCounts {
    pub travel: usize,
    pub shopping: usize,
    pub general_web_research: usize,
    pub coding_agent: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetricDelta {
    pub false_success_claim_rate: f64,
    pub completion_artifact_pass_rate: f64,
    pub deterministic_fallback_correctness: f64,
    pub retry_bound_adherence: f64,
    pub mutation_state_pass_rate: f64,
    pub tool_result_reuse_rate: f64,
}

pub fn run(options: Options) -> anyhow::Result<i32> {
    let cases = scenarios_for_mode(options.mode);
    let report = build_report(options.mode, &cases, options.baseline.as_ref())?;

    if options.fail_on_regression {
        if let Some(delta) = &report.trend_vs_baseline {
            let has_regression = delta.false_success_claim_rate > 0.0
                || delta.completion_artifact_pass_rate < 0.0
                || delta.deterministic_fallback_correctness < 0.0
                || delta.retry_bound_adherence < 0.0
                || delta.mutation_state_pass_rate < 0.0
                || delta.tool_result_reuse_rate < 0.0;
            if has_regression {
                anyhow::bail!(
                    "metric regression detected vs baseline (false-success should not increase; other metrics should not decrease)"
                );
            }
        }
    }

    if let Some(parent) = options.output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create output dir {}", parent.display()))?;
    }
    fs::write(&options.output, serde_json::to_string_pretty(&report)?)
        .with_context(|| format!("failed writing report to {}", options.output.display()))?;

    if options.update_baseline {
        let baseline_path = options
            .baseline
            .unwrap_or_else(|| default_baseline_path(options.mode));
        if let Some(parent) = baseline_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create baseline dir {}", parent.display()))?;
        }
        fs::write(&baseline_path, serde_json::to_string_pretty(&report)?)
            .with_context(|| format!("failed writing baseline to {}", baseline_path.display()))?;
        println!("Updated baseline: {}", baseline_path.display());
    }

    println!("Determinism eval completed ({})", options.mode.as_str());
    println!("Report: {}", options.output.display());
    println!(
        "Metrics: false-success={:.3}, artifact-pass={:.3}, fallback-correct={:.3}, retry-adherence={:.3}, mutation-state={:.3}, tool-result-reuse={:.3}",
        report.metrics.false_success_claim_rate,
        report.metrics.completion_artifact_pass_rate,
        report.metrics.deterministic_fallback_correctness,
        report.metrics.retry_bound_adherence,
        report.metrics.mutation_state_pass_rate,
        report.metrics.tool_result_reuse_rate
    );

    if let Some(delta) = &report.trend_vs_baseline {
        println!(
            "Delta vs baseline: false-success={:+.3}, artifact-pass={:+.3}, fallback-correct={:+.3}, retry-adherence={:+.3}, mutation-state={:+.3}, tool-result-reuse={:+.3}",
            delta.false_success_claim_rate,
            delta.completion_artifact_pass_rate,
            delta.deterministic_fallback_correctness,
            delta.retry_bound_adherence,
            delta.mutation_state_pass_rate,
            delta.tool_result_reuse_rate
        );
    }

    Ok(0)
}

fn build_report(
    mode: EvalMode,
    cases: &[ScenarioCase],
    baseline_path: Option<&PathBuf>,
) -> anyhow::Result<EvalReport> {
    if cases.is_empty() {
        anyhow::bail!("no scenarios to evaluate");
    }

    let scenario_count = cases.len() as f64;

    let false_success_claims = cases.iter().filter(|c| c.false_success_claim).count() as f64;
    let artifacts_pass = cases.iter().filter(|c| c.artifacts_complete).count() as f64;
    let fallback_correct = cases
        .iter()
        .filter(|c| c.deterministic_fallback_correct)
        .count() as f64;
    let retry_within_bound = cases
        .iter()
        .filter(|c| c.retries_observed <= c.retry_bound)
        .count() as f64;
    let mutation_state_pass = cases.iter().filter(|c| c.mutation_state_pass).count() as f64;
    let tool_result_reused = cases.iter().filter(|c| c.tool_result_reused).count() as f64;

    let metrics = EvalMetrics {
        false_success_claim_rate: false_success_claims / scenario_count,
        completion_artifact_pass_rate: artifacts_pass / scenario_count,
        deterministic_fallback_correctness: fallback_correct / scenario_count,
        retry_bound_adherence: retry_within_bound / scenario_count,
        mutation_state_pass_rate: mutation_state_pass / scenario_count,
        tool_result_reuse_rate: tool_result_reused / scenario_count,
    };

    let domain_counts = DomainCounts {
        travel: cases.iter().filter(|c| c.domain == "travel").count(),
        shopping: cases.iter().filter(|c| c.domain == "shopping").count(),
        general_web_research: cases
            .iter()
            .filter(|c| c.domain == "general_web_research")
            .count(),
        coding_agent: cases.iter().filter(|c| c.domain == "coding_agent").count(),
    };

    let trend_vs_baseline = match baseline_path {
        Some(path) if path.exists() => {
            let baseline: EvalReport = serde_json::from_str(
                &fs::read_to_string(path)
                    .with_context(|| format!("failed reading baseline from {}", path.display()))?,
            )
            .with_context(|| format!("failed parsing baseline {}", path.display()))?;

            Some(MetricDelta {
                false_success_claim_rate: metrics.false_success_claim_rate
                    - baseline.metrics.false_success_claim_rate,
                completion_artifact_pass_rate: metrics.completion_artifact_pass_rate
                    - baseline.metrics.completion_artifact_pass_rate,
                deterministic_fallback_correctness: metrics.deterministic_fallback_correctness
                    - baseline.metrics.deterministic_fallback_correctness,
                retry_bound_adherence: metrics.retry_bound_adherence
                    - baseline.metrics.retry_bound_adherence,
                mutation_state_pass_rate: metrics.mutation_state_pass_rate
                    - baseline.metrics.mutation_state_pass_rate,
                tool_result_reuse_rate: metrics.tool_result_reuse_rate
                    - baseline.metrics.tool_result_reuse_rate,
            })
        }
        _ => None,
    };

    Ok(EvalReport {
        report_version: 1,
        generated_at: Utc::now().to_rfc3339(),
        mode,
        scenario_count: cases.len(),
        domain_counts,
        metrics,
        trend_vs_baseline,
    })
}

fn default_baseline_path(mode: EvalMode) -> PathBuf {
    PathBuf::from(format!(".ci/determinism/baseline-{}.json", mode.as_str()))
}

fn scenarios_for_mode(mode: EvalMode) -> Vec<ScenarioCase> {
    // Baseline harness scope (issue #833): fixed synthetic fixtures for deterministic
    // metric diffing in CI. This intentionally does not execute the live Android loop;
    // realism/live wiring is tracked as follow-up in issue #835.
    let ci_lite = vec![
        ScenarioCase {
            id: "travel-lite-1",
            domain: "travel",
            false_success_claim: false,
            artifacts_complete: true,
            deterministic_fallback_correct: true,
            retries_observed: 1,
            retry_bound: 2,
            mutation_state_pass: true,
            tool_result_reused: true,
        },
        ScenarioCase {
            id: "shopping-lite-1",
            domain: "shopping",
            false_success_claim: false,
            artifacts_complete: true,
            deterministic_fallback_correct: true,
            retries_observed: 0,
            retry_bound: 2,
            mutation_state_pass: true,
            tool_result_reused: true,
        },
        ScenarioCase {
            id: "research-lite-1",
            domain: "general_web_research",
            false_success_claim: false,
            artifacts_complete: true,
            deterministic_fallback_correct: true,
            retries_observed: 1,
            retry_bound: 1,
            mutation_state_pass: true,
            tool_result_reused: true,
        },
        ScenarioCase {
            id: "coding-lite-review-fix",
            domain: "coding_agent",
            false_success_claim: false,
            artifacts_complete: true,
            deterministic_fallback_correct: true,
            retries_observed: 0,
            retry_bound: 1,
            mutation_state_pass: true,
            tool_result_reused: true,
        },
    ];

    if mode == EvalMode::CiLite {
        return ci_lite;
    }

    let mut full = ci_lite;
    full.extend([
        ScenarioCase {
            id: "travel-full-2",
            domain: "travel",
            false_success_claim: false,
            artifacts_complete: true,
            deterministic_fallback_correct: true,
            retries_observed: 2,
            retry_bound: 2,
            mutation_state_pass: true,
            tool_result_reused: true,
        },
        ScenarioCase {
            id: "travel-full-3",
            domain: "travel",
            false_success_claim: false,
            artifacts_complete: true,
            deterministic_fallback_correct: true,
            retries_observed: 1,
            retry_bound: 2,
            mutation_state_pass: true,
            tool_result_reused: true,
        },
        ScenarioCase {
            id: "shopping-full-2",
            domain: "shopping",
            false_success_claim: false,
            artifacts_complete: true,
            deterministic_fallback_correct: true,
            retries_observed: 1,
            retry_bound: 2,
            mutation_state_pass: true,
            tool_result_reused: true,
        },
        ScenarioCase {
            id: "shopping-full-3",
            domain: "shopping",
            false_success_claim: false,
            artifacts_complete: false,
            deterministic_fallback_correct: true,
            retries_observed: 2,
            retry_bound: 2,
            mutation_state_pass: true,
            tool_result_reused: true,
        },
        ScenarioCase {
            id: "research-full-2",
            domain: "general_web_research",
            false_success_claim: false,
            artifacts_complete: true,
            deterministic_fallback_correct: true,
            retries_observed: 1,
            retry_bound: 2,
            mutation_state_pass: true,
            tool_result_reused: true,
        },
        ScenarioCase {
            id: "research-full-3",
            domain: "general_web_research",
            false_success_claim: true,
            artifacts_complete: false,
            deterministic_fallback_correct: false,
            retries_observed: 3,
            retry_bound: 2,
            mutation_state_pass: false,
            tool_result_reused: false,
        },
        ScenarioCase {
            id: "coding-full-git-credential-push",
            domain: "coding_agent",
            false_success_claim: false,
            artifacts_complete: true,
            deterministic_fallback_correct: true,
            retries_observed: 1,
            retry_bound: 2,
            mutation_state_pass: true,
            tool_result_reused: true,
        },
        ScenarioCase {
            id: "coding-full-code-review-fix",
            domain: "coding_agent",
            false_success_claim: false,
            artifacts_complete: true,
            deterministic_fallback_correct: true,
            retries_observed: 1,
            retry_bound: 2,
            mutation_state_pass: true,
            tool_result_reused: true,
        },
        ScenarioCase {
            id: "coding-full-wrong-branch-detection",
            domain: "coding_agent",
            false_success_claim: false,
            artifacts_complete: true,
            deterministic_fallback_correct: true,
            retries_observed: 0,
            retry_bound: 1,
            mutation_state_pass: true,
            tool_result_reused: true,
        },
    ]);

    full
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn ci_lite_has_required_domain_coverage() {
        let cases = scenarios_for_mode(EvalMode::CiLite);
        assert!(cases.iter().any(|c| c.domain == "travel"));
        assert!(cases.iter().any(|c| c.domain == "shopping"));
        assert!(cases.iter().any(|c| c.domain == "general_web_research"));
        assert!(cases.iter().any(|c| c.domain == "coding_agent"));
    }

    #[test]
    fn coding_agent_metrics_are_state_based() {
        let report =
            build_report(EvalMode::Full, &scenarios_for_mode(EvalMode::Full), None).unwrap();

        assert!(report.domain_counts.coding_agent >= 4);
        assert!(
            report.metrics.mutation_state_pass_rate > 0.0,
            "mutation metric should track artifact/workspace state, not transcript phrasing"
        );
        assert!(
            report.metrics.tool_result_reuse_rate > 0.0,
            "tool-result reuse metric should catch loops that ignore prior tool evidence"
        );
    }

    #[test]
    fn report_metrics_are_deterministic_and_diffable() {
        let cases = scenarios_for_mode(EvalMode::Full);
        let report_a = build_report(EvalMode::Full, &cases, None).unwrap();
        let report_b = build_report(EvalMode::Full, &cases, None).unwrap();

        assert_eq!(report_a.metrics, report_b.metrics);
        assert_eq!(report_a.domain_counts, report_b.domain_counts);
    }

    #[test]
    fn baseline_delta_is_computed() {
        let temp = tempdir().unwrap();
        let baseline_path = temp.path().join("baseline.json");

        let baseline = EvalReport {
            report_version: 1,
            generated_at: "2026-01-01T00:00:00Z".to_string(),
            mode: EvalMode::Full,
            scenario_count: 9,
            domain_counts: DomainCounts {
                travel: 3,
                shopping: 3,
                general_web_research: 3,
                coding_agent: 0,
            },
            metrics: EvalMetrics {
                false_success_claim_rate: 0.20,
                completion_artifact_pass_rate: 0.60,
                deterministic_fallback_correctness: 0.70,
                retry_bound_adherence: 0.80,
                mutation_state_pass_rate: 0.70,
                tool_result_reuse_rate: 0.70,
            },
            trend_vs_baseline: None,
        };

        fs::write(
            &baseline_path,
            serde_json::to_string_pretty(&baseline).unwrap(),
        )
        .unwrap();

        let report = build_report(
            EvalMode::Full,
            &scenarios_for_mode(EvalMode::Full),
            Some(&baseline_path),
        )
        .unwrap();

        let delta = report.trend_vs_baseline.expect("delta");
        assert!(delta.false_success_claim_rate.abs() > 0.0);
        assert!(delta.completion_artifact_pass_rate.abs() > 0.0);
    }

    #[test]
    fn ids_are_unique_for_full_suite() {
        let cases = scenarios_for_mode(EvalMode::Full);
        let mut ids = std::collections::HashSet::new();
        for case in cases {
            assert!(ids.insert(case.id), "duplicate case id: {}", case.id);
        }
    }

    #[test]
    fn build_report_rejects_empty_cases() {
        let err = build_report(EvalMode::CiLite, &[], None).unwrap_err();
        assert!(
            err.to_string().contains("no scenarios to evaluate"),
            "unexpected error: {err}"
        );
    }
}
