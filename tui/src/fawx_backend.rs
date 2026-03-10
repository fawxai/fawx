#[cfg(test)]
use crate::DEFAULT_ENGINE_URL;
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc::UnboundedSender;

#[async_trait]
pub trait EngineBackend: Send + Sync {
    async fn stream_message(&self, message: String, tx: UnboundedSender<BackendEvent>);
    async fn check_health(&self, tx: UnboundedSender<BackendEvent>);
}

#[derive(Clone)]
pub struct HttpBackend {
    base_url: String,
    client: reqwest::Client,
    bearer_token: Option<String>,
}

/// Status payload returned by the engine's `/status` endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct EngineStatus {
    #[allow(dead_code)]
    pub status: String,
    pub model: String,
    #[serde(default)]
    pub memory_entries: usize,
}

#[derive(Debug)]
pub enum BackendEvent {
    Connected(EngineStatus),
    ConnectionError(String),
    ToolUse {
        name: String,
        arguments: Value,
    },
    ToolResult {
        name: Option<String>,
        success: bool,
        content: String,
    },
    TextDelta(String),
    Done {
        model: Option<String>,
        iterations: Option<u32>,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
    },
    StreamError(String),
}

#[derive(Serialize, Deserialize)]
struct MessageRequest {
    message: String,
}

#[derive(Deserialize)]
struct LegacyMessageResponse {
    response: String,
    model: String,
    iterations: u32,
}

#[derive(Deserialize)]
struct HealthResponse {
    status: String,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
/// SSE event parsed from `event:` + `data:` lines.
///
/// Field names match the engine's `serialize_stream_event` output in
/// `http_serve.rs`.  The `event_type` is set by `parse_sse_frame` from the
/// `event:` line; serde then only needs to match the data payload.
struct SseFrame {
    event_type: String,
    data: String,
}

/// Data payload for `text_delta` events.
#[derive(Deserialize)]
struct TextDeltaData {
    text: String,
}

/// Data payload for `tool_call_start` events.
#[derive(Deserialize)]
struct ToolCallStartData {
    #[serde(default)]
    name: Option<String>,
}

/// Data payload for `tool_call_complete` events.
#[derive(Deserialize)]
struct ToolCallCompleteData {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Value,
}

/// Data payload for `tool_result` events.
#[derive(Deserialize)]
struct ToolResultData {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    output: Option<String>,
    #[serde(default)]
    is_error: bool,
}

/// Data payload for `done` events.
#[derive(Deserialize)]
struct DoneData {
    /// The final response text. Currently unused by the TUI (it
    /// reconstructs the response from streamed deltas) but parsed
    /// for forward compatibility.
    #[serde(default)]
    #[allow(dead_code)]
    response: Option<String>,
}

/// Data payload for `error` events.
#[derive(Deserialize)]
struct ErrorData {
    error: String,
}

fn friendly_http_status_message(status: reqwest::StatusCode, body: &str) -> String {
    match status {
        reqwest::StatusCode::TOO_MANY_REQUESTS => {
            "Fawx is rate limited right now. Wait a moment and try again.".to_string()
        }
        reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN => {
            "Fawx could not authenticate with the engine. Check your bearer token or config and try again."
                .to_string()
        }
        _ if body.trim_start().starts_with('{') || body.trim_start().starts_with('[') => {
            "Fawx hit an unexpected API error. Check the engine logs and try again.".to_string()
        }
        _ => format!("Fawx engine request failed with HTTP {status}."),
    }
}

fn is_sse_content_type(content_type: &str) -> bool {
    content_type
        .trim()
        .to_ascii_lowercase()
        .starts_with("text/event-stream")
}

fn push_sse_chunk(pending: &mut String, chunk: &str) -> Vec<String> {
    pending.push_str(chunk);
    normalize_sse_newlines(pending);
    drain_complete_sse_frames(pending)
}

fn normalize_sse_newlines(pending: &mut String) {
    if pending.contains('\r') {
        *pending = pending.replace("\r\n", "\n").replace('\r', "\n");
    }
}

fn drain_complete_sse_frames(pending: &mut String) -> Vec<String> {
    let mut frames = Vec::new();
    while let Some(index) = pending.find("\n\n") {
        frames.push(pending[..index].to_string());
        pending.drain(..index + 2);
    }
    frames
}

fn parse_sse_frame(frame: &str) -> anyhow::Result<Option<SseFrame>> {
    let mut event_type = String::from("message");
    let mut saw_data = false;
    let mut saw_invalid_content = false;
    let payload = frame
        .lines()
        .filter_map(|line| {
            let line = line.trim_end_matches('\r');
            if let Some(data) = line.strip_prefix("data:") {
                saw_data = true;
                return Some(data.trim_start());
            }
            if let Some(evt) = line.strip_prefix("event:") {
                event_type = evt.trim().to_string();
                return None;
            }
            if line.is_empty()
                || line.starts_with(':')
                || line.starts_with("id:")
                || line.starts_with("retry:")
            {
                return None;
            }
            saw_invalid_content = true;
            None
        })
        .collect::<Vec<_>>()
        .join("\n");

    if saw_data {
        if payload.is_empty() || payload == "[DONE]" {
            return Ok(None);
        }
        return Ok(Some(SseFrame {
            event_type,
            data: payload,
        }));
    }

    if saw_invalid_content {
        return Err(anyhow!("missing SSE data prefix"));
    }

    Ok(None)
}

/// Resolve a bearer token from environment, credential store, or config file.
///
/// Priority:
/// 1. `FAWX_TUI_BEARER_TOKEN` environment variable (highest, override)
/// 2. Encrypted credential store (`~/.fawx/auth.db`, key `http_bearer`)
/// 3. `~/.fawx/config.toml` `[http]` section, `bearer_token` key
/// 4. `None` (server may have auth disabled)
fn resolve_bearer_token() -> Option<String> {
    if let Ok(token) = std::env::var("FAWX_TUI_BEARER_TOKEN") {
        let trimmed = token.trim().to_string();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }
    if let Some(token) = read_token_from_credential_store() {
        return Some(token);
    }
    read_token_from_config()
}

/// Read the HTTP bearer token from the encrypted credential store.
fn read_token_from_credential_store() -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    let data_dir = std::path::PathBuf::from(home).join(".fawx");
    crate::credential_reader::read_provider_token(&data_dir, "http_bearer")
}

/// Parse `bearer_token` from `~/.fawx/config.toml` under `[http]`.
///
/// Uses simple line-by-line parsing to avoid adding a `toml` dependency.
fn read_token_from_config() -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    let path = std::path::PathBuf::from(home)
        .join(".fawx")
        .join("config.toml");
    let content = std::fs::read_to_string(path).ok()?;
    parse_bearer_token_from_toml(&content)
}

/// Extract `bearer_token` value from the `[http]` section of a TOML string.
///
/// Scans for a `[http]` header, then looks for a `bearer_token = "..."` or
/// `bearer_token = '...'` line before the next section header.
fn parse_bearer_token_from_toml(content: &str) -> Option<String> {
    let mut in_http_section = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_http_section = trimmed == "[http]";
            continue;
        }
        if !in_http_section {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("bearer_token") {
            let rest = rest.trim_start();
            let rest = rest.strip_prefix('=')?;
            let val = rest.trim();
            let unquoted = val
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .or_else(|| val.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')));
            let token = unquoted.unwrap_or(val).trim().to_string();
            if !token.is_empty() {
                return Some(token);
            }
        }
    }
    None
}

pub fn friendly_error_message(raw: &str) -> String {
    let trimmed = raw.trim();
    let lower = trimmed.to_ascii_lowercase();

    if trimmed.contains("429") || lower.contains("rate limit") {
        return "Fawx is rate limited right now. Wait a moment and try again.".to_string();
    }

    if trimmed.contains("401")
        || trimmed.contains("403")
        || lower.contains("unauthorized")
        || lower.contains("forbidden")
        || lower.contains("authentication")
        || lower.contains("auth failure")
    {
        return "Fawx could not authenticate with the engine. Check your bearer token or config and try again."
            .to_string();
    }

    if lower.contains("connection refused")
        || lower.contains("request /health")
        || lower.contains("request /status")
        || lower.contains("request /message")
        || lower.contains("error trying to connect")
    {
        return "Fawx could not reach the local engine. Make sure `fawx serve --http` is running."
            .to_string();
    }

    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return "Fawx hit an unexpected API error. Check the engine logs and try again."
            .to_string();
    }

    trimmed.to_string()
}

impl HttpBackend {
    #[cfg(test)]
    pub fn from_env() -> Self {
        let base_url =
            std::env::var("FAWX_TUI_BASE_URL").unwrap_or_else(|_| DEFAULT_ENGINE_URL.to_string());
        Self::new(&base_url)
    }

    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: reqwest::Client::new(),
            bearer_token: resolve_bearer_token(),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn auth_request(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.bearer_token {
            Some(token) => req.bearer_auth(token),
            None => req,
        }
    }

    pub async fn bootstrap(&self) -> anyhow::Result<EngineStatus> {
        let health = self
            .auth_request(self.client.get(format!("{}/health", self.base_url)))
            .send()
            .await
            .context("request /health")?;
        if !health.status().is_success() {
            let status = health.status();
            let body = health.text().await.unwrap_or_default();
            return Err(anyhow!(friendly_http_status_message(status, &body)));
        }
        let health: HealthResponse = health.json().await.context("decode /health")?;
        if health.status != "ok" {
            return Err(anyhow!("engine health returned {}", health.status));
        }

        self.fetch_status().await
    }

    pub async fn fetch_status(&self) -> anyhow::Result<EngineStatus> {
        let status = self
            .auth_request(self.client.get(format!("{}/status", self.base_url)))
            .send()
            .await
            .context("request /status")?;
        if !status.status().is_success() {
            let code = status.status();
            let body = status.text().await.unwrap_or_default();
            return Err(anyhow!(friendly_http_status_message(code, &body)));
        }
        status.json().await.context("decode /status")
    }

    async fn send_message(&self, message: String, tx: UnboundedSender<BackendEvent>) {
        let result = self.stream_message_inner(message, tx.clone()).await;
        if let Err(error) = result {
            try_send(
                &tx,
                BackendEvent::StreamError(friendly_error_message(&error.to_string())),
            );
        }
    }

    async fn stream_message_inner(
        &self,
        message: String,
        tx: UnboundedSender<BackendEvent>,
    ) -> anyhow::Result<()> {
        let response = self
            .auth_request(
                self.client
                    .post(format!("{}/message", self.base_url))
                    .header(reqwest::header::ACCEPT, "text/event-stream")
                    .json(&MessageRequest { message }),
            )
            .send()
            .await
            .context("request /message")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!(friendly_http_status_message(status, &body)));
        }

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();

        if is_sse_content_type(&content_type) {
            self.consume_sse(response, tx).await
        } else {
            self.consume_legacy_json(response, tx).await
        }
    }

    async fn consume_legacy_json(
        &self,
        response: reqwest::Response,
        tx: UnboundedSender<BackendEvent>,
    ) -> anyhow::Result<()> {
        let body: LegacyMessageResponse = response.json().await.context("decode JSON response")?;
        try_send(&tx, BackendEvent::TextDelta(body.response));
        try_send(
            &tx,
            BackendEvent::Done {
                model: Some(body.model),
                iterations: Some(body.iterations),
                input_tokens: None,
                output_tokens: None,
            },
        );
        Ok(())
    }

    async fn consume_sse(
        &self,
        response: reqwest::Response,
        tx: UnboundedSender<BackendEvent>,
    ) -> anyhow::Result<()> {
        let mut pending = String::new();
        let mut stream = response.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("read SSE chunk")?;
            let chunk = String::from_utf8_lossy(&chunk);
            for frame in push_sse_chunk(&mut pending, &chunk) {
                dispatch_sse_frame(&frame, &tx)?;
            }
        }

        if !pending.trim().is_empty() {
            dispatch_sse_frame(&pending, &tx)?;
        }
        Ok(())
    }
}

#[async_trait]
impl EngineBackend for HttpBackend {
    async fn stream_message(&self, message: String, tx: UnboundedSender<BackendEvent>) {
        self.send_message(message, tx).await;
    }

    async fn check_health(&self, tx: UnboundedSender<BackendEvent>) {
        let event = match self.bootstrap().await {
            Ok(status) => BackendEvent::Connected(status),
            Err(error) => BackendEvent::ConnectionError(error.to_string()),
        };
        try_send(&tx, event);
    }
}

/// Send a backend event, logging when the receiver has been dropped.
///
/// Returns `true` if the event was delivered, `false` if the receiver is
/// gone (the TUI event loop has exited and further sends are pointless).
pub(crate) fn try_send(tx: &UnboundedSender<BackendEvent>, event: BackendEvent) -> bool {
    if tx.send(event).is_err() {
        tracing::debug!("backend event receiver dropped");
        return false;
    }
    true
}

fn dispatch_sse_frame(frame: &str, tx: &UnboundedSender<BackendEvent>) -> anyhow::Result<()> {
    let Some(sse) = parse_sse_frame(frame)? else {
        return Ok(());
    };

    match sse.event_type.as_str() {
        "text_delta" => {
            let d: TextDeltaData = serde_json::from_str(&sse.data).context("decode text_delta")?;
            try_send(tx, BackendEvent::TextDelta(d.text));
        }
        "tool_call_start" => {
            let d: ToolCallStartData =
                serde_json::from_str(&sse.data).context("decode tool_call_start")?;
            try_send(
                tx,
                BackendEvent::ToolUse {
                    name: d.name.unwrap_or_default(),
                    arguments: Value::Null,
                },
            );
        }
        "tool_call_complete" => {
            let d: ToolCallCompleteData =
                serde_json::from_str(&sse.data).context("decode tool_call_complete")?;
            try_send(
                tx,
                BackendEvent::ToolUse {
                    name: d.name.unwrap_or_default(),
                    arguments: d.arguments,
                },
            );
        }
        "tool_result" => {
            let d: ToolResultData =
                serde_json::from_str(&sse.data).context("decode tool_result")?;
            try_send(
                tx,
                BackendEvent::ToolResult {
                    name: d.id,
                    success: !d.is_error,
                    content: d.output.unwrap_or_default(),
                },
            );
        }
        "done" => {
            let _d: DoneData = serde_json::from_str(&sse.data).context("decode done")?;
            try_send(
                tx,
                BackendEvent::Done {
                    model: None,
                    iterations: None,
                    input_tokens: None,
                    output_tokens: None,
                },
            );
        }
        "phase" => {
            // Phase changes are informational; TUI doesn't need them yet.
        }
        "error" => {
            let d: ErrorData = serde_json::from_str(&sse.data).context("decode error")?;
            try_send(
                tx,
                BackendEvent::StreamError(friendly_error_message(&d.error)),
            );
        }
        other => {
            tracing::debug!("ignoring unknown SSE event type: {other}");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc::unbounded_channel;

    #[test]
    fn auth_request_adds_bearer_header_when_token_set() {
        let backend = HttpBackend {
            base_url: "http://localhost:8400".to_string(),
            client: reqwest::Client::new(),
            bearer_token: Some("test-secret-token".to_string()),
        };
        let req = backend
            .auth_request(backend.client.get("http://localhost:8400/health"))
            .build()
            .unwrap();
        let auth = req
            .headers()
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(auth, "Bearer test-secret-token");
    }

    #[test]
    fn auth_request_omits_header_when_no_token() {
        let backend = HttpBackend {
            base_url: "http://localhost:8400".to_string(),
            client: reqwest::Client::new(),
            bearer_token: None,
        };
        let req = backend
            .auth_request(backend.client.get("http://localhost:8400/health"))
            .build()
            .unwrap();
        assert!(req.headers().get("authorization").is_none());
    }

    #[test]
    fn parses_bearer_token_from_toml_http_section() {
        let toml = r#"
[general]
name = "fawx"

[http]
bearer_token = "my-secret-123"
port = 8400

[llm]
model = "gpt-4"
"#;
        assert_eq!(
            parse_bearer_token_from_toml(toml),
            Some("my-secret-123".to_string())
        );
    }

    #[test]
    fn parses_bearer_token_with_single_quotes() {
        let toml = "[http]\nbearer_token = 'single-quoted'\n";
        assert_eq!(
            parse_bearer_token_from_toml(toml),
            Some("single-quoted".to_string())
        );
    }

    #[test]
    fn returns_none_when_no_http_section() {
        let toml = "[general]\nbearer_token = \"wrong-section\"\n";
        assert_eq!(parse_bearer_token_from_toml(toml), None);
    }

    #[test]
    fn returns_none_for_empty_bearer_token() {
        let toml = "[http]\nbearer_token = \"\"\n";
        assert_eq!(parse_bearer_token_from_toml(toml), None);
    }

    #[test]
    fn detects_sse_content_type_with_parameters() {
        assert!(is_sse_content_type("text/event-stream; charset=utf-8"));
    }

    #[test]
    fn non_sse_content_type_falls_back_to_legacy_json() {
        assert!(!is_sse_content_type("application/json"));
        assert!(!is_sse_content_type(""));
    }

    #[test]
    fn structured_http_status_messages_cover_auth_rate_limit_and_generic_errors() {
        assert_eq!(
            friendly_http_status_message(reqwest::StatusCode::TOO_MANY_REQUESTS, ""),
            "Fawx is rate limited right now. Wait a moment and try again."
        );
        assert_eq!(
            friendly_http_status_message(reqwest::StatusCode::UNAUTHORIZED, ""),
            "Fawx could not authenticate with the engine. Check your bearer token or config and try again."
        );
        assert_eq!(
            friendly_http_status_message(reqwest::StatusCode::BAD_GATEWAY, "{\"error\":\"boom\"}"),
            "Fawx hit an unexpected API error. Check the engine logs and try again."
        );
        assert_eq!(
            friendly_http_status_message(reqwest::StatusCode::BAD_GATEWAY, "gateway down"),
            "Fawx engine request failed with HTTP 502 Bad Gateway."
        );
    }

    #[test]
    fn reassembles_sse_frames_across_multiple_chunks() {
        let mut pending = String::new();
        let (tx, mut rx) = unbounded_channel();

        assert!(
            push_sse_chunk(&mut pending, "event: text_delta\ndata: {\"text\":\"Hel").is_empty()
        );
        let frames = push_sse_chunk(&mut pending, "lo\"}\n");
        assert!(frames.is_empty());
        let frames = push_sse_chunk(&mut pending, "\n");
        assert_eq!(
            frames,
            vec!["event: text_delta\ndata: {\"text\":\"Hello\"}"]
        );

        dispatch_sse_frame(&frames[0], &tx).expect("frame should decode");
        match rx.try_recv().expect("event should be sent") {
            BackendEvent::TextDelta(content) => assert_eq!(content, "Hello"),
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn normalizes_crlf_and_drains_multiple_frames() {
        let mut pending = String::new();
        let frames = push_sse_chunk(
            &mut pending,
            "event: text_delta\r\ndata: {\"text\":\"A\"}\r\n",
        );
        assert!(frames.is_empty());

        let frames = push_sse_chunk(
            &mut pending,
            "\r\nevent: text_delta\r\ndata: {\"text\":\"B\"}\r\n\r\n",
        );
        assert_eq!(
            frames,
            vec![
                "event: text_delta\ndata: {\"text\":\"A\"}",
                "event: text_delta\ndata: {\"text\":\"B\"}",
            ]
        );
        assert!(pending.is_empty());
    }

    #[test]
    fn done_frame_is_ignored() {
        let (tx, mut rx) = unbounded_channel();
        dispatch_sse_frame("data: [DONE]", &tx).expect("done frame should be ignored");
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn invalid_json_frame_returns_error() {
        let (tx, _rx) = unbounded_channel();
        let error = dispatch_sse_frame("event: text_delta\ndata: {not valid json}", &tx)
            .expect_err("invalid JSON must fail");
        assert!(error.to_string().contains("decode text_delta"));
    }

    #[test]
    fn frame_without_data_prefix_returns_error() {
        let (tx, _rx) = unbounded_channel();
        let error =
            dispatch_sse_frame("payload: nope", &tx).expect_err("malformed frame must fail");
        assert!(error.to_string().contains("missing SSE data prefix"));
    }

    #[test]
    fn keepalive_comment_frame_is_ignored() {
        let (tx, mut rx) = unbounded_channel();
        dispatch_sse_frame(": keep-alive", &tx).expect("comment frame should be ignored");
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn try_send_returns_false_when_receiver_dropped() {
        let (tx, rx) = unbounded_channel();
        drop(rx);
        assert!(!try_send(&tx, BackendEvent::TextDelta("gone".to_string())));
    }

    #[test]
    fn try_send_returns_true_when_receiver_alive() {
        let (tx, _rx) = unbounded_channel();
        assert!(try_send(&tx, BackendEvent::TextDelta("hello".to_string())));
    }
}
