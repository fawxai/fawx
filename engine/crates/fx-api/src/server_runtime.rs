#[cfg(target_os = "macos")]
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[cfg(target_os = "macos")]
const LAUNCHAGENT_LABEL: &str = "ai.fawx.server";
const RESTART_DELAY: Duration = Duration::from_millis(100);

#[derive(Clone)]
pub struct ServerRuntime {
    pub host: String,
    pub port: u16,
    pub https_enabled: bool,
    restart: RestartController,
}

impl ServerRuntime {
    pub fn local(port: u16) -> Self {
        Self::new("127.0.0.1", port, false, RestartController::live())
    }

    pub fn new(
        host: impl Into<String>,
        port: u16,
        https_enabled: bool,
        restart: RestartController,
    ) -> Self {
        Self {
            host: host.into(),
            port,
            https_enabled,
            restart,
        }
    }

    pub fn request_restart(&self) -> Result<RestartAction, String> {
        self.restart.request_restart()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartAction {
    pub restart_via: &'static str,
    pub message: &'static str,
}

pub trait RestartRequestor: Send + Sync {
    fn request_restart(&self) -> Result<RestartAction, String>;
}

#[derive(Clone)]
pub struct RestartController {
    inner: Arc<dyn RestartRequestor>,
}

impl RestartController {
    pub fn live() -> Self {
        Self {
            inner: Arc::new(LiveRestartRequestor),
        }
    }

    #[cfg(test)]
    pub(crate) fn from_requestor(inner: Arc<dyn RestartRequestor>) -> Self {
        Self { inner }
    }

    pub fn request_restart(&self) -> Result<RestartAction, String> {
        self.inner.request_restart()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LaunchAgentState {
    pub installed: bool,
    pub loaded: bool,
    pub auto_start_enabled: bool,
}

#[cfg(target_os = "macos")]
pub fn detect_launchagent_state() -> LaunchAgentState {
    let Some(plist_path) = launchagent_plist_path() else {
        tracing::warn!("HOME unset; skipping LaunchAgent detection");
        return LaunchAgentState::default();
    };
    build_launchagent_state(plist_path)
}

#[cfg(not(target_os = "macos"))]
pub fn detect_launchagent_state() -> LaunchAgentState {
    LaunchAgentState::default()
}

#[cfg(target_os = "macos")]
fn build_launchagent_state(plist_path: PathBuf) -> LaunchAgentState {
    let installed = plist_path.is_file();
    let loaded = installed && launchagent_loaded();
    LaunchAgentState {
        installed,
        loaded,
        auto_start_enabled: installed,
    }
}

struct LiveRestartRequestor;

impl RestartRequestor for LiveRestartRequestor {
    fn request_restart(&self) -> Result<RestartAction, String> {
        let plan = RestartPlan::detect();
        schedule_restart_signal(plan.signal)?;
        Ok(plan.action())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RestartSignal {
    Hangup,
    Terminate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RestartPlan {
    signal: RestartSignal,
    restart_via: &'static str,
}

impl RestartPlan {
    fn detect() -> Self {
        restart_plan_for_state(
            detect_launchagent_state(),
            current_process_managed_by_launchagent(),
        )
    }

    fn action(self) -> RestartAction {
        RestartAction {
            restart_via: self.restart_via,
            message: "Server restart requested.",
        }
    }
}

fn restart_plan_for_state(
    launchagent: LaunchAgentState,
    managed_by_launchagent: bool,
) -> RestartPlan {
    if launchagent.installed
        && launchagent.loaded
        && launchagent.auto_start_enabled
        && managed_by_launchagent
    {
        return RestartPlan {
            signal: RestartSignal::Terminate,
            restart_via: "launchagent_keepalive",
        };
    }

    RestartPlan {
        signal: RestartSignal::Hangup,
        restart_via: "sighup_reexec",
    }
}

#[cfg(target_os = "macos")]
fn current_process_managed_by_launchagent() -> bool {
    if std::env::var("LAUNCH_JOB_LABEL")
        .ok()
        .as_deref()
        .is_some_and(|label| label == LAUNCHAGENT_LABEL)
    {
        return true;
    }

    current_parent_pid().is_some_and(|ppid| ppid == 1)
}

#[cfg(not(target_os = "macos"))]
fn current_process_managed_by_launchagent() -> bool {
    false
}

#[cfg(target_os = "macos")]
fn current_parent_pid() -> Option<u32> {
    let pid = std::process::id().to_string();
    let output = Command::new("ps")
        .args(["-o", "ppid=", "-p", &pid])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

fn schedule_restart_signal(signal: RestartSignal) -> Result<(), String> {
    #[cfg(not(unix))]
    {
        let _ = signal;
        Err("server restart is only supported on Unix hosts".to_string())
    }

    #[cfg(unix)]
    {
        let pid = std::process::id();
        thread::spawn(move || {
            thread::sleep(RESTART_DELAY);
            if let Err(error) = send_signal(pid, signal) {
                tracing::error!(error = %error, pid, "failed to send restart signal");
            }
        });
        Ok(())
    }
}

#[cfg(unix)]
fn send_signal(pid: u32, signal: RestartSignal) -> Result<(), String> {
    let status = Command::new("kill")
        .args([signal_flag(signal), &pid.to_string()])
        .status()
        .map_err(|error| format!("failed to invoke kill: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("kill exited with status {status}"))
    }
}

#[cfg(unix)]
fn signal_flag(signal: RestartSignal) -> &'static str {
    match signal {
        RestartSignal::Hangup => "-HUP",
        RestartSignal::Terminate => "-TERM",
    }
}

#[cfg(target_os = "macos")]
fn launchagent_plist_path() -> Option<PathBuf> {
    Some(home_dir()?.join("Library/LaunchAgents/ai.fawx.server.plist"))
}

#[cfg(target_os = "macos")]
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

#[cfg(target_os = "macos")]
fn launchagent_loaded() -> bool {
    command_output_contains("launchctl", &["list"], LAUNCHAGENT_LABEL)
}

#[cfg(target_os = "macos")]
fn command_output_contains(command: &str, args: &[&str], needle: &str) -> bool {
    Command::new(command)
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).contains(needle))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_server_restart_uses_sighup_even_if_launchagent_is_installed() {
        let plan = restart_plan_for_state(
            LaunchAgentState {
                installed: true,
                loaded: true,
                auto_start_enabled: true,
            },
            false,
        );

        assert_eq!(plan.signal, RestartSignal::Hangup);
        assert_eq!(plan.restart_via, "sighup_reexec");
    }

    #[test]
    fn launchagent_managed_server_uses_keepalive_restart() {
        let plan = restart_plan_for_state(
            LaunchAgentState {
                installed: true,
                loaded: true,
                auto_start_enabled: true,
            },
            true,
        );

        assert_eq!(plan.signal, RestartSignal::Terminate);
        assert_eq!(plan.restart_via, "launchagent_keepalive");
    }
}
