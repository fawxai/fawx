use crate::llm_source::build_subagent_experiment_prompt;
use crate::response_parser::parse_patch_response;
use crate::{ConsensusError, Experiment, GenerationStrategy, PatchResponse, PatchSource};
use fx_subagent::{SpawnConfig, SpawnMode, SubagentControl, SubagentStatus};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tracing::warn;

pub struct SubagentPatchSource {
    manager: Arc<dyn SubagentControl>,
    strategy: GenerationStrategy,
    working_dir: PathBuf,
    poll_interval: Duration,
}

impl SubagentPatchSource {
    pub fn new(
        manager: Arc<dyn SubagentControl>,
        strategy: GenerationStrategy,
        working_dir: PathBuf,
    ) -> Self {
        Self {
            manager,
            strategy,
            working_dir,
            poll_interval: Duration::from_millis(100),
        }
    }

    #[cfg(test)]
    fn with_poll_interval(mut self, poll_interval: Duration) -> Self {
        self.poll_interval = poll_interval;
        self
    }

    fn spawn_config(&self, system_prompt: &str, experiment: &Experiment) -> SpawnConfig {
        let mut config = SpawnConfig::new(build_subagent_experiment_prompt(experiment));
        config.label = Some(format!("experiment-{}", strategy_slug(&self.strategy)));
        config.mode = SpawnMode::Run;
        config.cwd = Some(self.working_dir.clone());
        config.system_prompt = Some(system_prompt.to_owned());
        config
    }

    async fn await_response(
        &self,
        id: &fx_subagent::SubagentId,
        timeout: Duration,
    ) -> Result<String, ConsensusError> {
        let manager = Arc::clone(&self.manager);
        let poll_interval = self.poll_interval;
        let poll_result = tokio::time::timeout(timeout, async move {
            loop {
                let status = manager
                    .status(id)
                    .await
                    .map_err(subagent_protocol_error)?
                    .ok_or_else(|| {
                        ConsensusError::Protocol(format!("subagent disappeared: {id}"))
                    })?;
                match status {
                    SubagentStatus::Running => tokio::time::sleep(poll_interval).await,
                    SubagentStatus::Completed { result, .. } => return Ok(result),
                    SubagentStatus::Failed { error } => {
                        return Err(ConsensusError::Protocol(format!(
                            "subagent failed: {error}"
                        )))
                    }
                    SubagentStatus::Cancelled => {
                        return Err(ConsensusError::Protocol(format!(
                            "subagent cancelled: {id}"
                        )))
                    }
                    SubagentStatus::TimedOut => {
                        return Err(ConsensusError::Protocol(format!(
                            "subagent timed out: {id}"
                        )))
                    }
                }
            }
        })
        .await;

        match poll_result {
            Ok(result) => result,
            Err(_) => {
                self.manager
                    .cancel(id)
                    .await
                    .map_err(subagent_protocol_error)?;
                Err(ConsensusError::NoConsensus(format!(
                    "subagent did not respond within {}s: {id}",
                    timeout.as_secs_f64()
                )))
            }
        }
    }
}

#[async_trait::async_trait]
impl PatchSource for SubagentPatchSource {
    async fn generate_patch(
        &self,
        system_prompt: &str,
        experiment: &Experiment,
    ) -> Result<PatchResponse, ConsensusError> {
        let config = self.spawn_config(system_prompt, experiment);
        let handle = self
            .manager
            .spawn(config)
            .await
            .map_err(subagent_protocol_error)?;
        let text = self.await_response(&handle.id, experiment.timeout).await?;
        match parse_patch_response(&text) {
            Ok(response) => Ok(response),
            Err(parse_error) => {
                warn!(
                    error = %parse_error,
                    "subagent response missing patch tags, falling back to git diff"
                );
                fallback_git_diff(&self.working_dir, &text, parse_error).await
            }
        }
    }
}

async fn fallback_git_diff(
    working_dir: &std::path::Path,
    text: &str,
    parse_error: ConsensusError,
) -> Result<PatchResponse, ConsensusError> {
    let output = tokio::process::Command::new("git")
        .args(["diff", "HEAD"])
        .current_dir(working_dir)
        .output()
        .await
        .map_err(|err| ConsensusError::Protocol(format!("git diff failed: {err}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!(%stderr, "git diff exited with non-zero status, returning original parse error");
        return Err(parse_error);
    }
    let diff = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if diff.is_empty() {
        return Err(ConsensusError::Protocol(
            "subagent made no file changes and provided no patch".to_owned(),
        ));
    }
    let approach = extract_approach_from_text(text);
    Ok(PatchResponse {
        patch: diff,
        approach,
        self_metrics: std::collections::BTreeMap::new(),
    })
}

fn extract_approach_from_text(text: &str) -> String {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(3)
        .collect();
    if lines.is_empty() {
        "Subagent made changes but did not provide an approach summary".to_owned()
    } else {
        lines.join(" ")
    }
}

fn subagent_protocol_error(error: fx_subagent::SubagentError) -> ConsensusError {
    ConsensusError::Protocol(format!("subagent execution failed: {error}"))
}

fn strategy_slug(strategy: &GenerationStrategy) -> &'static str {
    match strategy {
        GenerationStrategy::Conservative => "conservative",
        GenerationStrategy::Aggressive => "aggressive",
        GenerationStrategy::Creative => "creative",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        FitnessCriterion, MetricType, ModificationScope, PathPattern, ProposalTier, Severity,
        Signal,
    };
    use chrono::Utc;
    use fx_subagent::{SubagentError, SubagentHandle, SubagentId};
    use std::sync::Mutex;
    use std::time::Instant;

    #[derive(Debug)]
    struct MockSubagentControl {
        state: Mutex<MockState>,
        spawn_error: Option<SubagentError>,
    }

    #[derive(Debug)]
    struct MockState {
        configs: Vec<SpawnConfig>,
        statuses: Vec<SubagentStatus>,
        cancelled: Vec<SubagentId>,
    }

    impl MockSubagentControl {
        fn new(statuses: Vec<SubagentStatus>) -> Self {
            Self {
                state: Mutex::new(MockState {
                    configs: Vec::new(),
                    statuses,
                    cancelled: Vec::new(),
                }),
                spawn_error: None,
            }
        }

        fn with_spawn_error(error: SubagentError) -> Self {
            Self {
                state: Mutex::new(MockState {
                    configs: Vec::new(),
                    statuses: Vec::new(),
                    cancelled: Vec::new(),
                }),
                spawn_error: Some(error),
            }
        }

        fn recorded_configs(&self) -> Vec<SpawnConfig> {
            self.state.lock().expect("mock lock").configs.clone()
        }

        fn cancelled_ids(&self) -> Vec<SubagentId> {
            self.state.lock().expect("mock lock").cancelled.clone()
        }
    }

    #[async_trait::async_trait]
    impl SubagentControl for MockSubagentControl {
        async fn spawn(&self, config: SpawnConfig) -> Result<SubagentHandle, SubagentError> {
            if let Some(error) = &self.spawn_error {
                return Err(error.clone());
            }
            self.state.lock().expect("mock lock").configs.push(config);
            Ok(SubagentHandle {
                id: SubagentId("agent-1".to_owned()),
                label: Some("experiment-conservative".to_owned()),
                status: SubagentStatus::Running,
                mode: SpawnMode::Run,
                started_at: Instant::now(),
                initial_response: None,
            })
        }

        async fn status(&self, _id: &SubagentId) -> Result<Option<SubagentStatus>, SubagentError> {
            let mut state = self.state.lock().expect("mock lock");
            let status = if state.statuses.len() > 1 {
                state.statuses.remove(0)
            } else {
                state
                    .statuses
                    .first()
                    .cloned()
                    .unwrap_or(SubagentStatus::Running)
            };
            Ok(Some(status))
        }

        async fn list(&self) -> Result<Vec<SubagentHandle>, SubagentError> {
            Ok(Vec::new())
        }

        async fn cancel(&self, id: &SubagentId) -> Result<(), SubagentError> {
            self.state
                .lock()
                .expect("mock lock")
                .cancelled
                .push(id.clone());
            Ok(())
        }

        async fn send(&self, _id: &SubagentId, _message: &str) -> Result<String, SubagentError> {
            Ok(String::new())
        }

        async fn gc(&self, _max_age: Duration) -> Result<(), SubagentError> {
            Ok(())
        }
    }

    #[test]
    fn spawn_config_uses_run_mode_system_prompt_and_task() {
        let manager = Arc::new(MockSubagentControl::new(vec![SubagentStatus::Running]));
        let source = SubagentPatchSource::new(
            manager.clone(),
            GenerationStrategy::Aggressive,
            PathBuf::from("/tmp/project"),
        );

        let config =
            source.spawn_config("system prompt", &sample_experiment(Duration::from_secs(60)));

        assert_eq!(config.mode, SpawnMode::Run);
        assert_eq!(config.system_prompt.as_deref(), Some("system prompt"));
        assert_eq!(
            config.cwd.as_deref(),
            Some(PathBuf::from("/tmp/project").as_path())
        );
        assert!(config
            .task
            .contains("Signal: latency — High latency detected"));
        assert!(config.task.contains("You MUST use tools"));
        assert!(config
            .task
            .contains("Use read_file to read EVERY target file"));
        assert!(config.task.contains("cargo build 2>&1"));
    }

    #[tokio::test]
    async fn generate_patch_parses_completed_subagent_response() {
        let manager = Arc::new(MockSubagentControl::new(vec![SubagentStatus::Completed {
            result: concat!(
                "<PATCH>
            ",
                "diff --git a/src/lib.rs b/src/lib.rs
            ",
                "--- a/src/lib.rs
            ",
                "+++ b/src/lib.rs
            ",
                "@@ -1 +1 @@
            ",
                "-old
            ",
                "+new
            ",
                "</PATCH>
            ",
                "<APPROACH>
            ",
                "Used a subagent-produced patch.
            ",
                "</APPROACH>
            ",
                "<METRICS>
            ",
                r#"{"build_success":1.0,"test_pass_rate":0.8,"signal_resolution":0.7}
            "#,
                "</METRICS>"
            )
            .to_owned(),
            tokens_used: 42,
        }]));
        let source = SubagentPatchSource::new(
            manager.clone(),
            GenerationStrategy::Conservative,
            PathBuf::from("/tmp/project"),
        );

        let response = source
            .generate_patch("system prompt", &sample_experiment(Duration::from_secs(60)))
            .await
            .expect("patch response");

        assert!(response
            .patch
            .contains("diff --git a/src/lib.rs b/src/lib.rs"));
        assert_eq!(response.approach, "Used a subagent-produced patch.");
        assert_eq!(response.self_metrics.get("build_success"), Some(&1.0));
        assert_eq!(response.self_metrics.get("test_pass_rate"), Some(&0.8));
        assert_eq!(response.self_metrics.get("signal_resolution"), Some(&0.7));
        let configs = manager.recorded_configs();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].label.as_deref(), Some("experiment-conservative"));
    }

    #[tokio::test]
    async fn generate_patch_returns_spawn_failure() {
        let manager = Arc::new(MockSubagentControl::with_spawn_error(SubagentError::Spawn(
            "boom".to_owned(),
        )));
        let source = SubagentPatchSource::new(
            manager,
            GenerationStrategy::Conservative,
            PathBuf::from("/tmp/project"),
        );

        let error = source
            .generate_patch("system prompt", &sample_experiment(Duration::from_secs(60)))
            .await
            .err()
            .expect("spawn should fail");

        assert!(error
            .to_string()
            .contains("subagent execution failed: subagent spawn failed: boom"));
    }

    #[tokio::test]
    async fn generate_patch_returns_failure_status() {
        let manager = Arc::new(MockSubagentControl::new(vec![SubagentStatus::Failed {
            error: "boom".to_owned(),
        }]));
        let source = SubagentPatchSource::new(
            manager,
            GenerationStrategy::Conservative,
            PathBuf::from("/tmp/project"),
        );

        let error = source
            .generate_patch("system prompt", &sample_experiment(Duration::from_secs(60)))
            .await
            .err()
            .expect("failed status should error");

        assert!(error.to_string().contains("subagent failed: boom"));
    }

    #[tokio::test]
    async fn generate_patch_returns_cancelled_status() {
        let manager = Arc::new(MockSubagentControl::new(vec![SubagentStatus::Cancelled]));
        let source = SubagentPatchSource::new(
            manager,
            GenerationStrategy::Conservative,
            PathBuf::from("/tmp/project"),
        );

        let error = source
            .generate_patch("system prompt", &sample_experiment(Duration::from_secs(60)))
            .await
            .err()
            .expect("cancelled status should error");

        assert!(error.to_string().contains("subagent cancelled: agent-1"));
    }

    #[tokio::test]
    async fn generate_patch_times_out_and_cancels_subagent() {
        let manager = Arc::new(MockSubagentControl::new(vec![SubagentStatus::Running]));
        let source = SubagentPatchSource::new(
            manager.clone(),
            GenerationStrategy::Conservative,
            PathBuf::from("/tmp/project"),
        )
        .with_poll_interval(Duration::from_millis(5));

        let error = source
            .generate_patch(
                "system prompt",
                &sample_experiment(Duration::from_millis(20)),
            )
            .await
            .err()
            .expect("timeout should fail");

        assert!(error
            .to_string()
            .contains("subagent did not respond within"));
        assert_eq!(
            manager.cancelled_ids(),
            vec![SubagentId("agent-1".to_owned())]
        );
    }

    #[tokio::test]
    async fn generate_patch_rejects_response_without_patch() {
        let manager = Arc::new(MockSubagentControl::new(vec![SubagentStatus::Completed {
            result: concat!(
                "<APPROACH>Summarized the change.</APPROACH>",
                "<METRICS>{\"build_success\":1.0,\"test_pass_rate\":1.0,\"signal_resolution\":1.0}</METRICS>"
            )
            .to_owned(),
            tokens_used: 7,
        }]));
        let source = SubagentPatchSource::new(
            manager,
            GenerationStrategy::Conservative,
            PathBuf::from("/tmp/project"),
        );

        let error = source
            .generate_patch("system prompt", &sample_experiment(Duration::from_secs(60)))
            .await
            .err()
            .expect("missing patch should fail");

        // Fallback runs git diff HEAD on /tmp/project which doesn't exist as a repo,
        // so it falls back to returning the original parse error
        let error_text = error.to_string();
        assert!(
            error_text.contains("git diff failed")
                || error_text.contains("no file changes")
                || error_text.contains("did not include a diff patch"),
            "unexpected error: {error_text}"
        );
    }

    #[test]
    fn extract_approach_from_text_uses_first_three_lines() {
        let text = "I added tests for scoring.\nAll tests pass.\nBuild successful.\nExtra line.";
        let approach = extract_approach_from_text(text);
        assert_eq!(
            approach,
            "I added tests for scoring. All tests pass. Build successful."
        );
    }

    #[test]
    fn extract_approach_from_empty_text_returns_default() {
        let approach = extract_approach_from_text("");
        assert!(approach.contains("did not provide an approach"));
    }

    fn sample_experiment(timeout: Duration) -> Experiment {
        Experiment {
            id: uuid::Uuid::new_v4(),
            trigger: Signal {
                id: uuid::Uuid::new_v4(),
                name: "latency".to_owned(),
                description: "High latency detected".to_owned(),
                severity: Severity::High,
            },
            hypothesis: "parallelism helps".to_owned(),
            fitness_criteria: vec![FitnessCriterion {
                name: "build_success".to_owned(),
                metric_type: MetricType::Higher,
                weight: 1.0,
            }],
            scope: ModificationScope {
                allowed_files: vec![PathPattern::from("src/**/*.rs")],
                proposal_tier: ProposalTier::Tier1,
            },
            timeout,
            min_candidates: 1,
            created_at: Utc::now(),
        }
    }
}
