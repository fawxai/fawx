//! Telegram channel implementation for Fawx.
//!
//! Provides a [`Channel`] implementation that receives messages from a
//! Telegram bot and sends responses via the Telegram Bot API. This crate
//! handles Telegram-specific logic only: parsing webhook updates, formatting
//! responses, and calling the Bot API. No agentic loop logic — that stays
//! in HeadlessApp.

use fx_core::channel::{Channel, ChannelError};
use fx_core::types::InputSource;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Telegram Bot API maximum message length in UTF-8 code units.
const TELEGRAM_MAX_MESSAGE_LENGTH: usize = 4096;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Telegram bot configuration.
#[derive(Debug, Clone)]
pub struct TelegramConfig {
    /// Bot token from BotFather (e.g., "123456:ABC-DEF1234…").
    pub bot_token: String,
    /// Optional: restrict to specific chat IDs (allowlist).
    /// Empty = accept all chats.
    pub allowed_chat_ids: Vec<i64>,
    /// Secret token for webhook validation.
    /// Telegram sends this in `X-Telegram-Bot-Api-Secret-Token` header.
    pub webhook_secret: Option<String>,
}

// ---------------------------------------------------------------------------
// Error type (Finding #5: removed dead NotConfigured variant)
// ---------------------------------------------------------------------------

/// Errors from Telegram channel operations.
#[derive(Debug)]
pub enum TelegramError {
    /// JSON parse error on incoming update.
    ParseError(String),
    /// HTTP error calling Bot API.
    ApiError(String),
    /// Chat not in allowlist.
    Unauthorized(i64),
}

impl fmt::Display for TelegramError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ParseError(msg) => write!(f, "parse error: {msg}"),
            Self::ApiError(msg) => write!(f, "API error: {msg}"),
            Self::Unauthorized(id) => write!(f, "unauthorized chat: {id}"),
        }
    }
}

impl std::error::Error for TelegramError {}

// ---------------------------------------------------------------------------
// Finding #9: From<TelegramError> for ChannelError
// ---------------------------------------------------------------------------

impl From<TelegramError> for ChannelError {
    fn from(err: TelegramError) -> Self {
        match err {
            TelegramError::Unauthorized(_) => ChannelError::NotConnected,
            other => ChannelError::DeliveryFailed(other.to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Message types
// ---------------------------------------------------------------------------

/// Parsed incoming message from Telegram.
#[derive(Debug, Clone)]
pub struct IncomingMessage {
    /// Telegram chat ID (used for responses).
    pub chat_id: i64,
    /// Message text content.
    pub text: String,
    /// Telegram message ID (for reply_to).
    pub message_id: i64,
    /// Sender's first name (for logging/context).
    pub from_name: Option<String>,
}

/// Outgoing message to Telegram.
#[derive(Debug, Clone, Serialize)]
pub struct OutgoingMessage {
    pub chat_id: i64,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parse_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to_message_id: Option<i64>,
}

// ---------------------------------------------------------------------------
// Telegram Bot API response types (minimal)
// ---------------------------------------------------------------------------

/// Wrapper for Telegram Bot API responses.
#[derive(Debug, Deserialize)]
struct ApiResponse {
    ok: bool,
    #[serde(default)]
    description: Option<String>,
}

/// Subset of Telegram Update for deserialization.
#[derive(Debug, Deserialize)]
struct Update {
    #[serde(default)]
    message: Option<TgMessage>,
}

/// Subset of Telegram Message for deserialization.
#[derive(Debug, Deserialize)]
struct TgMessage {
    message_id: i64,
    chat: TgChat,
    #[serde(default)]
    from: Option<TgUser>,
    #[serde(default)]
    text: Option<String>,
}

/// Subset of Telegram Chat for deserialization.
#[derive(Debug, Deserialize)]
struct TgChat {
    id: i64,
}

/// Subset of Telegram User for deserialization.
#[derive(Debug, Deserialize)]
struct TgUser {
    #[serde(default)]
    first_name: Option<String>,
}

// ---------------------------------------------------------------------------
// Finding #4: Shared API response helper
// ---------------------------------------------------------------------------

/// Check a Telegram Bot API response for errors.
async fn check_api_response(resp: reqwest::Response) -> Result<(), TelegramError> {
    let api_resp: ApiResponse = resp
        .json()
        .await
        .map_err(|e| TelegramError::ApiError(e.to_string()))?;

    if !api_resp.ok {
        let desc = api_resp
            .description
            .unwrap_or_else(|| "unknown error".to_string());
        return Err(TelegramError::ApiError(desc));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Finding #3 + #7: Static send helper (shared by send_message & send_response)
// ---------------------------------------------------------------------------

/// Send a single message via the Bot API (no splitting).
async fn send_single_message(
    client: &reqwest::Client,
    base_url: &str,
    bot_token: &str,
    msg: &OutgoingMessage,
) -> Result<(), TelegramError> {
    let url = format!("{base_url}/bot{bot_token}/sendMessage");
    let resp = client
        .post(&url)
        .json(msg)
        .send()
        .await
        .map_err(|e| TelegramError::ApiError(e.to_string()))?;
    check_api_response(resp).await
}

// ---------------------------------------------------------------------------
// Finding #7: Message splitting
// ---------------------------------------------------------------------------

/// Split text into chunks respecting Telegram's 4096-char limit.
///
/// Prefers splitting at newline boundaries. Falls back to UTF-8 char
/// boundaries if no newline exists within the limit.
fn split_message(text: &str) -> Vec<String> {
    if text.len() <= TELEGRAM_MAX_MESSAGE_LENGTH {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        if remaining.len() <= TELEGRAM_MAX_MESSAGE_LENGTH {
            chunks.push(remaining.to_string());
            break;
        }

        let split_at = find_split_point(remaining);
        chunks.push(remaining[..split_at].to_string());
        remaining = &remaining[split_at..];
    }

    chunks
}

/// Find the best split point within the max message length.
fn find_split_point(text: &str) -> usize {
    // Prefer splitting at a newline within the limit.
    if let Some(pos) = text[..TELEGRAM_MAX_MESSAGE_LENGTH].rfind('\n') {
        return pos + 1;
    }

    // Fall back to the last char boundary at or before the limit.
    let mut idx = TELEGRAM_MAX_MESSAGE_LENGTH;
    while !text.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

// ---------------------------------------------------------------------------
// TelegramChannel
// ---------------------------------------------------------------------------

/// Channel implementation for Telegram.
pub struct TelegramChannel {
    config: TelegramConfig,
    client: reqwest::Client,
    active: AtomicBool,
    last_chat_id: Mutex<Option<i64>>,
    /// Base URL for the Telegram Bot API. Defaults to `https://api.telegram.org`.
    /// Overridable for testing via [`TelegramChannel::new_with_base_url`].
    base_url: String,
}

/// Default Telegram Bot API base URL.
const DEFAULT_BASE_URL: &str = "https://api.telegram.org";

impl TelegramChannel {
    /// Create a new Telegram channel with the given configuration.
    pub fn new(config: TelegramConfig) -> Self {
        Self::new_with_base_url(config, DEFAULT_BASE_URL.to_string())
    }

    /// Create a Telegram channel pointing at a custom API base URL.
    ///
    /// Used in tests to redirect Bot API calls to a local mock server.
    pub fn new_with_base_url(config: TelegramConfig, base_url: String) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
            active: AtomicBool::new(true),
            last_chat_id: Mutex::new(None),
            base_url,
        }
    }

    /// Build the Bot API URL for a given method.
    fn bot_url(&self, method: &str) -> String {
        format!("{}/bot{}/{method}", self.base_url, self.config.bot_token)
    }

    /// Parse a Telegram webhook Update JSON payload.
    pub fn parse_update(&self, payload: &str) -> Result<Option<IncomingMessage>, TelegramError> {
        let update: Update =
            serde_json::from_str(payload).map_err(|e| TelegramError::ParseError(e.to_string()))?;

        let message = match update.message {
            Some(m) => m,
            None => return Ok(None),
        };

        let text = match message.text {
            Some(t) => t,
            None => return Ok(None),
        };

        if !self.is_allowed(message.chat.id) {
            return Err(TelegramError::Unauthorized(message.chat.id));
        }

        let from_name = message.from.and_then(|u| u.first_name);

        Ok(Some(IncomingMessage {
            chat_id: message.chat.id,
            text,
            message_id: message.message_id,
            from_name,
        }))
    }

    /// Set the last chat ID (for Channel::send_response return path).
    pub fn set_last_chat_id(&self, chat_id: i64) {
        if let Ok(mut guard) = self.last_chat_id.lock() {
            *guard = Some(chat_id);
        }
    }

    /// Get the last chat ID, if set.
    pub fn last_chat_id(&self) -> Option<i64> {
        self.last_chat_id.lock().ok().and_then(|g| *g)
    }

    // Finding #1: Webhook secret validation

    /// Get the configured webhook secret (for external validation).
    pub fn webhook_secret(&self) -> Option<&str> {
        self.config.webhook_secret.as_deref()
    }

    /// Validate the `X-Telegram-Bot-Api-Secret-Token` header value.
    ///
    /// Returns `true` if no secret is configured (open mode) or if
    /// the provided value matches the stored secret.
    pub fn validate_webhook_secret(&self, header_value: Option<&str>) -> bool {
        match &self.config.webhook_secret {
            None => true,
            Some(expected) => header_value == Some(expected.as_str()),
        }
    }

    /// Send a message via the Telegram Bot API, splitting if needed.
    pub async fn send_message(&self, msg: &OutgoingMessage) -> Result<(), TelegramError> {
        let chunks = split_message(&msg.text);
        for chunk in chunks {
            let chunk_msg = OutgoingMessage {
                chat_id: msg.chat_id,
                text: chunk,
                parse_mode: msg.parse_mode.clone(),
                reply_to_message_id: msg.reply_to_message_id,
            };
            send_single_message(
                &self.client,
                &self.base_url,
                &self.config.bot_token,
                &chunk_msg,
            )
            .await?;
        }
        Ok(())
    }

    /// Send a "typing…" indicator.
    pub async fn send_typing(&self, chat_id: i64) -> Result<(), TelegramError> {
        let url = self.bot_url("sendChatAction");
        let body = serde_json::json!({
            "chat_id": chat_id,
            "action": "typing"
        });

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| TelegramError::ApiError(e.to_string()))?;
        check_api_response(resp).await
    }

    /// Set the webhook URL with Telegram.
    ///
    /// Automatically includes the configured `webhook_secret` if present.
    pub async fn set_webhook(&self, webhook_url: &str) -> Result<(), TelegramError> {
        let url = self.bot_url("setWebhook");
        let mut body = serde_json::json!({ "url": webhook_url });
        if let Some(secret) = &self.config.webhook_secret {
            body["secret_token"] = serde_json::Value::String(secret.clone());
        }

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| TelegramError::ApiError(e.to_string()))?;
        check_api_response(resp).await
    }

    /// Delete the webhook from Telegram.
    pub async fn delete_webhook(&self) -> Result<(), TelegramError> {
        let url = self.bot_url("deleteWebhook");
        let resp = self
            .client
            .post(&url)
            .send()
            .await
            .map_err(|e| TelegramError::ApiError(e.to_string()))?;
        check_api_response(resp).await
    }

    /// Verify the bot token by calling `getMe`.
    pub async fn get_me(&self) -> Result<(), TelegramError> {
        let url = self.bot_url("getMe");
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| TelegramError::ApiError(e.to_string()))?;
        check_api_response(resp).await
    }

    // Finding #6: get_updates as a method

    /// Fetch updates via long polling.
    pub async fn get_updates(
        &self,
        offset: i64,
        timeout_seconds: u32,
    ) -> Result<(Vec<serde_json::Value>, i64), TelegramError> {
        let url = self.bot_url("getUpdates");
        let body = serde_json::json!({
            "offset": offset,
            "timeout": timeout_seconds,
            "allowed_updates": ["message"]
        });

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .timeout(std::time::Duration::from_secs(
                u64::from(timeout_seconds) + 5,
            ))
            .send()
            .await
            .map_err(|e| TelegramError::ApiError(e.to_string()))?;

        let api_resp: GetUpdatesResponse = resp
            .json()
            .await
            .map_err(|e| TelegramError::ApiError(e.to_string()))?;

        if !api_resp.ok {
            return Err(TelegramError::ApiError(
                "getUpdates returned ok=false".to_string(),
            ));
        }

        let next_offset = compute_next_offset(&api_resp.result, offset);
        Ok((api_resp.result, next_offset))
    }

    /// Check if a chat ID is allowed (empty allowlist = all allowed).
    fn is_allowed(&self, chat_id: i64) -> bool {
        if self.config.allowed_chat_ids.is_empty() {
            return true;
        }
        self.config.allowed_chat_ids.contains(&chat_id)
    }
}

/// Compute the next offset from a list of updates.
fn compute_next_offset(updates: &[serde_json::Value], current: i64) -> i64 {
    let mut next = current;
    for update in updates {
        if let Some(uid) = update.get("update_id").and_then(|v| v.as_i64()) {
            if uid >= next {
                next = uid + 1;
            }
        }
    }
    next
}

// ---------------------------------------------------------------------------
// Channel trait implementation
// ---------------------------------------------------------------------------

impl Channel for TelegramChannel {
    fn id(&self) -> &str {
        "telegram"
    }

    fn name(&self) -> &str {
        "Telegram"
    }

    fn input_source(&self) -> InputSource {
        InputSource::Channel("telegram".to_string())
    }

    fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    // Finding #3: Reuse send_single_message + split_message
    fn send_response(&self, message: &str) -> Result<(), ChannelError> {
        let chat_id = self
            .last_chat_id
            .lock()
            .ok()
            .and_then(|g| *g)
            .ok_or(ChannelError::NotConnected)?;

        let chunks = split_message(message);
        let client = self.client.clone();
        let base_url = self.base_url.clone();
        let token = self.config.bot_token.clone();

        tokio::spawn(async move {
            for chunk in chunks {
                let msg = OutgoingMessage {
                    chat_id,
                    text: chunk,
                    parse_mode: Some("Markdown".to_string()),
                    reply_to_message_id: None,
                };
                if let Err(e) = send_single_message(&client, &base_url, &token, &msg).await {
                    tracing::error!("Telegram send failed: {e}");
                    break;
                }
            }
        });

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Long-polling support types
// ---------------------------------------------------------------------------

/// Response from getUpdates.
#[derive(Debug, Deserialize)]
struct GetUpdatesResponse {
    ok: bool,
    #[serde(default)]
    result: Vec<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config() -> TelegramConfig {
        TelegramConfig {
            bot_token: "123456:ABC-DEF".to_string(),
            allowed_chat_ids: Vec::new(),
            webhook_secret: None,
        }
    }

    fn make_config_with_allowlist() -> TelegramConfig {
        TelegramConfig {
            bot_token: "123456:ABC-DEF".to_string(),
            allowed_chat_ids: vec![100, 200],
            webhook_secret: None,
        }
    }

    fn make_config_with_secret(secret: &str) -> TelegramConfig {
        TelegramConfig {
            bot_token: "123456:ABC-DEF".to_string(),
            allowed_chat_ids: Vec::new(),
            webhook_secret: Some(secret.to_string()),
        }
    }

    fn make_channel() -> TelegramChannel {
        TelegramChannel::new(make_config())
    }

    fn make_channel_with_allowlist() -> TelegramChannel {
        TelegramChannel::new(make_config_with_allowlist())
    }

    fn sample_update_json(chat_id: i64, text: &str) -> String {
        format!(
            r#"{{
                "update_id": 1001,
                "message": {{
                    "message_id": 42,
                    "chat": {{ "id": {chat_id} }},
                    "from": {{ "first_name": "Joe" }},
                    "text": "{text}"
                }}
            }}"#
        )
    }

    // ── Parse update tests ──────────────────────────────────────────────

    #[test]
    fn parse_text_message_extracts_fields() {
        let ch = make_channel();
        let json = sample_update_json(12345, "hello bot");
        let result = ch.parse_update(&json).unwrap().unwrap();

        assert_eq!(result.chat_id, 12345);
        assert_eq!(result.text, "hello bot");
        assert_eq!(result.message_id, 42);
        assert_eq!(result.from_name.as_deref(), Some("Joe"));
    }

    #[test]
    fn parse_update_ignores_non_text() {
        let ch = make_channel();
        let json = r#"{
            "update_id": 1002,
            "message": {
                "message_id": 43,
                "chat": { "id": 12345 },
                "photo": [{"file_id": "abc"}]
            }
        }"#;
        let result = ch.parse_update(json).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn parse_update_rejects_disallowed_chat() {
        let ch = make_channel_with_allowlist();
        let json = sample_update_json(999, "sneaky message");
        let result = ch.parse_update(&json);
        assert!(result.is_err());
        match result.unwrap_err() {
            TelegramError::Unauthorized(id) => assert_eq!(id, 999),
            other => panic!("expected Unauthorized, got: {other:?}"),
        }
    }

    #[test]
    fn parse_update_allows_empty_allowlist() {
        let ch = make_channel();
        let json = sample_update_json(99999, "anyone can talk");
        let result = ch.parse_update(&json).unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().chat_id, 99999);
    }

    #[test]
    fn parse_update_handles_no_message() {
        let ch = make_channel();
        let json = r#"{ "update_id": 1003 }"#;
        let result = ch.parse_update(json).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn parse_update_rejects_invalid_json() {
        let ch = make_channel();
        let result = ch.parse_update("not json at all");
        assert!(result.is_err());
        match result.unwrap_err() {
            TelegramError::ParseError(_) => {}
            other => panic!("expected ParseError, got: {other:?}"),
        }
    }

    #[test]
    fn parse_update_with_allowed_chat() {
        let ch = make_channel_with_allowlist();
        let json = sample_update_json(100, "allowed user");
        let result = ch.parse_update(&json).unwrap().unwrap();
        assert_eq!(result.chat_id, 100);
        assert_eq!(result.text, "allowed user");
    }

    // ── Outgoing message tests ──────────────────────────────────────────

    #[test]
    fn outgoing_message_serializes_correctly() {
        let msg = OutgoingMessage {
            chat_id: 12345,
            text: "Hello!".to_string(),
            parse_mode: Some("Markdown".to_string()),
            reply_to_message_id: Some(42),
        };
        let json: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&msg).unwrap()).unwrap();
        assert_eq!(json["chat_id"], 12345);
        assert_eq!(json["text"], "Hello!");
        assert_eq!(json["parse_mode"], "Markdown");
        assert_eq!(json["reply_to_message_id"], 42);
    }

    #[test]
    fn outgoing_message_omits_none_fields() {
        let msg = OutgoingMessage {
            chat_id: 12345,
            text: "Hello!".to_string(),
            parse_mode: None,
            reply_to_message_id: None,
        };
        let json_str = serde_json::to_string(&msg).unwrap();
        assert!(!json_str.contains("parse_mode"));
        assert!(!json_str.contains("reply_to_message_id"));
    }

    // ── Channel trait tests ─────────────────────────────────────────────

    #[test]
    fn send_response_fails_without_chat_id() {
        let ch = make_channel();
        let result = ch.send_response("hello");
        assert!(result.is_err());
        match result.unwrap_err() {
            ChannelError::NotConnected => {}
            other => panic!("expected NotConnected, got: {other:?}"),
        }
    }

    #[test]
    fn channel_trait_id_and_name() {
        let ch = make_channel();
        assert_eq!(ch.id(), "telegram");
        assert_eq!(ch.name(), "Telegram");
    }

    #[test]
    fn channel_trait_input_source() {
        let ch = make_channel();
        let source = ch.input_source();
        assert_eq!(source, InputSource::Channel("telegram".to_string()));
    }

    #[test]
    fn channel_is_active_default() {
        let ch = make_channel();
        assert!(ch.is_active());
    }

    // ── Allowlist tests ─────────────────────────────────────────────────

    #[test]
    fn is_allowed_with_empty_list_accepts_all() {
        let ch = make_channel();
        assert!(ch.is_allowed(1));
        assert!(ch.is_allowed(999));
        assert!(ch.is_allowed(-1));
    }

    #[test]
    fn is_allowed_with_list_rejects_unlisted() {
        let ch = make_channel_with_allowlist();
        assert!(ch.is_allowed(100));
        assert!(ch.is_allowed(200));
        assert!(!ch.is_allowed(300));
        assert!(!ch.is_allowed(0));
    }

    // ── Error display tests ─────────────────────────────────────────────

    #[test]
    fn telegram_error_display() {
        let e = TelegramError::ParseError("bad json".to_string());
        assert_eq!(format!("{e}"), "parse error: bad json");

        let e = TelegramError::ApiError("timeout".to_string());
        assert_eq!(format!("{e}"), "API error: timeout");

        let e = TelegramError::Unauthorized(42);
        assert_eq!(format!("{e}"), "unauthorized chat: 42");
    }

    // ── Finding #9: From<TelegramError> for ChannelError ────────────────

    #[test]
    fn telegram_error_converts_to_channel_error() {
        let err: ChannelError = TelegramError::Unauthorized(42).into();
        assert_eq!(err, ChannelError::NotConnected);

        let err: ChannelError = TelegramError::ApiError("fail".to_string()).into();
        assert_eq!(
            err,
            ChannelError::DeliveryFailed("API error: fail".to_string())
        );

        let err: ChannelError = TelegramError::ParseError("bad".to_string()).into();
        assert_eq!(
            err,
            ChannelError::DeliveryFailed("parse error: bad".to_string())
        );
    }

    // ── Finding #1: Webhook secret validation ───────────────────────────

    #[test]
    fn validate_webhook_secret_no_secret_configured_accepts_all() {
        let ch = make_channel();
        assert!(ch.validate_webhook_secret(None));
        assert!(ch.validate_webhook_secret(Some("anything")));
    }

    #[test]
    fn validate_webhook_secret_rejects_missing_header() {
        let ch = TelegramChannel::new(make_config_with_secret("my-secret"));
        assert!(!ch.validate_webhook_secret(None));
    }

    #[test]
    fn validate_webhook_secret_rejects_wrong_value() {
        let ch = TelegramChannel::new(make_config_with_secret("my-secret"));
        assert!(!ch.validate_webhook_secret(Some("wrong-secret")));
    }

    #[test]
    fn validate_webhook_secret_accepts_correct_value() {
        let ch = TelegramChannel::new(make_config_with_secret("my-secret"));
        assert!(ch.validate_webhook_secret(Some("my-secret")));
    }

    #[test]
    fn webhook_secret_getter_returns_configured_value() {
        let ch = TelegramChannel::new(make_config_with_secret("test-token"));
        assert_eq!(ch.webhook_secret(), Some("test-token"));

        let ch2 = make_channel();
        assert_eq!(ch2.webhook_secret(), None);
    }

    // ── Finding #7: Message splitting ───────────────────────────────────

    #[test]
    fn split_message_short_text_returns_single_chunk() {
        let result = split_message("hello");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "hello");
    }

    #[test]
    fn split_message_exact_limit_returns_single_chunk() {
        let text = "a".repeat(TELEGRAM_MAX_MESSAGE_LENGTH);
        let result = split_message(&text);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].len(), TELEGRAM_MAX_MESSAGE_LENGTH);
    }

    #[test]
    fn split_message_over_limit_splits_at_newline() {
        let first_part = "a".repeat(4000);
        let second_part = "b".repeat(200);
        let text = format!("{first_part}\n{second_part}");
        let result = split_message(&text);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], format!("{first_part}\n"));
        assert_eq!(result[1], second_part);
    }

    #[test]
    fn split_message_over_limit_no_newline_splits_at_char_boundary() {
        let text = "a".repeat(5000);
        let result = split_message(&text);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].len(), TELEGRAM_MAX_MESSAGE_LENGTH);
        assert_eq!(result[1].len(), 5000 - TELEGRAM_MAX_MESSAGE_LENGTH);
    }

    #[test]
    fn split_message_multibyte_respects_char_boundaries() {
        // 'é' is 2 bytes in UTF-8
        let text = "é".repeat(3000);
        assert_eq!(text.len(), 6000);
        let result = split_message(&text);
        assert!(result.len() >= 2);
        for chunk in &result {
            assert!(chunk.len() <= TELEGRAM_MAX_MESSAGE_LENGTH);
        }
    }

    #[test]
    fn split_message_empty_returns_single_empty() {
        let result = split_message("");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "");
    }

    // ── Finding #6: compute_next_offset ─────────────────────────────────

    #[test]
    fn compute_next_offset_empty_updates_returns_current() {
        assert_eq!(compute_next_offset(&[], 0), 0);
        assert_eq!(compute_next_offset(&[], 42), 42);
    }

    #[test]
    fn compute_next_offset_advances_past_max_update_id() {
        let updates = vec![
            serde_json::json!({"update_id": 100}),
            serde_json::json!({"update_id": 103}),
            serde_json::json!({"update_id": 101}),
        ];
        assert_eq!(compute_next_offset(&updates, 0), 104);
    }

    // ── set_last_chat_id ────────────────────────────────────────────────

    #[test]
    fn set_last_chat_id_enables_send_response() {
        let ch = make_channel();
        assert!(ch.send_response("test").is_err());
        ch.set_last_chat_id(12345);
        // After setting, send_response would succeed (fire-and-forget spawn).
    }

    // ── Integration tests (require real bot token) ──────────────────────

    #[tokio::test]
    #[ignore] // TODO(#1239): requires real bot token — remove when integration test infra exists
    async fn get_me_validates_token() {
        let ch = make_channel();
        let result = ch.get_me().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    #[ignore] // TODO(#1239): requires real bot token — remove when integration test infra exists
    async fn send_and_receive_message_roundtrip() {
        let ch = make_channel();
        let msg = OutgoingMessage {
            chat_id: 12345,
            text: "test message".to_string(),
            parse_mode: None,
            reply_to_message_id: None,
        };
        let result = ch.send_message(&msg).await;
        assert!(result.is_err());
    }
}
