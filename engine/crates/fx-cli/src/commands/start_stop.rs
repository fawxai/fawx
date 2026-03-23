use crate::restart::{self, LiveRestartSystem, RestartSignal, RestartSystem};
use crate::startup;
use anyhow::{anyhow, Context};
#[cfg(test)]
use fx_api::launchagent::LaunchAgentError;
use fx_api::launchagent::LaunchAgentStatus;
use std::{
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

const STOP_TIMEOUT: Duration = Duration::from_secs(5);
const START_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(100);
/// Give the OS a moment to release redb's file lock after the daemon exits.
/// Without this small delay, a rapid stop/start can race the lock cleanup.
const POST_STOP_DELAY: Duration = Duration::from_millis(200);
const LAUNCHAGENT_START_DELAY: Duration = Duration::from_millis(500);
const DAEMON_LOG_FILE: &str = "fawx-daemon.log";

pub(crate) fn run_start() -> anyhow::Result<i32> {
    let outcome = execute_start(&LiveRestartSystem, &StartPaths::detect()?)?;
    println!("{}", outcome.message());
    Ok(0)
}

pub(crate) fn run_stop() -> anyhow::Result<i32> {
    let outcome = execute_stop(&LiveRestartSystem, &restart::pid_file_path())?;
    println!("{}", outcome.message());
    Ok(0)
}

#[derive(Debug, Clone)]
struct StartPaths {
    current_exe: PathBuf,
    pid_file: PathBuf,
    logs_dir: PathBuf,
}

impl StartPaths {
    fn detect() -> anyhow::Result<Self> {
        Ok(Self {
            current_exe: std::env::current_exe().context("failed to locate current executable")?,
            pid_file: restart::pid_file_path(),
            logs_dir: detect_logs_dir(),
        })
    }
}

fn detect_logs_dir() -> PathBuf {
    let config = startup::load_config().unwrap_or_default();
    startup::resolve_log_dir(&config.logging)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StartOutcome {
    AlreadyRunning(u32),
    AlreadyManagedByLaunchAgent,
    Started,
}

impl StartOutcome {
    fn message(&self) -> String {
        match self {
            Self::AlreadyRunning(pid) => format!("Fawx is already running (PID: {pid})."),
            Self::AlreadyManagedByLaunchAgent => {
                "Fawx is already running (managed by LaunchAgent).".to_string()
            }
            Self::Started => "Fawx started. Logs: ~/.fawx/logs/".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StopOutcome {
    NotRunning,
    StalePidRemoved,
    Stopped,
}

impl StopOutcome {
    fn message(&self) -> &'static str {
        match self {
            Self::NotRunning => "Fawx is not running.",
            Self::StalePidRemoved => "Fawx is not running (stale PID file removed).",
            Self::Stopped => "Fawx stopped.",
        }
    }
}

#[derive(Debug, Clone)]
struct SpawnRequest {
    executable: PathBuf,
    log_file: PathBuf,
}

impl SpawnRequest {
    fn new(paths: &StartPaths) -> Self {
        Self {
            executable: paths.current_exe.clone(),
            log_file: paths.logs_dir.join(DAEMON_LOG_FILE),
        }
    }
}

trait StartStopSystem: RestartSystem {
    fn spawn_background(&self, request: &SpawnRequest) -> anyhow::Result<()>;
    fn sleep(&self, duration: Duration);
}

impl StartStopSystem for LiveRestartSystem {
    fn spawn_background(&self, request: &SpawnRequest) -> anyhow::Result<()> {
        restart::spawn_serve(&request.executable, Some(&request.log_file)).map(|_| ())
    }

    fn sleep(&self, duration: Duration) {
        thread::sleep(duration);
    }
}

fn execute_start(
    system: &impl StartStopSystem,
    paths: &StartPaths,
) -> anyhow::Result<StartOutcome> {
    if let Some(pid) = live_pid_from_file(system, &paths.pid_file)? {
        return Ok(StartOutcome::AlreadyRunning(pid));
    }
    let launchagent_status = system.launchagent_status();
    if launchagent_is_already_running(&launchagent_status) {
        return Ok(StartOutcome::AlreadyManagedByLaunchAgent);
    }
    if start_via_launchagent(system, &launchagent_status, &paths.pid_file)? {
        return Ok(StartOutcome::Started);
    }
    system.spawn_background(&SpawnRequest::new(paths))?;
    wait_for_pid_file(system, &paths.pid_file)?;
    Ok(StartOutcome::Started)
}

fn execute_stop(system: &impl StartStopSystem, pid_file: &Path) -> anyhow::Result<StopOutcome> {
    let launchagent_status = system.launchagent_status();
    if stop_via_launchagent(system, &launchagent_status, pid_file)? {
        return Ok(StopOutcome::Stopped);
    }
    let Some(pid) = restart::read_pid_file(pid_file)? else {
        return Ok(StopOutcome::NotRunning);
    };
    if !system.process_exists(pid)? {
        restart::remove_pid_file(pid_file)?;
        return Ok(StopOutcome::StalePidRemoved);
    }
    terminate_process(system, pid)?;
    // Allow OS to fully release file handles before returning.
    system.sleep(POST_STOP_DELAY);
    restart::remove_pid_file(pid_file)?;
    Ok(StopOutcome::Stopped)
}

fn stop_via_launchagent(
    system: &impl RestartSystem,
    status: &LaunchAgentStatus,
    pid_file: &Path,
) -> anyhow::Result<bool> {
    if !status.installed || !status.loaded {
        return Ok(false);
    }
    match system.stop_launchagent_service() {
        Ok(()) => {
            let _ = restart::remove_pid_file(pid_file);
            Ok(true)
        }
        Err(error) => {
            tracing::warn!("LaunchAgent stop failed, falling back to signal: {error}");
            Ok(false)
        }
    }
}

fn start_via_launchagent(
    system: &impl StartStopSystem,
    status: &LaunchAgentStatus,
    pid_file: &Path,
) -> anyhow::Result<bool> {
    if !status.installed || status.loaded {
        return Ok(false);
    }
    match system.start_launchagent_service() {
        Ok(()) => {
            system.sleep(LAUNCHAGENT_START_DELAY);
            warn_if_launchagent_start_unverified(system, pid_file)?;
            Ok(true)
        }
        Err(error) => {
            tracing::warn!("LaunchAgent start failed, falling back to direct spawn: {error}");
            Ok(false)
        }
    }
}

fn launchagent_is_already_running(status: &LaunchAgentStatus) -> bool {
    status.installed && status.loaded
}

fn warn_if_launchagent_start_unverified(
    system: &impl RestartSystem,
    pid_file: &Path,
) -> anyhow::Result<()> {
    if !launchagent_start_verified(system, pid_file)? {
        tracing::warn!(
            "LaunchAgent start could not be verified yet; the service may still be starting"
        );
    }
    Ok(())
}

fn launchagent_start_verified(
    system: &impl RestartSystem,
    pid_file: &Path,
) -> anyhow::Result<bool> {
    if live_pid_from_file(system, pid_file)?.is_some() {
        return Ok(true);
    }
    Ok(system.launchagent_status().loaded)
}

fn live_pid_from_file(system: &impl RestartSystem, pid_file: &Path) -> anyhow::Result<Option<u32>> {
    let Some(pid) = restart::read_pid_file(pid_file)? else {
        return Ok(None);
    };
    if system.process_exists(pid)? {
        return Ok(Some(pid));
    }
    restart::remove_pid_file(pid_file)?;
    Ok(None)
}

fn terminate_process(system: &impl StartStopSystem, pid: u32) -> anyhow::Result<()> {
    system.send_signal(pid, RestartSignal::Terminate)?;
    if wait_for_exit(system, pid, STOP_TIMEOUT)? {
        return Ok(());
    }
    system.send_signal(pid, RestartSignal::Kill)?;
    ensure_process_stopped(system, pid)
}

fn ensure_process_stopped(system: &impl StartStopSystem, pid: u32) -> anyhow::Result<()> {
    if wait_for_exit(system, pid, STOP_TIMEOUT)? {
        return Ok(());
    }
    Err(anyhow!(
        "timed out waiting for fawx (pid {pid}) to exit after SIGKILL"
    ))
}

fn wait_for_exit(
    system: &impl StartStopSystem,
    pid: u32,
    timeout: Duration,
) -> anyhow::Result<bool> {
    for _ in 0..poll_attempts(timeout) {
        if !system.process_exists(pid)? {
            return Ok(true);
        }
        system.sleep(POLL_INTERVAL);
    }
    system.process_exists(pid).map(|running| !running)
}

fn wait_for_pid_file(system: &impl StartStopSystem, pid_file: &Path) -> anyhow::Result<()> {
    for _ in 0..poll_attempts(START_TIMEOUT) {
        if pid_from_ready_file(system, pid_file)? {
            return Ok(());
        }
        system.sleep(POLL_INTERVAL);
    }
    Err(anyhow!(
        "fawx did not create a pid file within {} seconds",
        START_TIMEOUT.as_secs()
    ))
}

fn pid_from_ready_file(system: &impl RestartSystem, pid_file: &Path) -> anyhow::Result<bool> {
    let Some(pid) = restart::read_pid_file(pid_file)? else {
        return Ok(false);
    };
    system.process_exists(pid)
}

fn poll_attempts(timeout: Duration) -> usize {
    let interval_ms = POLL_INTERVAL.as_millis();
    let attempts = timeout.as_millis().div_ceil(interval_ms);
    usize::try_from(attempts).unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, collections::VecDeque, fs};
    use tempfile::TempDir;

    struct MockStartStopSystem {
        process_exists: RefCell<VecDeque<bool>>,
        sent_signals: RefCell<Vec<(u32, RestartSignal)>>,
        slept: RefCell<Vec<Duration>>,
        spawn_requests: RefCell<Vec<SpawnRequest>>,
        pid_written_on_spawn: Option<(PathBuf, u32)>,
        pid_written_on_sleep: Option<(PathBuf, u32)>,
        launchagent_status: RefCell<LaunchAgentStatus>,
        launchagent_status_calls: RefCell<usize>,
        loaded_after_start: bool,
        start_error: Option<&'static str>,
        stop_error: Option<&'static str>,
        start_calls: RefCell<usize>,
        stop_calls: RefCell<usize>,
    }

    impl MockStartStopSystem {
        fn new(process_exists: Vec<bool>, installed: bool, loaded: bool) -> Self {
            Self {
                process_exists: RefCell::new(process_exists.into()),
                sent_signals: RefCell::new(Vec::new()),
                slept: RefCell::new(Vec::new()),
                spawn_requests: RefCell::new(Vec::new()),
                pid_written_on_spawn: None,
                pid_written_on_sleep: None,
                launchagent_status: RefCell::new(LaunchAgentStatus { installed, loaded }),
                launchagent_status_calls: RefCell::new(0),
                loaded_after_start: loaded,
                start_error: None,
                stop_error: None,
                start_calls: RefCell::new(0),
                stop_calls: RefCell::new(0),
            }
        }

        fn with_pid_written_on_spawn(mut self, pid_file: PathBuf, pid: u32) -> Self {
            self.pid_written_on_spawn = Some((pid_file, pid));
            self
        }

        fn with_pid_written_on_sleep(mut self, pid_file: PathBuf, pid: u32) -> Self {
            self.pid_written_on_sleep = Some((pid_file, pid));
            self
        }

        fn with_loaded_after_start(mut self, loaded: bool) -> Self {
            self.loaded_after_start = loaded;
            self
        }

        fn with_start_error(mut self, error: &'static str) -> Self {
            self.start_error = Some(error);
            self
        }

        fn with_stop_error(mut self, error: &'static str) -> Self {
            self.stop_error = Some(error);
            self
        }
    }

    impl RestartSystem for MockStartStopSystem {
        fn process_exists(&self, _pid: u32) -> anyhow::Result<bool> {
            Ok(self
                .process_exists
                .borrow_mut()
                .pop_front()
                .unwrap_or(false))
        }

        fn find_fawx_process(&self, _exclude_pid: u32) -> anyhow::Result<Option<u32>> {
            Ok(None)
        }

        fn send_signal(&self, pid: u32, signal: RestartSignal) -> anyhow::Result<()> {
            self.sent_signals.borrow_mut().push((pid, signal));
            Ok(())
        }

        fn build_all(
            &self,
            _repo_root: &Path,
            _skip_skills: bool,
        ) -> anyhow::Result<crate::restart::BuildOutcome> {
            unreachable!("start_stop tests should not build binaries")
        }

        fn spawn_serve(&self, _executable: &Path) -> anyhow::Result<u32> {
            unreachable!("start_stop tests use spawn_background instead")
        }

        fn launchagent_status(&self) -> LaunchAgentStatus {
            *self.launchagent_status_calls.borrow_mut() += 1;
            self.launchagent_status.borrow().clone()
        }

        fn stop_launchagent_service(&self) -> Result<(), LaunchAgentError> {
            *self.stop_calls.borrow_mut() += 1;
            let result = service_result("bootout", self.stop_error);
            if result.is_ok() {
                self.launchagent_status.borrow_mut().loaded = false;
            }
            result
        }

        fn start_launchagent_service(&self) -> Result<(), LaunchAgentError> {
            *self.start_calls.borrow_mut() += 1;
            let result = service_result("bootstrap", self.start_error);
            if result.is_ok() {
                self.launchagent_status.borrow_mut().loaded = self.loaded_after_start;
            }
            result
        }
    }

    impl StartStopSystem for MockStartStopSystem {
        fn spawn_background(&self, request: &SpawnRequest) -> anyhow::Result<()> {
            self.spawn_requests.borrow_mut().push(request.clone());
            if let Some((pid_file, pid)) = &self.pid_written_on_spawn {
                fs::write(pid_file, format!("{pid}\n")).context("write spawned pid file")?;
            }
            Ok(())
        }

        fn sleep(&self, duration: Duration) {
            self.slept.borrow_mut().push(duration);
            if let Some((pid_file, pid)) = &self.pid_written_on_sleep {
                fs::write(pid_file, format!("{pid}\n")).expect("write launchagent pid file");
            }
        }
    }

    fn service_result(command: &str, error: Option<&'static str>) -> Result<(), LaunchAgentError> {
        match error {
            Some(stderr) => Err(LaunchAgentError::LaunchctlFailed {
                command: command.to_string(),
                stderr: stderr.to_string(),
                exit_code: Some(1),
            }),
            None => Ok(()),
        }
    }

    fn write_pid_file(temp: &TempDir, pid: u32) -> PathBuf {
        let path = temp.path().join("fawx.pid");
        fs::write(&path, format!("{pid}\n")).expect("write pid file");
        path
    }

    fn start_paths(temp: &TempDir) -> StartPaths {
        StartPaths {
            current_exe: temp.path().join("fawx"),
            pid_file: temp.path().join("fawx.pid"),
            logs_dir: temp.path().join("logs"),
        }
    }

    fn sigkill_fallback_responses() -> Vec<bool> {
        let mut responses = vec![true];
        responses.extend(std::iter::repeat_n(true, poll_attempts(STOP_TIMEOUT) + 1));
        responses.push(false);
        responses
    }

    #[test]
    fn stop_with_no_pid_file_reports_not_running() {
        let temp = TempDir::new().expect("tempdir");
        let system = MockStartStopSystem::new(Vec::new(), false, false);

        let outcome = execute_stop(&system, &temp.path().join("fawx.pid")).expect("stop outcome");

        assert_eq!(outcome, StopOutcome::NotRunning);
        assert!(system.sent_signals.borrow().is_empty());
    }

    #[test]
    fn stop_with_stale_pid_file_removes_it() {
        let temp = TempDir::new().expect("tempdir");
        let pid_file = write_pid_file(&temp, 4321);
        let system = MockStartStopSystem::new(vec![false], false, false);

        let outcome = execute_stop(&system, &pid_file).expect("stop outcome");

        assert_eq!(outcome, StopOutcome::StalePidRemoved);
        assert!(!pid_file.exists());
    }

    #[test]
    fn stop_with_loaded_launchagent_unloads_service_without_signaling() {
        let temp = TempDir::new().expect("tempdir");
        let pid_file = write_pid_file(&temp, 4321);
        let system = MockStartStopSystem::new(Vec::new(), true, true);

        let outcome = execute_stop(&system, &pid_file).expect("stop outcome");

        assert_eq!(outcome, StopOutcome::Stopped);
        assert_eq!(*system.stop_calls.borrow(), 1);
        assert!(system.sent_signals.borrow().is_empty());
        assert!(!pid_file.exists());
    }

    #[test]
    fn stop_with_failed_launchagent_falls_back_to_sigterm() {
        let temp = TempDir::new().expect("tempdir");
        let pid_file = write_pid_file(&temp, 4321);
        let system =
            MockStartStopSystem::new(vec![true, false], true, true).with_stop_error("denied");

        let outcome = execute_stop(&system, &pid_file).expect("stop outcome");

        assert_eq!(outcome, StopOutcome::Stopped);
        assert_eq!(*system.stop_calls.borrow(), 1);
        assert_eq!(
            *system.sent_signals.borrow(),
            vec![(4321, RestartSignal::Terminate)]
        );
        assert!(!pid_file.exists());
    }

    #[test]
    fn stop_happy_path_sends_sigterm_and_removes_pid_file() {
        let temp = TempDir::new().expect("tempdir");
        let pid_file = write_pid_file(&temp, 4321);
        let system = MockStartStopSystem::new(vec![true, false], false, false);

        let outcome = execute_stop(&system, &pid_file).expect("stop outcome");

        assert_eq!(outcome, StopOutcome::Stopped);
        assert_eq!(
            *system.sent_signals.borrow(),
            vec![(4321, RestartSignal::Terminate)]
        );
        assert!(!pid_file.exists());
    }

    #[test]
    fn stop_sigkill_fallback_verifies_process_exit() {
        let temp = TempDir::new().expect("tempdir");
        let pid_file = write_pid_file(&temp, 4321);
        let system = MockStartStopSystem::new(sigkill_fallback_responses(), false, false);

        let outcome = execute_stop(&system, &pid_file).expect("stop outcome");

        assert_eq!(outcome, StopOutcome::Stopped);
        assert_eq!(
            *system.sent_signals.borrow(),
            vec![
                (4321, RestartSignal::Terminate),
                (4321, RestartSignal::Kill),
            ]
        );
        assert!(system.process_exists.borrow().is_empty());
        assert!(!pid_file.exists());
    }

    #[test]
    fn stop_includes_post_termination_delay() {
        let temp = TempDir::new().expect("tempdir");
        let pid_file = write_pid_file(&temp, 4321);
        let system = MockStartStopSystem::new(vec![true, false], false, false);

        let outcome = execute_stop(&system, &pid_file).expect("stop outcome");

        assert_eq!(outcome, StopOutcome::Stopped);
        assert_eq!(
            *system.sent_signals.borrow(),
            vec![(4321, RestartSignal::Terminate)]
        );
        assert_eq!(*system.slept.borrow(), vec![POST_STOP_DELAY]);
    }

    #[test]
    fn start_with_live_pid_file_reports_already_running() {
        let temp = TempDir::new().expect("tempdir");
        let paths = start_paths(&temp);
        fs::write(&paths.pid_file, "777\n").expect("write pid file");
        let system = MockStartStopSystem::new(vec![true], false, false);

        let outcome = execute_start(&system, &paths).expect("start outcome");

        assert_eq!(outcome, StartOutcome::AlreadyRunning(777));
        assert!(system.spawn_requests.borrow().is_empty());
    }

    #[test]
    fn start_with_loaded_launchagent_and_missing_pid_skips_direct_spawn() {
        let temp = TempDir::new().expect("tempdir");
        let paths = start_paths(&temp);
        let system = MockStartStopSystem::new(Vec::new(), true, true);

        let outcome = execute_start(&system, &paths).expect("start outcome");

        assert_eq!(outcome, StartOutcome::AlreadyManagedByLaunchAgent);
        assert!(system.spawn_requests.borrow().is_empty());
        assert_eq!(*system.start_calls.borrow(), 0);
    }

    #[test]
    fn start_with_installed_launchagent_bootstraps_without_direct_spawn() {
        let temp = TempDir::new().expect("tempdir");
        let paths = start_paths(&temp);
        let system =
            MockStartStopSystem::new(Vec::new(), true, false).with_loaded_after_start(true);

        let outcome = execute_start(&system, &paths).expect("start outcome");

        assert_eq!(outcome, StartOutcome::Started);
        assert_eq!(*system.start_calls.borrow(), 1);
        assert!(system.spawn_requests.borrow().is_empty());
    }

    #[test]
    fn start_with_launchagent_pid_file_created_after_bootstrap_counts_as_verified() {
        let temp = TempDir::new().expect("tempdir");
        let paths = start_paths(&temp);
        let system = MockStartStopSystem::new(vec![true], true, false)
            .with_pid_written_on_sleep(paths.pid_file.clone(), 9006);

        let outcome = execute_start(&system, &paths).expect("start outcome");

        assert_eq!(outcome, StartOutcome::Started);
        assert_eq!(
            restart::read_pid_file(&paths.pid_file).expect("read pid file"),
            Some(9006)
        );
        assert_eq!(*system.start_calls.borrow(), 1);
        assert_eq!(*system.launchagent_status_calls.borrow(), 1);
        assert!(system.spawn_requests.borrow().is_empty());
    }

    #[test]
    fn start_with_failed_launchagent_falls_back_to_direct_spawn() {
        let temp = TempDir::new().expect("tempdir");
        let paths = start_paths(&temp);
        let system = MockStartStopSystem::new(vec![true], true, false)
            .with_pid_written_on_spawn(paths.pid_file.clone(), 9005)
            .with_start_error("denied");

        let outcome = execute_start(&system, &paths).expect("start outcome");

        assert_eq!(outcome, StartOutcome::Started);
        assert_eq!(*system.start_calls.borrow(), 1);
        assert_eq!(system.spawn_requests.borrow().len(), 1);
    }

    #[test]
    fn start_happy_path_spawns_process_and_waits_for_pid_file() {
        let temp = TempDir::new().expect("tempdir");
        let paths = start_paths(&temp);
        let system = MockStartStopSystem::new(vec![true], false, false)
            .with_pid_written_on_spawn(paths.pid_file.clone(), 9001);

        let outcome = execute_start(&system, &paths).expect("start outcome");

        assert_eq!(outcome, StartOutcome::Started);
        assert_eq!(
            restart::read_pid_file(&paths.pid_file).expect("read pid file"),
            Some(9001)
        );
        assert_eq!(system.spawn_requests.borrow().len(), 1);
        assert_eq!(
            system.spawn_requests.borrow()[0].log_file,
            paths.logs_dir.join(DAEMON_LOG_FILE)
        );
    }

    #[test]
    fn start_with_stale_pid_file_removes_it_before_spawning() {
        let temp = TempDir::new().expect("tempdir");
        let paths = start_paths(&temp);
        fs::write(&paths.pid_file, "777\n").expect("write stale pid file");
        let system = MockStartStopSystem::new(vec![false, true], false, false)
            .with_pid_written_on_spawn(paths.pid_file.clone(), 9002);

        let outcome = execute_start(&system, &paths).expect("start outcome");

        assert_eq!(outcome, StartOutcome::Started);
        assert_eq!(
            restart::read_pid_file(&paths.pid_file).expect("read pid file"),
            Some(9002)
        );
        assert_eq!(system.spawn_requests.borrow().len(), 1);
    }

    #[test]
    fn start_timeout_errors_when_pid_file_never_appears() {
        let temp = TempDir::new().expect("tempdir");
        let paths = start_paths(&temp);
        let system = MockStartStopSystem::new(Vec::new(), false, false);

        let error = execute_start(&system, &paths).expect_err("start should time out");

        assert!(error.to_string().contains("within 5 seconds"));
        assert!(!paths.pid_file.exists());
    }
}
