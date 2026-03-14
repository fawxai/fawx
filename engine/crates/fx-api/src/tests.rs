use crate::config_redaction;
use crate::engine::{AppEngine, ConfigManagerHandle, CycleResult as ApiCycleResult};
use crate::error::HttpError;
use crate::handlers::health::sanitize_config;
use crate::listener::{
    bind_listener, listen_targets, optional_bound_listener, optional_tailscale_ip, run_listeners,
    wait_for_server_pair, BoundListener, BoundListeners, ListenTarget,
};
use crate::middleware::verify_token;
use crate::router::build_router;
use crate::sse::{send_sse_frame, serialize_stream_event};
use crate::state::{build_channel_runtime, ChannelRuntime, HttpState};
use crate::token::{validate_bearer_token, BearerTokenStore};
use crate::types::{
    AuthProviderDto, ContextInfoDto, ContextInfoSnapshotLike, ErrorBody, HealthResponse,
    MessageRequest, MessageResponse, ModelInfoDto, SkillSummaryDto, StatusResponse,
    ThinkingLevelDto,
};
use async_trait::async_trait;
use axum::body::Body;
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::middleware;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use fx_bus::SessionBus;
use fx_channel_telegram::{IncomingMessage, TelegramChannel};
use fx_channel_webhook::WebhookChannel;
use fx_cli::headless::{
    process_input_with_commands, process_input_with_commands_streaming, HeadlessApp,
    HeadlessAppDeps,
};
use fx_config::HttpConfig;
use fx_core::channel::{Channel, ResponseContext};
use fx_core::runtime_info::{ConfigSummary, RuntimeInfo};
use fx_core::types::InputSource;
use fx_fleet::FleetManager;
use fx_kernel::{ChannelRegistry, HttpChannel, ResponseRouter, StreamCallback, StreamEvent};
use fx_llm::{
    CompletionResponse, CompletionStream, ContentBlock, ImageAttachment, Message, StreamChunk,
};
use http_body_util::BodyExt;
use hyper::Request;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, Mutex};
use tower::ServiceExt;

const TEST_TOKEN: &str = "test-secret-token-abc123";

impl ContextInfoSnapshotLike for fx_cli::headless::ContextInfoSnapshot {
    fn used_tokens(&self) -> usize {
        self.used_tokens
    }

    fn max_tokens(&self) -> usize {
        self.max_tokens
    }

    fn percentage(&self) -> f32 {
        self.percentage
    }

    fn compaction_threshold(&self) -> f32 {
        self.compaction_threshold
    }
}

#[async_trait]
impl AppEngine for HeadlessApp {
    async fn process_message(
        &mut self,
        input: &str,
        images: Vec<ImageAttachment>,
        source: InputSource,
        callback: Option<StreamCallback>,
    ) -> Result<ApiCycleResult, anyhow::Error> {
        let result = match (images.is_empty(), callback) {
            (true, Some(callback)) => {
                process_input_with_commands_streaming(self, input, Some(&source), callback).await?
            }
            (true, None) => process_input_with_commands(self, input, Some(&source)).await?,
            (false, _) => {
                self.process_message_with_images(input, &images, &source)
                    .await?
            }
        };

        Ok(ApiCycleResult {
            response: result.response,
            model: result.model,
            iterations: result.iterations,
        })
    }

    async fn process_message_with_context(
        &mut self,
        input: &str,
        images: Vec<ImageAttachment>,
        context: Vec<Message>,
        source: InputSource,
        callback: Option<StreamCallback>,
    ) -> Result<(ApiCycleResult, Vec<Message>), anyhow::Error> {
        let (result, updated_history) = HeadlessApp::process_message_with_context(
            self, input, images, context, &source, callback,
        )
        .await?;

        Ok((
            ApiCycleResult {
                response: result.response,
                model: result.model,
                iterations: result.iterations,
            },
            updated_history,
        ))
    }

    fn active_model(&self) -> &str {
        HeadlessApp::active_model(self)
    }

    fn available_models(&self) -> Vec<ModelInfoDto> {
        HeadlessApp::available_models(self)
            .into_iter()
            .map(ModelInfoDto::from)
            .collect()
    }

    fn set_active_model(&mut self, selector: &str) -> Result<String, anyhow::Error> {
        HeadlessApp::set_active_model(self, selector)
    }

    fn thinking_level(&self) -> ThinkingLevelDto {
        HeadlessApp::thinking_budget(self).into()
    }

    fn context_info(&self) -> ContextInfoDto {
        ContextInfoDto::from_snapshot(&HeadlessApp::context_info_snapshot(self))
    }

    fn set_thinking_level(&mut self, level: &str) -> Result<ThinkingLevelDto, anyhow::Error> {
        HeadlessApp::handle_thinking(self, Some(level))?;
        Ok(self.thinking_level())
    }

    fn skill_summaries(&self) -> Vec<SkillSummaryDto> {
        HeadlessApp::skill_summaries(self)
            .into_iter()
            .map(SkillSummaryDto::from)
            .collect()
    }

    fn auth_provider_statuses(&self) -> Vec<AuthProviderDto> {
        HeadlessApp::auth_provider_statuses(self)
            .into_iter()
            .map(|s| AuthProviderDto {
                provider: s.provider,
                auth_methods: s.auth_methods.into_iter().collect(),
                model_count: s.model_count,
                status: "registered".to_string(),
            })
            .collect()
    }

    fn config_manager(&self) -> Option<ConfigManagerHandle> {
        HeadlessApp::config_manager(self).cloned()
    }

    fn session_bus(&self) -> Option<&SessionBus> {
        HeadlessApp::session_bus(self)
    }
}

#[test]
fn context_info_dto_from_snapshot_uses_shared_helper() {
    let snapshot = fx_cli::headless::ContextInfoSnapshot {
        used_tokens: 120,
        max_tokens: 1000,
        percentage: 12.0,
        compaction_threshold: 0.8,
    };

    let dto = ContextInfoDto::from_snapshot(&snapshot);

    assert_eq!(dto.used_tokens, 120);
    assert_eq!(dto.max_tokens, 1000);
    assert_eq!(dto.percentage, 12.0);
    assert_eq!(dto.compaction_threshold, 0.8);
}

fn test_runtime_info() -> Arc<std::sync::RwLock<RuntimeInfo>> {
    Arc::new(std::sync::RwLock::new(RuntimeInfo {
        active_model: String::new(),
        provider: String::new(),
        skills: Vec::new(),
        config_summary: ConfigSummary {
            max_iterations: 3,
            max_history: 20,
            memory_enabled: false,
        },
        version: "test".to_string(),
    }))
}

fn mock_completion_response() -> CompletionResponse {
    CompletionResponse {
        content: vec![ContentBlock::Text {
            text: "Mock response".to_string(),
        }],
        tool_calls: Vec::new(),
        usage: None,
        stop_reason: Some("end_turn".to_string()),
    }
}

fn mock_completion_stream() -> CompletionStream {
    let chunk = StreamChunk {
        delta_content: Some("Mock response".to_string()),
        stop_reason: Some("end_turn".to_string()),
        ..Default::default()
    };
    Box::pin(futures::stream::once(async move { Ok(chunk) }))
}

#[derive(Clone)]
struct TestAuthState {
    bearer_token: String,
}

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
        Some(header) => header,
        None => return unauthorized(),
    };
    let header_str = match header.to_str() {
        Ok(value) => value,
        Err(_) => return unauthorized(),
    };
    let token = match header_str.strip_prefix("Bearer ") {
        Some(token) => token,
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

#[derive(Default)]
struct MockBearerTokenStore {
    token: Option<String>,
}

impl BearerTokenStore for MockBearerTokenStore {
    fn get_provider_token(&self, provider: &str) -> Result<Option<String>, String> {
        assert_eq!(provider, "http_bearer");
        Ok(self.token.clone())
    }
}

#[test]
fn tailscale_ip_accepts_valid_range() {
    assert!(crate::tailscale::is_tailscale_ip(&IpAddr::V4(
        Ipv4Addr::new(100, 64, 0, 1)
    )));
    assert!(crate::tailscale::is_tailscale_ip(&IpAddr::V4(
        Ipv4Addr::new(100, 127, 255, 255)
    )));
    assert!(crate::tailscale::is_tailscale_ip(&IpAddr::V4(
        Ipv4Addr::new(100, 93, 251, 101)
    )));
}

#[test]
fn tailscale_ip_rejects_outside_range() {
    assert!(!crate::tailscale::is_tailscale_ip(&IpAddr::V4(
        Ipv4Addr::new(100, 63, 0, 0)
    )));
    assert!(!crate::tailscale::is_tailscale_ip(&IpAddr::V4(
        Ipv4Addr::new(100, 128, 0, 0)
    )));
    assert!(!crate::tailscale::is_tailscale_ip(&IpAddr::V4(
        Ipv4Addr::new(192, 168, 1, 1)
    )));
}

#[test]
fn tailscale_ip_rejects_ipv6() {
    let ipv6: IpAddr = "::1".parse().expect("valid ipv6");
    assert!(!crate::tailscale::is_tailscale_ip(&ipv6));
}

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
        tailscale: optional_bound_listener(tailscale_target, Err(anyhow::anyhow!("bind failed"))),
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

#[test]
fn extract_ip_parses_ip_addr_output() {
    let line = "4: tailscale0    inet 100.93.251.101/32 scope global tailscale0";
    let ip = crate::tailscale::extract_ip_from_line(line);
    assert_eq!(ip, Some(IpAddr::V4(Ipv4Addr::new(100, 93, 251, 101))));
}

#[test]
fn extract_ip_returns_none_for_no_inet() {
    let line = "4: tailscale0    link/none";
    assert!(crate::tailscale::extract_ip_from_line(line).is_none());
}

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
    let response = MessageResponse {
        response: "hi there".to_string(),
        model: "gpt-4".to_string(),
        iterations: 2,
    };
    let json: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&response).expect("serialize")).expect("parse");
    assert_eq!(json["response"], "hi there");
    assert_eq!(json["model"], "gpt-4");
    assert_eq!(json["iterations"], 2);
}

#[test]
fn health_response_has_expected_fields() {
    let response = HealthResponse {
        status: "ok",
        model: "claude-3".to_string(),
        uptime_seconds: 60,
        skills_loaded: 3,
    };
    let json: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&response).expect("serialize")).expect("parse");
    assert_eq!(json["status"], "ok");
    assert_eq!(json["model"], "claude-3");
    assert_eq!(json["uptime_seconds"], 60);
    assert_eq!(json["skills_loaded"], 3);
}

#[test]
fn status_response_has_expected_fields() {
    let response = StatusResponse {
        status: "ok",
        model: "claude-3".to_string(),
        skills: vec!["read_file".to_string()],
        memory_entries: 42,
        tailscale_ip: Some("100.93.251.101".to_string()),
        config: None,
    };
    let json: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&response).expect("serialize")).expect("parse");
    assert_eq!(json["status"], "ok");
    assert_eq!(json["tailscale_ip"], "100.93.251.101");
    assert_eq!(json["memory_entries"], 42);
    assert!(json["skills"].is_array());
}

#[test]
fn status_response_omits_tailscale_ip_when_unavailable() {
    let response = StatusResponse {
        status: "ok",
        model: "claude-3".to_string(),
        skills: vec!["read_file".to_string()],
        memory_entries: 42,
        tailscale_ip: None,
        config: None,
    };
    let json: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&response).expect("serialize")).expect("parse");
    assert_eq!(json["status"], "ok");
    assert!(json.get("tailscale_ip").is_none());
}

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
    let req = Request::builder()
        .method("GET")
        .uri("/health")
        .body(Body::empty())
        .expect("request");

    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
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
}

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
    assert!(verify_token("", ""));
}

#[test]
fn serialize_stream_event_uses_distinct_tool_call_event_names() {
    let start = serialize_stream_event(StreamEvent::ToolCallStart {
        id: "call-1".to_string(),
        name: "read_file".to_string(),
    })
    .expect("start frame");
    let complete = serialize_stream_event(StreamEvent::ToolCallComplete {
        id: "call-1".to_string(),
        name: "read_file".to_string(),
        arguments: r#"{"path":"README.md"}"#.to_string(),
    })
    .expect("complete frame");

    assert!(start.contains("event: tool_call_start"));
    assert!(complete.contains("event: tool_call_complete"));
}

#[test]
fn serialize_stream_event_serializes_typed_phase() {
    let frame = serialize_stream_event(StreamEvent::PhaseChange {
        phase: fx_kernel::Phase::Perceive,
    })
    .expect("phase frame");

    assert_eq!(frame, "event: phase\ndata: {\"phase\":\"perceive\"}\n\n");
}

#[test]
fn serialize_stream_event_serializes_error_event_payload() {
    let frame = serialize_stream_event(StreamEvent::Error {
        category: fx_kernel::ErrorCategory::Memory,
        message: "memory flush failed".to_string(),
        recoverable: true,
    })
    .expect("error frame");

    assert!(frame.contains("event: engine_error"));
    assert!(frame.contains("\"category\":\"memory\""));
    assert!(frame.contains("\"message\":\"memory flush failed\""));
    assert!(frame.contains("\"recoverable\":true"));
}

#[test]
fn send_sse_frame_stops_when_receiver_is_closed() {
    let (sender, receiver) = mpsc::channel(1);
    let disconnected = Arc::new(AtomicBool::new(false));
    drop(receiver);

    assert!(!send_sse_frame(&sender, &disconnected, "frame".to_string()));
    assert!(disconnected.load(Ordering::Relaxed));
}

#[test]
fn send_sse_frame_stops_when_channel_is_full() {
    let (sender, mut receiver) = mpsc::channel(1);
    let disconnected = Arc::new(AtomicBool::new(false));

    assert!(send_sse_frame(&sender, &disconnected, "first".to_string()));
    assert!(!send_sse_frame(
        &sender,
        &disconnected,
        "second".to_string()
    ));
    assert!(disconnected.load(Ordering::Relaxed));
    assert_eq!(receiver.try_recv().expect("queued frame"), "first");
}

#[test]
fn validate_bearer_token_trims_whitespace() {
    let config = HttpConfig {
        bearer_token: Some("  my-secret  ".to_string()),
    };
    let result = validate_bearer_token(&config, None).expect("should accept");
    assert_eq!(result, "my-secret");
}

#[tokio::test]
async fn auth_empty_bearer_value_returns_401() {
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
    let header_bytes = format!("Bearer {TEST_TOKEN}\x00extradata");
    assert!(axum::http::HeaderValue::from_bytes(header_bytes.as_bytes()).is_err());
}

#[tokio::test]
async fn auth_non_ascii_header_returns_401() {
    let app = authed_test_router();
    let header_val = axum::http::HeaderValue::from_bytes(b"Bearer t\xc3\xa9st").expect("bytes");
    let req = Request::builder()
        .method("GET")
        .uri("/status")
        .header("authorization", header_val)
        .body(Body::empty())
        .expect("request");

    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[test]
fn validate_bearer_token_prefers_credential_store() {
    let store = MockBearerTokenStore {
        token: Some("store-token".to_string()),
    };
    let config = HttpConfig {
        bearer_token: Some("config-token".to_string()),
    };
    let result = validate_bearer_token(&config, Some(&store)).expect("should succeed");
    assert_eq!(result, "store-token");
}

#[test]
fn validate_bearer_token_falls_back_to_config() {
    let store = MockBearerTokenStore::default();
    let config = HttpConfig {
        bearer_token: Some("config-token".to_string()),
    };
    let result = validate_bearer_token(&config, Some(&store)).expect("should succeed");
    assert_eq!(result, "config-token");
}

#[test]
fn validate_bearer_token_fails_when_neither_source_has_token() {
    let store = MockBearerTokenStore::default();
    let config = HttpConfig { bearer_token: None };
    assert!(validate_bearer_token(&config, Some(&store)).is_err());
}

#[test]
fn validate_bearer_token_store_roundtrip() {
    let store = MockBearerTokenStore {
        token: Some("my-secret-bearer-token-abc123".to_string()),
    };
    let retrieved = store
        .get_provider_token("http_bearer")
        .expect("get")
        .expect("should have value");
    assert_eq!(retrieved, "my-secret-bearer-token-abc123");
}

#[test]
fn validate_bearer_token_store_ignores_empty() {
    let store = MockBearerTokenStore {
        token: Some("  ".to_string()),
    };
    let config = HttpConfig {
        bearer_token: Some("config-fallback".to_string()),
    };
    let result = validate_bearer_token(&config, Some(&store)).expect("should succeed");
    assert_eq!(result, "config-fallback");
}

mod routing_and_status {
    use super::*;
    use async_trait::async_trait;
    use fx_bus::{BusStore, Payload, SessionBus};
    use fx_config::manager::ConfigManager;
    use fx_kernel::act::{ToolExecutor, ToolExecutorError, ToolResult};
    use fx_kernel::budget::{BudgetConfig, BudgetTracker};
    use fx_kernel::cancellation::CancellationToken;
    use fx_kernel::context_manager::ContextCompactor;
    use fx_kernel::loop_engine::LoopEngine;
    use fx_llm::{
        CompletionProvider, CompletionRequest, CompletionResponse, CompletionStream, ModelRouter,
        ProviderCapabilities, ProviderError as LlmError,
    };
    use fx_session::{
        MessageRole as SessionMessageRole, SessionConfig, SessionError, SessionKey, SessionKind,
        SessionRegistry, SessionStatus, SessionStore,
    };
    use fx_subagent::{
        test_support::DisabledSubagentFactory, SubagentLimits, SubagentManager, SubagentManagerDeps,
    };
    use std::sync::{Arc, Mutex as StdMutex};
    use tempfile::TempDir;

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
            Ok(mock_completion_response())
        }

        async fn complete_stream(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionStream, LlmError> {
            Ok(mock_completion_stream())
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

    struct StaticProvider {
        name: &'static str,
        models: Vec<&'static str>,
    }

    #[async_trait]
    impl CompletionProvider for StaticProvider {
        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionResponse, LlmError> {
            Ok(mock_completion_response())
        }

        async fn complete_stream(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionStream, LlmError> {
            Ok(mock_completion_stream())
        }

        fn name(&self) -> &str {
            self.name
        }

        fn supported_models(&self) -> Vec<String> {
            self.models.iter().map(ToString::to_string).collect()
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

    fn settings_router() -> ModelRouter {
        let mut router = ModelRouter::new();
        router.register_provider_with_auth(
            Box::new(StaticProvider {
                name: "anthropic",
                models: vec!["claude-sonnet-4-20250514"],
            }),
            "api_key",
        );
        router.register_provider_with_auth(
            Box::new(StaticProvider {
                name: "openai",
                models: vec!["gpt-4o"],
            }),
            "api_key",
        );
        router
            .set_active("claude-sonnet-4-20250514")
            .expect("set active");
        router
    }

    fn make_test_app(config_manager: Option<Arc<StdMutex<ConfigManager>>>) -> HeadlessApp {
        make_test_app_with_config(fx_config::FawxConfig::default(), config_manager)
    }

    fn make_test_app_with_config(
        config: fx_config::FawxConfig,
        config_manager: Option<Arc<StdMutex<ConfigManager>>>,
    ) -> HeadlessApp {
        build_test_app(mock_router(), config, config_manager, test_runtime_info())
    }

    fn build_test_app(
        router: ModelRouter,
        config: fx_config::FawxConfig,
        config_manager: Option<Arc<StdMutex<ConfigManager>>>,
        runtime_info: Arc<std::sync::RwLock<RuntimeInfo>>,
    ) -> HeadlessApp {
        build_test_app_with_bus(router, config, config_manager, runtime_info, None)
    }

    fn build_test_app_with_bus(
        router: ModelRouter,
        config: fx_config::FawxConfig,
        config_manager: Option<Arc<StdMutex<ConfigManager>>>,
        runtime_info: Arc<std::sync::RwLock<RuntimeInfo>>,
        session_bus: Option<SessionBus>,
    ) -> HeadlessApp {
        let subagent_manager = Arc::new(SubagentManager::new(SubagentManagerDeps {
            factory: Arc::new(DisabledSubagentFactory::new("disabled")),
            limits: SubagentLimits::default(),
        }));

        HeadlessApp::new(HeadlessAppDeps {
            loop_engine: test_engine(),
            router: Arc::new(router),
            runtime_info,
            config,
            memory: None,
            embedding_index_persistence: None,
            system_prompt_path: None,
            config_manager,
            system_prompt_text: None,
            subagent_manager,
            canary_monitor: None,
            session_bus,
            session_key: None,
        })
        .expect("test app")
    }

    fn runtime_info_with_skills(
        skills: &[(&str, Option<&str>, &[&str])],
    ) -> Arc<std::sync::RwLock<RuntimeInfo>> {
        let info = test_runtime_info();
        let mut guard = info.write().expect("runtime info lock");
        guard.skills = skills
            .iter()
            .map(
                |(name, description, tools)| fx_core::runtime_info::SkillInfo {
                    name: (*name).to_string(),
                    description: (*description).map(ToString::to_string),
                    tool_names: tools.iter().map(ToString::to_string).collect(),
                },
            )
            .collect();
        drop(guard);
        info
    }

    fn temp_config_manager(
        config_toml: &str,
    ) -> (TempDir, fx_config::FawxConfig, Arc<StdMutex<ConfigManager>>) {
        let temp = TempDir::new().expect("tempdir");
        std::fs::write(temp.path().join("config.toml"), config_toml).expect("write config");
        let manager = Arc::new(StdMutex::new(
            ConfigManager::new(temp.path()).expect("config manager"),
        ));
        let mut config = fx_config::FawxConfig::load(temp.path()).expect("load config");
        config.general.data_dir = Some(temp.path().to_path_buf());
        (temp, config, manager)
    }

    fn test_state(
        config_manager: Option<Arc<StdMutex<ConfigManager>>>,
        webhooks: Vec<Arc<WebhookChannel>>,
    ) -> HttpState {
        test_state_with_config(fx_config::FawxConfig::default(), config_manager, webhooks)
    }

    fn test_state_with_app(app: HeadlessApp, webhooks: Vec<Arc<WebhookChannel>>) -> HttpState {
        let data_dir = app
            .config()
            .general
            .data_dir
            .clone()
            .unwrap_or_else(std::env::temp_dir);
        HttpState {
            app: Arc::new(Mutex::new(app)),
            session_registry: None,
            start_time: Instant::now(),
            tailscale_ip: None,
            bearer_token: TEST_TOKEN.to_string(),
            channels: build_channel_runtime(None, webhooks),
            data_dir,
        }
    }

    fn test_state_with_config(
        config: fx_config::FawxConfig,
        config_manager: Option<Arc<StdMutex<ConfigManager>>>,
        webhooks: Vec<Arc<WebhookChannel>>,
    ) -> HttpState {
        let data_dir = config
            .general
            .data_dir
            .clone()
            .unwrap_or_else(std::env::temp_dir);
        HttpState {
            app: Arc::new(Mutex::new(make_test_app_with_config(
                config,
                config_manager,
            ))),
            session_registry: None,
            start_time: Instant::now(),
            tailscale_ip: None,
            bearer_token: TEST_TOKEN.to_string(),
            channels: build_channel_runtime(None, webhooks),
            data_dir,
        }
    }

    fn authed_request(method: &str, uri: &str) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header("authorization", format!("Bearer {TEST_TOKEN}"))
            .body(Body::empty())
            .expect("request")
    }

    fn authed_json_request(method: &str, uri: &str, body: &str) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header("authorization", format!("Bearer {TEST_TOKEN}"))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .expect("request")
    }

    async fn response_json(response: axum::response::Response) -> serde_json::Value {
        let body = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        serde_json::from_slice(&body).expect("json")
    }

    fn make_session_registry() -> SessionRegistry {
        let storage = fx_storage::Storage::open_in_memory().expect("in-memory storage");
        SessionRegistry::new(SessionStore::new(storage)).expect("session registry")
    }

    fn make_session_bus() -> (SessionBus, BusStore) {
        let store =
            BusStore::new(fx_storage::Storage::open_in_memory().expect("in-memory storage"));
        (SessionBus::new(store.clone()), store)
    }

    fn test_state_with_sessions(registry: SessionRegistry) -> HttpState {
        let mut state = test_state(None, Vec::new());
        state.session_registry = Some(registry);
        state
    }

    fn seed_session(registry: &SessionRegistry, key: &str) -> SessionKey {
        let key = SessionKey::new(key).expect("session key");
        registry
            .create(
                key.clone(),
                SessionKind::Main,
                SessionConfig {
                    label: Some(format!("label-{key}")),
                    model: "mock-model".to_string(),
                },
            )
            .expect("create session");
        registry
            .set_status(&key, SessionStatus::Idle)
            .expect("set idle");
        key
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
            assert!(
                config_redaction::is_secret_key(key),
                "expected `{key}` to be redacted"
            );
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
        let app = build_router(test_state(Some(manager), Vec::new()), None);
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
    async fn build_router_mounts_fleet_routes_when_manager_present() {
        let temp = TempDir::new().expect("tempdir");
        let mut manager = FleetManager::init(temp.path()).expect("fleet init");
        let token = manager
            .add_node("macmini", "100.75.191.19", 8400)
            .expect("node should add");
        let app = build_router(
            test_state(None, Vec::new()),
            Some(Arc::new(Mutex::new(manager))),
        );
        let request = Request::builder()
            .method("POST")
            .uri("/fleet/register")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&fx_fleet::FleetRegistrationRequest {
                    node_name: "macmini".to_string(),
                    bearer_token: token.secret,
                    capabilities: vec!["agentic_loop".to_string()],
                    rust_version: None,
                    os: Some("macos".to_string()),
                    cpus: Some(8),
                    ram_gb: None,
                })
                .expect("request should serialize"),
            ))
            .expect("request should build");

        let response = app.oneshot(request).await.expect("route should respond");
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn message_endpoint_intercepts_server_side_status_command() {
        let app = build_router(test_state(None, Vec::new()), None);
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
        let app = build_router(test_state(None, Vec::new()), None);
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
        let app = build_router(test_state(None, Vec::new()), None);
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
        let app = build_router(test_state(None, Vec::new()), None);
        let req = Request::builder()
            .method("POST")
            .uri("/message")
            .header("authorization", format!("Bearer {TEST_TOKEN}"))
            .header("accept", "application/json")
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
    async fn message_endpoint_streams_sse_when_requested() {
        let app = build_router(test_state(None, Vec::new()), None);
        let req = Request::builder()
            .method("POST")
            .uri("/message")
            .header("authorization", format!("Bearer {TEST_TOKEN}"))
            .header("accept", "text/event-stream")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"message":"hello there"}"#))
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get(header::CONTENT_TYPE)
                .expect("content-type"),
            "text/event-stream"
        );
        let body = resp.into_body().collect().await.expect("body").to_bytes();
        let text = String::from_utf8(body.to_vec()).expect("utf8 body");
        assert!(text.contains("event: phase\ndata: {\"phase\":\"perceive\"}"));
        assert!(text.contains("event: text_delta\ndata: {\"text\":\"Mock response\"}"));
        assert!(text.contains("event: done\ndata: {\"response\":\"Mock response\"}"));
    }

    #[tokio::test]
    async fn create_session_returns_201() {
        let registry = make_session_registry();
        let app = build_router(test_state_with_sessions(registry), None);
        let req = Request::builder()
            .method("POST")
            .uri("/v1/sessions")
            .header("authorization", format!("Bearer {TEST_TOKEN}"))
            .header("content-type", "application/json")
            .body(Body::from(r#"{"label":"Primary"}"#))
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = resp.into_body().collect().await.expect("body").to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert!(json["key"]
            .as_str()
            .expect("session key")
            .starts_with("sess-"));
        assert_eq!(json["kind"], "main");
        assert_eq!(json["status"], "idle");
        assert_eq!(json["label"], "Primary");
        assert_eq!(json["model"], "mock-model");
        assert_eq!(json["message_count"], 0);
    }

    #[tokio::test]
    async fn list_sessions_returns_array() {
        let registry = make_session_registry();
        seed_session(&registry, "sess-one");
        seed_session(&registry, "sess-two");
        let app = build_router(test_state_with_sessions(registry), None);
        let req = Request::builder()
            .method("GET")
            .uri("/v1/sessions")
            .header("authorization", format!("Bearer {TEST_TOKEN}"))
            .body(Body::empty())
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.expect("body").to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(json["total"], 2);
        assert_eq!(
            json["sessions"].as_array().expect("sessions array").len(),
            2
        );
    }

    #[tokio::test]
    async fn get_session_returns_info() {
        let registry = make_session_registry();
        let key = seed_session(&registry, "sess-info");
        let app = build_router(test_state_with_sessions(registry), None);
        let req = Request::builder()
            .method("GET")
            .uri(format!("/v1/sessions/{key}"))
            .header("authorization", format!("Bearer {TEST_TOKEN}"))
            .body(Body::empty())
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.expect("body").to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(json["key"], key.as_str());
        assert_eq!(json["model"], "mock-model");
        assert_eq!(json["label"], "label-sess-info");
    }

    #[tokio::test]
    async fn get_nonexistent_session_returns_404() {
        let registry = make_session_registry();
        let app = build_router(test_state_with_sessions(registry), None);
        let req = Request::builder()
            .method("GET")
            .uri("/v1/sessions/sess-missing")
            .header("authorization", format!("Bearer {TEST_TOKEN}"))
            .body(Body::empty())
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let body = resp.into_body().collect().await.expect("body").to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(json["error"], "session not found: sess-missing");
    }

    #[tokio::test]
    async fn context_endpoint_returns_budget_info() {
        let registry = make_session_registry();
        let key = seed_session(&registry, "sess-context");
        let app = build_router(test_state_with_sessions(registry), None);

        let warmup = app
            .clone()
            .oneshot(authed_json_request(
                "POST",
                "/message",
                r#"{"message":"hello there"}"#,
            ))
            .await
            .expect("warmup response");
        assert_eq!(warmup.status(), StatusCode::OK);

        let response = app
            .oneshot(authed_request(
                "GET",
                &format!("/v1/sessions/{key}/context"),
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert!(json["used_tokens"].as_u64().expect("used tokens") > 0);
        assert!(json["max_tokens"].as_u64().expect("max tokens") > 0);
        assert!(json["percentage"].as_f64().expect("percentage") > 0.0);
        let threshold = json["compaction_threshold"]
            .as_f64()
            .expect("compaction threshold");
        assert!((threshold - 0.8).abs() < 0.000_1);
    }

    #[tokio::test]
    async fn context_endpoint_rejects_unknown_session() {
        let registry = make_session_registry();
        let app = build_router(test_state_with_sessions(registry), None);

        let response = app
            .oneshot(authed_request("GET", "/v1/sessions/sess-missing/context"))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let json = response_json(response).await;
        assert_eq!(json["error"], "session not found: sess-missing");
    }

    #[tokio::test]
    async fn delete_session_removes_it() {
        let registry = make_session_registry();
        let key = seed_session(&registry, "sess-delete");
        let app = build_router(test_state_with_sessions(registry.clone()), None);
        let req = Request::builder()
            .method("DELETE")
            .uri(format!("/v1/sessions/{key}"))
            .header("authorization", format!("Bearer {TEST_TOKEN}"))
            .body(Body::empty())
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(matches!(
            registry.get_info(&key),
            Err(SessionError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn clear_session_empties_history() {
        let registry = make_session_registry();
        let key = seed_session(&registry, "sess-clear");
        registry
            .record_message(&key, SessionMessageRole::User, "hello")
            .expect("record user");
        registry
            .record_message(&key, SessionMessageRole::Assistant, "Mock response")
            .expect("record assistant");
        let app = build_router(test_state_with_sessions(registry), None);

        let clear_req = Request::builder()
            .method("POST")
            .uri(format!("/v1/sessions/{key}/clear"))
            .header("authorization", format!("Bearer {TEST_TOKEN}"))
            .body(Body::empty())
            .expect("clear request");
        let clear_resp = app
            .clone()
            .oneshot(clear_req)
            .await
            .expect("clear response");
        assert_eq!(clear_resp.status(), StatusCode::OK);

        let history_req = Request::builder()
            .method("GET")
            .uri(format!("/v1/sessions/{key}/messages"))
            .header("authorization", format!("Bearer {TEST_TOKEN}"))
            .body(Body::empty())
            .expect("history request");
        let history_resp = app.oneshot(history_req).await.expect("history response");
        assert_eq!(history_resp.status(), StatusCode::OK);
        let body = history_resp
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(json["total"], 0);
        assert!(json["messages"].as_array().expect("messages").is_empty());
    }

    #[tokio::test]
    async fn session_message_records_history() {
        let registry = make_session_registry();
        let key = seed_session(&registry, "sess-history");
        let app = build_router(test_state_with_sessions(registry.clone()), None);
        let req = Request::builder()
            .method("POST")
            .uri(format!("/v1/sessions/{key}/messages"))
            .header("authorization", format!("Bearer {TEST_TOKEN}"))
            .header("content-type", "application/json")
            .body(Body::from(r#"{"message":"hello there"}"#))
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);

        let history = registry.history(&key, 10).expect("history");
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].role, SessionMessageRole::User);
        assert_eq!(history[0].content, "hello there");
        assert_eq!(history[1].role, SessionMessageRole::Assistant);
        assert_eq!(history[1].content, "Mock response");
    }

    #[tokio::test]
    async fn session_message_streams_sse() {
        let registry = make_session_registry();
        let key = seed_session(&registry, "sess-stream");
        let app = build_router(test_state_with_sessions(registry), None);
        let req = Request::builder()
            .method("POST")
            .uri(format!("/v1/sessions/{key}/messages"))
            .header("authorization", format!("Bearer {TEST_TOKEN}"))
            .header("accept", "text/event-stream")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"message":"hello there"}"#))
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get(header::CONTENT_TYPE)
                .expect("content-type"),
            "text/event-stream"
        );
        let body = resp.into_body().collect().await.expect("body").to_bytes();
        let text = String::from_utf8(body.to_vec()).expect("utf8 body");
        assert!(text.contains("event: phase\ndata: {\"phase\":\"perceive\"}"));
        assert!(text.contains("event: text_delta\ndata: {\"text\":\"Mock response\"}"));
        assert!(text.contains("event: done\ndata: {\"response\":\"Mock response\"}"));
    }

    #[tokio::test]
    async fn send_to_session_endpoint_returns_envelope_id() {
        let (bus, _store) = make_session_bus();
        let session = SessionKey::new("sess-online").expect("session key");
        let mut receiver = bus.subscribe(&session);
        let app = build_router(
            test_state_with_app(
                build_test_app_with_bus(
                    mock_router(),
                    fx_config::FawxConfig::default(),
                    None,
                    test_runtime_info(),
                    Some(bus.clone()),
                ),
                Vec::new(),
            ),
            None,
        );

        let response = app
            .oneshot(authed_json_request(
                "POST",
                "/v1/sessions/sess-online/send",
                r#"{"text":"hello session"}"#,
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert!(json["envelope_id"].as_str().expect("envelope id").len() > 10);
        assert_eq!(json["delivered"], true);
        assert_eq!(
            receiver.try_recv().expect("delivered envelope").payload,
            Payload::Text("hello session".to_string())
        );
    }

    #[tokio::test]
    async fn send_to_nonexistent_session_queues_for_later() {
        let (bus, store) = make_session_bus();
        let app = build_router(
            test_state_with_app(
                build_test_app_with_bus(
                    mock_router(),
                    fx_config::FawxConfig::default(),
                    None,
                    test_runtime_info(),
                    Some(bus),
                ),
                Vec::new(),
            ),
            None,
        );

        let response = app
            .oneshot(authed_json_request(
                "POST",
                "/v1/sessions/sess-missing/send",
                r#"{"text":"wake up later"}"#,
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["delivered"], false);
        assert!(json["envelope_id"].as_str().expect("envelope id").len() > 10);
        let session = SessionKey::new("sess-missing").expect("session key");
        assert_eq!(store.count(&session).expect("count"), 1);
    }

    #[tokio::test]
    async fn message_with_images_accepted() {
        let app = build_router(test_state(None, Vec::new()), None);
        let req = Request::builder()
            .method("POST")
            .uri("/message")
            .header("authorization", format!("Bearer {TEST_TOKEN}"))
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"message":"describe this","images":[{"data":"AQIDBA==","media_type":"image/png"}]}"#,
            ))
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.expect("body").to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(json["response"], "Mock response");
    }

    #[tokio::test]
    async fn message_with_session_id_routes_to_session() {
        let registry = make_session_registry();
        let key = seed_session(&registry, "sess-route");
        let app = build_router(test_state_with_sessions(registry.clone()), None);
        let req = Request::builder()
            .method("POST")
            .uri("/message")
            .header("authorization", format!("Bearer {TEST_TOKEN}"))
            .header("content-type", "application/json")
            .body(Body::from(format!(
                r#"{{"message":"hello there","session_id":"{}"}}"#,
                key.as_str()
            )))
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);

        let history = registry.history(&key, 10).expect("history");
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].content, "hello there");
        assert_eq!(history[1].content, "Mock response");
    }

    #[tokio::test]
    async fn sessions_require_auth() {
        let registry = make_session_registry();
        let app = build_router(test_state_with_sessions(registry), None);
        let req = Request::builder()
            .method("GET")
            .uri("/v1/sessions")
            .body(Body::empty())
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn list_models_returns_active_and_catalog() {
        let state = test_state_with_app(
            build_test_app(
                settings_router(),
                fx_config::FawxConfig::default(),
                None,
                test_runtime_info(),
            ),
            Vec::new(),
        );
        let app = build_router(state, None);

        let response = app
            .oneshot(authed_request("GET", "/v1/models"))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["active_model"], "claude-sonnet-4-20250514");
        assert_eq!(json["models"].as_array().expect("models").len(), 2);
        assert_eq!(json["models"][0]["provider"], "anthropic");
        assert_eq!(json["models"][1]["model_id"], "gpt-4o");
    }

    #[tokio::test]
    async fn set_model_switches_and_returns_previous() {
        let (_temp, config, manager) =
            temp_config_manager("[model]\ndefault_model = \"claude-sonnet-4-20250514\"\n");
        let state = test_state_with_app(
            build_test_app(
                settings_router(),
                config,
                Some(manager),
                test_runtime_info(),
            ),
            Vec::new(),
        );
        let app = build_router(state, None);

        let update = app
            .clone()
            .oneshot(authed_json_request(
                "PUT",
                "/v1/model",
                r#"{"model":"gpt-4o"}"#,
            ))
            .await
            .expect("response");
        assert_eq!(update.status(), StatusCode::OK);
        let update_json = response_json(update).await;
        assert_eq!(update_json["previous_model"], "claude-sonnet-4-20250514");
        assert_eq!(update_json["active_model"], "gpt-4o");

        let models = app
            .oneshot(authed_request("GET", "/v1/models"))
            .await
            .expect("response");
        let models_json = response_json(models).await;
        assert_eq!(models_json["active_model"], "gpt-4o");
    }

    #[tokio::test]
    async fn set_model_invalid_returns_400() {
        let state = test_state_with_app(
            build_test_app(
                settings_router(),
                fx_config::FawxConfig::default(),
                None,
                test_runtime_info(),
            ),
            Vec::new(),
        );
        let app = build_router(state, None);

        let response = app
            .oneshot(authed_json_request(
                "PUT",
                "/v1/model",
                r#"{"model":"nonexistent"}"#,
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let json = response_json(response).await;
        assert_eq!(json["error"], "model not found: nonexistent");
    }

    #[tokio::test]
    async fn set_model_empty_selector_returns_400() {
        let state = test_state_with_app(
            build_test_app(
                settings_router(),
                fx_config::FawxConfig::default(),
                None,
                test_runtime_info(),
            ),
            Vec::new(),
        );
        let app = build_router(state, None);

        let response = app
            .oneshot(authed_json_request("PUT", "/v1/model", r#"{"model":""}"#))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn set_model_unresolvable_selector_returns_400() {
        let state = test_state_with_app(
            build_test_app(
                settings_router(),
                fx_config::FawxConfig::default(),
                None,
                test_runtime_info(),
            ),
            Vec::new(),
        );
        let app = build_router(state, None);

        // A partial prefix that doesn't match any model or alias pattern
        let response = app
            .oneshot(authed_json_request(
                "PUT",
                "/v1/model",
                r#"{"model":"claude"}"#,
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let json = response_json(response).await;
        let error = json["error"].as_str().expect("error string");
        assert!(
            error.contains("not found"),
            "expected not found error, got: {error}"
        );
    }

    #[tokio::test]
    async fn get_thinking_returns_current_level() {
        let app = build_router(test_state(None, Vec::new()), None);
        let response = app
            .oneshot(authed_request("GET", "/v1/thinking"))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["level"], "adaptive");
        assert_eq!(json["budget_tokens"], 5000);
    }

    #[tokio::test]
    async fn set_thinking_valid_level_returns_200() {
        let (_temp, config, manager) = temp_config_manager(
            "[model]\ndefault_model = \"claude-sonnet-4-20250514\"\n\n[general]\nthinking = \"adaptive\"\n",
        );
        let state = test_state_with_app(
            build_test_app(
                settings_router(),
                config,
                Some(manager),
                test_runtime_info(),
            ),
            Vec::new(),
        );
        let app = build_router(state, None);

        let update = app
            .clone()
            .oneshot(authed_json_request(
                "PUT",
                "/v1/thinking",
                r#"{"level":"high"}"#,
            ))
            .await
            .expect("response");
        assert_eq!(update.status(), StatusCode::OK);
        let update_json = response_json(update).await;
        assert_eq!(update_json["previous_level"], "adaptive");
        assert_eq!(update_json["level"], "high");
        assert_eq!(update_json["budget_tokens"], 10000);

        let thinking = app
            .oneshot(authed_request("GET", "/v1/thinking"))
            .await
            .expect("response");
        let thinking_json = response_json(thinking).await;
        assert_eq!(thinking_json["level"], "high");
    }

    #[tokio::test]
    async fn set_thinking_invalid_level_returns_400() {
        let app = build_router(test_state(None, Vec::new()), None);
        let response = app
            .oneshot(authed_json_request(
                "PUT",
                "/v1/thinking",
                r#"{"level":"turbo"}"#,
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let json = response_json(response).await;
        assert_eq!(
            json["error"],
            "unknown thinking budget 'turbo'; expected adaptive, high, low, or off"
        );
    }

    #[tokio::test]
    async fn list_skills_returns_summaries() {
        let runtime_info = runtime_info_with_skills(&[
            (
                "brave-search",
                Some("Search the web"),
                &["brave_search"][..],
            ),
            ("journal", None, &["journal_write", "journal_search"][..]),
        ]);
        let state = test_state_with_app(
            build_test_app(
                mock_router(),
                fx_config::FawxConfig::default(),
                None,
                runtime_info,
            ),
            Vec::new(),
        );
        let app = build_router(state, None);

        let response = app
            .oneshot(authed_request("GET", "/v1/skills"))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["total"], 2);
        assert_eq!(json["skills"][0]["name"], "brave-search");
        assert_eq!(json["skills"][0]["description"], "Search the web");
        assert_eq!(json["skills"][1]["description"], "");
        assert_eq!(json["skills"][1]["tools"][1], "journal_search");
    }

    #[tokio::test]
    async fn list_skills_empty_returns_zero() {
        let app = build_router(test_state(None, Vec::new()), None);
        let response = app
            .oneshot(authed_request("GET", "/v1/skills"))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["total"], 0);
        assert!(json["skills"].as_array().expect("skills").is_empty());
    }

    #[tokio::test]
    async fn list_auth_returns_provider_statuses() {
        let state = test_state_with_app(
            build_test_app(
                settings_router(),
                fx_config::FawxConfig::default(),
                None,
                test_runtime_info(),
            ),
            Vec::new(),
        );
        let app = build_router(state, None);

        let response = app
            .oneshot(authed_request("GET", "/v1/auth"))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["providers"].as_array().expect("providers").len(), 2);
        assert_eq!(json["providers"][0]["provider"], "anthropic");
        assert_eq!(json["providers"][0]["model_count"], 1);
        assert_eq!(json["providers"][1]["status"], "registered");
    }

    #[tokio::test]
    async fn sprint2_endpoints_require_auth() {
        let state = test_state_with_app(
            build_test_app(
                settings_router(),
                fx_config::FawxConfig::default(),
                None,
                test_runtime_info(),
            ),
            Vec::new(),
        );
        let app = build_router(state, None);
        let requests = [
            Request::builder()
                .method("GET")
                .uri("/v1/models")
                .body(Body::empty())
                .expect("request"),
            Request::builder()
                .method("PUT")
                .uri("/v1/model")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"model":"gpt-4o"}"#))
                .expect("request"),
            Request::builder()
                .method("GET")
                .uri("/v1/thinking")
                .body(Body::empty())
                .expect("request"),
            Request::builder()
                .method("PUT")
                .uri("/v1/thinking")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"level":"high"}"#))
                .expect("request"),
            Request::builder()
                .method("GET")
                .uri("/v1/skills")
                .body(Body::empty())
                .expect("request"),
            Request::builder()
                .method("GET")
                .uri("/v1/auth")
                .body(Body::empty())
                .expect("request"),
        ];

        for request in requests {
            let response = app.clone().oneshot(request).await.expect("response");
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }
    }

    fn config_reload_success_message(config_path: &Path) -> String {
        format!("Configuration reloaded from {}", config_path.display())
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
        let app = build_router(
            test_state_with_config(config, Some(Arc::clone(&manager)), Vec::new()),
            None,
        );

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
            config_reload_success_message(&temp.path().join("config.toml"))
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
        let app = build_router(test_state_with_config(config, None, Vec::new()), None);
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
        assert_eq!(
            json["response"].as_str().expect("response string"),
            "No patterns found. Collect more signals first."
        );
    }

    #[tokio::test]
    async fn message_endpoint_improve_runs_server_side() {
        let temp = TempDir::new().expect("tempdir");
        let mut config = fx_config::FawxConfig::default();
        config.general.data_dir = Some(temp.path().to_path_buf());
        let app = build_router(test_state_with_config(config, None, Vec::new()), None);
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
        let app = build_router(test_state(None, vec![webhook]), None);
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
        let data_dir = std::env::temp_dir();
        let state = HttpState {
            app: Arc::new(Mutex::new(make_test_app(None))),
            session_registry: None,
            start_time: Instant::now(),
            tailscale_ip: None,
            bearer_token: TEST_TOKEN.to_string(),
            channels: ChannelRuntime {
                router: Arc::new(ResponseRouter::new(Arc::new(router_registry))),
                http: Arc::new(HttpChannel::new()),
                telegram: None,
                webhooks: Arc::new(webhooks),
            },
            data_dir,
        };
        let app = build_router(state, None);
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

mod config_endpoint {
    use super::*;
    use fx_config::manager::ConfigManager;
    use fx_tools::ConfigSetRequest;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex as StdMutex};
    use tempfile::TempDir;

    #[derive(Clone)]
    struct ConfigTestState {
        config_mgr: Arc<StdMutex<ConfigManager>>,
    }

    async fn test_config_get(
        State(state): State<ConfigTestState>,
        query: axum::extract::Query<HashMap<String, String>>,
    ) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorBody>)> {
        let section = query.get("section").map(|s| s.as_str()).unwrap_or("all");
        let guard = state.config_mgr.lock().map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorBody {
                    error: format!("{error}"),
                }),
            )
        })?;
        let value = guard
            .get(section)
            .map_err(|error| (StatusCode::BAD_REQUEST, Json(ErrorBody { error })))?;
        Ok(Json(value))
    }

    async fn test_config_set(
        State(state): State<ConfigTestState>,
        Json(request): Json<ConfigSetRequest>,
    ) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorBody>)> {
        let mut guard = state.config_mgr.lock().map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorBody {
                    error: format!("{error}"),
                }),
            )
        })?;
        guard
            .set(&request.key, &request.value)
            .map_err(|error| (StatusCode::BAD_REQUEST, Json(ErrorBody { error })))?;
        Ok(Json(serde_json::json!({
            "updated": request.key,
            "value": request.value,
        })))
    }

    fn config_test_router(dir: &Path) -> Router {
        let manager = ConfigManager::new(dir).expect("config manager");
        let state = ConfigTestState {
            config_mgr: Arc::new(StdMutex::new(manager)),
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
        .expect("write config");
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
        .expect("write config");
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
        .expect("write config");
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

mod telegram_update {
    use super::*;
    use crate::handlers::message::process_and_route_message;
    use crate::telegram::helpers::{
        encode_downloaded_photo, encode_photos, flush_telegram_outbound, MAX_IMAGE_BYTES,
    };
    use crate::telegram::polling::handle_telegram_update;
    use crate::types::EncodedImage;
    use async_trait::async_trait;
    use fx_channel_telegram::TelegramConfig;
    use fx_kernel::act::{ToolExecutor, ToolExecutorError, ToolResult};
    use fx_kernel::budget::{BudgetConfig, BudgetTracker};
    use fx_kernel::cancellation::CancellationToken;
    use fx_kernel::context_manager::ContextCompactor;
    use fx_kernel::loop_engine::LoopEngine;
    use fx_llm::{
        CompletionProvider, CompletionRequest, CompletionResponse, CompletionStream, ContentBlock,
        ModelRouter, ProviderCapabilities, ProviderError as LlmError, StreamChunk,
    };

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

    #[derive(Clone)]
    struct CapturingProvider {
        captured: Arc<std::sync::Mutex<Vec<CompletionRequest>>>,
    }

    #[async_trait]
    impl CompletionProvider for CapturingProvider {
        async fn complete(
            &self,
            request: CompletionRequest,
        ) -> Result<CompletionResponse, LlmError> {
            self.capture_request(request);
            Ok(mock_completion_response())
        }

        async fn complete_stream(
            &self,
            request: CompletionRequest,
        ) -> Result<CompletionStream, LlmError> {
            self.capture_request(request);
            Ok(mock_completion_stream())
        }

        fn name(&self) -> &str {
            "capturing"
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["capturing-model".to_string()]
        }

        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities {
                supports_temperature: false,
                requires_streaming: false,
            }
        }
    }

    impl CapturingProvider {
        fn capture_request(&self, request: CompletionRequest) {
            self.captured.lock().expect("capture lock").push(request);
        }
    }

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

    #[derive(Debug, Clone)]
    struct CapturedTelegramRequest {
        path: String,
        body: serde_json::Value,
    }

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
        use fx_subagent::{
            test_support::DisabledSubagentFactory, SubagentLimits, SubagentManager,
            SubagentManagerDeps,
        };

        let subagent_manager = Arc::new(SubagentManager::new(SubagentManagerDeps {
            factory: Arc::new(DisabledSubagentFactory::new("disabled")),
            limits: SubagentLimits::default(),
        }));

        HeadlessApp::new(HeadlessAppDeps {
            loop_engine: test_engine(),
            router: Arc::new(router),
            runtime_info: test_runtime_info(),
            config: fx_config::FawxConfig::default(),
            memory: None,
            embedding_index_persistence: None,
            system_prompt_path: None,
            config_manager: None,
            system_prompt_text: None,
            subagent_manager,
            canary_monitor: None,
            session_bus: None,
            session_key: None,
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

    #[tokio::test]
    async fn happy_path_valid_update_processed() {
        let (base_url, _server) = mock_telegram_server().await;
        let telegram = TelegramChannel::new_with_base_url(test_telegram_config(), base_url);
        let app: Arc<Mutex<dyn AppEngine>> = Arc::new(Mutex::new(make_test_app(mock_router())));
        let update = sample_update(12345, "hello bot");
        let registry = Arc::new(ChannelRegistry::new());
        let router = ResponseRouter::new(registry);
        let temp = tempfile::tempdir().expect("tempdir");

        handle_telegram_update(&telegram, &app, &router, &update, temp.path()).await;

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
        let app: Arc<Mutex<dyn AppEngine>> = Arc::new(Mutex::new(make_test_app(mock_router())));
        let update = sample_update(12345, "/status");
        let mut registry = ChannelRegistry::new();
        let telegram_channel: Arc<dyn Channel> = telegram.clone();
        registry.register(telegram_channel);
        let router = ResponseRouter::new(Arc::new(registry));
        let temp = tempfile::tempdir().expect("tempdir");

        handle_telegram_update(telegram.as_ref(), &app, &router, &update, temp.path()).await;
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

    #[tokio::test]
    async fn parse_error_handled_gracefully() {
        let telegram = TelegramChannel::new(test_telegram_config());
        let app: Arc<Mutex<dyn AppEngine>> = Arc::new(Mutex::new(make_test_app(mock_router())));
        let invalid_update = serde_json::json!({
            "message": { "bad_field": true }
        });
        let registry = Arc::new(ChannelRegistry::new());
        let router = ResponseRouter::new(registry);
        let temp = tempfile::tempdir().expect("tempdir");

        handle_telegram_update(&telegram, &app, &router, &invalid_update, temp.path()).await;

        assert!(telegram.drain_outbound().is_empty());
    }

    #[tokio::test]
    async fn process_message_error_sends_error_response() {
        let (base_url, _server) = mock_telegram_server().await;
        let telegram = TelegramChannel::new_with_base_url(test_telegram_config(), base_url);
        let app: Arc<Mutex<dyn AppEngine>> = Arc::new(Mutex::new(make_test_app(failing_router())));
        let update = sample_update(12345, "trigger error");
        let registry = Arc::new(ChannelRegistry::new());
        let router = ResponseRouter::new(registry);
        let temp = tempfile::tempdir().expect("tempdir");

        handle_telegram_update(&telegram, &app, &router, &update, temp.path()).await;

        assert!(telegram.drain_outbound().is_empty());
    }

    #[tokio::test]
    async fn process_message_error_sends_plain_text_telegram_error() {
        let captured = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (base_url, _server) = capturing_telegram_server(Arc::clone(&captured)).await;
        let telegram = TelegramChannel::new_with_base_url(test_telegram_config(), base_url);
        let app: Arc<Mutex<dyn AppEngine>> = Arc::new(Mutex::new(make_test_app(failing_router())));
        let update = sample_update(12345, "trigger error");
        let registry = Arc::new(ChannelRegistry::new());
        let router = ResponseRouter::new(registry);
        let temp = tempfile::tempdir().expect("tempdir");

        handle_telegram_update(&telegram, &app, &router, &update, temp.path()).await;

        let send_message = captured
            .lock()
            .expect("capture lock")
            .iter()
            .find(|request| request.path.ends_with("/sendMessage"))
            .expect("sendMessage request")
            .clone();
        assert!(send_message.body.get("parse_mode").is_none());
    }

    #[tokio::test]
    async fn encode_photos_returns_empty_for_no_photos() {
        let telegram = TelegramChannel::new(test_telegram_config());
        let mut incoming = IncomingMessage {
            chat_id: 12345,
            text: "hello".to_string(),
            message_id: 42,
            from_name: Some("TestUser".to_string()),
            photos: Vec::new(),
        };
        let temp = tempfile::tempdir().expect("tempdir");

        let images = encode_photos(&telegram, &mut incoming, temp.path()).await;

        assert!(images.is_empty());
    }

    #[test]
    fn encode_skips_oversized_photo() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("large.jpg");
        std::fs::write(&path, vec![0u8; MAX_IMAGE_BYTES + 1]).expect("write image");
        let photo = fx_channel_telegram::PhotoAttachment {
            file_id: "file-1".to_string(),
            width: 1,
            height: 1,
            file_path: None,
            mime_type: "image/jpeg".to_string(),
        };

        let encoded = encode_downloaded_photo(&photo, &path);

        assert!(encoded.is_none());
    }

    #[test]
    fn encode_trims_valid_mime_type() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("photo.bin");
        std::fs::write(&path, [1u8, 2, 3, 4]).expect("write image");
        let photo = fx_channel_telegram::PhotoAttachment {
            file_id: "file-2".to_string(),
            width: 1,
            height: 1,
            file_path: None,
            mime_type: "  image/png  ".to_string(),
        };

        let encoded = encode_downloaded_photo(&photo, &path).expect("encoded image");

        assert_eq!(encoded.media_type, "image/png");
        assert_eq!(encoded.base64_data, "AQIDBA==");
    }

    #[test]
    fn encode_defaults_unknown_mime_type() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("photo.bin");
        std::fs::write(&path, [1u8, 2, 3, 4]).expect("write image");
        let photo = fx_channel_telegram::PhotoAttachment {
            file_id: "file-2".to_string(),
            width: 1,
            height: 1,
            file_path: None,
            mime_type: "application/octet-stream".to_string(),
        };

        let encoded = encode_downloaded_photo(&photo, &path).expect("encoded image");

        assert_eq!(encoded.media_type, "image/jpeg");
        assert_eq!(encoded.base64_data, "AQIDBA==");
    }

    fn capturing_router(captured: Arc<std::sync::Mutex<Vec<CompletionRequest>>>) -> ModelRouter {
        let mut router = ModelRouter::new();
        router.register_provider(Box::new(CapturingProvider { captured }));
        router
            .set_active("capturing-model")
            .expect("set active capturing model");
        router
    }

    #[tokio::test]
    async fn process_and_route_message_with_images_passes_through() {
        let captured = Arc::new(std::sync::Mutex::new(Vec::new()));
        let app: Arc<Mutex<dyn AppEngine>> = Arc::new(Mutex::new(make_test_app(capturing_router(
            Arc::clone(&captured),
        ))));
        let (base_url, _server) = mock_telegram_server().await;
        let telegram = Arc::new(TelegramChannel::new_with_base_url(
            test_telegram_config(),
            base_url,
        ));
        let mut registry = ChannelRegistry::new();
        let telegram_channel: Arc<dyn Channel> = telegram;
        registry.register(telegram_channel);
        let router = ResponseRouter::new(Arc::new(registry));
        let images = vec![EncodedImage {
            media_type: "image/jpeg".to_string(),
            base64_data: "abc123".to_string(),
        }];

        let result = process_and_route_message(
            &app,
            &router,
            "what's in this image?",
            images,
            InputSource::Channel("telegram".to_string()),
            ResponseContext {
                routing_key: Some("12345".to_string()),
                reply_to: None,
            },
        )
        .await
        .expect("process with images");

        assert_eq!(result.response, "Mock response");
        let requests = captured.lock().expect("capture lock");
        let last_request = requests.last().expect("captured request");
        let last_message = last_request.messages.last().expect("user message");
        assert_eq!(last_message.role, fx_llm::MessageRole::User);
        assert!(last_message.content.iter().any(|block| {
            block
                == &ContentBlock::Image {
                    media_type: "image/jpeg".to_string(),
                    data: "abc123".to_string(),
                }
        }));
        assert!(last_message.content.iter().any(|block| {
            matches!(block, ContentBlock::Text { text } if text.contains("what's in this image?"))
        }));
    }
}
