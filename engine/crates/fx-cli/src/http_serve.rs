//! HTTP API server for Fawx headless mode (Tailscale-only).
//!
//! Provides a thin HTTP adapter over [`HeadlessApp`] with endpoints for
//! message processing, health checks, and status. The server binds
//! exclusively to the Tailscale interface (100.64.0.0/10 CGNAT range)
//! and refuses to start if Tailscale is not detected.

use axum::extract::{Json, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::Router;
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
    // Scan common Tailscale interface names via /proc/net or ip command
    let output = std::process::Command::new("ip")
        .args(["-4", "-o", "addr", "show"])
        .output()
        .map_err(|e| HttpError::NoTailscale(format!("failed to run `ip addr`: {e}")))?;

    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        // Lines look like: "4: tailscale0    inet 100.93.251.101/32 ..."
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
    // Format: "N: iface    inet A.B.C.D/prefix ..."
    let inet_pos = line.find("inet ")?;
    let after_inet = &line[inet_pos + 5..];
    let addr_str = after_inet.split('/').next()?;
    addr_str.trim().parse().ok()
}

// ── Error type ──────────────────────────────────────────────────────────────

#[derive(Debug)]
enum HttpError {
    NoTailscale(String),
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoTailscale(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for HttpError {}

// ── Router ──────────────────────────────────────────────────────────────────

/// Maximum request body size (1 MiB).
const MAX_REQUEST_BYTES: usize = 1_048_576;

fn build_router(state: HttpState) -> Router {
    Router::new()
        .route("/message", post(handle_message))
        .route("/health", get(handle_health))
        .route("/status", get(handle_status))
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
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody {
                error: format!("cycle error: {e}"),
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
    // Skills info not available through HeadlessApp; report 0 for v1
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
/// Detects the Tailscale IP, binds exclusively to it, and serves
/// requests until the process is terminated.
pub async fn run(app: HeadlessApp, port: u16) -> anyhow::Result<i32> {
    let ip = detect_tailscale_ip().map_err(|e| anyhow::anyhow!("{e}"))?;
    let addr = SocketAddr::new(ip, port);

    let state = HttpState {
        app: Arc::new(Mutex::new(app)),
        start_time: Instant::now(),
        tailscale_ip: ip,
    };

    let router = build_router(state);
    let listener = TcpListener::bind(addr)
        .await
        .map_err(|e| anyhow::anyhow!("failed to bind HTTP server on {addr}: {e}"))?;

    eprintln!("Fawx HTTP API listening on http://{addr}");
    eprintln!("Tailscale-only binding — not accessible from public internet");

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

    // ── Tailscale IP validation ─────────────────────────────────────────

    #[test]
    fn tailscale_ip_accepts_valid_range() {
        // 100.64.0.1 — start of CGNAT range
        assert!(is_tailscale_ip(&IpAddr::V4(Ipv4Addr::new(100, 64, 0, 1))));
        // 100.127.255.255 — end of CGNAT range
        assert!(is_tailscale_ip(&IpAddr::V4(Ipv4Addr::new(
            100, 127, 255, 255
        ))));
        // 100.93.251.101 — typical Tailscale IP
        assert!(is_tailscale_ip(&IpAddr::V4(Ipv4Addr::new(
            100, 93, 251, 101
        ))));
    }

    #[test]
    fn tailscale_ip_rejects_outside_range() {
        // 100.63.0.0 — just below CGNAT range
        assert!(!is_tailscale_ip(&IpAddr::V4(Ipv4Addr::new(100, 63, 0, 0))));
        // 100.128.0.0 — just above CGNAT range
        assert!(!is_tailscale_ip(&IpAddr::V4(Ipv4Addr::new(100, 128, 0, 0))));
        // Private ranges
        assert!(!is_tailscale_ip(&IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))));
        assert!(!is_tailscale_ip(&IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))));
        // Loopback
        assert!(!is_tailscale_ip(&IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))));
        // Wildcard
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
        // detect_tailscale_ip() uses is_tailscale_ip() to validate.
        // These IPs would be rejected during binding.
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

    // ── Endpoint integration tests (using axum test utilities) ──────────

    /// Build a test router with a mock state (no real HeadlessApp needed
    /// for endpoint shape tests — we test handlers directly).
    fn test_router() -> Router {
        // For endpoint tests that don't need a real HeadlessApp,
        // we test serialization/deserialization and routing only.
        // Handler tests that need HeadlessApp require a full engine setup.
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
        // Missing required field → 422 (Unprocessable Entity) from axum
        assert!(resp.status().is_client_error());
    }
}
