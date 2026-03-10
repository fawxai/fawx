use crate::startup;
use anyhow::{anyhow, Context};
use clap::Args;
use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const PID_FILE_NAME: &str = "fawx.pid";
const DEFAULT_STOP_TIMEOUT: Duration = Duration::from_secs(10);
const RESTART_POLL_INTERVAL: Duration = Duration::from_millis(100);
const DEFAULT_START_ARGS: &[&str] = &["serve", "--http"];

#[derive(Args, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RestartArgs {
    /// Stop the engine, rebuild the release binary, then start it again
    #[arg(long, conflicts_with = "hard")]
    pub(crate) rebuild: bool,

    /// Stop the engine and start it again without rebuilding
    #[arg(long, conflicts_with = "rebuild")]
    pub(crate) hard: bool,
}

impl RestartArgs {
    pub(crate) fn request(self) -> RestartRequest {
        let mode = if self.rebuild {
            RestartMode::Rebuild
        } else if self.hard {
            RestartMode::Hard
        } else {
            RestartMode::Graceful
        };
        RestartRequest { mode }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RestartRequest {
    mode: RestartMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RestartMode {
    Graceful,
    Hard,
    Rebuild,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RestartSignal {
    Hangup,
    Terminate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RestartConfig {
    pid_file: PathBuf,
    current_exe: PathBuf,
    repo_root: Option<PathBuf>,
    stop_timeout: Duration,
}

pub(crate) fn run(args: RestartArgs) -> anyhow::Result<i32> {
    let config = restart_config()?;
    execute_restart(&LiveRestartSystem, &config, args.request())?;
    Ok(0)
}

pub(crate) fn create_serve_pid_file_guard() -> anyhow::Result<PidFileGuard> {
    PidFileGuard::create(pid_file_path())
}

fn restart_config() -> anyhow::Result<RestartConfig> {
    let current_exe = std::env::current_exe().context("failed to locate current executable")?;
    let current_dir = std::env::current_dir().context("failed to read current directory")?;
    Ok(RestartConfig {
        pid_file: pid_file_path(),
        repo_root: discover_repo_root(&current_exe, &current_dir),
        current_exe,
        stop_timeout: DEFAULT_STOP_TIMEOUT,
    })
}

fn pid_file_path() -> PathBuf {
    let base_data_dir = startup::fawx_data_dir();
    let data_dir = startup::load_config()
        .map(|config| startup::configured_data_dir(&base_data_dir, &config))
        .unwrap_or(base_data_dir);
    data_dir.join(PID_FILE_NAME)
}

fn discover_repo_root(current_exe: &Path, current_dir: &Path) -> Option<PathBuf> {
    find_repo_root(current_dir).or_else(|| find_repo_root_from_exe(current_exe))
}

fn find_repo_root(current_dir: &Path) -> Option<PathBuf> {
    current_dir
        .ancestors()
        .find(|path| is_repo_root(path))
        .map(Path::to_path_buf)
}

fn find_repo_root_from_exe(current_exe: &Path) -> Option<PathBuf> {
    current_exe
        .parent()
        .and_then(|path| path.ancestors().find(|candidate| is_repo_root(candidate)))
        .map(Path::to_path_buf)
}

fn is_repo_root(path: &Path) -> bool {
    path.join("Cargo.toml").is_file() && path.join("engine/crates/fx-cli/Cargo.toml").is_file()
}

fn execute_restart(
    system: &impl RestartSystem,
    config: &RestartConfig,
    request: RestartRequest,
) -> anyhow::Result<()> {
    let pid = resolve_target_pid(system, &config.pid_file)?;
    match request.mode {
        RestartMode::Graceful => graceful_restart(system, pid),
        RestartMode::Hard => stop_and_start(system, config, pid, false),
        RestartMode::Rebuild => stop_and_start(system, config, pid, true),
    }
}

fn graceful_restart(system: &impl RestartSystem, pid: u32) -> anyhow::Result<()> {
    system.send_signal(pid, RestartSignal::Hangup)?;
    println!("Sent SIGHUP to fawx (pid {pid})");
    Ok(())
}

fn stop_and_start(
    system: &impl RestartSystem,
    config: &RestartConfig,
    pid: u32,
    rebuild: bool,
) -> anyhow::Result<()> {
    system.send_signal(pid, RestartSignal::Terminate)?;
    wait_for_exit(system, pid, config.stop_timeout)?;
    if rebuild {
        rebuild_binary(system, config.repo_root.as_deref())?;
    }
    let executable = executable_to_start(config, rebuild);
    system.spawn_serve(&executable)?;
    println!("Started fawx via {}", executable.display());
    Ok(())
}

fn rebuild_binary(system: &impl RestartSystem, repo_root: Option<&Path>) -> anyhow::Result<()> {
    let repo_root =
        repo_root.ok_or_else(|| anyhow!("unable to locate the fawx repo for --rebuild"))?;
    system.run_rebuild(repo_root)
}

fn executable_to_start(config: &RestartConfig, rebuild: bool) -> PathBuf {
    if !rebuild {
        return config.current_exe.clone();
    }
    config
        .repo_root
        .as_deref()
        .map(release_binary_path)
        .filter(|path| path.is_file())
        .unwrap_or_else(|| config.current_exe.clone())
}

fn release_binary_path(repo_root: &Path) -> PathBuf {
    repo_root
        .join("target")
        .join("release")
        .join(fawx_binary_name())
}

fn fawx_binary_name() -> &'static str {
    if cfg!(windows) {
        "fawx.exe"
    } else {
        "fawx"
    }
}

fn resolve_target_pid(system: &impl RestartSystem, pid_file: &Path) -> anyhow::Result<u32> {
    if let Some(pid) = read_pid_file(pid_file)? {
        if system.process_exists(pid)? {
            return Ok(pid);
        }
        remove_pid_file(pid_file)?;
    }
    system
        .find_fawx_process(std::process::id())?
        .ok_or_else(|| anyhow!("no running fawx serve process found"))
}

fn read_pid_file(path: &Path) -> anyhow::Result<Option<u32>> {
    match fs::read_to_string(path) {
        Ok(contents) => parse_pid_file(&contents).map(Some),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).context(format!("failed to read pid file {}", path.display())),
    }
}

fn parse_pid_file(contents: &str) -> anyhow::Result<u32> {
    let trimmed = contents.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("pid file is empty"));
    }
    trimmed
        .parse()
        .context("pid file did not contain a valid pid")
}

fn write_pid_file(path: &Path, pid: u32) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("pid file path {} has no parent directory", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    fs::write(path, format!("{pid}\n"))
        .with_context(|| format!("failed to write pid file {}", path.display()))
}

fn remove_pid_file(path: &Path) -> anyhow::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).context(format!("failed to remove pid file {}", path.display())),
    }
}

fn remove_pid_file_if_owned(path: &Path, pid: u32) -> anyhow::Result<()> {
    match read_pid_file(path)? {
        Some(stored_pid) if stored_pid == pid => remove_pid_file(path),
        _ => Ok(()),
    }
}

fn wait_for_exit(system: &impl RestartSystem, pid: u32, timeout: Duration) -> anyhow::Result<()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !system.process_exists(pid)? {
            return Ok(());
        }
        thread::sleep(RESTART_POLL_INTERVAL);
    }
    if system.process_exists(pid)? {
        Err(anyhow!("timed out waiting for fawx (pid {pid}) to exit"))
    } else {
        Ok(())
    }
}

pub(crate) struct PidFileGuard {
    path: PathBuf,
    pid: u32,
}

impl PidFileGuard {
    pub(crate) fn create(path: PathBuf) -> anyhow::Result<Self> {
        let pid = std::process::id();
        write_pid_file(&path, pid)?;
        Ok(Self { path, pid })
    }
}

impl Drop for PidFileGuard {
    fn drop(&mut self) {
        if let Err(error) = remove_pid_file_if_owned(&self.path, self.pid) {
            eprintln!(
                "warning: failed to clean up pid file {}: {error}",
                self.path.display()
            );
        }
    }
}

trait RestartSystem {
    fn process_exists(&self, pid: u32) -> anyhow::Result<bool>;
    fn find_fawx_process(&self, exclude_pid: u32) -> anyhow::Result<Option<u32>>;
    fn send_signal(&self, pid: u32, signal: RestartSignal) -> anyhow::Result<()>;
    fn run_rebuild(&self, repo_root: &Path) -> anyhow::Result<()>;
    fn spawn_serve(&self, executable: &Path) -> anyhow::Result<()>;
}

struct LiveRestartSystem;

impl RestartSystem for LiveRestartSystem {
    fn process_exists(&self, pid: u32) -> anyhow::Result<bool> {
        process_exists(pid)
    }

    fn find_fawx_process(&self, exclude_pid: u32) -> anyhow::Result<Option<u32>> {
        find_fawx_process(exclude_pid)
    }

    fn send_signal(&self, pid: u32, signal: RestartSignal) -> anyhow::Result<()> {
        send_signal(pid, signal)
    }

    fn run_rebuild(&self, repo_root: &Path) -> anyhow::Result<()> {
        run_rebuild(repo_root)
    }

    fn spawn_serve(&self, executable: &Path) -> anyhow::Result<()> {
        spawn_serve(executable)
    }
}

fn find_fawx_process(exclude_pid: u32) -> anyhow::Result<Option<u32>> {
    let output = Command::new("ps")
        .args(["-A", "-ww", "-o", "pid=,command="])
        .output()
        .context("failed to run ps while locating fawx")?;
    ensure_command_succeeded(output.status.success(), "ps")?;
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(parse_process_line)
        .find(|(pid, command)| *pid != exclude_pid && is_fawx_serve_command(command))
        .map(|(pid, _)| pid))
}

fn ensure_command_succeeded(success: bool, name: &str) -> anyhow::Result<()> {
    if success {
        Ok(())
    } else {
        Err(anyhow!("{name} exited with a non-zero status"))
    }
}

fn parse_process_line(line: &str) -> Option<(u32, &str)> {
    let trimmed = line.trim_start();
    let split_at = trimmed.find(char::is_whitespace)?;
    let pid = trimmed[..split_at].trim().parse().ok()?;
    Some((pid, trimmed[split_at..].trim()))
}

fn is_fawx_serve_command(command: &str) -> bool {
    command.contains("fawx") && command.contains(" serve") && !command.contains(" restart")
}

fn run_rebuild(repo_root: &Path) -> anyhow::Result<()> {
    let cargo = cargo_binary()?;
    let status = Command::new(cargo)
        .current_dir(repo_root)
        .args(["build", "--release", "-p", "fx-cli"])
        .status()
        .context("failed to start cargo build for fx-cli")?;
    ensure_command_succeeded(status.success(), "cargo build")?;
    Ok(())
}

fn cargo_binary() -> anyhow::Result<PathBuf> {
    which::which("cargo").context("failed to locate cargo in PATH for --rebuild")
}

fn spawn_serve(executable: &Path) -> anyhow::Result<()> {
    let mut command = Command::new(executable);
    command
        .args(DEFAULT_START_ARGS)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command
        .spawn()
        .with_context(|| format!("failed to start {} serve --http", executable.display()))?;
    Ok(())
}

#[cfg(unix)]
fn unix_pid(pid: u32) -> anyhow::Result<nix::unistd::Pid> {
    let pid = i32::try_from(pid).map_err(|_| anyhow!("pid {pid} exceeds Unix pid range"))?;
    Ok(nix::unistd::Pid::from_raw(pid))
}

#[cfg(unix)]
fn process_exists(pid: u32) -> anyhow::Result<bool> {
    use nix::{errno::Errno, sys::signal};
    match signal::kill(unix_pid(pid)?, None) {
        Ok(()) => Ok(true),
        Err(Errno::EPERM) => Ok(true),
        Err(Errno::ESRCH) => Ok(false),
        Err(_) => Ok(false),
    }
}

#[cfg(not(unix))]
/// Non-Unix fallback used by restart pid resolution; always returns `false`.
fn process_exists(_pid: u32) -> anyhow::Result<bool> {
    Ok(false)
}

#[cfg(unix)]
fn send_signal(pid: u32, signal: RestartSignal) -> anyhow::Result<()> {
    use nix::sys::signal;
    let signal = match signal {
        RestartSignal::Hangup => signal::Signal::SIGHUP,
        RestartSignal::Terminate => signal::Signal::SIGTERM,
    };
    signal::kill(unix_pid(pid)?, signal)
        .map_err(|error| anyhow!(error))
        .with_context(|| format!("failed to signal fawx pid {pid}"))
}

#[cfg(not(unix))]
fn send_signal(_pid: u32, _signal: RestartSignal) -> anyhow::Result<()> {
    Err(anyhow!("fawx restart is only supported on Unix hosts"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct MockRestartSystem {
        process_exists_responses: RefCell<Vec<bool>>,
        search_result: Option<u32>,
        sent_signals: RefCell<Vec<(u32, RestartSignal)>>,
        rebuild_roots: RefCell<Vec<PathBuf>>,
        spawned_paths: RefCell<Vec<PathBuf>>,
    }

    impl MockRestartSystem {
        fn new(process_exists_responses: Vec<bool>, search_result: Option<u32>) -> Self {
            Self {
                process_exists_responses: RefCell::new(process_exists_responses),
                search_result,
                sent_signals: RefCell::new(Vec::new()),
                rebuild_roots: RefCell::new(Vec::new()),
                spawned_paths: RefCell::new(Vec::new()),
            }
        }
    }

    impl RestartSystem for MockRestartSystem {
        fn process_exists(&self, _pid: u32) -> anyhow::Result<bool> {
            let next = self.process_exists_responses.borrow_mut().pop();
            Ok(next.unwrap_or(false))
        }

        fn find_fawx_process(&self, _exclude_pid: u32) -> anyhow::Result<Option<u32>> {
            Ok(self.search_result)
        }

        fn send_signal(&self, pid: u32, signal: RestartSignal) -> anyhow::Result<()> {
            self.sent_signals.borrow_mut().push((pid, signal));
            Ok(())
        }

        fn run_rebuild(&self, repo_root: &Path) -> anyhow::Result<()> {
            self.rebuild_roots
                .borrow_mut()
                .push(repo_root.to_path_buf());
            Ok(())
        }

        fn spawn_serve(&self, executable: &Path) -> anyhow::Result<()> {
            self.spawned_paths
                .borrow_mut()
                .push(executable.to_path_buf());
            Ok(())
        }
    }

    fn test_restart_config(temp_dir: &tempfile::TempDir) -> RestartConfig {
        RestartConfig {
            pid_file: temp_dir.path().join(PID_FILE_NAME),
            current_exe: temp_dir.path().join("target").join("debug").join("fawx"),
            repo_root: None,
            stop_timeout: Duration::from_millis(1),
        }
    }

    #[test]
    fn pid_file_guard_writes_reads_and_cleans_up() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let path = temp_dir.path().join(PID_FILE_NAME);
        let guard = PidFileGuard::create(path.clone()).expect("create pid guard");

        assert_eq!(
            read_pid_file(&path).expect("read pid"),
            Some(std::process::id())
        );

        drop(guard);
        assert_eq!(read_pid_file(&path).expect("read missing pid"), None);
    }

    #[test]
    fn resolve_target_pid_prefers_live_pid_file() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let pid_file = temp_dir.path().join(PID_FILE_NAME);
        write_pid_file(&pid_file, 4242).expect("write pid file");
        let system = MockRestartSystem::new(vec![true], Some(7));

        let pid = resolve_target_pid(&system, &pid_file).expect("resolve pid");

        assert_eq!(pid, 4242);
    }

    #[test]
    fn resolve_target_pid_removes_stale_pid_file_and_falls_back_to_search() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let pid_file = temp_dir.path().join(PID_FILE_NAME);
        write_pid_file(&pid_file, 4242).expect("write pid file");
        let system = MockRestartSystem::new(vec![false], Some(77));

        let pid = resolve_target_pid(&system, &pid_file).expect("resolve pid");

        assert_eq!(pid, 77);
        assert_eq!(read_pid_file(&pid_file).expect("read pid file"), None);
    }

    #[test]
    fn resolve_target_pid_uses_process_search_when_pid_file_is_missing() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let pid_file = temp_dir.path().join(PID_FILE_NAME);
        let system = MockRestartSystem::new(Vec::new(), Some(55));

        let pid = resolve_target_pid(&system, &pid_file).expect("resolve pid");

        assert_eq!(pid, 55);
    }

    #[test]
    fn resolve_target_pid_rejects_invalid_pid_files() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let pid_file = temp_dir.path().join(PID_FILE_NAME);
        fs::write(&pid_file, "not-a-pid\n").expect("write invalid pid file");
        let system = MockRestartSystem::new(Vec::new(), Some(55));

        let error = resolve_target_pid(&system, &pid_file).expect_err("invalid pid should fail");

        assert!(error.to_string().contains("valid pid"));
    }

    #[cfg(unix)]
    #[test]
    fn process_exists_rejects_pid_values_outside_unix_range() {
        let error = process_exists(u32::MAX).expect_err("out-of-range pid should fail");

        assert!(error.to_string().contains("exceeds Unix pid range"));
    }

    #[test]
    fn graceful_restart_sends_sighup_without_spawning() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let config = test_restart_config(&temp_dir);
        write_pid_file(&config.pid_file, 9001).expect("write pid file");
        let system = MockRestartSystem::new(vec![true], None);

        execute_restart(
            &system,
            &config,
            RestartRequest {
                mode: RestartMode::Graceful,
            },
        )
        .expect("graceful restart");

        assert_eq!(
            *system.sent_signals.borrow(),
            vec![(9001, RestartSignal::Hangup)]
        );
        assert!(system.spawned_paths.borrow().is_empty());
    }

    #[test]
    fn hard_restart_sends_sigterm_waits_and_restarts() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let config = test_restart_config(&temp_dir);
        write_pid_file(&config.pid_file, 9002).expect("write pid file");
        let system = MockRestartSystem::new(vec![false, true], None);

        execute_restart(
            &system,
            &config,
            RestartRequest {
                mode: RestartMode::Hard,
            },
        )
        .expect("hard restart");

        assert_eq!(
            *system.sent_signals.borrow(),
            vec![(9002, RestartSignal::Terminate)]
        );
        assert_eq!(
            *system.spawned_paths.borrow(),
            vec![config.current_exe.clone()]
        );
    }

    #[test]
    fn rebuild_restart_runs_cargo_then_starts_release_binary() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let repo_root = temp_dir.path().join("repo");
        let release_binary = release_binary_path(&repo_root);
        fs::create_dir_all(release_binary.parent().expect("parent")).expect("release dir");
        fs::write(&release_binary, "binary").expect("release binary");
        let config = RestartConfig {
            pid_file: temp_dir.path().join(PID_FILE_NAME),
            current_exe: temp_dir.path().join("target").join("debug").join("fawx"),
            repo_root: Some(repo_root.clone()),
            stop_timeout: Duration::from_millis(1),
        };
        write_pid_file(&config.pid_file, 9003).expect("write pid file");
        let system = MockRestartSystem::new(vec![false, true], None);

        execute_restart(
            &system,
            &config,
            RestartRequest {
                mode: RestartMode::Rebuild,
            },
        )
        .expect("rebuild restart");

        assert_eq!(
            *system.sent_signals.borrow(),
            vec![(9003, RestartSignal::Terminate)]
        );
        assert_eq!(*system.rebuild_roots.borrow(), vec![repo_root]);
        assert_eq!(*system.spawned_paths.borrow(), vec![release_binary]);
    }

    #[test]
    fn parse_process_line_extracts_pid_and_command() {
        let parsed = parse_process_line(" 123 /usr/bin/fawx serve --http").expect("parsed line");

        assert_eq!(parsed.0, 123);
        assert_eq!(parsed.1, "/usr/bin/fawx serve --http");
    }

    #[test]
    fn is_fawx_serve_command_filters_out_restart_calls() {
        assert!(is_fawx_serve_command("/tmp/fawx serve --http"));
        assert!(!is_fawx_serve_command("/tmp/fawx restart --hard"));
    }

    #[cfg(unix)]
    #[test]
    fn send_signal_rejects_pid_values_outside_unix_range() {
        let error = send_signal(u32::MAX, RestartSignal::Terminate)
            .expect_err("out-of-range pid should fail");

        assert!(error.to_string().contains("exceeds Unix pid range"));
    }

    #[test]
    fn pid_file_cleanup_preserves_newer_owner() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let path = temp_dir.path().join(PID_FILE_NAME);
        write_pid_file(&path, 42).expect("write pid file");

        remove_pid_file_if_owned(&path, 7).expect("cleanup should ignore newer pid");

        assert_eq!(read_pid_file(&path).expect("read pid"), Some(42));
    }

    #[test]
    fn discover_repo_root_prefers_current_directory() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let repo_root = temp_dir.path().join("repo");
        fs::create_dir_all(repo_root.join("engine/crates/fx-cli")).expect("crate dir");
        fs::write(repo_root.join("Cargo.toml"), "[workspace]\n").expect("workspace file");
        fs::write(
            repo_root.join("engine/crates/fx-cli/Cargo.toml"),
            "[package]\nname = \"fx-cli\"\n",
        )
        .expect("crate manifest");
        let current_dir = repo_root.join("engine").join("crates");
        let current_exe = repo_root.join("target/release/fawx");

        let discovered = discover_repo_root(&current_exe, &current_dir).expect("repo root");

        assert_eq!(discovered, repo_root);
    }
}
