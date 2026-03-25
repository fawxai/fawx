use crate::venv::VenvManager;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::fs;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;

const MAX_CAPTURE_BYTES: u64 = 512 * 1024;
const TIMEOUT_EXIT_CODE: i32 = -1;

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

struct CaptureTasks {
    stdout: JoinHandle<Result<String, String>>,
    stderr: JoinHandle<Result<String, String>>,
}

struct WaitOutcome {
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

    pub async fn run(&self, args: PythonRunArgs) -> Result<RunResult, String> {
        self.venv_manager.ensure_venv(&args.venv).await?;
        let work_dir = self.experiments_root.join(&args.venv);
        fs::create_dir_all(&work_dir).await.map_err(|error| {
            format!(
                "failed to create experiment dir '{}': {error}",
                work_dir.display()
            )
        })?;

        let script_path = work_dir.join(script_file_name());
        fs::write(&script_path, args.code)
            .await
            .map_err(|error| format!("failed to write '{}': {error}", script_path.display()))?;

        let before = snapshot_files(&work_dir)?;
        let started = Instant::now();
        let output = execute_script(ExecutionRequest {
            python_path: self.venv_manager.python_path(&args.venv),
            script_path,
            work_dir: work_dir.clone(),
            timeout_seconds: args.timeout_seconds,
        })
        .await?;
        let after = snapshot_files(&work_dir)?;

        Ok(RunResult {
            stdout: output.stdout,
            stderr: finalize_stderr(output.stderr, args.timeout_seconds, output.timed_out),
            exit_code: output.exit_code,
            artifacts: detect_artifacts(&before, &after),
            duration_ms: elapsed_millis(started.elapsed()),
        })
    }
}

async fn execute_script(request: ExecutionRequest) -> Result<CommandOutput, String> {
    let mut child = spawn_python(&request)?;
    let captures = spawn_capture_tasks(&mut child)?;
    let wait = wait_for_child(&mut child, request.timeout_seconds).await?;
    let stdout = join_capture(captures.stdout, "stdout").await?;
    let stderr = join_capture(captures.stderr, "stderr").await?;

    Ok(CommandOutput {
        stdout,
        stderr,
        exit_code: wait.exit_code,
        timed_out: wait.timed_out,
    })
}

fn spawn_python(request: &ExecutionRequest) -> Result<Child, String> {
    let mut command = Command::new(&request.python_path);
    command
        .arg(&request.script_path)
        .current_dir(&request.work_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
        .spawn()
        .map_err(|error| format!("failed to start python: {error}"))
}

fn spawn_capture_tasks(child: &mut Child) -> Result<CaptureTasks, String> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "python stdout pipe unavailable".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "python stderr pipe unavailable".to_string())?;

    Ok(CaptureTasks {
        stdout: spawn_capture_task(stdout),
        stderr: spawn_capture_task(stderr),
    })
}

fn spawn_capture_task<R>(stream: R) -> JoinHandle<Result<String, String>>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move { capture_stream(stream).await })
}

async fn capture_stream<R>(stream: R) -> Result<String, String>
where
    R: AsyncRead + Unpin,
{
    let mut reader = stream.take(MAX_CAPTURE_BYTES);
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .await
        .map_err(|error| format!("failed to read process output: {error}"))?;
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

async fn wait_for_child(child: &mut Child, timeout_seconds: u64) -> Result<WaitOutcome, String> {
    let timeout = Duration::from_secs(timeout_seconds);
    match tokio::time::timeout(timeout, child.wait()).await {
        Ok(result) => {
            let status = result.map_err(|error| format!("failed to wait for python: {error}"))?;
            Ok(WaitOutcome {
                exit_code: status.code().unwrap_or_default(),
                timed_out: false,
            })
        }
        Err(_) => kill_timed_out_child(child).await,
    }
}

async fn kill_timed_out_child(child: &mut Child) -> Result<WaitOutcome, String> {
    child
        .kill()
        .await
        .map_err(|error| format!("failed to kill timed out python process: {error}"))?;
    let _ = child
        .wait()
        .await
        .map_err(|error| format!("failed to wait after killing python: {error}"))?;

    Ok(WaitOutcome {
        exit_code: TIMEOUT_EXIT_CODE,
        timed_out: true,
    })
}

async fn join_capture(
    handle: JoinHandle<Result<String, String>>,
    stream_name: &str,
) -> Result<String, String> {
    handle
        .await
        .map_err(|error| format!("failed to join {stream_name} capture: {error}"))?
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

fn snapshot_files(dir: &Path) -> Result<FileSnapshot, String> {
    if !dir.exists() {
        return Ok(BTreeMap::new());
    }

    let mut snapshot = BTreeMap::new();
    collect_snapshot(dir, dir, &mut snapshot)?;
    Ok(snapshot)
}

fn collect_snapshot(base: &Path, dir: &Path, snapshot: &mut FileSnapshot) -> Result<(), String> {
    let entries = std::fs::read_dir(dir)
        .map_err(|error| format!("failed to read '{}': {error}", dir.display()))?;

    for entry in entries {
        let entry = entry.map_err(|error| format!("failed to inspect directory entry: {error}"))?;
        let path = entry.path();
        let metadata = entry.metadata().map_err(|error| {
            format!("failed to read metadata for '{}': {error}", path.display())
        })?;
        if metadata.is_dir() {
            collect_snapshot(base, &path, snapshot)?;
            continue;
        }
        if metadata.is_file() {
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
        }
    }

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

fn script_file_name() -> String {
    format!("run_{}.py", unix_timestamp_millis())
}

fn unix_timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn elapsed_millis(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

fn default_timeout_seconds() -> u64 {
    300
}

#[cfg(test)]
mod tests {
    use super::*;
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
            .run(run_args("print(1 + 1)\n", 300))
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

        let result = runner.run(run_args(code, 1)).await.expect("python ran");

        assert_eq!(result.exit_code, TIMEOUT_EXIT_CODE);
        assert!(result.stderr.contains("timed out"));
    }

    #[tokio::test]
    async fn artifact_detection() {
        let temp_dir = TempDir::new().expect("tempdir");
        let runner = runner_in_tempdir(&temp_dir);
        let code = "from pathlib import Path\nPath('artifact.txt').write_text('hello')\n";

        let result = runner.run(run_args(code, 300)).await.expect("python ran");

        assert!(result.artifacts.contains(&"artifact.txt".to_string()));
    }
}
