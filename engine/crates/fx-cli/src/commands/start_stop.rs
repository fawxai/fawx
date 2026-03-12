use crate::restart::{self, LiveRestartSystem, RestartSignal, RestartSystem};
use crate::startup;
use anyhow::{anyhow, Context};
use std::{
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

const STOP_TIMEOUT: Duration = Duration::from_secs(5);
const START_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(100);
const DAEMON_LOG_FILE: &str = "fawx-daemon.log";

pub(crate) fn run_start() -> anyhow::Result<i32> {
    let outcome = execute_start(&LiveProcessControl, &StartPaths::detect()?)?;
    println!("{}", outcome.message());
    Ok(0)
}

pub(crate) fn run_stop() -> anyhow::Result<i32> {
    let outcome = execute_stop(&LiveProcessControl, &restart::pid_file_path())?;
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
    Started,
}

impl StartOutcome {
    fn message(&self) -> String {
        match self {
            Self::AlreadyRunning(pid) => format!("Fawx is already running (PID: {pid})."),
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

trait ProcessControl {
    fn process_exists(&self, pid: u32) -> anyhow::Result<bool>;
    fn send_signal(&self, pid: u32, signal: RestartSignal) -> anyhow::Result<()>;
    fn spawn_background(&self, request: &SpawnRequest) -> anyhow::Result<()>;
    fn sleep(&self, duration: Duration);
}

struct LiveProcessControl;

impl ProcessControl for LiveProcessControl {
    fn process_exists(&self, pid: u32) -> anyhow::Result<bool> {
        LiveRestartSystem.process_exists(pid)
    }

    fn send_signal(&self, pid: u32, signal: RestartSignal) -> anyhow::Result<()> {
        LiveRestartSystem.send_signal(pid, signal)
    }

    fn spawn_background(&self, request: &SpawnRequest) -> anyhow::Result<()> {
        restart::spawn_serve(&request.executable, Some(&request.log_file)).map(|_| ())
    }

    fn sleep(&self, duration: Duration) {
        thread::sleep(duration);
    }
}

fn execute_start(system: &impl ProcessControl, paths: &StartPaths) -> anyhow::Result<StartOutcome> {
    if let Some(pid) = live_pid_from_file(system, &paths.pid_file)? {
        return Ok(StartOutcome::AlreadyRunning(pid));
    }
    system.spawn_background(&SpawnRequest::new(paths))?;
    wait_for_pid_file(system, &paths.pid_file)?;
    Ok(StartOutcome::Started)
}

fn execute_stop(system: &impl ProcessControl, pid_file: &Path) -> anyhow::Result<StopOutcome> {
    let Some(pid) = restart::read_pid_file(pid_file)? else {
        return Ok(StopOutcome::NotRunning);
    };
    if !system.process_exists(pid)? {
        restart::remove_pid_file(pid_file)?;
        return Ok(StopOutcome::StalePidRemoved);
    }
    terminate_process(system, pid)?;
    restart::remove_pid_file(pid_file)?;
    Ok(StopOutcome::Stopped)
}

fn live_pid_from_file(
    system: &impl ProcessControl,
    pid_file: &Path,
) -> anyhow::Result<Option<u32>> {
    let Some(pid) = restart::read_pid_file(pid_file)? else {
        return Ok(None);
    };
    if system.process_exists(pid)? {
        return Ok(Some(pid));
    }
    restart::remove_pid_file(pid_file)?;
    Ok(None)
}

fn terminate_process(system: &impl ProcessControl, pid: u32) -> anyhow::Result<()> {
    system.send_signal(pid, RestartSignal::Terminate)?;
    if wait_for_exit(system, pid, STOP_TIMEOUT)? {
        return Ok(());
    }
    system.send_signal(pid, RestartSignal::Kill)?;
    ensure_process_stopped(system, pid)
}

fn ensure_process_stopped(system: &impl ProcessControl, pid: u32) -> anyhow::Result<()> {
    if wait_for_exit(system, pid, STOP_TIMEOUT)? {
        return Ok(());
    }
    Err(anyhow!(
        "timed out waiting for fawx (pid {pid}) to exit after SIGKILL"
    ))
}

fn wait_for_exit(
    system: &impl ProcessControl,
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

fn wait_for_pid_file(system: &impl ProcessControl, pid_file: &Path) -> anyhow::Result<()> {
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

fn pid_from_ready_file(system: &impl ProcessControl, pid_file: &Path) -> anyhow::Result<bool> {
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

    struct MockProcessControl {
        process_exists: RefCell<VecDeque<bool>>,
        sent_signals: RefCell<Vec<(u32, RestartSignal)>>,
        spawn_requests: RefCell<Vec<SpawnRequest>>,
        pid_written_on_spawn: Option<(PathBuf, u32)>,
    }

    impl MockProcessControl {
        fn new(process_exists: Vec<bool>) -> Self {
            Self {
                process_exists: RefCell::new(process_exists.into()),
                sent_signals: RefCell::new(Vec::new()),
                spawn_requests: RefCell::new(Vec::new()),
                pid_written_on_spawn: None,
            }
        }

        fn with_pid_written_on_spawn(mut self, pid_file: PathBuf, pid: u32) -> Self {
            self.pid_written_on_spawn = Some((pid_file, pid));
            self
        }
    }

    impl ProcessControl for MockProcessControl {
        fn process_exists(&self, _pid: u32) -> anyhow::Result<bool> {
            Ok(self
                .process_exists
                .borrow_mut()
                .pop_front()
                .unwrap_or(false))
        }

        fn send_signal(&self, pid: u32, signal: RestartSignal) -> anyhow::Result<()> {
            self.sent_signals.borrow_mut().push((pid, signal));
            Ok(())
        }

        fn spawn_background(&self, request: &SpawnRequest) -> anyhow::Result<()> {
            self.spawn_requests.borrow_mut().push(request.clone());
            if let Some((pid_file, pid)) = &self.pid_written_on_spawn {
                fs::write(pid_file, format!("{pid}\n")).context("write spawned pid file")?;
            }
            Ok(())
        }

        fn sleep(&self, _duration: Duration) {}
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
        let control = MockProcessControl::new(Vec::new());

        let outcome = execute_stop(&control, &temp.path().join("fawx.pid")).expect("stop outcome");

        assert_eq!(outcome, StopOutcome::NotRunning);
        assert!(control.sent_signals.borrow().is_empty());
    }

    #[test]
    fn stop_with_stale_pid_file_removes_it() {
        let temp = TempDir::new().expect("tempdir");
        let pid_file = write_pid_file(&temp, 4321);
        let control = MockProcessControl::new(vec![false]);

        let outcome = execute_stop(&control, &pid_file).expect("stop outcome");

        assert_eq!(outcome, StopOutcome::StalePidRemoved);
        assert!(!pid_file.exists());
    }

    #[test]
    fn stop_happy_path_sends_sigterm_and_removes_pid_file() {
        let temp = TempDir::new().expect("tempdir");
        let pid_file = write_pid_file(&temp, 4321);
        let control = MockProcessControl::new(vec![true, false]);

        let outcome = execute_stop(&control, &pid_file).expect("stop outcome");

        assert_eq!(outcome, StopOutcome::Stopped);
        assert_eq!(
            *control.sent_signals.borrow(),
            vec![(4321, RestartSignal::Terminate)]
        );
        assert!(!pid_file.exists());
    }

    #[test]
    fn stop_sigkill_fallback_verifies_process_exit() {
        let temp = TempDir::new().expect("tempdir");
        let pid_file = write_pid_file(&temp, 4321);
        let control = MockProcessControl::new(sigkill_fallback_responses());

        let outcome = execute_stop(&control, &pid_file).expect("stop outcome");

        assert_eq!(outcome, StopOutcome::Stopped);
        assert_eq!(
            *control.sent_signals.borrow(),
            vec![
                (4321, RestartSignal::Terminate),
                (4321, RestartSignal::Kill),
            ]
        );
        assert!(control.process_exists.borrow().is_empty());
        assert!(!pid_file.exists());
    }

    #[test]
    fn start_with_live_pid_file_reports_already_running() {
        let temp = TempDir::new().expect("tempdir");
        let paths = start_paths(&temp);
        fs::write(&paths.pid_file, "777\n").expect("write pid file");
        let control = MockProcessControl::new(vec![true]);

        let outcome = execute_start(&control, &paths).expect("start outcome");

        assert_eq!(outcome, StartOutcome::AlreadyRunning(777));
        assert!(control.spawn_requests.borrow().is_empty());
    }

    #[test]
    fn start_happy_path_spawns_process_and_waits_for_pid_file() {
        let temp = TempDir::new().expect("tempdir");
        let paths = start_paths(&temp);
        let control = MockProcessControl::new(vec![true])
            .with_pid_written_on_spawn(paths.pid_file.clone(), 9001);

        let outcome = execute_start(&control, &paths).expect("start outcome");

        assert_eq!(outcome, StartOutcome::Started);
        assert_eq!(
            restart::read_pid_file(&paths.pid_file).expect("read pid file"),
            Some(9001)
        );
        assert_eq!(control.spawn_requests.borrow().len(), 1);
        assert_eq!(
            control.spawn_requests.borrow()[0].log_file,
            paths.logs_dir.join(DAEMON_LOG_FILE)
        );
    }

    #[test]
    fn start_with_stale_pid_file_removes_it_before_spawning() {
        let temp = TempDir::new().expect("tempdir");
        let paths = start_paths(&temp);
        fs::write(&paths.pid_file, "777\n").expect("write stale pid file");
        let control = MockProcessControl::new(vec![false, true])
            .with_pid_written_on_spawn(paths.pid_file.clone(), 9002);

        let outcome = execute_start(&control, &paths).expect("start outcome");

        assert_eq!(outcome, StartOutcome::Started);
        assert_eq!(
            restart::read_pid_file(&paths.pid_file).expect("read pid file"),
            Some(9002)
        );
        assert_eq!(control.spawn_requests.borrow().len(), 1);
    }

    #[test]
    fn start_timeout_errors_when_pid_file_never_appears() {
        let temp = TempDir::new().expect("tempdir");
        let paths = start_paths(&temp);
        let control = MockProcessControl::new(Vec::new());

        let error = execute_start(&control, &paths).expect_err("start should time out");

        assert!(error.to_string().contains("within 5 seconds"));
        assert!(!paths.pid_file.exists());
    }
}
