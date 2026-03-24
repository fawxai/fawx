//! OAuth bridge server for Android Codex sign-in.

use anyhow::Context;
use axum::extract::{Json, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::{rngs::OsRng, Rng};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use subtle::ConstantTimeEq;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use uuid::Uuid;

const DEFAULT_LISTEN: &str = "127.0.0.1:4318";
const DEFAULT_SCOPE: &str = "openid profile email offline_access";
const SESSION_TTL_SECS: u64 = 15 * 60;
const PKCE_LENGTH: usize = 64;
const PKCE_CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";

/// Configuration for the OAuth bridge server.
///
/// Options can be provided via CLI flags or environment variables (used as fallback).
#[derive(Debug, Clone)]
pub struct Options {
    /// Listen address for the HTTP server (e.g. `127.0.0.1:4318`).
    pub listen: String,
    /// OAuth provider authorize endpoint URL. Falls back to `FAWX_OPENAI_AUTH_URL`.
    pub auth_url: Option<String>,
    /// OAuth provider token endpoint URL. Falls back to `FAWX_OPENAI_TOKEN_URL`.
    pub token_url: Option<String>,
    /// OAuth client ID. Falls back to `FAWX_OPENAI_CLIENT_ID`.
    pub client_id: Option<String>,
    /// OAuth client secret (optional). Falls back to `FAWX_OPENAI_CLIENT_SECRET`.
    pub client_secret: Option<String>,
    /// OAuth scope string. Falls back to `FAWX_OPENAI_SCOPE`, defaults to `openid profile email offline_access`.
    pub scope: Option<String>,
}

/// OAuth provider configuration resolved from CLI flags and environment variables.
#[derive(Debug, Clone)]
pub struct BridgeSettings {
    /// OAuth authorize endpoint URL.
    pub authorize_url: String,
    /// OAuth token exchange endpoint URL.
    pub token_url: String,
    /// OAuth client identifier.
    pub client_id: String,
    /// OAuth client secret (optional for public PKCE clients).
    pub client_secret: Option<String>,
    /// OAuth scope string (defaults to `openid profile email offline_access`).
    pub scope: String,
}

impl BridgeSettings {
    fn from_options(options: &Options) -> anyhow::Result<Self> {
        let authorize_url = option_or_env(&options.auth_url, "FAWX_OPENAI_AUTH_URL")
            .context("Missing authorize URL. Set --auth-url or FAWX_OPENAI_AUTH_URL")?;
        let token_url = option_or_env(&options.token_url, "FAWX_OPENAI_TOKEN_URL")
            .context("Missing token URL. Set --token-url or FAWX_OPENAI_TOKEN_URL")?;
        let client_id = option_or_env(&options.client_id, "FAWX_OPENAI_CLIENT_ID")
            .context("Missing client ID. Set --client-id or FAWX_OPENAI_CLIENT_ID")?;
        let client_secret = option_or_env(&options.client_secret, "FAWX_OPENAI_CLIENT_SECRET");
        let scope = option_or_env(&options.scope, "FAWX_OPENAI_SCOPE")
            .unwrap_or_else(|| DEFAULT_SCOPE.to_string());

        Ok(Self {
            authorize_url,
            token_url,
            client_id,
            client_secret,
            scope,
        })
    }
}

#[derive(Debug, Clone)]
pub(crate) struct LoginSession {
    state: String,
    redirect_uri: String,
    code_verifier: String,
    created_at_unix_secs: u64,
}

/// Shared application state for the OAuth bridge server.
#[derive(Clone)]
pub struct AppState {
    /// Resolved OAuth provider configuration.
    pub settings: BridgeSettings,
    /// Active login sessions keyed by `login_id` (UUID v4).
    pub sessions: Arc<RwLock<HashMap<String, LoginSession>>>,
    /// HTTP client for upstream token exchange requests.
    pub http_client: reqwest::Client,
}

#[derive(Debug, Deserialize)]
struct StartLoginRequest {
    #[serde(default, rename = "redirect_uri")]
    redirect_uri_snake: Option<String>,
    #[serde(default, rename = "redirectUri")]
    redirect_uri_camel: Option<String>,
    #[serde(default)]
    state: Option<String>,
}

#[derive(Debug, Serialize)]
struct StartLoginResponse {
    #[serde(rename = "authUrl")]
    auth_url_camel: String,
    #[serde(rename = "auth_url")]
    auth_url_snake: String,
    #[serde(rename = "loginId")]
    login_id_camel: String,
    #[serde(rename = "login_id")]
    login_id_snake: String,
}

#[derive(Debug, Deserialize)]
struct ExchangeCodeRequest {
    code: String,
    #[serde(default)]
    state: Option<String>,
    #[serde(default, rename = "login_id")]
    login_id_snake: Option<String>,
    #[serde(default, rename = "loginId")]
    login_id_camel: Option<String>,
    #[serde(default, rename = "code_verifier")]
    code_verifier_snake: Option<String>,
    #[serde(default, rename = "codeVerifier")]
    code_verifier_camel: Option<String>,
    #[serde(default, rename = "redirect_uri")]
    redirect_uri_snake: Option<String>,
    #[serde(default, rename = "redirectUri")]
    redirect_uri_camel: Option<String>,
}

#[derive(Debug, Serialize)]
struct ExchangeCodeResponse {
    #[serde(rename = "accessToken")]
    access_token_camel: String,
    #[serde(rename = "access_token")]
    access_token_snake: String,
    #[serde(rename = "tokenType", skip_serializing_if = "Option::is_none")]
    token_type_camel: Option<String>,
    #[serde(rename = "token_type", skip_serializing_if = "Option::is_none")]
    token_type_snake: Option<String>,
    #[serde(rename = "expiresIn", skip_serializing_if = "Option::is_none")]
    expires_in_camel: Option<u64>,
    #[serde(rename = "expires_in", skip_serializing_if = "Option::is_none")]
    expires_in_snake: Option<u64>,
    #[serde(rename = "refreshToken", skip_serializing_if = "Option::is_none")]
    refresh_token_camel: Option<String>,
    #[serde(rename = "refresh_token", skip_serializing_if = "Option::is_none")]
    refresh_token_snake: Option<String>,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    ok: bool,
    service: &'static str,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: &'static str,
    error_description: String,
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    error: &'static str,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            error: "invalid_request",
            message: message.into(),
        }
    }

    fn bad_gateway(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_GATEWAY,
            error: "upstream_error",
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorResponse {
                error: self.error,
                error_description: self.message,
            }),
        )
            .into_response()
    }
}

#[derive(Debug, Clone)]
struct PreparedExchangeInput {
    code: String,
    redirect_uri: String,
    code_verifier: String,
}

/// Runs the OAuth bridge HTTP server.
///
/// Starts an Axum-based server that proxies OAuth PKCE flows between the
/// Android client and the upstream provider. Returns `Ok(0)` on clean shutdown.
///
/// # Errors
///
/// Returns an error if required configuration is missing or the server fails to bind.
pub async fn run(options: Options) -> anyhow::Result<i32> {
    let listen = resolve_listen_address(&options);
    let settings = BridgeSettings::from_options(&options)?;
    let state = build_app_state(&settings);

    spawn_session_cleanup(state.clone());

    let app = build_router(state);
    let listener = bind_listener(&listen).await?;

    print_startup_banner(&listen, &settings);
    axum::serve(listener, app).await?;
    Ok(0)
}

fn resolve_listen_address(options: &Options) -> String {
    let trimmed = options.listen.trim();
    if trimmed.is_empty() {
        DEFAULT_LISTEN.to_string()
    } else {
        trimmed.to_string()
    }
}

fn build_app_state(settings: &BridgeSettings) -> AppState {
    AppState {
        settings: settings.clone(),
        sessions: Arc::new(RwLock::new(HashMap::new())),
        http_client: reqwest::Client::new(),
    }
}

fn spawn_session_cleanup(state: AppState) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            let mut sessions = state.sessions.write().await;
            evict_expired_sessions(&mut sessions);
        }
    });
}

async fn bind_listener(listen: &str) -> anyhow::Result<TcpListener> {
    TcpListener::bind(listen)
        .await
        .with_context(|| format!("Failed to bind OAuth bridge on {listen}"))
}

fn print_startup_banner(listen: &str, settings: &BridgeSettings) {
    println!("Fawx OAuth bridge listening on http://{listen}");
    println!("Authorize URL: {}", settings.authorize_url);
    println!("Token URL: {}", settings.token_url);
}

/// Builds the Axum router with all endpoints.
///
/// Exposed for testing purposes.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/oauth/codex/start", post(start_login))
        .route("/oauth/start", post(start_login))
        .route("/account/login/start", post(start_login))
        .route("/oauth/codex/exchange", post(exchange_code))
        .route("/oauth/exchange", post(exchange_code))
        .route("/account/login/exchange", post(exchange_code))
        .with_state(state)
}

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    let healthy = !state.settings.authorize_url.is_empty()
        && !state.settings.token_url.is_empty()
        && !state.settings.client_id.is_empty();
    Json(HealthResponse {
        ok: healthy,
        service: "fawx-oauth-bridge",
    })
}

async fn start_login(
    State(state): State<AppState>,
    Json(request): Json<StartLoginRequest>,
) -> Result<Json<StartLoginResponse>, ApiError> {
    let redirect_uri = request
        .redirect_uri_snake
        .or(request.redirect_uri_camel)
        .and_then(|value| trim_non_empty(&value))
        .ok_or_else(|| ApiError::bad_request("Missing redirect_uri"))?;

    validate_redirect_uri(&redirect_uri)?;

    let caller_state = request
        .state
        .and_then(|value| trim_non_empty(&value))
        .ok_or_else(|| ApiError::bad_request("Missing state"))?;

    let login_id = Uuid::new_v4().to_string();
    let code_verifier = generate_pkce_verifier();
    let code_challenge = pkce_code_challenge_s256(&code_verifier);

    let auth_url = build_authorize_url(
        &state.settings,
        &redirect_uri,
        &caller_state,
        &code_challenge,
    )?;

    let session = LoginSession {
        state: caller_state,
        redirect_uri,
        code_verifier,
        created_at_unix_secs: now_unix_secs(),
    };

    {
        let mut sessions = state.sessions.write().await;
        evict_expired_sessions(&mut sessions);
        sessions.insert(login_id.clone(), session);
    }

    Ok(Json(StartLoginResponse {
        auth_url_camel: auth_url.clone(),
        auth_url_snake: auth_url,
        login_id_camel: login_id.clone(),
        login_id_snake: login_id,
    }))
}

async fn exchange_code(
    State(state): State<AppState>,
    Json(request): Json<ExchangeCodeRequest>,
) -> Result<Json<ExchangeCodeResponse>, ApiError> {
    let prepared = prepare_exchange_input(&state, request).await?;
    let provider_response = exchange_with_provider(&state, prepared).await?;

    Ok(Json(ExchangeCodeResponse {
        access_token_camel: provider_response.access_token.clone(),
        access_token_snake: provider_response.access_token,
        token_type_camel: provider_response.token_type.clone(),
        token_type_snake: provider_response.token_type,
        expires_in_camel: provider_response.expires_in,
        expires_in_snake: provider_response.expires_in,
        refresh_token_camel: provider_response.refresh_token.clone(),
        refresh_token_snake: provider_response.refresh_token,
    }))
}

async fn prepare_exchange_input(
    state: &AppState,
    request: ExchangeCodeRequest,
) -> Result<PreparedExchangeInput, ApiError> {
    let params = ExchangeRequestParams::from_request(request)?;

    if let Some(login_id) = params.login_id.clone() {
        return prepare_exchange_from_session(state, params.code, login_id, params.state).await;
    }

    prepare_exchange_from_direct_values(params)
}

#[derive(Debug)]
struct ExchangeRequestParams {
    code: String,
    login_id: Option<String>,
    state: Option<String>,
    code_verifier: Option<String>,
    redirect_uri: Option<String>,
}

impl ExchangeRequestParams {
    fn from_request(request: ExchangeCodeRequest) -> Result<Self, ApiError> {
        let code =
            trim_non_empty(&request.code).ok_or_else(|| ApiError::bad_request("Missing code"))?;

        Ok(Self {
            code,
            login_id: request
                .login_id_snake
                .or(request.login_id_camel)
                .and_then(|value| trim_non_empty(&value)),
            state: request.state.and_then(|value| trim_non_empty(&value)),
            code_verifier: request
                .code_verifier_snake
                .or(request.code_verifier_camel)
                .and_then(|value| trim_non_empty(&value)),
            redirect_uri: request
                .redirect_uri_snake
                .or(request.redirect_uri_camel)
                .and_then(|value| trim_non_empty(&value)),
        })
    }
}

async fn prepare_exchange_from_session(
    state: &AppState,
    code: String,
    login_id: String,
    state_value: Option<String>,
) -> Result<PreparedExchangeInput, ApiError> {
    let session = load_session(state, &login_id).await?;
    validate_session_state(state_value, &session)?;
    remove_session(state, &login_id).await;

    Ok(PreparedExchangeInput {
        code,
        redirect_uri: session.redirect_uri,
        code_verifier: session.code_verifier,
    })
}

async fn load_session(state: &AppState, login_id: &str) -> Result<LoginSession, ApiError> {
    let sessions = state.sessions.read().await;
    sessions
        .get(login_id)
        .cloned()
        .ok_or_else(|| ApiError::bad_request("Unknown or expired login_id"))
}

fn validate_session_state(
    incoming_state: Option<String>,
    session: &LoginSession,
) -> Result<(), ApiError> {
    let incoming_state =
        incoming_state.ok_or_else(|| ApiError::bad_request("Missing state parameter"))?;

    if constant_time_eq(incoming_state.as_bytes(), session.state.as_bytes()) {
        Ok(())
    } else {
        Err(ApiError::bad_request("State mismatch for login session"))
    }
}

async fn remove_session(state: &AppState, login_id: &str) {
    let mut sessions = state.sessions.write().await;
    evict_expired_sessions(&mut sessions);
    sessions.remove(login_id);
}

fn prepare_exchange_from_direct_values(
    params: ExchangeRequestParams,
) -> Result<PreparedExchangeInput, ApiError> {
    let code_verifier = params
        .code_verifier
        .ok_or_else(|| ApiError::bad_request("Missing code_verifier or login_id"))?;
    let redirect_uri = params
        .redirect_uri
        .ok_or_else(|| ApiError::bad_request("Missing redirect_uri or login_id"))?;

    Ok(PreparedExchangeInput {
        code: params.code,
        redirect_uri,
        code_verifier,
    })
}

#[derive(Debug)]
struct ProviderTokenResponse {
    access_token: String,
    token_type: Option<String>,
    expires_in: Option<u64>,
    refresh_token: Option<String>,
}

async fn exchange_with_provider(
    state: &AppState,
    input: PreparedExchangeInput,
) -> Result<ProviderTokenResponse, ApiError> {
    let form = build_token_form(state, input);
    let (status, body) = request_token_exchange(state, &form).await?;
    ensure_success_status(status, &body)?;
    parse_token_response(&body)
}

fn build_token_form(state: &AppState, input: PreparedExchangeInput) -> Vec<(String, String)> {
    let mut form = vec![
        ("grant_type".to_string(), "authorization_code".to_string()),
        ("code".to_string(), input.code),
        ("client_id".to_string(), state.settings.client_id.clone()),
        ("redirect_uri".to_string(), input.redirect_uri),
        ("code_verifier".to_string(), input.code_verifier),
    ];

    if let Some(secret) = &state.settings.client_secret {
        form.push(("client_secret".to_string(), secret.clone()));
    }

    form
}

async fn request_token_exchange(
    state: &AppState,
    form: &[(String, String)],
) -> Result<(StatusCode, String), ApiError> {
    let response = state
        .http_client
        .post(&state.settings.token_url)
        .form(form)
        .send()
        .await
        .map_err(|error| ApiError::bad_gateway(format!("Token request failed: {error}")))?;

    let status = response.status();
    let body = response.text().await.map_err(|error| {
        ApiError::bad_gateway(format!("Failed to read token response: {error}"))
    })?;

    Ok((status, body))
}

fn ensure_success_status(status: StatusCode, body: &str) -> Result<(), ApiError> {
    if status.is_success() {
        return Ok(());
    }

    let safe_message = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|json| json.get("error").and_then(|e| e.as_str()).map(String::from))
        .unwrap_or_else(|| "upstream_error".to_string());

    Err(ApiError::bad_gateway(format!(
        "Token endpoint returned {}: {}",
        status.as_u16(),
        safe_message
    )))
}

fn parse_token_response(body: &str) -> Result<ProviderTokenResponse, ApiError> {
    let json_value: Value = serde_json::from_str(body).map_err(|error| {
        ApiError::bad_gateway(format!("Token endpoint returned invalid JSON: {error}"))
    })?;

    let access_token = json_string(&json_value, &["access_token", "accessToken", "token"])
        .ok_or_else(|| ApiError::bad_gateway("Token endpoint response missing access token"))?;

    Ok(ProviderTokenResponse {
        access_token,
        token_type: json_string(&json_value, &["token_type", "tokenType"]),
        expires_in: json_u64(&json_value, &["expires_in", "expiresIn"]),
        refresh_token: json_string(&json_value, &["refresh_token", "refreshToken"]),
    })
}

fn build_authorize_url(
    settings: &BridgeSettings,
    redirect_uri: &str,
    state: &str,
    code_challenge: &str,
) -> Result<String, ApiError> {
    let mut url = Url::parse(&settings.authorize_url)
        .map_err(|error| ApiError::bad_request(format!("Invalid authorize URL: {error}")))?;

    {
        let mut query = url.query_pairs_mut();
        query.append_pair("response_type", "code");
        query.append_pair("client_id", &settings.client_id);
        query.append_pair("redirect_uri", redirect_uri);
        query.append_pair("scope", &settings.scope);
        query.append_pair("state", state);
        query.append_pair("code_challenge", code_challenge);
        query.append_pair("code_challenge_method", "S256");
    }

    Ok(url.into())
}

fn validate_redirect_uri(uri: &str) -> Result<(), ApiError> {
    // Allow fawx:// custom scheme and localhost for testing
    if uri.starts_with("fawx://")
        || uri.starts_with("http://localhost")
        || uri.starts_with("http://127.0.0.1")
    {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            "Invalid redirect_uri: must use fawx:// scheme or localhost",
        ))
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.ct_eq(b).into()
}

fn evict_expired_sessions(sessions: &mut HashMap<String, LoginSession>) {
    let now = now_unix_secs();
    sessions
        .retain(|_, session| now.saturating_sub(session.created_at_unix_secs) <= SESSION_TTL_SECS);
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_else(|e| {
            eprintln!("Warning: system time error: {e}");
            0
        })
}

fn trim_non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn option_or_env(value: &Option<String>, env_key: &str) -> Option<String> {
    value.as_deref().and_then(trim_non_empty).or_else(|| {
        std::env::var(env_key)
            .ok()
            .and_then(|raw| trim_non_empty(&raw))
    })
}

fn generate_pkce_verifier() -> String {
    let mut rng = OsRng;
    (0..PKCE_LENGTH)
        .map(|_| {
            let idx = rng.gen_range(0..PKCE_CHARSET.len());
            PKCE_CHARSET[idx] as char
        })
        .collect()
}

fn pkce_code_challenge_s256(code_verifier: &str) -> String {
    let digest = Sha256::digest(code_verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

fn json_string(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .and_then(trim_non_empty)
}

fn json_u64(value: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|key| {
        let node = value.get(*key)?;
        if let Some(number) = node.as_u64() {
            return Some(number);
        }
        node.as_str()?.parse::<u64>().ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode as HttpStatusCode};
    use http_body_util::BodyExt;
    use serde_json::json;
    use tower::ServiceExt;

    fn test_state() -> AppState {
        AppState {
            settings: BridgeSettings {
                authorize_url: "https://auth.example.com/oauth2/authorize".to_string(),
                token_url: "https://auth.example.com/oauth2/token".to_string(),
                client_id: "test-client".to_string(),
                client_secret: Some("test-secret".to_string()),
                scope: "openid profile".to_string(),
            },
            sessions: Arc::new(RwLock::new(HashMap::new())),
            http_client: reqwest::Client::new(),
        }
    }

    #[tokio::test]
    async fn test_start_login_valid_request() {
        let app = build_router(test_state());
        let request = Request::builder()
            .method("POST")
            .uri("/oauth/codex/start")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "redirect_uri": "fawx://oauth/callback",
                    "state": "test-state-123"
                })
                .to_string(),
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), HttpStatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // Should NOT contain code_verifier
        assert!(json.get("codeVerifier").is_none());
        assert!(json.get("code_verifier").is_none());

        // Should contain auth_url and login_id
        assert!(json.get("authUrl").is_some());
        assert!(json.get("loginId").is_some());
    }

    #[tokio::test]
    async fn test_start_login_missing_redirect_uri() {
        let app = build_router(test_state());
        let request = Request::builder()
            .method("POST")
            .uri("/oauth/codex/start")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "state": "test-state"
                })
                .to_string(),
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), HttpStatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_start_login_missing_state() {
        let app = build_router(test_state());
        let request = Request::builder()
            .method("POST")
            .uri("/oauth/codex/start")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "redirect_uri": "fawx://oauth/callback"
                })
                .to_string(),
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), HttpStatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_start_login_invalid_redirect_uri() {
        let app = build_router(test_state());
        let request = Request::builder()
            .method("POST")
            .uri("/oauth/codex/start")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "redirect_uri": "https://evil.com/callback",
                    "state": "test-state"
                })
                .to_string(),
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), HttpStatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_exchange_unknown_login_id() {
        let app = build_router(test_state());
        let request = Request::builder()
            .method("POST")
            .uri("/oauth/codex/exchange")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "code": "auth-code-123",
                    "login_id": "unknown-id",
                    "state": "test-state"
                })
                .to_string(),
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), HttpStatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_exchange_missing_state() {
        let state = test_state();

        // First create a session
        let login_id = "test-login-id".to_string();
        let session = LoginSession {
            state: "expected-state".to_string(),
            redirect_uri: "fawx://oauth/callback".to_string(),
            code_verifier: "test-verifier".to_string(),
            created_at_unix_secs: now_unix_secs(),
        };
        state
            .sessions
            .write()
            .await
            .insert(login_id.clone(), session);

        let app = build_router(state);
        let request = Request::builder()
            .method("POST")
            .uri("/oauth/codex/exchange")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "code": "auth-code-123",
                    "login_id": login_id
                    // Missing state
                })
                .to_string(),
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), HttpStatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_exchange_state_mismatch() {
        let state = test_state();

        // Create a session with expected state
        let login_id = "test-login-id".to_string();
        let session = LoginSession {
            state: "expected-state".to_string(),
            redirect_uri: "fawx://oauth/callback".to_string(),
            code_verifier: "test-verifier".to_string(),
            created_at_unix_secs: now_unix_secs(),
        };
        state
            .sessions
            .write()
            .await
            .insert(login_id.clone(), session);

        let app = build_router(state);
        let request = Request::builder()
            .method("POST")
            .uri("/oauth/codex/exchange")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "code": "auth-code-123",
                    "login_id": login_id,
                    "state": "wrong-state"
                })
                .to_string(),
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), HttpStatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_health_endpoint_with_valid_config() {
        let app = build_router(test_state());
        let request = Request::builder()
            .method("GET")
            .uri("/health")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), HttpStatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json.get("ok").and_then(|v| v.as_bool()), Some(true));
    }

    #[tokio::test]
    async fn test_health_endpoint_with_empty_config() {
        let state = AppState {
            settings: BridgeSettings {
                authorize_url: "".to_string(),
                token_url: "".to_string(),
                client_id: "".to_string(),
                client_secret: None,
                scope: "".to_string(),
            },
            sessions: Arc::new(RwLock::new(HashMap::new())),
            http_client: reqwest::Client::new(),
        };
        let app = build_router(state);
        let request = Request::builder()
            .method("GET")
            .uri("/health")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), HttpStatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json.get("ok").and_then(|v| v.as_bool()), Some(false));
    }

    #[tokio::test]
    async fn test_session_eviction() {
        let mut sessions = HashMap::new();

        // Create an expired session
        let expired_id = "expired-id".to_string();
        let expired_session = LoginSession {
            state: "test-state".to_string(),
            redirect_uri: "fawx://oauth/callback".to_string(),
            code_verifier: "test-verifier".to_string(),
            created_at_unix_secs: now_unix_secs() - SESSION_TTL_SECS - 100,
        };
        sessions.insert(expired_id.clone(), expired_session);

        // Create a valid session
        let valid_id = "valid-id".to_string();
        let valid_session = LoginSession {
            state: "test-state".to_string(),
            redirect_uri: "fawx://oauth/callback".to_string(),
            code_verifier: "test-verifier".to_string(),
            created_at_unix_secs: now_unix_secs(),
        };
        sessions.insert(valid_id.clone(), valid_session);

        evict_expired_sessions(&mut sessions);

        assert!(!sessions.contains_key(&expired_id));
        assert!(sessions.contains_key(&valid_id));
    }

    #[test]
    fn option_or_env_prefers_option_value() {
        let option = Some("https://example.com/auth".to_string());
        let resolved = option_or_env(&option, "FAWX_TEST_MISSING");
        assert_eq!(resolved.as_deref(), Some("https://example.com/auth"));
    }

    #[test]
    fn build_authorize_url_contains_required_parameters() {
        let settings = BridgeSettings {
            authorize_url: "https://auth.example.com/oauth2/authorize".to_string(),
            token_url: "https://auth.example.com/oauth2/token".to_string(),
            client_id: "client-123".to_string(),
            client_secret: None,
            scope: "openid profile".to_string(),
        };

        let url = build_authorize_url(
            &settings,
            "fawx://oauth/callback",
            "state-123",
            "challenge-xyz",
        )
        .expect("should build auth URL");

        assert!(url.contains("response_type=code"));
        assert!(url.contains("client_id=client-123"));
        assert!(url.contains("redirect_uri=fawx%3A%2F%2Foauth%2Fcallback"));
        assert!(url.contains("state=state-123"));
        assert!(url.contains("code_challenge=challenge-xyz"));
        assert!(url.contains("code_challenge_method=S256"));
    }

    #[test]
    fn json_extractors_support_multiple_key_formats() {
        let value: Value = serde_json::json!({
            "access_token": "token-1",
            "expiresIn": "3600"
        });

        assert_eq!(
            json_string(&value, &["accessToken", "access_token"]).as_deref(),
            Some("token-1")
        );
        assert_eq!(json_u64(&value, &["expires_in", "expiresIn"]), Some(3600));
    }

    #[test]
    fn pkce_verifier_and_challenge_formats_are_valid() {
        let verifier = generate_pkce_verifier();
        assert_eq!(verifier.len(), PKCE_LENGTH);
        assert!(verifier
            .chars()
            .all(|ch| PKCE_CHARSET.contains(&(ch as u8))));

        let challenge = pkce_code_challenge_s256(&verifier);
        assert!(!challenge.is_empty());
        assert!(!challenge.contains('='));
    }

    #[test]
    fn redirect_uri_validation_allows_fawx_scheme() {
        assert!(validate_redirect_uri("fawx://oauth/callback").is_ok());
    }

    #[test]
    fn redirect_uri_validation_allows_localhost() {
        assert!(validate_redirect_uri("http://localhost:3000/callback").is_ok());
        assert!(validate_redirect_uri("http://127.0.0.1:3000/callback").is_ok());
    }

    #[test]
    fn redirect_uri_validation_rejects_external_urls() {
        assert!(validate_redirect_uri("https://evil.com/callback").is_err());
    }

    #[test]
    fn constant_time_eq_returns_true_for_equal_strings() {
        assert!(constant_time_eq(b"test", b"test"));
    }

    #[test]
    fn constant_time_eq_returns_false_for_different_strings() {
        assert!(!constant_time_eq(b"test", b"different"));
    }

    #[test]
    fn constant_time_eq_returns_false_for_different_lengths() {
        assert!(!constant_time_eq(b"test", b"testing"));
    }
}
