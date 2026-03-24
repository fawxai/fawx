use crate::cargo_workspace::parse_test_result;
use crate::{ConsensusError, EvaluationWorkspace, Experiment, Result, Signal, TestResult};
use async_trait::async_trait;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::process::{Command as StdCommand, Output};
use std::sync::{Arc, Mutex, OnceLock};
use tempfile::NamedTempFile;
use tokio::process::Command;
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};
use uuid::Uuid;

static REMOTE_REPO_LOCKS: OnceLock<Mutex<BTreeMap<String, RepoLock>>> = OnceLock::new();

type RepoLock = Arc<AsyncMutex<()>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteEvalTarget {
    pub ssh_user: String,
    pub ssh_host: String,
    pub remote_project_dir: String,
}

impl std::str::FromStr for RemoteEvalTarget {
    type Err = ConsensusError;

    fn from_str(spec: &str) -> Result<Self> {
        let (user_host, remote_project_dir) = spec
            .split_once(':')
            .ok_or_else(|| invalid_target_format(spec))?;
        let (ssh_user, ssh_host) = user_host
            .split_once('@')
            .ok_or_else(|| invalid_target_format(spec))?;
        if ssh_user.is_empty() || ssh_host.is_empty() || remote_project_dir.is_empty() {
            return Err(invalid_target_format(spec));
        }
        if !remote_project_dir.starts_with('/') && !remote_project_dir.starts_with('~') {
            return Err(ConsensusError::WorkspaceError(format!(
                "remote_project_dir must be an absolute path, got '{remote_project_dir}'"
            )));
        }
        Ok(Self {
            ssh_user: ssh_user.to_owned(),
            ssh_host: ssh_host.to_owned(),
            remote_project_dir: remote_project_dir.to_owned(),
        })
    }
}

fn invalid_target_format(spec: &str) -> ConsensusError {
    ConsensusError::WorkspaceError(format!(
        "invalid eval node '{spec}'; expected user@host:/path"
    ))
}

pub struct RemoteEvaluationWorkspace {
    pub ssh_host: String,
    pub ssh_user: String,
    pub remote_project_dir: String,
    pub package: Option<String>,
    baseline_tests: TestResult,
    repo_lock: RepoLock,
    active_guard: AsyncMutex<Option<OwnedMutexGuard<()>>>,
}

impl RemoteEvaluationWorkspace {
    pub fn new(target: RemoteEvalTarget, package: Option<String>) -> Result<Self> {
        cleanup_remote_blocking(&target)?;
        let baseline_tests = collect_baseline_tests_blocking(&target, package.as_deref())?;
        let repo_key = remote_repo_key(&target);
        Ok(Self {
            ssh_host: target.ssh_host,
            ssh_user: target.ssh_user,
            remote_project_dir: target.remote_project_dir,
            package,
            baseline_tests,
            repo_lock: shared_repo_lock(&repo_key),
            active_guard: AsyncMutex::new(None),
        })
    }

    async fn run_cargo(&self, subcommand: &str) -> Result<Output> {
        let command = cargo_command(
            &self.remote_project_dir,
            subcommand,
            self.package.as_deref(),
        );
        run_command(&ssh_command_spec(&self.ssh_user, &self.ssh_host, &command))
            .await
            .map_err(ConsensusError::WorkspaceError)
    }

    pub async fn cleanup(&self) -> Result<()> {
        let output = self
            .run_workspace_command(&cleanup_command(&self.remote_project_dir))
            .await?;
        ensure_workspace_success(output)
    }

    async fn run_workspace_command(&self, command: &str) -> Result<Output> {
        run_command(&ssh_command_spec(&self.ssh_user, &self.ssh_host, command))
            .await
            .map_err(ConsensusError::WorkspaceError)
    }

    async fn acquire_repo_lock(&self) -> Result<()> {
        let guard = Arc::clone(&self.repo_lock).lock_owned().await;
        let mut active_guard = self.active_guard.lock().await;
        if active_guard.is_some() {
            return Err(ConsensusError::WorkspaceError(
                "remote evaluation already in progress".to_owned(),
            ));
        }
        *active_guard = Some(guard);
        Ok(())
    }

    async fn release_repo_lock(&self) {
        let mut active_guard = self.active_guard.lock().await;
        active_guard.take();
    }
}

#[async_trait]
impl EvaluationWorkspace for RemoteEvaluationWorkspace {
    async fn begin_evaluation(&self) -> Result<()> {
        self.acquire_repo_lock().await
    }

    async fn apply_patch(&self, patch: &str) -> Result<()> {
        let patch_file = write_patch_file(patch)?;
        let remote_patch = remote_patch_path();
        let scp = scp_command_spec(
            &self.ssh_user,
            &self.ssh_host,
            patch_file.path(),
            &remote_patch,
        );
        let output = run_command(&scp)
            .await
            .map_err(ConsensusError::PatchFailed)?;
        ensure_patch_success(output)?;
        let apply = apply_patch_command(&self.remote_project_dir, &remote_patch);
        let output = self.run_workspace_command(&apply).await?;
        ensure_patch_success(output)
    }

    async fn build(&self) -> Result<()> {
        let output = self.run_cargo("build").await?;
        if output.status.success() {
            Ok(())
        } else {
            Err(ConsensusError::BuildFailed(output_error(&output)))
        }
    }

    async fn test(&self) -> Result<TestResult> {
        let output = self.run_cargo("test").await?;
        test_result_from_output(output)
    }

    async fn check_signal(&self, _signal: &Signal) -> Result<bool> {
        Ok(false)
    }

    async fn check_regression(&self, _experiment: &Experiment) -> Result<bool> {
        let regression = regression_from_test_result(self.test().await, &self.baseline_tests)?;
        Ok(regression)
    }

    async fn reset(&self) -> Result<()> {
        self.cleanup().await
    }

    async fn finish_evaluation(&self) -> Result<()> {
        let cleanup = self.cleanup().await;
        self.release_repo_lock().await;
        cleanup
    }
}

fn shared_repo_lock(key: &str) -> RepoLock {
    let locks = REMOTE_REPO_LOCKS.get_or_init(|| Mutex::new(BTreeMap::new()));
    let mut locks = match locks.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    locks
        .entry(key.to_owned())
        .or_insert_with(|| Arc::new(AsyncMutex::new(())))
        .clone()
}

fn remote_repo_key(target: &RemoteEvalTarget) -> String {
    format!(
        "{}@{}:{}",
        target.ssh_user, target.ssh_host, target.remote_project_dir
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandSpec {
    program: &'static str,
    args: Vec<String>,
}

fn ssh_command_spec(user: &str, host: &str, command: &str) -> CommandSpec {
    CommandSpec {
        program: "ssh",
        args: vec![
            "-o".to_owned(),
            "StrictHostKeyChecking=accept-new".to_owned(),
            "-o".to_owned(),
            "BatchMode=yes".to_owned(),
            "-o".to_owned(),
            "ConnectTimeout=30".to_owned(),
            "-o".to_owned(),
            "ServerAliveInterval=15".to_owned(),
            "-o".to_owned(),
            "ServerAliveCountMax=3".to_owned(),
            format!("{user}@{host}"),
            command.to_owned(),
        ],
    }
}

fn scp_command_spec(user: &str, host: &str, local_path: &Path, remote_path: &str) -> CommandSpec {
    CommandSpec {
        program: "scp",
        args: vec![
            "-o".to_owned(),
            "StrictHostKeyChecking=accept-new".to_owned(),
            "-o".to_owned(),
            "BatchMode=yes".to_owned(),
            "-o".to_owned(),
            "ConnectTimeout=30".to_owned(),
            local_path.display().to_string(),
            format!("{user}@{host}:{remote_path}"),
        ],
    }
}

async fn run_command(spec: &CommandSpec) -> std::result::Result<Output, String> {
    let mut command = Command::new(spec.program);
    command.args(spec.args.iter().map(String::as_str));
    command.output().await.map_err(|error| error.to_string())
}

fn run_command_blocking(spec: &CommandSpec) -> std::result::Result<Output, String> {
    let mut command = StdCommand::new(spec.program);
    command.args(spec.args.iter().map(String::as_str));
    command.output().map_err(|error| error.to_string())
}

fn cleanup_remote_blocking(target: &RemoteEvalTarget) -> Result<()> {
    let command = cleanup_command(&target.remote_project_dir);
    let output = run_command_blocking(&ssh_command_spec(
        &target.ssh_user,
        &target.ssh_host,
        &command,
    ))
    .map_err(ConsensusError::WorkspaceError)?;
    ensure_workspace_success(output)
}

fn collect_baseline_tests_blocking(
    target: &RemoteEvalTarget,
    package: Option<&str>,
) -> Result<TestResult> {
    let command = cargo_command(&target.remote_project_dir, "test", package);
    let output = run_command_blocking(&ssh_command_spec(
        &target.ssh_user,
        &target.ssh_host,
        &command,
    ))
    .map_err(ConsensusError::WorkspaceError)?;
    baseline_test_result_from_output(output)
}

fn ensure_workspace_success(output: Output) -> Result<()> {
    if output.status.success() {
        Ok(())
    } else {
        Err(ConsensusError::WorkspaceError(output_error(&output)))
    }
}

fn ensure_patch_success(output: Output) -> Result<()> {
    if output.status.success() {
        Ok(())
    } else {
        Err(ConsensusError::PatchFailed(output_error(&output)))
    }
}

fn baseline_test_result_from_output(output: Output) -> Result<TestResult> {
    let combined = combine_output(&output.stdout, &output.stderr);
    let result = parse_test_result(&combined);
    if output.status.success() || result.total > 0 {
        Ok(result)
    } else {
        Err(ConsensusError::WorkspaceError(format!(
            "failed to collect baseline tests: {}",
            output_error(&output)
        )))
    }
}

fn test_result_from_output(output: Output) -> Result<TestResult> {
    let combined = combine_output(&output.stdout, &output.stderr);
    let result = parse_test_result(&combined);
    if output.status.success() {
        Ok(result)
    } else {
        Err(ConsensusError::TestFailed {
            passed: result.passed,
            failed: result.failed,
            total: result.total,
        })
    }
}

fn regression_from_test_result(
    candidate: Result<TestResult>,
    baseline: &TestResult,
) -> Result<bool> {
    match candidate {
        Ok(result) => Ok(result.passed < baseline.passed),
        Err(ConsensusError::TestFailed { passed, .. }) => Ok(passed < baseline.passed),
        Err(error) => Err(error),
    }
}

fn write_patch_file(patch: &str) -> Result<NamedTempFile> {
    let mut patch_file =
        NamedTempFile::new().map_err(|error| ConsensusError::PatchFailed(error.to_string()))?;
    patch_file
        .write_all(normalize_patch(patch).as_bytes())
        .map_err(|error| ConsensusError::PatchFailed(error.to_string()))?;
    Ok(patch_file)
}

fn normalize_patch(patch: &str) -> String {
    if patch.ends_with('\n') {
        patch.to_owned()
    } else {
        format!("{patch}\n")
    }
}

fn remote_patch_path() -> String {
    format!("/tmp/fx-remote-eval-{}.patch", Uuid::new_v4())
}

fn cargo_command(project_dir: &str, subcommand: &str, package: Option<&str>) -> String {
    let package_arg = package
        .map(|package| format!(" -p {}", shell_quote(package)))
        .unwrap_or_default();
    format!(
        "cd {} && cargo {}{}",
        shell_quote(project_dir),
        subcommand,
        package_arg
    )
}

fn apply_patch_command(project_dir: &str, remote_patch: &str) -> String {
    format!(
        "cd {} && git apply {}; rm -f {}",
        shell_quote(project_dir),
        shell_quote(remote_patch),
        shell_quote(remote_patch)
    )
}

fn cleanup_command(project_dir: &str) -> String {
    format!(
        "cd {} && git checkout -- . && git clean -fd",
        shell_quote(project_dir)
    )
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn combine_output(stdout: &[u8], stderr: &[u8]) -> String {
    let stdout = String::from_utf8_lossy(stdout);
    let stderr = String::from_utf8_lossy(stderr);
    format!("{stdout}{stderr}")
}

fn output_error(output: &Output) -> String {
    let combined = combine_output(&output.stdout, &output.stderr);
    if combined.trim().is_empty() {
        format!("command exited with status {}", output.status)
    } else {
        combined
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::ExitStatus;

    #[test]
    fn remote_eval_target_parses_user_host_and_path() {
        let target: RemoteEvalTarget = "user@example.com:/srv/fawx".parse().expect("target");

        assert_eq!(target.ssh_user, "user");
        assert_eq!(target.ssh_host, "example.com");
        assert_eq!(target.remote_project_dir, "/srv/fawx");
    }

    #[test]
    fn remote_eval_target_rejects_invalid_format() {
        let error = "not-a-target"
            .parse::<RemoteEvalTarget>()
            .expect_err("invalid target");

        assert!(error.to_string().contains("expected user@host:/path"));
    }

    #[test]
    fn ssh_command_format_builds_expected_args() {
        let spec = ssh_command_spec("user", "10.0.0.1", "cd '/srv/fawx' && cargo test");

        assert_eq!(spec.program, "ssh");
        assert_eq!(
            spec.args,
            vec![
                "-o",
                "StrictHostKeyChecking=accept-new",
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=30",
                "-o",
                "ServerAliveInterval=15",
                "-o",
                "ServerAliveCountMax=3",
                "user@10.0.0.1",
                "cd '/srv/fawx' && cargo test",
            ]
        );
    }

    #[test]
    fn patch_application_builds_scp_and_git_apply_commands() {
        let scp = scp_command_spec(
            "user",
            "10.0.0.1",
            Path::new("/tmp/local.patch"),
            "/tmp/remote.patch",
        );
        let apply = apply_patch_command("/srv/fawx", "/tmp/remote.patch");

        assert_eq!(scp.program, "scp");
        assert_eq!(
            scp.args,
            vec![
                "-o",
                "StrictHostKeyChecking=accept-new",
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=30",
                "/tmp/local.patch",
                "user@10.0.0.1:/tmp/remote.patch",
            ]
        );
        assert_eq!(
            apply,
            "cd '/srv/fawx' && git apply '/tmp/remote.patch'; rm -f '/tmp/remote.patch'"
        );
    }

    #[test]
    fn baseline_test_collection_parses_cargo_test_output_correctly() {
        let output = fake_output(
            true,
            "running 3 tests\n\ntest result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n",
            "",
        );

        let result = baseline_test_result_from_output(output).expect("baseline result");

        assert_eq!(result.passed, 3);
        assert_eq!(result.failed, 0);
        assert_eq!(result.total, 3);
    }

    #[test]
    fn cleanup_restores_remote_repo_state() {
        assert_eq!(
            cleanup_command("/srv/fawx"),
            "cd '/srv/fawx' && git checkout -- . && git clean -fd"
        );
    }

    fn fake_output(success: bool, stdout: &str, stderr: &str) -> Output {
        Output {
            status: status(success),
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    fn status(success: bool) -> ExitStatus {
        let program = if success { "true" } else { "false" };
        StdCommand::new(program).status().expect("status")
    }

    #[test]
    fn shell_quote_handles_embedded_single_quotes() {
        assert_eq!(shell_quote("it's a test"), "'it'\"'\"'s a test'");
    }

    #[test]
    fn remote_eval_target_rejects_relative_path() {
        let error = "user@host:relative/path"
            .parse::<RemoteEvalTarget>()
            .expect_err("relative path");
        assert!(error.to_string().contains("absolute path"));
    }

    #[test]
    fn remote_eval_target_accepts_tilde_path() {
        let target: RemoteEvalTarget = "user@host:~/fawx".parse().expect("tilde path");
        assert_eq!(target.remote_project_dir, "~/fawx");
    }
}
