//! Telegram channel implementation for Fawx.
//!
//! Provides a [`Channel`] implementation that receives messages from a
//! Telegram bot and sends responses via the Telegram Bot API. This crate
//! handles Telegram-specific logic only: parsing webhook updates, formatting
//! responses, and calling the Bot API. No agentic loop logic — that stays
//! in HeadlessApp.

pub mod progress;

pub use progress::build_telegram_progress_callback;

/// Redact Telegram bot tokens from error messages to prevent log leaks.
pub fn sanitize_telegram_error(error: &impl std::fmt::Display) -> String {
    let text = error.to_string();
    let mut result = String::with_capacity(text.len());
    let mut remaining = text.as_str();
    while let Some(start) = remaining.find("/bot") {
        result.push_str(&remaining[..start]);
        result.push_str("/bot<REDACTED>");
        let after_bot = &remaining[start + 4..];
        match after_bot.find('/') {
            Some(end) => remaining = &after_bot[end..],
            None => {
                remaining = "";
                break;
            }
        }
    }
    result.push_str(remaining);
    result
}

use fx_core::channel::{Channel, ChannelError, ResponseContext};
use fx_core::types::InputSource;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Telegram Bot API maximum message length in UTF-8 code units.
pub(crate) const TELEGRAM_MAX_MESSAGE_LENGTH: usize = 4096;

/// Slash commands registered with Telegram on startup via `setMyCommands`.
/// Keep in sync with `fx-cli/src/commands/slash.rs`.
const TELEGRAM_COMMANDS: &[(&str, &str)] = &[
    ("model", "Switch or list LLM models"),
    ("status", "Engine status and health"),
    ("budget", "Show token budget"),
    ("new", "Start a new conversation"),
    ("history", "Show conversation history"),
    ("thinking", "Toggle thinking mode"),
    ("config", "Show or reload configuration"),
    ("help", "Show available commands"),
];

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
    /// File system I/O error (e.g., saving downloaded media).
    IoError(String),
}

impl fmt::Display for TelegramError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ParseError(msg) => write!(f, "parse error: {msg}"),
            Self::ApiError(msg) => write!(f, "API error: {msg}"),
            Self::Unauthorized(id) => write!(f, "unauthorized chat: {id}"),
            Self::IoError(msg) => write!(f, "I/O error: {msg}"),
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
            other => ChannelError::DeliveryFailed(format!("{other}")),
        }
    }
}

// ---------------------------------------------------------------------------
// Message types
// ---------------------------------------------------------------------------

/// A photo attachment parsed from a Telegram message.
#[derive(Debug, Clone)]
pub struct PhotoAttachment {
    /// Telegram file ID (used to download the file).
    pub file_id: String,
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// MIME type of the photo (e.g., "image/jpeg").
    pub mime_type: String,
    /// Local file path after download (None until downloaded).
    pub file_path: Option<PathBuf>,
}

/// Parsed incoming message from Telegram.
#[derive(Debug, Clone)]
pub struct IncomingMessage {
    /// Telegram chat ID (used for responses).
    pub chat_id: i64,
    /// Message text content (caption if photo-only message).
    pub text: String,
    /// Telegram message ID (for reply_to).
    pub message_id: i64,
    /// Sender's first name (for logging/context).
    pub from_name: Option<String>,
    /// Photo attachments (empty if no photos).
    pub photos: Vec<PhotoAttachment>,
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

/// Internal queued Telegram response awaiting API delivery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedMessage {
    pub chat_id: i64,
    pub text: String,
    pub parse_mode: Option<String>,
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
    #[serde(default)]
    photo: Option<Vec<TgPhotoSize>>,
    #[serde(default)]
    caption: Option<String>,
}

/// Subset of Telegram PhotoSize for deserialization.
#[derive(Debug, Deserialize)]
struct TgPhotoSize {
    file_id: String,
    #[serde(default)]
    width: u32,
    #[serde(default)]
    height: u32,
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

/// Pick the largest photo from a Telegram photo array by pixel count.
///
/// Returns a single-element vec with the largest photo, or an empty vec
/// if no photos are present.
fn extract_largest_photo(message: &TgMessage) -> Vec<PhotoAttachment> {
    let sizes = match &message.photo {
        Some(sizes) if !sizes.is_empty() => sizes,
        _ => return Vec::new(),
    };

    let largest = sizes
        .iter()
        .max_by_key(|p| u64::from(p.width) * u64::from(p.height));

    match largest {
        Some(photo) => vec![PhotoAttachment {
            file_id: photo.file_id.clone(),
            width: photo.width,
            height: photo.height,
            mime_type: "image/jpeg".to_string(),
            file_path: None,
        }],
        None => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Telegram getFile API response types
// ---------------------------------------------------------------------------

/// Response from the `getFile` Bot API method.
#[derive(Debug, Deserialize)]
struct GetFileResponse {
    ok: bool,
    #[serde(default)]
    result: Option<TgFile>,
}

/// File metadata returned by `getFile`.
#[derive(Debug, Deserialize)]
struct TgFile {
    #[serde(default)]
    file_path: Option<String>,
}

/// Check a Telegram Bot API response for errors.
pub(crate) async fn check_api_response(resp: reqwest::Response) -> Result<(), TelegramError> {
    let api_resp: ApiResponse = resp
        .json()
        .await
        .map_err(|e| TelegramError::ApiError(sanitize_telegram_error(&e)))?;

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
        .map_err(|e| TelegramError::ApiError(sanitize_telegram_error(&e)))?;
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

#[derive(Debug, PartialEq, Eq)]
enum MarkdownSegment {
    Text(String),
    CodeBlock(String),
}

fn markdown_to_telegram_html(message: &str) -> String {
    split_markdown_segments(message)
        .into_iter()
        .map(render_markdown_segment)
        .collect::<Vec<_>>()
        .join("")
}

fn split_markdown_segments(message: &str) -> Vec<MarkdownSegment> {
    let mut segments = Vec::new();
    let mut cursor = 0;

    while let Some(rel_start) = message[cursor..].find("```") {
        let start = cursor + rel_start;
        if start > cursor {
            segments.push(MarkdownSegment::Text(message[cursor..start].to_string()));
        }
        if let Some((end, block)) = take_code_block(message, start) {
            segments.push(block);
            cursor = end;
            continue;
        }
        segments.push(MarkdownSegment::Text(message[start..].to_string()));
        return segments;
    }

    segments.push(MarkdownSegment::Text(message[cursor..].to_string()));
    segments
}

fn take_code_block(message: &str, start: usize) -> Option<(usize, MarkdownSegment)> {
    let content_start = start + 3;
    let rel_end = message[content_start..].find("```")?;
    let content_end = content_start + rel_end;
    let code = code_block_contents(&message[content_start..content_end]);
    let end = content_end + 3;
    Some((end, MarkdownSegment::CodeBlock(code.to_string())))
}

fn code_block_contents(block: &str) -> &str {
    let without_language = match block.find('\n') {
        Some(newline) => &block[newline + 1..],
        None => block,
    };
    without_language
        .strip_suffix('\n')
        .unwrap_or(without_language)
}

fn render_markdown_segment(segment: MarkdownSegment) -> String {
    match segment {
        MarkdownSegment::Text(text) => render_text_segment(&text),
        MarkdownSegment::CodeBlock(code) => format!("<pre>{}</pre>", escape_html(&code)),
    }
}

fn render_text_segment(text: &str) -> String {
    let escaped = escape_html(text);
    let (protected, codes) = protect_inline_code(&escaped);
    let bolded = bold_regex()
        .replace_all(&protected, "<b>$1</b>")
        .into_owned();
    let italicized = apply_italic_tags(&bolded);
    restore_code_tokens(italicized, codes)
}

fn protect_inline_code(text: &str) -> (String, Vec<String>) {
    let mut output = String::new();
    let mut codes = Vec::new();
    let mut cursor = 0;

    while let Some(rel_start) = text[cursor..].find('`') {
        let start = cursor + rel_start;
        output.push_str(&text[cursor..start]);
        let code_start = start + 1;
        let Some(rel_end) = text[code_start..].find('`') else {
            output.push_str(&text[start..]);
            return (output, codes);
        };
        let code_end = code_start + rel_end;
        if code_end == code_start {
            output.push('`');
            cursor = code_start;
            continue;
        }
        push_code_token(&mut output, &mut codes, &text[code_start..code_end]);
        cursor = code_end + 1;
    }

    output.push_str(&text[cursor..]);
    (output, codes)
}

fn push_code_token(output: &mut String, codes: &mut Vec<String>, code: &str) {
    let index = codes.len();
    output.push_str(&format!("\u{E000}CODE{index}\u{E001}"));
    codes.push(format!("<code>{code}</code>"));
}

fn restore_code_tokens(mut text: String, codes: Vec<String>) -> String {
    for (index, code) in codes.into_iter().enumerate() {
        text = text.replace(&format!("\u{E000}CODE{index}\u{E001}"), &code);
    }
    text
}

fn bold_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(r"\*\*(.+?)\*\*").expect("valid bold regex"))
}

fn apply_italic_tags(text: &str) -> String {
    let mut output = String::new();
    let mut cursor = 0;

    while let Some(rel_start) = text[cursor..].find('*') {
        let start = cursor + rel_start;
        output.push_str(&text[cursor..start]);
        if !is_single_asterisk(text, start) {
            output.push('*');
            cursor = start + 1;
            continue;
        }
        let Some(end) = find_italic_end(text, start + 1) else {
            output.push('*');
            cursor = start + 1;
            continue;
        };
        output.push_str("<i>");
        output.push_str(&text[start + 1..end]);
        output.push_str("</i>");
        cursor = end + 1;
    }

    output.push_str(&text[cursor..]);
    output
}

fn is_single_asterisk(text: &str, index: usize) -> bool {
    let bytes = text.as_bytes();
    bytes[index] == b'*'
        && (index == 0 || bytes[index - 1] != b'*')
        && (index + 1 == bytes.len() || bytes[index + 1] != b'*')
}

fn find_italic_end(text: &str, start: usize) -> Option<usize> {
    let mut cursor = start;
    while let Some(rel_index) = text[cursor..].find('*') {
        let index = cursor + rel_index;
        if index > start && is_single_asterisk(text, index) {
            return Some(index);
        }
        cursor = index + 1;
    }
    None
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

// ---------------------------------------------------------------------------
// TelegramChannel
// ---------------------------------------------------------------------------

/// Channel implementation for Telegram.
pub struct TelegramChannel {
    config: TelegramConfig,
    client: reqwest::Client,
    active: AtomicBool,
    outbound: Mutex<VecDeque<QueuedMessage>>,
    /// Base URL for the Telegram Bot API. Defaults to `https://api.telegram.org`.
    /// Overridable for testing via [`TelegramChannel::new_with_base_url`].
    base_url: String,
}

/// Default Telegram Bot API base URL.
pub(crate) const DEFAULT_BASE_URL: &str = "https://api.telegram.org";

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
            outbound: Mutex::new(VecDeque::new()),
            base_url,
        }
    }

    /// Build a Telegram-backed experiment progress callback for one
    /// `run_experiment` invocation in `chat_id`.
    ///
    /// Headless callers should install the returned callback on a short-lived
    /// `FawxToolExecutor` or per-call tool context, drop that callback when the
    /// tool returns, then await the join handle so the last buffered edit is
    /// flushed to Telegram.
    pub fn build_experiment_progress(
        &self,
        chat_id: i64,
    ) -> (fx_consensus::ProgressCallback, tokio::task::JoinHandle<()>) {
        progress::build_telegram_progress_callback_with_base_url(
            self.config.bot_token.clone(),
            chat_id,
            self.base_url.clone(),
        )
    }

    /// Build the Bot API URL for a given method.
    fn bot_url(&self, method: &str) -> String {
        format!("{}/bot{}/{method}", self.base_url, self.config.bot_token)
    }

    /// Parse a Telegram webhook Update JSON payload.
    ///
    /// Handles both text-only and photo messages. For photos, picks the
    /// largest resolution and uses the caption (if any) as the text.
    /// Returns `None` if the update has neither text nor photos.
    pub fn parse_update(&self, payload: &str) -> Result<Option<IncomingMessage>, TelegramError> {
        let update: Update =
            serde_json::from_str(payload).map_err(|e| TelegramError::ParseError(e.to_string()))?;

        let message = match update.message {
            Some(m) => m,
            None => return Ok(None),
        };

        let photos = extract_largest_photo(&message);
        let text = message.text.or(message.caption).unwrap_or_default();

        if text.is_empty() && photos.is_empty() {
            return Ok(None);
        }

        if !self.is_allowed(message.chat.id) {
            return Err(TelegramError::Unauthorized(message.chat.id));
        }

        let from_name = message.from.and_then(|u| u.first_name);

        Ok(Some(IncomingMessage {
            chat_id: message.chat.id,
            text,
            message_id: message.message_id,
            from_name,
            photos,
        }))
    }

    /// Take all queued outbound messages.
    pub fn drain_outbound(&self) -> Vec<QueuedMessage> {
        match self.outbound.lock() {
            Ok(mut queue) => queue.drain(..).collect(),
            Err(_) => Vec::new(),
        }
    }

    pub fn queue_response(
        &self,
        message: &str,
        context: &ResponseContext,
        parse_mode: Option<String>,
    ) -> Result<(), ChannelError> {
        let chat_id = parse_chat_id(context)?;
        let reply_to_message_id = parse_reply_to(context)?;
        self.queue_outbound(QueuedMessage {
            chat_id,
            text: message.to_string(),
            parse_mode,
            reply_to_message_id,
        })
    }

    fn queue_outbound(&self, message: QueuedMessage) -> Result<(), ChannelError> {
        const MAX_OUTBOUND_MESSAGES: usize = 100;
        let mut queue = self
            .outbound
            .lock()
            .map_err(|error| ChannelError::DeliveryFailed(error.to_string()))?;
        if queue.len() >= MAX_OUTBOUND_MESSAGES {
            queue.pop_front();
        }
        queue.push_back(message);
        Ok(())
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

    /// Download a file by `file_id` from Telegram servers.
    ///
    /// Calls `getFile` to obtain the server-side path, then downloads the
    /// file bytes and saves them to `dest_dir/<message_id>-<sanitized_file_id>.jpg`.
    pub async fn download_file(
        &self,
        file_id: &str,
        message_id: i64,
        dest_dir: &Path,
    ) -> Result<PathBuf, TelegramError> {
        let tg_path = self.fetch_file_path(file_id).await?;
        let url = format!(
            "{}/file/bot{}/{}",
            self.base_url, self.config.bot_token, tg_path
        );
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| TelegramError::ApiError(sanitize_telegram_error(&e)))?
            .error_for_status()
            .map_err(|e| TelegramError::ApiError(sanitize_telegram_error(&e)))?;
        let bytes = response
            .bytes()
            .await
            .map_err(|e| TelegramError::ApiError(sanitize_telegram_error(&e)))?;

        let safe_name = sanitize_file_id(file_id);
        let dest = dest_dir.join(format!("{message_id}-{safe_name}.jpg"));
        std::fs::write(&dest, &bytes).map_err(|e| TelegramError::IoError(e.to_string()))?;
        Ok(dest)
    }

    /// Fetch the server-side file path for a given `file_id`.
    async fn fetch_file_path(&self, file_id: &str) -> Result<String, TelegramError> {
        let url = self.bot_url("getFile");
        let body = serde_json::json!({ "file_id": file_id });
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| TelegramError::ApiError(sanitize_telegram_error(&e)))?;

        let file_resp: GetFileResponse = resp
            .json()
            .await
            .map_err(|e| TelegramError::ApiError(sanitize_telegram_error(&e)))?;

        if !file_resp.ok {
            return Err(TelegramError::ApiError("getFile failed".to_string()));
        }

        file_resp
            .result
            .and_then(|r| r.file_path)
            .ok_or_else(|| TelegramError::ApiError("no file_path in getFile response".to_string()))
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
            .map_err(|e| TelegramError::ApiError(sanitize_telegram_error(&e)))?;
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
            .map_err(|e| TelegramError::ApiError(sanitize_telegram_error(&e)))?;
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
            .map_err(|e| TelegramError::ApiError(sanitize_telegram_error(&e)))?;
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
            .map_err(|e| TelegramError::ApiError(sanitize_telegram_error(&e)))?;
        check_api_response(resp).await
    }

    /// Register slash commands with Telegram via `setMyCommands`.
    ///
    /// Called on startup to keep bot commands in sync with the codebase.
    /// Fire-and-forget: logs a warning on failure but does not block startup.
    pub async fn register_commands(&self) {
        let commands = serde_json::json!({
            "commands": TELEGRAM_COMMANDS.iter().map(|(cmd, desc)| {
                serde_json::json!({ "command": cmd, "description": desc })
            }).collect::<Vec<_>>()
        });

        let url = self.bot_url("setMyCommands");
        let result = async {
            let resp = self
                .client
                .post(&url)
                .json(&commands)
                .send()
                .await
                .map_err(|e| TelegramError::ApiError(sanitize_telegram_error(&e)))?;
            check_api_response(resp).await
        }
        .await;

        if let Err(e) = result {
            tracing::warn!("failed to register Telegram commands: {e}");
        }
    }

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
            .map_err(|e| TelegramError::ApiError(sanitize_telegram_error(&e)))?;

        let api_resp: GetUpdatesResponse = resp
            .json()
            .await
            .map_err(|e| TelegramError::ApiError(sanitize_telegram_error(&e)))?;

        if !api_resp.ok {
            return Err(TelegramError::ApiError(format!(
                "getUpdates failed: {}",
                api_resp.description.as_deref().unwrap_or("unknown error")
            )));
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

/// Sanitize a Telegram file_id for use as a filename.
///
/// Replaces any character that is not alphanumeric, `-`, or `_` with `_`.
fn sanitize_file_id(file_id: &str) -> String {
    file_id
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
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

fn parse_chat_id(context: &ResponseContext) -> Result<i64, ChannelError> {
    let value = context
        .routing_key
        .as_deref()
        .ok_or(ChannelError::NotConnected)?;
    value.parse::<i64>().map_err(|error| {
        ChannelError::DeliveryFailed(format!("invalid telegram chat_id `{value}`: {error}"))
    })
}

fn parse_reply_to(context: &ResponseContext) -> Result<Option<i64>, ChannelError> {
    let Some(value) = context.reply_to.as_deref() else {
        return Ok(None);
    };
    let parsed = value.parse::<i64>().map_err(|error| {
        ChannelError::DeliveryFailed(format!("invalid telegram reply_to `{value}`: {error}"))
    })?;
    Ok(Some(parsed))
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

    fn send_response(&self, message: &str, context: &ResponseContext) -> Result<(), ChannelError> {
        let html = markdown_to_telegram_html(message);
        self.queue_response(&html, context, Some("HTML".to_string()))
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
    description: Option<String>,
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
                    "from": {{ "first_name": "Example" }},
                    "text": "{text}"
                }}
            }}"#
        )
    }

    fn assert_markdown_to_html(input: &str, expected: &str) {
        assert_eq!(markdown_to_telegram_html(input), expected);
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
        assert_eq!(result.from_name.as_deref(), Some("Example"));
    }

    #[test]
    fn parse_update_handles_photo_without_text() {
        let ch = make_channel();
        let json = r#"{
            "update_id": 1002,
            "message": {
                "message_id": 43,
                "chat": { "id": 12345 },
                "photo": [
                    {"file_id": "small", "width": 90, "height": 90},
                    {"file_id": "large", "width": 800, "height": 600}
                ]
            }
        }"#;
        let result = ch.parse_update(json).unwrap().unwrap();
        assert_eq!(result.photos.len(), 1);
        assert_eq!(result.photos[0].file_id, "large");
        assert_eq!(result.photos[0].width, 800);
        assert_eq!(result.photos[0].height, 600);
        assert!(result.text.is_empty());
    }

    #[test]
    fn parse_update_ignores_no_text_no_photo() {
        let ch = make_channel();
        // A message with neither text nor photo should be skipped.
        let json = r#"{
            "update_id": 1002,
            "message": {
                "message_id": 43,
                "chat": { "id": 12345 }
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

    // ── Photo parsing tests ───────────────────────────────────────────────

    #[test]
    fn parse_update_photo_with_caption() {
        let ch = make_channel();
        let json = r#"{
            "update_id": 1003,
            "message": {
                "message_id": 44,
                "chat": { "id": 12345 },
                "from": { "first_name": "Example" },
                "photo": [
                    {"file_id": "thumb", "width": 90, "height": 90},
                    {"file_id": "medium", "width": 320, "height": 240},
                    {"file_id": "full", "width": 1280, "height": 960}
                ],
                "caption": "Check this out"
            }
        }"#;
        let result = ch.parse_update(json).unwrap().unwrap();
        assert_eq!(result.text, "Check this out");
        assert_eq!(result.photos.len(), 1);
        assert_eq!(result.photos[0].file_id, "full");
        assert_eq!(result.photos[0].width, 1280);
        assert_eq!(result.photos[0].height, 960);
        assert_eq!(result.photos[0].mime_type, "image/jpeg");
        assert!(result.photos[0].file_path.is_none());
    }

    #[test]
    fn parse_update_selects_largest_photo_by_area() {
        let ch = make_channel();
        // Tall-and-narrow vs short-and-wide: area decides.
        let json = r#"{
            "update_id": 1004,
            "message": {
                "message_id": 45,
                "chat": { "id": 12345 },
                "photo": [
                    {"file_id": "tall", "width": 100, "height": 1000},
                    {"file_id": "wide", "width": 1000, "height": 200}
                ]
            }
        }"#;
        let result = ch.parse_update(json).unwrap().unwrap();
        assert_eq!(result.photos.len(), 1);
        // wide: 1000*200 = 200_000 > tall: 100*1000 = 100_000
        assert_eq!(result.photos[0].file_id, "wide");
    }

    #[test]
    fn parse_update_text_message_has_empty_photos() {
        let ch = make_channel();
        let json = sample_update_json(12345, "just text");
        let result = ch.parse_update(&json).unwrap().unwrap();
        assert!(result.photos.is_empty());
        assert_eq!(result.text, "just text");
    }

    #[test]
    fn extract_largest_photo_empty_array() {
        let message = TgMessage {
            message_id: 1,
            chat: TgChat { id: 1 },
            from: None,
            text: None,
            photo: Some(Vec::new()),
            caption: None,
        };
        let photos = extract_largest_photo(&message);
        assert!(photos.is_empty());
    }

    #[test]
    fn extract_largest_photo_none() {
        let message = TgMessage {
            message_id: 1,
            chat: TgChat { id: 1 },
            from: None,
            text: None,
            photo: None,
            caption: None,
        };
        let photos = extract_largest_photo(&message);
        assert!(photos.is_empty());
    }

    #[test]
    fn sanitize_file_id_removes_special_chars() {
        assert_eq!(sanitize_file_id("AgACAgIAAx0Cf"), "AgACAgIAAx0Cf");
        assert_eq!(sanitize_file_id("file/path.jpg"), "file_path_jpg");
        assert_eq!(sanitize_file_id("a-b_c"), "a-b_c");
    }

    // ── Download file tests ─────────────────────────────────────────────

    #[tokio::test]
    async fn download_file_fetches_and_saves() {
        use axum::routing::any;
        use tokio::net::TcpListener;

        // Mock server that handles getFile and file download.
        let app = axum::Router::new()
            .route(
                "/bot000000:TESTTOKEN/getFile",
                any(|| async {
                    axum::Json(serde_json::json!({
                        "ok": true,
                        "result": { "file_path": "photos/test.jpg" }
                    }))
                }),
            )
            .route(
                "/file/bot000000:TESTTOKEN/photos/test.jpg",
                any(|| async { vec![0xFF_u8, 0xD8, 0xFF, 0xE0] }),
            );
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind mock");
        let addr = listener.local_addr().expect("addr");
        let base_url = format!("http://{addr}");
        tokio::spawn(async move {
            axum::serve(listener, app).await.ok();
        });

        let config = TelegramConfig {
            bot_token: "000000:TESTTOKEN".to_string(),
            allowed_chat_ids: Vec::new(),
            webhook_secret: None,
        };
        let ch = TelegramChannel::new_with_base_url(config, base_url);

        let tmp = tempfile::tempdir().expect("tempdir");
        let result = ch.download_file("AgACAgIAA", 42, tmp.path()).await;
        assert!(result.is_ok());

        let path = result.unwrap();
        assert!(path.exists());
        assert_eq!(path.file_name().unwrap(), "42-AgACAgIAA.jpg");
        assert_eq!(std::fs::read(&path).unwrap(), vec![0xFF, 0xD8, 0xFF, 0xE0]);
    }

    #[tokio::test]
    async fn download_file_handles_api_error() {
        use axum::routing::any;
        use tokio::net::TcpListener;

        let app = axum::Router::new().route(
            "/bot000000:TESTTOKEN/getFile",
            any(|| async {
                axum::Json(serde_json::json!({
                    "ok": false,
                    "description": "Bad Request: invalid file_id"
                }))
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind mock");
        let addr = listener.local_addr().expect("addr");
        let base_url = format!("http://{addr}");
        tokio::spawn(async move {
            axum::serve(listener, app).await.ok();
        });

        let config = TelegramConfig {
            bot_token: "000000:TESTTOKEN".to_string(),
            allowed_chat_ids: Vec::new(),
            webhook_secret: None,
        };
        let ch = TelegramChannel::new_with_base_url(config, base_url);

        let tmp = tempfile::tempdir().expect("tempdir");
        let result = ch.download_file("bad-id", 99, tmp.path()).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            TelegramError::ApiError(msg) => assert!(msg.contains("getFile failed")),
            other => panic!("expected ApiError, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn download_file_rejects_non_200_response() {
        use axum::http::StatusCode;
        use axum::routing::any;
        use tokio::net::TcpListener;

        let app = axum::Router::new()
            .route(
                "/bot000000:TESTTOKEN/getFile",
                any(|| async {
                    axum::Json(serde_json::json!({
                        "ok": true,
                        "result": { "file_path": "photos/missing.jpg" }
                    }))
                }),
            )
            .route(
                "/file/bot000000:TESTTOKEN/photos/missing.jpg",
                any(|| async { StatusCode::NOT_FOUND }),
            );
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind mock");
        let addr = listener.local_addr().expect("addr");
        let base_url = format!("http://{addr}");
        tokio::spawn(async move {
            axum::serve(listener, app).await.ok();
        });

        let config = TelegramConfig {
            bot_token: "000000:TESTTOKEN".to_string(),
            allowed_chat_ids: Vec::new(),
            webhook_secret: None,
        };
        let ch = TelegramChannel::new_with_base_url(config, base_url);

        let tmp = tempfile::tempdir().expect("tempdir");
        let result = ch.download_file("some-file-id", 50, tmp.path()).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            TelegramError::ApiError(msg) => {
                assert!(msg.contains("404"), "expected 404 in error: {msg}");
            }
            other => panic!("expected ApiError for non-200, got: {other:?}"),
        }
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

    #[test]
    fn markdown_to_html_converts_bold() {
        assert_markdown_to_html("**hello**", "<b>hello</b>");
    }

    #[test]
    fn markdown_to_html_converts_italic() {
        assert_markdown_to_html("*hello*", "<i>hello</i>");
    }

    #[test]
    fn markdown_to_html_converts_inline_code() {
        assert_markdown_to_html("`foo`", "<code>foo</code>");
    }

    #[test]
    fn markdown_to_html_converts_code_block() {
        assert_markdown_to_html("```\ncode\n```", "<pre>code</pre>");
    }

    #[test]
    fn markdown_to_html_escapes_html_in_plain_text() {
        assert_markdown_to_html("<script>", "&lt;script&gt;");
    }

    #[test]
    fn markdown_to_html_handles_mixed_formatting() {
        assert_markdown_to_html(
            "**hello** `foo` world",
            "<b>hello</b> <code>foo</code> world",
        );
    }

    #[test]
    fn markdown_to_html_leaves_plain_text_unchanged() {
        assert_markdown_to_html("just plain text", "just plain text");
    }

    #[test]
    fn markdown_to_html_keeps_code_blocks_literal() {
        assert_markdown_to_html("```\n**hello**\n```", "<pre>**hello**</pre>");
    }

    #[test]
    fn markdown_to_html_handles_bold_with_inline_code_inside() {
        assert_markdown_to_html(
            "**bold with `code` inside**",
            "<b>bold with <code>code</code> inside</b>",
        );
    }

    #[test]
    fn markdown_to_html_returns_empty_string_for_empty_input() {
        assert_markdown_to_html("", "");
    }

    #[test]
    fn markdown_to_html_escapes_ampersands() {
        assert_markdown_to_html("AT&T", "AT&amp;T");
    }

    #[test]
    fn markdown_to_html_leaves_unclosed_bold_as_plain_text() {
        assert_markdown_to_html("**hello", "**hello");
    }

    #[test]
    fn markdown_to_html_leaves_unclosed_inline_code_as_plain_text() {
        assert_markdown_to_html("`hello", "`hello");
    }

    #[test]
    fn markdown_to_html_leaves_unclosed_code_block_as_plain_text() {
        assert_markdown_to_html("```\nhello", "```\nhello");
    }

    #[test]
    fn markdown_to_html_strips_code_block_language_tag() {
        assert_markdown_to_html("```rust\nlet x = 1;\n```", "<pre>let x = 1;</pre>");
    }

    #[test]
    fn markdown_to_html_double_escapes_existing_html_entities() {
        assert_markdown_to_html("&amp; &lt;", "&amp;amp; &amp;lt;");
    }

    // ── Channel trait tests ─────────────────────────────────────────────

    #[test]
    fn send_response_fails_without_routing_key() {
        let ch = make_channel();
        let result = ch.send_response("hello", &ResponseContext::default());
        assert!(result.is_err());
        match result.unwrap_err() {
            ChannelError::NotConnected => {}
            other => panic!("expected NotConnected, got: {other:?}"),
        }
    }

    #[test]
    fn send_response_queues_html_formatted_message() {
        let ch = make_channel();
        let context = ResponseContext {
            routing_key: Some("12345".to_string()),
            reply_to: Some("99".to_string()),
        };
        ch.send_response("**hello**", &context)
            .expect("queue response");

        let outbound = ch.drain_outbound();
        assert_eq!(
            outbound,
            vec![QueuedMessage {
                chat_id: 12345,
                text: "<b>hello</b>".to_string(),
                parse_mode: Some("HTML".to_string()),
                reply_to_message_id: Some(99),
            }]
        );
    }

    #[test]
    fn queue_response_allows_plain_text_messages() {
        let ch = make_channel();
        let context = ResponseContext {
            routing_key: Some("12345".to_string()),
            reply_to: Some("99".to_string()),
        };
        ch.queue_response("hello_error", &context, None)
            .expect("queue response");

        let outbound = ch.drain_outbound();
        assert_eq!(outbound[0].parse_mode, None);
    }

    #[test]
    fn send_response_rejects_invalid_routing_key() {
        let ch = make_channel();
        let context = ResponseContext {
            routing_key: Some("not-a-chat".to_string()),
            reply_to: None,
        };

        let result = ch.send_response("hello", &context);
        assert!(matches!(result, Err(ChannelError::DeliveryFailed(_))));
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

        let e = TelegramError::IoError("disk full".to_string());
        assert_eq!(format!("{e}"), "I/O error: disk full");
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

        let err: ChannelError = TelegramError::IoError("disk full".to_string()).into();
        assert_eq!(
            err,
            ChannelError::DeliveryFailed("I/O error: disk full".to_string())
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

    // ── outbound queue ─────────────────────────────────────────────────

    #[test]
    fn drain_outbound_clears_queue() {
        let ch = make_channel();
        let context = ResponseContext {
            routing_key: Some("12345".to_string()),
            reply_to: None,
        };
        ch.send_response("first", &context).expect("first response");

        assert_eq!(ch.drain_outbound().len(), 1);
        assert!(ch.drain_outbound().is_empty());
    }

    #[test]
    fn outbound_queue_is_bounded_to_one_hundred_messages() {
        let ch = make_channel();
        let context = ResponseContext {
            routing_key: Some("12345".to_string()),
            reply_to: None,
        };

        for idx in 0..101 {
            ch.send_response(&format!("msg-{idx}"), &context)
                .expect("queued response");
        }

        let outbound = ch.drain_outbound();
        assert_eq!(outbound.len(), 100);
        assert_eq!(outbound.first().map(|msg| msg.text.as_str()), Some("msg-1"));
        assert_eq!(
            outbound.last().map(|msg| msg.text.as_str()),
            Some("msg-100")
        );
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

    #[test]
    fn get_updates_response_deserializes_description() {
        let json = r#"{"ok": false, "description": "Unauthorized"}"#;
        let resp: GetUpdatesResponse = serde_json::from_str(json).expect("should deserialize");
        assert!(!resp.ok);
        assert_eq!(resp.description.as_deref(), Some("Unauthorized"));
        assert!(resp.result.is_empty());
    }

    #[test]
    fn get_updates_response_missing_description_defaults_none() {
        let json = r#"{"ok": true, "result": []}"#;
        let resp: GetUpdatesResponse = serde_json::from_str(json).expect("should deserialize");
        assert!(resp.ok);
        assert!(resp.description.is_none());
    }

    #[tokio::test]
    async fn register_commands_sends_set_my_commands() {
        use axum::routing::post;
        use std::sync::Arc;
        use tokio::net::TcpListener;
        use tokio::sync::Mutex;

        let captured = Arc::new(Mutex::new(None::<serde_json::Value>));
        let captured_clone = captured.clone();

        let app = axum::Router::new().route(
            "/bot123456:ABC-DEF/setMyCommands",
            post(move |axum::Json(body): axum::Json<serde_json::Value>| {
                let cap = captured_clone.clone();
                async move {
                    *cap.lock().await = Some(body);
                    axum::Json(serde_json::json!({ "ok": true, "result": true }))
                }
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.ok();
        });

        let ch = TelegramChannel::new_with_base_url(make_config(), format!("http://{addr}"));
        ch.register_commands().await;

        let body = captured.lock().await;
        let body = body.as_ref().expect("should have received request");
        let commands = body["commands"].as_array().expect("commands array");
        assert_eq!(commands.len(), TELEGRAM_COMMANDS.len());
        // Verify first and last command match
        assert_eq!(commands[0]["command"], TELEGRAM_COMMANDS[0].0);
        assert_eq!(commands[0]["description"], TELEGRAM_COMMANDS[0].1);
        let last = TELEGRAM_COMMANDS.len() - 1;
        assert_eq!(commands[last]["command"], TELEGRAM_COMMANDS[last].0);
    }

    #[tokio::test]
    async fn register_commands_logs_warning_on_api_failure() {
        use axum::routing::post;
        use tokio::net::TcpListener;

        let app = axum::Router::new().route(
            "/bot123456:ABC-DEF/setMyCommands",
            post(|| async {
                axum::Json(serde_json::json!({
                    "ok": false,
                    "description": "Unauthorized"
                }))
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.ok();
        });

        let ch = TelegramChannel::new_with_base_url(make_config(), format!("http://{addr}"));
        // Should not panic — fire-and-forget with warning
        ch.register_commands().await;
    }

    #[test]
    fn telegram_commands_all_have_nonempty_descriptions() {
        for (cmd, desc) in TELEGRAM_COMMANDS {
            assert!(!cmd.is_empty(), "command name is empty");
            assert!(!desc.is_empty(), "description for /{cmd} is empty");
            assert!(
                !cmd.starts_with('/'),
                "/{cmd} should not include the slash prefix"
            );
        }
    }

    #[test]
    fn sanitize_telegram_error_redacts_bot_token() {
        let error = "error for url (https://api.telegram.org/bot123456:ABC-DEF/getUpdates)";
        let sanitized = sanitize_telegram_error(&error);
        assert!(!sanitized.contains("123456:ABC-DEF"));
        assert!(sanitized.contains("/bot<REDACTED>/getUpdates"));
    }

    #[test]
    fn sanitize_telegram_error_preserves_non_telegram_errors() {
        let error = "connection refused";
        assert_eq!(sanitize_telegram_error(&error), "connection refused");
    }

    #[test]
    fn api_error_from_reqwest_style_error_is_sanitized_at_construction() {
        // Simulate what happens when reqwest returns an error containing a bot
        // token URL — the token should already be redacted inside the ApiError
        // because construction sites call sanitize_telegram_error.
        let fake_reqwest_error =
            "error sending request for url (https://api.telegram.org/bot123456:ABC-DEF/getUpdates): connection reset";
        let err = TelegramError::ApiError(sanitize_telegram_error(&fake_reqwest_error));
        let displayed = format!("{err}");
        assert!(
            !displayed.contains("123456:ABC-DEF"),
            "token leaked in display: {displayed}"
        );
        assert!(
            displayed.contains("/bot<REDACTED>/getUpdates"),
            "expected redacted URL in: {displayed}"
        );
    }

    #[test]
    fn sanitize_telegram_error_handles_multiple_occurrences() {
        let error = "tried /bot111:AAA/send then /bot222:BBB/edit";
        let sanitized = sanitize_telegram_error(&error);
        assert!(!sanitized.contains("111:AAA"));
        assert!(!sanitized.contains("222:BBB"));
    }
}
