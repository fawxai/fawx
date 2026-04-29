use axum::body::{to_bytes, Body};
use axum::extract::{ConnectInfo, State};
use axum::http::{header, HeaderMap, Method, Request, StatusCode, Uri};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use std::fmt;
use std::fs::{create_dir_all, metadata, remove_file, rename, File, OpenOptions};
use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

const AUDIT_LOG_RELATIVE_PATH: &[&str] = &["logs", "http_audit.jsonl"];
const MAX_AUDIT_BODY_BYTES: usize = 1_048_576;
const MAX_AUDIT_LOG_BYTES: u64 = 10 * 1024 * 1024;
const MAX_MESSAGE_SUMMARY_CHARS: usize = 100;
// Bounded enough to cap memory under bursty request load, large enough to
// absorb short local UI/API bursts without making audit I/O a request-latency
// bottleneck. A typical compact audit entry is a few hundred bytes, so this
// keeps buffered memory on the order of hundreds of KiB during bursts.
const AUDIT_QUEUE_CAPACITY: usize = 1024;
// Audit durability is intentionally batched. Synchronous file writes run on a
// dedicated writer worker and hand data to the OS, while sync_data is
// amortized to avoid forcing disk syncs on every request.
const AUDIT_SYNC_EVERY_ENTRIES: u64 = 100;
const AUDIT_SYNC_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub(crate) struct HttpAuditState {
    writer: Arc<AuditLogWorker>,
}

impl HttpAuditState {
    pub(crate) fn from_data_dir(data_dir: impl AsRef<Path>) -> Self {
        let mut path = data_dir.as_ref().to_path_buf();
        for segment in AUDIT_LOG_RELATIVE_PATH {
            path.push(segment);
        }
        Self {
            writer: Arc::new(AuditLogWorker::spawn(path)),
        }
    }

    #[cfg(test)]
    fn from_log_path(path: impl Into<PathBuf>) -> Self {
        Self {
            writer: Arc::new(AuditLogWorker::spawn(path.into())),
        }
    }
}

struct AuditLogCommand {
    entry: HttpAuditEntry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuditLogSendError {
    Full,
    Closed,
}

struct AuditLogWorker {
    sender: Option<mpsc::Sender<AuditLogCommand>>,
    join_handle: Option<thread::JoinHandle<()>>,
    dropped_entries: AtomicU64,
}

impl AuditLogWorker {
    fn spawn(path: PathBuf) -> Self {
        let (sender, receiver) = mpsc::channel(AUDIT_QUEUE_CAPACITY);
        match thread::Builder::new()
            .name("fawx-http-audit-writer".to_string())
            .spawn(move || run_audit_writer(path, receiver))
        {
            Ok(join_handle) => Self {
                sender: Some(sender),
                join_handle: Some(join_handle),
                dropped_entries: AtomicU64::new(0),
            },
            Err(error) => {
                tracing::warn!(error = %error, "failed to start HTTP audit log writer");
                Self {
                    sender: None,
                    join_handle: None,
                    dropped_entries: AtomicU64::new(0),
                }
            }
        }
    }

    fn send(&self, command: AuditLogCommand) -> Result<(), AuditLogSendError> {
        let Some(sender) = &self.sender else {
            return Err(AuditLogSendError::Closed);
        };
        sender.try_send(command).map_err(|error| match error {
            mpsc::error::TrySendError::Full(_) => AuditLogSendError::Full,
            mpsc::error::TrySendError::Closed(_) => AuditLogSendError::Closed,
        })
    }

    fn record_dropped_entry(&self) -> u64 {
        self.dropped_entries.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn dropped_entries(&self) -> u64 {
        self.dropped_entries.load(Ordering::Relaxed)
    }
}

impl Drop for AuditLogWorker {
    fn drop(&mut self) {
        // Close the queue before joining so the writer drains any accepted
        // entries, drops AuditLogWriter, and performs its final best-effort
        // sync/close without requiring Tokio runtime services.
        self.sender.take();
        if let Some(join_handle) = self.join_handle.take() {
            if join_handle.join().is_err() {
                tracing::warn!("HTTP audit writer thread panicked");
            }
        }
    }
}

impl fmt::Debug for AuditLogWorker {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuditLogWorker")
            .field("started", &self.sender.is_some())
            .field("dropped_entries", &self.dropped_entries())
            .finish_non_exhaustive()
    }
}

fn run_audit_writer(path: PathBuf, mut receiver: mpsc::Receiver<AuditLogCommand>) {
    let mut writer = AuditLogWriter::new(path);
    while let Some(command) = receiver.blocking_recv() {
        if let Err(error) = writer.write(&command.entry) {
            tracing::warn!(error = %error, "failed to write HTTP audit log entry");
        }
    }
}

#[derive(Debug)]
struct AuditLogWriter {
    path: PathBuf,
    file: Option<File>,
    current_log_bytes: Option<u64>,
    parent_ready: bool,
    unsynced_entries: u64,
    last_sync: Instant,
}

impl AuditLogWriter {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            file: None,
            current_log_bytes: None,
            parent_ready: false,
            unsynced_entries: 0,
            last_sync: Instant::now(),
        }
    }

    fn write(&mut self, entry: &HttpAuditEntry) -> anyhow::Result<()> {
        let mut line = serde_json::to_vec(entry)?;
        line.push(b'\n');
        self.rotate_if_needed(line.len())?;
        {
            let file = self.open_file()?;
            file.write_all(&line)?;
        }
        let next_len = self.current_log_bytes()?.saturating_add(line.len() as u64);
        self.current_log_bytes = Some(next_len);
        self.unsynced_entries = self.unsynced_entries.saturating_add(1);
        self.sync_if_due()?;
        Ok(())
    }

    fn open_file(&mut self) -> anyhow::Result<&mut File> {
        if self.file.is_none() {
            self.ensure_parent_dir()?;
            let _ = self.current_log_bytes()?;
            self.file = Some(
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.path)?,
            );
        }
        Ok(self.file.as_mut().expect("file was just opened"))
    }

    fn ensure_parent_dir(&mut self) -> anyhow::Result<()> {
        if self.parent_ready {
            return Ok(());
        }
        if let Some(parent) = self.path.parent() {
            create_dir_all(parent)?;
        }
        self.parent_ready = true;
        Ok(())
    }

    fn current_log_bytes(&mut self) -> anyhow::Result<u64> {
        if let Some(bytes) = self.current_log_bytes {
            return Ok(bytes);
        }
        let bytes = match metadata(&self.path) {
            Ok(metadata) => metadata.len(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
            Err(error) => return Err(error.into()),
        };
        self.current_log_bytes = Some(bytes);
        Ok(bytes)
    }

    fn rotate_if_needed(&mut self, next_line_bytes: usize) -> anyhow::Result<()> {
        let current_len = self.current_log_bytes()?;
        if current_len.saturating_add(next_line_bytes as u64) <= MAX_AUDIT_LOG_BYTES {
            return Ok(());
        }

        // Invariant: AuditLogWriter is owned by the single audit writer thread.
        // The size check, close, and rename stay serialized with writes so
        // rotation cannot race with an append.
        self.sync_now()?;
        self.file = None;
        let rotated_path = rotated_log_path(&self.path);
        match remove_file(&rotated_path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        match rename(&self.path, &rotated_path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        self.current_log_bytes = Some(0);
        self.unsynced_entries = 0;
        Ok(())
    }

    fn sync_if_due(&mut self) -> anyhow::Result<()> {
        if self.unsynced_entries == 0 {
            return Ok(());
        }
        if self.unsynced_entries < AUDIT_SYNC_EVERY_ENTRIES
            && self.last_sync.elapsed() < AUDIT_SYNC_INTERVAL
        {
            return Ok(());
        }
        self.sync_now()
    }

    fn sync_now(&mut self) -> anyhow::Result<()> {
        if let Some(file) = &self.file {
            // fdatasync is enough for request-audit durability: callers need
            // accepted append bytes to survive, not a full metadata sync on
            // every amortized checkpoint.
            file.sync_data()?;
        }
        self.unsynced_entries = 0;
        self.last_sync = Instant::now();
        Ok(())
    }
}

impl Drop for AuditLogWriter {
    fn drop(&mut self) {
        if self.unsynced_entries == 0 {
            return;
        }

        // Best-effort graceful-shutdown durability. The cached file is a
        // std::fs::File, so final sync and close do not need a live Tokio
        // runtime.
        if let Some(file) = &self.file {
            let _ = file.sync_data();
        }
    }
}

#[derive(Debug, Serialize)]
struct HttpAuditEntry {
    timestamp: DateTime<Utc>,
    source_ip: Option<String>,
    method: String,
    endpoint: String,
    status: u16,
    token_fingerprint: Option<String>,
    processing_time_ms: u64,
    message_summary: Option<String>,
}

pub(crate) async fn audit_middleware(
    State(audit): State<HttpAuditState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let started = Instant::now();
    let source_ip = source_ip(&request);
    let method = request.method().clone();
    let uri = request.uri().clone();
    let token_fingerprint = bearer_token_fingerprint(request.headers());

    let (parts, body) = request.into_parts();
    // SAFETY: audit middleware buffers only the request body, then rebuilds the
    // request before handing it to downstream handlers. Do not buffer response
    // bodies here without a streaming-aware design; SSE endpoints must pass
    // through untouched.
    let body_bytes = match to_bytes(body, MAX_AUDIT_BODY_BYTES).await {
        Ok(bytes) => bytes,
        Err(error) => {
            let status = StatusCode::PAYLOAD_TOO_LARGE;
            let entry = HttpAuditEntry {
                timestamp: Utc::now(),
                source_ip,
                method: method.to_string(),
                endpoint: uri.path().to_owned(),
                status: status.as_u16(),
                token_fingerprint,
                processing_time_ms: elapsed_millis(started),
                message_summary: None,
            };
            audit.write(entry);
            tracing::warn!(error = %error, endpoint = %uri.path(), "HTTP request body exceeded audit read limit");
            return status.into_response();
        }
    };

    let message_summary = safe_message_summary(&method, &uri, &parts.headers, &body_bytes);
    let request = Request::from_parts(parts, Body::from(body_bytes));
    let response = next.run(request).await;
    let status = response.status();

    let entry = HttpAuditEntry {
        timestamp: Utc::now(),
        source_ip,
        method: method.to_string(),
        endpoint: uri.path().to_owned(),
        status: status.as_u16(),
        token_fingerprint,
        processing_time_ms: elapsed_millis(started),
        message_summary,
    };
    audit.write(entry);
    response
}

impl HttpAuditState {
    fn write(&self, entry: HttpAuditEntry) {
        match self.writer.send(AuditLogCommand { entry }) {
            Ok(()) => {}
            Err(AuditLogSendError::Full) => {
                let dropped_entries = self.writer.record_dropped_entry();
                tracing::warn!(
                    dropped_entries,
                    "HTTP audit log entry dropped: writer queue full"
                );
            }
            Err(AuditLogSendError::Closed) => {
                let dropped_entries = self.writer.record_dropped_entry();
                tracing::warn!(
                    dropped_entries,
                    "HTTP audit log entry dropped: writer unavailable"
                );
            }
        }
    }
}

fn elapsed_millis(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn rotated_log_path(path: &Path) -> PathBuf {
    match path.file_name() {
        Some(file_name) => {
            let mut rotated_name = file_name.to_os_string();
            rotated_name.push(".1");
            path.with_file_name(rotated_name)
        }
        None => path.with_extension("1"),
    }
}

fn source_ip(request: &Request<Body>) -> Option<String> {
    request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| addr.ip().to_string())
}

fn bearer_token_fingerprint(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?.trim();
    let mut parts = value.splitn(2, char::is_whitespace);
    let scheme = parts.next()?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = parts.next()?;
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return None;
    }
    let suffix = suffix_chars(trimmed, 4);
    Some(format!("last4:{suffix}"))
}

fn suffix_chars(value: &str, max_chars: usize) -> &str {
    let mut start = value.len();
    for (idx, _) in value.char_indices().rev().take(max_chars) {
        start = idx;
    }
    &value[start..]
}

fn safe_message_summary(
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
    body_bytes: &[u8],
) -> Option<String> {
    let sensitive = is_sensitive_endpoint(uri.path());
    if body_bytes.is_empty() || sensitive {
        return sensitive.then(|| "[redacted sensitive request]".to_string());
    }
    if !is_json_content(headers) {
        return None;
    }

    let value = serde_json::from_slice::<Value>(body_bytes).ok()?;
    let candidate = extract_summary_candidate(&value)?;
    let normalized = normalize_summary_text(candidate);
    if normalized.is_empty() {
        return None;
    }
    Some(truncate_chars(
        &redact_summary_text(&normalized),
        MAX_MESSAGE_SUMMARY_CHARS,
    ))
    .filter(|summary| !summary.is_empty())
    .or_else(|| Some(format!("{method} {}", uri.path())).filter(|summary| !summary.is_empty()))
}

fn is_sensitive_endpoint(path: &str) -> bool {
    let mut segments = path.split('/').filter(|segment| !segment.is_empty());
    let first = segments.next();
    let first = if first.is_some_and(|segment| segment.eq_ignore_ascii_case("v1")) {
        segments.next()
    } else {
        first
    };
    let second = segments.next();

    // Keep this route-shaped rather than matching arbitrary path substrings:
    // audit redaction should protect known secret-bearing surfaces without
    // hiding unrelated future routes such as /sessions/{id}/auth-log. Token
    // routes are intentionally not globally redacted; token-bearing setup
    // routes live under /auth, while list-style token routes can remain useful
    // in audit summaries.
    first.is_some_and(is_sensitive_route_segment)
        || first.is_some_and(|segment| segment.eq_ignore_ascii_case("telegram"))
            && second.is_some_and(|segment| segment.eq_ignore_ascii_case("webhook"))
}

fn is_sensitive_route_segment(segment: &str) -> bool {
    [
        "auth", "config", "pair", "webhook", "oauth", "api-key", "api_key", "secret",
    ]
    .iter()
    .any(|candidate| segment.eq_ignore_ascii_case(candidate))
}

fn is_json_content(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("json"))
}

fn extract_summary_candidate(value: &Value) -> Option<&str> {
    let object = value.as_object()?;
    for key in ["message", "text", "prompt", "query", "content"] {
        if let Some(text) = object.get(key).and_then(Value::as_str) {
            return Some(text);
        }
    }
    None
}

fn normalize_summary_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn redact_summary_text(text: &str) -> String {
    let mut redacted = String::with_capacity(text.len());
    for (index, token) in text.split_whitespace().enumerate() {
        if index > 0 {
            redacted.push(' ');
        }
        if looks_sensitive_token(token) {
            redacted.push_str("[redacted]");
        } else {
            redacted.push_str(token);
        }
    }
    redacted
}

fn looks_sensitive_token(token: &str) -> bool {
    let trimmed = token.trim_matches(|ch: char| {
        matches!(ch, '"' | '\'' | '`' | ',' | ';' | ':' | ')' | ']' | '}')
    });
    starts_with_any_ignore_ascii_case(
        trimmed,
        &[
            "sk-",
            "sk_",
            "key-",
            "key_",
            "tok-",
            "tok_",
            "xox",
            "ghp_",
            "github_pat_",
            "bearer",
        ],
    ) || looks_like_encoded_secret(trimmed)
}

fn starts_with_any_ignore_ascii_case(value: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|prefix| {
        value
            .as_bytes()
            .get(..prefix.len())
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix.as_bytes()))
    })
}

fn looks_like_encoded_secret(token: &str) -> bool {
    if token.len() < 32 {
        return false;
    }
    if token.chars().all(|ch| ch.is_ascii_hexdigit() || ch == '-') {
        return false;
    }
    if token
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        return false;
    }

    let mut jwt_segments = token.split('.');
    let is_jwt_like = match (
        jwt_segments.next(),
        jwt_segments.next(),
        jwt_segments.next(),
        jwt_segments.next(),
    ) {
        (Some(header), Some(payload), Some(signature), None) => {
            // JWT headers are base64url-encoded JSON and normally begin with
            // "eyJ" ("{\""). Requiring that structural marker avoids
            // redacting ordinary dotted identifiers such as
            // my-application.version.identifier.
            header.starts_with("eyJ")
                && !payload.is_empty()
                && !signature.is_empty()
                && token
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
        }
        _ => false,
    };
    let is_base64_like = (token.contains('+') || token.contains('/') || token.contains('='))
        && token
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '/' | '='));
    is_jwt_like || is_base64_like
}

fn truncate_chars(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let marker = "...";
    if limit <= marker.len() {
        return marker.chars().take(limit).collect();
    }
    let take = limit - marker.len();
    format!("{}{}", text.chars().take(take).collect::<String>(), marker)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::routing::post;
    use axum::Router;
    use serde_json::json;
    use tempfile::TempDir;
    use tokio::time::{sleep, Duration};
    use tower::ServiceExt;

    fn audit_log_text_is_complete(content: &str) -> bool {
        !content.trim().is_empty()
            && content
                .lines()
                .all(|line| serde_json::from_str::<Value>(line).is_ok())
    }

    async fn read_audit_log_text(path: &Path) -> String {
        for _ in 0..100 {
            match tokio::fs::read_to_string(path).await {
                Ok(content) if audit_log_text_is_complete(&content) => return content,
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => panic!("audit log should be readable: {error}"),
            }
            sleep(Duration::from_millis(10)).await;
        }
        tokio::fs::read_to_string(path)
            .await
            .expect("audit log should exist after writer drain")
    }

    async fn read_audit_entries(path: &Path) -> Vec<Value> {
        let content = read_audit_log_text(path).await;
        content
            .lines()
            .map(|line| serde_json::from_str(line).expect("audit entry should be JSON"))
            .collect()
    }

    fn sample_audit_entry() -> HttpAuditEntry {
        HttpAuditEntry {
            timestamp: Utc::now(),
            source_ip: None,
            method: "POST".to_string(),
            endpoint: "/v1/test".to_string(),
            status: 200,
            token_fingerprint: None,
            processing_time_ms: 1,
            message_summary: Some("sample".to_string()),
        }
    }

    #[tokio::test]
    async fn audit_middleware_logs_success_with_source_status_fingerprint_and_summary() {
        let temp = TempDir::new().expect("temp dir");
        let log_path = temp.path().join("http_audit.jsonl");
        let app = Router::new()
            .route("/v1/sessions/session-a/messages", post(|| async { "ok" }))
            .layer(axum::middleware::from_fn_with_state(
                HttpAuditState::from_log_path(&log_path),
                audit_middleware,
            ));

        let mut request = Request::builder()
            .method("POST")
            .uri("/v1/sessions/session-a/messages")
            .header(header::AUTHORIZATION, "Bearer test-token-1234")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({ "message": "hello from the audit logger" }).to_string(),
            ))
            .expect("request");
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([192, 0, 2, 4], 9000))));

        let response = app.oneshot(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::OK);

        let entries = read_audit_entries(&log_path).await;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["source_ip"], "192.0.2.4");
        assert_eq!(entries[0]["method"], "POST");
        assert_eq!(entries[0]["endpoint"], "/v1/sessions/session-a/messages");
        assert_eq!(entries[0]["status"], 200);
        assert_eq!(entries[0]["token_fingerprint"], "last4:1234");
        assert_eq!(entries[0]["message_summary"], "hello from the audit logger");
        assert!(entries[0]["processing_time_ms"].as_u64().is_some());
    }

    #[tokio::test]
    async fn audit_middleware_logs_failed_status_without_token() {
        let temp = TempDir::new().expect("temp dir");
        let log_path = temp.path().join("http_audit.jsonl");
        let app = Router::new()
            .route("/v1/protected", post(|| async { StatusCode::UNAUTHORIZED }))
            .layer(axum::middleware::from_fn_with_state(
                HttpAuditState::from_log_path(&log_path),
                audit_middleware,
            ));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/protected")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let entries = read_audit_entries(&log_path).await;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["status"], 401);
        assert_eq!(entries[0]["token_fingerprint"], Value::Null);
    }

    #[tokio::test]
    async fn audit_middleware_accepts_case_insensitive_bearer_scheme() {
        let temp = TempDir::new().expect("temp dir");
        let log_path = temp.path().join("http_audit.jsonl");
        let app = Router::new()
            .route("/v1/protected", post(|| async { StatusCode::OK }))
            .layer(axum::middleware::from_fn_with_state(
                HttpAuditState::from_log_path(&log_path),
                audit_middleware,
            ));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/protected")
                    .header(header::AUTHORIZATION, "bearer lower-case-5678")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);

        let entries = read_audit_entries(&log_path).await;
        assert_eq!(entries[0]["token_fingerprint"], "last4:5678");
    }

    #[tokio::test]
    async fn audit_middleware_logs_payload_too_large_without_calling_handler() {
        let temp = TempDir::new().expect("temp dir");
        let log_path = temp.path().join("http_audit.jsonl");
        let app = Router::new()
            .route(
                "/v1/sessions/session-a/messages",
                post(|| async { StatusCode::OK }),
            )
            .layer(axum::middleware::from_fn_with_state(
                HttpAuditState::from_log_path(&log_path),
                audit_middleware,
            ));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/sessions/session-a/messages")
                    .body(Body::from(vec![b'x'; MAX_AUDIT_BODY_BYTES + 1]))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);

        let entries = read_audit_entries(&log_path).await;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["endpoint"], "/v1/sessions/session-a/messages");
        assert_eq!(entries[0]["status"], 413);
        assert_eq!(entries[0]["message_summary"], Value::Null);
    }

    #[tokio::test]
    async fn audit_middleware_redacts_sensitive_endpoint_bodies() {
        let temp = TempDir::new().expect("temp dir");
        let log_path = temp.path().join("http_audit.jsonl");
        let app = Router::new()
            .route("/v1/auth/openai/api-key", post(|| async { "ok" }))
            .layer(axum::middleware::from_fn_with_state(
                HttpAuditState::from_log_path(&log_path),
                audit_middleware,
            ));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/auth/openai/api-key")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({ "api_key": "test-secret-value" }).to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);

        let raw_log = read_audit_log_text(&log_path).await;
        assert!(!raw_log.contains("test-secret-value"));
        let entries = read_audit_entries(&log_path).await;
        assert_eq!(
            entries[0]["message_summary"],
            "[redacted sensitive request]"
        );
    }

    #[tokio::test]
    async fn audit_middleware_does_not_redact_path_substring_matches() {
        let temp = TempDir::new().expect("temp dir");
        let log_path = temp.path().join("http_audit.jsonl");
        let app = Router::new()
            .route(
                "/v1/sessions/session-tokenish/messages",
                post(|| async { "ok" }),
            )
            .layer(axum::middleware::from_fn_with_state(
                HttpAuditState::from_log_path(&log_path),
                audit_middleware,
            ));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/sessions/session-tokenish/messages")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({ "message": "keep this safe summary visible" }).to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);

        let entries = read_audit_entries(&log_path).await;
        assert_eq!(
            entries[0]["message_summary"],
            "keep this safe summary visible"
        );
    }

    #[test]
    fn truncation_marks_shortened_summaries() {
        assert_eq!(truncate_chars("abcdef", 6), "abcdef");
        assert_eq!(truncate_chars("abcdef", 5), "ab...");
    }

    #[test]
    fn rotated_log_path_appends_suffix_without_replacing_extension() {
        assert_eq!(
            rotated_log_path(Path::new("/tmp/http_audit.jsonl")),
            PathBuf::from("/tmp/http_audit.jsonl.1")
        );
    }

    #[test]
    fn sensitive_endpoint_detection_uses_route_shapes() {
        assert!(is_sensitive_endpoint("/v1/auth/openai/api-key"));
        assert!(is_sensitive_endpoint("/v1/oauth/callback"));
        assert!(is_sensitive_endpoint("/v1/api-key"));
        assert!(is_sensitive_endpoint("/v1/secret/import"));
        assert!(is_sensitive_endpoint("/telegram/webhook"));
        assert!(is_sensitive_endpoint("/config"));
        assert!(!is_sensitive_endpoint("/v1/sessions/session-a/auth-log"));
        assert!(!is_sensitive_endpoint("/v1/tokens/list"));
    }

    #[test]
    fn audit_state_counts_dropped_entries_when_writer_unavailable() {
        let audit = HttpAuditState {
            writer: Arc::new(AuditLogWorker {
                sender: None,
                join_handle: None,
                dropped_entries: AtomicU64::new(0),
            }),
        };

        audit.write(sample_audit_entry());

        assert_eq!(audit.writer.dropped_entries(), 1);
    }

    #[test]
    fn summary_redaction_catches_common_secret_shapes() {
        let redacted = redact_summary_text(
            "token key-super-secret tok_value \
             eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.payload.signature \
             0123456789abcdef0123456789abcdef01234567",
        );
        assert!(redacted.contains("token [redacted] [redacted] [redacted]"));
        assert!(redacted.contains("0123456789abcdef0123456789abcdef01234567"));
    }

    #[test]
    fn encoded_secret_detection_does_not_redact_plain_long_identifiers() {
        let text = "my-application-instance-identifier-value should stay visible";
        assert_eq!(redact_summary_text(text), text);

        let dotted_identifier = "my-long-application-name.version.identifier should stay visible";
        assert_eq!(redact_summary_text(dotted_identifier), dotted_identifier);
    }
}
