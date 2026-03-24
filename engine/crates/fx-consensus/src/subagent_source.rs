use crate::cargo_workspace::CargoWorkspace;
use crate::llm_source::build_subagent_experiment_prompt;
use crate::response_parser::parse_patch_response;
use crate::{ConsensusError, Experiment, GenerationStrategy, PatchResponse, PatchSource};
use fx_subagent::{
    SpawnConfig, SpawnMode, SubagentControl, SubagentHandle, SubagentId, SubagentStatus,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::process::Command;
use tracing::warn;

const BUILD_RETRY_LIMIT: usize = 1;
const BUILD_ERROR_TAIL_LINES: usize = 60;
const DEFAULT_BUILD_VERIFY_TIMEOUT: Duration = Duration::from_secs(120);

pub struct SubagentPatchSource {
    manager: Arc<dyn SubagentControl>,
    strategy: GenerationStrategy,
    working_dir: PathBuf,
    chain_history: String,
    /// Keeps the cloned workspace alive so the temp directory is not deleted.
    _workspace: Option<CargoWorkspace>,
    poll_interval: Duration,
    build_timeout: Duration,
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
            chain_history: String::new(),
            _workspace: None,
            poll_interval: Duration::from_millis(100),
            build_timeout: DEFAULT_BUILD_VERIFY_TIMEOUT,
        }
    }

    pub fn with_workspace(
        manager: Arc<dyn SubagentControl>,
        strategy: GenerationStrategy,
        workspace: CargoWorkspace,
    ) -> Self {
        let working_dir = workspace.project_dir().to_path_buf();
        Self {
            manager,
            strategy,
            working_dir,
            chain_history: String::new(),
            _workspace: Some(workspace),
            poll_interval: Duration::from_millis(100),
            build_timeout: DEFAULT_BUILD_VERIFY_TIMEOUT,
        }
    }

    pub fn with_chain_history(mut self, chain_history: String) -> Self {
        self.chain_history = chain_history;
        self
    }

    #[cfg(test)]
    fn with_poll_interval(mut self, poll_interval: Duration) -> Self {
        self.poll_interval = poll_interval;
        self
    }

    #[cfg(test)]
    fn with_build_timeout(mut self, build_timeout: Duration) -> Self {
        self.build_timeout = build_timeout;
        self
    }

    fn spawn_config(&self, system_prompt: &str, experiment: &Experiment) -> SpawnConfig {
        let mut config = SpawnConfig::new(build_subagent_experiment_prompt(
            experiment,
            &self.chain_history,
        ));
        config.label = Some(format!("experiment-{}", strategy_slug(&self.strategy)));
        config.mode = SpawnMode::Session;
        config.timeout = experiment.timeout;
        config.cwd = Some(self.working_dir.clone());
        config.system_prompt = Some(system_prompt.to_owned());
        config
    }

    async fn run_session(
        &self,
        handle: &SubagentHandle,
        experiment: &Experiment,
    ) -> Result<PatchResponse, ConsensusError> {
        let deadline = Instant::now() + experiment.timeout;
        let initial_text = self
            .await_initial_response(handle, experiment.timeout)
            .await?;
        self.verify_build_or_retry(&handle.id, initial_text, deadline, experiment.timeout)
            .await
    }

    async fn await_initial_response(
        &self,
        handle: &SubagentHandle,
        timeout: Duration,
    ) -> Result<String, ConsensusError> {
        if let Some(response) = handle.initial_response.clone() {
            return Ok(response);
        }
        let handle = await_handle(&*self.manager, &handle.id, self.poll_interval, timeout).await?;
        handle
            .initial_response
            .ok_or_else(|| handle_status_error(&handle.id, handle.status))
    }

    async fn verify_build_or_retry(
        &self,
        id: &SubagentId,
        initial_text: String,
        deadline: Instant,
        experiment_timeout: Duration,
    ) -> Result<PatchResponse, ConsensusError> {
        let mut response_text = initial_text;
        let mut retries_left = BUILD_RETRY_LIMIT;
        loop {
            let build_timeout =
                effective_build_timeout(id, deadline, experiment_timeout, self.build_timeout)?;
            match verify_build(&self.working_dir, build_timeout).await {
                Ok(()) => return parse_verified_response(&self.working_dir, &response_text).await,
                Err(ConsensusError::BuildFailed(output)) if retries_left > 0 => {
                    response_text = self
                        .retry_after_build_failure(id, &output, deadline, experiment_timeout)
                        .await?;
                    retries_left -= 1;
                }
                Err(ConsensusError::BuildFailed(output)) => return Err(final_build_error(output)),
                Err(error) => return Err(error),
            }
        }
    }

    async fn retry_after_build_failure(
        &self,
        id: &SubagentId,
        build_output: &str,
        deadline: Instant,
        experiment_timeout: Duration,
    ) -> Result<String, ConsensusError> {
        let retry_timeout = remaining_experiment_timeout(id, deadline, experiment_timeout)?;
        let prompt = build_retry_prompt(build_output);
        tokio::time::timeout(retry_timeout, self.manager.send(id, &prompt))
            .await
            .map_err(|_| retry_timeout_error(id, retry_timeout))?
            .map_err(subagent_protocol_error)
    }

    async fn cancel_session(&self, id: &SubagentId) {
        if let Err(error) = self.manager.cancel(id).await {
            warn!(%id, %error, "failed to cancel experiment subagent session");
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
        let result = self.run_session(&handle, experiment).await;
        self.cancel_session(&handle.id).await;
        result
    }
}

async fn await_handle(
    manager: &dyn SubagentControl,
    id: &SubagentId,
    poll_interval: Duration,
    timeout: Duration,
) -> Result<SubagentHandle, ConsensusError> {
    let wait = tokio::time::timeout(timeout, async {
        loop {
            let handle = find_handle(manager, id).await?;
            if handle.initial_response.is_some() || handle.status != SubagentStatus::Running {
                return Ok(handle);
            }
            tokio::time::sleep(poll_interval).await;
        }
    })
    .await;
    match wait {
        Ok(result) => result,
        Err(_) => Err(timeout_error(id, timeout)),
    }
}

async fn find_handle(
    manager: &dyn SubagentControl,
    id: &SubagentId,
) -> Result<SubagentHandle, ConsensusError> {
    manager
        .get(id)
        .await
        .map_err(subagent_protocol_error)?
        .ok_or_else(|| ConsensusError::Protocol(format!("subagent disappeared: {id}")))
}

fn handle_status_error(id: &SubagentId, status: SubagentStatus) -> ConsensusError {
    match status {
        SubagentStatus::Failed { error } => {
            ConsensusError::Protocol(format!("subagent failed: {error}"))
        }
        SubagentStatus::Cancelled => ConsensusError::Protocol(format!("subagent cancelled: {id}")),
        SubagentStatus::TimedOut => ConsensusError::Protocol(format!("subagent timed out: {id}")),
        SubagentStatus::Completed { .. } | SubagentStatus::Running => {
            ConsensusError::Protocol(format!("subagent did not provide a session response: {id}"))
        }
    }
}

fn timeout_error(id: &SubagentId, timeout: Duration) -> ConsensusError {
    ConsensusError::NoConsensus(format!(
        "subagent did not respond within {}s: {id}",
        timeout.as_secs_f64()
    ))
}

fn remaining_experiment_timeout(
    id: &SubagentId,
    deadline: Instant,
    experiment_timeout: Duration,
) -> Result<Duration, ConsensusError> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(timeout_error(id, experiment_timeout));
    }
    Ok(remaining)
}

fn effective_build_timeout(
    id: &SubagentId,
    deadline: Instant,
    experiment_timeout: Duration,
    build_timeout: Duration,
) -> Result<Duration, ConsensusError> {
    Ok(remaining_experiment_timeout(id, deadline, experiment_timeout)?.min(build_timeout))
}

fn retry_timeout_error(id: &SubagentId, timeout: Duration) -> ConsensusError {
    ConsensusError::NoConsensus(format!(
        "subagent retry did not respond within {}s: {id}",
        timeout.as_secs_f64()
    ))
}

async fn parse_verified_response(
    working_dir: &Path,
    text: &str,
) -> Result<PatchResponse, ConsensusError> {
    match parse_patch_response(text) {
        Ok(response) => Ok(verified_response(response)),
        Err(parse_error) => {
            warn!(
                error = %parse_error,
                "subagent response missing patch tags, falling back to git diff"
            );
            fallback_git_diff(working_dir, text, parse_error)
                .await
                .map(verified_response)
        }
    }
}

fn verified_response(mut response: PatchResponse) -> PatchResponse {
    response
        .self_metrics
        .insert("build_success".to_owned(), 1.0);
    response
}

async fn verify_build(working_dir: &Path, timeout: Duration) -> Result<(), ConsensusError> {
    let mut command = Command::new("cargo");
    command
        .arg("check")
        .current_dir(working_dir)
        .kill_on_drop(true);
    let output = tokio::time::timeout(timeout, command.output())
        .await
        .map_err(|_| build_timeout_error(timeout))?
        .map_err(|error| ConsensusError::Protocol(format!("cargo check failed: {error}")))?;
    if output.status.success() {
        return Ok(());
    }
    Err(ConsensusError::BuildFailed(command_output(
        &output.stdout,
        &output.stderr,
    )))
}

fn build_timeout_error(timeout: Duration) -> ConsensusError {
    ConsensusError::BuildFailed(format!(
        "cargo check timed out after {}s",
        timeout.as_secs_f64()
    ))
}

fn final_build_error(output: String) -> ConsensusError {
    ConsensusError::BuildFailed(format!(
        "subagent candidate failed build verification after retry:\n{}",
        tail_lines(&output, BUILD_ERROR_TAIL_LINES)
    ))
}

fn build_retry_prompt(build_output: &str) -> String {
    format!(
        concat!(
            "Your last patch does not compile in the current workspace.\n",
            "Fix the build errors below, rerun `cargo check`, and then reply again with updated ",
            "<PATCH>, <APPROACH>, and <METRICS> tags.\n\n",
            "Build output:\n{}"
        ),
        tail_lines(build_output, BUILD_ERROR_TAIL_LINES)
    )
}

fn tail_lines(output: &str, count: usize) -> String {
    let lines = output.lines().collect::<Vec<_>>();
    if lines.is_empty() {
        return "(no build output captured)".to_owned();
    }
    let start = lines.len().saturating_sub(count);
    lines[start..].join("\n")
}

fn command_output(stdout: &[u8], stderr: &[u8]) -> String {
    let mut sections = Vec::new();
    push_command_section(&mut sections, "stdout", stdout);
    push_command_section(&mut sections, "stderr", stderr);
    sections.join("\n")
}

fn push_command_section(sections: &mut Vec<String>, label: &str, bytes: &[u8]) {
    let text = String::from_utf8_lossy(bytes).trim().to_owned();
    if !text.is_empty() {
        sections.push(format!("--- {label} ---\n{text}"));
    }
}

async fn fallback_git_diff(
    working_dir: &Path,
    text: &str,
    parse_error: ConsensusError,
) -> Result<PatchResponse, ConsensusError> {
    let output = Command::new("git")
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
    let diff = String::from_utf8_lossy(&output.stdout).into_owned();
    if diff.trim().is_empty() {
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
    use fx_subagent::{SubagentError, SubagentHandle};
    use std::collections::VecDeque;
    use std::fs;
    use std::sync::Mutex;
    use std::time::Instant;
    use tempfile::TempDir;

    #[derive(Debug)]
    struct MockSubagentControl {
        state: Mutex<MockState>,
        spawn_error: Option<SubagentError>,
    }

    #[derive(Debug)]
    struct MockState {
        configs: Vec<SpawnConfig>,
        handles: VecDeque<SubagentHandle>,
        send_steps: VecDeque<SendStep>,
        sent_messages: Vec<String>,
        cancelled: Vec<SubagentId>,
    }

    #[derive(Debug)]
    struct SendStep {
        reply: Result<String, SubagentError>,
        file_update: Option<FileUpdate>,
        delay: Duration,
    }

    #[derive(Debug)]
    struct FileUpdate {
        path: PathBuf,
        contents: String,
    }

    impl MockSubagentControl {
        fn new(handles: Vec<SubagentHandle>, send_steps: Vec<SendStep>) -> Self {
            Self {
                state: Mutex::new(MockState {
                    configs: Vec::new(),
                    handles: VecDeque::from(handles),
                    send_steps: VecDeque::from(send_steps),
                    sent_messages: Vec::new(),
                    cancelled: Vec::new(),
                }),
                spawn_error: None,
            }
        }

        fn with_spawn_error(error: SubagentError) -> Self {
            Self {
                state: Mutex::new(MockState {
                    configs: Vec::new(),
                    handles: VecDeque::new(),
                    send_steps: VecDeque::new(),
                    sent_messages: Vec::new(),
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

        fn sent_messages(&self) -> Vec<String> {
            self.state.lock().expect("mock lock").sent_messages.clone()
        }
    }

    #[async_trait::async_trait]
    impl SubagentControl for MockSubagentControl {
        async fn spawn(
            &self,
            config: SpawnConfig,
        ) -> Result<fx_subagent::SubagentHandle, SubagentError> {
            if let Some(error) = &self.spawn_error {
                return Err(error.clone());
            }
            self.state
                .lock()
                .expect("mock lock")
                .configs
                .push(config.clone());
            Ok(SubagentHandle {
                id: SubagentId("agent-1".to_owned()),
                label: config.label,
                status: SubagentStatus::Running,
                mode: config.mode,
                started_at: Instant::now(),
                initial_response: None,
            })
        }

        async fn status(&self, _id: &SubagentId) -> Result<Option<SubagentStatus>, SubagentError> {
            let state = self.state.lock().expect("mock lock");
            Ok(state.handles.front().map(|handle| handle.status.clone()))
        }

        async fn list(&self) -> Result<Vec<SubagentHandle>, SubagentError> {
            let mut state = self.state.lock().expect("mock lock");
            let handle = if state.handles.len() > 1 {
                state.handles.pop_front()
            } else {
                state.handles.front().cloned()
            };
            Ok(handle.into_iter().collect())
        }

        async fn get(&self, id: &SubagentId) -> Result<Option<SubagentHandle>, SubagentError> {
            let mut state = self.state.lock().expect("mock lock");
            let handle = if state.handles.len() > 1 {
                state.handles.pop_front()
            } else {
                state.handles.front().cloned()
            };
            Ok(handle.filter(|handle| handle.id == *id))
        }

        async fn cancel(&self, id: &SubagentId) -> Result<(), SubagentError> {
            self.state
                .lock()
                .expect("mock lock")
                .cancelled
                .push(id.clone());
            Ok(())
        }

        async fn send(&self, _id: &SubagentId, message: &str) -> Result<String, SubagentError> {
            let step = {
                let mut state = self.state.lock().expect("mock lock");
                state.sent_messages.push(message.to_owned());
                state.send_steps.pop_front().unwrap_or(SendStep {
                    reply: Ok(String::new()),
                    file_update: None,
                    delay: Duration::ZERO,
                })
            };
            if !step.delay.is_zero() {
                tokio::time::sleep(step.delay).await;
            }
            if let Some(update) = step.file_update {
                fs::write(update.path, update.contents)
                    .map_err(|error| SubagentError::Execution(error.to_string()))?;
            }
            step.reply
        }

        async fn gc(&self, _max_age: Duration) -> Result<(), SubagentError> {
            Ok(())
        }
    }

    #[derive(Debug)]
    struct GetOnlySubagentControl {
        handle: SubagentHandle,
    }

    #[async_trait::async_trait]
    impl SubagentControl for GetOnlySubagentControl {
        async fn spawn(&self, _config: SpawnConfig) -> Result<SubagentHandle, SubagentError> {
            unreachable!("spawn is not used in this test")
        }

        async fn status(&self, _id: &SubagentId) -> Result<Option<SubagentStatus>, SubagentError> {
            Ok(Some(self.handle.status.clone()))
        }

        async fn list(&self) -> Result<Vec<SubagentHandle>, SubagentError> {
            panic!("find_handle should use get instead of list")
        }

        async fn get(&self, id: &SubagentId) -> Result<Option<SubagentHandle>, SubagentError> {
            Ok((self.handle.id == *id).then_some(self.handle.clone()))
        }

        async fn cancel(&self, _id: &SubagentId) -> Result<(), SubagentError> {
            unreachable!("cancel is not used in this test")
        }

        async fn send(&self, _id: &SubagentId, _message: &str) -> Result<String, SubagentError> {
            unreachable!("send is not used in this test")
        }

        async fn gc(&self, _max_age: Duration) -> Result<(), SubagentError> {
            unreachable!("gc is not used in this test")
        }
    }

    #[test]
    fn spawn_config_uses_session_mode_system_prompt_and_task() {
        let manager = Arc::new(MockSubagentControl::new(Vec::new(), Vec::new()));
        let source = SubagentPatchSource::new(
            manager,
            GenerationStrategy::Aggressive,
            PathBuf::from("/tmp/project"),
        )
        .with_chain_history(
            "- Entry #2 | hypothesis: tried batching | decision: reject".to_owned(),
        );

        let config =
            source.spawn_config("system prompt", &sample_experiment(Duration::from_secs(60)));

        assert_eq!(config.mode, SpawnMode::Session);
        assert_eq!(config.timeout, Duration::from_secs(60));
        assert_eq!(config.system_prompt.as_deref(), Some("system prompt"));
        assert_eq!(
            config.cwd.as_deref(),
            Some(PathBuf::from("/tmp/project").as_path())
        );
        assert!(config
            .task
            .contains("Signal: latency — High latency detected"));
        assert!(config.task.contains("Recent experiments for this signal:"));
        assert!(config
            .task
            .contains("Entry #2 | hypothesis: tried batching"));
        assert!(config.task.contains("You MUST use tools"));
        assert!(config
            .task
            .contains("Use read_file to read EVERY target file"));
        assert!(config.task.contains("cargo check 2>&1"));
    }

    #[tokio::test]
    async fn generate_patch_returns_verified_initial_response_and_cancels_session() {
        let (_repo, workspace, project_dir) = cloned_workspace("verified-initial");
        fs::write(
            project_dir.join("src/lib.rs"),
            "pub fn value() -> i32 { 2 }\n",
        )
        .expect("write updated library");
        let manager = Arc::new(MockSubagentControl::new(
            vec![running_handle(Some(&patch_response_text(
                diff_patch(1, 2),
                "Used a verified patch.",
                0.0,
            )))],
            Vec::new(),
        ));
        let source = SubagentPatchSource::with_workspace(
            manager.clone(),
            GenerationStrategy::Conservative,
            workspace,
        );

        let response = source
            .generate_patch("system prompt", &sample_experiment(Duration::from_secs(60)))
            .await
            .expect("patch response");

        assert_eq!(response.approach, "Used a verified patch.");
        assert_eq!(response.self_metrics.get("build_success"), Some(&1.0));
        assert!(response
            .patch
            .contains("diff --git a/src/lib.rs b/src/lib.rs"));
        assert_eq!(manager.recorded_configs()[0].mode, SpawnMode::Session);
        assert_eq!(
            manager.cancelled_ids(),
            vec![SubagentId("agent-1".to_owned())]
        );
    }

    #[tokio::test]
    async fn generate_patch_retries_once_after_build_failure() {
        let (_repo, workspace, project_dir) = cloned_workspace("retry-success");
        fs::write(project_dir.join("src/lib.rs"), invalid_library()).expect("write invalid lib");
        let manager = Arc::new(MockSubagentControl::new(
            vec![running_handle(Some(&patch_response_text(
                diff_patch(1, 999),
                "First attempt compiled in my head.",
                1.0,
            )))],
            vec![SendStep {
                reply: Ok(patch_response_text(
                    diff_patch(1, 2),
                    "Fixed the build error.",
                    1.0,
                )),
                file_update: Some(FileUpdate {
                    path: project_dir.join("src/lib.rs"),
                    contents: "pub fn value() -> i32 { 2 }\n".to_owned(),
                }),
                delay: Duration::ZERO,
            }],
        ));
        let source = SubagentPatchSource::with_workspace(
            manager.clone(),
            GenerationStrategy::Conservative,
            workspace,
        )
        .with_build_timeout(Duration::from_secs(5));

        let response = source
            .generate_patch("system prompt", &sample_experiment(Duration::from_secs(60)))
            .await
            .expect("retry should succeed");

        assert_eq!(response.approach, "Fixed the build error.");
        assert_eq!(response.self_metrics.get("build_success"), Some(&1.0));
        assert_eq!(manager.sent_messages().len(), 1);
        assert!(manager.sent_messages()[0].contains("does not compile"));
        assert!(manager.sent_messages()[0].contains("Fix the build errors"));
        assert!(manager.sent_messages()[0].contains("rerun `cargo check`"));
    }

    #[tokio::test]
    async fn generate_patch_returns_error_when_retry_still_fails_to_build() {
        let (_repo, workspace, project_dir) = cloned_workspace("retry-fail");
        fs::write(project_dir.join("src/lib.rs"), invalid_library()).expect("write invalid lib");
        let manager = Arc::new(MockSubagentControl::new(
            vec![running_handle(Some(&patch_response_text(
                diff_patch(1, 999),
                "Broken patch.",
                1.0,
            )))],
            vec![SendStep {
                reply: Ok(patch_response_text(
                    diff_patch(1, 999),
                    "Still broken.",
                    1.0,
                )),
                file_update: None,
                delay: Duration::ZERO,
            }],
        ));
        let source = SubagentPatchSource::with_workspace(
            manager.clone(),
            GenerationStrategy::Conservative,
            workspace,
        );

        let error = source
            .generate_patch("system prompt", &sample_experiment(Duration::from_secs(60)))
            .await
            .err()
            .expect("retry should still fail");

        assert!(error
            .to_string()
            .contains("subagent candidate failed build verification after retry"));
        assert_eq!(manager.sent_messages().len(), 1);
        assert_eq!(
            manager.cancelled_ids(),
            vec![SubagentId("agent-1".to_owned())]
        );
    }

    #[tokio::test]
    async fn generate_patch_uses_git_diff_fallback_after_successful_build() {
        let (_repo, workspace, project_dir) = cloned_workspace("git-diff-fallback");
        fs::write(
            project_dir.join("src/lib.rs"),
            "pub fn value() -> i32 { 2 }\n",
        )
        .expect("write updated library");
        let manager = Arc::new(MockSubagentControl::new(
            vec![running_handle(Some(
                "Updated the return value.\nBuild now passes cleanly.",
            ))],
            Vec::new(),
        ));
        let source = SubagentPatchSource::with_workspace(
            manager,
            GenerationStrategy::Conservative,
            workspace,
        );

        let response = source
            .generate_patch("system prompt", &sample_experiment(Duration::from_secs(60)))
            .await
            .expect("git diff fallback should succeed");

        assert!(response
            .patch
            .contains("diff --git a/src/lib.rs b/src/lib.rs"));
        assert_eq!(
            response.approach,
            "Updated the return value. Build now passes cleanly."
        );
        assert_eq!(response.self_metrics.get("build_success"), Some(&1.0));
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
        let manager = Arc::new(MockSubagentControl::new(
            vec![failed_handle("boom")],
            Vec::new(),
        ));
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
    async fn generate_patch_times_out_waiting_for_initial_response_and_cancels_subagent() {
        let manager = Arc::new(MockSubagentControl::new(
            vec![running_handle(None)],
            Vec::new(),
        ));
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
    async fn find_handle_uses_targeted_get_lookup() {
        let id = SubagentId("agent-1".to_owned());
        let control = GetOnlySubagentControl {
            handle: running_handle(Some("ready")),
        };

        let handle = find_handle(&control, &id).await.expect("find handle");

        assert_eq!(handle.id, id);
        assert_eq!(handle.initial_response.as_deref(), Some("ready"));
    }

    #[tokio::test]
    async fn retry_after_build_failure_times_out_when_send_blocks() {
        let manager = Arc::new(MockSubagentControl::new(
            Vec::new(),
            vec![SendStep {
                reply: Ok("late reply".to_owned()),
                file_update: None,
                delay: Duration::from_millis(50),
            }],
        ));
        let source = SubagentPatchSource::new(
            manager,
            GenerationStrategy::Conservative,
            PathBuf::from("/tmp/project"),
        );
        let id = SubagentId("agent-1".to_owned());

        let error = source
            .retry_after_build_failure(
                &id,
                "compile error",
                Instant::now() + Duration::from_millis(10),
                Duration::from_millis(10),
            )
            .await
            .expect_err("retry send should time out");

        assert!(error
            .to_string()
            .contains("subagent retry did not respond within"));
    }

    #[tokio::test]
    async fn verify_build_times_out_long_running_cargo_check() {
        let temp = TempDir::new().expect("temp dir");
        fs::create_dir_all(temp.path().join("src")).expect("src dir");
        fs::write(
            temp.path().join("Cargo.toml"),
            concat!(
                "[package]\n",
                "name = \"slow-build\"\n",
                "version = \"0.1.0\"\n",
                "edition = \"2021\"\n",
                "build = \"build.rs\"\n"
            ),
        )
        .expect("manifest");
        fs::write(
            temp.path().join("src/lib.rs"),
            "pub fn value() -> i32 { 1 }\n",
        )
        .expect("library");
        fs::write(
            temp.path().join("build.rs"),
            concat!(
                "fn main() {\n",
                "    std::thread::sleep(std::time::Duration::from_secs(2));\n",
                "}\n"
            ),
        )
        .expect("build script");

        let error = verify_build(temp.path(), Duration::from_millis(50))
            .await
            .expect_err("cargo check should time out");

        assert!(error.to_string().contains("cargo check timed out"));
    }

    #[test]
    fn tail_lines_returns_last_lines_in_original_order() {
        let output = "one\ntwo\nthree\nfour";

        assert_eq!(tail_lines(output, 2), "three\nfour");
    }

    #[test]
    fn command_output_separates_stdout_and_stderr() {
        let output = command_output(b"compiled\n", b"warning\n");

        assert_eq!(output, "--- stdout ---\ncompiled\n--- stderr ---\nwarning");
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

    #[test]
    fn with_workspace_keeps_temp_dir_alive() {
        let (_repo, workspace, cloned_dir) = cloned_workspace("keep-alive");
        assert!(
            cloned_dir.exists(),
            "cloned dir should exist before ownership transfer"
        );

        let manager: Arc<dyn SubagentControl> =
            Arc::new(MockSubagentControl::new(Vec::new(), Vec::new()));
        let source = SubagentPatchSource::with_workspace(
            manager,
            GenerationStrategy::Conservative,
            workspace,
        );

        assert!(
            cloned_dir.exists(),
            "cloned dir must survive — SubagentPatchSource owns the workspace"
        );
        assert_eq!(source.working_dir, cloned_dir);

        drop(source);
        assert!(
            !cloned_dir.exists(),
            "cloned dir should be cleaned up after SubagentPatchSource is dropped"
        );
    }

    fn running_handle(initial_response: Option<&str>) -> SubagentHandle {
        SubagentHandle {
            id: SubagentId("agent-1".to_owned()),
            label: Some("experiment-conservative".to_owned()),
            status: SubagentStatus::Running,
            mode: SpawnMode::Session,
            started_at: Instant::now(),
            initial_response: initial_response.map(str::to_owned),
        }
    }

    fn failed_handle(error: &str) -> SubagentHandle {
        SubagentHandle {
            id: SubagentId("agent-1".to_owned()),
            label: Some("experiment-conservative".to_owned()),
            status: SubagentStatus::Failed {
                error: error.to_owned(),
            },
            mode: SpawnMode::Session,
            started_at: Instant::now(),
            initial_response: None,
        }
    }

    fn patch_response_text(patch: String, approach: &str, build_success: f64) -> String {
        format!(
            concat!(
                "<PATCH>\n",
                "{patch}\n",
                "</PATCH>\n",
                "<APPROACH>\n",
                "{approach}\n",
                "</APPROACH>\n",
                "<METRICS>\n",
                "{{\"build_success\":{build_success},\"test_pass_rate\":1.0,\"signal_resolution\":0.8}}\n",
                "</METRICS>"
            ),
            patch = patch,
            approach = approach,
            build_success = build_success,
        )
    }

    fn diff_patch(old_value: i32, new_value: i32) -> String {
        format!(
            concat!(
                "diff --git a/src/lib.rs b/src/lib.rs\n",
                "--- a/src/lib.rs\n",
                "+++ b/src/lib.rs\n",
                "@@ -1 +1 @@\n",
                "-pub fn value() -> i32 {{ {old_value} }}\n",
                "+pub fn value() -> i32 {{ {new_value} }}"
            ),
            old_value = old_value,
            new_value = new_value,
        )
    }

    fn invalid_library() -> &'static str {
        "pub fn value() -> i32 { \"oops\" }\n"
    }

    fn cloned_workspace(label: &str) -> (TempDir, CargoWorkspace, PathBuf) {
        let temp = TempDir::new().expect("temp dir");
        init_git_project(temp.path());
        write_project_files(temp.path(), "pub fn value() -> i32 { 1 }\n");
        commit_all(temp.path(), "init");
        let workspace = CargoWorkspace::clone_from(temp.path(), label).expect("clone workspace");
        let project_dir = workspace.project_dir().to_path_buf();
        (temp, workspace, project_dir)
    }

    fn init_git_project(path: &Path) {
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(path)
            .status()
            .expect("git init");
        std::process::Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(path)
            .status()
            .expect("git config email");
        std::process::Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(path)
            .status()
            .expect("git config name");
    }

    fn write_project_files(path: &Path, library: &str) {
        fs::create_dir_all(path.join("src")).expect("src dir");
        fs::write(
            path.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .expect("manifest");
        fs::write(path.join("src/lib.rs"), library).expect("library");
    }

    fn commit_all(path: &Path, message: &str) {
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(path)
            .status()
            .expect("git add");
        std::process::Command::new("git")
            .args(["commit", "-m", message])
            .current_dir(path)
            .status()
            .expect("git commit");
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
