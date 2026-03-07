//! HTTP API server for Fawx headless mode (Tailscale-only).
//!
//! Provides a thin HTTP adapter over [`HeadlessApp`] with endpoints for
//! message processing, health checks, and status. The server binds
//! exclusively to the Tailscale interface (100.64.0.0/10 CGNAT range)
//! and refuses to start if Tailscale is not detected.
//!
//! All authenticated endpoints require a `Bearer <token>` header validated
//! via HMAC-based constant-time comparison (`ring::hmac`). The `/health`
//! endpoint is public for monitoring.

use axum::extract::{Json, State};
use axum::http::StatusCode;
use axum::middleware;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Router;
use fx_config::HttpConfig;
use ring::hmac;
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Instant;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use crate::headless::HeadlessApp;

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
    tailscale_ip: String,
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
    tailscale_ip: IpAddr,
    // TODO(#1203): Token is stored as plaintext String — could appear in heap
    // dumps. Future hardening: wrap with `secrecy::SecretString` to ensure
    // zeroization on drop and prevent accidental logging.
    bearer_token: String,
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
/// finds a Tailscale interface.
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
         fawx serve --http requires Tailscale to be running.\n\
         The HTTP server only binds to the Tailscale network for security."
            .to_string(),
    ))
}

fn extract_ip_from_line(line: &str) -> Option<IpAddr> {
    let inet_pos = line.find("inet ")?;
    let after_inet = &line[inet_pos + 5..];
    let addr_str = after_inet.split('/').next()?;
    addr_str.trim().parse().ok()
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
                "HTTP API requires a bearer token for authentication.\n\
                 Add to ~/.fawx/config.toml:\n\n\
                 [http]\n\
                 bearer_token = \"your-secret-token\"\n\n\
                 Generate one with: openssl rand -hex 32"
            ),
        }
    }
}

impl std::error::Error for HttpError {}

// ── Token validation ────────────────────────────────────────────────────────

/// Validate that the HTTP config contains a non-empty bearer token.
///
/// Trims leading/trailing whitespace from the configured value so that
/// `bearer_token = "  mytoken  "` compares against `"mytoken"` rather than
/// silently including the spaces (review finding #4).
fn validate_bearer_token(config: &HttpConfig) -> Result<String, HttpError> {
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

// ── Router ──────────────────────────────────────────────────────────────────

/// Maximum request body size (1 MiB).
const MAX_REQUEST_BYTES: usize = 1_048_576;

fn build_router(state: HttpState) -> Router {
    let authenticated = Router::new()
        .route("/message", post(handle_message))
        .route("/status", get(handle_status))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    let public = Router::new().route("/health", get(handle_health));

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

    let mut app = state.app.lock().await;
    let result = app.process_message(&request.message).await.map_err(|e| {
        // Log full error details to stderr for debugging; never expose
        // internal error text to HTTP clients (review finding #2).
        tracing::error!(error = %e, "message processing failed");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody {
                error: "internal_error".to_string(),
            }),
        )
    })?;

    Ok(Json(MessageResponse {
        response: result.response,
        model: result.model,
        iterations: result.iterations,
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

async fn handle_status(State(state): State<HttpState>) -> Json<StatusResponse> {
    let app = state.app.lock().await;
    let model = app.active_model().to_string();

    Json(StatusResponse {
        status: "ok",
        model,
        skills: Vec::new(),
        memory_entries: 0,
        tailscale_ip: state.tailscale_ip.to_string(),
    })
}

// ── Public API ──────────────────────────────────────────────────────────────

/// Run the HTTP server for headless mode.
///
/// Validates that a bearer token is configured, detects the Tailscale IP,
/// binds exclusively to it, and serves requests until the process is
/// terminated.
pub async fn run(app: HeadlessApp, port: u16, http_config: &HttpConfig) -> anyhow::Result<i32> {
    let bearer_token = validate_bearer_token(http_config).map_err(|e| anyhow::anyhow!("{e}"))?;

    let ip = detect_tailscale_ip().map_err(|e| anyhow::anyhow!("{e}"))?;
    let addr = SocketAddr::new(ip, port);

    let state = HttpState {
        app: Arc::new(Mutex::new(app)),
        start_time: Instant::now(),
        tailscale_ip: ip,
        bearer_token,
    };

    let router = build_router(state);
    let listener = TcpListener::bind(addr)
        .await
        .map_err(|e| anyhow::anyhow!("failed to bind HTTP server on {addr}: {e}"))?;

    eprintln!("Fawx HTTP API listening on http://{addr}");
    eprintln!("Tailscale-only binding — not accessible from public internet");
    eprintln!("Bearer token authentication: enabled");

    axum::serve(listener, router)
        .await
        .map_err(|e| anyhow::anyhow!("HTTP server error: {e}"))?;

    Ok(0)
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
            tailscale_ip: "100.64.0.1".to_string(),
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
    fn binding_rejects_non_tailscale_ips() {
        assert!(!is_tailscale_ip(&IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))));
        assert!(!is_tailscale_ip(&IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0))));
        assert!(!is_tailscale_ip(&IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))));
    }

    #[test]
    fn binding_accepts_tailscale_ip() {
        assert!(is_tailscale_ip(&IpAddr::V4(Ipv4Addr::new(
            100, 93, 251, 101
        ))));
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
            tailscale_ip: "100.93.251.101".to_string(),
        };
        let json: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&resp).expect("serialize")).expect("parse");
        assert_eq!(json["status"], "ok");
        assert_eq!(json["tailscale_ip"], "100.93.251.101");
        assert_eq!(json["memory_entries"], 42);
        assert!(json["skills"].is_array());
    }

    // ── Bearer token validation ─────────────────────────────────────────

    #[test]
    fn validate_bearer_token_accepts_valid_token() {
        let config = HttpConfig {
            bearer_token: Some("my-secret".to_string()),
        };
        assert!(validate_bearer_token(&config).is_ok());
    }

    #[test]
    fn validate_bearer_token_rejects_none() {
        let config = HttpConfig { bearer_token: None };
        assert!(validate_bearer_token(&config).is_err());
    }

    #[test]
    fn validate_bearer_token_rejects_empty() {
        let config = HttpConfig {
            bearer_token: Some(String::new()),
        };
        assert!(validate_bearer_token(&config).is_err());
    }

    #[test]
    fn validate_bearer_token_rejects_whitespace_only() {
        let config = HttpConfig {
            bearer_token: Some("   ".to_string()),
        };
        assert!(validate_bearer_token(&config).is_err());
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
        let result = validate_bearer_token(&config).expect("should accept");
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
}
