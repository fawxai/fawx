use crate::state::HttpState;
use crate::types::ErrorBody;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use fx_auth::auth::AuthMethod;
use fx_auth::oauth::{
    extract_openai_account_id, PkceFlow, TokenExchangeRequest, TokenRefreshRequest, TokenResponse,
    OPENAI_CLIENT_ID, OPENAI_TOKEN_URL,
};
use ring::rand::SecureRandom;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::auth::save_auth_method;
use super::HandlerResult;

const FLOW_TTL_SECONDS: u64 = 600;
const MAX_CONCURRENT_FLOWS: usize = 10;
const APP_REDIRECT_URI: &str = "http://localhost:1455/auth/callback";

pub struct OAuthFlowStore {
    flows: Mutex<HashMap<String, StoredFlow>>,
}

struct StoredFlow {
    flow: PkceFlow,
    created_at: Instant,
}

impl OAuthFlowStore {
    pub fn new() -> Self {
        Self {
            flows: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for OAuthFlowStore {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OAuthStartResponse {
    pub provider: String,
    pub authorize_url: String,
    pub flow_token: String,
    pub redirect_uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OAuthCallbackRequest {
    pub code: String,
    pub flow_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OAuthCallbackResponse {
    pub provider: String,
    pub status: String,
    pub auth_method: String,
    pub verified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OAuthRefreshResponse {
    pub provider: String,
    pub status: String,
    pub expires_at: u64,
}

pub async fn handle_oauth_start(
    State(state): State<HttpState>,
    Path(provider): Path<String>,
) -> HandlerResult<Json<OAuthStartResponse>> {
    validate_oauth_provider(&provider)?;
    let flow = PkceFlow::try_new()
        .map_err(|error| internal_error(error.to_string()))?
        .with_redirect_uri(APP_REDIRECT_URI);
    let authorize_url = flow.authorization_url(OPENAI_CLIENT_ID);
    let flow_token = store_flow(state.oauth_flows.as_ref(), flow)?;

    tracing::info!(provider = %provider, "OAuth flow started");
    Ok(Json(OAuthStartResponse {
        provider,
        authorize_url,
        flow_token,
        redirect_uri: APP_REDIRECT_URI.to_string(),
    }))
}

pub async fn handle_oauth_callback(
    State(state): State<HttpState>,
    Path(provider): Path<String>,
    Json(request): Json<OAuthCallbackRequest>,
) -> HandlerResult<Json<OAuthCallbackResponse>> {
    validate_oauth_provider(&provider)?;
    let flow = retrieve_flow(state.oauth_flows.as_ref(), &request.flow_token)?;
    tracing::info!(provider = %provider, "OAuth callback: exchanging authorization code");

    let token_response = exchange_token(&flow, &request.code)
        .await
        .map_err(|error| {
            tracing::warn!(provider = %provider, error = %error, "OAuth token exchange failed");
            bad_gateway(error)
        })?;
    let expires_in = token_response.expires_in;

    store_oauth_credential(&state, &provider, token_response).await?;
    tracing::info!(provider = %provider, expires_in, "OAuth authentication successful");

    Ok(Json(OAuthCallbackResponse {
        provider,
        status: "authenticated".to_string(),
        auth_method: "oauth".to_string(),
        verified: true,
    }))
}

pub async fn handle_oauth_refresh(
    State(state): State<HttpState>,
    Path(provider): Path<String>,
) -> HandlerResult<Json<OAuthRefreshResponse>> {
    validate_oauth_provider(&provider)?;
    let refresh_token = load_refresh_token(&state, &provider)?;
    tracing::info!(provider = %provider, "OAuth: refreshing access token");

    let token_response = refresh_access_token(&refresh_token)
        .await
        .map_err(|error| {
            tracing::warn!(provider = %provider, error = %error, "OAuth token refresh failed");
            bad_gateway(error)
        })?;
    let expires_in = token_response.expires_in;
    store_oauth_credential(&state, &provider, token_response).await?;
    let expires_at = expires_at_ms(expires_in).map_err(internal_error)?;
    tracing::info!(provider = %provider, "OAuth token refreshed successfully");

    Ok(Json(OAuthRefreshResponse {
        provider,
        status: "authenticated".to_string(),
        expires_at,
    }))
}

fn load_refresh_token(state: &HttpState, provider: &str) -> HandlerResult<String> {
    let store = crate::auth_store::AuthStore::open(&state.data_dir).map_err(internal_error)?;
    let auth_manager = store.load_auth_manager().map_err(internal_error)?;

    match auth_manager.get(provider) {
        Some(AuthMethod::OAuth { refresh_token, .. }) => Ok(refresh_token.clone()),
        Some(_) => Err(error_response(
            StatusCode::BAD_REQUEST,
            format!("Provider '{provider}' is not using OAuth authentication"),
        )),
        None => Err(error_response(
            StatusCode::NOT_FOUND,
            format!("No credentials found for provider '{provider}'"),
        )),
    }
}

async fn exchange_token(flow: &PkceFlow, code: &str) -> Result<TokenResponse, String> {
    exchange_token_with_url(flow, code, OPENAI_TOKEN_URL).await
}

async fn exchange_token_with_url(
    flow: &PkceFlow,
    code: &str,
    token_url: &str,
) -> Result<TokenResponse, String> {
    let response = reqwest::Client::new()
        .post(token_url)
        .form(&build_token_exchange_request(flow, code))
        .send()
        .await
        .map_err(|error| format!("token exchange request failed: {error}"))?;

    parse_token_exchange_response(response).await
}

fn build_token_exchange_request(flow: &PkceFlow, code: &str) -> TokenExchangeRequest {
    TokenExchangeRequest {
        grant_type: "authorization_code".to_string(),
        code: code.to_string(),
        redirect_uri: flow.redirect_uri().to_string(),
        code_verifier: flow.code_verifier().to_string(),
        client_id: OPENAI_CLIENT_ID.to_string(),
    }
}

async fn refresh_access_token(refresh_token: &str) -> Result<TokenResponse, String> {
    refresh_access_token_with_url(refresh_token, OPENAI_TOKEN_URL).await
}

async fn refresh_access_token_with_url(
    refresh_token: &str,
    token_url: &str,
) -> Result<TokenResponse, String> {
    let request = TokenRefreshRequest {
        grant_type: "refresh_token".to_string(),
        refresh_token: refresh_token.to_string(),
        client_id: OPENAI_CLIENT_ID.to_string(),
    };
    let response = reqwest::Client::new()
        .post(token_url)
        .form(&request)
        .send()
        .await
        .map_err(|error| format!("token refresh request failed: {error}"))?;

    parse_token_exchange_response(response).await
}

async fn parse_token_exchange_response(
    response: reqwest::Response,
) -> Result<TokenResponse, String> {
    let status = response.status();
    if !status.is_success() {
        let body = token_exchange_error_body(response).await;
        return Err(format!("token exchange failed ({status}): {body}"));
    }

    response
        .json::<TokenResponse>()
        .await
        .map_err(|error| format!("failed to parse token response: {error}"))
}

async fn token_exchange_error_body(response: reqwest::Response) -> String {
    match response.text().await {
        Ok(body) if !body.is_empty() => body,
        Ok(_) => "empty response body".to_string(),
        Err(error) => format!("failed to read error body: {error}"),
    }
}

async fn store_oauth_credential(
    state: &HttpState,
    provider: &str,
    token_response: TokenResponse,
) -> HandlerResult<()> {
    let auth_method = oauth_auth_method(provider, token_response).map_err(internal_error)?;
    save_auth_method(state, provider, auth_method).await
}

fn oauth_auth_method(provider: &str, token_response: TokenResponse) -> Result<AuthMethod, String> {
    let account_id = extract_openai_account_id(&token_response.access_token);
    let expires_at = expires_at_ms(token_response.expires_in)?;

    Ok(AuthMethod::OAuth {
        provider: provider.to_string(),
        access_token: token_response.access_token,
        refresh_token: token_response.refresh_token,
        expires_at,
        account_id,
    })
}

fn expires_at_ms(expires_in_seconds: u64) -> Result<u64, String> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("failed to compute OAuth token expiry: {error}"))?;
    let now_ms = u64::try_from(elapsed.as_millis())
        .map_err(|_| "failed to compute OAuth token expiry: timestamp overflow".to_string())?;

    Ok(now_ms.saturating_add(expires_in_seconds.saturating_mul(1_000)))
}

fn validate_oauth_provider(provider: &str) -> HandlerResult<()> {
    if provider == "openai" {
        return Ok(());
    }
    Err(validation_error(format!(
        "OAuth not supported for provider '{provider}'"
    )))
}

fn store_flow(store: &OAuthFlowStore, flow: PkceFlow) -> HandlerResult<String> {
    let mut flows = lock_flows(store)?;
    let now = Instant::now();
    prune_expired_flows(&mut flows, now);
    if flows.len() >= MAX_CONCURRENT_FLOWS {
        tracing::warn!("OAuth flow store full, rejecting new flow");
        return Err(error_response(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many concurrent OAuth flows",
        ));
    }

    let flow_token = generate_flow_token().map_err(internal_error)?;
    flows.insert(
        flow_token.clone(),
        StoredFlow {
            flow,
            created_at: now,
        },
    );
    Ok(flow_token)
}

fn retrieve_flow(store: &OAuthFlowStore, token: &str) -> HandlerResult<PkceFlow> {
    let mut flows = lock_flows(store)?;
    prune_expired_flows(&mut flows, Instant::now());
    flows
        .remove(token)
        .map(|stored| stored.flow)
        .ok_or_else(|| {
            tracing::warn!(
                flow_token_prefix = %flow_token_prefix(token),
                "OAuth flow expired or not found"
            );
            invalid_flow_error()
        })
}

fn lock_flows(
    store: &OAuthFlowStore,
) -> HandlerResult<MutexGuard<'_, HashMap<String, StoredFlow>>> {
    store.flows.lock().map_err(|error| {
        tracing::error!(error = %error, "oauth flow store lock failed");
        internal_error("internal_error")
    })
}

fn prune_expired_flows(flows: &mut HashMap<String, StoredFlow>, now: Instant) {
    flows.retain(|_, stored| !is_expired(stored, now));
}

fn is_expired(stored: &StoredFlow, now: Instant) -> bool {
    now.duration_since(stored.created_at) >= Duration::from_secs(FLOW_TTL_SECONDS)
}

fn invalid_flow_error() -> (StatusCode, Json<ErrorBody>) {
    validation_error("Invalid or expired flow token")
}

fn flow_token_prefix(token: &str) -> String {
    token.chars().take(8).collect()
}

fn generate_flow_token() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    ring::rand::SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| "Failed to generate random flow token".to_string())?;

    Ok(bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>())
}

fn validation_error(error: impl Into<String>) -> (StatusCode, Json<ErrorBody>) {
    error_response(StatusCode::BAD_REQUEST, error)
}

fn internal_error(error: impl Into<String>) -> (StatusCode, Json<ErrorBody>) {
    error_response(StatusCode::INTERNAL_SERVER_ERROR, error)
}

fn bad_gateway(error: impl Into<String>) -> (StatusCode, Json<ErrorBody>) {
    error_response(StatusCode::BAD_GATEWAY, error)
}

fn error_response(status: StatusCode, error: impl Into<String>) -> (StatusCode, Json<ErrorBody>) {
    (
        status,
        Json(ErrorBody {
            error: error.into(),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_response_serializes() {
        let response = OAuthStartResponse {
            provider: "openai".to_string(),
            authorize_url: "https://auth.openai.com/oauth/authorize?client_id=test".to_string(),
            flow_token: "oauth_123".to_string(),
            redirect_uri: APP_REDIRECT_URI.to_string(),
        };

        let json = serde_json::to_string(&response).expect("serialize response");
        let decoded: OAuthStartResponse =
            serde_json::from_str(&json).expect("deserialize response");
        assert_eq!(decoded, response);
    }

    #[test]
    fn callback_request_deserializes() {
        let json = r#"{"code":"auth-code","flow_token":"oauth_123"}"#;

        let request: OAuthCallbackRequest =
            serde_json::from_str(json).expect("deserialize request");

        assert_eq!(request.code, "auth-code");
        assert_eq!(request.flow_token, "oauth_123");
    }

    #[test]
    fn callback_response_serializes() {
        let response = OAuthCallbackResponse {
            provider: "openai".to_string(),
            status: "authenticated".to_string(),
            auth_method: "oauth".to_string(),
            verified: true,
        };

        let json = serde_json::to_string(&response).expect("serialize response");
        let decoded: OAuthCallbackResponse =
            serde_json::from_str(&json).expect("deserialize response");
        assert_eq!(decoded, response);
    }

    #[test]
    fn refresh_response_serializes_and_round_trips() {
        let response = OAuthRefreshResponse {
            provider: "openai".to_string(),
            status: "authenticated".to_string(),
            expires_at: 1_742_000_000,
        };

        let json = serde_json::to_string(&response).expect("serialize");
        let decoded: OAuthRefreshResponse = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, response);
        assert_eq!(decoded.expires_at, 1_742_000_000u64);
    }

    #[test]
    fn load_refresh_token_returns_not_found_for_missing_provider() {
        let temp = tempfile::TempDir::new().expect("tempdir");

        let error = load_refresh_token_from_dir(temp.path(), "openai").expect_err("should fail");
        assert_eq!(error.0, StatusCode::NOT_FOUND);
        assert!(error.1 .0.error.contains("No credentials found"));
    }

    #[test]
    fn load_refresh_token_returns_bad_request_for_non_oauth_provider() {
        let temp = tempfile::TempDir::new().expect("tempdir");

        // Store an API key credential (not OAuth)
        crate::auth_store::AuthStore::open(temp.path())
            .and_then(|store| {
                let mut manager = store.load_auth_manager()?;
                manager.store(
                    "openai",
                    AuthMethod::ApiKey {
                        provider: "openai".to_string(),
                        key: "sk-test".to_string(),
                    },
                );
                store.save_auth_manager(&manager)
            })
            .expect("store api key");

        let error = load_refresh_token_from_dir(temp.path(), "openai").expect_err("should fail");
        assert_eq!(error.0, StatusCode::BAD_REQUEST);
        assert!(error.1 .0.error.contains("not using OAuth"));
    }

    #[test]
    fn flow_store_stores_hex_token_and_retrieves() {
        let store = OAuthFlowStore::new();
        let flow = PkceFlow::try_new().expect("pkce flow");

        let token = store_flow(&store, flow.clone()).expect("store flow");
        let retrieved = retrieve_flow(&store, &token).expect("retrieve flow");

        assert_eq!(token.len(), 32);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(retrieved, flow);
        assert!(store.flows.lock().expect("lock store").is_empty());
    }

    #[test]
    fn flow_store_rejects_unknown_token() {
        let store = OAuthFlowStore::new();

        let error = retrieve_flow(&store, "oauth_missing").expect_err("unknown token");

        assert_eq!(error.0, StatusCode::BAD_REQUEST);
        assert_eq!(error.1 .0.error, "Invalid or expired flow token");
    }

    #[test]
    fn flow_store_enforces_max_concurrent() {
        let store = OAuthFlowStore::new();
        seed_store(&store, MAX_CONCURRENT_FLOWS, Instant::now());

        let error = store_flow(&store, PkceFlow::try_new().expect("pkce flow"))
            .expect_err("max concurrent flows");

        assert_eq!(error.0, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(error.1 .0.error, "Too many concurrent OAuth flows");
    }

    #[test]
    fn flow_store_rejects_expired_token() {
        let store = OAuthFlowStore::new();
        let created_at = Instant::now() - Duration::from_secs(FLOW_TTL_SECONDS + 1);
        insert_flow(&store, "oauth_expired", created_at);

        let error = retrieve_flow(&store, "oauth_expired").expect_err("expired token");

        assert_eq!(error.0, StatusCode::BAD_REQUEST);
        assert!(store.flows.lock().expect("lock store").is_empty());
    }

    #[test]
    fn validate_rejects_unsupported_provider() {
        let error = validate_oauth_provider("anthropic").expect_err("unsupported provider");

        assert_eq!(error.0, StatusCode::BAD_REQUEST);
        assert_eq!(
            error.1 .0.error,
            "OAuth not supported for provider 'anthropic'"
        );
    }

    #[tokio::test]
    async fn exchange_token_posts_pkce_form_and_parses_response() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let addr = listener.local_addr().expect("local addr");
        let flow = PkceFlow::try_new()
            .expect("pkce flow")
            .with_redirect_uri(APP_REDIRECT_URI);
        let url = format!("http://{addr}/oauth/token");

        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept");
            let mut buf = vec![0u8; 8192];
            let size = stream.read(&mut buf).await.expect("read request");
            let request = String::from_utf8_lossy(&buf[..size]).to_string();

            let body = r#"{"access_token":"access-token-value","refresh_token":"refresh-token-value","expires_in":3600,"token_type":"Bearer"}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .await
                .expect("write response");
            request
        });

        let token_response = exchange_token_with_url(&flow, "auth-code", &url)
            .await
            .expect("exchange token");
        let request = server.await.expect("server task");

        assert!(request.starts_with("POST /oauth/token HTTP/1.1"));
        assert!(request.contains("content-type: application/x-www-form-urlencoded"));
        assert!(request.contains("grant_type=authorization_code"));
        assert!(request.contains("code=auth-code"));
        assert!(request.contains(&format!("code_verifier={}", flow.code_verifier())));
        assert!(request.contains("redirect_uri=http%3A%2F%2Flocalhost%3A1455%2Fauth%2Fcallback"));
        assert!(request.contains(&format!("client_id={OPENAI_CLIENT_ID}")));
        assert_eq!(token_response.access_token, "access-token-value");
        assert_eq!(token_response.refresh_token, "refresh-token-value");
        assert_eq!(token_response.expires_in, 3600);
        assert_eq!(token_response.token_type, "Bearer");
    }

    #[tokio::test]
    async fn exchange_token_returns_response_body_on_failure() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let addr = listener.local_addr().expect("local addr");
        let flow = PkceFlow::try_new().expect("pkce flow");
        let url = format!("http://{addr}/oauth/token");

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept");
            let mut buf = vec![0u8; 4096];
            let _ = stream.read(&mut buf).await.expect("read request");

            let body = r#"{"error":"invalid_grant"}"#;
            let response = format!(
                "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .await
                .expect("write response");
        });

        let error = exchange_token_with_url(&flow, "auth-code", &url)
            .await
            .expect_err("exchange should fail");

        assert!(error.contains("token exchange failed (400 Bad Request)"));
        assert!(error.contains(r#"{"error":"invalid_grant"}"#));
    }

    #[tokio::test]
    async fn refresh_access_token_posts_form_and_parses_response() {
        let body = r#"{"access_token":"access-token-value","refresh_token":"refresh-token-value","expires_in":3600,"token_type":"Bearer"}"#;
        let (url, server) = spawn_token_server(body, "HTTP/1.1 200 OK").await;

        let token_response = refresh_access_token_with_url("refresh-token-value", &url)
            .await
            .expect("refresh token");
        let request = server.await.expect("server task");

        assert!(request.starts_with("POST /oauth/token HTTP/1.1"));
        assert!(request.contains("content-type: application/x-www-form-urlencoded"));
        assert!(request.contains("grant_type=refresh_token"));
        assert!(request.contains("refresh_token=refresh-token-value"));
        assert!(request.contains(&format!("client_id={OPENAI_CLIENT_ID}")));
        assert_eq!(token_response.access_token, "access-token-value");
        assert_eq!(token_response.refresh_token, "refresh-token-value");
    }

    #[tokio::test]
    async fn refresh_access_token_returns_response_body_on_failure() {
        let (url, _) =
            spawn_token_server(r#"{"error":"invalid_grant"}"#, "HTTP/1.1 400 Bad Request").await;

        let error = refresh_access_token_with_url("refresh-token-value", &url)
            .await
            .expect_err("refresh should fail");

        assert!(error.contains("token exchange failed (400 Bad Request)"));
        assert!(error.contains(r#"{"error":"invalid_grant"}"#));
    }

    async fn spawn_token_server(
        body: &'static str,
        status_line: &'static str,
    ) -> (String, tokio::task::JoinHandle<String>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let addr = listener.local_addr().expect("local addr");
        let url = format!("http://{addr}/oauth/token");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept");
            let mut buf = vec![0u8; 8192];
            let size = stream.read(&mut buf).await.expect("read request");
            let request = String::from_utf8_lossy(&buf[..size]).to_string();
            let response = format!(
                "{status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .await
                .expect("write response");
            request
        });

        (url, server)
    }

    /// Test load_refresh_token by directly calling the function with a minimal data_dir setup.
    /// We avoid creating a full HttpState by testing the auth store operations directly.
    fn load_refresh_token_from_dir(
        data_dir: &std::path::Path,
        provider: &str,
    ) -> Result<String, (StatusCode, Json<ErrorBody>)> {
        let store = crate::auth_store::AuthStore::open(data_dir).map_err(internal_error)?;
        let auth_manager = store.load_auth_manager().map_err(internal_error)?;

        match auth_manager.get(provider) {
            Some(AuthMethod::OAuth { refresh_token, .. }) => Ok(refresh_token.clone()),
            Some(_) => Err(error_response(
                StatusCode::BAD_REQUEST,
                format!("Provider '{provider}' is not using OAuth authentication"),
            )),
            None => Err(error_response(
                StatusCode::NOT_FOUND,
                format!("No credentials found for provider '{provider}'"),
            )),
        }
    }

    fn seed_store(store: &OAuthFlowStore, count: usize, created_at: Instant) {
        for index in 0..count {
            insert_flow(store, &format!("seed_{index}"), created_at);
        }
    }

    fn insert_flow(store: &OAuthFlowStore, token: &str, created_at: Instant) {
        let flow = PkceFlow::try_new().expect("pkce flow");
        store
            .flows
            .lock()
            .expect("lock store")
            .insert(token.to_string(), StoredFlow { flow, created_at });
    }
}
