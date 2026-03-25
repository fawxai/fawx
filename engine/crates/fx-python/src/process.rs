use fx_kernel::cancellation::CancellationToken;
use std::process::Output;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;

pub(crate) const MAX_TIMEOUT_SECONDS: u64 = 3_600;
const MAX_CAPTURE_BYTES: u64 = 512 * 1024;

pub(crate) struct CapturedProcess {
    pub stdout: String,
    pub stderr: String,
    pub status: ProcessStatus,
}

pub(crate) enum ProcessStatus {
    Exited(i32),
    TimedOut,
    Cancelled,
}

struct CaptureTasks {
    stdout: JoinHandle<Result<String, String>>,
    stderr: JoinHandle<Result<String, String>>,
}

enum StopReason {
    Timeout,
    Cancellation,
}

impl StopReason {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::Cancellation => "cancellation",
        }
    }

    fn status(&self) -> ProcessStatus {
        match self {
            Self::Timeout => ProcessStatus::TimedOut,
            Self::Cancellation => ProcessStatus::Cancelled,
        }
    }
}

pub(crate) async fn run_command(
    mut command: Command,
    action: &str,
    timeout: Duration,
    cancel: Option<&CancellationToken>,
) -> Result<CapturedProcess, String> {
    configure_process_group(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| format!("failed to start {action}: {error}"))?;
    let captures = spawn_capture_tasks(&mut child)?;
    let status = wait_for_child(&mut child, action, timeout, cancel).await?;
    let stdout = join_capture(captures.stdout, action, "stdout").await?;
    let stderr = join_capture(captures.stderr, action, "stderr").await?;

    Ok(CapturedProcess {
        stdout,
        stderr,
        status,
    })
}

pub(crate) fn elapsed_millis(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

pub(crate) fn format_process_output(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    format_process_detail(output.status.code(), &stdout, &stderr)
}

pub(crate) fn format_process_detail(
    status_code: Option<i32>,
    stdout: &str,
    stderr: &str,
) -> String {
    let detail = stderr.trim();
    let detail = if detail.is_empty() {
        stdout.trim()
    } else {
        detail
    };
    format!("status {status_code:?}; {detail}")
}

async fn wait_for_child(
    child: &mut Child,
    action: &str,
    timeout: Duration,
    cancel: Option<&CancellationToken>,
) -> Result<ProcessStatus, String> {
    if let Some(token) = cancel {
        tokio::select! {
            result = tokio::time::timeout(timeout, child.wait()) => {
                handle_wait_result(child, action, result).await
            }
            _ = token.cancelled() => stop_child(child, action, StopReason::Cancellation).await,
        }
    } else {
        let result = tokio::time::timeout(timeout, child.wait()).await;
        handle_wait_result(child, action, result).await
    }
}

async fn handle_wait_result(
    child: &mut Child,
    action: &str,
    result: Result<Result<std::process::ExitStatus, std::io::Error>, tokio::time::error::Elapsed>,
) -> Result<ProcessStatus, String> {
    match result {
        Ok(wait) => {
            let status = wait.map_err(|error| format!("failed to wait for {action}: {error}"))?;
            Ok(ProcessStatus::Exited(status.code().unwrap_or(-1)))
        }
        Err(_) => stop_child(child, action, StopReason::Timeout).await,
    }
}

async fn stop_child(
    child: &mut Child,
    action: &str,
    reason: StopReason,
) -> Result<ProcessStatus, String> {
    signal_child(child, action, &reason).await?;
    child
        .wait()
        .await
        .map_err(|error| format!("failed to wait after stopping {action}: {error}"))?;
    Ok(reason.status())
}

#[cfg(unix)]
async fn signal_child(child: &mut Child, action: &str, reason: &StopReason) -> Result<(), String> {
    if let Some(pid) = child.id() {
        // SAFETY: killpg targets the child process group created in configure_process_group.
        let result = unsafe { libc::killpg(pid as i32, libc::SIGKILL) };
        if result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        return Err(format!(
            "failed to kill {action} process group after {}: {error}",
            reason.as_str()
        ));
    }

    child
        .kill()
        .await
        .map_err(|error| format!("failed to kill {action} after {}: {error}", reason.as_str()))
}

#[cfg(not(unix))]
async fn signal_child(child: &mut Child, action: &str, reason: &StopReason) -> Result<(), String> {
    child
        .kill()
        .await
        .map_err(|error| format!("failed to kill {action} after {}: {error}", reason.as_str()))
}

#[cfg(unix)]
fn configure_process_group(command: &mut Command) {
    // SAFETY: pre_exec runs in the child just before exec; the closure only
    // calls async-signal-safe libc::setpgid to move that child into its own
    // process group so timeout/cancel can terminate the full tree.
    unsafe {
        command.pre_exec(|| {
            // SAFETY: setpgid(0, 0) only affects the current child process.
            if libc::setpgid(0, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(not(unix))]
fn configure_process_group(_command: &mut Command) {}

fn spawn_capture_tasks(child: &mut Child) -> Result<CaptureTasks, String> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "stdout pipe unavailable".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "stderr pipe unavailable".to_string())?;

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

async fn join_capture(
    handle: JoinHandle<Result<String, String>>,
    action: &str,
    stream_name: &str,
) -> Result<String, String> {
    handle
        .await
        .map_err(|error| format!("failed to join {action} {stream_name} capture: {error}"))?
}
