use crate::process::{
    elapsed_millis, run_command, CapturedProcess, ProcessStatus, MAX_TIMEOUT_SECONDS,
};
use crate::venv::VenvManager;
use fx_kernel::cancellation::CancellationToken;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::fs;
use tokio::process::Command;

const TIMEOUT_EXIT_CODE: i32 = -1;
const DEFAULT_TIMEOUT_SECONDS: u64 = 300;
static SCRIPT_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

type FileSnapshot = BTreeMap<PathBuf, FileState>;

#[derive(Debug, Clone)]
pub struct PythonRunner {
    venv_manager: VenvManager,
    experiments_root: PathBuf,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PythonRunArgs {
    pub code: String,
    pub venv: String,
    #[serde(default = "default_timeout_seconds")]
    pub timeout_seconds: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct RunResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub artifacts: Vec<String>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileState {
    modified: SystemTime,
    size: u64,
}

struct ExecutionRequest {
    python_path: PathBuf,
    script_path: PathBuf,
    work_dir: PathBuf,
    timeout_seconds: u64,
}

struct CommandOutput {
    stdout: String,
    stderr: String,
    exit_code: i32,
    timed_out: bool,
}

impl PythonRunner {
    #[must_use]
    pub fn new(venv_manager: VenvManager, experiments_root: PathBuf) -> Self {
        Self {
            venv_manager,
            experiments_root,
        }
    }

    pub async fn run(
        &self,
        args: PythonRunArgs,
        cancel: Option<&CancellationToken>,
    ) -> Result<RunResult, String> {
        self.venv_manager.ensure_venv(&args.venv).await?;
        let work_dir = self.experiments_root.join(&args.venv);
        fs::create_dir_all(&work_dir).await.map_err(|error| {
            format!(
                "failed to create experiment dir '{}': {error}",
                work_dir.display()
            )
        })?;

        let timeout_seconds = clamp_timeout_seconds(args.timeout_seconds);
        let script_path = work_dir.join(script_file_name());
        fs::write(&script_path, args.code)
            .await
            .map_err(|error| format!("failed to write '{}': {error}", script_path.display()))?;

        let before = snapshot_files(&work_dir).await?;
        let started = Instant::now();
        let output = execute_script(
            ExecutionRequest {
                python_path: self.venv_manager.python_path(&args.venv),
                script_path: script_path.clone(),
                work_dir: work_dir.clone(),
                timeout_seconds,
            },
            cancel,
        )
        .await?;
        cleanup_script_after_success(&script_path, output.exit_code).await?;
        let after = snapshot_files(&work_dir).await?;

        Ok(RunResult {
            stdout: output.stdout,
            stderr: finalize_stderr(output.stderr, timeout_seconds, output.timed_out),
            exit_code: output.exit_code,
            artifacts: detect_artifacts(&before, &after),
            duration_ms: elapsed_millis(started.elapsed()),
        })
    }
}

async fn execute_script(
    request: ExecutionRequest,
    cancel: Option<&CancellationToken>,
) -> Result<CommandOutput, String> {
    let mut command = Command::new(&request.python_path);
    command
        .arg(&request.script_path)
        .current_dir(&request.work_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = run_command(
        command,
        "python",
        Duration::from_secs(request.timeout_seconds),
        cancel,
    )
    .await?;
    to_command_output(output)
}

fn to_command_output(output: CapturedProcess) -> Result<CommandOutput, String> {
    match output.status {
        ProcessStatus::Exited(exit_code) => Ok(CommandOutput {
            stdout: output.stdout,
            stderr: output.stderr,
            exit_code,
            timed_out: false,
        }),
        ProcessStatus::TimedOut => Ok(CommandOutput {
            stdout: output.stdout,
            stderr: output.stderr,
            exit_code: TIMEOUT_EXIT_CODE,
            timed_out: true,
        }),
        ProcessStatus::Cancelled => Err("python execution cancelled".to_string()),
    }
}

fn finalize_stderr(stderr: String, timeout_seconds: u64, timed_out: bool) -> String {
    if !timed_out {
        return stderr;
    }

    let suffix = format!("Execution timed out after {timeout_seconds} seconds");
    if stderr.trim().is_empty() {
        return suffix;
    }

    format!("{stderr}\n{suffix}")
}

async fn snapshot_files(dir: &Path) -> Result<FileSnapshot, String> {
    let exists = fs::try_exists(dir)
        .await
        .map_err(|error| format!("failed to inspect '{}': {error}", dir.display()))?;
    if !exists {
        return Ok(BTreeMap::new());
    }

    let mut snapshot = BTreeMap::new();
    collect_snapshot(dir, &mut snapshot).await?;
    Ok(snapshot)
}

async fn collect_snapshot(base: &Path, snapshot: &mut FileSnapshot) -> Result<(), String> {
    let mut pending = vec![base.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let mut entries = fs::read_dir(&dir)
            .await
            .map_err(|error| format!("failed to read '{}': {error}", dir.display()))?;
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|error| format!("failed to inspect directory entry: {error}"))?
        {
            let path = entry.path();
            let metadata = entry.metadata().await.map_err(|error| {
                format!("failed to read metadata for '{}': {error}", path.display())
            })?;
            if metadata.is_dir() {
                pending.push(path);
                continue;
            }
            if metadata.is_file() {
                insert_snapshot_entry(base, &path, metadata, snapshot)?;
            }
        }
    }
    Ok(())
}

fn insert_snapshot_entry(
    base: &Path,
    path: &Path,
    metadata: std::fs::Metadata,
    snapshot: &mut FileSnapshot,
) -> Result<(), String> {
    let relative = path
        .strip_prefix(base)
        .map_err(|error| format!("failed to strip base path: {error}"))?;
    snapshot.insert(
        relative.to_path_buf(),
        FileState {
            modified: metadata.modified().map_err(|error| {
                format!("failed to read mtime for '{}': {error}", path.display())
            })?,
            size: metadata.len(),
        },
    );
    Ok(())
}

fn detect_artifacts(before: &FileSnapshot, after: &FileSnapshot) -> Vec<String> {
    after
        .iter()
        .filter_map(|(path, state)| match before.get(path) {
            Some(previous) if previous == state => None,
            _ => Some(path.to_string_lossy().into_owned()),
        })
        .collect()
}

async fn cleanup_script_after_success(script_path: &Path, exit_code: i32) -> Result<(), String> {
    if exit_code != 0 {
        return Ok(());
    }

    match fs::remove_file(script_path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "failed to clean up script '{}': {error}",
            script_path.display()
        )),
    }
}

fn script_file_name() -> String {
    let timestamp = unix_timestamp_millis();
    let counter = SCRIPT_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("run_{timestamp}_{counter}.py")
}

fn unix_timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn clamp_timeout_seconds(timeout_seconds: u64) -> u64 {
    timeout_seconds.min(MAX_TIMEOUT_SECONDS)
}

fn default_timeout_seconds() -> u64 {
    DEFAULT_TIMEOUT_SECONDS
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use tempfile::TempDir;

    fn runner_in_tempdir(temp_dir: &TempDir) -> PythonRunner {
        let venv_root = temp_dir.path().join("venvs");
        let experiments_root = temp_dir.path().join("experiments");
        let manager = VenvManager::new(&venv_root);
        PythonRunner::new(manager, experiments_root)
    }

    fn run_args(code: &str, timeout_seconds: u64) -> PythonRunArgs {
        PythonRunArgs {
            code: code.to_string(),
            venv: "test".to_string(),
            timeout_seconds,
        }
    }

    #[tokio::test]
    async fn run_simple_code() {
        let temp_dir = TempDir::new().expect("tempdir");
        let runner = runner_in_tempdir(&temp_dir);

        let result = runner
            .run(run_args("print(1 + 1)\n", 300), None)
            .await
            .expect("python ran");

        assert_eq!(result.stdout.trim(), "2");
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn timeout_kills() {
        let temp_dir = TempDir::new().expect("tempdir");
        let runner = runner_in_tempdir(&temp_dir);
        let code = "import time\ntime.sleep(2)\n";

        let result = runner
            .run(run_args(code, 1), None)
            .await
            .expect("python ran");

        assert_eq!(result.exit_code, TIMEOUT_EXIT_CODE);
        assert!(result.stderr.contains("timed out"));
    }

    #[tokio::test]
    async fn artifact_detection() {
        let temp_dir = TempDir::new().expect("tempdir");
        let runner = runner_in_tempdir(&temp_dir);
        let code = "from pathlib import Path\nPath('artifact.txt').write_text('hello')\n";

        let result = runner
            .run(run_args(code, 300), None)
            .await
            .expect("python ran");

        assert!(result.artifacts.contains(&"artifact.txt".to_string()));
    }

    #[tokio::test]
    async fn cancellation_stops_python_run() {
        let temp_dir = TempDir::new().expect("tempdir");
        let runner = runner_in_tempdir(&temp_dir);
        let token = CancellationToken::new();
        let cancel = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            cancel.cancel();
        });

        let error = runner
            .run(run_args("import time\ntime.sleep(5)\n", 300), Some(&token))
            .await
            .expect_err("run should be cancelled");

        assert!(error.contains("cancelled"));
    }

    #[tokio::test]
    async fn successful_run_cleans_up_script_file() {
        let temp_dir = TempDir::new().expect("tempdir");
        let runner = runner_in_tempdir(&temp_dir);

        runner
            .run(run_args("print('ok')\n", 300), None)
            .await
            .expect("python ran");

        let experiment_dir = temp_dir.path().join("experiments/test");
        let mut entries = fs::read_dir(&experiment_dir)
            .await
            .expect("read experiment dir");
        while let Some(entry) = entries.next_entry().await.expect("read entry") {
            let file_name = entry.file_name().to_string_lossy().into_owned();
            assert!(
                !file_name.starts_with("run_"),
                "unexpected script file: {file_name}"
            );
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn timeout_kills_process_group_children() {
        let temp_dir = TempDir::new().expect("tempdir");
        let runner = runner_in_tempdir(&temp_dir);
        let code = concat!(
            "import subprocess, sys, time\n",
            "subprocess.Popen([\n",
            "    sys.executable,\n",
            "    '-c',\n",
            "    \"import time; from pathlib import Path; time.sleep(2); Path('orphan.txt').write_text('still-running')\",\n",
            "])\n",
            "time.sleep(10)\n"
        );

        let result = runner
            .run(run_args(code, 1), None)
            .await
            .expect("python ran");

        assert_eq!(result.exit_code, TIMEOUT_EXIT_CODE);
        tokio::time::sleep(Duration::from_secs(3)).await;
        assert!(!temp_dir.path().join("experiments/test/orphan.txt").exists());
    }

    #[test]
    fn timeout_seconds_are_clamped() {
        assert_eq!(
            clamp_timeout_seconds(MAX_TIMEOUT_SECONDS + 99),
            MAX_TIMEOUT_SECONDS
        );
    }

    #[test]
    fn script_file_names_are_unique() {
        let names: BTreeSet<_> = (0..64).map(|_| script_file_name()).collect();
        assert_eq!(names.len(), 64);
    }
}
