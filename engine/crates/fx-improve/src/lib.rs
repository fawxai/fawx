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
pub use planner::{FileChange, FixPlan, ImprovementPlanner, RiskLevel};

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
) -> Result<ExecutionResult, ImprovementError> {
    config.validate()?;

    let engine = AnalysisEngine::new(signal_store);
    let findings = engine
        .analyze(llm_provider)
        .await
        .map_err(|e| ImprovementError::Analysis(e.to_string()))?;

    let mut detector = ImprovementDetector::new(config.clone(), paths.data_dir)?;
    let candidates = detector.detect(&findings);

    if candidates.is_empty() {
        return Ok(ExecutionResult::empty());
    }

    let plans = ImprovementPlanner::plan(&candidates, llm_provider, paths.repo_root).await?;

    let executor = ImprovementExecutor::new(
        config.clone(),
        paths.proposals_dir.to_path_buf(),
        paths.repo_root.to_path_buf(),
    );
    executor.execute(&plans, &mut detector)
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use fx_llm::{
        CompletionRequest, CompletionResponse, CompletionStream, ProviderCapabilities,
        ProviderError,
    };
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

        assert!(result.proposals_written.is_empty());
        assert!(result.branches_created.is_empty());
        assert!(result.skipped.is_empty());
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
