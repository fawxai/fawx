//! HTTP API server for Fawx headless mode.
//!
//! Provides a thin HTTP adapter over [`HeadlessApp`] with endpoints for
//! message processing, health checks, and status. The server always binds
//! to localhost so the local TUI can connect, and also binds to the
//! Tailscale interface (100.64.0.0/10 CGNAT range) when one is available.
//!
//! All authenticated endpoints require a `Bearer <token>` header validated
//! via HMAC-based constant-time comparison (`ring::hmac`). The `/health`
//! endpoint is public for monitoring.

use axum::extract::{Json, Path, State};
use axum::http::StatusCode;
use axum::middleware;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Router;
use fx_channel_telegram::{IncomingMessage, OutgoingMessage, TelegramChannel};
use fx_channel_webhook::{WebhookChannel, WebhookMessage, WebhookResponse};
use fx_config::HttpConfig;
use fx_core::channel::{Channel, ResponseContext};
use fx_core::types::InputSource;
use fx_kernel::{ChannelRegistry, HttpChannel, ResponseRouter};
use ring::hmac;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Instant;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use crate::headless::{process_input_with_commands, CycleResult, HeadlessApp};

// ── Request/Response types ──────────────────────────────────────────────────

#[derive(Deserialize)]
struct MessageRequest {
    message: String,
}

#[derive(Serialize)]
struct MessageResponse {
    response: String,
    model: String,
    iterations: u32,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    model: String,
    uptime_seconds: u64,
    skills_loaded: usize,
}

#[derive(Serialize)]
struct StatusResponse {
    status: &'static str,
    model: String,
    skills: Vec<String>,
    memory_entries: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    tailscale_ip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    config: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

// ── Shared state ────────────────────────────────────────────────────────────

#[derive(Clone)]
struct HttpState {
    app: Arc<Mutex<HeadlessApp>>,
    start_time: Instant,
    tailscale_ip: Option<String>,
    // TODO(#1203): Token is stored as plaintext String — could appear in heap
    // dumps. Future hardening: wrap with `secrecy::SecretString` to ensure
    // zeroization on drop and prevent accidental logging.
    bearer_token: String,
    channels: ChannelRuntime,
}

#[derive(Clone)]
struct ChannelRuntime {
    router: Arc<ResponseRouter>,
    http: Arc<HttpChannel>,
    telegram: Option<Arc<TelegramChannel>>,
    webhooks: Arc<HashMap<String, Arc<WebhookChannel>>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ListenTarget {
    addr: SocketAddr,
    label: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ListenPlan {
    local: ListenTarget,
    tailscale: Option<ListenTarget>,
}

struct BoundListener {
    target: ListenTarget,
    listener: TcpListener,
}

struct BoundListeners {
    local: BoundListener,
    tailscale: Option<BoundListener>,
}

// ── Token verification ──────────────────────────────────────────────────────

/// Constant-time token comparison using HMAC.
///
/// Uses the expected token as the HMAC key, signs the expected value, then
/// verifies the provided value against that tag. This avoids length-based
/// timing leaks because HMAC produces fixed-size output and `hmac::verify`
/// performs constant-time comparison internally.
///
/// Shared between production middleware and test helpers (single source of
/// truth for auth logic — see #1204 review finding #3).
fn verify_token(expected: &str, provided: &str) -> bool {
    let key = hmac::Key::new(hmac::HMAC_SHA256, expected.as_bytes());
    let tag = hmac::sign(&key, expected.as_bytes());
    hmac::verify(&key, provided.as_bytes(), tag.as_ref()).is_ok()
}

// ── Authentication middleware ───────────────────────────────────────────────

/// Axum middleware that validates `Authorization: Bearer <token>` headers.
///
/// Uses HMAC-based constant-time comparison via [`verify_token`] to prevent
/// timing side-channel attacks on the bearer token.
async fn auth_middleware(
    State(state): State<HttpState>,
    request: axum::http::Request<axum::body::Body>,
    next: middleware::Next,
) -> axum::response::Response {
    let unauthorized = || {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                error: "unauthorized".to_string(),
            }),
        )
            .into_response()
    };

    // Log request metadata for audit purposes (review finding #7).
    // Never log the Authorization header or message content.
    // Full audit logging is tracked in #1203.
    let path = request.uri().path().to_string();
    tracing::info!(endpoint = %path, "HTTP request");

    let header = match request.headers().get("authorization") {
        Some(h) => h,
        None => return unauthorized(),
    };

    let header_str = match header.to_str() {
        Ok(s) => s,
        Err(_) => return unauthorized(),
    };

    let token = match header_str.strip_prefix("Bearer ") {
        Some(t) => t,
        None => return unauthorized(),
    };

    if !verify_token(&state.bearer_token, token) {
        return unauthorized();
    }

    next.run(request).await
}

// ── Tailscale detection ─────────────────────────────────────────────────────

/// Check whether an IP address falls within the Tailscale CGNAT range
/// (100.64.0.0/10).
pub fn is_tailscale_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            octets[0] == 100 && (octets[1] & 0xC0) == 64
        }
        _ => false,
    }
}

/// Detect the local Tailscale IP address.
///
/// First tries `tailscale ip -4`; falls back to scanning for an address
/// in the 100.64.0.0/10 CGNAT range. Returns an error if neither method
/// finds a Tailscale interface. Callers may choose to continue with a
/// localhost-only binding when detection fails.
fn detect_tailscale_ip() -> Result<IpAddr, HttpError> {
    if let Some(ip) = detect_via_tailscale_cli() {
        return Ok(ip);
    }
    detect_via_cgnat_scan()
}

fn detect_via_tailscale_cli() -> Option<IpAddr> {
    let output = std::process::Command::new("tailscale")
        .args(["ip", "-4"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let ip: IpAddr = text.trim().parse().ok()?;
    if is_tailscale_ip(&ip) {
        Some(ip)
    } else {
        None
    }
}

fn detect_via_cgnat_scan() -> Result<IpAddr, HttpError> {
    let output = std::process::Command::new("ip")
        .args(["-4", "-o", "addr", "show"])
        .output()
        .map_err(|e| HttpError::NoTailscale(format!("failed to run `ip addr`: {e}")))?;

    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        if let Some(ip) = extract_ip_from_line(line) {
            if is_tailscale_ip(&ip) {
                return Ok(ip);
            }
        }
    }

    Err(HttpError::NoTailscale(
        "Could not detect Tailscale interface.\n\
         The HTTP server will continue with a localhost-only binding."
            .to_string(),
    ))
}

fn extract_ip_from_line(line: &str) -> Option<IpAddr> {
    let inet_pos = line.find("inet ")?;
    let after_inet = &line[inet_pos + 5..];
    let addr_str = after_inet.split('/').next()?;
    addr_str.trim().parse().ok()
}

fn listen_targets(port: u16, tailscale_ip: Option<IpAddr>) -> ListenPlan {
    ListenPlan {
        local: ListenTarget {
            addr: SocketAddr::from(([127, 0, 0, 1], port)),
            label: "local",
        },
        tailscale: tailscale_ip.map(|ip| ListenTarget {
            addr: SocketAddr::new(ip, port),
            label: "Tailscale",
        }),
    }
}

fn optional_tailscale_ip(result: Result<IpAddr, HttpError>) -> Option<IpAddr> {
    match result {
        Ok(ip) => Some(ip),
        Err(error) => {
            tracing::warn!(error = %error, "tailscale IP not detected; serving localhost only");
            None
        }
    }
}

fn detect_optional_tailscale_ip() -> Option<IpAddr> {
    optional_tailscale_ip(detect_tailscale_ip())
}

// ── Error type ──────────────────────────────────────────────────────────────

#[derive(Debug)]
enum HttpError {
    NoTailscale(String),
    MissingBearerToken,
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoTailscale(msg) => write!(f, "{msg}"),
            Self::MissingBearerToken => write!(
                f,
                "HTTP API requires a bearer token for authentication.\n\n\
                 Option 1 (recommended): Use the TUI command:\n\
                 \x20 /auth http set-bearer <TOKEN>\n\n\
                 Option 2 (deprecated): Add to ~/.fawx/config.toml:\n\
                 \x20 [http]\n\
                 \x20 bearer_token = \"your-secret-token\"\n\n\
                 Generate a token with: openssl rand -hex 32"
            ),
        }
    }
}

impl std::error::Error for HttpError {}

// ── Token validation ────────────────────────────────────────────────────────

/// Resolve the bearer token, preferring the encrypted credential store over config.
///
/// Checks the credential store first (token stored via `/auth http set-bearer`),
/// then falls back to `config.bearer_token` for backward compatibility.
/// Trims leading/trailing whitespace from configured values.
fn validate_bearer_token(
    config: &HttpConfig,
    auth_store: Option<&crate::auth_store::AuthStore>,
) -> Result<String, HttpError> {
    // 1. Check encrypted credential store first.
    if let Some(store) = auth_store {
        if let Ok(Some(token)) = store.get_provider_token("http_bearer") {
            let trimmed = token.trim().to_string();
            if !trimmed.is_empty() {
                return Ok(trimmed);
            }
        }
    }

    // 2. Fall back to config.toml (deprecated).
    match &config.bearer_token {
        Some(token) => {
            let trimmed = token.trim().to_string();
            if trimmed.is_empty() {
                Err(HttpError::MissingBearerToken)
            } else {
                Ok(trimmed)
            }
        }
        _ => Err(HttpError::MissingBearerToken),
    }
}

fn build_channel_runtime(
    telegram: Option<Arc<TelegramChannel>>,
    webhook_channels: Vec<Arc<WebhookChannel>>,
) -> ChannelRuntime {
    let http = Arc::new(HttpChannel::new());
    let webhooks = webhook_channels
        .into_iter()
        .fold(HashMap::new(), |mut map, channel| {
            map.insert(channel.id().to_string(), channel);
            map
        });

    let mut registry = ChannelRegistry::new();
    registry.register(http.clone());
    if let Some(channel) = &telegram {
        registry.register(channel.clone());
    }
    for channel in webhooks.values() {
        registry.register(channel.clone());
    }

    let registry = Arc::new(registry);
    ChannelRuntime {
        router: Arc::new(ResponseRouter::new(registry)),
        http,
        telegram,
        webhooks: Arc::new(webhooks),
    }
}

async fn process_and_route_message(
    app: &Arc<Mutex<HeadlessApp>>,
    router: &ResponseRouter,
    text: &str,
    source: InputSource,
    context: ResponseContext,
) -> Result<CycleResult, anyhow::Error> {
    let mut guard = app.lock().await;
    let result = process_input_with_commands(&mut guard, text, Some(&source)).await?;
    router
        .route(&source, &result.response, &context)
        .map_err(|error| anyhow::anyhow!("response routing failed: {error}"))?;
    Ok(result)
}

fn sanitize_config(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => sanitize_config_object(map),
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(sanitize_config).collect())
        }
        other => other,
    }
}

fn sanitize_config_object(map: serde_json::Map<String, serde_json::Value>) -> serde_json::Value {
    let sanitized = map
        .into_iter()
        .map(|(key, value)| {
            let next = if is_secret_key(&key) {
                serde_json::Value::String("[REDACTED]".to_string())
            } else {
                sanitize_config(value)
            };
            (key, next)
        })
        .collect();
    serde_json::Value::Object(sanitized)
}

fn is_secret_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    let exact = [
        "access_key",
        "api_key",
        "auth_token",
        "bearer_token",
        "bot_token",
        "credential",
        "password",
        "private_key",
        "secret",
        "ssh_key",
        "token",
        "webhook_secret",
    ];
    if exact.iter().any(|candidate| key == *candidate) {
        return true;
    }
    [
        "_access_key",
        "_api_key",
        "_credential",
        "_password",
        "_private_key",
        "_secret",
        "_ssh_key",
        "_token",
    ]
    .iter()
    .any(|suffix| key.ends_with(suffix))
}

fn sanitized_status_config(app: &HeadlessApp) -> Option<serde_json::Value> {
    let manager = app.config_manager()?;
    let guard = manager.lock().ok()?;
    let config = guard.get("all").ok()?;
    Some(sanitize_config(config))
}

// ── Router ──────────────────────────────────────────────────────────────────

/// Maximum request body size (1 MiB).
const MAX_REQUEST_BYTES: usize = 1_048_576;

fn build_router(state: HttpState) -> Router {
    let authenticated = Router::new()
        .route("/message", post(handle_message))
        .route("/status", get(handle_status))
        .route("/config", get(handle_config_get).post(handle_config_set))
        .route("/webhook/{channel_id}", post(handle_webhook))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    // Telegram webhook is public (Telegram servers POST to it).
    // Security is handled by webhook secret_token validation inside the handler.
    let public = Router::new()
        .route("/health", get(handle_health))
        .route("/telegram/webhook", post(handle_telegram_webhook));

    authenticated
        .merge(public)
        .layer(axum::extract::DefaultBodyLimit::max(MAX_REQUEST_BYTES))
        .with_state(state)
}

// ── Handlers ────────────────────────────────────────────────────────────────

async fn handle_message(
    State(state): State<HttpState>,
    Json(request): Json<MessageRequest>,
) -> Result<Json<MessageResponse>, (StatusCode, Json<ErrorBody>)> {
    if request.message.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "message must not be empty".to_string(),
            }),
        ));
    }

    let result = process_and_route_message(
        &state.app,
        state.channels.router.as_ref(),
        &request.message,
        InputSource::Http,
        ResponseContext::default(),
    )
    .await
    .map_err(internal_error)?;
    let response = state
        .channels
        .http
        .take_response()
        .unwrap_or_else(|| result.response.clone());

    Ok(Json(MessageResponse {
        response,
        model: result.model,
        iterations: result.iterations,
    }))
}

fn internal_error(error: anyhow::Error) -> (StatusCode, Json<ErrorBody>) {
    tracing::error!(error = %error, "message processing failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorBody {
            error: "internal_error".to_string(),
        }),
    )
}

async fn handle_webhook(
    State(state): State<HttpState>,
    Path(channel_id): Path<String>,
    Json(request): Json<WebhookMessage>,
) -> Result<Json<WebhookResponse>, (StatusCode, Json<ErrorBody>)> {
    let Some(channel) = state.channels.webhooks.get(&channel_id) else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                error: "webhook channel not found".to_string(),
            }),
        ));
    };
    if request.text.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "message must not be empty".to_string(),
            }),
        ));
    }

    let source = InputSource::Channel(channel_id.clone());
    let result = process_and_route_message(
        &state.app,
        state.channels.router.as_ref(),
        &request.text,
        source,
        ResponseContext::default(),
    )
    .await
    .map_err(internal_error)?;
    let text = channel
        .take_response()
        .unwrap_or_else(|| result.response.clone());

    Ok(Json(WebhookResponse {
        text,
        channel_id,
        complete: true,
    }))
}

async fn handle_health(State(state): State<HttpState>) -> Json<HealthResponse> {
    let app = state.app.lock().await;
    let uptime = state.start_time.elapsed().as_secs();
    let model = app.active_model().to_string();
    Json(HealthResponse {
        status: "ok",
        model,
        uptime_seconds: uptime,
        skills_loaded: 0,
    })
}

// ── Config request/response types ────────────────────────────────────────────

use fx_tools::ConfigSetRequest;

async fn handle_config_get(
    State(state): State<HttpState>,
    query: axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorBody>)> {
    let section = query.get("section").map(|s| s.as_str()).unwrap_or("all");
    let app = state.app.lock().await;
    let mgr = app.config_manager().ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                error: "config manager not available".to_string(),
            }),
        )
    })?;
    let guard = mgr.lock().map_err(|e| {
        tracing::error!(error = %e, "config manager lock failed");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody {
                error: "internal_error".to_string(),
            }),
        )
    })?;
    let value = guard
        .get(section)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorBody { error: e })))?;
    Ok(Json(value))
}

async fn handle_config_set(
    State(state): State<HttpState>,
    Json(request): Json<ConfigSetRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorBody>)> {
    let app = state.app.lock().await;
    let mgr = app.config_manager().ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                error: "config manager not available".to_string(),
            }),
        )
    })?;
    let mut guard = mgr.lock().map_err(|e| {
        tracing::error!(error = %e, "config manager lock failed");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody {
                error: "internal_error".to_string(),
            }),
        )
    })?;
    guard
        .set(&request.key, &request.value)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorBody { error: e })))?;
    Ok(Json(serde_json::json!({
        "updated": request.key,
        "value": request.value,
    })))
}

async fn handle_status(State(state): State<HttpState>) -> Json<StatusResponse> {
    let app = state.app.lock().await;
    let model = app.active_model().to_string();
    let config = sanitized_status_config(&app);

    Json(StatusResponse {
        status: "ok",
        model,
        skills: Vec::new(),
        memory_entries: 0,
        tailscale_ip: state.tailscale_ip.clone(),
        config,
    })
}

// ── Telegram photo helpers ──────────────────────────────────────────────────

/// Return the media inbound directory (`~/.fawx/media/inbound/`).
fn media_inbound_dir() -> std::path::PathBuf {
    crate::tui::fawx_data_dir().join("media").join("inbound")
}

/// Download photos and build the final message text.
///
/// If the incoming message has photos, downloads each one and prepends
/// `[Image: /path/to/file.jpg]\n` to the text. Creates the media directory
/// on first use.
async fn build_text_with_photos(
    telegram: &TelegramChannel,
    incoming: &mut IncomingMessage,
) -> String {
    if incoming.photos.is_empty() {
        return incoming.text.clone();
    }

    let media_dir = media_inbound_dir();
    if let Err(e) = std::fs::create_dir_all(&media_dir) {
        tracing::error!("Failed to create media dir: {e}");
        return incoming.text.clone();
    }

    let mut prefix = String::new();
    for photo in &mut incoming.photos {
        match telegram
            .download_file(&photo.file_id, incoming.message_id, &media_dir)
            .await
        {
            Ok(path) => {
                prefix.push_str(&format!("[Image: {}]\n", path.display()));
                photo.file_path = Some(path);
            }
            Err(e) => {
                tracing::error!("Failed to download photo {}: {e}", photo.file_id);
            }
        }
    }

    if prefix.is_empty() {
        incoming.text.clone()
    } else {
        format!("{prefix}{}", incoming.text)
    }
}

// ── Telegram long-polling loop ──────────────────────────────────────────────

fn telegram_context(incoming: &IncomingMessage) -> ResponseContext {
    ResponseContext {
        routing_key: Some(incoming.chat_id.to_string()),
        reply_to: Some(incoming.message_id.to_string()),
    }
}

fn queue_telegram_error(
    telegram: &TelegramChannel,
    incoming: &IncomingMessage,
    error: &anyhow::Error,
) {
    let context = telegram_context(incoming);
    let message = format!("⚠️ Error: {error}");
    let _ = telegram.queue_response(&message, &context, None);
}

async fn flush_telegram_outbound(telegram: &TelegramChannel) {
    for outbound in telegram.drain_outbound() {
        let message = OutgoingMessage {
            chat_id: outbound.chat_id,
            text: outbound.text,
            parse_mode: outbound.parse_mode,
            reply_to_message_id: outbound.reply_to_message_id,
        };
        if let Err(error) = telegram.send_message(&message).await {
            tracing::error!("Telegram send failed: {error}");
            break;
        }
    }
}

async fn handle_telegram_update(
    telegram: &TelegramChannel,
    app: &Arc<Mutex<HeadlessApp>>,
    router: &ResponseRouter,
    raw_update: &serde_json::Value,
) {
    let payload = raw_update.to_string();
    let mut incoming = match telegram.parse_update(&payload) {
        Ok(Some(message)) => message,
        Ok(None) => return,
        Err(error) => {
            tracing::warn!("Telegram poll parse error: {error}");
            return;
        }
    };

    tracing::info!(
        chat_id = incoming.chat_id,
        from = ?incoming.from_name,
        photos = incoming.photos.len(),
        "Telegram poll: message received"
    );
    let _ = telegram.send_typing(incoming.chat_id).await;

    let message_text = build_text_with_photos(telegram, &mut incoming).await;
    let source = InputSource::Channel("telegram".to_string());
    let context = telegram_context(&incoming);
    if let Err(error) = process_and_route_message(app, router, &message_text, source, context).await
    {
        tracing::error!("Telegram poll loop error: {error}");
        queue_telegram_error(telegram, &incoming, &error);
    }
    flush_telegram_outbound(telegram).await;
}

/// Run the Telegram long-polling loop.
///
/// Calls `get_updates` in a loop with a 30-second long-poll timeout.
/// Errors are logged and the loop continues — it never crashes.
async fn run_telegram_polling(
    telegram: Arc<TelegramChannel>,
    app: Arc<Mutex<HeadlessApp>>,
    router: Arc<ResponseRouter>,
) {
    if let Err(error) = telegram.delete_webhook().await {
        tracing::error!("Telegram poll: failed to delete webhook: {error}");
    }

    let mut offset: i64 = 0;
    loop {
        flush_telegram_outbound(&telegram).await;
        match telegram.get_updates(offset, 30).await {
            Ok((updates, next_offset)) => {
                for update in &updates {
                    handle_telegram_update(&telegram, &app, router.as_ref(), update).await;
                }
                offset = next_offset;
            }
            Err(error) => {
                tracing::error!("Telegram poll error: {error}");
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }
}

// ── Telegram webhook handler ────────────────────────────────────────────────

async fn handle_telegram_webhook(
    State(state): State<HttpState>,
    headers: axum::http::HeaderMap,
    body: String,
) -> impl IntoResponse {
    let telegram = match &state.channels.telegram {
        Some(channel) => Arc::clone(channel),
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    let secret_header = headers
        .get("x-telegram-bot-api-secret-token")
        .and_then(|value| value.to_str().ok());
    if !telegram.validate_webhook_secret(secret_header) {
        tracing::warn!("Telegram webhook: invalid or missing secret token");
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let mut incoming = match telegram.parse_update(&body) {
        Ok(Some(message)) => message,
        Ok(None) => return StatusCode::OK.into_response(),
        Err(error) => {
            tracing::warn!("Telegram parse error: {error:?}");
            return StatusCode::OK.into_response();
        }
    };

    tracing::info!(
        chat_id = incoming.chat_id,
        from = ?incoming.from_name,
        photos = incoming.photos.len(),
        "Telegram message received"
    );
    let _ = telegram.send_typing(incoming.chat_id).await;

    let message_text = build_text_with_photos(&telegram, &mut incoming).await;
    let source = InputSource::Channel("telegram".to_string());
    let context = telegram_context(&incoming);
    if let Err(error) = process_and_route_message(
        &state.app,
        state.channels.router.as_ref(),
        &message_text,
        source,
        context,
    )
    .await
    {
        tracing::error!("Telegram loop error: {error:?}");
        queue_telegram_error(&telegram, &incoming, &error);
    }
    flush_telegram_outbound(&telegram).await;
    StatusCode::OK.into_response()
}

// ── Public API ──────────────────────────────────────────────────────────────

/// Run the HTTP server for headless mode.
///
/// Validates that a bearer token is configured, always binds localhost for
/// local clients, optionally binds Tailscale for remote access, and serves
/// requests until the process is terminated.
pub async fn run(
    app: HeadlessApp,
    port: u16,
    http_config: &HttpConfig,
    telegram: Option<Arc<TelegramChannel>>,
    webhook_channels: Vec<Arc<WebhookChannel>>,
) -> anyhow::Result<i32> {
    let data_dir = crate::tui::fawx_data_dir();
    let auth_store = match crate::auth_store::AuthStore::open(&data_dir) {
        Ok(store) => Some(store),
        Err(e) => {
            tracing::warn!(error = %e, "could not open credential store; falling back to config-only bearer token");
            None
        }
    };
    let bearer_token = validate_bearer_token(http_config, auth_store.as_ref())
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let listen_plan = listen_targets(port, detect_optional_tailscale_ip());
    let listeners = bind_listeners(listen_plan).await?;
    let shared_app = Arc::new(Mutex::new(app));
    let channels = build_channel_runtime(telegram.clone(), webhook_channels);
    let state = HttpState {
        app: Arc::clone(&shared_app),
        start_time: Instant::now(),
        tailscale_ip: active_tailscale_ip(&listeners),
        bearer_token,
        channels: channels.clone(),
    };
    let router = build_router(state);

    print_startup_targets(&listeners);
    eprintln!("Bearer token authentication: enabled");
    validate_telegram_startup(channels.telegram.as_ref()).await;
    start_telegram_polling(&channels, &shared_app);

    run_listeners(router, listeners).await?;
    Ok(0)
}

fn active_tailscale_ip(listeners: &BoundListeners) -> Option<String> {
    listeners
        .tailscale
        .as_ref()
        .map(|listener| listener.target.addr.ip().to_string())
}

fn print_startup_targets(listeners: &BoundListeners) {
    eprintln!("Fawx HTTP API listening on:");
    eprintln!(
        "  http://{} ({})",
        listeners.local.target.addr, listeners.local.target.label
    );
    match &listeners.tailscale {
        Some(listener) => {
            eprintln!(
                "  http://{} ({})",
                listener.target.addr, listener.target.label
            );
        }
        None => {
            eprintln!("  Tailscale not detected or unavailable; serving localhost only");
        }
    }
}

async fn validate_telegram_startup(telegram: Option<&Arc<TelegramChannel>>) {
    let Some(tg) = telegram else {
        return;
    };

    match tg.get_me().await {
        Ok(()) => {
            eprintln!("Telegram channel: enabled (token valid, webhook at /telegram/webhook)")
        }
        Err(e) => {
            eprintln!("Warning: Telegram get_me failed: {e}");
            eprintln!(
                "Telegram channel: enabled (webhook at /telegram/webhook) — token may be invalid"
            );
        }
    }
}

fn start_telegram_polling(channels: &ChannelRuntime, shared_app: &Arc<Mutex<HeadlessApp>>) {
    let Some(telegram) = channels.telegram.as_ref() else {
        return;
    };

    tokio::spawn(run_telegram_polling(
        Arc::clone(telegram),
        Arc::clone(shared_app),
        Arc::clone(&channels.router),
    ));
    eprintln!("Telegram long-polling loop: started");
}

async fn run_listeners(router: Router, listeners: BoundListeners) -> anyhow::Result<()> {
    match listeners.tailscale {
        Some(tailscale) => run_listener_pair(router, listeners.local, tailscale).await,
        None => {
            serve_listener(
                listeners.local.listener,
                router,
                listeners.local.target.label,
            )
            .await
        }
    }
}

async fn bind_listeners(plan: ListenPlan) -> anyhow::Result<BoundListeners> {
    let local = bind_required_listener(plan.local).await?;
    let tailscale = bind_optional_listener(plan.tailscale).await;
    Ok(BoundListeners { local, tailscale })
}

async fn bind_required_listener(target: ListenTarget) -> anyhow::Result<BoundListener> {
    let listener = bind_listener(target).await?;
    Ok(BoundListener { target, listener })
}

async fn bind_optional_listener(target: Option<ListenTarget>) -> Option<BoundListener> {
    let target = target?;
    optional_bound_listener(target, bind_listener(target).await)
}

fn optional_bound_listener(
    target: ListenTarget,
    result: anyhow::Result<TcpListener>,
) -> Option<BoundListener> {
    match result {
        Ok(listener) => Some(BoundListener { target, listener }),
        Err(error) => {
            tracing::warn!(
                error = %error,
                addr = %target.addr,
                "Tailscale bind failed; continuing with localhost only"
            );
            eprintln!(
                "  Warning: Tailscale bind failed on {}, serving localhost only",
                target.addr
            );
            None
        }
    }
}

async fn run_listener_pair(
    router: Router,
    local: BoundListener,
    tailscale: BoundListener,
) -> anyhow::Result<()> {
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let local_label = local.target.label;
    let tailscale_label = tailscale.target.label;
    let local_server = tokio::spawn(serve_listener_with_shutdown(
        local,
        router.clone(),
        shutdown_rx.clone(),
    ));
    let tailscale_server =
        tokio::spawn(serve_listener_with_shutdown(tailscale, router, shutdown_rx));

    wait_for_server_pair(
        local_label,
        local_server,
        tailscale_label,
        tailscale_server,
        shutdown_tx,
    )
    .await
}

async fn bind_listener(target: ListenTarget) -> anyhow::Result<TcpListener> {
    TcpListener::bind(target.addr).await.map_err(|e| {
        anyhow::anyhow!(
            "failed to bind {} HTTP server on {}: {e}",
            target.label,
            target.addr
        )
    })
}

async fn serve_listener(
    listener: TcpListener,
    router: Router,
    label: &'static str,
) -> anyhow::Result<()> {
    axum::serve(listener, router)
        .await
        .map_err(|e| anyhow::anyhow!("{label} HTTP server error: {e}"))
}

async fn serve_listener_with_shutdown(
    listener: BoundListener,
    router: Router,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) -> anyhow::Result<()> {
    let label = listener.target.label;
    axum::serve(listener.listener, router)
        .with_graceful_shutdown(async move {
            if !*shutdown.borrow() {
                let _ = shutdown.changed().await;
            }
        })
        .await
        .map_err(|e| anyhow::anyhow!("{label} HTTP server error: {e}"))
}

async fn wait_for_server_pair(
    local_label: &'static str,
    local_server: tokio::task::JoinHandle<anyhow::Result<()>>,
    tailscale_label: &'static str,
    tailscale_server: tokio::task::JoinHandle<anyhow::Result<()>>,
    shutdown_tx: tokio::sync::watch::Sender<bool>,
) -> anyhow::Result<()> {
    let mut local_server = local_server;
    let mut tailscale_server = tailscale_server;

    tokio::select! {
        result = &mut local_server => {
            finalize_server_exit(local_label, result, tailscale_label, tailscale_server, shutdown_tx).await
        }
        result = &mut tailscale_server => {
            finalize_server_exit(tailscale_label, result, local_label, local_server, shutdown_tx).await
        }
    }
}

async fn finalize_server_exit(
    exited_label: &'static str,
    exited_result: Result<anyhow::Result<()>, tokio::task::JoinError>,
    peer_label: &'static str,
    peer_server: tokio::task::JoinHandle<anyhow::Result<()>>,
    shutdown_tx: tokio::sync::watch::Sender<bool>,
) -> anyhow::Result<()> {
    let exited = join_server_result(exited_label, exited_result);
    log_server_exit(
        exited_label,
        &exited,
        "HTTP server exited; shutting down peer",
    );
    let _ = shutdown_tx.send(true);
    let peer = join_server_result(peer_label, peer_server.await);
    log_server_exit(
        peer_label,
        &peer,
        "Peer HTTP server stopped after shutdown signal",
    );
    exited.and(peer)
}

fn log_server_exit(label: &str, result: &anyhow::Result<()>, message: &str) {
    match result {
        Ok(()) => tracing::warn!(server = label, "{message}"),
        Err(error) => tracing::warn!(server = label, error = %error, "{message}"),
    }
}

fn join_server_result(
    label: &str,
    result: Result<anyhow::Result<()>, tokio::task::JoinError>,
) -> anyhow::Result<()> {
    match result {
        Ok(inner) => inner,
        Err(error) => Err(anyhow::anyhow!("{label} HTTP server task failed: {error}")),
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use http_body_util::BodyExt;
    use hyper::Request;
    use std::net::Ipv4Addr;
    use tower::ServiceExt;

    const TEST_TOKEN: &str = "test-secret-token-abc123";

    // ── Auth-only state for test middleware ──────────────────────────────

    /// Minimal state used by test routers — only carries the bearer token
    /// needed by the auth middleware. Mock handlers are stateless.
    #[derive(Clone)]
    struct TestAuthState {
        bearer_token: String,
    }

    /// Auth middleware for tests — uses the shared [`verify_token`] function
    /// (single source of truth, review finding #3).
    async fn test_auth_middleware(
        State(state): State<TestAuthState>,
        request: axum::http::Request<axum::body::Body>,
        next: middleware::Next,
    ) -> axum::response::Response {
        let unauthorized = || {
            (
                StatusCode::UNAUTHORIZED,
                Json(ErrorBody {
                    error: "unauthorized".to_string(),
                }),
            )
                .into_response()
        };

        let header = match request.headers().get("authorization") {
            Some(h) => h,
            None => return unauthorized(),
        };
        let header_str = match header.to_str() {
            Ok(s) => s,
            Err(_) => return unauthorized(),
        };
        let token = match header_str.strip_prefix("Bearer ") {
            Some(t) => t,
            None => return unauthorized(),
        };

        if !verify_token(&state.bearer_token, token) {
            return unauthorized();
        }

        next.run(request).await
    }

    fn authed_test_router() -> Router {
        let state = TestAuthState {
            bearer_token: TEST_TOKEN.to_string(),
        };

        let authenticated = Router::new()
            .route("/status", get(mock_status))
            .route("/message", post(mock_message))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                test_auth_middleware,
            ));

        let public = Router::new().route("/health", get(mock_health));

        authenticated.merge(public).with_state(state)
    }

    /// Build a test router WITHOUT auth (for backward-compat endpoint tests).
    fn test_router() -> Router {
        Router::new()
            .route("/health", get(mock_health))
            .route("/status", get(mock_status))
            .route("/message", post(mock_message))
    }

    async fn mock_health() -> Json<HealthResponse> {
        Json(HealthResponse {
            status: "ok",
            model: "test-model".to_string(),
            uptime_seconds: 42,
            skills_loaded: 2,
        })
    }

    async fn mock_status() -> Json<StatusResponse> {
        Json(StatusResponse {
            status: "ok",
            model: "test-model".to_string(),
            skills: vec!["skill-a".to_string()],
            memory_entries: 10,
            tailscale_ip: Some("100.64.0.1".to_string()),
            config: None,
        })
    }

    async fn mock_message(
        Json(req): Json<MessageRequest>,
    ) -> Result<Json<MessageResponse>, (StatusCode, Json<ErrorBody>)> {
        if req.message.trim().is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorBody {
                    error: "message must not be empty".to_string(),
                }),
            ));
        }
        Ok(Json(MessageResponse {
            response: format!("echo: {}", req.message),
            model: "test-model".to_string(),
            iterations: 1,
        }))
    }

    // ── Tailscale IP validation ─────────────────────────────────────────

    #[test]
    fn tailscale_ip_accepts_valid_range() {
        assert!(is_tailscale_ip(&IpAddr::V4(Ipv4Addr::new(100, 64, 0, 1))));
        assert!(is_tailscale_ip(&IpAddr::V4(Ipv4Addr::new(
            100, 127, 255, 255
        ))));
        assert!(is_tailscale_ip(&IpAddr::V4(Ipv4Addr::new(
            100, 93, 251, 101
        ))));
    }

    #[test]
    fn tailscale_ip_rejects_outside_range() {
        assert!(!is_tailscale_ip(&IpAddr::V4(Ipv4Addr::new(100, 63, 0, 0))));
        assert!(!is_tailscale_ip(&IpAddr::V4(Ipv4Addr::new(100, 128, 0, 0))));
        assert!(!is_tailscale_ip(&IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))));
        assert!(!is_tailscale_ip(&IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))));
        assert!(!is_tailscale_ip(&IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))));
        assert!(!is_tailscale_ip(&IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0))));
    }

    #[test]
    fn tailscale_ip_rejects_ipv6() {
        let ipv6: IpAddr = "::1".parse().expect("valid ipv6");
        assert!(!is_tailscale_ip(&ipv6));
    }

    // ── Binding validation ──────────────────────────────────────────────

    #[test]
    fn listen_targets_bind_localhost_and_tailscale() {
        let plan = listen_targets(8400, Some(IpAddr::V4(Ipv4Addr::new(100, 93, 251, 101))));
        let tailscale = plan.tailscale.expect("tailscale target");

        assert_eq!(plan.local.addr, SocketAddr::from(([127, 0, 0, 1], 8400)));
        assert_eq!(plan.local.label, "local");
        assert_eq!(
            tailscale.addr,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(100, 93, 251, 101)), 8400)
        );
        assert_eq!(tailscale.label, "Tailscale");
    }

    #[test]
    fn listen_targets_fall_back_to_localhost_only() {
        let plan = listen_targets(8400, None);

        assert_eq!(plan.local.addr, SocketAddr::from(([127, 0, 0, 1], 8400)));
        assert_eq!(plan.local.label, "local");
        assert!(plan.tailscale.is_none());
    }

    #[test]
    fn optional_tailscale_ip_returns_none_when_detection_fails() {
        let result = optional_tailscale_ip(Err(HttpError::NoTailscale("missing".to_string())));
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn tailscale_bind_failure_falls_back_to_localhost_server() {
        let local_target = ListenTarget {
            addr: SocketAddr::from(([127, 0, 0, 1], 0)),
            label: "local",
        };
        let local_listener = bind_listener(local_target).await.expect("bind localhost");
        let local_addr = local_listener.local_addr().expect("local addr");
        let tailscale_target = ListenTarget {
            addr: SocketAddr::from(([100, 93, 251, 101], 8400)),
            label: "Tailscale",
        };
        let listeners = BoundListeners {
            local: BoundListener {
                target: ListenTarget {
                    addr: local_addr,
                    label: "local",
                },
                listener: local_listener,
            },
            tailscale: optional_bound_listener(
                tailscale_target,
                Err(anyhow::anyhow!("bind failed")),
            ),
        };

        let server = tokio::spawn(run_listeners(
            Router::new().route("/health", get(mock_health)),
            listeners,
        ));
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let response = reqwest::get(format!("http://{local_addr}/health"))
            .await
            .expect("request localhost health");
        assert_eq!(response.status(), reqwest::StatusCode::OK);

        server.abort();
        let _ = server.await;
    }

    #[tokio::test]
    async fn wait_for_server_pair_shuts_down_peer_when_one_server_exits() {
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let local_server = tokio::spawn(async { Ok(()) });
        let tailscale_server = tokio::spawn(async move {
            let mut shutdown_rx = shutdown_rx;
            let _ = shutdown_rx.changed().await;
            Ok(())
        });

        let result = wait_for_server_pair(
            "local",
            local_server,
            "Tailscale",
            tailscale_server,
            shutdown_tx,
        )
        .await;

        assert!(result.is_ok());
    }

    // ── IP extraction from `ip addr` output ─────────────────────────────

    #[test]
    fn extract_ip_parses_ip_addr_output() {
        let line = "4: tailscale0    inet 100.93.251.101/32 scope global tailscale0";
        let ip = extract_ip_from_line(line);
        assert_eq!(ip, Some(IpAddr::V4(Ipv4Addr::new(100, 93, 251, 101))));
    }

    #[test]
    fn extract_ip_returns_none_for_no_inet() {
        let line = "4: tailscale0    link/none";
        assert!(extract_ip_from_line(line).is_none());
    }

    // ── Request/response serialization ──────────────────────────────────

    #[test]
    fn message_request_deserializes() {
        let json = r#"{"message": "hello"}"#;
        let req: MessageRequest = serde_json::from_str(json).expect("valid json");
        assert_eq!(req.message, "hello");
    }

    #[test]
    fn message_request_rejects_missing_message() {
        let json = r#"{}"#;
        let result = serde_json::from_str::<MessageRequest>(json);
        assert!(result.is_err());
    }

    #[test]
    fn message_response_serializes_correctly() {
        let resp = MessageResponse {
            response: "hi there".to_string(),
            model: "gpt-4".to_string(),
            iterations: 2,
        };
        let json: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&resp).expect("serialize")).expect("parse");
        assert_eq!(json["response"], "hi there");
        assert_eq!(json["model"], "gpt-4");
        assert_eq!(json["iterations"], 2);
    }

    #[test]
    fn health_response_has_expected_fields() {
        let resp = HealthResponse {
            status: "ok",
            model: "claude-3".to_string(),
            uptime_seconds: 60,
            skills_loaded: 3,
        };
        let json: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&resp).expect("serialize")).expect("parse");
        assert_eq!(json["status"], "ok");
        assert_eq!(json["model"], "claude-3");
        assert_eq!(json["uptime_seconds"], 60);
        assert_eq!(json["skills_loaded"], 3);
    }

    #[test]
    fn status_response_has_expected_fields() {
        let resp = StatusResponse {
            status: "ok",
            model: "claude-3".to_string(),
            skills: vec!["read_file".to_string()],
            memory_entries: 42,
            tailscale_ip: Some("100.93.251.101".to_string()),
            config: None,
        };
        let json: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&resp).expect("serialize")).expect("parse");
        assert_eq!(json["status"], "ok");
        assert_eq!(json["tailscale_ip"], "100.93.251.101");
        assert_eq!(json["memory_entries"], 42);
        assert!(json["skills"].is_array());
    }

    #[test]
    fn status_response_omits_tailscale_ip_when_unavailable() {
        let resp = StatusResponse {
            status: "ok",
            model: "claude-3".to_string(),
            skills: vec!["read_file".to_string()],
            memory_entries: 42,
            tailscale_ip: None,
            config: None,
        };
        let json: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&resp).expect("serialize")).expect("parse");
        assert_eq!(json["status"], "ok");
        assert!(json.get("tailscale_ip").is_none());
    }

    // ── Bearer token validation ─────────────────────────────────────────

    #[test]
    fn validate_bearer_token_accepts_valid_token() {
        let config = HttpConfig {
            bearer_token: Some("my-secret".to_string()),
        };
        assert!(validate_bearer_token(&config, None).is_ok());
    }

    #[test]
    fn validate_bearer_token_rejects_none() {
        let config = HttpConfig { bearer_token: None };
        assert!(validate_bearer_token(&config, None).is_err());
    }

    #[test]
    fn validate_bearer_token_rejects_empty() {
        let config = HttpConfig {
            bearer_token: Some(String::new()),
        };
        assert!(validate_bearer_token(&config, None).is_err());
    }

    #[test]
    fn validate_bearer_token_rejects_whitespace_only() {
        let config = HttpConfig {
            bearer_token: Some("   ".to_string()),
        };
        assert!(validate_bearer_token(&config, None).is_err());
    }

    // ── Endpoint integration tests (no auth) ────────────────────────────

    #[tokio::test]
    async fn health_endpoint_returns_ok() {
        let app = test_router();
        let req = Request::builder()
            .method("GET")
            .uri("/health")
            .body(Body::empty())
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);

        let body = resp.into_body().collect().await.expect("body").to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(json["status"], "ok");
        assert_eq!(json["model"], "test-model");
    }

    #[tokio::test]
    async fn status_endpoint_returns_ok() {
        let app = test_router();
        let req = Request::builder()
            .method("GET")
            .uri("/status")
            .body(Body::empty())
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);

        let body = resp.into_body().collect().await.expect("body").to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(json["status"], "ok");
        assert_eq!(json["tailscale_ip"], "100.64.0.1");
        assert!(json["skills"].is_array());
    }

    #[tokio::test]
    async fn message_endpoint_returns_response() {
        let app = test_router();
        let req = Request::builder()
            .method("POST")
            .uri("/message")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"message": "hello"}"#))
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);

        let body = resp.into_body().collect().await.expect("body").to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(json["response"], "echo: hello");
        assert_eq!(json["iterations"], 1);
    }

    #[tokio::test]
    async fn message_endpoint_rejects_empty_message() {
        let app = test_router();
        let req = Request::builder()
            .method("POST")
            .uri("/message")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"message": "   "}"#))
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let body = resp.into_body().collect().await.expect("body").to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert!(json["error"]
            .as_str()
            .expect("error field")
            .contains("empty"));
    }

    #[tokio::test]
    async fn message_endpoint_rejects_missing_body() {
        let app = test_router();
        let req = Request::builder()
            .method("POST")
            .uri("/message")
            .header("content-type", "application/json")
            .body(Body::from(r#"{}"#))
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert!(resp.status().is_client_error());
    }

    // ── Auth middleware tests ────────────────────────────────────────────

    #[tokio::test]
    async fn auth_missing_header_returns_401() {
        let app = authed_test_router();
        let req = Request::builder()
            .method("GET")
            .uri("/status")
            .body(Body::empty())
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let body = resp.into_body().collect().await.expect("body").to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(json["error"], "unauthorized");
    }

    #[tokio::test]
    async fn auth_wrong_token_returns_401() {
        let app = authed_test_router();
        let req = Request::builder()
            .method("GET")
            .uri("/status")
            .header("authorization", "Bearer wrong-token")
            .body(Body::empty())
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn auth_correct_token_returns_200() {
        let app = authed_test_router();
        let req = Request::builder()
            .method("GET")
            .uri("/status")
            .header("authorization", format!("Bearer {TEST_TOKEN}"))
            .body(Body::empty())
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn auth_bearer_prefix_required() {
        let app = authed_test_router();
        // Token without "Bearer " prefix
        let req = Request::builder()
            .method("GET")
            .uri("/status")
            .header("authorization", TEST_TOKEN)
            .body(Body::empty())
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn auth_health_endpoint_public() {
        let app = authed_test_router();
        // No auth header — health should still work
        let req = Request::builder()
            .method("GET")
            .uri("/health")
            .body(Body::empty())
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);

        let body = resp.into_body().collect().await.expect("body").to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(json["status"], "ok");
    }

    #[tokio::test]
    async fn auth_message_endpoint_requires_token() {
        let app = authed_test_router();
        let req = Request::builder()
            .method("POST")
            .uri("/message")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"message": "hello"}"#))
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn auth_message_with_valid_token_succeeds() {
        let app = authed_test_router();
        let req = Request::builder()
            .method("POST")
            .uri("/message")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {TEST_TOKEN}"))
            .body(Body::from(r#"{"message": "hello"}"#))
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);

        let body = resp.into_body().collect().await.expect("body").to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(json["response"], "echo: hello");
    }

    // ── HMAC-based token verification ───────────────────────────────────

    #[test]
    fn verify_token_correct_token_accepted() {
        assert!(verify_token("test-token-123", "test-token-123"));
    }

    #[test]
    fn verify_token_wrong_token_rejected() {
        assert!(!verify_token("test-token-123", "wrong-token-456"));
    }

    #[test]
    fn verify_token_different_lengths_rejected() {
        assert!(!verify_token("short", "longer-token"));
    }

    #[test]
    fn verify_token_empty_provided_rejected() {
        assert!(!verify_token("some-token", ""));
    }

    #[test]
    fn verify_token_empty_both_accepted() {
        // Edge case: both empty — HMAC of empty against empty matches.
        assert!(verify_token("", ""));
    }

    // ── Bearer token validation (config) ────────────────────────────────

    #[test]
    fn validate_bearer_token_trims_whitespace() {
        let config = HttpConfig {
            bearer_token: Some("  my-secret  ".to_string()),
        };
        let result = validate_bearer_token(&config, None).expect("should accept");
        assert_eq!(result, "my-secret");
    }

    // ── Auth edge case tests (review finding #5) ────────────────────────

    #[tokio::test]
    async fn auth_empty_bearer_value_returns_401() {
        // "Bearer " with nothing after it → empty token → 401
        let app = authed_test_router();
        let req = Request::builder()
            .method("GET")
            .uri("/status")
            .header("authorization", "Bearer ")
            .body(Body::empty())
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn auth_lowercase_bearer_returns_401() {
        // RFC 7235 says scheme is case-insensitive, but we enforce exact
        // "Bearer " prefix for strictness. This is a deliberate security
        // choice: case-insensitive matching would require additional code
        // and the only legitimate client is our own CLI/SDK.
        let app = authed_test_router();
        let req = Request::builder()
            .method("GET")
            .uri("/status")
            .header("authorization", format!("bearer {TEST_TOKEN}"))
            .body(Body::empty())
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn auth_null_byte_in_token_rejected_at_http_layer() {
        // Null bytes in HTTP header values are rejected by the HTTP layer
        // (hyper/http crate) before reaching our auth middleware. This is
        // the correct behavior — verify the header value is rejected.
        let header_bytes = format!("Bearer {TEST_TOKEN}\x00extradata");
        assert!(
            axum::http::HeaderValue::from_bytes(header_bytes.as_bytes()).is_err(),
            "null bytes in header values must be rejected by the HTTP layer"
        );
    }

    #[tokio::test]
    async fn auth_non_ascii_header_returns_401() {
        // Non-ASCII bytes in the Authorization header should fail to_str()
        // and return 401.
        let app = authed_test_router();
        // Build a header value from raw bytes containing non-ASCII (é = 0xC3 0xA9)
        let header_val =
            axum::http::HeaderValue::from_bytes(b"Bearer t\xc3\xa9st").expect("raw bytes");
        let req = Request::builder()
            .method("GET")
            .uri("/status")
            .header("authorization", header_val)
            .body(Body::empty())
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    // ── Credential store bearer token tests ─────────────────────────────

    fn test_auth_store() -> crate::auth_store::AuthStore {
        crate::auth_store::AuthStore::open_for_testing().expect("should create test auth store")
    }

    #[test]
    fn validate_bearer_token_prefers_credential_store() {
        let store = test_auth_store();
        store
            .store_provider_token("http_bearer", "store-token")
            .expect("store");
        let config = HttpConfig {
            bearer_token: Some("config-token".to_string()),
        };
        let result = validate_bearer_token(&config, Some(&store)).expect("should succeed");
        assert_eq!(result, "store-token");
    }

    #[test]
    fn validate_bearer_token_falls_back_to_config() {
        let store = test_auth_store();
        let config = HttpConfig {
            bearer_token: Some("config-token".to_string()),
        };
        let result = validate_bearer_token(&config, Some(&store)).expect("should succeed");
        assert_eq!(result, "config-token");
    }

    #[test]
    fn validate_bearer_token_fails_when_neither_source_has_token() {
        let store = test_auth_store();
        let config = HttpConfig { bearer_token: None };
        assert!(validate_bearer_token(&config, Some(&store)).is_err());
    }

    #[test]
    fn validate_bearer_token_store_roundtrip() {
        let store = test_auth_store();
        let token = "my-secret-bearer-token-abc123";
        store
            .store_provider_token("http_bearer", token)
            .expect("store");
        let retrieved = store
            .get_provider_token("http_bearer")
            .expect("get")
            .expect("should have value");
        assert_eq!(*retrieved, token);
    }

    #[test]
    fn validate_bearer_token_store_ignores_empty() {
        let store = test_auth_store();
        store
            .store_provider_token("http_bearer", "  ")
            .expect("store");
        let config = HttpConfig {
            bearer_token: Some("config-fallback".to_string()),
        };
        let result = validate_bearer_token(&config, Some(&store)).expect("should succeed");
        assert_eq!(result, "config-fallback");
    }

    mod routing_and_status {
        use super::*;
        use async_trait::async_trait;
        use fx_config::manager::ConfigManager;
        use fx_kernel::act::{ToolExecutor, ToolExecutorError, ToolResult};
        use fx_kernel::budget::{BudgetConfig, BudgetTracker};
        use fx_kernel::cancellation::CancellationToken;
        use fx_kernel::context_manager::ContextCompactor;
        use fx_kernel::loop_engine::LoopEngine;
        use fx_llm::{
            CompletionProvider, CompletionRequest, CompletionResponse, CompletionStream,
            ContentBlock, ModelRouter, ProviderCapabilities, ProviderError as LlmError,
            StreamChunk,
        };
        use fx_subagent::{
            test_support::DisabledSubagentFactory, SubagentLimits, SubagentManager,
            SubagentManagerDeps,
        };
        use std::sync::{Arc, Mutex as StdMutex};
        use tempfile::TempDir;
        use tokio::sync::Mutex;

        #[derive(Debug)]
        struct StubToolExecutor;

        #[async_trait]
        impl ToolExecutor for StubToolExecutor {
            async fn execute_tools(
                &self,
                _calls: &[fx_llm::ToolCall],
                _cancel: Option<&CancellationToken>,
            ) -> Result<Vec<ToolResult>, ToolExecutorError> {
                Ok(Vec::new())
            }
        }

        struct MockProvider;

        #[async_trait]
        impl CompletionProvider for MockProvider {
            async fn complete(
                &self,
                _request: CompletionRequest,
            ) -> Result<CompletionResponse, LlmError> {
                Ok(CompletionResponse {
                    content: vec![ContentBlock::Text {
                        text: "Mock response".to_string(),
                    }],
                    tool_calls: Vec::new(),
                    usage: None,
                    stop_reason: Some("end_turn".to_string()),
                })
            }

            async fn complete_stream(
                &self,
                _request: CompletionRequest,
            ) -> Result<CompletionStream, LlmError> {
                let chunk = StreamChunk {
                    delta_content: Some("Mock response".to_string()),
                    stop_reason: Some("end_turn".to_string()),
                    ..Default::default()
                };
                let stream = futures::stream::once(async move { Ok(chunk) });
                Ok(Box::pin(stream))
            }

            fn name(&self) -> &str {
                "mock"
            }

            fn supported_models(&self) -> Vec<String> {
                vec!["mock-model".to_string()]
            }

            fn capabilities(&self) -> ProviderCapabilities {
                ProviderCapabilities {
                    supports_temperature: false,
                    requires_streaming: false,
                }
            }
        }

        fn test_engine() -> LoopEngine {
            LoopEngine::builder()
                .budget(BudgetTracker::new(BudgetConfig::default(), 0, 0))
                .context(ContextCompactor::new(2048, 256))
                .max_iterations(3)
                .tool_executor(Arc::new(StubToolExecutor))
                .synthesis_instruction("Summarize".to_string())
                .build()
                .expect("test engine")
        }

        fn mock_router() -> ModelRouter {
            let mut router = ModelRouter::new();
            router.register_provider(Box::new(MockProvider));
            router.set_active("mock-model").expect("set active");
            router
        }

        fn make_test_app(config_manager: Option<Arc<StdMutex<ConfigManager>>>) -> HeadlessApp {
            make_test_app_with_config(fx_config::FawxConfig::default(), config_manager)
        }

        fn make_test_app_with_config(
            config: fx_config::FawxConfig,
            config_manager: Option<Arc<StdMutex<ConfigManager>>>,
        ) -> HeadlessApp {
            use crate::headless::HeadlessAppDeps;

            let subagent_manager = Arc::new(SubagentManager::new(SubagentManagerDeps {
                factory: Arc::new(DisabledSubagentFactory::new("disabled")),
                limits: SubagentLimits::default(),
            }));

            HeadlessApp::new(HeadlessAppDeps {
                loop_engine: test_engine(),
                router: Arc::new(mock_router()),
                config,
                memory: None,
                system_prompt_path: None,
                config_manager,
                system_prompt_text: None,
                subagent_manager,
                canary_monitor: None,
            })
            .expect("test app")
        }

        fn test_state(
            config_manager: Option<Arc<StdMutex<ConfigManager>>>,
            webhooks: Vec<Arc<WebhookChannel>>,
        ) -> HttpState {
            test_state_with_config(fx_config::FawxConfig::default(), config_manager, webhooks)
        }

        fn test_state_with_config(
            config: fx_config::FawxConfig,
            config_manager: Option<Arc<StdMutex<ConfigManager>>>,
            webhooks: Vec<Arc<WebhookChannel>>,
        ) -> HttpState {
            HttpState {
                app: Arc::new(Mutex::new(make_test_app_with_config(
                    config,
                    config_manager,
                ))),
                start_time: Instant::now(),
                tailscale_ip: None,
                bearer_token: TEST_TOKEN.to_string(),
                channels: build_channel_runtime(None, webhooks),
            }
        }

        #[test]
        fn sanitize_config_redacts_nested_secrets() {
            let sanitized = sanitize_config(serde_json::json!({
                "model": { "default_model": "test-model" },
                "telegram": {
                    "bot_token": "secret-token",
                    "allowed_chat_ids": [123],
                    "nested": { "api_key": "secret-key" }
                },
                "http": { "bearer_token": "secret-bearer" },
                "limits": { "max_tokens": 4096 }
            }));

            assert_eq!(sanitized["model"]["default_model"], "test-model");
            assert_eq!(sanitized["telegram"]["bot_token"], "[REDACTED]");
            assert_eq!(sanitized["telegram"]["allowed_chat_ids"][0], 123);
            assert_eq!(sanitized["telegram"]["nested"]["api_key"], "[REDACTED]");
            assert_eq!(sanitized["http"]["bearer_token"], "[REDACTED]");
            assert_eq!(sanitized["limits"]["max_tokens"], 4096);
        }

        #[test]
        fn sanitize_config_redacts_private_access_and_credential_keys() {
            for key in [
                "private_key",
                "service_private_key",
                "access_key",
                "aws_access_key",
                "credential",
                "db_credential",
            ] {
                assert!(is_secret_key(key), "expected `{key}` to be redacted");
            }
        }

        #[tokio::test]
        async fn status_endpoint_returns_sanitized_config() {
            let temp = TempDir::new().expect("tempdir");
            std::fs::write(
                temp.path().join("config.toml"),
                r#"[model]
default_model = "test-model"

[http]
bearer_token = "super-secret"

[telegram]
bot_token = "telegram-secret"
allowed_chat_ids = [123]
"#,
            )
            .expect("write config");
            let manager = Arc::new(StdMutex::new(
                ConfigManager::new(temp.path()).expect("config manager"),
            ));
            let app = build_router(test_state(Some(manager), Vec::new()));
            let req = Request::builder()
                .method("GET")
                .uri("/status")
                .header("authorization", format!("Bearer {TEST_TOKEN}"))
                .body(Body::empty())
                .expect("request");

            let resp = app.oneshot(req).await.expect("response");
            assert_eq!(resp.status(), StatusCode::OK);
            let body = resp.into_body().collect().await.expect("body").to_bytes();
            let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
            assert_eq!(json["config"]["model"]["default_model"], "test-model");
            assert_eq!(json["config"]["http"]["bearer_token"], "[REDACTED]");
            assert_eq!(json["config"]["telegram"]["bot_token"], "[REDACTED]");
            assert_eq!(json["config"]["telegram"]["allowed_chat_ids"][0], 123);
        }

        #[tokio::test]
        async fn message_endpoint_intercepts_server_side_status_command() {
            let app = build_router(test_state(None, Vec::new()));
            let req = Request::builder()
                .method("POST")
                .uri("/message")
                .header("authorization", format!("Bearer {TEST_TOKEN}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"message":"/status"}"#))
                .expect("request");

            let resp = app.oneshot(req).await.expect("response");
            assert_eq!(resp.status(), StatusCode::OK);
            let body = resp.into_body().collect().await.expect("body").to_bytes();
            let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
            assert_eq!(json["model"], "mock-model");
            assert_eq!(json["iterations"], 0);
            assert!(json["response"]
                .as_str()
                .expect("response string")
                .contains("Fawx Status"));
        }

        #[tokio::test]
        async fn message_endpoint_returns_client_only_message_for_quit() {
            let app = build_router(test_state(None, Vec::new()));
            let req = Request::builder()
                .method("POST")
                .uri("/message")
                .header("authorization", format!("Bearer {TEST_TOKEN}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"message":"/quit"}"#))
                .expect("request");

            let resp = app.oneshot(req).await.expect("response");
            assert_eq!(resp.status(), StatusCode::OK);
            let body = resp.into_body().collect().await.expect("body").to_bytes();
            let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
            assert_eq!(json["model"], "mock-model");
            assert_eq!(json["iterations"], 0);
            assert_eq!(
                json["response"],
                "/quit is a client-side command (only available in the TUI)"
            );
        }

        #[tokio::test]
        async fn message_endpoint_routes_auth_server_side() {
            let app = build_router(test_state(None, Vec::new()));
            let req = Request::builder()
                .method("POST")
                .uri("/message")
                .header("authorization", format!("Bearer {TEST_TOKEN}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"message":"/auth"}"#))
                .expect("request");

            let resp = app.oneshot(req).await.expect("response");
            assert_eq!(resp.status(), StatusCode::OK);
            let body = resp.into_body().collect().await.expect("body").to_bytes();
            let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
            assert_eq!(json["model"], "mock-model");
            assert_eq!(json["iterations"], 0);
            assert_eq!(
                json["response"],
                "Configured credentials:\n  ✓ mock: configured (api_key) — 1 model"
            );
        }

        #[tokio::test]
        async fn message_endpoint_routes_plain_text_to_agentic_loop() {
            let app = build_router(test_state(None, Vec::new()));
            let req = Request::builder()
                .method("POST")
                .uri("/message")
                .header("authorization", format!("Bearer {TEST_TOKEN}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"message":"hello there"}"#))
                .expect("request");

            let resp = app.oneshot(req).await.expect("response");
            assert_eq!(resp.status(), StatusCode::OK);
            let body = resp.into_body().collect().await.expect("body").to_bytes();
            let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
            assert_eq!(json["model"], "mock-model");
            assert_eq!(json["iterations"], 1);
            assert_eq!(json["response"], "Mock response");
        }

        #[tokio::test]
        async fn message_endpoint_config_reload_updates_runtime_config() {
            let temp = TempDir::new().expect("tempdir");
            std::fs::write(
                temp.path().join("config.toml"),
                "[model]\ndefault_model = \"mock-model\"\n\n[general]\nmax_history = 3\n",
            )
            .expect("write initial config");
            let manager = Arc::new(StdMutex::new(
                ConfigManager::new(temp.path()).expect("config manager"),
            ));
            let config = fx_config::FawxConfig::load(temp.path()).expect("load config");
            let app = build_router(test_state_with_config(
                config,
                Some(Arc::clone(&manager)),
                Vec::new(),
            ));

            std::fs::write(
                temp.path().join("config.toml"),
                "[model]\ndefault_model = \"mock-model\"\n\n[general]\nmax_history = 7\n",
            )
            .expect("write updated config");

            let reload = Request::builder()
                .method("POST")
                .uri("/message")
                .header("authorization", format!("Bearer {TEST_TOKEN}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"message":"/config reload"}"#))
                .expect("reload request");
            let reload_resp = app.clone().oneshot(reload).await.expect("reload response");
            assert_eq!(reload_resp.status(), StatusCode::OK);
            let reload_body = reload_resp
                .into_body()
                .collect()
                .await
                .expect("reload body")
                .to_bytes();
            let reload_json: serde_json::Value =
                serde_json::from_slice(&reload_body).expect("reload json");
            assert_eq!(
                reload_json["response"],
                crate::commands::slash::config_reload_success_message(
                    &temp.path().join("config.toml")
                )
            );
            assert_eq!(reload_json["model"], "mock-model");

            let show = Request::builder()
                .method("POST")
                .uri("/message")
                .header("authorization", format!("Bearer {TEST_TOKEN}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"message":"/config"}"#))
                .expect("show request");
            let show_resp = app.oneshot(show).await.expect("show response");
            assert_eq!(show_resp.status(), StatusCode::OK);
            let body = show_resp
                .into_body()
                .collect()
                .await
                .expect("body")
                .to_bytes();
            let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
            assert!(json["response"]
                .as_str()
                .expect("response string")
                .contains("\"max_history\": 7"));
        }

        #[tokio::test]
        async fn message_endpoint_analyze_runs_server_side() {
            let temp = TempDir::new().expect("tempdir");
            let mut config = fx_config::FawxConfig::default();
            config.general.data_dir = Some(temp.path().to_path_buf());
            let app = build_router(test_state_with_config(config, None, Vec::new()));
            let req = Request::builder()
                .method("POST")
                .uri("/message")
                .header("authorization", format!("Bearer {TEST_TOKEN}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"message":"/analyze"}"#))
                .expect("request");

            let resp = app.oneshot(req).await.expect("response");
            assert_eq!(resp.status(), StatusCode::OK);
            let body = resp.into_body().collect().await.expect("body").to_bytes();
            let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
            let response = json["response"].as_str().expect("response string");
            assert_eq!(response, "No patterns found. Collect more signals first.");
        }

        #[tokio::test]
        async fn message_endpoint_improve_runs_server_side() {
            let temp = TempDir::new().expect("tempdir");
            let mut config = fx_config::FawxConfig::default();
            config.general.data_dir = Some(temp.path().to_path_buf());
            let app = build_router(test_state_with_config(config, None, Vec::new()));
            let req = Request::builder()
                .method("POST")
                .uri("/message")
                .header("authorization", format!("Bearer {TEST_TOKEN}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"message":"/improve"}"#))
                .expect("request");

            let resp = app.oneshot(req).await.expect("response");
            assert_eq!(resp.status(), StatusCode::OK);
            let body = resp.into_body().collect().await.expect("body").to_bytes();
            let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
            let response = json["response"].as_str().expect("response string");
            assert!(response.contains("⚡ Improvement cycle complete."));
            assert!(response.contains("No actionable improvements found."));
        }

        #[tokio::test]
        async fn generic_webhook_endpoint_routes_response() {
            let webhook = Arc::new(WebhookChannel::new(
                "alpha".to_string(),
                "Alpha".to_string(),
                None,
            ));
            let app = build_router(test_state(None, vec![webhook]));
            let req = Request::builder()
                .method("POST")
                .uri("/webhook/alpha")
                .header("authorization", format!("Bearer {TEST_TOKEN}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"text":"hello from webhook"}"#))
                .expect("request");

            let resp = app.oneshot(req).await.expect("response");
            assert_eq!(resp.status(), StatusCode::OK);
            let body = resp.into_body().collect().await.expect("body").to_bytes();
            let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
            assert_eq!(json["channel_id"], "alpha");
            assert_eq!(json["text"], "Mock response");
            assert_eq!(json["complete"], true);
        }

        #[tokio::test]
        async fn generic_webhook_handler_uses_webhook_map_for_lookup() {
            let webhook = Arc::new(WebhookChannel::new(
                "alpha".to_string(),
                "Alpha".to_string(),
                None,
            ));
            let mut router_registry = ChannelRegistry::new();
            router_registry.register(webhook.clone());
            let mut webhooks = std::collections::HashMap::new();
            webhooks.insert("alpha".to_string(), webhook);
            let state = HttpState {
                app: Arc::new(Mutex::new(make_test_app(None))),
                start_time: Instant::now(),
                tailscale_ip: None,
                bearer_token: TEST_TOKEN.to_string(),
                channels: ChannelRuntime {
                    router: Arc::new(ResponseRouter::new(Arc::new(router_registry))),
                    http: Arc::new(HttpChannel::new()),
                    telegram: None,
                    webhooks: Arc::new(webhooks),
                },
            };
            let app = build_router(state);
            let req = Request::builder()
                .method("POST")
                .uri("/webhook/alpha")
                .header("authorization", format!("Bearer {TEST_TOKEN}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"text":"hello from webhook"}"#))
                .expect("request");

            let resp = app.oneshot(req).await.expect("response");
            assert_eq!(resp.status(), StatusCode::OK);
        }
    }

    // ── Config endpoint tests ────────────────────────────────────────────

    mod config_endpoint {
        use super::*;
        use fx_config::manager::ConfigManager;
        use std::sync::{Arc, Mutex as StdMutex};
        use tempfile::TempDir;

        /// State for config endpoint tests.
        #[derive(Clone)]
        struct ConfigTestState {
            config_mgr: Arc<StdMutex<ConfigManager>>,
        }

        async fn test_config_get(
            State(state): State<ConfigTestState>,
            query: axum::extract::Query<std::collections::HashMap<String, String>>,
        ) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorBody>)> {
            let section = query.get("section").map(|s| s.as_str()).unwrap_or("all");
            let guard = state.config_mgr.lock().map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorBody {
                        error: format!("{e}"),
                    }),
                )
            })?;
            let value = guard
                .get(section)
                .map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorBody { error: e })))?;
            Ok(Json(value))
        }

        async fn test_config_set(
            State(state): State<ConfigTestState>,
            Json(request): Json<ConfigSetRequest>,
        ) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorBody>)> {
            let mut guard = state.config_mgr.lock().map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorBody {
                        error: format!("{e}"),
                    }),
                )
            })?;
            guard
                .set(&request.key, &request.value)
                .map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorBody { error: e })))?;
            Ok(Json(serde_json::json!({
                "updated": request.key,
                "value": request.value,
            })))
        }

        fn config_test_router(dir: &std::path::Path) -> Router {
            let mgr = ConfigManager::new(dir).expect("config manager");
            let state = ConfigTestState {
                config_mgr: Arc::new(StdMutex::new(mgr)),
            };
            Router::new()
                .route("/config", get(test_config_get).post(test_config_set))
                .with_state(state)
        }

        #[tokio::test]
        async fn config_get_returns_full_config() {
            let temp = TempDir::new().expect("tempdir");
            std::fs::write(
                temp.path().join("config.toml"),
                "[model]\ndefault_model = \"test-model\"\n",
            )
            .unwrap();
            let app = config_test_router(temp.path());
            let req = Request::builder()
                .method("GET")
                .uri("/config")
                .body(Body::empty())
                .expect("request");
            let resp = app.oneshot(req).await.expect("response");
            assert_eq!(resp.status(), StatusCode::OK);
            let body = resp.into_body().collect().await.expect("body").to_bytes();
            let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
            assert!(json.get("model").is_some());
            assert!(json.get("general").is_some());
        }

        #[tokio::test]
        async fn config_get_section_filter() {
            let temp = TempDir::new().expect("tempdir");
            std::fs::write(
                temp.path().join("config.toml"),
                "[model]\ndefault_model = \"my-model\"\n",
            )
            .unwrap();
            let app = config_test_router(temp.path());
            let req = Request::builder()
                .method("GET")
                .uri("/config?section=model")
                .body(Body::empty())
                .expect("request");
            let resp = app.oneshot(req).await.expect("response");
            assert_eq!(resp.status(), StatusCode::OK);
            let body = resp.into_body().collect().await.expect("body").to_bytes();
            let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
            assert_eq!(json["default_model"], "my-model");
        }

        #[tokio::test]
        async fn config_get_unknown_section_returns_400() {
            let temp = TempDir::new().expect("tempdir");
            let app = config_test_router(temp.path());
            let req = Request::builder()
                .method("GET")
                .uri("/config?section=bogus")
                .body(Body::empty())
                .expect("request");
            let resp = app.oneshot(req).await.expect("response");
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        }

        #[tokio::test]
        async fn config_set_updates_value() {
            let temp = TempDir::new().expect("tempdir");
            std::fs::write(
                temp.path().join("config.toml"),
                "[model]\ndefault_model = \"old\"\n",
            )
            .unwrap();
            let app = config_test_router(temp.path());
            let req = Request::builder()
                .method("POST")
                .uri("/config")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"key":"model.default_model","value":"new"}"#))
                .expect("request");
            let resp = app.oneshot(req).await.expect("response");
            assert_eq!(resp.status(), StatusCode::OK);
            let body = resp.into_body().collect().await.expect("body").to_bytes();
            let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
            assert_eq!(json["updated"], "model.default_model");
            assert_eq!(json["value"], "new");
        }

        #[tokio::test]
        async fn config_set_rejects_immutable() {
            let temp = TempDir::new().expect("tempdir");
            let app = config_test_router(temp.path());
            let req = Request::builder()
                .method("POST")
                .uri("/config")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"key":"general.data_dir","value":"/tmp"}"#))
                .expect("request");
            let resp = app.oneshot(req).await.expect("response");
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        }
    }

    // ── handle_telegram_update tests ────────────────────────────────────

    mod telegram_update {
        use super::*;
        use async_trait::async_trait;
        use fx_channel_telegram::TelegramConfig;
        use fx_kernel::act::{ToolExecutor, ToolExecutorError, ToolResult};
        use fx_kernel::budget::{BudgetConfig, BudgetTracker};
        use fx_kernel::cancellation::CancellationToken;
        use fx_kernel::context_manager::ContextCompactor;
        use fx_kernel::loop_engine::LoopEngine;
        use fx_llm::{
            CompletionProvider, CompletionRequest, CompletionResponse, CompletionStream,
            ContentBlock, ModelRouter, ProviderCapabilities, ProviderError as LlmError,
            StreamChunk,
        };
        use std::sync::Arc;
        use tokio::sync::Mutex;

        // ── Stub tool executor (no tools in headless tests) ─────────────

        #[derive(Debug)]
        struct StubToolExecutor;

        #[async_trait]
        impl ToolExecutor for StubToolExecutor {
            async fn execute_tools(
                &self,
                _calls: &[fx_llm::ToolCall],
                _cancel: Option<&CancellationToken>,
            ) -> Result<Vec<ToolResult>, ToolExecutorError> {
                Ok(Vec::new())
            }
        }

        // ── Mock completion provider (returns canned response) ──────────

        struct MockProvider;

        #[async_trait]
        impl CompletionProvider for MockProvider {
            async fn complete(
                &self,
                _request: CompletionRequest,
            ) -> Result<CompletionResponse, LlmError> {
                Ok(CompletionResponse {
                    content: vec![ContentBlock::Text {
                        text: "Mock response".to_string(),
                    }],
                    tool_calls: Vec::new(),
                    usage: None,
                    stop_reason: Some("end_turn".to_string()),
                })
            }

            async fn complete_stream(
                &self,
                _request: CompletionRequest,
            ) -> Result<CompletionStream, LlmError> {
                let chunk = StreamChunk {
                    delta_content: Some("Mock response".to_string()),
                    stop_reason: Some("end_turn".to_string()),
                    ..Default::default()
                };
                let stream = futures::stream::once(async move { Ok(chunk) });
                Ok(Box::pin(stream))
            }

            fn name(&self) -> &str {
                "mock"
            }

            fn supported_models(&self) -> Vec<String> {
                vec!["mock-model".to_string()]
            }

            fn capabilities(&self) -> ProviderCapabilities {
                ProviderCapabilities {
                    supports_temperature: false,
                    requires_streaming: false,
                }
            }
        }

        // ── Failing completion provider (always errors) ─────────────────

        struct FailingProvider;

        #[async_trait]
        impl CompletionProvider for FailingProvider {
            async fn complete(
                &self,
                _request: CompletionRequest,
            ) -> Result<CompletionResponse, LlmError> {
                Err(LlmError::Provider("simulated LLM failure".to_string()))
            }

            async fn complete_stream(
                &self,
                _request: CompletionRequest,
            ) -> Result<CompletionStream, LlmError> {
                Err(LlmError::Provider("simulated LLM failure".to_string()))
            }

            fn name(&self) -> &str {
                "failing"
            }

            fn supported_models(&self) -> Vec<String> {
                vec!["failing-model".to_string()]
            }

            fn capabilities(&self) -> ProviderCapabilities {
                ProviderCapabilities {
                    supports_temperature: false,
                    requires_streaming: false,
                }
            }
        }

        // ── Mock Telegram API server ────────────────────────────────────

        #[derive(Debug, Clone)]
        struct CapturedTelegramRequest {
            path: String,
            body: serde_json::Value,
        }

        /// Spin up a minimal HTTP server that returns `{"ok": true}` for
        /// all POSTs (mimics the Telegram Bot API enough for unit tests).
        async fn mock_telegram_server() -> (String, tokio::task::JoinHandle<()>) {
            let app = axum::Router::new().fallback(axum::routing::any(|| async {
                axum::Json(serde_json::json!({ "ok": true }))
            }));
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind mock server");
            let addr = listener.local_addr().expect("local addr");
            let base_url = format!("http://{addr}");
            let handle = tokio::spawn(async move {
                axum::serve(listener, app).await.ok();
            });
            (base_url, handle)
        }

        async fn capturing_telegram_server(
            captured: Arc<std::sync::Mutex<Vec<CapturedTelegramRequest>>>,
        ) -> (String, tokio::task::JoinHandle<()>) {
            let app = axum::Router::new().fallback(axum::routing::any(
                move |uri: axum::http::Uri, body: String| {
                    let captured = Arc::clone(&captured);
                    async move {
                        let parsed = serde_json::from_str(&body).expect("capture telegram body");
                        captured
                            .lock()
                            .expect("capture lock")
                            .push(CapturedTelegramRequest {
                                path: uri.path().to_string(),
                                body: parsed,
                            });
                        axum::Json(serde_json::json!({ "ok": true }))
                    }
                },
            ));
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind capture server");
            let addr = listener.local_addr().expect("local addr");
            let base_url = format!("http://{addr}");
            let handle = tokio::spawn(async move {
                axum::serve(listener, app).await.ok();
            });
            (base_url, handle)
        }

        // ── Test helpers ────────────────────────────────────────────────

        fn test_telegram_config() -> TelegramConfig {
            TelegramConfig {
                bot_token: "000000:TESTTOKEN".to_string(),
                allowed_chat_ids: Vec::new(),
                webhook_secret: None,
            }
        }

        fn test_engine() -> LoopEngine {
            LoopEngine::builder()
                .budget(BudgetTracker::new(BudgetConfig::default(), 0, 0))
                .context(ContextCompactor::new(2048, 256))
                .max_iterations(3)
                .tool_executor(Arc::new(StubToolExecutor))
                .synthesis_instruction("Summarize".to_string())
                .build()
                .expect("test engine")
        }

        fn make_test_app(router: ModelRouter) -> HeadlessApp {
            use crate::headless::HeadlessAppDeps;
            use fx_subagent::{
                test_support::DisabledSubagentFactory, SubagentLimits, SubagentManager,
                SubagentManagerDeps,
            };
            use std::sync::Arc;

            let subagent_manager = Arc::new(SubagentManager::new(SubagentManagerDeps {
                factory: Arc::new(DisabledSubagentFactory::new("disabled")),
                limits: SubagentLimits::default(),
            }));

            HeadlessApp::new(HeadlessAppDeps {
                loop_engine: test_engine(),
                router: Arc::new(router),
                config: fx_config::FawxConfig::default(),
                memory: None,
                system_prompt_path: None,
                config_manager: None,
                system_prompt_text: None,
                subagent_manager,
                canary_monitor: None,
            })
            .expect("test app")
        }

        fn mock_router() -> ModelRouter {
            let mut router = ModelRouter::new();
            router.register_provider(Box::new(MockProvider));
            router.set_active("mock-model").expect("set active");
            router
        }

        fn failing_router() -> ModelRouter {
            let mut router = ModelRouter::new();
            router.register_provider(Box::new(FailingProvider));
            router.set_active("failing-model").expect("set active");
            router
        }

        fn sample_update(chat_id: i64, text: &str) -> serde_json::Value {
            serde_json::json!({
                "update_id": 1001,
                "message": {
                    "message_id": 42,
                    "chat": { "id": chat_id },
                    "from": { "first_name": "TestUser" },
                    "text": text
                }
            })
        }

        // ── Tests ───────────────────────────────────────────────────────

        /// Happy path: valid update is processed, response is sent.
        #[tokio::test]
        async fn happy_path_valid_update_processed() {
            let (base_url, _server) = mock_telegram_server().await;
            let telegram = TelegramChannel::new_with_base_url(test_telegram_config(), base_url);
            let app = Arc::new(Mutex::new(make_test_app(mock_router())));

            let update = sample_update(12345, "hello bot");

            let registry = Arc::new(ChannelRegistry::new());
            let router = ResponseRouter::new(registry);

            handle_telegram_update(&telegram, &app, &router, &update).await;

            assert!(telegram.drain_outbound().is_empty());
        }

        #[tokio::test]
        async fn slash_command_update_routes_server_side_response() {
            let captured = Arc::new(std::sync::Mutex::new(Vec::new()));
            let (base_url, _server) = capturing_telegram_server(Arc::clone(&captured)).await;
            let telegram = Arc::new(TelegramChannel::new_with_base_url(
                test_telegram_config(),
                base_url,
            ));
            let app = Arc::new(Mutex::new(make_test_app(mock_router())));
            let update = sample_update(12345, "/status");
            let mut registry = ChannelRegistry::new();
            let telegram_channel: Arc<dyn Channel> = telegram.clone();
            registry.register(telegram_channel);
            let router = ResponseRouter::new(Arc::new(registry));

            handle_telegram_update(telegram.as_ref(), &app, &router, &update).await;
            flush_telegram_outbound(telegram.as_ref()).await;

            let send_message = captured
                .lock()
                .expect("capture lock")
                .iter()
                .find(|request| request.path.ends_with("/sendMessage"))
                .expect("sendMessage request")
                .clone();
            assert!(send_message.body["text"]
                .as_str()
                .expect("text body")
                .contains("Fawx Status"));
        }

        /// Parse error: invalid update JSON is handled gracefully.
        #[tokio::test]
        async fn parse_error_handled_gracefully() {
            let telegram = TelegramChannel::new(test_telegram_config());
            // App is never touched — parse fails before reaching it.
            let app = Arc::new(Mutex::new(make_test_app(mock_router())));

            // JSON that is valid serde_json::Value but fails to
            // deserialize into a Telegram Update with required fields.
            let invalid_update = serde_json::json!({
                "message": { "bad_field": true }
            });

            let registry = Arc::new(ChannelRegistry::new());
            let router = ResponseRouter::new(registry);

            handle_telegram_update(&telegram, &app, &router, &invalid_update).await;

            assert!(telegram.drain_outbound().is_empty());
        }

        /// process_message error: app returns error, error message sent.
        #[tokio::test]
        async fn process_message_error_sends_error_response() {
            let (base_url, _server) = mock_telegram_server().await;
            let telegram = TelegramChannel::new_with_base_url(test_telegram_config(), base_url);
            let app = Arc::new(Mutex::new(make_test_app(failing_router())));

            let update = sample_update(12345, "trigger error");

            let registry = Arc::new(ChannelRegistry::new());
            let router = ResponseRouter::new(registry);

            handle_telegram_update(&telegram, &app, &router, &update).await;

            assert!(telegram.drain_outbound().is_empty());
        }

        #[tokio::test]
        async fn process_message_error_sends_plain_text_telegram_error() {
            let captured = Arc::new(std::sync::Mutex::new(Vec::new()));
            let (base_url, _server) = capturing_telegram_server(Arc::clone(&captured)).await;
            let telegram = TelegramChannel::new_with_base_url(test_telegram_config(), base_url);
            let app = Arc::new(Mutex::new(make_test_app(failing_router())));
            let update = sample_update(12345, "trigger error");
            let registry = Arc::new(ChannelRegistry::new());
            let router = ResponseRouter::new(registry);

            handle_telegram_update(&telegram, &app, &router, &update).await;

            let send_message = captured
                .lock()
                .expect("capture lock")
                .iter()
                .find(|request| request.path.ends_with("/sendMessage"))
                .expect("sendMessage request")
                .clone();
            assert!(send_message.body.get("parse_mode").is_none());
        }
    }
}
