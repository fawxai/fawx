use crate::config_redaction;
use crate::devices::DeviceStore;
use crate::engine::{AppEngine, ConfigManagerHandle, CycleResult as ApiCycleResult, ResultKind};
use crate::error::HttpError;
use crate::experiment_registry::ExperimentRegistry;
use crate::handlers::health::sanitize_config;
use crate::listener::{
    bind_listener, detect_tls_config, listen_targets, optional_bound_listener,
    optional_tailscale_ip, run_listeners, startup_target_lines, wait_for_server_pair,
    BoundListener, BoundListeners, ListenTarget, ServerIdentity, ServerProtocol,
};
use crate::middleware::verify_token;
use crate::pairing::PairingState;
use crate::router::build_router;
use crate::server_runtime::ServerRuntime;
use crate::sse::{send_sse_frame, serialize_stream_event};
use crate::state::{
    build_channel_runtime, in_memory_telemetry, ChannelRuntime, HttpState, SharedReadState,
};
use crate::token::{validate_bearer_token, BearerTokenStore};
use crate::types::{
    ApiKeyRequest, AuthProviderDto, ContextInfoDto, ContextInfoSnapshotLike, DocumentPayload,
    ErrorBody, ErrorRecordDto, HealthResponse, MessageRequest, MessageResponse, ModelInfoDto,
    ModelSwitchDto, SetupTokenRequest, SkillSummaryDto, StatusResponse, ThinkingLevelDto,
};
use async_trait::async_trait;
use axum::body::Body;
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::middleware;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine;
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
    CompletionResponse, CompletionStream, ContentBlock, DocumentAttachment, ImageAttachment,
    Message, StreamChunk,
};
use fx_telemetry::{SignalCategory, SignalCollector, TelemetryConsent};
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
        documents: Vec<DocumentAttachment>,
        source: InputSource,
        callback: Option<StreamCallback>,
    ) -> Result<ApiCycleResult, anyhow::Error> {
        let result = match (images.is_empty() && documents.is_empty(), callback) {
            (true, Some(callback)) => {
                process_input_with_commands_streaming(self, input, Some(&source), callback).await?
            }
            (true, None) => process_input_with_commands(self, input, Some(&source)).await?,
            (false, _) => {
                self.process_message_with_attachments(input, &images, &documents, &source)
                    .await?
            }
        };

        Ok(ApiCycleResult {
            response: result.response,
            model: result.model,
            iterations: result.iterations,
            result_kind: result.result_kind.into(),
        })
    }

    async fn process_message_with_context(
        &mut self,
        input: &str,
        images: Vec<ImageAttachment>,
        documents: Vec<DocumentAttachment>,
        context: Vec<Message>,
        source: InputSource,
        callback: Option<StreamCallback>,
    ) -> Result<(ApiCycleResult, Vec<Message>), anyhow::Error> {
        let (result, updated_history) = HeadlessApp::process_message_with_context(
            self, input, images, documents, context, &source, callback,
        )
        .await?;

        Ok((
            ApiCycleResult {
                response: result.response,
                model: result.model,
                iterations: result.iterations,
                result_kind: result.result_kind.into(),
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

    fn set_active_model(&mut self, selector: &str) -> Result<ModelSwitchDto, anyhow::Error> {
        let switched = HeadlessApp::switch_active_model(self, selector)?;
        Ok(ModelSwitchDto {
            previous_model: switched.previous_model,
            active_model: switched.active_model,
            thinking_adjusted: switched.thinking_adjusted.map(|adjusted| {
                crate::types::ThinkingAdjustedDto {
                    from: adjusted.from,
                    to: adjusted.to,
                    reason: adjusted.reason,
                }
            }),
        })
    }

    fn thinking_level(&self) -> ThinkingLevelDto {
        let dto = HeadlessApp::thinking_level_dto(self);
        ThinkingLevelDto {
            level: dto.level,
            budget_tokens: dto.budget_tokens,
            available: dto.available,
        }
    }

    fn context_info(&self) -> ContextInfoDto {
        ContextInfoDto::from_snapshot(&HeadlessApp::context_info_snapshot(self))
    }

    fn context_info_for_messages(&self, messages: &[Message]) -> ContextInfoDto {
        let used_tokens = messages.len() * 100;
        ContextInfoDto {
            used_tokens,
            max_tokens: 4_096,
            percentage: (used_tokens as f32 / 4_096.0) * 100.0,
            compaction_threshold: 0.8,
        }
    }

    fn set_thinking_level(&mut self, level: &str) -> Result<ThinkingLevelDto, anyhow::Error> {
        let dto = HeadlessApp::set_supported_thinking_level(self, level)?;
        Ok(ThinkingLevelDto {
            level: dto.level,
            budget_tokens: dto.budget_tokens,
            available: dto.available,
        })
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

    fn recent_errors(&self, limit: usize) -> Vec<ErrorRecordDto> {
        HeadlessApp::recent_errors(self, limit)
            .into_iter()
            .map(|record| ErrorRecordDto {
                timestamp: record.timestamp,
                category: record.category,
                message: record.message,
                recoverable: record.recoverable,
            })
            .collect()
    }

    fn take_last_session_messages(&mut self) -> Vec<fx_session::SessionMessage> {
        HeadlessApp::take_last_session_messages(self)
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

impl From<fx_cli::headless::ResultKind> for ResultKind {
    fn from(kind: fx_cli::headless::ResultKind) -> Self {
        match kind {
            fx_cli::headless::ResultKind::Complete => Self::Complete,
            fx_cli::headless::ResultKind::Partial => Self::Partial,
            fx_cli::headless::ResultKind::Error => Self::Error,
            fx_cli::headless::ResultKind::Empty => Self::Empty,
        }
    }
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
        https_enabled: false,
    })
}

async fn mock_status() -> Json<StatusResponse> {
    Json(StatusResponse {
        status: "ok",
        model: "test-model".to_string(),
        skills: vec!["skill-a".to_string()],
        memory_entries: 10,
        tailscale_ip: Some("192.0.2.10".to_string()),
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
        result_kind: ResultKind::Complete,
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
        Ipv4Addr::new(100, 100, 100, 2)
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
    let plan = listen_targets(8400, Some(IpAddr::V4(Ipv4Addr::new(100, 100, 100, 2))));
    let tailscale = plan.tailscale.expect("tailscale target");

    assert_eq!(plan.local.addr, SocketAddr::from(([127, 0, 0, 1], 8400)));
    assert_eq!(plan.local.label, "local");
    assert_eq!(
        tailscale.addr,
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(100, 100, 100, 2)), 8400)
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

#[test]
fn detect_tls_config_returns_none_when_files_are_missing() {
    let temp = tempfile::tempdir().expect("tempdir");

    assert!(detect_tls_config(temp.path()).is_none());
}

#[test]
fn detect_tls_config_returns_some_when_both_files_exist() {
    let temp = tempfile::tempdir().expect("tempdir");
    let tls_dir = temp.path().join("tls");
    std::fs::create_dir_all(&tls_dir).expect("create tls dir");
    std::fs::write(tls_dir.join("cert.pem"), "cert").expect("write cert");
    std::fs::write(tls_dir.join("key.pem"), "key").expect("write key");

    let tls_config = detect_tls_config(temp.path()).expect("detect tls");
    assert_eq!(tls_config.cert_path, tls_dir.join("cert.pem"));
    assert_eq!(tls_config.key_path, tls_dir.join("key.pem"));
}

#[test]
fn detect_tls_config_returns_none_when_only_one_file_exists() {
    let temp = tempfile::tempdir().expect("tempdir");
    let tls_dir = temp.path().join("tls");
    std::fs::create_dir_all(&tls_dir).expect("create tls dir");
    std::fs::write(tls_dir.join("cert.pem"), "cert").expect("write cert");

    assert!(detect_tls_config(temp.path()).is_none());
}

#[test]
fn startup_target_lines_use_https_for_tailscale_when_enabled() {
    let lines = startup_target_lines(
        ListenTarget {
            addr: SocketAddr::from(([127, 0, 0, 1], 8400)),
            label: "local",
        },
        Some(ListenTarget {
            addr: SocketAddr::from(([192, 0, 2, 1], 8400)),
            label: "Tailscale",
        }),
        true,
    );

    assert_eq!(lines[0], "Fawx API listening on:");
    assert_eq!(lines[1], "  http://127.0.0.1:8400 (local)");
    assert_eq!(lines[2], "  https://192.0.2.1:8400 (Tailscale)");
}

#[test]
fn startup_target_lines_use_http_for_tailscale_when_tls_disabled() {
    let lines = startup_target_lines(
        ListenTarget {
            addr: SocketAddr::from(([127, 0, 0, 1], 8400)),
            label: "local",
        },
        Some(ListenTarget {
            addr: SocketAddr::from(([192, 0, 2, 1], 8400)),
            label: "Tailscale",
        }),
        false,
    );

    assert_eq!(lines[0], "Fawx HTTP API listening on:");
    assert_eq!(lines[2], "  http://192.0.2.1:8400 (Tailscale)");
}

#[tokio::test]
async fn tailscale_bind_failure_falls_back_to_localhost_server() {
    let local_target = ListenTarget {
        addr: SocketAddr::from(([127, 0, 0, 1], 0)),
        label: "local",
    };
    let local_listener = bind_listener(local_target, ServerProtocol::Http)
        .await
        .expect("bind localhost");
    let local_addr = local_listener.local_addr().expect("local addr");
    let tailscale_target = ListenTarget {
        addr: SocketAddr::from(([192, 0, 2, 1], 8400)),
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
        None,
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
        ServerIdentity {
            label: "local",
            protocol: ServerProtocol::Http,
        },
        local_server,
        ServerIdentity {
            label: "Tailscale",
            protocol: ServerProtocol::Http,
        },
        tailscale_server,
        shutdown_tx,
    )
    .await;

    assert!(result.is_ok());
}

#[test]
fn extract_ip_parses_ip_addr_output() {
    let line = "4: tailscale0    inet 198.51.100.2/32 scope global tailscale0";
    let ip = crate::tailscale::extract_ip_from_line(line);
    assert_eq!(ip, Some(IpAddr::V4(Ipv4Addr::new(100, 100, 100, 2))));
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
fn validate_and_encode_documents_rejects_oversized_payloads() {
    let document = DocumentPayload {
        data: base64::engine::general_purpose::STANDARD.encode(vec![0_u8; (10 * 1024 * 1024) + 1]),
        media_type: "application/pdf".to_string(),
        filename: Some("too-large.pdf".to_string()),
    };

    let error = crate::handlers::message::validate_and_encode_documents(&[document])
        .expect_err("oversized document should be rejected");

    assert_eq!(error.0, StatusCode::BAD_REQUEST);
    assert_eq!(
        error.1 .0.error,
        "document at index 0 exceeds the 10MB limit"
    );
}

#[test]
fn setup_token_request_deserializes_without_label() {
    let json = r#"{"setup_token":"token"}"#;
    let request: SetupTokenRequest = serde_json::from_str(json).expect("valid json");

    assert_eq!(request.setup_token, "token");
}

#[test]
fn api_key_request_deserializes_without_label() {
    let json = r#"{"api_key":"sk-test"}"#;
    let request: ApiKeyRequest = serde_json::from_str(json).expect("valid json");

    assert_eq!(request.api_key, "sk-test");
}

#[test]
fn message_response_serializes_correctly() {
    let response = MessageResponse {
        response: "hi there".to_string(),
        model: "gpt-4".to_string(),
        iterations: 2,
        result_kind: ResultKind::Complete,
    };
    let json: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&response).expect("serialize")).expect("parse");
    assert_eq!(json["response"], "hi there");
    assert_eq!(json["model"], "gpt-4");
    assert_eq!(json["iterations"], 2);
    assert_eq!(json["result_kind"], "complete");
}

#[test]
fn health_response_has_expected_fields() {
    let response = HealthResponse {
        status: "ok",
        model: "claude-3".to_string(),
        uptime_seconds: 60,
        skills_loaded: 3,
        https_enabled: true,
    };
    let json: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&response).expect("serialize")).expect("parse");
    assert_eq!(json["status"], "ok");
    assert_eq!(json["model"], "claude-3");
    assert_eq!(json["uptime_seconds"], 60);
    assert_eq!(json["skills_loaded"], 3);
    assert_eq!(json["https_enabled"], true);
}

#[test]
fn status_response_has_expected_fields() {
    let response = StatusResponse {
        status: "ok",
        model: "claude-3".to_string(),
        skills: vec!["read_file".to_string()],
        memory_entries: 42,
        tailscale_ip: Some("192.0.2.1".to_string()),
        config: None,
    };
    let json: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&response).expect("serialize")).expect("parse");
    assert_eq!(json["status"], "ok");
    assert_eq!(json["tailscale_ip"], "192.0.2.1");
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
    assert_eq!(json["tailscale_ip"], "192.0.2.10");
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
fn verify_token_empty_expected_rejected() {
    assert!(!verify_token("", "some-token"));
}

#[test]
fn verify_token_empty_both_rejected() {
    // Empty tokens must never authenticate — defense-in-depth against
    // misconfigured HttpState with an empty bearer_token.
    assert!(!verify_token("", ""));
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
fn serialize_stream_event_serializes_notification_payload() {
    let frame = serialize_stream_event(StreamEvent::Notification {
        title: "Fawx".to_string(),
        body: "Task complete".to_string(),
    })
    .expect("notification frame");

    assert!(frame.contains("event: notification"));
    assert!(frame.contains("\"title\":\"Fawx\""));
    assert!(frame.contains("\"body\":\"Task complete\""));
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
fn serialize_stream_event_serializes_tool_error_payload() {
    let frame = serialize_stream_event(StreamEvent::ToolError {
        tool_name: "read_file".to_string(),
        error: "permission denied".to_string(),
    })
    .expect("tool error frame");

    assert!(frame.contains("event: tool_error"));
    assert!(frame.contains("\"tool_name\":\"read_file\""));
    assert!(frame.contains("\"error\":\"permission denied\""));
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
    use crate::server_runtime::{RestartAction, RestartController, RestartRequestor};
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
        MessageRole as SessionMessageRole, SessionConfig, SessionContentBlock, SessionError,
        SessionKey, SessionKind, SessionMemory, SessionMessage, SessionRegistry, SessionStatus,
        SessionStore,
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

    #[derive(Clone)]
    struct StubRestartRequestor {
        action: RestartAction,
    }

    impl RestartRequestor for StubRestartRequestor {
        fn request_restart(&self) -> Result<RestartAction, String> {
            Ok(self.action.clone())
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
            Arc::new(StaticProvider {
                name: "anthropic",
                models: vec!["claude-sonnet-4-20250514"],
            }),
            "api_key",
        );
        router.register_provider_with_auth(
            Arc::new(StaticProvider {
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

    fn thinking_test_router() -> ModelRouter {
        let mut router = ModelRouter::new();
        router.register_provider_with_auth(
            Arc::new(StaticProvider {
                name: "anthropic",
                models: vec!["claude-sonnet-4-6", "claude-opus-4-6"],
            }),
            "api_key",
        );
        router.register_provider_with_auth(
            Arc::new(StaticProvider {
                name: "openai",
                models: vec!["gpt-5.4"],
            }),
            "api_key",
        );
        router.set_active("claude-sonnet-4-6").expect("set active");
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
        mut config: fx_config::FawxConfig,
        config_manager: Option<Arc<StdMutex<ConfigManager>>>,
        runtime_info: Arc<std::sync::RwLock<RuntimeInfo>>,
    ) -> HeadlessApp {
        if config.general.data_dir.is_none() {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let data_dir = std::env::temp_dir().join(format!("fawx-api-tests-{unique}"));
            std::fs::create_dir_all(&data_dir).expect("create temp data dir");
            config.general.data_dir = Some(data_dir);
        }
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
            router: Arc::new(std::sync::RwLock::new(router)),
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
            cron_store: None,
            startup_warnings: Vec::new(),
            stream_callback_slot: Arc::new(std::sync::Mutex::new(None)),
            ripcord_journal: Arc::new(fx_ripcord::RipcordJournal::new(
                std::env::temp_dir().as_path(),
            )),
            experiment_registry: None,
        })
        .expect("test app")
    }

    #[derive(Debug, Default)]
    struct SessionMemoryPersistingState {
        current_memory: SessionMemory,
        loaded_memories: Vec<SessionMemory>,
        last_session_messages: Vec<SessionMessage>,
        loaded_session_key: Option<SessionKey>,
    }

    #[derive(Clone)]
    struct SessionMemoryPersistingTestApp {
        state: Arc<StdMutex<SessionMemoryPersistingState>>,
    }

    impl SessionMemoryPersistingTestApp {
        fn new(initial_memory: SessionMemory, loaded_session_key: Option<SessionKey>) -> Self {
            Self {
                state: Arc::new(StdMutex::new(SessionMemoryPersistingState {
                    current_memory: initial_memory,
                    loaded_memories: Vec::new(),
                    last_session_messages: Vec::new(),
                    loaded_session_key,
                })),
            }
        }
    }

    #[async_trait]
    impl AppEngine for SessionMemoryPersistingTestApp {
        async fn process_message(
            &mut self,
            input: &str,
            images: Vec<ImageAttachment>,
            documents: Vec<DocumentAttachment>,
            source: InputSource,
            callback: Option<StreamCallback>,
        ) -> Result<ApiCycleResult, anyhow::Error> {
            let (result, _) = self
                .process_message_with_context(
                    input,
                    images,
                    documents,
                    Vec::new(),
                    source,
                    callback,
                )
                .await?;
            Ok(result)
        }

        async fn process_message_with_context(
            &mut self,
            input: &str,
            _images: Vec<ImageAttachment>,
            _documents: Vec<DocumentAttachment>,
            context: Vec<Message>,
            _source: InputSource,
            callback: Option<StreamCallback>,
        ) -> Result<(ApiCycleResult, Vec<Message>), anyhow::Error> {
            let response = "Stored memory response".to_string();
            {
                let mut state = self.state.lock().expect("state lock");
                let loaded_memory = state.current_memory.clone();
                state.loaded_memories.push(loaded_memory);
                state.current_memory.current_state = Some("updated during turn".to_string());
                state.last_session_messages = vec![
                    SessionMessage::text(SessionMessageRole::User, input, 2),
                    SessionMessage::text(SessionMessageRole::Assistant, &response, 3),
                ];
            }
            if let Some(callback) = callback {
                callback(StreamEvent::PhaseChange {
                    phase: fx_kernel::Phase::Synthesize,
                });
                callback(StreamEvent::TextDelta {
                    text: response.clone(),
                });
                callback(StreamEvent::Done {
                    response: response.clone(),
                });
            }
            Ok((
                ApiCycleResult {
                    response,
                    model: "mock-model".to_string(),
                    iterations: 1,
                    result_kind: ResultKind::Complete,
                },
                context,
            ))
        }

        fn active_model(&self) -> &str {
            "mock-model"
        }

        fn available_models(&self) -> Vec<ModelInfoDto> {
            vec![ModelInfoDto {
                model_id: "mock-model".to_string(),
                provider: "test".to_string(),
                auth_method: "none".to_string(),
            }]
        }

        fn set_active_model(&mut self, selector: &str) -> Result<ModelSwitchDto, anyhow::Error> {
            Ok(ModelSwitchDto {
                previous_model: "mock-model".to_string(),
                active_model: selector.to_string(),
                thinking_adjusted: None,
            })
        }

        fn thinking_level(&self) -> ThinkingLevelDto {
            ThinkingLevelDto {
                level: "minimal".to_string(),
                budget_tokens: None,
                available: vec!["minimal".to_string()],
            }
        }

        fn context_info(&self) -> ContextInfoDto {
            ContextInfoDto {
                used_tokens: 0,
                max_tokens: 0,
                percentage: 0.0,
                compaction_threshold: 0.0,
            }
        }

        fn context_info_for_messages(&self, _messages: &[Message]) -> ContextInfoDto {
            self.context_info()
        }

        fn set_thinking_level(&mut self, level: &str) -> Result<ThinkingLevelDto, anyhow::Error> {
            Ok(ThinkingLevelDto {
                level: level.to_string(),
                budget_tokens: None,
                available: vec![level.to_string()],
            })
        }

        fn skill_summaries(&self) -> Vec<SkillSummaryDto> {
            Vec::new()
        }

        fn auth_provider_statuses(&self) -> Vec<AuthProviderDto> {
            Vec::new()
        }

        fn config_manager(&self) -> Option<ConfigManagerHandle> {
            None
        }

        fn session_bus(&self) -> Option<&SessionBus> {
            None
        }

        fn recent_errors(&self, _limit: usize) -> Vec<ErrorRecordDto> {
            Vec::new()
        }

        fn replace_session_memory(&mut self, memory: SessionMemory) -> SessionMemory {
            let mut state = self.state.lock().expect("state lock");
            std::mem::replace(&mut state.current_memory, memory)
        }

        fn session_memory(&self) -> SessionMemory {
            self.state
                .lock()
                .expect("state lock")
                .current_memory
                .clone()
        }

        fn loaded_session_key(&self) -> Option<SessionKey> {
            self.state
                .lock()
                .expect("state lock")
                .loaded_session_key
                .clone()
        }

        fn take_last_session_messages(&mut self) -> Vec<SessionMessage> {
            std::mem::take(&mut self.state.lock().expect("state lock").last_session_messages)
        }
    }

    fn session_memory_test_router(
        registry: SessionRegistry,
        initial_memory: SessionMemory,
        loaded_session_key: Option<SessionKey>,
    ) -> (Router, Arc<StdMutex<SessionMemoryPersistingState>>) {
        let app = SessionMemoryPersistingTestApp::new(initial_memory, loaded_session_key);
        let app_state = Arc::clone(&app.state);
        let mut state = test_state_with_sessions(registry);
        state.shared = Arc::new(SharedReadState::from_app(&app));
        state.app = Arc::new(Mutex::new(app));
        (build_router(state, None), app_state)
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
                    capabilities: Vec::new(),
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

    fn synthesis_state(has_initial_value: bool) -> Arc<crate::handlers::synthesis::SynthesisState> {
        Arc::new(crate::handlers::synthesis::SynthesisState::new(
            has_initial_value,
        ))
    }

    fn telemetry_with_enabled_categories(categories: &[SignalCategory]) -> Arc<SignalCollector> {
        let mut consent = TelemetryConsent {
            enabled: true,
            ..TelemetryConsent::default()
        };
        for category in categories {
            consent.enable_category(category.clone());
        }
        Arc::new(SignalCollector::new(consent))
    }

    fn test_state(
        config_manager: Option<Arc<StdMutex<ConfigManager>>>,
        webhooks: Vec<Arc<WebhookChannel>>,
    ) -> HttpState {
        test_state_with_config(fx_config::FawxConfig::default(), config_manager, webhooks)
    }

    fn test_server_runtime() -> ServerRuntime {
        ServerRuntime::local(8400)
    }

    fn test_server_runtime_with_restart(action: RestartAction) -> ServerRuntime {
        ServerRuntime::new(
            "127.0.0.1",
            8400,
            false,
            RestartController::from_requestor(Arc::new(StubRestartRequestor { action })),
        )
    }

    fn test_state_with_app(app: HeadlessApp, webhooks: Vec<Arc<WebhookChannel>>) -> HttpState {
        let data_dir = app
            .config()
            .general
            .data_dir
            .clone()
            .unwrap_or_else(std::env::temp_dir);
        let has_synthesis = app.config().model.synthesis_instruction.is_some();
        let shared = Arc::new(SharedReadState::from_app(&app));
        HttpState {
            app: Arc::new(Mutex::new(app)),
            shared,
            config_manager: None,
            session_registry: None,
            start_time: Instant::now(),
            server_runtime: test_server_runtime(),
            tailscale_ip: None,
            bearer_token: TEST_TOKEN.to_string(),
            pairing: Arc::new(Mutex::new(PairingState::new())),
            devices: Arc::new(Mutex::new(DeviceStore::new())),
            devices_path: None,
            channels: build_channel_runtime(None, webhooks),
            data_dir,
            synthesis: synthesis_state(has_synthesis),
            oauth_flows: Arc::new(crate::handlers::oauth::OAuthFlowStore::new()),
            permission_prompts: Arc::new(fx_kernel::PermissionPromptState::new()),
            ripcord: None,
            fleet_manager: None,
            cron_store: None,
            experiment_registry: {
                let registry = ExperimentRegistry::new(std::env::temp_dir().as_path()).unwrap();
                Arc::new(tokio::sync::Mutex::new(registry))
            },
            improvement_provider: None,
            telemetry: in_memory_telemetry(),
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
        let has_synthesis = config.model.synthesis_instruction.is_some();
        let app = make_test_app_with_config(config, config_manager.clone());
        let shared = Arc::new(SharedReadState::from_app(&app));
        HttpState {
            app: Arc::new(Mutex::new(app)),
            shared,
            config_manager,
            session_registry: None,
            start_time: Instant::now(),
            server_runtime: test_server_runtime(),
            tailscale_ip: None,
            bearer_token: TEST_TOKEN.to_string(),
            pairing: Arc::new(Mutex::new(PairingState::new())),
            devices: Arc::new(Mutex::new(DeviceStore::new())),
            devices_path: None,
            channels: build_channel_runtime(None, webhooks),
            data_dir,
            synthesis: synthesis_state(has_synthesis),
            oauth_flows: Arc::new(crate::handlers::oauth::OAuthFlowStore::new()),
            permission_prompts: Arc::new(fx_kernel::PermissionPromptState::new()),
            ripcord: None,
            fleet_manager: None,
            cron_store: None,
            experiment_registry: {
                let registry = ExperimentRegistry::new(std::env::temp_dir().as_path()).unwrap();
                Arc::new(tokio::sync::Mutex::new(registry))
            },
            improvement_provider: None,
            telemetry: in_memory_telemetry(),
        }
    }

    fn test_state_with_devices(devices: DeviceStore) -> HttpState {
        let mut state = test_state(None, Vec::new());
        state.devices = Arc::new(Mutex::new(devices));
        state
    }

    fn test_state_with_telemetry(telemetry: Arc<SignalCollector>) -> HttpState {
        let mut state = test_state(None, Vec::new());
        state.telemetry = telemetry;
        state
    }

    fn test_state_with_server_runtime(server_runtime: ServerRuntime) -> HttpState {
        let mut state = test_state(None, Vec::new());
        state.server_runtime = server_runtime;
        state
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

    #[tokio::test]
    async fn telemetry_consent_endpoint_returns_defaults() {
        let app = build_router(test_state(None, Vec::new()), None);

        let response = app
            .oneshot(authed_request("GET", "/v1/telemetry/consent"))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["enabled"], false);
        assert_eq!(json["categories"].as_object().expect("categories").len(), 6);
        assert_eq!(json["categories"]["tool_usage"]["enabled"], false);
        assert_eq!(
            json["categories"]["proposal_gate"]["description"],
            "How often the safety gate activates"
        );
        assert!(json["updated_at"].as_str().is_some());
    }

    #[tokio::test]
    async fn telemetry_consent_patch_updates_master_and_categories() {
        let app = build_router(test_state(None, Vec::new()), None);

        let update = app
            .clone()
            .oneshot(authed_json_request(
                "PATCH",
                "/v1/telemetry/consent",
                r#"{"enabled":true,"categories":{"tool_usage":true,"errors":true}}"#,
            ))
            .await
            .expect("response");

        assert_eq!(update.status(), StatusCode::OK);
        let update_json = response_json(update).await;
        assert_eq!(update_json["enabled"], true);
        assert_eq!(update_json["categories"]["tool_usage"]["enabled"], true);
        assert_eq!(update_json["categories"]["errors"]["enabled"], true);
        assert_eq!(update_json["categories"]["model_usage"]["enabled"], false);

        let current = app
            .oneshot(authed_request("GET", "/v1/telemetry/consent"))
            .await
            .expect("response");
        let current_json = response_json(current).await;
        assert_eq!(current_json["enabled"], true);
        assert_eq!(current_json["categories"]["tool_usage"]["enabled"], true);
        assert_eq!(current_json["categories"]["errors"]["enabled"], true);
    }

    #[tokio::test]
    async fn telemetry_signals_endpoint_drains_buffer() {
        let telemetry =
            telemetry_with_enabled_categories(&[SignalCategory::ToolUsage, SignalCategory::Errors]);
        telemetry.record(
            SignalCategory::ToolUsage,
            "tool_call",
            serde_json::json!({"tool": "read"}),
        );
        telemetry.record(
            SignalCategory::Errors,
            "error",
            serde_json::json!({"code": 500}),
        );
        let app = build_router(test_state_with_telemetry(telemetry), None);

        let response = app
            .clone()
            .oneshot(authed_request("GET", "/v1/telemetry/signals"))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["count"], 2);
        assert_eq!(json["signals"][0]["category"], "tool_usage");
        assert_eq!(json["signals"][1]["category"], "errors");

        let empty = app
            .oneshot(authed_request("GET", "/v1/telemetry/signals"))
            .await
            .expect("response");
        let empty_json = response_json(empty).await;
        assert_eq!(empty_json["count"], 0);
        assert_eq!(empty_json["signals"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn telemetry_delete_signals_endpoint_clears_buffer() {
        let telemetry = telemetry_with_enabled_categories(&[SignalCategory::Errors]);
        telemetry.record(
            SignalCategory::Errors,
            "error",
            serde_json::json!({"code": 500}),
        );
        let app = build_router(test_state_with_telemetry(telemetry), None);

        let delete = app
            .clone()
            .oneshot(authed_request("DELETE", "/v1/telemetry/signals"))
            .await
            .expect("response");
        assert_eq!(delete.status(), StatusCode::NO_CONTENT);

        let response = app
            .oneshot(authed_request("GET", "/v1/telemetry/signals"))
            .await
            .expect("response");
        let json = response_json(response).await;
        assert_eq!(json["count"], 0);
        assert_eq!(json["signals"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn get_devices_requires_auth() {
        let app = build_router(test_state(None, Vec::new()), None);
        let req = Request::builder()
            .method("GET")
            .uri("/v1/devices")
            .body(Body::empty())
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn get_devices_returns_device_list() {
        let mut devices = DeviceStore::new();
        let (_, first) = devices.create_device("Joe's MacBook");
        let (_, second) = devices.create_device("Joe's iPhone");
        let app = build_router(test_state_with_devices(devices), None);

        let response = app
            .oneshot(authed_request("GET", "/v1/devices"))
            .await
            .expect("response");
        let json = response_json(response).await;

        assert_eq!(json["devices"].as_array().expect("devices").len(), 2);
        assert_eq!(json["devices"][0]["id"], first.id);
        assert_eq!(json["devices"][0]["device_name"], first.device_name);
        assert_eq!(json["devices"][1]["id"], second.id);
        assert_eq!(json["devices"][1]["device_name"], second.device_name);
    }

    #[tokio::test]
    async fn get_devices_excludes_token_hash() {
        let mut devices = DeviceStore::new();
        let _ = devices.create_device("Joe's MacBook");
        let app = build_router(test_state_with_devices(devices), None);

        let response = app
            .oneshot(authed_request("GET", "/v1/devices"))
            .await
            .expect("response");
        let json = response_json(response).await;

        assert!(json["devices"][0].get("token_hash").is_none());
    }

    #[tokio::test]
    async fn delete_device_revokes_token() {
        let mut devices = DeviceStore::new();
        let (raw_token, device) = devices.create_device("Joe's MacBook");
        let app = build_router(test_state_with_devices(devices), None);

        let before_delete = Request::builder()
            .method("GET")
            .uri("/status")
            .header("authorization", format!("Bearer {raw_token}"))
            .body(Body::empty())
            .expect("request");
        let before_response = app
            .clone()
            .oneshot(before_delete)
            .await
            .expect("response before delete");
        assert_eq!(before_response.status(), StatusCode::OK);

        let delete_response = app
            .clone()
            .oneshot(authed_request(
                "DELETE",
                &format!("/v1/devices/{}", device.id),
            ))
            .await
            .expect("delete response");
        assert_eq!(delete_response.status(), StatusCode::OK);

        let after_delete = Request::builder()
            .method("GET")
            .uri("/status")
            .header("authorization", format!("Bearer {raw_token}"))
            .body(Body::empty())
            .expect("request");
        let after_response = app
            .oneshot(after_delete)
            .await
            .expect("response after delete");
        assert_eq!(after_response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn delete_device_not_found() {
        let app = build_router(test_state(None, Vec::new()), None);

        let response = app
            .oneshot(authed_request("DELETE", "/v1/devices/dev-missing"))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let json = response_json(response).await;

        assert_eq!(json["error"], "device not found");
    }

    #[tokio::test]
    async fn delete_device_requires_auth() {
        let app = build_router(test_state(None, Vec::new()), None);
        let req = Request::builder()
            .method("DELETE")
            .uri("/v1/devices/dev-123")
            .body(Body::empty())
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn setup_status_endpoint_returns_expected_shape() {
        let temp = TempDir::new().expect("tempdir");
        std::fs::write(
            temp.path().join("config.toml"),
            "[model]\ndefault_model = \"mock-model\"\n",
        )
        .expect("write config");
        let mut config = fx_config::FawxConfig::default();
        config.general.data_dir = Some(temp.path().to_path_buf());
        let app = build_router(test_state_with_config(config, None, Vec::new()), None);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/v1/setup/status")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["mode"], "local");
        assert_eq!(json["setup_complete"], true);
        assert_eq!(json["has_valid_config"], true);
        assert_eq!(json["server_running"], true);
        assert_eq!(json["local_server"]["host"], "127.0.0.1");
        assert_eq!(json["local_server"]["port"], 8400);
        assert_eq!(json["local_server"]["https_enabled"], false);
        assert!(json["launchagent"]["installed"].is_boolean());
        assert!(json["launchagent"]["loaded"].is_boolean());
        assert!(json["launchagent"]["auto_start_enabled"].is_boolean());
        assert_eq!(json["auth"]["bearer_token_present"], true);
        assert!(json["auth"]["providers_configured"].is_array());
        assert!(json["tailscale"]["installed"].is_boolean());
        assert!(json["tailscale"]["running"].is_boolean());
        assert!(json["tailscale"]["logged_in"].is_boolean());
    }

    #[tokio::test]
    async fn server_status_endpoint_returns_expected_shape() {
        let app = build_router(test_state(None, Vec::new()), None);

        let response = app
            .oneshot(authed_request("GET", "/v1/server/status"))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["status"], "running");
        assert_eq!(json["host"], "127.0.0.1");
        assert_eq!(json["port"], 8400);
        assert_eq!(json["https_enabled"], false);
        assert!(json["uptime_seconds"].as_u64().is_some());
        assert!(json["pid"].as_u64().is_some());
    }

    #[tokio::test]
    async fn server_restart_endpoint_returns_accepted_response() {
        let server_runtime = test_server_runtime_with_restart(RestartAction {
            restart_via: "launchagent_keepalive",
            message: "Server restart requested.",
        });
        let app = build_router(test_state_with_server_runtime(server_runtime), None);

        let response = app
            .oneshot(authed_request("POST", "/v1/server/restart"))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["accepted"], true);
        assert_eq!(json["restart_via"], "launchagent_keepalive");
        assert_eq!(json["message"], "Server restart requested.");
    }

    #[tokio::test]
    async fn config_patch_endpoint_merges_changes_and_returns_changed_keys() {
        let (temp, config, manager) = temp_config_manager("[http]\nbearer_token = \"secret\"\n");
        let app = build_router(
            test_state_with_config(config, Some(manager), Vec::new()),
            None,
        );

        let response = app
            .oneshot(authed_json_request(
                "PATCH",
                "/v1/config",
                r#"{"changes":{"http":{"port":8401},"ui":{"auto_start":true}}}"#,
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["updated"], true);
        assert_eq!(json["restart_required"], true);
        assert_eq!(
            json["changed_keys"],
            serde_json::json!(["http.port", "ui.auto_start"])
        );

        let content =
            std::fs::read_to_string(temp.path().join("config.toml")).expect("read config");
        assert!(content.contains("port = 8401"));
        assert!(content.contains("[ui]"));
        assert!(content.contains("auto_start = true"));
    }

    #[tokio::test]
    async fn config_presets_endpoint_lists_available_presets() {
        let temp = TempDir::new().expect("tempdir");
        let mut config = fx_config::FawxConfig::default();
        config.general.data_dir = Some(temp.path().to_path_buf());
        let app = build_router(test_state_with_config(config, None, Vec::new()), None);

        let response = app
            .oneshot(authed_request("GET", "/v1/config/presets"))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["total"], 2);
        assert_eq!(json["presets"][0]["name"], "safe");
        assert_eq!(
            json["presets"][0]["description"],
            "Conservative defaults for cautious use."
        );
        assert_eq!(json["presets"][1]["name"], "power-user");
        assert_eq!(
            json["presets"][1]["description"],
            "Fewer confirmations, higher autonomy."
        );
    }

    #[tokio::test]
    async fn config_patch_endpoint_rejects_invalid_json_body() {
        let app = build_router(test_state(None, Vec::new()), None);

        let response = app
            .oneshot(authed_json_request(
                "PATCH",
                "/v1/config",
                r#"{"changes":{"http":{"port":8401}"#,
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn apply_config_preset_endpoint_returns_not_found_for_unknown_preset() {
        let app = build_router(test_state(None, Vec::new()), None);

        let response = app
            .oneshot(authed_json_request(
                "POST",
                "/v1/config/preset/unknown",
                r#"{"confirm":true}"#,
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let json = response_json(response).await;
        assert_eq!(json["error"], "unknown config preset: unknown");
    }

    #[tokio::test]
    async fn apply_config_preset_endpoint_rejects_confirm_false() {
        let app = build_router(test_state(None, Vec::new()), None);

        let response = app
            .oneshot(authed_json_request(
                "POST",
                "/v1/config/preset/safe",
                r#"{"confirm":false}"#,
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let json = response_json(response).await;
        assert_eq!(json["error"], "preset application requires confirm=true");
    }

    #[tokio::test]
    async fn apply_config_preset_endpoint_updates_permissions_config() {
        let (temp, config, manager) = temp_config_manager("");
        let app = build_router(
            test_state_with_config(config, Some(manager), Vec::new()),
            None,
        );

        let response = app
            .oneshot(authed_json_request(
                "POST",
                "/v1/config/preset/safe",
                r#"{"confirm":true}"#,
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["name"], "safe");
        assert_eq!(json["applied"], true);
        assert_eq!(json["restart_required"], false);
        assert!(json["changed_keys"]
            .as_array()
            .is_some_and(|keys| !keys.is_empty()));

        let content =
            std::fs::read_to_string(temp.path().join("config.toml")).expect("read config");
        assert!(content.contains("preset = \"cautious\""));
        assert!(content.contains("proposal_required"));
    }

    #[tokio::test]
    async fn config_preset_diff_endpoint_previews_changes() {
        let (temp, config, manager) = temp_config_manager("");
        let app = build_router(
            test_state_with_config(config, Some(manager), Vec::new()),
            None,
        );

        let response = app
            .oneshot(authed_request("GET", "/v1/config/preset/safe/diff"))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["name"], "safe");
        assert_eq!(json["changes"][0]["key"], "permissions.preset");
        assert_eq!(json["changes"][0]["old"], "power");
        assert_eq!(json["changes"][0]["new"], "cautious");

        let content =
            std::fs::read_to_string(temp.path().join("config.toml")).expect("read config");
        assert!(content.is_empty());
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
            .add_node("macmini", "198.51.100.19", 8400)
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

        let message_response = app
            .clone()
            .oneshot(authed_json_request(
                "POST",
                &format!("/v1/sessions/{key}/messages"),
                r#"{"message":"hello there"}"#,
            ))
            .await
            .expect("message response");
        assert_eq!(message_response.status(), StatusCode::OK);

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
    async fn documents_field_accepted_in_message_api() {
        let registry = make_session_registry();
        let key = seed_session(&registry, "sess-documents");
        let app = build_router(test_state_with_sessions(registry), None);
        let body = serde_json::json!({
            "message": "Summarize this brief",
            "documents": [{
                "data": base64::engine::general_purpose::STANDARD.encode(b"%PDF-1.4\n"),
                "media_type": "application/pdf",
                "filename": "brief.pdf"
            }]
        });

        let response = app
            .oneshot(authed_json_request(
                "POST",
                &format!("/v1/sessions/{key}/messages"),
                &body.to_string(),
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["model"], "mock-model");
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
    async fn get_session_messages_returns_structured_blocks() {
        let registry = make_session_registry();
        let key = seed_session(&registry, "sess-structured");
        registry
            .record_message_blocks(
                &key,
                SessionMessageRole::Assistant,
                vec![SessionContentBlock::ToolUse {
                    id: "call_1".to_string(),
                    provider_id: None,
                    name: "read_file".to_string(),
                    input: serde_json::json!({"path": "README.md"}),
                }],
                Some(12),
            )
            .expect("record tool use");
        registry
            .record_message_blocks(
                &key,
                SessionMessageRole::Tool,
                vec![SessionContentBlock::ToolResult {
                    tool_use_id: "call_1".to_string(),
                    content: serde_json::json!("file contents"),
                    is_error: Some(false),
                }],
                None,
            )
            .expect("record tool result");
        let app = build_router(test_state_with_sessions(registry), None);

        let req = Request::builder()
            .method("GET")
            .uri(format!("/v1/sessions/{key}/messages"))
            .header("authorization", format!("Bearer {TEST_TOKEN}"))
            .body(Body::empty())
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.expect("body").to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");

        assert_eq!(json["total"], 2);
        assert_eq!(json["messages"][0]["role"], "assistant");
        assert_eq!(json["messages"][0]["content"][0]["type"], "tool_use");
        assert_eq!(json["messages"][0]["token_count"], 12);
        assert_eq!(json["messages"][1]["role"], "tool");
        assert_eq!(json["messages"][1]["content"][0]["type"], "tool_result");
        assert_eq!(json["messages"][1]["content"][0]["is_error"], false);
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
        assert_eq!(
            history[0].content,
            vec![SessionContentBlock::Text {
                text: "hello there".to_string()
            }]
        );
        assert_eq!(history[1].role, SessionMessageRole::Assistant);
        assert_eq!(
            history[1].content,
            vec![SessionContentBlock::Text {
                text: "Mock response".to_string()
            }]
        );
    }

    #[tokio::test]
    async fn session_message_ignores_unresolved_prior_tool_use_in_context() {
        #[derive(Clone)]
        struct LocalCapturingProvider {
            captured: Arc<std::sync::Mutex<Vec<fx_llm::CompletionRequest>>>,
        }

        #[async_trait]
        impl fx_llm::CompletionProvider for LocalCapturingProvider {
            async fn complete(
                &self,
                request: fx_llm::CompletionRequest,
            ) -> Result<CompletionResponse, fx_llm::ProviderError> {
                self.captured.lock().expect("capture lock").push(request);
                Ok(mock_completion_response())
            }

            async fn complete_stream(
                &self,
                request: fx_llm::CompletionRequest,
            ) -> Result<CompletionStream, fx_llm::ProviderError> {
                self.captured.lock().expect("capture lock").push(request);
                Ok(mock_completion_stream())
            }

            fn name(&self) -> &str {
                "capturing"
            }

            fn supported_models(&self) -> Vec<String> {
                vec!["capturing-model".to_string()]
            }

            fn capabilities(&self) -> fx_llm::ProviderCapabilities {
                fx_llm::ProviderCapabilities {
                    supports_temperature: false,
                    requires_streaming: false,
                }
            }
        }

        let registry = make_session_registry();
        let key = seed_session(&registry, "sess-orphan-tool");
        registry
            .record_message(&key, SessionMessageRole::User, "first request")
            .expect("record user");
        registry
            .record_message_blocks(
                &key,
                SessionMessageRole::Assistant,
                vec![SessionContentBlock::ToolUse {
                    id: "call_good".to_string(),
                    provider_id: Some("fc_good".to_string()),
                    name: "read_file".to_string(),
                    input: serde_json::json!({"path": "good.txt"}),
                }],
                Some(10),
            )
            .expect("record resolved tool use");
        registry
            .record_message_blocks(
                &key,
                SessionMessageRole::Tool,
                vec![SessionContentBlock::ToolResult {
                    tool_use_id: "call_good".to_string(),
                    content: serde_json::json!("ok"),
                    is_error: Some(false),
                }],
                None,
            )
            .expect("record resolved tool result");
        registry
            .record_message_blocks(
                &key,
                SessionMessageRole::Assistant,
                vec![SessionContentBlock::ToolUse {
                    id: "call_bad".to_string(),
                    provider_id: Some("fc_bad".to_string()),
                    name: "read_file".to_string(),
                    input: serde_json::json!({"path": "bad.txt"}),
                }],
                Some(10),
            )
            .expect("record orphan tool use");

        let captured = Arc::new(std::sync::Mutex::new(
            Vec::<fx_llm::CompletionRequest>::new(),
        ));
        let mut router = fx_llm::ModelRouter::new();
        router.register_provider(Box::new(LocalCapturingProvider {
            captured: Arc::clone(&captured),
        }));
        router
            .set_active("capturing-model")
            .expect("set active capturing model");
        let app = build_test_app(
            router,
            fx_config::FawxConfig::default(),
            None,
            test_runtime_info(),
        );
        let mut state = test_state_with_app(app, Vec::new());
        state.session_registry = Some(registry);
        let app = build_router(state, None);

        let req = Request::builder()
            .method("POST")
            .uri(format!("/v1/sessions/{key}/messages"))
            .header("authorization", format!("Bearer {TEST_TOKEN}"))
            .header("content-type", "application/json")
            .body(Body::from(r#"{"message":"continue from here"}"#))
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);

        let captured_request = captured
            .lock()
            .expect("capture lock")
            .last()
            .cloned()
            .expect("captured request");

        assert!(captured_request
            .messages
            .iter()
            .flat_map(|message| &message.content)
            .any(|block| matches!(block, ContentBlock::ToolUse { id, .. } if id == "call_good")));
        assert!(!captured_request
            .messages
            .iter()
            .flat_map(|message| &message.content)
            .any(|block| matches!(block, ContentBlock::ToolUse { id, .. } if id == "call_bad")));
    }

    #[tokio::test]
    async fn get_session_memory_returns_stored_memory() {
        let registry = make_session_registry();
        let key = seed_session(&registry, "sess-memory-get");
        let mut memory = SessionMemory::default();
        memory.project = Some("Phase 6".to_string());
        memory.current_state = Some("Reviewing compaction UX".to_string());
        memory.key_decisions = vec!["Use a subtle banner".to_string()];
        memory.active_files = vec!["app/Fawx/ViewModels/ChatViewModel.swift".to_string()];
        memory.custom_context = vec!["Keep session memory user-editable".to_string()];
        memory.last_updated = 1_742_000_000;
        registry
            .record_turn(&key, Vec::new(), memory.clone())
            .expect("seed memory");
        let app = build_router(test_state_with_sessions(registry), None);

        let resp = app
            .oneshot(authed_request("GET", &format!("/v1/sessions/{key}/memory")))
            .await
            .expect("response");

        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_json(resp).await;
        assert_eq!(body, serde_json::to_value(memory).expect("memory json"));
    }

    #[tokio::test]
    async fn get_empty_session_memory_returns_empty_arrays() {
        let registry = make_session_registry();
        let key = seed_session(&registry, "sess-memory-empty");
        let app = build_router(test_state_with_sessions(registry), None);

        let resp = app
            .oneshot(authed_request("GET", &format!("/v1/sessions/{key}/memory")))
            .await
            .expect("response");

        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_json(resp).await;
        assert_eq!(
            body,
            serde_json::json!({
                "key_decisions": [],
                "active_files": [],
                "custom_context": [],
                "last_updated": 0
            })
        );
    }

    #[tokio::test]
    async fn put_session_memory_persists_and_updates_loaded_session_memory() {
        let registry = make_session_registry();
        let key = seed_session(&registry, "sess-memory-put");
        let mut initial_loaded_memory = SessionMemory::default();
        initial_loaded_memory.project = Some("Old loaded memory".to_string());
        let (app, app_state) =
            session_memory_test_router(registry.clone(), initial_loaded_memory, Some(key.clone()));

        let request_body = serde_json::json!({
            "project": "Phase 6",
            "current_state": "Implementing memory editing",
            "key_decisions": ["Expose memory in the UI"],
            "active_files": ["app/Fawx/Views/Shared/SessionMemoryPanel.swift"],
            "custom_context": ["Keep the panel lightweight"],
            "last_updated": 0
        })
        .to_string();

        let resp = app
            .oneshot(authed_json_request(
                "PUT",
                &format!("/v1/sessions/{key}/memory"),
                &request_body,
            ))
            .await
            .expect("response");

        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_json(resp).await;
        let stored_memory = registry.memory(&key).expect("memory");
        assert_eq!(stored_memory.project.as_deref(), Some("Phase 6"));
        assert_eq!(
            stored_memory.current_state.as_deref(),
            Some("Implementing memory editing")
        );
        assert_eq!(stored_memory.key_decisions, vec!["Expose memory in the UI"]);
        assert_eq!(
            stored_memory.active_files,
            vec!["app/Fawx/Views/Shared/SessionMemoryPanel.swift"]
        );
        assert_eq!(
            stored_memory.custom_context,
            vec!["Keep the panel lightweight"]
        );
        assert!(stored_memory.last_updated > 0);
        assert_eq!(
            body["last_updated"].as_u64(),
            Some(stored_memory.last_updated)
        );

        let state = app_state.lock().expect("state lock");
        assert_eq!(state.current_memory, stored_memory);
    }

    #[tokio::test]
    async fn put_session_memory_rejects_payloads_that_exceed_token_cap() {
        let registry = make_session_registry();
        let key = seed_session(&registry, "sess-memory-too-large");
        let mut seeded_memory = SessionMemory::default();
        seeded_memory.project = Some("Existing memory".to_string());
        registry
            .record_turn(&key, Vec::new(), seeded_memory.clone())
            .expect("seed memory");
        let app = build_router(test_state_with_sessions(registry.clone()), None);

        let oversized_project = "a ".repeat(8_100);
        let request_body = serde_json::json!({
            "project": oversized_project,
            "last_updated": 0
        })
        .to_string();

        let resp = app
            .oneshot(authed_json_request(
                "PUT",
                &format!("/v1/sessions/{key}/memory"),
                &request_body,
            ))
            .await
            .expect("response");

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = response_json(resp).await;
        assert!(body["error"]
            .as_str()
            .expect("error message")
            .contains("token cap"));
        assert_eq!(registry.memory(&key).expect("memory"), seeded_memory);
    }

    #[tokio::test]
    async fn session_message_persists_updated_session_memory() {
        let registry = make_session_registry();
        let key = seed_session(&registry, "sess-memory-persist");
        let mut seeded_memory = SessionMemory::default();
        seeded_memory.project = Some("persistent project".to_string());
        let mut restored_memory = SessionMemory::default();
        restored_memory.project = Some("shared app memory".to_string());
        registry
            .record_turn(&key, Vec::new(), seeded_memory.clone())
            .expect("seed memory");
        let (app, app_state) =
            session_memory_test_router(registry.clone(), restored_memory.clone(), None);

        let resp = app
            .oneshot(authed_json_request(
                "POST",
                &format!("/v1/sessions/{key}/messages"),
                r#"{"message":"hello there"}"#,
            ))
            .await
            .expect("response");
        assert_eq!(resp.status(), StatusCode::OK);

        let state = app_state.lock().expect("state lock");
        assert_eq!(state.loaded_memories, vec![seeded_memory.clone()]);
        assert_eq!(state.current_memory, restored_memory);
        drop(state);

        let history = registry.history(&key, 10).expect("history");
        let stored_memory = registry.memory(&key).expect("memory");
        assert_eq!(history.len(), 2);
        assert_eq!(history[1].render_text(), "Stored memory response");
        assert_eq!(stored_memory.project, seeded_memory.project);
        assert_eq!(
            stored_memory.current_state.as_deref(),
            Some("updated during turn")
        );
    }

    #[tokio::test]
    async fn session_message_stream_persists_updated_session_memory() {
        let registry = make_session_registry();
        let key = seed_session(&registry, "sess-memory-stream-persist");
        let mut seeded_memory = SessionMemory::default();
        seeded_memory.project = Some("persistent project".to_string());
        let mut restored_memory = SessionMemory::default();
        restored_memory.project = Some("shared app memory".to_string());
        registry
            .record_turn(&key, Vec::new(), seeded_memory.clone())
            .expect("seed memory");
        let (app, app_state) =
            session_memory_test_router(registry.clone(), restored_memory.clone(), None);

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
        let body = resp.into_body().collect().await.expect("body").to_bytes();
        let text = String::from_utf8(body.to_vec()).expect("utf8 body");
        assert!(text.contains("Stored memory response"));

        let state = app_state.lock().expect("state lock");
        assert_eq!(state.loaded_memories, vec![seeded_memory.clone()]);
        assert_eq!(state.current_memory, restored_memory);
        drop(state);

        let history = registry.history(&key, 10).expect("history");
        let stored_memory = registry.memory(&key).expect("memory");
        assert_eq!(history.len(), 2);
        assert_eq!(history[1].render_text(), "Stored memory response");
        assert_eq!(stored_memory.project, seeded_memory.project);
        assert_eq!(
            stored_memory.current_state.as_deref(),
            Some("updated during turn")
        );
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
        assert_eq!(history[0].render_text(), "hello there");
        assert_eq!(history[1].render_text(), "Mock response");
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
    async fn recent_errors_endpoint_returns_errors() {
        let startup_warning = fx_cli::headless::StartupWarning {
            category: fx_kernel::ErrorCategory::Memory,
            message: "Failed to initialize memory: broken store".to_string(),
        };
        let state = test_state_with_app(
            HeadlessApp::new(HeadlessAppDeps {
                loop_engine: test_engine(),
                router: Arc::new(std::sync::RwLock::new(settings_router())),
                runtime_info: test_runtime_info(),
                config: fx_config::FawxConfig::default(),
                memory: None,
                embedding_index_persistence: None,
                system_prompt_path: None,
                config_manager: None,
                system_prompt_text: None,
                subagent_manager: Arc::new(SubagentManager::new(SubagentManagerDeps {
                    factory: Arc::new(DisabledSubagentFactory::new("disabled")),
                    limits: SubagentLimits::default(),
                })),
                canary_monitor: None,
                session_bus: None,
                session_key: None,
                cron_store: None,
                startup_warnings: vec![startup_warning],
                stream_callback_slot: Arc::new(std::sync::Mutex::new(None)),
                ripcord_journal: Arc::new(fx_ripcord::RipcordJournal::new(
                    std::env::temp_dir().as_path(),
                )),
                experiment_registry: None,
            })
            .expect("test app"),
            Vec::new(),
        );
        let app = build_router(state, None);

        let response = app
            .oneshot(authed_request("GET", "/v1/errors/recent"))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["errors"].as_array().expect("errors").len(), 1);
        assert_eq!(json["errors"][0]["category"], "memory");
        assert_eq!(
            json["errors"][0]["message"],
            "Failed to initialize memory: broken store"
        );
        assert_eq!(json["errors"][0]["recoverable"], true);
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
        let mut config = fx_config::FawxConfig::default();
        config.model.default_model = Some("claude-sonnet-4-6".to_string());
        // Default ThinkingBudget is Adaptive
        let state = test_state_with_app(
            build_test_app(thinking_test_router(), config, None, test_runtime_info()),
            Vec::new(),
        );
        let app = build_router(state, None);
        let response = app
            .oneshot(authed_request("GET", "/v1/thinking"))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["level"], "adaptive");
        assert_eq!(
            json["available"],
            serde_json::json!(["off", "adaptive", "low", "medium", "high"])
        );
    }

    #[tokio::test]
    async fn set_thinking_valid_level_returns_200() {
        let (_temp, config, manager) = temp_config_manager(
            "[model]\ndefault_model = \"claude-sonnet-4-6\"\n\n[general]\nthinking = \"high\"\n",
        );
        let state = test_state_with_app(
            build_test_app(
                thinking_test_router(),
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
        assert_eq!(update_json["previous_level"], "high");
        assert_eq!(update_json["level"], "high");

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
            "unknown thinking level 'turbo'; expected off, none, minimal, low, medium, high, xhigh, max, or adaptive"
        );
    }

    #[tokio::test]
    async fn set_thinking_rejects_level_unsupported_by_current_provider() {
        let (_temp, config, manager) = temp_config_manager(
            r#"[model]
default_model = "gpt-5.4"

[general]
thinking = "low"
"#,
        );
        let state = test_state_with_app(
            build_test_app(
                thinking_test_router(),
                config,
                Some(manager),
                test_runtime_info(),
            ),
            Vec::new(),
        );
        let app = build_router(state, None);

        let response = app
            .oneshot(authed_json_request(
                "PUT",
                "/v1/thinking",
                r#"{"level":"adaptive"}"#,
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let json = response_json(response).await;
        assert_eq!(
            json["error"],
            "Thinking level 'adaptive' is not supported by the current model. Available: none, low, medium, high, xhigh"
        );
    }

    #[tokio::test]
    async fn set_model_auto_downgrades_unsupported_thinking_level() {
        let (_temp, config, manager) = temp_config_manager(
            r#"[model]
default_model = "claude-sonnet-4-6"

[general]
thinking = "adaptive"
"#,
        );
        let state = test_state_with_app(
            build_test_app(
                thinking_test_router(),
                config,
                Some(manager),
                test_runtime_info(),
            ),
            Vec::new(),
        );
        let app = build_router(state, None);

        let response = app
            .clone()
            .oneshot(authed_json_request(
                "PUT",
                "/v1/model",
                r#"{"model":"gpt-5.4"}"#,
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["active_model"], "gpt-5.4");
        assert_eq!(json["thinking_adjusted"]["from"], "adaptive");
        assert_eq!(json["thinking_adjusted"]["to"], "high");
        assert_eq!(
            json["thinking_adjusted"]["reason"],
            "adaptive not supported by openai; adjusted to high"
        );

        let thinking = app
            .oneshot(authed_request("GET", "/v1/thinking"))
            .await
            .expect("response");
        let thinking_json = response_json(thinking).await;
        assert_eq!(thinking_json["level"], "high");
        assert_eq!(
            thinking_json["available"],
            serde_json::json!(["none", "low", "medium", "high", "xhigh"])
        );
    }

    #[tokio::test]
    async fn set_model_same_provider_preserves_thinking_level() {
        let (_temp, config, manager) = temp_config_manager(
            r#"[model]
default_model = "claude-sonnet-4-6"

[general]
thinking = "high"
"#,
        );
        let mut router = ModelRouter::new();
        router.register_provider_with_auth(
            Arc::new(StaticProvider {
                name: "anthropic",
                models: vec!["claude-sonnet-4-6", "claude-opus-4-6"],
            }),
            "api_key",
        );
        router.set_active("claude-sonnet-4-6").expect("set active");
        let state = test_state_with_app(
            build_test_app(router, config, Some(manager), test_runtime_info()),
            Vec::new(),
        );
        let app = build_router(state, None);

        let response = app
            .clone()
            .oneshot(authed_json_request(
                "PUT",
                "/v1/model",
                r#"{"model":"claude-opus-4-6"}"#,
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert!(json["thinking_adjusted"].is_null());

        let thinking = app
            .oneshot(authed_request("GET", "/v1/thinking"))
            .await
            .expect("response");
        let thinking_json = response_json(thinking).await;
        assert_eq!(thinking_json["level"], "high");
    }

    #[tokio::test]
    async fn get_thinking_unknown_provider_only_allows_off() {
        let mut router = ModelRouter::new();
        router.register_provider_with_auth(
            Arc::new(StaticProvider {
                name: "mystery",
                models: vec!["mystery-model"],
            }),
            "api_key",
        );
        router.set_active("mystery-model").expect("set active");
        let state = test_state_with_app(
            build_test_app(
                router,
                fx_config::FawxConfig::default(),
                None,
                test_runtime_info(),
            ),
            Vec::new(),
        );
        let app = build_router(state, None);

        let response = app
            .oneshot(authed_request("GET", "/v1/thinking"))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["available"], serde_json::json!(["off"]));
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
        let app = make_test_app(None);
        let shared = Arc::new(SharedReadState::from_app(&app));
        let state = HttpState {
            app: Arc::new(Mutex::new(app)),
            shared,
            config_manager: None,
            session_registry: None,
            start_time: Instant::now(),
            server_runtime: test_server_runtime(),
            tailscale_ip: None,
            bearer_token: TEST_TOKEN.to_string(),
            pairing: Arc::new(Mutex::new(PairingState::new())),
            devices: Arc::new(Mutex::new(DeviceStore::new())),
            devices_path: None,
            channels: ChannelRuntime {
                router: Arc::new(ResponseRouter::new(Arc::new(router_registry))),
                http: Arc::new(HttpChannel::new()),
                telegram: None,
                webhooks: Arc::new(webhooks),
            },
            data_dir,
            synthesis: synthesis_state(false),
            oauth_flows: Arc::new(crate::handlers::oauth::OAuthFlowStore::new()),
            permission_prompts: Arc::new(fx_kernel::PermissionPromptState::new()),
            ripcord: None,
            fleet_manager: None,
            cron_store: None,
            experiment_registry: {
                let registry = ExperimentRegistry::new(std::env::temp_dir().as_path()).unwrap();
                Arc::new(tokio::sync::Mutex::new(registry))
            },
            improvement_provider: None,
            telemetry: in_memory_telemetry(),
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

    #[tokio::test]
    async fn get_synthesis_returns_current_value_and_metadata() {
        let (_temp, config, manager) = temp_config_manager(
            "[model]\ndefault_model = \"mock-model\"\nsynthesis_instruction = \"Be concise\"\n",
        );
        let app = build_router(
            test_state_with_app(make_test_app_with_config(config, Some(manager)), Vec::new()),
            None,
        );

        let response = app
            .oneshot(authed_request("GET", "/v1/synthesis"))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["synthesis"], "Be concise");
        assert_eq!(json["source"], "config");
        assert_eq!(json["version"], 1);
        assert_eq!(
            json["max_length"],
            fx_config::MAX_SYNTHESIS_INSTRUCTION_LENGTH
        );
        assert!(json["updated_at"].is_number());
    }

    #[tokio::test]
    async fn get_synthesis_returns_null_when_unset() {
        let (_temp, config, manager) =
            temp_config_manager("[model]\ndefault_model = \"mock-model\"\n");
        let app = build_router(
            test_state_with_app(make_test_app_with_config(config, Some(manager)), Vec::new()),
            None,
        );

        let response = app
            .oneshot(authed_request("GET", "/v1/synthesis"))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert!(json["synthesis"].is_null());
        assert!(json["updated_at"].is_null());
        assert_eq!(json["version"], 0);
    }

    #[tokio::test]
    async fn put_synthesis_updates_config_and_bumps_version() {
        let (_temp, config, manager) = temp_config_manager(
            "[model]\ndefault_model = \"mock-model\"\nsynthesis_instruction = \"Be concise\"\n",
        );
        let router = build_router(
            test_state_with_app(
                make_test_app_with_config(config, Some(Arc::clone(&manager))),
                Vec::new(),
            ),
            None,
        );

        let response = router
            .clone()
            .oneshot(authed_json_request(
                "PUT",
                "/v1/synthesis",
                r#"{"synthesis":"Ask one clarifying question if needed.","version":1}"#,
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["updated"], true);
        assert_eq!(json["version"], 2);
        assert_eq!(json["synthesis"], "Ask one clarifying question if needed.");
        assert!(json["updated_at"].is_number());

        let synthesis = manager
            .lock()
            .expect("config manager")
            .config()
            .model
            .synthesis_instruction
            .clone();
        assert_eq!(
            synthesis,
            Some("Ask one clarifying question if needed.".to_string())
        );

        let get_response = router
            .oneshot(authed_request("GET", "/v1/synthesis"))
            .await
            .expect("response");
        let get_json = response_json(get_response).await;
        assert_eq!(get_json["version"], 2);
        assert_eq!(
            get_json["synthesis"],
            "Ask one clarifying question if needed."
        );
    }

    #[tokio::test]
    async fn put_synthesis_validates_length() {
        let (_temp, config, manager) = temp_config_manager(
            "[model]\ndefault_model = \"mock-model\"\nsynthesis_instruction = \"Be concise\"\n",
        );
        let app = build_router(
            test_state_with_app(make_test_app_with_config(config, Some(manager)), Vec::new()),
            None,
        );
        let synthesis = "a".repeat(fx_config::MAX_SYNTHESIS_INSTRUCTION_LENGTH + 1);
        let body = serde_json::json!({ "synthesis": synthesis }).to_string();

        let response = app
            .oneshot(authed_json_request("PUT", "/v1/synthesis", &body))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let json = response_json(response).await;
        assert_eq!(
            json["error"],
            format!(
                "synthesis_instruction exceeds {} characters",
                fx_config::MAX_SYNTHESIS_INSTRUCTION_LENGTH
            )
        );
    }

    #[tokio::test]
    async fn put_synthesis_without_version_succeeds_when_server_version_is_nonzero() {
        let (_temp, config, manager) = temp_config_manager(
            "[model]\ndefault_model = \"mock-model\"\nsynthesis_instruction = \"Be concise\"\n",
        );
        let router = build_router(
            test_state_with_app(
                make_test_app_with_config(config, Some(Arc::clone(&manager))),
                Vec::new(),
            ),
            None,
        );

        let response = router
            .clone()
            .oneshot(authed_json_request(
                "PUT",
                "/v1/synthesis",
                r#"{"synthesis":"New value without version"}"#,
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["version"], 2);
        assert_eq!(json["synthesis"], "New value without version");

        let synthesis = manager
            .lock()
            .expect("config manager")
            .config()
            .model
            .synthesis_instruction
            .clone();
        assert_eq!(synthesis, Some("New value without version".to_string()));
    }

    #[tokio::test]
    async fn put_synthesis_rejects_stale_version() {
        let (_temp, config, manager) = temp_config_manager(
            "[model]\ndefault_model = \"mock-model\"\nsynthesis_instruction = \"Be concise\"\n",
        );
        let app = build_router(
            test_state_with_app(make_test_app_with_config(config, Some(manager)), Vec::new()),
            None,
        );

        let response = app
            .oneshot(authed_json_request(
                "PUT",
                "/v1/synthesis",
                r#"{"synthesis":"New value","version":99}"#,
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::CONFLICT);
        let json = response_json(response).await;
        assert_eq!(json["error"], "Version mismatch: expected 1, got 99");
    }

    #[tokio::test]
    async fn put_synthesis_rejects_invalid_value() {
        let (_temp, config, manager) = temp_config_manager(
            "[model]\ndefault_model = \"mock-model\"\nsynthesis_instruction = \"Be concise\"\n",
        );
        let app = build_router(
            test_state_with_app(make_test_app_with_config(config, Some(manager)), Vec::new()),
            None,
        );

        let response = app
            .oneshot(authed_json_request(
                "PUT",
                "/v1/synthesis",
                r#"{"synthesis":"   "}"#,
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let json = response_json(response).await;
        assert_eq!(json["error"], "synthesis_instruction must not be empty");
    }

    #[tokio::test]
    async fn delete_synthesis_clears_config_and_bumps_version() {
        let (_temp, config, manager) = temp_config_manager(
            "[model]\ndefault_model = \"mock-model\"\nsynthesis_instruction = \"Be concise\"\n",
        );
        let router = build_router(
            test_state_with_app(
                make_test_app_with_config(config, Some(Arc::clone(&manager))),
                Vec::new(),
            ),
            None,
        );

        let response = router
            .clone()
            .oneshot(authed_request("DELETE", "/v1/synthesis"))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["cleared"], true);
        assert_eq!(json["version"], 2);
        let synthesis = manager
            .lock()
            .expect("config manager")
            .config()
            .model
            .synthesis_instruction
            .clone();
        assert_eq!(synthesis, None);

        let get_response = router
            .oneshot(authed_request("GET", "/v1/synthesis"))
            .await
            .expect("response");
        let get_json = response_json(get_response).await;
        assert!(get_json["synthesis"].is_null());
        assert!(get_json["updated_at"].is_null());
        assert_eq!(get_json["version"], 2);
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
            router: Arc::new(std::sync::RwLock::new(router)),
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
            cron_store: None,
            startup_warnings: Vec::new(),
            stream_callback_slot: Arc::new(std::sync::Mutex::new(None)),
            ripcord_journal: Arc::new(fx_ripcord::RipcordJournal::new(
                std::env::temp_dir().as_path(),
            )),
            experiment_registry: None,
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
            Vec::new(),
            InputSource::Channel("telegram".to_string()),
            ResponseContext {
                routing_key: Some("12345".to_string()),
                reply_to: None,
            },
        )
        .await
        .expect("process with images");

        assert_eq!(result.response, "Mock response");
        assert_eq!(result.result_kind, ResultKind::Complete);
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
