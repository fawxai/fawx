use crate::{check_api_response, TelegramError, DEFAULT_BASE_URL, TELEGRAM_MAX_MESSAGE_LENGTH};
use fx_consensus::{ProgressCallback, ProgressEvent};
use serde::Deserialize;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

const MIN_EDIT_INTERVAL: Duration = Duration::from_secs(1);
const PROGRESS_BUFFER_LIMIT: usize = 4000;
const TRUNCATED_TAIL_BYTES: usize = 3900;
const TRUNCATION_PREFIX: &str = "...\n";

/// Sends experiment progress as a single self-updating Telegram message.
pub struct TelegramProgressSender {
    bot_token: String,
    chat_id: i64,
    message_id: Option<i64>,
    buffer: String,
    last_edit: Option<Instant>,
    client: reqwest::Client,
    base_url: String,
}

impl TelegramProgressSender {
    pub fn new(bot_token: String, chat_id: i64) -> Self {
        Self::new_with_base_url(bot_token, chat_id, DEFAULT_BASE_URL.to_string())
    }

    fn new_with_base_url(bot_token: String, chat_id: i64, base_url: String) -> Self {
        Self {
            bot_token,
            chat_id,
            message_id: None,
            buffer: String::new(),
            last_edit: None,
            client: reqwest::Client::new(),
            base_url,
        }
    }

    /// Send initial message. Stores message_id for edits.
    pub async fn send_initial(&mut self, text: &str) -> Result<(), TelegramError> {
        let body = serde_json::json!({
            "chat_id": self.chat_id,
            "text": text,
        });
        let response = self.api_request("sendMessage", &body).await?;
        self.message_id = Some(parse_message_id(response).await?);
        Ok(())
    }

    /// Edit the message. Rate-limited to 1/second.
    pub async fn edit(&mut self, text: &str) -> Result<(), TelegramError> {
        if self.should_skip_edit() {
            return Ok(());
        }
        self.force_edit(text).await
    }

    /// Append a progress line and trigger edit if rate limit allows.
    pub async fn push_line(&mut self, line: &str) -> Result<(), TelegramError> {
        append_line(&mut self.buffer, line);
        truncate_buffer(&mut self.buffer);
        if self.message_id.is_none() {
            let text = self.buffer.clone();
            self.send_initial(&text).await
        } else if self.should_skip_edit() {
            Ok(())
        } else {
            let text = self.buffer.clone();
            self.force_edit(&text).await
        }
    }

    /// Flush any remaining buffered content (final edit).
    pub async fn flush(&mut self) -> Result<(), TelegramError> {
        if self.message_id.is_some() && !self.buffer.is_empty() {
            let text = self.buffer.clone();
            return self.force_edit(&text).await;
        }
        Ok(())
    }

    async fn force_edit(&mut self, text: &str) -> Result<(), TelegramError> {
        let message_id = self.message_id.ok_or_else(uninitialized_message_error)?;
        let body = serde_json::json!({
            "chat_id": self.chat_id,
            "message_id": message_id,
            "text": text,
        });
        let response = self.api_request("editMessageText", &body).await?;
        match check_api_response(response).await {
            Ok(()) => {}
            Err(TelegramError::ApiError(ref msg)) if msg.contains("message is not modified") => {
                // Telegram returns 400 when editMessageText is called with
                // identical content. This is expected during rate-limited
                // flushes and is safe to ignore.
            }
            Err(error) => return Err(error),
        }
        self.last_edit = Some(Instant::now());
        Ok(())
    }

    async fn api_request<T: serde::Serialize>(
        &self,
        method: &str,
        body: &T,
    ) -> Result<reqwest::Response, TelegramError> {
        let url = format!("{}/bot{}/{}", self.base_url, self.bot_token, method);
        self.client
            .post(url)
            .json(body)
            .send()
            .await
            .map_err(|error| TelegramError::ApiError(crate::sanitize_telegram_error(&error)))
    }

    fn should_skip_edit(&self) -> bool {
        self.last_edit
            .map(|last_edit| last_edit.elapsed() < MIN_EDIT_INTERVAL)
            .unwrap_or(false)
    }
}

/// Build a progress callback that streams formatted experiment events into a
/// TelegramProgressSender. The caller should drop the callback after the
/// experiment finishes, then await the join handle to flush the final edit.
pub fn build_telegram_progress_callback(
    bot_token: String,
    chat_id: i64,
) -> (ProgressCallback, tokio::task::JoinHandle<()>) {
    build_telegram_progress_callback_with_base_url(bot_token, chat_id, DEFAULT_BASE_URL.to_string())
}

pub(crate) fn build_telegram_progress_callback_with_base_url(
    bot_token: String,
    chat_id: i64,
    base_url: String,
) -> (ProgressCallback, tokio::task::JoinHandle<()>) {
    let sender = TelegramProgressSender::new_with_base_url(bot_token, chat_id, base_url);
    build_progress_callback_from_sender(sender)
}

fn build_progress_callback_from_sender(
    sender: TelegramProgressSender,
) -> (ProgressCallback, tokio::task::JoinHandle<()>) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let callback = build_callback_sender(tx);
    let handle = tokio::spawn(async move {
        forward_progress_lines(sender, rx).await;
    });
    (callback, handle)
}

fn build_callback_sender(tx: UnboundedSender<String>) -> ProgressCallback {
    Arc::new(move |event: &ProgressEvent| {
        let line = fx_consensus::format_progress_event(event);
        let _ = tx.send(line);
    })
}

async fn forward_progress_lines(
    mut sender: TelegramProgressSender,
    mut rx: UnboundedReceiver<String>,
) {
    while let Some(line) = rx.recv().await {
        if let Err(error) = sender.push_line(&line).await {
            tracing::warn!(%error, "telegram progress send failed");
            return;
        }
    }

    if let Err(error) = sender.flush().await {
        tracing::warn!(%error, "telegram progress flush failed");
    }
}

#[derive(Debug, Deserialize)]
struct SendMessageResponse {
    ok: bool,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    result: Option<SendMessageResult>,
}

#[derive(Debug, Deserialize)]
struct SendMessageResult {
    message_id: i64,
}

async fn parse_message_id(response: reqwest::Response) -> Result<i64, TelegramError> {
    let payload: SendMessageResponse = response
        .json()
        .await
        .map_err(|error| TelegramError::ApiError(crate::sanitize_telegram_error(&error)))?;

    if !payload.ok {
        let description = payload
            .description
            .unwrap_or_else(|| "unknown error".to_string());
        return Err(TelegramError::ApiError(description));
    }

    payload
        .result
        .map(|result| result.message_id)
        .ok_or_else(missing_message_id_error)
}

fn append_line(buffer: &mut String, line: &str) {
    if !buffer.is_empty() {
        buffer.push('\n');
    }
    buffer.push_str(line);
}

fn truncate_buffer(buffer: &mut String) {
    if buffer.len() <= PROGRESS_BUFFER_LIMIT {
        return;
    }

    let start = floor_char_boundary(buffer, buffer.len() - TRUNCATED_TAIL_BYTES);
    *buffer = format!("{TRUNCATION_PREFIX}{}", &buffer[start..]);
    // TRUNCATION_PREFIX (4) + TRUNCATED_TAIL_BYTES (3900) = 3904 < 4096.
    // The assert holds by construction; kept as a runtime safety net.
    debug_assert!(buffer.len() <= TELEGRAM_MAX_MESSAGE_LENGTH);
}

// TODO(#1372): replace with str::floor_char_boundary when stabilized (rust-lang/rust#93743)
fn floor_char_boundary(text: &str, index: usize) -> usize {
    if index >= text.len() {
        return text.len();
    }

    let mut boundary = index;
    while boundary > 0 && !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    boundary
}

fn uninitialized_message_error() -> TelegramError {
    TelegramError::ApiError("progress message not initialized".to_string())
}

fn missing_message_id_error() -> TelegramError {
    TelegramError::ApiError("sendMessage response missing result.message_id".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::State;
    use axum::routing::post;
    use axum::{Json, Router};
    use serde_json::{json, Value};
    use std::sync::Arc;
    use tokio::net::TcpListener;
    use tokio::sync::Mutex;

    type RequestLog = Arc<Mutex<Vec<(String, Value)>>>;

    #[derive(Clone)]
    struct MockState {
        requests: RequestLog,
    }

    #[test]
    fn new_creates_sender_without_message_id() {
        let sender = TelegramProgressSender::new("token".to_string(), 123);
        assert!(sender.message_id.is_none());
        assert!(sender.buffer.is_empty());
        assert!(sender.last_edit.is_none());
    }

    #[tokio::test]
    async fn send_initial_stores_message_id() {
        let (base_url, requests) = spawn_mock_server().await;
        let mut sender = make_sender(&base_url);

        sender.send_initial("starting experiment").await.unwrap();

        assert_eq!(sender.message_id, Some(456));
        assert_eq!(requests.lock().await.len(), 1);
    }

    #[tokio::test]
    async fn push_line_appends_to_buffer() {
        let mut sender = make_rate_limited_sender();

        sender.push_line("round 1").await.unwrap();
        sender.push_line("round 2").await.unwrap();

        assert_eq!(sender.buffer, "round 1\nround 2");
    }

    #[tokio::test]
    async fn push_line_truncates_buffer_to_keep_tail() {
        let mut sender = make_rate_limited_sender();
        let long_line = format!("prefix{}TAIL", "x".repeat(4100));

        sender.push_line(&long_line).await.unwrap();

        assert!(sender.buffer.starts_with(TRUNCATION_PREFIX));
        assert!(sender.buffer.ends_with("TAIL"));
        assert!(sender.buffer.len() <= TELEGRAM_MAX_MESSAGE_LENGTH);
    }

    #[tokio::test]
    async fn edit_skips_requests_within_rate_limit_window() {
        let (base_url, requests) = spawn_mock_server().await;
        let mut sender = make_sender(&base_url);
        sender.message_id = Some(456);
        sender.last_edit = Some(Instant::now());

        sender.edit("updated progress").await.unwrap();

        assert!(requests.lock().await.is_empty());
    }

    #[tokio::test]
    async fn flush_forces_edit_even_when_rate_limited() {
        let (base_url, requests) = spawn_mock_server().await;
        let mut sender = make_sender(&base_url);
        sender.message_id = Some(456);
        sender.buffer = "done".to_string();
        sender.last_edit = Some(Instant::now());

        sender.flush().await.unwrap();

        let requests = requests.lock().await;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].0, "editMessageText");
        assert_eq!(requests[0].1["text"], "done");
    }

    #[tokio::test]
    async fn progress_callback_bridge_flushes_buffered_lines() {
        let (base_url, requests) = spawn_mock_server().await;
        let mut sender = make_sender(&base_url);
        sender.message_id = Some(456);
        sender.last_edit = Some(Instant::now());

        let (callback, handle) = build_progress_callback_from_sender(sender);
        let first = ProgressEvent::RoundStarted {
            round: 1,
            max_rounds: 1,
            signal: "signal".to_string(),
        };
        let second = ProgressEvent::BaselineCollected {
            round: 1,
            max_rounds: 1,
            node_count: 2,
        };
        callback(&first);
        callback(&second);
        drop(callback);
        handle.await.expect("join progress forwarder");

        let expected = format!(
            "{}\n{}",
            fx_consensus::format_progress_event(&first),
            fx_consensus::format_progress_event(&second)
        );
        let requests = requests.lock().await;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].0, "editMessageText");
        assert_eq!(requests[0].1["text"], expected);
    }

    fn make_rate_limited_sender() -> TelegramProgressSender {
        let mut sender = TelegramProgressSender::new("token".to_string(), 123);
        sender.message_id = Some(456);
        sender.last_edit = Some(Instant::now());
        sender
    }

    fn make_sender(base_url: &str) -> TelegramProgressSender {
        TelegramProgressSender {
            bot_token: "token".to_string(),
            chat_id: 123,
            message_id: None,
            buffer: String::new(),
            last_edit: None,
            client: reqwest::Client::new(),
            base_url: base_url.to_string(),
        }
    }

    async fn spawn_mock_server() -> (String, RequestLog) {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let state = MockState {
            requests: requests.clone(),
        };
        let app = Router::new()
            .route("/bottoken/sendMessage", post(send_message))
            .route("/bottoken/editMessageText", post(edit_message))
            .with_state(state);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.ok();
        });
        (format!("http://{addr}"), requests)
    }

    async fn send_message(State(state): State<MockState>, Json(body): Json<Value>) -> Json<Value> {
        record_request(&state, "sendMessage", body).await;
        Json(json!({
            "ok": true,
            "result": { "message_id": 456 }
        }))
    }

    async fn edit_message(State(state): State<MockState>, Json(body): Json<Value>) -> Json<Value> {
        record_request(&state, "editMessageText", body).await;
        Json(json!({ "ok": true, "result": true }))
    }

    async fn record_request(state: &MockState, method: &str, body: Value) {
        state.requests.lock().await.push((method.to_string(), body));
    }

    // ── Error-path tests ──

    async fn spawn_error_server(send_resp: Value, edit_resp: Value) -> String {
        let send = Arc::new(send_resp);
        let edit = Arc::new(edit_resp);
        let app = Router::new()
            .route(
                "/bottoken/sendMessage",
                post({
                    let send = send.clone();
                    move |_body: Json<Value>| async move { Json((*send).clone()) }
                }),
            )
            .route(
                "/bottoken/editMessageText",
                post({
                    let edit = edit.clone();
                    move |_body: Json<Value>| async move { Json((*edit).clone()) }
                }),
            );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.ok();
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn send_initial_returns_error_on_api_failure() {
        let base_url = spawn_error_server(
            json!({ "ok": false, "description": "Unauthorized" }),
            json!({ "ok": true }),
        )
        .await;
        let mut sender = make_sender(&base_url);
        let result = sender.send_initial("test").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unauthorized"));
    }

    #[tokio::test]
    async fn send_initial_returns_error_on_missing_result() {
        let base_url = spawn_error_server(json!({ "ok": true }), json!({ "ok": true })).await;
        let mut sender = make_sender(&base_url);
        let result = sender.send_initial("test").await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("missing result.message_id"));
    }

    #[tokio::test]
    async fn force_edit_returns_error_when_message_id_unset() {
        let (base_url, _requests) = spawn_mock_server().await;
        let mut sender = make_sender(&base_url);
        // message_id is None — force_edit returns uninitialized error
        let result = sender.edit("test").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not initialized"));
    }

    #[tokio::test]
    async fn force_edit_ignores_message_not_modified_error() {
        let base_url = spawn_error_server(
            json!({ "ok": true, "result": { "message_id": 456 } }),
            json!({ "ok": false, "description": "Bad Request: message is not modified" }),
        )
        .await;
        let mut sender = make_sender(&base_url);
        sender.message_id = Some(456);
        // Should succeed despite "message is not modified" error
        let result = sender.edit("same text").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn force_edit_returns_real_api_errors() {
        let base_url = spawn_error_server(
            json!({ "ok": true, "result": { "message_id": 456 } }),
            json!({ "ok": false, "description": "Too Many Requests: retry after 30" }),
        )
        .await;
        let mut sender = make_sender(&base_url);
        sender.message_id = Some(456);
        let result = sender.edit("test").await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Too Many Requests"));
    }
}
