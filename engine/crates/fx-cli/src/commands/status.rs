use super::runtime_layout::RuntimeLayout;
use crate::persisted_memory::persisted_memory_entry_count;
use serde::Deserialize;
use std::fs;
use std::path::Path;
use std::time::Duration;

pub async fn run() -> anyhow::Result<i32> {
    let layout = RuntimeLayout::detect()?;
    let snapshot = collect_status(&layout).await;
    for line in render_status(&snapshot) {
        println!("{line}");
    }
    Ok(0)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct StatusSnapshot {
    running: bool,
    stale_pid_file: bool,
    pid: Option<u32>,
    uptime: Option<String>,
    http_address: Option<String>,
    provider: Option<String>,
    model: Option<String>,
    memory: Option<String>,
    skills: Option<String>,
    sessions: Option<String>,
}

#[derive(Debug, Deserialize)]
struct HealthPayload {
    model: String,
    uptime_seconds: u64,
    skills_loaded: usize,
}

#[derive(Debug, Deserialize)]
struct StatusPayload {
    model: String,
    skills: Vec<String>,
    memory_entries: usize,
}

async fn collect_status(layout: &RuntimeLayout) -> StatusSnapshot {
    let Some(pid) = read_pid(&layout.pid_file) else {
        return StatusSnapshot::default();
    };
    if !process_exists(pid) {
        return stale_pid_snapshot();
    }
    let mut snapshot = running_snapshot(pid, read_process_uptime(pid));
    apply_http_snapshot(&mut snapshot, layout).await;
    apply_filesystem_snapshot(&mut snapshot, layout);
    snapshot
}

fn stale_pid_snapshot() -> StatusSnapshot {
    StatusSnapshot {
        stale_pid_file: true,
        ..Default::default()
    }
}

fn running_snapshot(pid: u32, uptime: Option<String>) -> StatusSnapshot {
    StatusSnapshot {
        running: true,
        pid: Some(pid),
        uptime,
        ..Default::default()
    }
}

async fn apply_http_snapshot(snapshot: &mut StatusSnapshot, layout: &RuntimeLayout) {
    if let Some(health) = fetch_health(layout.http_port).await {
        snapshot.http_address = Some(format!("127.0.0.1:{}", layout.http_port));
        apply_model_selector(snapshot, &health.model);
        snapshot.uptime = Some(format_duration(health.uptime_seconds));
        snapshot.skills = Some(format!("{} loaded", health.skills_loaded));
    }
    if let Some(status) = fetch_status(layout).await {
        apply_model_selector(snapshot, &status.model);
        snapshot.skills = Some(format!("{} installed", status.skills.len()));
        snapshot.memory = Some(format_memory(status.memory_entries, layout));
    }
}

fn apply_filesystem_snapshot(snapshot: &mut StatusSnapshot, layout: &RuntimeLayout) {
    if snapshot.provider.is_none() || snapshot.model.is_none() {
        if let Some(selector) = layout.config.model.default_model.as_deref() {
            apply_model_selector(snapshot, selector);
        }
    }
    if snapshot.memory.is_none() {
        let count = persisted_memory_entry_count(&layout.memory_json_path);
        snapshot.memory = Some(format_memory(count, layout));
    }
    if snapshot.skills.is_none() {
        snapshot.skills =
            count_skill_dirs(&layout.skills_dir).map(|count| format!("{count} installed"));
    }
    snapshot.sessions =
        count_session_files(&layout.sessions_dir).map(|count| format!("{count} known"));
}

fn format_memory(entries: usize, layout: &RuntimeLayout) -> String {
    let embeddings = if layout.config.memory.embeddings_enabled {
        "embeddings enabled"
    } else {
        "embeddings disabled"
    };
    format!("{entries} entries, {embeddings}")
}

async fn fetch_health(port: u16) -> Option<HealthPayload> {
    let client = http_client()?;
    let url = format!("http://127.0.0.1:{port}/health");
    client.get(url).send().await.ok()?.json().await.ok()
}

async fn fetch_status(layout: &RuntimeLayout) -> Option<StatusPayload> {
    let token = super::api_client::bearer_token(layout).ok()?;
    let client = http_client()?;
    let url = format!("http://127.0.0.1:{}/status", layout.http_port);
    client
        .get(url)
        .bearer_auth(token)
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()
}

fn http_client() -> Option<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .ok()
}

fn apply_model_selector(snapshot: &mut StatusSnapshot, selector: &str) {
    let (provider, model) = split_model_selector(selector);
    snapshot.provider = Some(provider);
    snapshot.model = Some(model);
}

fn split_model_selector(selector: &str) -> (String, String) {
    match selector.split_once('/') {
        Some((provider, model)) => (provider.to_string(), model.to_string()),
        None => ("unknown".to_string(), selector.to_string()),
    }
}

fn read_pid(path: &Path) -> Option<u32> {
    let content = fs::read_to_string(path).ok()?;
    parse_pid(&content)
}

fn parse_pid(content: &str) -> Option<u32> {
    content.trim().parse().ok()
}

fn process_exists(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let result = unsafe { libc::kill(pid as i32, 0) };
        result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

fn read_process_uptime(pid: u32) -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        linux_process_uptime(pid)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        None
    }
}

#[cfg(target_os = "linux")]
fn linux_process_uptime(pid: u32) -> Option<String> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let uptime = fs::read_to_string("/proc/uptime").ok()?;
    let start_ticks = parse_linux_start_ticks(&stat)?;
    let uptime_seconds = parse_linux_uptime_seconds(&uptime)?;
    let ticks_per_second = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if ticks_per_second <= 0 {
        return None;
    }
    let started = start_ticks as f64 / ticks_per_second as f64;
    Some(format_duration((uptime_seconds - started).max(0.0) as u64))
}

#[cfg(target_os = "linux")]
fn parse_linux_start_ticks(stat: &str) -> Option<u64> {
    let after_comm = stat.rsplit_once(") ")?.1;
    // Field 22 (`starttime`) sits at offset 19 after stripping `pid`, `(comm)`, and `state`.
    after_comm.split_whitespace().nth(19)?.parse().ok()
}

#[cfg(target_os = "linux")]
fn parse_linux_uptime_seconds(content: &str) -> Option<f64> {
    content.split_whitespace().next()?.parse().ok()
}

fn count_skill_dirs(path: &Path) -> Option<usize> {
    read_directory_count(path, |entry| entry.path().is_dir()).ok()
}

fn count_session_files(path: &Path) -> Option<usize> {
    read_directory_count(path, |entry| entry.path().is_file()).ok()
}

fn read_directory_count<F>(path: &Path, filter: F) -> anyhow::Result<usize>
where
    F: Fn(&fs::DirEntry) -> bool,
{
    if !path.exists() {
        return Ok(0);
    }
    let mut count = 0;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if filter(&entry) {
            count += 1;
        }
    }
    Ok(count)
}

fn format_duration(total_seconds: u64) -> String {
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    match (hours, minutes) {
        (0, 0) => format!("{seconds}s"),
        (0, _) => format!("{minutes}m {seconds}s"),
        _ => format!("{hours}h {minutes}m"),
    }
}

fn render_status(snapshot: &StatusSnapshot) -> Vec<String> {
    let mut lines = vec!["Fawx Status".to_string(), "───────────".to_string()];
    if !snapshot.running {
        lines.push(format_row("Engine", stopped_engine_message(snapshot)));
        return lines;
    }
    lines.push(format_row("Engine", &engine_line(snapshot)));
    push_optional_line(
        &mut lines,
        "HTTP API",
        snapshot
            .http_address
            .clone()
            .map(|v| format!("listening on {v}")),
    );
    push_optional_line(&mut lines, "Provider", provider_line(snapshot));
    push_optional_line(&mut lines, "Memory", snapshot.memory.clone());
    push_optional_line(&mut lines, "Skills", snapshot.skills.clone());
    push_optional_line(&mut lines, "Sessions", snapshot.sessions.clone());
    lines
}

fn stopped_engine_message(snapshot: &StatusSnapshot) -> &str {
    if snapshot.stale_pid_file {
        "not running (stale PID file)"
    } else {
        "not running"
    }
}

fn engine_line(snapshot: &StatusSnapshot) -> String {
    match (snapshot.pid, snapshot.uptime.as_deref()) {
        (Some(pid), Some(uptime)) => format!("running (PID {pid}, uptime {uptime})"),
        (Some(pid), None) => format!("running (PID {pid})"),
        _ => "running".to_string(),
    }
}

fn provider_line(snapshot: &StatusSnapshot) -> Option<String> {
    let provider = snapshot.provider.as_deref()?;
    let model = snapshot.model.as_deref()?;
    Some(format!("{provider} ({model})"))
}

fn push_optional_line(lines: &mut Vec<String>, label: &str, value: Option<String>) {
    if let Some(value) = value {
        lines.push(format_row(label, &value));
    }
}

fn format_row(label: &str, value: &str) -> String {
    let padding = " ".repeat(11usize.saturating_sub(label.len() + 1));
    format!("{label}:{padding}{value}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_config::FawxConfig;
    use std::path::{Path, PathBuf};

    fn test_layout(root: &Path) -> RuntimeLayout {
        let mut config = FawxConfig::default();
        config.memory.embeddings_enabled = true;
        RuntimeLayout {
            data_dir: root.to_path_buf(),
            config_path: root.join("config.toml"),
            storage_dir: root.join("storage"),
            audit_log_path: root.join("audit.log"),
            auth_db_path: root.join("auth.db"),
            logs_dir: root.join("logs"),
            skills_dir: root.join("skills"),
            trusted_keys_dir: root.join("trusted_keys"),
            embedding_model_dir: root.join("models"),
            pid_file: root.join("fawx.pid"),
            memory_json_path: root.join("memory").join("memory.json"),
            sessions_dir: root.join("signals"),
            security_baseline_path: root.join("security-baseline.json"),
            repo_root: PathBuf::from("/tmp/fawx"),
            http_port: 8400,
            config,
        }
    }

    fn write_memory_store(path: &Path, count: usize) {
        let Some(parent) = path.parent() else {
            panic!("memory path missing parent");
        };
        fs::create_dir_all(parent).expect("create memory dir");
        let mut store = serde_json::Map::new();
        for index in 0..count {
            store.insert(
                format!("memory-{index}"),
                serde_json::json!({
                    "value": format!("entry-{index}"),
                    "created_at_ms": 1,
                    "last_accessed_at_ms": 2,
                    "access_count": 3,
                    "source": "User",
                    "tags": []
                }),
            );
        }
        fs::write(path, serde_json::Value::Object(store).to_string()).expect("write memory json");
    }

    #[test]
    fn parse_pid_file_content() {
        assert_eq!(parse_pid("12345\n"), Some(12345));
        assert_eq!(parse_pid("not-a-pid"), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_pid_stat_parser_reads_start_ticks() {
        let stat = "12345 (fawx) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 123456 21 22";
        assert_eq!(parse_linux_start_ticks(stat), Some(123456));
    }

    #[test]
    fn filesystem_snapshot_counts_object_shaped_memory_store() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let layout = test_layout(tempdir.path());
        write_memory_store(&layout.memory_json_path, 2);
        let mut snapshot = StatusSnapshot::default();

        apply_filesystem_snapshot(&mut snapshot, &layout);

        assert_eq!(
            snapshot.memory.as_deref(),
            Some("2 entries, embeddings enabled")
        );
    }

    #[test]
    fn filesystem_snapshot_defaults_missing_memory_store_to_zero() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let layout = test_layout(tempdir.path());
        let mut snapshot = StatusSnapshot::default();

        apply_filesystem_snapshot(&mut snapshot, &layout);

        assert_eq!(
            snapshot.memory.as_deref(),
            Some("0 entries, embeddings enabled")
        );
    }

    #[test]
    fn render_status_formats_running_output() {
        let snapshot = StatusSnapshot {
            running: true,
            pid: Some(12345),
            uptime: Some("2h 34m".to_string()),
            http_address: Some("127.0.0.1:8400".to_string()),
            provider: Some("anthropic".to_string()),
            model: Some("claude-sonnet-4-20250514".to_string()),
            memory: Some("42 entries, embeddings enabled".to_string()),
            skills: Some("8 installed".to_string()),
            sessions: Some("1 known".to_string()),
            ..Default::default()
        };

        let rendered = render_status(&snapshot).join("\n");
        assert!(rendered.contains("Engine:    running (PID 12345, uptime 2h 34m)"));
        assert!(rendered.contains("HTTP API:  listening on 127.0.0.1:8400"));
        assert!(rendered.contains("Provider:  anthropic (claude-sonnet-4-20250514)"));
    }

    #[test]
    fn render_status_handles_missing_pid_file() {
        let rendered = render_status(&StatusSnapshot::default()).join("\n");
        assert!(rendered.contains("Engine:    not running"));
    }

    #[test]
    fn render_status_calls_out_stale_pid_file() {
        let rendered = render_status(&StatusSnapshot {
            stale_pid_file: true,
            ..Default::default()
        })
        .join("\n");
        assert!(rendered.contains("Engine:    not running (stale PID file)"));
    }
}
