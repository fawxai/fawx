//! Live host API implementation for WASM skills running in the kernel.
//!
//! Provides a real [`HostApi`] that routes WASM host calls to the appropriate
//! runtime services (tracing for logs, [`SkillStorage`] for key-value ops).

use fx_core::error::SkillError;
use fx_skills::host_api::HostApi;
use fx_skills::live_host_api::{execute_http_request, CredentialProvider};
use fx_skills::manifest::Capability;
use fx_skills::storage::SkillStorage;
use serde::Serialize;
use std::io::Read;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, ChildStderr, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Default storage quota per skill: 64 KiB.
const DEFAULT_STORAGE_QUOTA: usize = 64 * 1024;
const COMMAND_OUTPUT_LIMIT_BYTES: usize = 512 * 1024;
const COMMAND_TIMEOUT_EXIT_CODE: i32 = -1;
const COMMAND_FAILURE_EXIT_CODE: i32 = -2;
const COMMAND_POLL_INTERVAL_MS: u64 = 10;

#[derive(Serialize)]
struct ShellCommandResult {
    stdout: String,
    stderr: String,
    exit_code: i32,
}

/// Live host API backed by real runtime services.
///
/// Routes WASM host function calls to:
/// - `tracing` for logging
/// - [`SkillStorage`] for key-value persistence
/// - [`CredentialProvider`] for secret retrieval (e.g., GitHub PAT)
/// - `execute_http_request` for outbound HTTP (capability-gated)
/// - Input/output buffers for skill invocation I/O
pub struct LiveHostApi {
    storage: Arc<Mutex<SkillStorage>>,
    input: String,
    output: Arc<Mutex<String>>,
    capabilities: Vec<Capability>,
    credential_provider: Option<Arc<dyn CredentialProvider>>,
}

impl std::fmt::Debug for LiveHostApi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveHostApi")
            .field("input", &self.input)
            .field("capabilities", &self.capabilities)
            .field("credential_provider", &self.credential_provider.is_some())
            .finish_non_exhaustive()
    }
}

/// Configuration for creating a [`LiveHostApi`].
pub struct LiveHostApiConfig<'a> {
    /// Skill name (used for storage isolation).
    pub skill_name: &'a str,
    /// Input JSON string for the skill invocation.
    pub input: String,
    /// Storage quota in bytes (defaults to [`DEFAULT_STORAGE_QUOTA`]).
    pub storage_quota: Option<usize>,
    /// Capabilities the skill has declared in its manifest.
    pub capabilities: Vec<Capability>,
    /// Optional credential provider for bridging secrets to skills.
    pub credential_provider: Option<Arc<dyn CredentialProvider>>,
}

impl LiveHostApi {
    /// Create a new live host API for a skill invocation.
    pub fn new(config: LiveHostApiConfig<'_>) -> Self {
        let quota = config.storage_quota.unwrap_or(DEFAULT_STORAGE_QUOTA);
        Self {
            storage: Arc::new(Mutex::new(SkillStorage::new(config.skill_name, quota))),
            input: config.input,
            output: Arc::new(Mutex::new(String::new())),
            capabilities: config.capabilities,
            credential_provider: config.credential_provider,
        }
    }

    /// Extract and drain the output set by the WASM skill.
    ///
    /// Uses `std::mem::take` to move the string out of the mutex,
    /// leaving an empty string behind.
    pub fn take_output(&self) -> String {
        std::mem::take(
            &mut *self
                .output
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        )
    }

    fn has_capability(&self, capability: Capability) -> bool {
        self.capabilities.contains(&capability)
    }
}

impl HostApi for LiveHostApi {
    fn log(&self, level: u32, message: &str) {
        match level {
            0 => tracing::trace!(target: "wasm_skill", "{}", message),
            1 => tracing::debug!(target: "wasm_skill", "{}", message),
            2 => tracing::info!(target: "wasm_skill", "{}", message),
            3 => tracing::warn!(target: "wasm_skill", "{}", message),
            4 => tracing::error!(target: "wasm_skill", "{}", message),
            _ => tracing::info!(target: "wasm_skill", "level={}: {}", level, message),
        }
    }

    fn kv_get(&self, key: &str) -> Option<String> {
        // Credential provider takes priority (bridges secrets to skills)
        if let Some(provider) = &self.credential_provider {
            if let Some(value) = provider.get_credential(key) {
                return Some((*value).clone());
            }
        }
        // Fall back to skill-local storage
        self.storage
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(key)
    }

    fn kv_set(&mut self, key: &str, value: &str) -> Result<(), SkillError> {
        self.storage
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .set(key, value)
    }

    fn get_input(&self) -> String {
        self.input.clone()
    }

    fn set_output(&mut self, text: &str) {
        *self
            .output
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = text.to_string();
    }

    fn http_request(&self, method: &str, url: &str, headers: &str, body: &str) -> Option<String> {
        if !is_network_allowed(url, &self.capabilities) {
            tracing::error!("http_request denied: domain not in allowlist");
            return None;
        }
        execute_http_request(method, url, headers, body)
    }

    fn exec_command(&self, command: &str, timeout_ms: u32) -> Option<String> {
        if !self.has_capability(Capability::Shell) {
            tracing::error!("exec_command denied: Shell capability not declared");
            return None;
        }
        execute_shell_command(command, timeout_ms)
    }

    fn read_file(&self, path: &str) -> Option<String> {
        if !self.has_capability(Capability::Filesystem) {
            tracing::error!("read_file denied: Filesystem capability not declared");
            return None;
        }
        read_utf8_file(path)
    }

    fn write_file(&self, path: &str, content: &str) -> bool {
        if !self.has_capability(Capability::Filesystem) {
            tracing::error!("write_file denied: Filesystem capability not declared");
            return false;
        }
        write_utf8_file(path, content)
    }

    fn get_output(&self) -> String {
        self.output
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn execute_shell_command(command: &str, timeout_ms: u32) -> Option<String> {
    let mut child = spawn_shell(command)
        .map_err(|error| {
            tracing::error!("exec_command failed to spawn '{command}': {error}");
            error
        })
        .ok()?;
    let (stdout, stderr) = take_command_pipes(&mut child)?;
    let stdout_handle = spawn_output_reader(stdout);
    let stderr_handle = spawn_output_reader(stderr);
    let exit_code = wait_for_command(&mut child, timeout_ms);
    let stdout = join_output_reader(stdout_handle, "stdout")?;
    let stderr = add_timeout_message(
        join_output_reader(stderr_handle, "stderr")?,
        exit_code,
        timeout_ms,
    );
    serialize_command_result(stdout, stderr, exit_code)
}

fn spawn_shell(command: &str) -> std::io::Result<Child> {
    let mut cmd = Command::new("sh");
    cmd.args(["-c", command])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_shell_process_group(&mut cmd);
    cmd.spawn()
}

#[cfg(unix)]
fn configure_shell_process_group(cmd: &mut Command) {
    unsafe {
        cmd.pre_exec(create_process_group);
    }
}

#[cfg(not(unix))]
fn configure_shell_process_group(_cmd: &mut Command) {}

#[cfg(unix)]
fn create_process_group() -> std::io::Result<()> {
    if unsafe { libc::setpgid(0, 0) } == -1 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

fn take_command_pipes(child: &mut Child) -> Option<(ChildStdout, ChildStderr)> {
    let stdout = child.stdout.take().or_else(|| {
        tracing::error!("exec_command: stdout pipe missing");
        None
    })?;
    let stderr = child.stderr.take().or_else(|| {
        tracing::error!("exec_command: stderr pipe missing");
        None
    })?;
    Some((stdout, stderr))
}

fn spawn_output_reader<R>(reader: R) -> JoinHandle<Vec<u8>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || read_capped_output(reader))
}

fn read_capped_output<R>(reader: R) -> Vec<u8>
where
    R: Read,
{
    let mut output = Vec::new();
    let mut limited_reader = reader.take(COMMAND_OUTPUT_LIMIT_BYTES as u64);
    if let Err(error) = limited_reader.read_to_end(&mut output) {
        tracing::error!("exec_command: failed to read process output: {error}");
    }
    output
}

fn wait_for_command(child: &mut Child, timeout_ms: u32) -> i32 {
    let deadline = Instant::now() + Duration::from_millis(u64::from(timeout_ms));
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.code().unwrap_or(COMMAND_FAILURE_EXIT_CODE),
            Ok(None) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(COMMAND_POLL_INTERVAL_MS));
            }
            Ok(None) => return kill_timed_out_child(child),
            Err(error) => {
                tracing::error!("exec_command: failed to wait on child: {error}");
                return COMMAND_FAILURE_EXIT_CODE;
            }
        }
    }
}

fn kill_timed_out_child(child: &mut Child) -> i32 {
    if let Err(error) = terminate_timed_out_child(child) {
        tracing::error!("exec_command: failed to kill timed out child: {error}");
        return COMMAND_FAILURE_EXIT_CODE;
    }
    if let Err(error) = child.wait() {
        tracing::error!("exec_command: failed to reap timed out child: {error}");
        return COMMAND_FAILURE_EXIT_CODE;
    }
    COMMAND_TIMEOUT_EXIT_CODE
}

#[cfg(unix)]
fn terminate_timed_out_child(child: &mut Child) -> std::io::Result<()> {
    if unsafe { libc::killpg(child.id() as i32, libc::SIGKILL) } == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        return Ok(());
    }
    Err(error)
}

#[cfg(not(unix))]
fn terminate_timed_out_child(child: &mut Child) -> std::io::Result<()> {
    child.kill()
}

fn join_output_reader(handle: JoinHandle<Vec<u8>>, stream_name: &str) -> Option<String> {
    match handle.join() {
        Ok(bytes) => Some(String::from_utf8_lossy(&bytes).into_owned()),
        Err(_) => {
            tracing::error!("exec_command: {stream_name} reader thread panicked");
            None
        }
    }
}

fn add_timeout_message(mut stderr: String, exit_code: i32, timeout_ms: u32) -> String {
    if exit_code == COMMAND_TIMEOUT_EXIT_CODE {
        if !stderr.is_empty() {
            stderr.push('\n');
        }
        stderr.push_str(&format!("command timed out after {timeout_ms}ms"));
    }
    stderr
}

fn serialize_command_result(stdout: String, stderr: String, exit_code: i32) -> Option<String> {
    serde_json::to_string(&ShellCommandResult {
        stdout,
        stderr,
        exit_code,
    })
    .map_err(|error| {
        tracing::error!("exec_command: failed to serialize result: {error}");
        error
    })
    .ok()
}

fn read_utf8_file(path: &str) -> Option<String> {
    std::fs::read_to_string(path)
        .map_err(|error| {
            tracing::error!("read_file failed for '{}': {}", path, error);
            error
        })
        .ok()
}

fn write_utf8_file(path: &str, content: &str) -> bool {
    let path = Path::new(path);
    if let Err(error) = ensure_parent_directory(path) {
        tracing::error!(
            "write_file failed to create parent for '{}': {}",
            path.display(),
            error
        );
        return false;
    }
    if let Err(error) = std::fs::write(path, content) {
        tracing::error!("write_file failed for '{}': {}", path.display(), error);
        return false;
    }
    true
}

fn ensure_parent_directory(path: &Path) -> std::io::Result<()> {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => std::fs::create_dir_all(parent),
        _ => Ok(()),
    }
}

fn is_network_allowed(url: &str, capabilities: &[Capability]) -> bool {
    for cap in capabilities {
        match cap {
            Capability::Network => return true,
            Capability::NetworkRestricted { allowed_domains } => {
                if let Some(host) = extract_host(url) {
                    let host_lower = host.to_ascii_lowercase();
                    if allowed_domains.iter().any(|domain| {
                        let domain_lower = domain.to_ascii_lowercase();
                        host_lower == domain_lower
                            || host_lower.ends_with(&format!(".{domain_lower}"))
                    }) {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

fn extract_host(url: &str) -> Option<&str> {
    let start = if url.get(..8)?.eq_ignore_ascii_case("https://") {
        8
    } else if url.get(..7)?.eq_ignore_ascii_case("http://") {
        7
    } else {
        return None;
    };
    let rest = &url[start..];
    let host_end = rest.find('/').unwrap_or(rest.len());
    let host_port = &rest[..host_end];
    let host = host_port.split(':').next().unwrap_or(host_port);
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::collections::HashMap;
    use std::env;
    use std::io::Write;
    use tempfile::TempDir;
    use zeroize::Zeroizing;

    fn make_config(input: &str) -> LiveHostApiConfig<'_> {
        LiveHostApiConfig {
            skill_name: "test_skill",
            input: input.to_string(),
            storage_quota: None,
            capabilities: vec![],
            credential_provider: None,
        }
    }

    const STDIN_HELPER_ENV: &str = "FX_LOADABLE_STDIN_HELPER";

    fn make_api(input: &str) -> LiveHostApi {
        LiveHostApi::new(make_config(input))
    }

    fn make_shell_api() -> LiveHostApi {
        LiveHostApi::new(LiveHostApiConfig {
            skill_name: "test",
            input: String::new(),
            storage_quota: None,
            capabilities: vec![Capability::Shell],
            credential_provider: None,
        })
    }

    fn parse_command_result(json: &str) -> Value {
        serde_json::from_str(json).expect("parse command result")
    }

    fn run_shell_command(command: &str, timeout_ms: u32) -> Value {
        let api = make_shell_api();
        let json = api
            .exec_command(command, timeout_ms)
            .expect("shell command result");
        parse_command_result(&json)
    }

    /// Mock credential provider for testing.
    struct MockCredentialProvider {
        credentials: HashMap<String, String>,
    }

    impl MockCredentialProvider {
        fn new() -> Self {
            Self {
                credentials: HashMap::new(),
            }
        }

        fn with_credential(mut self, key: &str, value: &str) -> Self {
            self.credentials.insert(key.to_string(), value.to_string());
            self
        }
    }

    impl CredentialProvider for MockCredentialProvider {
        fn get_credential(&self, key: &str) -> Option<Zeroizing<String>> {
            self.credentials.get(key).map(|v| Zeroizing::new(v.clone()))
        }
    }

    struct FailAfterLimitReader {
        bytes_remaining: usize,
        chunk_size: usize,
    }

    impl Read for FailAfterLimitReader {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.bytes_remaining == 0 {
                panic!("reader was polled after hitting the output cap");
            }
            let bytes_to_read = self.bytes_remaining.min(self.chunk_size).min(buf.len());
            buf[..bytes_to_read].fill(b'x');
            self.bytes_remaining -= bytes_to_read;
            Ok(bytes_to_read)
        }
    }

    #[cfg(unix)]
    fn read_background_pid(pid_file: &Path) -> i32 {
        let deadline = Instant::now() + Duration::from_millis(500);
        while Instant::now() < deadline {
            if let Ok(pid) = std::fs::read_to_string(pid_file) {
                return pid.trim().parse().expect("background pid");
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!(
            "timed out waiting for background pid at {}",
            pid_file.display()
        );
    }

    #[cfg(unix)]
    fn process_exists(pid: i32) -> bool {
        if unsafe { libc::kill(pid, 0) } == 0 {
            return true;
        }
        std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }

    #[cfg(unix)]
    fn wait_for_process_exit(pid: i32) {
        let deadline = Instant::now() + Duration::from_millis(500);
        while Instant::now() < deadline {
            if !process_exists(pid) {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn input_output_round_trip() {
        let mut api = make_api("hello world");
        assert_eq!(api.get_input(), "hello world");

        api.set_output("response");
        assert_eq!(api.take_output(), "response");
    }

    #[test]
    fn kv_storage_round_trip() {
        let mut api = make_api("");
        assert_eq!(api.kv_get("key"), None);

        api.kv_set("key", "value").expect("should set");
        assert_eq!(api.kv_get("key"), Some("value".to_string()));
    }

    #[test]
    fn kv_storage_respects_quota() {
        let mut api = LiveHostApi::new(LiveHostApiConfig {
            skill_name: "test",
            input: String::new(),
            storage_quota: Some(10),
            capabilities: vec![],
            credential_provider: None,
        });

        // 3 + 3 = 6 bytes, within quota
        api.kv_set("abc", "def").expect("should fit");

        // 3 + 8 = 11, total would be 17 — exceeds 10 byte quota
        let result = api.kv_set("xyz", "12345678");
        assert!(result.is_err());
    }

    #[test]
    fn log_does_not_panic() {
        let api = make_api("");
        for level in 0..=5 {
            api.log(level, "test message");
        }
    }

    #[test]
    fn empty_output_by_default() {
        let api = make_api("input");
        assert_eq!(api.take_output(), "");
    }

    #[test]
    fn output_overwrites_previous() {
        let mut api = make_api("");
        api.set_output("first");
        api.set_output("second");
        assert_eq!(api.take_output(), "second");
    }

    /// Regression test: take_output() drains the string (uses std::mem::take),
    /// leaving an empty string behind. Previously it cloned, which was
    /// misleading given the "take" naming.
    #[test]
    fn take_output_drains_string() {
        let mut api = make_api("");
        api.set_output("hello");
        let first = api.take_output();
        assert_eq!(first, "hello");
        // After taking, the output should be empty
        let second = api.take_output();
        assert_eq!(second, "");
    }

    #[test]
    fn kv_get_bridges_credential_provider() {
        let provider =
            MockCredentialProvider::new().with_credential("github_token", "ghp_test_token_12345");
        let api = LiveHostApi::new(LiveHostApiConfig {
            skill_name: "test",
            input: String::new(),
            storage_quota: None,
            capabilities: vec![],
            credential_provider: Some(Arc::new(provider)),
        });
        assert_eq!(
            api.kv_get("github_token"),
            Some("ghp_test_token_12345".to_string())
        );
    }

    #[test]
    fn kv_get_credential_provider_priority_over_storage() {
        let provider =
            MockCredentialProvider::new().with_credential("github_token", "from_provider");
        let mut api = LiveHostApi::new(LiveHostApiConfig {
            skill_name: "test",
            input: String::new(),
            storage_quota: None,
            capabilities: vec![],
            credential_provider: Some(Arc::new(provider)),
        });
        // Store a value in skill-local storage under the same key
        api.kv_set("github_token", "from_storage")
            .expect("should set");
        // Provider wins
        assert_eq!(
            api.kv_get("github_token"),
            Some("from_provider".to_string())
        );
    }

    #[test]
    fn exec_command_denied_without_shell_capability() {
        let api = make_api("");
        assert_eq!(api.exec_command("printf hello", 1_000), None);
    }

    #[test]
    fn exec_command_allowed_with_shell_capability() {
        let result = run_shell_command("printf hello", 1_000);
        assert_eq!(result["stdout"], "hello");
        assert_eq!(result["stderr"], "");
        assert_eq!(result["exit_code"], 0);
    }

    #[test]
    fn read_capped_output_stops_after_limit() {
        let reader = FailAfterLimitReader {
            bytes_remaining: COMMAND_OUTPUT_LIMIT_BYTES,
            chunk_size: 8192,
        };

        let output = read_capped_output(reader);

        assert_eq!(output.len(), COMMAND_OUTPUT_LIMIT_BYTES);
    }

    #[test]
    #[cfg(unix)]
    fn exec_command_stdin_helper_reads_eof() {
        if env::var_os(STDIN_HELPER_ENV).is_none() {
            return;
        }
        let result = run_shell_command(
            r#"if read value; then printf '%s' "$value"; else printf eof; fi"#,
            1_000,
        );
        assert_eq!(result["stdout"], "eof");
        assert_eq!(result["stderr"], "");
        assert_eq!(result["exit_code"], 0);
    }

    #[test]
    #[cfg(unix)]
    fn exec_command_does_not_inherit_parent_stdin() {
        let exe = env::current_exe().expect("test binary path");
        let mut child = Command::new(exe)
            .arg("--exact")
            .arg("exec_command_stdin_helper_reads_eof")
            .env(STDIN_HELPER_ENV, "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn stdin helper");
        child
            .stdin
            .take()
            .expect("helper stdin")
            .write_all(b"from-parent\n")
            .expect("write helper stdin");
        let output = child.wait_with_output().expect("helper output");
        assert!(
            output.status.success(),
            "stdin helper failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    #[cfg(unix)]
    fn exec_command_timeout_kills_background_process_group() {
        let temp_dir = TempDir::new().expect("temp dir");
        let pid_file = temp_dir.path().join("background.pid");
        let command = format!("sleep 1 & echo $! > '{}' && wait", pid_file.display());
        let started_at = Instant::now();

        let result = run_shell_command(&command, 50);
        let elapsed = started_at.elapsed();
        let background_pid = read_background_pid(&pid_file);
        wait_for_process_exit(background_pid);
        if process_exists(background_pid) {
            let _ = unsafe { libc::kill(background_pid, libc::SIGKILL) };
        }

        assert_eq!(result["exit_code"], COMMAND_TIMEOUT_EXIT_CODE);
        assert!(elapsed < Duration::from_millis(500));
        assert!(!process_exists(background_pid));
    }

    #[test]
    fn read_file_denied_without_filesystem_capability() {
        let temp_dir = TempDir::new().expect("temp dir");
        let path = temp_dir.path().join("data.txt");
        std::fs::write(&path, "hello").expect("write fixture");
        let api = make_api("");

        assert_eq!(api.read_file(path.to_str().expect("utf-8 path")), None);
    }

    #[test]
    fn write_file_denied_without_filesystem_capability() {
        let temp_dir = TempDir::new().expect("temp dir");
        let path = temp_dir.path().join("data.txt");
        let api = make_api("");

        assert!(!api.write_file(path.to_str().expect("utf-8 path"), "hello"));
        assert!(!path.exists());
    }

    #[test]
    fn read_write_file_round_trip() {
        let temp_dir = TempDir::new().expect("temp dir");
        let path = temp_dir.path().join("nested").join("data.txt");
        let api = LiveHostApi::new(LiveHostApiConfig {
            skill_name: "test",
            input: String::new(),
            storage_quota: None,
            capabilities: vec![Capability::Filesystem],
            credential_provider: None,
        });

        assert!(api.write_file(path.to_str().expect("utf-8 path"), "hello"));
        assert_eq!(
            api.read_file(path.to_str().expect("utf-8 path")),
            Some("hello".to_string())
        );
    }

    #[test]
    fn network_allowed_unrestricted() {
        assert!(is_network_allowed(
            "https://anything.com",
            &[Capability::Network]
        ));
    }

    #[test]
    fn network_allowed_exact_domain() {
        let caps = vec![Capability::NetworkRestricted {
            allowed_domains: vec!["api.weather.gov".into()],
        }];
        assert!(is_network_allowed("https://api.weather.gov/points", &caps));
    }

    #[test]
    fn network_denied_wrong_domain() {
        let caps = vec![Capability::NetworkRestricted {
            allowed_domains: vec!["api.weather.gov".into()],
        }];
        assert!(!is_network_allowed("https://evil.com/exfil", &caps));
    }

    #[test]
    fn network_allowed_subdomain() {
        let caps = vec![Capability::NetworkRestricted {
            allowed_domains: vec!["weather.gov".into()],
        }];
        assert!(is_network_allowed("https://api.weather.gov/points", &caps));
    }

    #[test]
    fn network_denied_partial_suffix() {
        let caps = vec![Capability::NetworkRestricted {
            allowed_domains: vec!["weather.gov".into()],
        }];
        assert!(!is_network_allowed("https://badweather.gov/attack", &caps));
    }

    #[test]
    fn network_denied_no_capability() {
        assert!(!is_network_allowed(
            "https://anything.com",
            &[Capability::Storage]
        ));
    }

    #[test]
    fn network_denied_empty_caps() {
        assert!(!is_network_allowed("https://anything.com", &[]));
    }

    #[test]
    fn network_allowed_case_insensitive() {
        let caps = vec![Capability::NetworkRestricted {
            allowed_domains: vec!["WEATHER.GOV".into()],
        }];
        assert!(is_network_allowed("https://Api.Weather.Gov/points", &caps));
    }

    #[test]
    fn extract_host_https() {
        assert_eq!(
            extract_host("https://api.weather.gov/foo"),
            Some("api.weather.gov")
        );
    }

    #[test]
    fn extract_host_with_port() {
        assert_eq!(
            extract_host("https://localhost:8080/path"),
            Some("localhost")
        );
    }

    #[test]
    fn extract_host_http() {
        assert_eq!(extract_host("http://example.com/path"), Some("example.com"));
    }

    #[test]
    fn extract_host_no_scheme() {
        assert_eq!(extract_host("ftp://example.com"), None);
    }

    #[test]
    fn extract_host_empty() {
        assert_eq!(extract_host(""), None);
    }

    #[test]
    fn extract_host_uppercase_scheme() {
        assert_eq!(
            extract_host("HTTPS://api.weather.gov/foo"),
            Some("api.weather.gov")
        );
    }

    #[test]
    fn http_request_denied_without_network_capability() {
        let api = make_api("");
        // No capabilities → denied
        let result = api.http_request("GET", "https://example.com", "", "");
        assert!(result.is_none());
    }

    /// Verifies that with Network capability, the request passes capability
    /// gating and reaches HTTPS enforcement. Using `http://` (not `https://`)
    /// triggers the HTTPS-only rejection in `execute_http_request`, proving
    /// the request was NOT short-circuited by capability denial.
    #[test]
    fn http_request_requires_https_when_capable() {
        let api = LiveHostApi::new(LiveHostApiConfig {
            skill_name: "test",
            input: String::new(),
            storage_quota: None,
            capabilities: vec![Capability::Network],
            credential_provider: None,
        });
        // Capability check passes, but HTTPS enforcement rejects http://
        let result = api.http_request("GET", "http://example.com", "{}", "");
        assert_eq!(result, None);
    }
}
