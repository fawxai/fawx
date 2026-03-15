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
    #[allow(dead_code)]
    pub(crate) fn new(inner: Arc<dyn RestartRequestor>) -> Self {
        Self { inner }
    }

    pub fn request_restart(&self) -> Result<RestartAction, String> {
        self.inner.request_restart()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaunchAgentState {
    pub installed: bool,
    pub loaded: bool,
    pub auto_start_enabled: bool,
}

pub fn detect_launchagent_state() -> LaunchAgentState {
    let installed = launchagent_plist_path().is_file();
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
        let launchagent = detect_launchagent_state();
        if launchagent.installed && launchagent.loaded && launchagent.auto_start_enabled {
            return Self {
                signal: RestartSignal::Terminate,
                restart_via: "launchagent_keepalive",
            };
        }

        Self {
            signal: RestartSignal::Hangup,
            restart_via: "sighup_reexec",
        }
    }

    fn action(self) -> RestartAction {
        RestartAction {
            restart_via: self.restart_via,
            message: "Server restart requested.",
        }
    }
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

fn launchagent_plist_path() -> PathBuf {
    home_dir()
        .join("Library")
        .join("LaunchAgents")
        .join("ai.fawx.server.plist")
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

#[cfg(target_os = "macos")]
fn launchagent_loaded() -> bool {
    command_output_contains("launchctl", &["list"], LAUNCHAGENT_LABEL)
}

#[cfg(not(target_os = "macos"))]
fn launchagent_loaded() -> bool {
    false
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
