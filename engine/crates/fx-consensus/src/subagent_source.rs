use crate::llm_source::build_experiment_prompt;
use crate::response_parser::parse_patch_response;
use crate::{ConsensusError, Experiment, GenerationStrategy, PatchResponse, PatchSource};
use fx_subagent::{SpawnConfig, SpawnMode, SubagentControl, SubagentStatus};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

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
        let mut config = SpawnConfig::new(build_experiment_prompt(experiment));
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
        parse_patch_response(&text)
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
        assert!(config
            .task
            .contains("Return exactly three tagged sections in this order"));
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

        assert!(error
            .to_string()
            .contains("generated response did not include a diff patch"));
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
