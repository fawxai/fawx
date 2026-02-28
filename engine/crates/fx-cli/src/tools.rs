use async_trait::async_trait;
use fx_kernel::act::{ToolExecutor, ToolExecutorError, ToolResult};
use fx_llm::{ToolCall, ToolDefinition};
use serde::Deserialize;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::process::Command;

const MAX_RECURSION_DEPTH: usize = 5;
const MAX_SEARCH_MATCHES: usize = 100;
const DEFAULT_MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;
const DEFAULT_COMMAND_TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Clone)]
pub struct FawxToolExecutor {
    working_dir: PathBuf,
    config: ToolConfig,
}

#[derive(Debug, Clone)]
pub struct ToolConfig {
    /// Maximum file size for read/write operations (bytes)
    pub max_file_size: u64,
    /// Command execution timeout
    pub command_timeout: Duration,
    /// Whether to allow commands outside working_dir
    pub jail_to_working_dir: bool,
}

impl Default for ToolConfig {
    fn default() -> Self {
        Self {
            max_file_size: DEFAULT_MAX_FILE_SIZE,
            command_timeout: Duration::from_secs(DEFAULT_COMMAND_TIMEOUT_SECS),
            jail_to_working_dir: true,
        }
    }
}

impl FawxToolExecutor {
    pub fn new(working_dir: PathBuf, config: ToolConfig) -> Self {
        Self {
            working_dir,
            config,
        }
    }

    async fn execute_call(&self, call: &ToolCall) -> ToolResult {
        let output = match call.name.as_str() {
            "read_file" => self.handle_read_file(&call.arguments),
            "write_file" => self.handle_write_file(&call.arguments),
            "list_directory" => self.handle_list_directory(&call.arguments),
            "run_command" => self.handle_run_command(&call.arguments).await,
            "search_text" => self.handle_search_text(&call.arguments),
            "current_time" => self.handle_current_time(),
            _ => Err(format!("unknown tool: {}", call.name)),
        };
        to_tool_result(&call.name, output)
    }

    fn jailed_path(&self, requested: &str) -> Result<PathBuf, String> {
        if !self.config.jail_to_working_dir {
            return canonicalize_existing_or_parent(Path::new(requested));
        }
        validate_path(&self.working_dir, requested)
    }

    fn validated_existing_entry(&self, path: &Path) -> Result<Option<PathBuf>, String> {
        if !self.config.jail_to_working_dir {
            return Ok(Some(path.to_path_buf()));
        }
        let requested = path.to_string_lossy().to_string();
        match validate_path(&self.working_dir, &requested) {
            Ok(validated) => Ok(Some(validated)),
            Err(_) => Ok(None),
        }
    }

    fn handle_read_file(&self, args: &serde_json::Value) -> Result<String, String> {
        let parsed: ReadFileArgs = parse_args(args)?;
        let path = self.jailed_path(&parsed.path)?;
        let metadata = fs::metadata(&path).map_err(|error| error.to_string())?;
        if metadata.len() > self.config.max_file_size {
            return Err("file exceeds maximum allowed size".to_string());
        }
        let bytes = fs::read(&path).map_err(|error| error.to_string())?;
        String::from_utf8(bytes).map_err(|_| "file appears to be binary".to_string())
    }

    fn handle_write_file(&self, args: &serde_json::Value) -> Result<String, String> {
        let parsed: WriteFileArgs = parse_args(args)?;
        let len = parsed.content.len() as u64;
        if len > self.config.max_file_size {
            return Err("content exceeds maximum allowed size".to_string());
        }
        let path = self.jailed_path(&parsed.path)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        fs::write(&path, parsed.content.as_bytes()).map_err(|error| error.to_string())?;
        Ok(format!("wrote {} bytes to {}", len, path.display()))
    }

    fn handle_list_directory(&self, args: &serde_json::Value) -> Result<String, String> {
        let parsed: ListDirectoryArgs = parse_args(args)?;
        let path = self.jailed_path(&parsed.path)?;
        let recursive = parsed.recursive.unwrap_or(false);
        if recursive {
            return self.list_recursive(&path, 0);
        }
        self.list_flat(&path)
    }

    fn list_flat(&self, path: &Path) -> Result<String, String> {
        let mut lines = Vec::new();
        for entry in fs::read_dir(path).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let kind = entry_kind(&entry.path())?;
            lines.push(format!("[{kind}] {}", entry.file_name().to_string_lossy()));
        }
        lines.sort();
        Ok(lines.join("\n"))
    }

    fn list_recursive(&self, path: &Path, depth: usize) -> Result<String, String> {
        if depth > MAX_RECURSION_DEPTH {
            return Ok(String::new());
        }
        let mut lines = Vec::new();
        for entry in fs::read_dir(path).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let entry_path = entry.path();

            if let Some(name) = entry_path.file_name().and_then(|n| n.to_str()) {
                if is_ignored_directory(name) && entry_path.is_dir() {
                    continue;
                }
            }

            let Some(validated) = self.validated_existing_entry(&entry_path)? else {
                continue;
            };
            let name = entry.file_name().to_string_lossy().to_string();
            let kind = entry_kind(&entry_path)?;
            lines.push(format!("{}[{}] {}", "  ".repeat(depth), kind, name));
            if kind == "dir" {
                let nested = self.list_recursive(&validated, depth + 1)?;
                if !nested.is_empty() {
                    lines.push(nested);
                }
            }
        }
        Ok(lines.join("\n"))
    }

    async fn handle_run_command(&self, args: &serde_json::Value) -> Result<String, String> {
        let parsed: RunCommandArgs = parse_args(args)?;
        let command = parsed.command.trim();
        if command.is_empty() {
            return Err("command cannot be empty".to_string());
        }
        let working_dir = self.resolve_command_dir(parsed.working_dir.as_deref())?;
        let child = build_command(command, parsed.shell.unwrap_or(false), &working_dir)?
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| error.to_string())?;
        let output = wait_with_timeout(child, self.config.command_timeout).await?;
        Ok(format_command_output(output, parsed.shell.unwrap_or(false)))
    }

    fn resolve_command_dir(&self, requested: Option<&str>) -> Result<PathBuf, String> {
        let desired = requested.unwrap_or_else(|| self.working_dir.to_str().unwrap_or("."));
        if !self.config.jail_to_working_dir {
            return canonicalize_existing_or_parent(Path::new(desired));
        }
        validate_path(&self.working_dir, desired)
    }

    fn handle_search_text(&self, args: &serde_json::Value) -> Result<String, String> {
        let parsed: SearchTextArgs = parse_args(args)?;
        let root = self.resolve_search_root(parsed.path.as_deref())?;
        let mut results = Vec::new();
        self.search_path(&root, &parsed, &mut results)?;
        Ok(results.join("\n"))
    }

    fn handle_current_time(&self) -> Result<String, String> {
        let now = SystemTime::now();
        let duration = now
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("system time before Unix epoch: {error}"))?;
        let epoch = duration.as_secs();
        let iso = iso8601_utc_from_epoch(epoch);
        let day_of_week = day_of_week_from_epoch(epoch);
        Ok(format!(
            "iso8601_utc: {iso}\nepoch: {epoch}\nday_of_week: {day_of_week}"
        ))
    }

    fn resolve_search_root(&self, requested: Option<&str>) -> Result<PathBuf, String> {
        let default_root = self.working_dir.to_string_lossy().to_string();
        let requested = requested.unwrap_or(&default_root);
        if !self.config.jail_to_working_dir {
            return canonicalize_existing_or_parent(Path::new(requested));
        }
        validate_path(&self.working_dir, requested)
    }

    fn search_path(
        &self,
        root: &Path,
        args: &SearchTextArgs,
        out: &mut Vec<String>,
    ) -> Result<(), String> {
        if out.len() >= MAX_SEARCH_MATCHES {
            return Ok(());
        }
        if root.is_dir() {
            self.search_directory(root, args, out)?;
        } else {
            self.search_file(root, args, out)?;
        }
        Ok(())
    }

    fn search_directory(
        &self,
        dir: &Path,
        args: &SearchTextArgs,
        out: &mut Vec<String>,
    ) -> Result<(), String> {
        for entry in fs::read_dir(dir).map_err(|error| error.to_string())? {
            if out.len() >= MAX_SEARCH_MATCHES {
                break;
            }
            let entry_path = entry.map_err(|error| error.to_string())?.path();

            // Skip build artifacts, VCS, and dependency directories
            if let Some(name) = entry_path.file_name().and_then(|n| n.to_str()) {
                if is_ignored_directory(name) && entry_path.is_dir() {
                    continue;
                }
            }

            let Some(validated) = self.validated_existing_entry(&entry_path)? else {
                continue;
            };
            if validated.is_dir() {
                self.search_directory(&validated, args, out)?;
                continue;
            }
            self.search_file(&validated, args, out)?;
        }
        Ok(())
    }

    fn search_file(
        &self,
        file: &Path,
        args: &SearchTextArgs,
        out: &mut Vec<String>,
    ) -> Result<(), String> {
        if !matches_glob(file, args.file_glob.as_deref()) {
            return Ok(());
        }
        let metadata = fs::metadata(file).map_err(|error| error.to_string())?;
        if metadata.len() > self.config.max_file_size {
            return Ok(());
        }
        let mut bytes = Vec::new();
        let mut reader = fs::File::open(file).map_err(|error| error.to_string())?;
        reader
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        let text = match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(_) => return Ok(()),
        };
        for (index, line) in text.lines().enumerate() {
            if out.len() >= MAX_SEARCH_MATCHES {
                break;
            }
            if line.contains(&args.pattern) {
                out.push(format!("{}:{}:{}", file.display(), index + 1, line));
            }
        }
        Ok(())
    }
}

#[async_trait]
impl ToolExecutor for FawxToolExecutor {
    async fn execute_tools(
        &self,
        calls: &[ToolCall],
    ) -> Result<Vec<ToolResult>, ToolExecutorError> {
        let mut results = Vec::with_capacity(calls.len());
        for call in calls {
            results.push(self.execute_call(call).await);
        }
        Ok(results)
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        fawx_tool_definitions()
    }
}

pub fn fawx_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "read_file".to_string(),
            description: "Read a UTF-8 text file from disk".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"]
            }),
        },
        ToolDefinition {
            name: "write_file".to_string(),
            description: "Write UTF-8 content to a file on disk".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["path", "content"]
            }),
        },
        ToolDefinition {
            name: "list_directory".to_string(),
            description: "List files and directories, optionally recursively".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "recursive": { "type": "boolean" }
                },
                "required": ["path"]
            }),
        },
        ToolDefinition {
            name: "run_command".to_string(),
            description: "Run a command and capture exit code, stdout, and stderr".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" },
                    "working_dir": { "type": "string" },
                    "shell": { "type": "boolean" }
                },
                "required": ["command"]
            }),
        },
        ToolDefinition {
            name: "search_text".to_string(),
            description: "Search text in files and return file:line matches".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string" },
                    "path": { "type": "string" },
                    "file_glob": { "type": "string" }
                },
                "required": ["pattern"]
            }),
        },
        ToolDefinition {
            name: "current_time".to_string(),
            description: "Get the current date, time, timezone, and Unix epoch timestamp"
                .to_string(),
            parameters: serde_json::json!({"type": "object", "properties": {}, "required": []}),
        },
    ]
}

pub fn validate_path(base: &Path, requested: &str) -> Result<PathBuf, String> {
    // NOTE: There is an unavoidable TOCTOU window between this validation and later
    // open/read/write calls that operate by path. Tightening this fully requires
    // fd-based operations end-to-end, which is not currently practical across all tools.
    let base_canon = fs::canonicalize(base).map_err(|error| error.to_string())?;
    let candidate = resolve_candidate(&base_canon, requested);
    let requested_canon = canonicalize_existing_or_parent(&candidate)?;
    if requested_canon.starts_with(&base_canon) {
        return Ok(requested_canon);
    }
    Err("path escapes working directory".to_string())
}

fn resolve_candidate(base: &Path, requested: &str) -> PathBuf {
    let requested_path = Path::new(requested);
    if requested_path.is_absolute() {
        requested_path.to_path_buf()
    } else {
        base.join(requested_path)
    }
}

fn canonicalize_existing_or_parent(path: &Path) -> Result<PathBuf, String> {
    if path.exists() {
        return fs::canonicalize(path).map_err(|error| error.to_string());
    }

    let mut missing_parts = Vec::new();
    let mut cursor = path;
    while !cursor.exists() {
        let name = cursor
            .file_name()
            .ok_or_else(|| "invalid target path".to_string())?;
        missing_parts.push(name.to_os_string());
        cursor = cursor
            .parent()
            .ok_or_else(|| "invalid target path".to_string())?;
    }

    let mut resolved = fs::canonicalize(cursor).map_err(|error| error.to_string())?;
    while let Some(part) = missing_parts.pop() {
        resolved.push(part);
    }
    Ok(resolved)
}

fn to_tool_result(tool_name: &str, output: Result<String, String>) -> ToolResult {
    match output {
        Ok(content) => ToolResult {
            tool_name: tool_name.to_string(),
            success: true,
            output: content,
        },
        Err(error) => ToolResult {
            tool_name: tool_name.to_string(),
            success: false,
            output: error,
        },
    }
}

fn entry_kind(path: &Path) -> Result<&'static str, String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    let kind = if metadata.file_type().is_dir() {
        "dir"
    } else if metadata.file_type().is_symlink() {
        "symlink"
    } else {
        "file"
    };
    Ok(kind)
}

fn build_command(command: &str, shell: bool, working_dir: &Path) -> Result<Command, String> {
    if shell {
        let mut built = Command::new("/bin/sh");
        built.kill_on_drop(true);
        built.arg("-c").arg(command).current_dir(working_dir);
        return Ok(built);
    }
    let mut parts = command.split_whitespace();
    let program = parts
        .next()
        .ok_or_else(|| "command cannot be empty".to_string())?;
    let mut built = Command::new(program);
    built.kill_on_drop(true);
    built.args(parts).current_dir(working_dir);
    Ok(built)
}

async fn wait_with_timeout(
    child: tokio::process::Child,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    let waited = tokio::time::timeout(timeout, child.wait_with_output()).await;
    match waited {
        Ok(result) => result.map_err(|error| error.to_string()),
        Err(_) => Err("command timed out".to_string()),
    }
}

fn format_command_output(output: std::process::Output, shell: bool) -> String {
    let mut lines = vec![format!("exit_code: {}", output.status.code().unwrap_or(-1))];
    if shell {
        lines.push("warning: command executed via shell=true".to_string());
    }
    lines.push(format!(
        "stdout:\n{}",
        String::from_utf8_lossy(&output.stdout)
    ));
    lines.push(format!(
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    ));
    lines.join("\n")
}

fn matches_glob(path: &Path, file_glob: Option<&str>) -> bool {
    let Some(pattern) = file_glob else {
        return true;
    };
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    simple_glob_match(name, pattern)
}

/// Directories that should never be searched — build artifacts, VCS, dependencies.
fn is_ignored_directory(name: &str) -> bool {
    matches!(
        name,
        "target"
            | ".git"
            | "node_modules"
            | ".build"
            | "build"
            | ".gradle"
            | "__pycache__"
            | ".mypy_cache"
            | ".pytest_cache"
            | "dist"
            | ".next"
            | ".turbo"
    )
}

fn simple_glob_match(name: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if !pattern.contains('*') {
        return name == pattern;
    }
    let parts = pattern.split('*').collect::<Vec<_>>();
    if parts.len() == 2 {
        return name.starts_with(parts[0]) && name.ends_with(parts[1]);
    }
    name.contains(&pattern.replace('*', ""))
}

fn day_of_week_from_epoch(epoch: u64) -> &'static str {
    let days_since_epoch = (epoch / 86_400) as i64;
    let weekday_index = (days_since_epoch + 4).rem_euclid(7);
    match weekday_index {
        0 => "Sunday",
        1 => "Monday",
        2 => "Tuesday",
        3 => "Wednesday",
        4 => "Thursday",
        5 => "Friday",
        _ => "Saturday",
    }
}

fn iso8601_utc_from_epoch(epoch: u64) -> String {
    let days_since_epoch = (epoch / 86_400) as i64;
    let seconds_of_day = epoch % 86_400;
    let (year, month, day) = civil_from_days(days_since_epoch);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }
    (year, month as u32, day as u32)
}

fn parse_args<T: for<'de> Deserialize<'de>>(value: &serde_json::Value) -> Result<T, String> {
    serde_json::from_value(value.clone()).map_err(|error| error.to_string())
}

#[derive(Deserialize)]
struct ReadFileArgs {
    path: String,
}

#[derive(Deserialize)]
struct WriteFileArgs {
    path: String,
    content: String,
}

#[derive(Deserialize)]
struct ListDirectoryArgs {
    path: String,
    recursive: Option<bool>,
}

#[derive(Deserialize)]
struct RunCommandArgs {
    command: String,
    working_dir: Option<String>,
    shell: Option<bool>,
}

#[derive(Deserialize)]
struct SearchTextArgs {
    pattern: String,
    path: Option<String>,
    file_glob: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_executor(root: &Path) -> FawxToolExecutor {
        FawxToolExecutor::new(root.to_path_buf(), ToolConfig::default())
    }

    #[test]
    fn validate_path_accepts_path_within_jail() {
        let temp = TempDir::new().expect("tempdir");
        let result = validate_path(temp.path(), "inside.txt");
        assert!(result.is_ok());
    }

    #[test]
    fn validate_path_rejects_traversal_escape() {
        let temp = TempDir::new().expect("tempdir");
        let result = validate_path(temp.path(), "../../etc/passwd");
        assert!(result.is_err());
    }

    #[cfg(unix)]
    #[test]
    fn validate_path_rejects_symlink_escape() {
        use std::os::unix::fs::symlink;

        let jail = TempDir::new().expect("jail");
        let outside = TempDir::new().expect("outside");
        let link_path = jail.path().join("link");
        symlink(outside.path(), &link_path).expect("symlink");

        let result = validate_path(jail.path(), "link/secrets.txt");
        assert!(result.is_err());
    }

    #[test]
    fn validate_path_rejects_absolute_path_outside() {
        let jail = TempDir::new().expect("jail");
        let outside = TempDir::new().expect("outside");
        let result = validate_path(jail.path(), &outside.path().to_string_lossy());
        assert!(result.is_err());
    }

    #[test]
    fn validate_path_accepts_jail_boundary() {
        let jail = TempDir::new().expect("jail");
        let result = validate_path(jail.path(), ".");
        assert!(result.is_ok());
    }

    #[test]
    fn read_file_reads_existing_file() {
        let temp = TempDir::new().expect("temp");
        let file = temp.path().join("a.txt");
        fs::write(&file, "hello").expect("write");
        let executor = test_executor(temp.path());

        let output = executor.handle_read_file(&serde_json::json!({"path": "a.txt"}));
        assert_eq!(output.expect("read"), "hello");
    }

    #[test]
    fn read_file_reports_missing_file() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let output = executor.handle_read_file(&serde_json::json!({"path": "missing.txt"}));
        assert!(output.is_err());
    }

    #[test]
    fn read_file_rejects_oversized_file() {
        let temp = TempDir::new().expect("temp");
        let file = temp.path().join("big.txt");
        fs::write(&file, "0123456789").expect("write");
        let executor = FawxToolExecutor::new(
            temp.path().to_path_buf(),
            ToolConfig {
                max_file_size: 4,
                ..ToolConfig::default()
            },
        );
        let output = executor.handle_read_file(&serde_json::json!({"path": "big.txt"}));
        assert!(output.is_err());
    }

    #[test]
    fn read_file_rejects_binary_file() {
        let temp = TempDir::new().expect("temp");
        let file = temp.path().join("bin.dat");
        fs::write(&file, [0, 159, 146, 150]).expect("write");
        let executor = test_executor(temp.path());
        let output = executor.handle_read_file(&serde_json::json!({"path": "bin.dat"}));
        assert!(matches!(output, Err(message) if message.contains("binary")));
    }

    #[test]
    fn read_file_rejects_outside_jail() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let output = executor.handle_read_file(&serde_json::json!({"path": "../escape.txt"}));
        assert!(output.is_err());
    }

    #[cfg(unix)]
    #[test]
    fn read_file_rejects_symlink_pointing_outside_jail() {
        use std::os::unix::fs::symlink;

        let jail = TempDir::new().expect("jail");
        let outside = TempDir::new().expect("outside");
        let outside_file = outside.path().join("secret.txt");
        fs::write(&outside_file, "secret").expect("write");
        symlink(&outside_file, jail.path().join("escape.txt")).expect("symlink");

        let executor = test_executor(jail.path());
        let output = executor.handle_read_file(&serde_json::json!({"path": "escape.txt"}));
        assert!(output.is_err());
    }

    #[test]
    fn write_file_creates_file_with_content() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());

        let result =
            executor.handle_write_file(&serde_json::json!({"path": "new.txt", "content": "hello"}));
        assert!(result.is_ok());
        assert_eq!(
            fs::read_to_string(temp.path().join("new.txt")).expect("read"),
            "hello"
        );
    }

    #[test]
    fn write_file_creates_parent_directories() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let result = executor
            .handle_write_file(&serde_json::json!({"path": "a/b/c.txt", "content": "nested"}));
        assert!(result.is_ok());
        assert_eq!(
            fs::read_to_string(temp.path().join("a/b/c.txt")).expect("read"),
            "nested"
        );
    }

    #[test]
    fn write_file_rejects_oversized_content() {
        let temp = TempDir::new().expect("temp");
        let executor = FawxToolExecutor::new(
            temp.path().to_path_buf(),
            ToolConfig {
                max_file_size: 3,
                ..ToolConfig::default()
            },
        );
        let result =
            executor.handle_write_file(&serde_json::json!({"path": "x.txt", "content": "hello"}));
        assert!(result.is_err());
    }

    #[test]
    fn write_file_rejects_outside_jail() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let result =
            executor.handle_write_file(&serde_json::json!({"path": "../x.txt", "content": "no"}));
        assert!(result.is_err());
    }

    #[test]
    fn list_directory_returns_entries_with_types() {
        let temp = TempDir::new().expect("temp");
        fs::create_dir(temp.path().join("d")).expect("mkdir");
        fs::write(temp.path().join("f.txt"), "x").expect("write");
        let executor = test_executor(temp.path());

        let output = executor
            .handle_list_directory(&serde_json::json!({"path": "."}))
            .expect("list");
        assert!(output.contains("[dir] d"));
        assert!(output.contains("[file] f.txt"));
    }

    #[test]
    fn list_directory_rejects_missing_directory() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let output = executor.handle_list_directory(&serde_json::json!({"path": "missing"}));
        assert!(output.is_err());
    }

    #[test]
    fn list_directory_rejects_outside_jail() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let output = executor.handle_list_directory(&serde_json::json!({"path": "../"}));
        assert!(output.is_err());
    }

    #[test]
    fn list_directory_recursive_honors_depth_limit() {
        let temp = TempDir::new().expect("temp");
        let mut current = temp.path().to_path_buf();
        for depth in 0..8 {
            current = current.join(format!("d{depth}"));
            fs::create_dir_all(&current).expect("mkdir");
            fs::write(current.join("f.txt"), "x").expect("write");
        }
        let executor = test_executor(temp.path());
        let output = executor
            .handle_list_directory(&serde_json::json!({"path": ".", "recursive": true}))
            .expect("recursive list");
        assert!(output.contains("d0"));
        assert!(!output.contains("d7"));
    }

    #[cfg(unix)]
    #[test]
    fn list_directory_recursive_skips_symlink_escape() {
        use std::os::unix::fs::symlink;

        let jail = TempDir::new().expect("jail");
        let outside = TempDir::new().expect("outside");
        let outside_dir = outside.path().join("secret-dir");
        fs::create_dir_all(&outside_dir).expect("mkdir");
        fs::write(outside_dir.join("secret.txt"), "secret").expect("write");
        symlink(&outside_dir, jail.path().join("escape")).expect("symlink");

        let executor = test_executor(jail.path());
        let output = executor
            .handle_list_directory(&serde_json::json!({"path": ".", "recursive": true}))
            .expect("recursive list");
        assert!(!output.contains("escape"));
        assert!(!output.contains("secret.txt"));
    }

    #[tokio::test]
    async fn run_command_captures_stdout() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let output = executor
            .handle_run_command(&serde_json::json!({"command": "echo hello"}))
            .await
            .expect("command");
        assert!(output.contains("exit_code: 0"));
        assert!(output.contains("hello"));
    }

    #[tokio::test]
    async fn run_command_captures_nonzero_exit_code() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let output = executor
            .handle_run_command(&serde_json::json!({"command": "false"}))
            .await
            .expect("command");
        assert!(output.contains("exit_code: 1"));
    }

    #[tokio::test]
    async fn run_command_times_out() {
        let temp = TempDir::new().expect("temp");
        let executor = FawxToolExecutor::new(
            temp.path().to_path_buf(),
            ToolConfig {
                command_timeout: Duration::from_millis(1),
                ..ToolConfig::default()
            },
        );
        let output = executor
            .handle_run_command(&serde_json::json!({"command": "sleep 1"}))
            .await;
        assert!(matches!(output, Err(message) if message.contains("timed out")));
    }

    #[tokio::test]
    async fn run_command_validates_working_directory_override() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let output = executor
            .handle_run_command(&serde_json::json!({"command": "echo hi", "working_dir": "../"}))
            .await;
        assert!(output.is_err());
    }

    #[test]
    fn search_text_finds_pattern_with_file_and_line() {
        let temp = TempDir::new().expect("temp");
        fs::write(temp.path().join("a.txt"), "first\nneedle\nthird").expect("write");
        let executor = test_executor(temp.path());
        let output = executor
            .handle_search_text(&serde_json::json!({"pattern": "needle"}))
            .expect("search");
        assert!(output.contains("a.txt:2:needle"));
    }

    #[test]
    fn search_text_returns_empty_when_not_found() {
        let temp = TempDir::new().expect("temp");
        fs::write(temp.path().join("a.txt"), "first").expect("write");
        let executor = test_executor(temp.path());
        let output = executor
            .handle_search_text(&serde_json::json!({"pattern": "needle"}))
            .expect("search");
        assert_eq!(output, "");
    }

    #[test]
    fn search_text_limits_results_to_max_matches() {
        let temp = TempDir::new().expect("temp");
        let mut content = String::new();
        for _ in 0..150 {
            content.push_str("needle\n");
        }
        fs::write(temp.path().join("a.txt"), content).expect("write");
        let executor = test_executor(temp.path());
        let output = executor
            .handle_search_text(&serde_json::json!({"pattern": "needle"}))
            .expect("search");
        assert_eq!(output.lines().count(), MAX_SEARCH_MATCHES);
    }

    #[test]
    fn search_text_rejects_outside_jail() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let output =
            executor.handle_search_text(&serde_json::json!({"pattern": "needle", "path": "../"}));
        assert!(output.is_err());
    }

    #[cfg(unix)]
    #[test]
    fn search_text_recursive_skips_symlink_escape() {
        use std::os::unix::fs::symlink;

        let jail = TempDir::new().expect("jail");
        let outside = TempDir::new().expect("outside");
        let outside_file = outside.path().join("secret.txt");
        fs::write(&outside_file, "needle").expect("write");
        symlink(&outside_file, jail.path().join("escape.txt")).expect("symlink");

        let executor = test_executor(jail.path());
        let output = executor
            .handle_search_text(&serde_json::json!({"pattern": "needle", "path": "."}))
            .expect("search");
        assert!(output.is_empty());
    }

    #[test]
    fn search_text_skips_target_directory() {
        let dir = TempDir::new().expect("tempdir");
        let target_dir = dir.path().join("target").join("debug");
        fs::create_dir_all(&target_dir).expect("mkdir target");
        fs::write(target_dir.join("foo.rs"), "needle in target").expect("write target");
        fs::write(dir.path().join("src.rs"), "needle in source").expect("write source");

        let executor = FawxToolExecutor::new(dir.path().to_path_buf(), ToolConfig::default());
        let result = executor
            .handle_search_text(&serde_json::json!({"pattern": "needle"}))
            .expect("search");

        assert!(result.contains("src.rs"), "should find needle in source");
        assert!(!result.contains("target"), "should skip target directory");
    }

    #[test]
    fn search_text_skips_git_directory() {
        let dir = TempDir::new().expect("tempdir");
        let git_dir = dir.path().join(".git").join("objects");
        fs::create_dir_all(&git_dir).expect("mkdir git");
        fs::write(git_dir.join("pack"), "needle in git").expect("write git");
        fs::write(dir.path().join("main.rs"), "needle in main").expect("write main");

        let executor = FawxToolExecutor::new(dir.path().to_path_buf(), ToolConfig::default());
        let result = executor
            .handle_search_text(&serde_json::json!({"pattern": "needle"}))
            .expect("search");

        assert!(result.contains("main.rs"), "should find needle in main");
        assert!(!result.contains(".git"), "should skip .git directory");
    }

    #[test]
    fn search_text_does_not_skip_file_named_target() {
        let dir = TempDir::new().expect("tempdir");
        fs::write(dir.path().join("target"), "needle in file named target")
            .expect("write target file");

        let executor = FawxToolExecutor::new(dir.path().to_path_buf(), ToolConfig::default());
        let result = executor
            .handle_search_text(&serde_json::json!({"pattern": "needle"}))
            .expect("search");

        assert!(result.contains("target"), "should search file named target");
    }

    #[test]
    fn search_text_skips_node_modules() {
        let dir = TempDir::new().expect("tempdir");
        let nm_dir = dir.path().join("node_modules").join("lodash");
        fs::create_dir_all(&nm_dir).expect("mkdir node_modules");
        fs::write(nm_dir.join("index.js"), "needle in node_modules").expect("write node_modules");
        fs::write(dir.path().join("app.rs"), "needle in app").expect("write app");

        let executor = FawxToolExecutor::new(dir.path().to_path_buf(), ToolConfig::default());
        let result = executor
            .handle_search_text(&serde_json::json!({"pattern": "needle"}))
            .expect("search");

        assert!(result.contains("app.rs"), "should find needle in app");
        assert!(!result.contains("node_modules"), "should skip node_modules");
    }

    #[test]
    fn is_ignored_directory_covers_known_dirs() {
        assert!(is_ignored_directory("target"));
        assert!(is_ignored_directory(".git"));
        assert!(is_ignored_directory("node_modules"));
        assert!(is_ignored_directory(".build"));
        assert!(!is_ignored_directory("src"));
        assert!(!is_ignored_directory("engine"));
        assert!(!is_ignored_directory("docs"));
    }

    #[test]
    fn search_text_ignores_files_over_max_file_size() {
        let temp = TempDir::new().expect("temp");
        fs::write(temp.path().join("big.txt"), "needle\nneedle").expect("write");
        let executor = FawxToolExecutor::new(
            temp.path().to_path_buf(),
            ToolConfig {
                max_file_size: 4,
                ..ToolConfig::default()
            },
        );

        let output = executor
            .handle_search_text(&serde_json::json!({"pattern": "needle"}))
            .expect("search");
        assert!(output.is_empty());
    }

    #[test]
    fn search_text_finds_nested_rust_and_markdown_when_large_files_exist() {
        let temp = TempDir::new().expect("temp");
        for index in 0..MAX_SEARCH_MATCHES {
            let path = temp.path().join(format!("large-{index}.bin"));
            fs::write(path, "x".repeat(64)).expect("write");
        }

        let nested = temp.path().join("engine/crates/fx-foo/src");
        fs::create_dir_all(&nested).expect("mkdir");
        let rust_file = nested.join("lib.rs");
        let markdown_file = temp.path().join("DOCTRINE.md");
        fs::write(&rust_file, "pub struct ToolExecutor;\n").expect("write");
        fs::write(&markdown_file, "ToolExecutor reference\n").expect("write");

        let executor = FawxToolExecutor::new(
            temp.path().to_path_buf(),
            ToolConfig {
                max_file_size: 32,
                ..ToolConfig::default()
            },
        );

        let output = executor
            .handle_search_text(&serde_json::json!({"pattern": "ToolExecutor"}))
            .expect("search");
        let matches = output.lines().collect::<Vec<_>>();

        assert_eq!(
            matches.len(),
            2,
            "oversized files must not consume match budget"
        );
        assert!(matches
            .iter()
            .any(|line| line.contains("lib.rs:1:pub struct ToolExecutor;")));
        assert!(matches
            .iter()
            .any(|line| line.contains("DOCTRINE.md:1:ToolExecutor reference")));
    }

    #[test]
    fn current_time_returns_epoch_and_date() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let output = executor.handle_current_time().expect("current_time");
        assert!(output.contains("epoch:"));
        assert!(output.contains("iso8601_utc:"));

        let epoch_line = output
            .lines()
            .find(|line| line.starts_with("epoch:"))
            .expect("epoch line");
        let epoch = epoch_line
            .split(':')
            .nth(1)
            .expect("epoch value")
            .trim()
            .parse::<u64>()
            .expect("parse epoch");
        assert!(epoch > 1_577_836_800);
    }

    #[test]
    fn time_format_helpers_format_epoch_zero_deterministically() {
        let epoch = 0;

        assert_eq!(iso8601_utc_from_epoch(epoch), "1970-01-01T00:00:00Z");
        assert_eq!(day_of_week_from_epoch(epoch), "Thursday");
    }

    #[test]
    fn time_format_helpers_format_known_friday_deterministically() {
        let epoch = 1_709_251_200;

        assert_eq!(iso8601_utc_from_epoch(epoch), "2024-03-01T00:00:00Z");
        assert_eq!(day_of_week_from_epoch(epoch), "Friday");
    }

    #[tokio::test]
    async fn current_time_tool_dispatch() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let calls = vec![ToolCall {
            id: "1".to_string(),
            name: "current_time".to_string(),
            arguments: serde_json::json!({}),
        }];

        let results = executor.execute_tools(&calls).await.expect("results");
        assert!(results[0].success);
        assert!(results[0].output.contains("day_of_week:"));
    }

    #[test]
    fn current_time_appears_in_definitions() {
        let definitions = fawx_tool_definitions();
        assert!(definitions.iter().any(|tool| tool.name == "current_time"));
    }

    #[tokio::test]
    async fn tool_dispatch_handles_known_tool() {
        let temp = TempDir::new().expect("temp");
        fs::write(temp.path().join("a.txt"), "hello").expect("write");
        let executor = test_executor(temp.path());
        let calls = vec![ToolCall {
            id: "1".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": "a.txt"}),
        }];
        let results = executor.execute_tools(&calls).await.expect("results");
        assert!(results[0].success);
    }

    #[tokio::test]
    async fn tool_dispatch_handles_unknown_tool() {
        let temp = TempDir::new().expect("temp");
        let executor = test_executor(temp.path());
        let calls = vec![ToolCall {
            id: "1".to_string(),
            name: "missing_tool".to_string(),
            arguments: serde_json::json!({}),
        }];
        let results = executor.execute_tools(&calls).await.expect("results");
        assert!(!results[0].success);
        assert!(results[0].output.contains("unknown tool"));
    }
}
