# Telegram Channel — fx-channel-telegram

**Status:** Draft
**Wave:** 8 (Phase 1 — Telegram integration)
**Depends on:** Channel trait (#1221, merged), HTTP server (http_serve.rs, merged)

---

## Goal

Give Fawx a Telegram bot interface so users can chat with their Fawx instance via Telegram. This is the first real chat channel — what makes Fawx a product, not just a CLI.

## Architecture

```
Telegram Cloud
    │
    │ HTTPS webhook POST
    ▼
fawx serve --http (axum)
    │
    │ /telegram/webhook endpoint
    ▼
fx-channel-telegram (this crate)
    │ parse Update → extract message
    │ route through HeadlessApp::process_message()
    │ format response → Telegram Bot API
    ▼
Telegram Cloud (sendMessage)
```

The crate handles Telegram-specific logic only: parsing updates, formatting responses, calling the Bot API. No agentic loop logic — that stays in HeadlessApp.

## New Crate: `engine/crates/fx-channel-telegram/`

### Dependencies (minimal)

```toml
[dependencies]
fx-core = { path = "../fx-core" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
reqwest = { version = "0.12", features = ["json"], default-features = false }
tokio = { version = "1", features = ["sync"] }
tracing = "0.1"
```

Note: `reqwest` is already a workspace dependency (used by fx-marketplace). Use the same version.

### Public API

```rust
// ── Types ───────────────────────────────────────────────────────────────

/// Telegram bot configuration.
#[derive(Debug, Clone)]
pub struct TelegramConfig {
    /// Bot token from BotFather (e.g., "123456:ABC-DEF1234ghIkl-zyx57W2v1u123ew11").
    pub bot_token: String,
    /// Optional: restrict to specific chat IDs (allowlist).
    /// Empty = accept all chats.
    pub allowed_chat_ids: Vec<i64>,
}

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
    pub parse_mode: Option<String>,
    pub reply_to_message_id: Option<i64>,
}

// ── TelegramChannel ─────────────────────────────────────────────────────

/// Channel implementation for Telegram.
///
/// Implements `fx_core::channel::Channel` and handles:
/// - Parsing webhook Update payloads
/// - Sending responses via Bot API
/// - Chat ID allowlisting
pub struct TelegramChannel {
    config: TelegramConfig,
    /// HTTP client for Bot API calls (sendMessage).
    client: reqwest::Client,
    /// Active state.
    active: AtomicBool,
    /// Last chat_id that sent a message (for send_response return path).
    last_chat_id: Mutex<Option<i64>>,
}

impl TelegramChannel {
    pub fn new(config: TelegramConfig) -> Self;

    /// Parse a Telegram webhook Update JSON payload.
    /// Returns None if the update has no text message or is from a
    /// disallowed chat.
    pub fn parse_update(&self, payload: &str) -> Result<Option<IncomingMessage>, TelegramError>;

    /// Send a message via the Telegram Bot API.
    /// POST https://api.telegram.org/bot<token>/sendMessage
    pub async fn send_message(&self, msg: OutgoingMessage) -> Result<(), TelegramError>;

    /// Send a "typing..." indicator.
    /// POST https://api.telegram.org/bot<token>/sendChatAction
    pub async fn send_typing(&self, chat_id: i64) -> Result<(), TelegramError>;

    /// Set the webhook URL with Telegram.
    /// POST https://api.telegram.org/bot<token>/setWebhook
    pub async fn set_webhook(&self, url: &str) -> Result<(), TelegramError>;

    /// Check if a chat ID is allowed (empty allowlist = all allowed).
    fn is_allowed(&self, chat_id: i64) -> bool;
}
```

### Channel trait implementation

```rust
impl Channel for TelegramChannel {
    fn id(&self) -> &str { "telegram" }
    fn name(&self) -> &str { "Telegram" }
    fn input_source(&self) -> InputSource { InputSource::Channel("telegram".to_string()) }
    fn is_active(&self) -> bool { self.active.load(Ordering::Relaxed) }

    fn send_response(&self, message: &str) -> Result<(), ChannelError> {
        // Get last_chat_id, format OutgoingMessage, spawn blocking send.
        // This is synchronous Channel trait — use tokio::spawn for the async HTTP call.
        let chat_id = self.last_chat_id.lock()
            .ok()
            .and_then(|g| *g)
            .ok_or(ChannelError::NotConnected)?;

        let msg = OutgoingMessage {
            chat_id,
            text: message.to_string(),
            parse_mode: Some("Markdown".to_string()),
            reply_to_message_id: None,
        };

        // Fire-and-forget via tokio runtime.
        // Error logged via tracing, not propagated (Channel::send_response is sync).
        let client = self.client.clone();
        let token = self.config.bot_token.clone();
        tokio::spawn(async move {
            if let Err(e) = send_telegram_message(&client, &token, &msg).await {
                tracing::error!("Telegram send failed: {e}");
            }
        });

        Ok(())
    }
}
```

### Error type

```rust
#[derive(Debug)]
pub enum TelegramError {
    /// JSON parse error on incoming update.
    ParseError(String),
    /// HTTP error calling Bot API.
    ApiError(String),
    /// Chat not in allowlist.
    Unauthorized(i64),
    /// Bot token not configured.
    NotConfigured,
}
```

## Config Addition (fx-config)

Add to `FawxConfig`:

```rust
pub struct FawxConfig {
    // ... existing fields ...
    pub telegram: TelegramChannelConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct TelegramChannelConfig {
    /// Whether the Telegram channel is enabled.
    pub enabled: bool,
    /// Bot token (from BotFather). Stored encrypted via credential store.
    /// Can also be set via FAWX_TELEGRAM_TOKEN env var.
    pub bot_token: Option<String>,
    /// Restrict to specific Telegram chat IDs. Empty = accept all.
    pub allowed_chat_ids: Vec<i64>,
}
```

**IMPORTANT:** The bot token should use the encrypted credential store (fx-auth, PR #953). Config file stores a reference or the token is set via `/auth telegram set-token <token>`. The `bot_token` field in config.toml is a fallback — prefer encrypted storage.

## HTTP Server Integration (http_serve.rs)

Add a webhook endpoint to the existing axum router:

```rust
// In build_router():
let router = Router::new()
    .route("/health", get(handle_health))
    .route("/message", post(handle_message))   // existing
    .route("/status", get(handle_status))       // existing
    .route("/telegram/webhook", post(handle_telegram_webhook))  // NEW
    // ...
```

### Webhook handler

```rust
async fn handle_telegram_webhook(
    State(state): State<HttpState>,
    body: String,  // Raw JSON — Telegram sends application/json
) -> impl IntoResponse {
    let telegram = match &state.telegram {
        Some(t) => t,
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    // Parse the update
    let incoming = match telegram.parse_update(&body) {
        Ok(Some(msg)) => msg,
        Ok(None) => return StatusCode::OK.into_response(),  // Not a text message, ignore
        Err(e) => {
            tracing::warn!("Telegram parse error: {e:?}");
            return StatusCode::OK.into_response();  // Always 200 to Telegram
        }
    };

    tracing::info!(
        chat_id = incoming.chat_id,
        from = ?incoming.from_name,
        "Telegram message received"
    );

    // Send typing indicator
    let _ = telegram.send_typing(incoming.chat_id).await;

    // Update last_chat_id for Channel::send_response return path
    if let Ok(mut guard) = telegram.last_chat_id.lock() {
        *guard = Some(incoming.chat_id);
    }

    // Process through the agentic loop
    let mut app = state.app.lock().await;
    match app.process_message(&incoming.text).await {
        Ok(result) => {
            let response = OutgoingMessage {
                chat_id: incoming.chat_id,
                text: result.response,
                parse_mode: Some("Markdown".to_string()),
                reply_to_message_id: Some(incoming.message_id),
            };
            if let Err(e) = telegram.send_message(response).await {
                tracing::error!("Failed to send Telegram response: {e:?}");
            }
        }
        Err(e) => {
            tracing::error!("Loop error: {e}");
            let error_msg = OutgoingMessage {
                chat_id: incoming.chat_id,
                text: format!("⚠️ Error: {e}"),
                parse_mode: None,
                reply_to_message_id: Some(incoming.message_id),
            };
            let _ = telegram.send_message(error_msg).await;
        }
    }

    StatusCode::OK.into_response()
}
```

### Webhook registration

On startup, if Telegram is enabled, auto-register the webhook with Telegram:

```rust
// In run() after binding:
if let Some(telegram) = &state.telegram {
    let webhook_url = format!("https://{tailscale_hostname}:{port}/telegram/webhook");
    // NOTE: Telegram requires HTTPS. Tailscale Serve or a reverse proxy is needed.
    // For Phase 0 dogfooding, long polling is simpler (no HTTPS requirement).
    telegram.set_webhook(&webhook_url).await?;
}
```

**IMPORTANT DECISION: Webhook vs Long Polling**

Webhooks require HTTPS with a valid certificate. On VPS over Tailscale, we'd need Tailscale Serve (HTTPS funnel) or a reverse proxy. This adds complexity.

**Long polling is simpler for Phase 0:**
- No HTTPS needed
- No public URL needed
- Fawx calls `getUpdates` in a loop
- Works behind any firewall/NAT
- Slight latency (~1-2s) but acceptable for dogfooding

Add a polling loop alternative:

```rust
impl TelegramChannel {
    /// Run long-polling loop. Calls getUpdates, processes messages,
    /// sends responses. Runs until cancelled.
    pub async fn poll_loop(
        &self,
        app: Arc<Mutex<HeadlessApp>>,
    ) -> Result<(), TelegramError>;
}
```

**Recommendation:** Implement BOTH, default to long polling. Webhook mode for production (when HTTPS is available).

## Telegram Bot API Subset (what we need)

Only these methods:

| Method | Purpose |
|--------|---------|
| `getUpdates` | Long polling for incoming messages |
| `sendMessage` | Send response text |
| `sendChatAction` | "typing..." indicator |
| `setWebhook` | Register webhook URL |
| `deleteWebhook` | Clean up on shutdown |
| `getMe` | Verify bot token on startup |

Base URL: `https://api.telegram.org/bot<token>/<method>`

All return `{ "ok": bool, "result": ... }` — only parse what we need.

## Security

1. **Bot token encrypted at rest** via fx-auth credential store (AES-256-GCM).
2. **Chat ID allowlist** — only configured chat IDs can interact. Empty = open (for initial testing, but warn in logs).
3. **No secrets in logs** — bot token is never logged. Chat IDs and message text logged at INFO level (operator's own messages on their own system).
4. **Webhook secret** — when using webhook mode, include a `secret_token` parameter so Telegram sends `X-Telegram-Bot-Api-Secret-Token` header. Validate it in the webhook handler. Prevents anyone who discovers the webhook URL from injecting fake updates.

## Tests (10 tests)

```rust
#[test] fn parse_text_message_extracts_fields()
#[test] fn parse_update_ignores_non_text()
#[test] fn parse_update_rejects_disallowed_chat()
#[test] fn parse_update_allows_empty_allowlist()
#[test] fn outgoing_message_serializes_correctly()
#[test] fn send_response_fails_without_chat_id()
#[test] fn channel_trait_id_and_name()
#[test] fn channel_trait_input_source()
#[test] fn is_allowed_with_empty_list_accepts_all()
#[test] fn is_allowed_with_list_rejects_unlisted()
```

Integration tests (require bot token, `#[ignore]`):
```rust
#[tokio::test] #[ignore] async fn get_me_validates_token()
#[tokio::test] #[ignore] async fn send_and_receive_message_roundtrip()
```

## Explore First (before implementing)

1. Read `engine/crates/fx-channel-webhook/src/lib.rs` — follow the same patterns (no-networking in crate, HTTP in fx-cli).
2. Read `engine/crates/fx-cli/src/http_serve.rs` — understand HttpState, router setup, auth middleware.
3. Read `engine/crates/fx-core/src/channel.rs` — the Channel trait you're implementing.
4. Read `engine/crates/fx-config/src/lib.rs` — how configs are structured (FawxConfig, defaults, serde).
5. Read `engine/crates/fx-cli/src/main.rs` — how `run_http_server()` constructs HeadlessApp.

## What NOT to build (YAGNI)

- ❌ Inline keyboards / buttons — plain text responses only
- ❌ Media handling (photos, voice, files) — text messages only
- ❌ Group chat support — 1:1 bot chat only for now
- ❌ Message editing — send new messages only
- ❌ Bot commands menu — no /start, /help registration with BotFather
- ❌ Rate limiting — single user, trusted environment
- ❌ Message queue / retry — fire and forget, log errors
- ❌ Markdown → HTML conversion — use Telegram's Markdown parse_mode directly

## Cargo.toml workspace

Add to root `Cargo.toml`:
```toml
members = [
    # ... existing ...
    "engine/crates/fx-channel-telegram",
]
```

Add to `fx-cli/Cargo.toml` behind the `http` feature:
```toml
[dependencies]
fx-channel-telegram = { path = "../fx-channel-telegram", optional = true }

[features]
http = ["dep:axum", "dep:ring", "dep:fx-channel-telegram"]
```

This keeps Telegram support gated behind `--features http` — no bloat for TUI-only builds.

## Estimated Size

- `fx-channel-telegram/src/lib.rs`: ~300-350 lines
- `fx-cli/src/http_serve.rs` additions: ~80-100 lines (webhook handler, state wiring, polling task)
- `fx-config/src/lib.rs` additions: ~20 lines (TelegramChannelConfig)
- Tests: ~150 lines
- **Total: ~550-620 lines**

## Success Criteria

1. `cargo build -p fx-cli --features http` compiles clean
2. `fawx serve --http` with Telegram config starts polling loop
3. Send message to bot on Telegram → bot responds with agentic loop output
4. Typing indicator shows while processing
5. Chat ID allowlist blocks unauthorized users
6. Bot token stored via encrypted credential store
7. All 10 unit tests pass
8. Logs show message received / response sent / errors
