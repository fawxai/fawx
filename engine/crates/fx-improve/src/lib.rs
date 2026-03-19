#![allow(clippy::field_reassign_with_default)]
//! Self-improvement pipeline: signals → analysis → plans → proposals.
//!
//! Connects the analysis engine (which detects recurring patterns from
//! runtime signals) to the proposal system, creating a closed loop
//! that generates actionable improvement proposals for human review.

pub mod config;
pub mod detector;
pub mod error;
pub mod executor;
pub mod planner;

pub use config::{ImprovementConfig, OutputMode};
pub use detector::{ImprovementCandidate, ImprovementDetector};
pub use error::ImprovementError;
pub use executor::{ExecutionResult, ImprovementExecutor};
pub use planner::{
    shared_skip_reason, skipped_candidate_summary, FileChange, FixPlan, ImprovementPlanner,
    ImprovementRunResult, PlanningResult, RiskLevel, SkippedCandidate,
};

use fx_analysis::AnalysisEngine;
use fx_llm::CompletionProvider;
use fx_memory::SignalStore;
use std::path::Path;

/// Path configuration for the improvement cycle.
pub struct CyclePaths<'a> {
    /// Directory for persistent data (fingerprint history, etc.).
    pub data_dir: &'a Path,
    /// Root directory of the repository being improved.
    pub repo_root: &'a Path,
    /// Directory where proposals are written.
    pub proposals_dir: &'a Path,
}

/// Run a full improvement cycle: analyze → detect → plan → execute.
///
/// Returns early with an empty result if no actionable candidates are found.
pub async fn run_improvement_cycle(
    signal_store: &SignalStore,
    llm_provider: &dyn CompletionProvider,
    config: &ImprovementConfig,
    paths: &CyclePaths<'_>,
) -> Result<ImprovementRunResult, ImprovementError> {
    config.validate()?;

    let engine = AnalysisEngine::new(signal_store);
    let findings = engine
        .analyze(llm_provider)
        .await
        .map_err(|e| ImprovementError::Analysis(e.to_string()))?;

    let mut detector = ImprovementDetector::new(config.clone(), paths.data_dir)?;
    let candidates = detector.detect(&findings);

    if candidates.is_empty() {
        return Ok(ImprovementRunResult::empty());
    }

    let planning = ImprovementPlanner::plan(&candidates, llm_provider, paths.repo_root).await;
    let executor = ImprovementExecutor::new(
        config.clone(),
        paths.proposals_dir.to_path_buf(),
        paths.repo_root.to_path_buf(),
    );
    let execution = executor.execute(&planning.plans, &mut detector)?;
    Ok(planning.into_run_result(execution))
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use fx_core::signals::{LoopStep, Signal, SignalKind};
    use fx_llm::{
        CompletionRequest, CompletionResponse, CompletionStream, ProviderCapabilities,
        ProviderError, ToolCall,
    };
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use tempfile::TempDir;

    #[derive(Debug)]
    struct EmptyMockProvider;

    #[async_trait]
    impl CompletionProvider for EmptyMockProvider {
        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionResponse, ProviderError> {
            Ok(CompletionResponse {
                content: Vec::new(),
                tool_calls: Vec::new(),
                usage: None,
                stop_reason: Some("end_turn".to_string()),
            })
        }

        async fn complete_stream(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionStream, ProviderError> {
            Err(ProviderError::Provider("not supported".to_string()))
        }

        fn name(&self) -> &str {
            "empty-mock"
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["test".to_string()]
        }

        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities {
                supports_temperature: true,
                requires_streaming: false,
            }
        }
    }

    #[derive(Debug)]
    struct QueueMockProvider {
        responses: Mutex<VecDeque<Result<CompletionResponse, ProviderError>>>,
    }

    impl QueueMockProvider {
        fn new(responses: Vec<Result<CompletionResponse, ProviderError>>) -> Self {
            Self {
                responses: Mutex::new(VecDeque::from(responses)),
            }
        }
    }

    #[async_trait]
    impl CompletionProvider for QueueMockProvider {
        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionResponse, ProviderError> {
            self.responses
                .lock()
                .expect("mock provider lock")
                .pop_front()
                .expect("mock response")
        }

        async fn complete_stream(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionStream, ProviderError> {
            Err(ProviderError::Provider("not supported".to_string()))
        }

        fn name(&self) -> &str {
            "queue-mock"
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["test".to_string()]
        }

        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities {
                supports_temperature: true,
                requires_streaming: false,
            }
        }
    }

    fn report_findings_response(findings: Vec<serde_json::Value>) -> CompletionResponse {
        CompletionResponse {
            content: Vec::new(),
            tool_calls: vec![ToolCall {
                id: "call-1".to_string(),
                name: "report_findings".to_string(),
                arguments: serde_json::json!({ "findings": findings }),
            }],
            usage: None,
            stop_reason: Some("tool_use".to_string()),
        }
    }

    fn empty_response() -> CompletionResponse {
        CompletionResponse {
            content: Vec::new(),
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: Some("end_turn".to_string()),
        }
    }

    fn sample_finding_json_with_evidence(
        name: &str,
        confidence: &str,
        evidence_count: usize,
    ) -> serde_json::Value {
        let evidence = (0..evidence_count)
            .map(|index| {
                serde_json::json!({
                    "session_id": format!("sess-{}", index + 1),
                    "signal_kind": "friction",
                    "message": format!("test signal {}", index + 1),
                    "timestamp_ms": 1000 + index as u64,
                })
            })
            .collect::<Vec<_>>();

        serde_json::json!({
            "pattern_name": name,
            "description": format!("Description for {name}"),
            "confidence": confidence,
            "evidence": evidence,
            "suggested_action": "Fix the issue"
        })
    }

    #[tokio::test]
    async fn full_cycle_empty_signals_produces_no_improvements() {
        let tmp = TempDir::new().unwrap();
        let data_dir = tmp.path().join("data");
        let proposals_dir = tmp.path().join("proposals");
        let repo_root = tmp.path().join("repo");

        let store = SignalStore::new(&data_dir, "empty-session").unwrap();
        let provider = EmptyMockProvider;
        let config = ImprovementConfig::default();
        let paths = CyclePaths {
            data_dir: &data_dir,
            repo_root: &repo_root,
            proposals_dir: &proposals_dir,
        };

        let result = run_improvement_cycle(&store, &provider, &config, &paths)
            .await
            .unwrap();

        assert_eq!(result.plans_generated, 0);
        assert!(result.proposals_written.is_empty());
        assert!(result.branches_created.is_empty());
        assert!(result.skipped.is_empty());
        assert!(result.skipped_candidates.is_empty());
    }

    #[tokio::test]
    async fn full_cycle_keeps_skipped_candidates_when_planning_fails() {
        let tmp = TempDir::new().unwrap();
        let data_dir = tmp.path().join("data");
        let proposals_dir = tmp.path().join("proposals");
        let repo_root = tmp.path().join("repo");
        std::fs::create_dir_all(&repo_root).unwrap();

        let store = SignalStore::new(&data_dir, "planning-failure-session").unwrap();
        store
            .persist(&[Signal {
                step: LoopStep::Act,
                kind: SignalKind::Friction,
                message: "test friction".to_string(),
                metadata: serde_json::json!({}),
                timestamp_ms: 1000,
            }])
            .unwrap();

        let provider = QueueMockProvider::new(vec![
            Ok(report_findings_response(vec![
                sample_finding_json_with_evidence("Fixable issue", "high", 3),
            ])),
            Ok(empty_response()),
        ]);
        let config = ImprovementConfig::default();
        let paths = CyclePaths {
            data_dir: &data_dir,
            repo_root: &repo_root,
            proposals_dir: &proposals_dir,
        };

        let result = run_improvement_cycle(&store, &provider, &config, &paths)
            .await
            .unwrap();

        assert_eq!(result.plans_generated, 0);
        assert!(result.proposals_written.is_empty());
        assert!(result.branches_created.is_empty());
        assert!(result.skipped.is_empty());
        assert_eq!(result.skipped_candidates.len(), 1);
        assert_eq!(result.skipped_candidates[0].name, "Fixable issue");
        assert_eq!(
            result.skipped_candidates[0].reason,
            "model did not produce a plan"
        );
    }

    #[tokio::test]
    async fn full_cycle_rejects_invalid_config() {
        let tmp = TempDir::new().unwrap();
        let data_dir = tmp.path().join("data");
        let proposals_dir = tmp.path().join("proposals");
        let repo_root = tmp.path().join("repo");

        let store = SignalStore::new(&data_dir, "invalid-config-session").unwrap();
        let provider = EmptyMockProvider;
        let mut config = ImprovementConfig::default();
        config.min_evidence_count = 0;

        let paths = CyclePaths {
            data_dir: &data_dir,
            repo_root: &repo_root,
            proposals_dir: &proposals_dir,
        };

        let error = run_improvement_cycle(&store, &provider, &config, &paths)
            .await
            .expect_err("invalid config must fail before analysis");

        assert!(
            matches!(error, ImprovementError::Config(message) if message.contains("min_evidence_count"))
        );
    }
}
