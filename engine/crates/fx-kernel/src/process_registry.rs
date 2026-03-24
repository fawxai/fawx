use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

const CLEANUP_INTERVAL: Duration = Duration::from_secs(30);
const SHUTDOWN_GRACE_PERIOD: Duration = Duration::from_secs(5);
const STOP_POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessConfig {
    pub max_concurrent: usize,
    pub max_lifetime_secs: u64,
    pub max_output_lines: usize,
    pub allowed_dirs: Vec<PathBuf>,
}

impl Default for ProcessConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 5,
            max_lifetime_secs: 3_600,
            max_output_lines: 10_000,
            allowed_dirs: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessStatus {
    Running,
    Completed { exit_code: i32 },
    Failed { exit_code: i32 },
    Killed,
    TimedOut,
}

impl ProcessStatus {
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed { .. } => "completed",
            Self::Failed { .. } => "failed",
            Self::Killed => "killed",
            Self::TimedOut => "timed_out",
        }
    }

    #[must_use]
    pub fn exit_code(&self) -> Option<i32> {
        match self {
            Self::Completed { exit_code } | Self::Failed { exit_code } => Some(*exit_code),
            Self::Running | Self::Killed | Self::TimedOut => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnResult {
    pub session_id: String,
    pub pid: u32,
    pub label: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusResult {
    pub session_id: String,
    pub label: String,
    pub working_dir: PathBuf,
    pub status: ProcessStatus,
    pub runtime_seconds: u64,
    pub output_lines: usize,
    pub tail: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListEntry {
    pub session_id: String,
    pub label: String,
    pub working_dir: PathBuf,
    pub status: ProcessStatus,
    pub runtime_seconds: u64,
    pub output_lines: usize,
}

#[derive(Debug)]
pub struct ProcessRegistry {
    config: ProcessConfig,
    processes: Mutex<HashMap<String, Arc<ProcessEntry>>>,
    next_id: AtomicU32,
}

#[derive(Debug)]
pub struct ProcessEntry {
    session_id: String,
    label: String,
    pid: u32,
    started_at: Instant,
    working_dir: PathBuf,
    state: Mutex<EntryState>,
}

#[derive(Debug)]
struct EntryState {
    status: ProcessStatus,
    pending_termination: Option<ProcessStatus>,
    output: OutputBuffer,
}

#[derive(Debug)]
struct OutputBuffer {
    lines: VecDeque<String>,
    total_lines: usize,
    max_lines: usize,
}

impl OutputBuffer {
    fn new(max_lines: usize) -> Self {
        Self {
            lines: VecDeque::new(),
            total_lines: 0,
            max_lines,
        }
    }

    fn push(&mut self, line: String) {
        self.total_lines += 1;
        if self.max_lines == 0 {
            return;
        }
        while self.lines.len() >= self.max_lines {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
    }

    fn tail(&self, limit: usize) -> Vec<String> {
        let start = self.lines.len().saturating_sub(limit);
        self.lines.iter().skip(start).cloned().collect()
    }
}

impl ProcessEntry {
    fn new(
        session_id: String,
        label: String,
        pid: u32,
        working_dir: PathBuf,
        max_output_lines: usize,
    ) -> Self {
        Self {
            session_id,
            label,
            pid,
            started_at: Instant::now(),
            working_dir,
            state: Mutex::new(EntryState {
                status: ProcessStatus::Running,
                pending_termination: None,
                output: OutputBuffer::new(max_output_lines),
            }),
        }
    }

    fn is_running(&self) -> bool {
        self.current_status() == ProcessStatus::Running
    }

    fn current_status(&self) -> ProcessStatus {
        self.lock_state().status.clone()
    }

    fn started_before(&self, cutoff: Duration) -> bool {
        self.started_at.elapsed() >= cutoff
    }

    fn queue_termination(&self, status: ProcessStatus) -> Result<(), String> {
        let mut state = self.lock_state();
        if state.status != ProcessStatus::Running {
            return Err("process is not running".to_string());
        }
        state.pending_termination = Some(status);
        Ok(())
    }

    fn push_output(&self, line: String) {
        self.lock_state().output.push(line);
    }

    fn finish(&self, result: std::io::Result<ExitStatus>) {
        let mut state = self.lock_state();
        if let Some(status) = state.pending_termination.take() {
            state.status = status;
            return;
        }
        state.status = completion_status(result);
    }

    fn status_result(&self, tail: usize) -> StatusResult {
        let state = self.lock_state();
        StatusResult {
            session_id: self.session_id.clone(),
            label: self.label.clone(),
            working_dir: self.working_dir.clone(),
            status: state.status.clone(),
            runtime_seconds: self.started_at.elapsed().as_secs(),
            output_lines: state.output.total_lines,
            tail: state.output.tail(tail),
        }
    }

    fn list_entry(&self) -> ListEntry {
        let state = self.lock_state();
        ListEntry {
            session_id: self.session_id.clone(),
            label: self.label.clone(),
            working_dir: self.working_dir.clone(),
            status: state.status.clone(),
            runtime_seconds: self.started_at.elapsed().as_secs(),
            output_lines: state.output.total_lines,
        }
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, EntryState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl ProcessRegistry {
    #[must_use]
    pub fn new(config: ProcessConfig) -> Self {
        Self {
            next_id: AtomicU32::new(seed_id()),
            config,
            processes: Mutex::new(HashMap::new()),
        }
    }

    pub fn spawn_cleanup_task(registry: &Arc<Self>) {
        let weak = Arc::downgrade(registry);
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                cleanup_loop(weak).await;
            });
        }
    }

    pub fn spawn(
        &self,
        command: String,
        working_dir: PathBuf,
        label: Option<String>,
    ) -> Result<SpawnResult, String> {
        self.validate_spawn(&command, &working_dir)?;
        let session_id = self.next_session_id();
        let label = resolve_label(label, &command);
        let mut child = spawn_child(&command, &working_dir)?;
        let pid = child
            .id()
            .ok_or_else(|| "failed to read child pid".to_string())?;
        let entry = Arc::new(ProcessEntry::new(
            session_id.clone(),
            label.clone(),
            pid,
            working_dir,
            self.config.max_output_lines,
        ));
        self.insert_entry(Arc::clone(&entry));
        let readers = spawn_monitors(&mut child, Arc::clone(&entry));
        tokio::spawn(async move {
            wait_for_exit(child, entry, readers).await;
        });
        Ok(SpawnResult {
            session_id,
            pid,
            label,
            status: ProcessStatus::Running.name().to_string(),
        })
    }

    #[must_use]
    pub fn status(&self, session_id: &str, tail: usize) -> Option<StatusResult> {
        self.entry(session_id)
            .map(|entry| entry.status_result(tail))
    }

    #[must_use]
    pub fn list(&self) -> Vec<ListEntry> {
        let mut entries = self
            .process_map()
            .values()
            .map(|entry| entry.list_entry())
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| left.session_id.cmp(&right.session_id));
        entries
    }

    pub async fn kill(&self, session_id: &str) -> Result<(), String> {
        let entry = self
            .entry(session_id)
            .ok_or_else(|| format!("unknown session_id: {session_id}"))?;
        self.terminate_entry(entry, ProcessStatus::Killed).await
    }

    pub async fn cleanup_expired_once(&self) {
        let expired = self.expired_entries();
        for entry in expired {
            let _ = self.terminate_entry(entry, ProcessStatus::TimedOut).await;
        }
    }

    pub async fn shutdown(&self) {
        let running = self.running_entries();
        for entry in running {
            let _ = self.terminate_entry(entry, ProcessStatus::Killed).await;
        }
    }

    fn validate_spawn(&self, command: &str, working_dir: &Path) -> Result<(), String> {
        if command.trim().is_empty() {
            return Err("command cannot be empty".to_string());
        }
        self.validate_working_dir(working_dir)?;
        if self.running_entries().len() >= self.config.max_concurrent {
            return Err(format!(
                "max concurrent background processes reached ({})",
                self.config.max_concurrent
            ));
        }
        Ok(())
    }

    fn validate_working_dir(&self, working_dir: &Path) -> Result<(), String> {
        if self.config.allowed_dirs.is_empty() {
            return canonicalize_existing_or_parent(working_dir).map(|_| ());
        }
        let resolved = canonicalize_existing_or_parent(working_dir)?;
        if self
            .allowed_dirs()
            .iter()
            .any(|allowed| path_within(&resolved, allowed))
        {
            return Ok(());
        }
        Err(format!(
            "working directory outside allowed_dirs: {}",
            resolved.display()
        ))
    }

    fn allowed_dirs(&self) -> Vec<PathBuf> {
        self.config
            .allowed_dirs
            .iter()
            .filter_map(|path| canonicalize_existing_or_parent(path).ok())
            .collect()
    }

    fn next_session_id(&self) -> String {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) & 0x00ff_ffff;
        format!("bg_{id:06x}")
    }

    fn insert_entry(&self, entry: Arc<ProcessEntry>) {
        self.process_map().insert(entry.session_id.clone(), entry);
    }

    fn expired_entries(&self) -> Vec<Arc<ProcessEntry>> {
        let cutoff = Duration::from_secs(self.config.max_lifetime_secs);
        self.process_map()
            .values()
            .filter(|entry| entry.is_running() && entry.started_before(cutoff))
            .cloned()
            .collect()
    }

    fn running_entries(&self) -> Vec<Arc<ProcessEntry>> {
        self.process_map()
            .values()
            .filter(|entry| entry.is_running())
            .cloned()
            .collect()
    }

    fn entry(&self, session_id: &str) -> Option<Arc<ProcessEntry>> {
        self.process_map().get(session_id).cloned()
    }

    async fn terminate_entry(
        &self,
        entry: Arc<ProcessEntry>,
        final_status: ProcessStatus,
    ) -> Result<(), String> {
        prepare_termination(&entry, final_status)?;
        terminate_process_group(&entry).await
    }

    fn shutdown_now(&self) {
        for entry in self.running_entries() {
            let _ = terminate_process_group_blocking(&entry, ProcessStatus::Killed);
        }
    }

    fn process_map(&self) -> std::sync::MutexGuard<'_, HashMap<String, Arc<ProcessEntry>>> {
        self.processes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Drop for ProcessRegistry {
    fn drop(&mut self) {
        self.shutdown_now();
    }
}

fn seed_id() -> u32 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.subsec_nanos())
        .unwrap_or(0);
    nanos & 0x00ff_ffff
}

fn resolve_label(label: Option<String>, command: &str) -> String {
    label
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| command.trim().to_string())
}

fn spawn_child(command: &str, working_dir: &Path) -> Result<Child, String> {
    let mut built = Command::new("/bin/sh");
    built
        .kill_on_drop(true)
        .arg("-c")
        .arg(command)
        .current_dir(working_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        built.process_group(0);
    }
    built.spawn().map_err(|error| error.to_string())
}

fn spawn_monitors(child: &mut Child, entry: Arc<ProcessEntry>) -> Vec<tokio::task::JoinHandle<()>> {
    let mut readers = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        readers.push(tokio::spawn(capture_stream(stdout, Arc::clone(&entry))));
    }
    if let Some(stderr) = child.stderr.take() {
        readers.push(tokio::spawn(capture_stream(stderr, entry)));
    }
    readers
}

async fn capture_stream<T>(stream: T, entry: Arc<ProcessEntry>)
where
    T: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    let mut lines = BufReader::new(stream).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => entry.push_output(line),
            Ok(None) => return,
            Err(error) => {
                entry.push_output(format!("[capture error] {error}"));
                return;
            }
        }
    }
}

async fn wait_for_exit(
    mut child: Child,
    entry: Arc<ProcessEntry>,
    readers: Vec<tokio::task::JoinHandle<()>>,
) {
    let result = child.wait().await;
    await_readers(readers).await;
    entry.finish(result);
}

async fn await_readers(readers: Vec<tokio::task::JoinHandle<()>>) {
    for reader in readers {
        let _ = reader.await;
    }
}

fn completion_status(result: std::io::Result<ExitStatus>) -> ProcessStatus {
    let Ok(status) = result else {
        return ProcessStatus::Failed { exit_code: -1 };
    };
    let code = status.code().unwrap_or(-1);
    if status.success() {
        ProcessStatus::Completed { exit_code: code }
    } else {
        ProcessStatus::Failed { exit_code: code }
    }
}

fn prepare_termination(entry: &ProcessEntry, final_status: ProcessStatus) -> Result<(), String> {
    entry.queue_termination(final_status)
}

async fn terminate_process_group(entry: &ProcessEntry) -> Result<(), String> {
    send_term(entry.pid)?;
    if wait_for_stop(entry, SHUTDOWN_GRACE_PERIOD).await {
        return Ok(());
    }
    send_kill(entry.pid)?;
    let _ = wait_for_stop(entry, SHUTDOWN_GRACE_PERIOD).await;
    Ok(())
}

fn terminate_process_group_blocking(
    entry: &ProcessEntry,
    final_status: ProcessStatus,
) -> Result<(), String> {
    prepare_termination(entry, final_status)?;
    send_term(entry.pid)?;
    if wait_for_stop_blocking(entry, SHUTDOWN_GRACE_PERIOD) {
        return Ok(());
    }
    send_kill(entry.pid)?;
    let _ = wait_for_stop_blocking(entry, SHUTDOWN_GRACE_PERIOD);
    Ok(())
}

async fn wait_for_stop(entry: &ProcessEntry, timeout: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if process_stopped(entry) {
            return true;
        }
        tokio::time::sleep(STOP_POLL_INTERVAL).await;
    }
    process_stopped(entry)
}

fn wait_for_stop_blocking(entry: &ProcessEntry, timeout: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if process_stopped(entry) {
            return true;
        }
        std::thread::sleep(STOP_POLL_INTERVAL);
    }
    process_stopped(entry)
}

fn process_stopped(entry: &ProcessEntry) -> bool {
    !entry.is_running() || !process_group_is_running(entry.pid)
}

async fn cleanup_loop(registry: std::sync::Weak<ProcessRegistry>) {
    loop {
        {
            let Some(strong) = registry.upgrade() else {
                return;
            };
            strong.cleanup_expired_once().await;
        }
        tokio::time::sleep(CLEANUP_INTERVAL).await;
    }
}

#[cfg(unix)]
fn send_term(pid: u32) -> Result<(), String> {
    use nix::sys::signal::{self, Signal};
    use nix::unistd::Pid;

    signal::killpg(Pid::from_raw(pid as i32), Signal::SIGTERM).map_err(|error| error.to_string())
}

#[cfg(not(unix))]
fn send_term(_pid: u32) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
fn send_kill(pid: u32) -> Result<(), String> {
    use nix::sys::signal::{self, Signal};
    use nix::unistd::Pid;

    signal::killpg(Pid::from_raw(pid as i32), Signal::SIGKILL).map_err(|error| error.to_string())
}

#[cfg(not(unix))]
fn send_kill(_pid: u32) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
fn process_group_is_running(pid: u32) -> bool {
    use nix::errno::Errno;
    use nix::sys::signal;
    use nix::unistd::Pid;

    match signal::killpg(Pid::from_raw(pid as i32), None::<signal::Signal>) {
        Ok(()) | Err(Errno::EPERM) => true,
        Err(Errno::ESRCH) => false,
        Err(_) => true,
    }
}

#[cfg(not(unix))]
fn process_group_is_running(_pid: u32) -> bool {
    true
}

fn canonicalize_existing_or_parent(path: &Path) -> Result<PathBuf, String> {
    if path.exists() {
        return fs::canonicalize(path).map_err(|error| error.to_string());
    }
    let mut missing = Vec::new();
    let mut cursor = path;
    while !cursor.exists() {
        let name = cursor
            .file_name()
            .ok_or_else(|| "invalid target path".to_string())?;
        missing.push(name.to_os_string());
        cursor = cursor
            .parent()
            .ok_or_else(|| "invalid target path".to_string())?;
    }
    let mut resolved = fs::canonicalize(cursor).map_err(|error| error.to_string())?;
    while let Some(part) = missing.pop() {
        resolved.push(part);
    }
    Ok(resolved)
}

fn path_within(path: &Path, allowed: &Path) -> bool {
    path == allowed || path.starts_with(allowed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_config(root: &Path) -> ProcessConfig {
        ProcessConfig {
            allowed_dirs: vec![root.to_path_buf()],
            ..ProcessConfig::default()
        }
    }

    fn test_registry(root: &Path) -> ProcessRegistry {
        ProcessRegistry::new(test_config(root))
    }

    async fn wait_for_status(
        registry: &ProcessRegistry,
        session_id: &str,
        expected: &ProcessStatus,
    ) -> StatusResult {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(5) {
            let Some(status) = registry.status(session_id, 20) else {
                panic!("missing status for {session_id}");
            };
            if &status.status == expected {
                return status;
            }
            tokio::time::sleep(STOP_POLL_INTERVAL).await;
        }
        panic!("timed out waiting for status {expected:?}");
    }

    async fn wait_for_process_group_exit(pid: u32, timeout: Duration) -> bool {
        let started = Instant::now();
        while started.elapsed() < timeout {
            if !process_group_is_running(pid) {
                return true;
            }
            tokio::time::sleep(STOP_POLL_INTERVAL).await;
        }
        !process_group_is_running(pid)
    }

    #[tokio::test]
    async fn spawn_process_reports_running_status() {
        let temp = TempDir::new().expect("tempdir");
        let registry = test_registry(temp.path());

        let result = registry
            .spawn("sleep 1".to_string(), temp.path().to_path_buf(), None)
            .expect("spawn");
        let status = registry.status(&result.session_id, 10).expect("status");

        assert_eq!(status.status, ProcessStatus::Running);
    }

    #[tokio::test]
    async fn completed_process_reports_exit_code() {
        let temp = TempDir::new().expect("tempdir");
        let registry = test_registry(temp.path());
        let result = registry
            .spawn(
                "printf 'done\\n'".to_string(),
                temp.path().to_path_buf(),
                None,
            )
            .expect("spawn");

        let status = wait_for_status(
            &registry,
            &result.session_id,
            &ProcessStatus::Completed { exit_code: 0 },
        )
        .await;

        assert_eq!(status.tail, vec!["done".to_string()]);
    }

    #[tokio::test]
    async fn kill_process_marks_status_killed() {
        let temp = TempDir::new().expect("tempdir");
        let registry = test_registry(temp.path());
        let result = registry
            .spawn("sleep 30".to_string(), temp.path().to_path_buf(), None)
            .expect("spawn");

        registry.kill(&result.session_id).await.expect("kill");
        let status = wait_for_status(&registry, &result.session_id, &ProcessStatus::Killed).await;

        assert_eq!(status.status, ProcessStatus::Killed);
    }

    #[tokio::test]
    async fn concurrent_limit_is_enforced() {
        let temp = TempDir::new().expect("tempdir");
        let mut config = test_config(temp.path());
        config.max_concurrent = 1;
        let registry = ProcessRegistry::new(config);

        registry
            .spawn("sleep 30".to_string(), temp.path().to_path_buf(), None)
            .expect("first spawn");
        let error = registry
            .spawn("sleep 30".to_string(), temp.path().to_path_buf(), None)
            .expect_err("second spawn should fail");

        assert!(error.contains("max concurrent"));
    }

    #[tokio::test]
    async fn cleanup_marks_expired_processes_timed_out() {
        let temp = TempDir::new().expect("tempdir");
        let mut config = test_config(temp.path());
        config.max_lifetime_secs = 0;
        let registry = ProcessRegistry::new(config);
        let result = registry
            .spawn("sleep 30".to_string(), temp.path().to_path_buf(), None)
            .expect("spawn");

        registry.cleanup_expired_once().await;
        let status = wait_for_status(&registry, &result.session_id, &ProcessStatus::TimedOut).await;

        assert_eq!(status.status, ProcessStatus::TimedOut);
    }

    #[tokio::test]
    async fn output_buffer_captures_stdout() {
        let temp = TempDir::new().expect("tempdir");
        let registry = test_registry(temp.path());
        let result = registry
            .spawn(
                "printf 'alpha\\nbeta\\n'".to_string(),
                temp.path().to_path_buf(),
                None,
            )
            .expect("spawn");

        let status = wait_for_status(
            &registry,
            &result.session_id,
            &ProcessStatus::Completed { exit_code: 0 },
        )
        .await;

        assert_eq!(status.tail, vec!["alpha".to_string(), "beta".to_string()]);
    }

    #[tokio::test]
    async fn output_buffer_captures_stderr() {
        let temp = TempDir::new().expect("tempdir");
        let registry = test_registry(temp.path());
        let result = registry
            .spawn(
                "printf 'error\\n' >&2".to_string(),
                temp.path().to_path_buf(),
                None,
            )
            .expect("spawn");

        let status = wait_for_status(
            &registry,
            &result.session_id,
            &ProcessStatus::Completed { exit_code: 0 },
        )
        .await;

        assert_eq!(status.tail, vec!["error".to_string()]);
    }

    #[tokio::test]
    async fn output_buffer_drops_oldest_lines_when_full() {
        let temp = TempDir::new().expect("tempdir");
        let mut config = test_config(temp.path());
        config.max_output_lines = 2;
        let registry = ProcessRegistry::new(config);
        let result = registry
            .spawn(
                "printf 'one\\ntwo\\nthree\\n'".to_string(),
                temp.path().to_path_buf(),
                None,
            )
            .expect("spawn");

        let status = wait_for_status(
            &registry,
            &result.session_id,
            &ProcessStatus::Completed { exit_code: 0 },
        )
        .await;

        assert_eq!(status.tail, vec!["two".to_string(), "three".to_string()]);
        assert_eq!(status.output_lines, 3);
    }

    #[tokio::test]
    async fn list_returns_all_processes() {
        let temp = TempDir::new().expect("tempdir");
        let registry = test_registry(temp.path());

        registry
            .spawn(
                "sleep 1".to_string(),
                temp.path().to_path_buf(),
                Some("one".to_string()),
            )
            .expect("spawn one");
        registry
            .spawn(
                "sleep 1".to_string(),
                temp.path().to_path_buf(),
                Some("two".to_string()),
            )
            .expect("spawn two");

        let entries = registry.list();

        assert_eq!(entries.len(), 2);
        assert!(entries.iter().any(|entry| entry.label == "one"));
        assert!(entries.iter().any(|entry| entry.label == "two"));
    }

    #[tokio::test]
    async fn dropping_registry_kills_running_processes() {
        let temp = TempDir::new().expect("tempdir");
        let registry = test_registry(temp.path());
        let result = registry
            .spawn(
                "trap '' TERM; while :; do sleep 1; done".to_string(),
                temp.path().to_path_buf(),
                None,
            )
            .expect("spawn");

        assert!(process_group_is_running(result.pid));
        drop(registry);

        assert!(wait_for_process_group_exit(result.pid, Duration::from_secs(1)).await);
    }

    #[tokio::test]
    async fn session_ids_use_bg_prefix_and_hex_suffix() {
        let temp = TempDir::new().expect("tempdir");
        let registry = test_registry(temp.path());

        let first = registry
            .spawn("sleep 1".to_string(), temp.path().to_path_buf(), None)
            .expect("first spawn");
        let second = registry
            .spawn("sleep 1".to_string(), temp.path().to_path_buf(), None)
            .expect("second spawn");

        assert!(first.session_id.starts_with("bg_"));
        assert_eq!(first.session_id.len(), 9);
        assert!(first.session_id[3..]
            .chars()
            .all(|ch| ch.is_ascii_hexdigit()));
        assert_ne!(first.session_id, second.session_id);
    }

    #[test]
    fn working_directory_validation_rejects_outside_path() {
        let allowed = TempDir::new().expect("allowed");
        let outside = TempDir::new().expect("outside");
        let registry = test_registry(allowed.path());

        let error = registry
            .spawn("sleep 1".to_string(), outside.path().to_path_buf(), None)
            .expect_err("spawn should fail");

        assert!(error.contains("outside allowed_dirs"));
    }
}
