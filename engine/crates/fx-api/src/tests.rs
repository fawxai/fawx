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
use crate::sse::{
    send_sse_frame, serialize_stream_event, SseEventKind, SseFrame, SseStreamContext,
    SseStreamState,
};
use crate::state::{
    build_channel_runtime, in_memory_telemetry, ChannelRuntime, HttpState, SessionRunCancelReason,
    SessionRunRegistry, SharedReadState, StopSessionRunOutcome,
};
use crate::test_support::StubAppEngine;
use crate::token::{validate_bearer_token, BearerTokenStore};
use crate::types::{
    ApiKeyRequest, AuthProviderDto, ContextInfoDto, ContextInfoSnapshotLike, CreateThreadRequest,
    CreateWorktreeRequest, DocumentPayload, ErrorBody, ErrorRecordDto, HealthResponse,
    MessageRequest, MessageResponse, ModelInfoDto, ModelSwitchDto, SetupTokenRequest,
    SkillSummaryDto, StatusResponse, ThinkingLevelDto, WorkspaceScope,
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
use fx_config::{HttpConfig, MAX_CUSTOM_INSTRUCTION_LENGTH};
use fx_core::channel::{Channel, ResponseContext};
use fx_core::runtime_info::{ConfigSummary, RuntimeInfo};
use fx_core::signals::{LoopStep, Signal, SignalKind};
use fx_core::types::InputSource;
use fx_fleet::FleetManager;
use fx_kernel::{
    ChannelRegistry, HttpChannel, PermissionPromptState, ResponseRouter, StreamCallback,
    StreamEvent,
};
use fx_llm::{
    CompletionResponse, CompletionStream, ContentBlock, DocumentAttachment, ImageAttachment,
    Message, StreamChunk,
};
use fx_memory::SignalStore;
use fx_telemetry::{SignalCategory, SignalCollector, TelemetryConsent};
use http_body_util::BodyExt;
use hyper::Request;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, Mutex};
use tower::ServiceExt;

const TEST_TOKEN: &str = "test-secret-token-abc123";

#[test]
fn app_permission_prompts_reuses_app_owned_prompt_state() {
    let prompt_state = Arc::new(PermissionPromptState::new());
    let app = StubAppEngine::default().with_permission_prompt_state(Arc::clone(&prompt_state));

    let resolved = crate::app_permission_prompts(&app);

    assert!(Arc::ptr_eq(&resolved, &prompt_state));
}

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

    async fn available_models_dynamic(&mut self) -> Vec<ModelInfoDto> {
        HeadlessApp::available_models_dynamic(self)
            .await
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

    fn replace_active_model(&mut self, selector: &str) -> Result<Option<String>, anyhow::Error> {
        HeadlessApp::replace_active_model_for_turn(self, selector).map(Some)
    }

    fn apply_turn_thinking_level(&mut self, level: Option<&str>) -> Result<(), anyhow::Error> {
        HeadlessApp::apply_turn_thinking_level(self, level)
    }

    fn thinking_levels_for_model(&self, model: &str) -> Vec<String> {
        HeadlessApp::thinking_available_levels_for_model(self, model)
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
            .map(|summary| SkillSummaryDto {
                name: summary.name,
                description: summary.description,
                tools: summary.tools,
                capabilities: summary.capabilities,
                version: summary.version,
                source: summary.source,
                revision_hash: summary.revision_hash,
                activated_at_ms: summary.activated_at_ms,
                signature_status: summary.signature_status,
                stale_source: summary.stale_source,
            })
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

    fn reload_config(&mut self) -> Result<(), anyhow::Error> {
        HeadlessApp::reload_runtime_config_from_disk(self)
    }

    fn session_bus(&self) -> Option<&SessionBus> {
        HeadlessApp::session_bus(self)
    }

    fn max_history(&self) -> usize {
        self.config().general.max_history
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
            tool_invocations_remaining: 0,
        },
        authority: None,
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
    let line = "4: tailscale0    inet 100.100.100.2/32 scope global tailscale0";
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
fn message_request_deserializes_turn_steering() {
    let json = r#"{"message": "hello", "steering": "keep it terse"}"#;
    let req: MessageRequest = serde_json::from_str(json).expect("valid json");
    assert_eq!(req.steering.as_deref(), Some("keep it terse"));
}

#[test]
fn message_request_rejects_missing_message() {
    let json = r#"{}"#;
    let result = serde_json::from_str::<MessageRequest>(json);
    assert!(result.is_err());
}

#[test]
fn workspace_scope_roundtrips_as_string() {
    let scope = WorkspaceScope::explicit("/tmp/repo");

    assert_eq!(scope.requested_path(), Some("/tmp/repo"));
    assert_eq!(
        serde_json::to_value(&scope).expect("serialize workspace scope"),
        serde_json::json!("/tmp/repo")
    );
}

#[test]
fn create_thread_request_uses_workspace_path_wire_key() {
    let request = CreateThreadRequest {
        workspace_id: "ws-repo".to_string(),
        title: Some("Thread title".to_string()),
        model: Some("gpt-5.4".to_string()),
        thinking: Some("high".to_string()),
        workspace_scope: WorkspaceScope::explicit("/tmp/repo"),
        worktree_id: Some("wt-1".to_string()),
    };

    let json = serde_json::to_value(&request).expect("serialize thread request");

    assert_eq!(json["workspace_id"], "ws-repo");
    assert_eq!(json["workspace_path"], "/tmp/repo");
    assert!(json.get("workspace_scope").is_none());
}

#[test]
fn create_worktree_request_omits_workspace_path_when_scope_is_default() {
    let request = CreateWorktreeRequest {
        workspace_id: "ws-repo".to_string(),
        branch: "feature/thread-state".to_string(),
        workspace_scope: WorkspaceScope::default(),
        base_ref: Some("origin/main".to_string()),
    };

    let json = serde_json::to_value(&request).expect("serialize worktree request");

    assert_eq!(json["workspace_id"], "ws-repo");
    assert_eq!(json["branch"], "feature/thread-state");
    assert!(json.get("workspace_path").is_none());
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
fn serialize_stream_event_serializes_transcript_phase_boundary() {
    let frame = serialize_stream_event(StreamEvent::TranscriptPhaseBoundary {
        phase: fx_kernel::TranscriptTurnPhase::Finalizing,
    })
    .expect("phase boundary frame");

    assert_eq!(
        frame,
        "event: phase_boundary\ndata: {\"phase\":\"finalizing\"}\n\n"
    );
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
fn serialize_stream_event_serializes_tool_result_payload() {
    let frame = serialize_stream_event(StreamEvent::ToolResult {
        id: "call-1".to_string(),
        tool_name: "read_file".to_string(),
        output: "file contents".to_string(),
        is_error: false,
    })
    .expect("tool result frame");

    assert!(frame.contains("event: tool_result"));
    assert!(frame.contains("\"id\":\"call-1\""));
    assert!(frame.contains("\"tool_name\":\"read_file\""));
    assert!(frame.contains("\"output\":\"file contents\""));
    assert!(frame.contains("\"is_error\":false"));
}

#[test]
fn serialize_stream_event_serializes_tool_progress_payload() {
    let frame = serialize_stream_event(StreamEvent::ToolProgress {
        activity_id: Some("tool-round-call-1".to_string()),
        id: "call-1".to_string(),
        tool_name: "read_file".to_string(),
        class: fx_kernel::StreamToolProgressClass::Observation,
        target: Some("PR 1834".to_string()),
        advances_slot: Some("evidence:required:pr:1834".to_string()),
        outcome: fx_kernel::StreamToolProgressOutcome::Advanced,
    })
    .expect("tool progress frame");

    assert!(frame.contains("event: tool_progress"));
    assert!(frame.contains("\"activity_id\":\"tool-round-call-1\""));
    assert!(frame.contains("\"id\":\"call-1\""));
    assert!(frame.contains("\"tool_name\":\"read_file\""));
    assert!(frame.contains("\"class\":\"observation\""));
    assert!(frame.contains("\"target\":\"PR 1834\""));
    assert!(frame.contains("\"advances_slot\":\"evidence:required:pr:1834\""));
    assert!(frame.contains("\"outcome\":\"advanced\""));
}

#[test]
fn serialize_stream_event_serializes_completed_summary_payload() {
    let frame = serialize_stream_event(StreamEvent::CompletedSummary {
        text: "Worked this turn: 2 searches.".to_string(),
    })
    .expect("completed summary frame");

    assert_eq!(
        frame,
        "event: completed_summary\ndata: {\"text\":\"Worked this turn: 2 searches.\"}\n\n"
    );
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
    let stream_state = SseStreamState::shared();
    let context = SseStreamContext::new("test");
    drop(receiver);

    assert!(!send_sse_frame(
        &sender,
        &stream_state,
        &context,
        SseFrame::new(SseEventKind::Done, "frame".to_string())
    ));
    assert!(stream_state.is_disconnected());
}

#[test]
fn send_sse_frame_drops_coalescible_when_channel_is_full() {
    let (sender, mut receiver) = mpsc::channel(1);
    let stream_state = SseStreamState::shared();
    let context = SseStreamContext::new("test");

    assert!(send_sse_frame(
        &sender,
        &stream_state,
        &context,
        SseFrame::new(SseEventKind::TextPreviewDelta, "first".to_string())
    ));
    assert!(send_sse_frame(
        &sender,
        &stream_state,
        &context,
        SseFrame::new(SseEventKind::TextPreviewDelta, "second".to_string())
    ));
    assert!(!stream_state.is_disconnected());
    assert_eq!(
        receiver.try_recv().expect("queued frame").into_body(),
        "first"
    );
    assert_eq!(stream_state.lifetime_dropped_coalescible(), 1);
}

#[test]
fn send_sse_frame_buffers_required_frame_when_channel_is_full() {
    let (sender, mut receiver) = mpsc::channel(1);
    let stream_state = SseStreamState::shared();
    let context = SseStreamContext::new("test");

    assert!(send_sse_frame(
        &sender,
        &stream_state,
        &context,
        SseFrame::new(SseEventKind::TextPreviewDelta, "first".to_string())
    ));
    assert!(send_sse_frame(
        &sender,
        &stream_state,
        &context,
        SseFrame::new(SseEventKind::Done, "second".to_string())
    ));
    assert!(!stream_state.is_disconnected());
    assert_eq!(
        receiver.try_recv().expect("queued frame").into_body(),
        "first"
    );
    assert_eq!(stream_state.lifetime_upstream_full(), 1);
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
    use crate::handlers::workspace_catalog::GENERAL_WORKSPACE_ID;
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
        MessageRole as SessionMessageRole, Session, SessionConfig, SessionContentBlock,
        SessionError, SessionKey, SessionKind, SessionMemory, SessionMessage, SessionRegistry,
        SessionStatus, SessionStore, SessionThreadBinding,
    };
    use fx_subagent::{
        test_support::DisabledSubagentFactory, SubagentLimits, SubagentManager, SubagentManagerDeps,
    };
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex as StdMutex,
    };
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
                prompt_cache: Default::default(),
                prompt_cache_affinity: Default::default(),
            }
        }
    }

    struct StaticProvider {
        name: &'static str,
        models: Vec<&'static str>,
    }

    fn static_provider_thinking_levels(name: &str, model: &str) -> &'static [&'static str] {
        match (name, model) {
            ("anthropic", "claude-opus-4-6") => {
                &["off", "adaptive", "low", "medium", "high", "max"]
            }
            ("anthropic", "claude-sonnet-4-20250514") => {
                &["off", "adaptive", "low", "medium", "high"]
            }
            ("anthropic", "claude-sonnet-4-6") => &["off", "adaptive", "low", "medium", "high"],
            ("openai", "gpt-5.4") => &["none", "low", "medium", "high", "xhigh"],
            _ => &["off"],
        }
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
                prompt_cache: Default::default(),
                prompt_cache_affinity: Default::default(),
            }
        }

        fn thinking_levels(&self, model: &str) -> &'static [&'static str] {
            static_provider_thinking_levels(self.name, model)
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
            execution_root: Arc::new(fx_kernel::ExecutionRoot::new(std::env::temp_dir())),
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
            permission_prompt_state: None,
            ripcord_journal: Arc::new(fx_ripcord::RipcordJournal::new(
                std::env::temp_dir().as_path(),
            )),
            improvement_provider: None,
            credential_store: None,
            token_broker: None,
            experiment_registry: None,
        })
        .expect("test app")
    }

    #[derive(Debug, Default)]
    struct SessionMemoryPersistingState {
        current_model: String,
        processed_models: Vec<String>,
        current_memory: SessionMemory,
        current_execution_root: Option<PathBuf>,
        loaded_memories: Vec<SessionMemory>,
        loaded_execution_roots: Vec<PathBuf>,
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
                    current_model: "mock-model".to_string(),
                    processed_models: Vec::new(),
                    current_memory: initial_memory,
                    current_execution_root: None,
                    loaded_memories: Vec::new(),
                    loaded_execution_roots: Vec::new(),
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
            let model;
            {
                let mut state = self.state.lock().expect("state lock");
                model = state.current_model.clone();
                state.processed_models.push(model.clone());
                let loaded_memory = state.current_memory.clone();
                state.loaded_memories.push(loaded_memory);
                if let Some(root) = state.current_execution_root.clone() {
                    state.loaded_execution_roots.push(root);
                }
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
                callback(StreamEvent::FinalAnswerDelta {
                    text: response.clone(),
                });
                callback(StreamEvent::Done {
                    response: response.clone(),
                });
            }
            Ok((
                ApiCycleResult {
                    response,
                    model,
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
                display_name: None,
                recommended: true,
                thinking_levels: vec!["off".to_string()],
            }]
        }

        fn set_active_model(&mut self, selector: &str) -> Result<ModelSwitchDto, anyhow::Error> {
            Ok(ModelSwitchDto {
                previous_model: "mock-model".to_string(),
                active_model: selector.to_string(),
                thinking_adjusted: None,
            })
        }

        fn replace_active_model(
            &mut self,
            selector: &str,
        ) -> Result<Option<String>, anyhow::Error> {
            let mut state = self.state.lock().expect("state lock");
            let previous = std::mem::replace(&mut state.current_model, selector.to_string());
            Ok(Some(previous))
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

        fn replace_execution_root(&mut self, root: PathBuf) -> Option<PathBuf> {
            let mut state = self.state.lock().expect("state lock");
            state.current_execution_root.replace(root)
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

    #[derive(Clone)]
    struct BlockingSessionTurnControl {
        started_calls: Arc<AtomicUsize>,
        release_first_turn: Arc<tokio::sync::Notify>,
        first_turn_started: Arc<tokio::sync::Notify>,
        processed_inputs: Arc<StdMutex<Vec<String>>>,
    }

    impl BlockingSessionTurnControl {
        fn new() -> Self {
            Self {
                started_calls: Arc::new(AtomicUsize::new(0)),
                release_first_turn: Arc::new(tokio::sync::Notify::new()),
                first_turn_started: Arc::new(tokio::sync::Notify::new()),
                processed_inputs: Arc::new(StdMutex::new(Vec::new())),
            }
        }

        async fn wait_for_first_turn(&self) {
            if self.started_calls.load(Ordering::Acquire) > 0 {
                return;
            }
            self.first_turn_started.notified().await;
        }

        fn release_first_turn(&self) {
            self.release_first_turn.notify_one();
        }

        fn processed_inputs(&self) -> Vec<String> {
            self.processed_inputs
                .lock()
                .expect("processed inputs lock")
                .clone()
        }
    }

    struct BlockingSessionTurnTestApp {
        control: BlockingSessionTurnControl,
        current_model: String,
        current_memory: SessionMemory,
        last_session_messages: Vec<SessionMessage>,
    }

    impl BlockingSessionTurnTestApp {
        fn new(control: BlockingSessionTurnControl) -> Self {
            Self {
                control,
                current_model: "mock-model".to_string(),
                current_memory: SessionMemory::default(),
                last_session_messages: Vec::new(),
            }
        }
    }

    #[async_trait]
    impl AppEngine for BlockingSessionTurnTestApp {
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
            self.control
                .processed_inputs
                .lock()
                .expect("processed inputs lock")
                .push(input.to_string());
            self.control.started_calls.fetch_add(1, Ordering::AcqRel);
            if input == "first" {
                self.control.first_turn_started.notify_waiters();
                self.control.release_first_turn.notified().await;
            }

            let response = format!("response for {input}");
            self.last_session_messages = vec![
                SessionMessage::text(SessionMessageRole::User, input, 2),
                SessionMessage::text(SessionMessageRole::Assistant, &response, 3),
            ];
            if let Some(callback) = callback {
                callback(StreamEvent::FinalAnswerDelta {
                    text: response.clone(),
                });
                callback(StreamEvent::Done {
                    response: response.clone(),
                });
            }

            Ok((
                ApiCycleResult {
                    response,
                    model: self.current_model.clone(),
                    iterations: 1,
                    result_kind: ResultKind::Complete,
                },
                context,
            ))
        }

        fn active_model(&self) -> &str {
            &self.current_model
        }

        fn available_models(&self) -> Vec<ModelInfoDto> {
            vec![ModelInfoDto {
                model_id: "mock-model".to_string(),
                provider: "test".to_string(),
                auth_method: "none".to_string(),
                display_name: None,
                recommended: true,
                thinking_levels: vec!["off".to_string()],
            }]
        }

        fn set_active_model(&mut self, selector: &str) -> Result<ModelSwitchDto, anyhow::Error> {
            let previous_model = std::mem::replace(&mut self.current_model, selector.to_string());
            Ok(ModelSwitchDto {
                previous_model,
                active_model: self.current_model.clone(),
                thinking_adjusted: None,
            })
        }

        fn replace_active_model(
            &mut self,
            selector: &str,
        ) -> Result<Option<String>, anyhow::Error> {
            Ok(Some(std::mem::replace(
                &mut self.current_model,
                selector.to_string(),
            )))
        }

        fn spawn_session_engine(
            &self,
            _session_key: &SessionKey,
            _execution_root: PathBuf,
        ) -> Result<Option<Box<dyn AppEngine>>, anyhow::Error> {
            Ok(Some(Box::new(Self::new(self.control.clone()))))
        }

        fn thinking_level(&self) -> ThinkingLevelDto {
            ThinkingLevelDto {
                level: "off".to_string(),
                budget_tokens: None,
                available: vec!["off".to_string()],
            }
        }

        fn context_info(&self) -> ContextInfoDto {
            ContextInfoDto {
                used_tokens: 0,
                max_tokens: 4_096,
                percentage: 0.0,
                compaction_threshold: 0.8,
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
            std::mem::replace(&mut self.current_memory, memory)
        }

        fn session_memory(&self) -> SessionMemory {
            self.current_memory.clone()
        }

        fn take_last_session_messages(&mut self) -> Vec<SessionMessage> {
            std::mem::take(&mut self.last_session_messages)
        }
    }

    fn blocking_session_turn_router(
        registry: SessionRegistry,
    ) -> (Router, BlockingSessionTurnControl) {
        let control = BlockingSessionTurnControl::new();
        let app = BlockingSessionTurnTestApp::new(control.clone());
        let mut state = test_state_with_sessions(registry);
        state.shared = Arc::new(SharedReadState::from_app(&app));
        state.app = Arc::new(Mutex::new(app));
        (build_router(state, None), control)
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
                    routing_tools: Vec::new(),
                    capabilities: Vec::new(),
                    version: None,
                    source: None,
                    revision_hash: None,
                    manifest_hash: None,
                    activated_at_ms: None,
                    signature_status: None,
                    stale_source: None,
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
            session_runs: crate::state::SessionRunRegistry::default(),
            session_engines: crate::state::SessionEnginePool::default(),
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
            credential_store: None,
            experiment_registry: {
                let registry = ExperimentRegistry::new(std::env::temp_dir().as_path()).unwrap();
                Arc::new(tokio::sync::Mutex::new(registry))
            },
            improvement_provider: None,
            telemetry: in_memory_telemetry(),
        }
    }

    fn test_state_with_engine(app: impl AppEngine + 'static) -> HttpState {
        let data_dir = std::env::temp_dir().join(format!(
            "fawx-api-engine-tests-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::create_dir_all(&data_dir).expect("create temp data dir");
        let shared = Arc::new(SharedReadState::from_app(&app));

        HttpState {
            app: Arc::new(Mutex::new(app)),
            shared,
            config_manager: None,
            session_registry: None,
            session_runs: crate::state::SessionRunRegistry::default(),
            session_engines: crate::state::SessionEnginePool::default(),
            start_time: Instant::now(),
            server_runtime: test_server_runtime(),
            tailscale_ip: None,
            bearer_token: TEST_TOKEN.to_string(),
            pairing: Arc::new(Mutex::new(PairingState::new())),
            devices: Arc::new(Mutex::new(DeviceStore::new())),
            devices_path: None,
            channels: build_channel_runtime(None, Vec::new()),
            data_dir,
            synthesis: synthesis_state(false),
            oauth_flows: Arc::new(crate::handlers::oauth::OAuthFlowStore::new()),
            permission_prompts: Arc::new(fx_kernel::PermissionPromptState::new()),
            ripcord: None,
            fleet_manager: None,
            cron_store: None,
            credential_store: None,
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
            session_runs: crate::state::SessionRunRegistry::default(),
            session_engines: crate::state::SessionEnginePool::default(),
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
            credential_store: None,
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

    fn authed_sse_json_request(method: &str, uri: &str, body: &str) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header("authorization", format!("Bearer {TEST_TOKEN}"))
            .header("accept", "text/event-stream")
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

    async fn response_text(response: axum::response::Response) -> String {
        let body = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        String::from_utf8(body.to_vec()).expect("utf8 body")
    }

    fn listed_session_keys(json: &serde_json::Value) -> Vec<String> {
        json["sessions"]
            .as_array()
            .expect("sessions array")
            .iter()
            .map(|session| session["key"].as_str().expect("session key").to_string())
            .collect()
    }

    fn listed_session<'a>(json: &'a serde_json::Value, key: &SessionKey) -> &'a serde_json::Value {
        json["sessions"]
            .as_array()
            .expect("sessions array")
            .iter()
            .find(|session| session["key"] == key.as_str())
            .expect("session entry")
    }

    fn assert_archive_metadata(json: &serde_json::Value, archived: bool) {
        assert_eq!(json["archived"], archived);
        if archived {
            assert!(json["archived_at"].as_u64().is_some());
        } else {
            assert!(json["archived_at"].is_null());
        }
    }

    fn exported_message_texts(json: &serde_json::Value) -> Vec<String> {
        json["messages"]
            .as_array()
            .expect("messages array")
            .iter()
            .map(|message| {
                message["content"][0]["text"]
                    .as_str()
                    .expect("message text")
                    .to_string()
            })
            .collect()
    }

    async fn expect_ok_json(app: Router, method: &str, uri: &str) -> serde_json::Value {
        let response = app
            .oneshot(authed_request(method, uri))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        response_json(response).await
    }

    async fn assert_list_membership(app: Router, uri: &str, key: &SessionKey, present: bool) {
        let json = expect_ok_json(app, "GET", uri).await;
        let keys = listed_session_keys(&json);
        assert_eq!(keys.contains(&key.to_string()), present);
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
        let (_, first) = devices.create_device("Alice's MacBook");
        let (_, second) = devices.create_device("Alice's iPhone");
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
        let _ = devices.create_device("Alice's MacBook");
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
        let (raw_token, device) = devices.create_device("Alice's MacBook");
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
    async fn config_patch_endpoint_refreshes_shared_max_history_snapshot() {
        let (_temp, config, manager) = temp_config_manager(
            "[model]\ndefault_model = \"mock-model\"\n\n[general]\nmax_history = 3\n",
        );
        let state = test_state_with_config(config, Some(manager), Vec::new());
        let shared = Arc::clone(&state.shared);
        let app = build_router(state, None);

        let response = app
            .oneshot(authed_json_request(
                "PATCH",
                "/v1/config",
                r#"{"changes":{"general":{"max_history":9}}}"#,
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(shared.read().await.max_history, 9);
    }

    #[tokio::test]
    async fn config_patch_endpoint_rejects_invalid_custom_instructions_without_writing() {
        let (temp, config, manager) = temp_config_manager(
            "[agent.behavior]\ncustom_instructions = \"Keep changes focused.\"\n",
        );
        let app = build_router(
            test_state_with_config(config, Some(manager), Vec::new()),
            None,
        );
        let oversized = "x".repeat(MAX_CUSTOM_INSTRUCTION_LENGTH + 1);
        let body = serde_json::json!({
            "changes": {
                "agent": {
                    "behavior": {
                        "custom_instructions": oversized
                    }
                }
            }
        })
        .to_string();

        let response = app
            .oneshot(authed_json_request("PATCH", "/v1/config", &body))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let json = response_json(response).await;
        assert!(json["error"]
            .as_str()
            .is_some_and(|error| error.contains("agent.behavior.custom_instructions")));

        let content =
            std::fs::read_to_string(temp.path().join("config.toml")).expect("read config");
        assert!(content.contains("Keep changes focused."));
        assert!(!content.contains(&"x".repeat(MAX_CUSTOM_INSTRUCTION_LENGTH + 1)));
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

    fn make_poisoned_session_registry(key: &str) -> SessionRegistry {
        let storage = fx_storage::Storage::open_in_memory().expect("in-memory storage");
        let store = SessionStore::new(storage.clone());
        store
            .save(&poisoned_session(key))
            .expect("save poisoned session");
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
                    thinking: None,
                },
            )
            .expect("create session");
        registry
            .set_status(&key, SessionStatus::Idle)
            .expect("set idle");
        key
    }

    fn seed_archived_session(registry: &SessionRegistry, key: &str) -> SessionKey {
        let key = seed_session(registry, key);
        registry.archive(&key).expect("archive session");
        key
    }

    #[tokio::test]
    async fn stop_session_endpoint_cancels_active_session_run() {
        let registry = make_session_registry();
        let key = seed_session(&registry, "sess-stop");
        let state = test_state_with_sessions(registry);
        let run_permit = state.session_runs.begin(&key).await;
        let app = build_router(state, None);

        let json = expect_ok_json(app, "POST", "/v1/sessions/sess-stop/stop").await;

        assert_eq!(json["key"], "sess-stop");
        assert_eq!(json["stopped"], true);
        assert!(run_permit.is_cancelled());
        assert_eq!(
            run_permit.cancel_reason(),
            Some(SessionRunCancelReason::StoppedByUser)
        );
    }

    #[tokio::test]
    async fn stop_session_endpoint_is_idempotent_when_no_run_is_active() {
        let registry = make_session_registry();
        seed_session(&registry, "sess-idle-stop");
        let app = build_router(test_state_with_sessions(registry), None);

        let json = expect_ok_json(app, "POST", "/v1/sessions/sess-idle-stop/stop").await;

        assert_eq!(json["key"], "sess-idle-stop");
        assert_eq!(json["stopped"], false);
    }

    #[tokio::test]
    async fn repeated_stop_on_same_active_run_reports_no_new_stop() {
        let runs = SessionRunRegistry::default();
        let key = SessionKey::new("sess-stop-twice").expect("session key");
        let permit = runs.begin(&key).await;

        assert_eq!(runs.stop(&key).await, StopSessionRunOutcome::Stopped);
        assert_eq!(runs.stop(&key).await, StopSessionRunOutcome::NoActiveRun);
        assert!(permit.is_cancelled());
        assert_eq!(
            permit.cancel_reason(),
            Some(SessionRunCancelReason::StoppedByUser)
        );
    }

    #[tokio::test]
    async fn session_run_registry_steer_delivers_to_active_run() {
        let runs = SessionRunRegistry::default();
        let key = SessionKey::new("sess-steer").expect("session key");
        let mut permit = runs.begin(&key).await;
        let mut input_channel = permit.take_input_channel().expect("input channel");

        assert_eq!(
            runs.steer(&key, "keep checking the reducer".to_string())
                .await,
            crate::state::SteerSessionRunOutcome::Steered
        );

        assert_eq!(
            input_channel.try_recv(),
            Some(fx_kernel::LoopCommand::Steer(
                "keep checking the reducer".to_string()
            ))
        );
    }

    #[tokio::test]
    async fn steer_session_endpoint_delivers_to_active_run() {
        let registry = make_session_registry();
        let key = seed_session(&registry, "sess-active-steer");
        let state = test_state_with_sessions(registry);
        let mut run_permit = state.session_runs.begin(&key).await;
        let mut input_channel = run_permit.take_input_channel().expect("input channel");
        let app = build_router(state, None);

        let response = app
            .oneshot(authed_json_request(
                "POST",
                "/v1/sessions/sess-active-steer/steer",
                r#"{"text": "keep checking the reducer"}"#,
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["key"], "sess-active-steer");
        assert_eq!(json["steered"], true);
        assert!(json.get("reason").is_none());
        assert_eq!(
            input_channel.try_recv(),
            Some(fx_kernel::LoopCommand::Steer(
                "keep checking the reducer".to_string()
            ))
        );
    }

    #[tokio::test]
    async fn steer_session_endpoint_rejects_empty_text() {
        let registry = make_session_registry();
        seed_session(&registry, "sess-empty-steer");
        let app = build_router(test_state_with_sessions(registry), None);

        let response = app
            .oneshot(authed_json_request(
                "POST",
                "/v1/sessions/sess-empty-steer/steer",
                r#"{"text": "   "}"#,
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let json = response_json(response).await;
        assert_eq!(json["error"], "steering text must not be empty");
    }

    #[tokio::test]
    async fn steer_session_endpoint_reports_no_active_run_when_idle() {
        let registry = make_session_registry();
        seed_session(&registry, "sess-idle-steer");
        let app = build_router(test_state_with_sessions(registry), None);

        let response = app
            .oneshot(authed_json_request(
                "POST",
                "/v1/sessions/sess-idle-steer/steer",
                r#"{"text": "focus on the UI"}"#,
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["key"], "sess-idle-steer");
        assert_eq!(json["steered"], false);
        assert_eq!(json["reason"], "no_active_run");
    }

    #[tokio::test]
    async fn overlapping_session_runs_cancel_previous_with_superseded_reason() {
        let runs = SessionRunRegistry::default();
        let key = SessionKey::new("sess-overlap").expect("session key");

        let first = runs.begin(&key).await;
        let second = runs.begin(&key).await;

        assert!(first.is_cancelled());
        assert_eq!(
            first.cancel_reason(),
            Some(SessionRunCancelReason::SupersededByNewerRequest)
        );
        assert!(!second.is_cancelled());
        assert_eq!(second.cancel_reason(), None);
    }

    #[tokio::test]
    async fn finishing_superseded_run_does_not_clear_replacement() {
        let runs = SessionRunRegistry::default();
        let key = SessionKey::new("sess-finish-race").expect("session key");

        let first = runs.begin(&key).await;
        let second = runs.begin(&key).await;
        runs.finish(&first).await;

        assert_eq!(runs.stop(&key).await, StopSessionRunOutcome::Stopped);
        assert!(second.is_cancelled());
        assert_eq!(
            second.cancel_reason(),
            Some(SessionRunCancelReason::StoppedByUser)
        );
    }

    fn record_session_messages(
        registry: &SessionRegistry,
        key: &SessionKey,
        messages: &[(SessionMessageRole, &str)],
    ) {
        for (role, content) in messages {
            registry
                .record_message(key, *role, content)
                .expect("record session message");
        }
    }

    fn failed_turn_terminal_signal(id: u64, timestamp_ms: u64) -> Signal {
        Signal::new(
            LoopStep::Synthesize,
            SignalKind::Trace,
            "loop turn terminal status",
            serde_json::json!({
                "decision_kind": "turn_stop",
                "decision": "failed",
                "failed": true,
                "result_kind": "incomplete",
                "stop_reason": "tool continuation did not produce a usable final response",
                "iterations": 2,
            }),
            timestamp_ms,
        )
        .with_id(id)
    }

    fn complete_turn_terminal_signal(id: u64, timestamp_ms: u64) -> Signal {
        Signal::new(
            LoopStep::Synthesize,
            SignalKind::Trace,
            "loop turn terminal status",
            serde_json::json!({
                "decision_kind": "turn_stop",
                "decision": "completed",
                "failed": false,
                "result_kind": "complete",
                "stop_reason": "complete",
                "iterations": 1,
            }),
            timestamp_ms,
        )
        .with_id(id)
    }

    fn persist_session_signals_for_test(data_dir: &Path, session_id: &str, signals: &[Signal]) {
        SignalStore::open(data_dir, session_id)
            .expect("open signal store")
            .persist(signals)
            .expect("persist signals");
    }

    fn seed_export_session(registry: &SessionRegistry, key: &str) -> SessionKey {
        let key = seed_session(registry, key);
        record_session_messages(
            registry,
            &key,
            &[
                (SessionMessageRole::User, "first message"),
                (SessionMessageRole::Assistant, "second message"),
                (SessionMessageRole::User, "third message"),
            ],
        );
        key
    }

    struct RepoWorkspaceFixture {
        _repo_temp: TempDir,
        _config_temp: TempDir,
        repo_root: PathBuf,
        linked_worktree: PathBuf,
        config: fx_config::FawxConfig,
        manager: Arc<StdMutex<ConfigManager>>,
    }

    struct GeneralWorkspaceFixture {
        _workspace_temp: TempDir,
        _config_temp: TempDir,
        config: fx_config::FawxConfig,
        manager: Arc<StdMutex<ConfigManager>>,
    }

    fn workspace_state(
        config: fx_config::FawxConfig,
        manager: Arc<StdMutex<ConfigManager>>,
        registry: Option<SessionRegistry>,
    ) -> HttpState {
        let mut state = test_state_with_config(config, Some(manager), Vec::new());
        state.session_registry = registry;
        state
    }

    fn general_workspace_fixture() -> GeneralWorkspaceFixture {
        let workspace_temp = TempDir::new().expect("tempdir");
        let workspace_root = workspace_temp.path().join("general");
        std::fs::create_dir_all(&workspace_root).expect("create general workspace root");
        let config_toml = format!(
            "[workspace]\nroot = \"{}\"\n\n[tools]\nworking_dir = \"{}\"\n",
            workspace_root.display(),
            workspace_root.display()
        );
        let (config_temp, config, manager) = temp_config_manager(&config_toml);
        GeneralWorkspaceFixture {
            _workspace_temp: workspace_temp,
            _config_temp: config_temp,
            config,
            manager,
        }
    }

    fn repo_workspace_fixture() -> RepoWorkspaceFixture {
        let repo_temp = TempDir::new().expect("tempdir");
        let repo_root = repo_temp.path().join("workspace");
        std::fs::create_dir_all(&repo_root).expect("create repo root");
        run_git(&repo_root, &["init"]);
        run_git(&repo_root, &["config", "user.name", "Fawx Tests"]);
        run_git(&repo_root, &["config", "user.email", "tests@example.com"]);
        std::fs::write(repo_root.join("README.md"), "# workspace\n").expect("write readme");
        run_git(&repo_root, &["add", "README.md"]);
        run_git(&repo_root, &["commit", "-m", "initial commit"]);
        run_git(&repo_root, &["branch", "-M", "main"]);

        let linked_worktree = repo_temp.path().join("workspace-feature");
        run_git(
            &repo_root,
            &[
                "worktree",
                "add",
                "-b",
                "feature/worktree",
                linked_worktree.to_str().expect("linked worktree path"),
            ],
        );
        let repo_root = std::fs::canonicalize(repo_root).expect("canonical repo root");
        let linked_worktree =
            std::fs::canonicalize(linked_worktree).expect("canonical linked worktree");

        let config_toml = format!(
            "[workspace]\nroot = \"{}\"\n\n[tools]\nworking_dir = \"{}\"\n",
            repo_root.display(),
            repo_root.display()
        );
        let (config_temp, config, manager) = temp_config_manager(&config_toml);
        RepoWorkspaceFixture {
            _repo_temp: repo_temp,
            _config_temp: config_temp,
            repo_root,
            linked_worktree,
            config,
            manager,
        }
    }

    fn run_git(path: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(path)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {:?} failed in {}:\nstdout: {}\nstderr: {}",
            args,
            path.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn listed_workspace_by_kind<'a>(
        json: &'a serde_json::Value,
        kind: &str,
    ) -> &'a serde_json::Value {
        json["workspaces"]
            .as_array()
            .expect("workspaces array")
            .iter()
            .find(|workspace| workspace["kind"] == kind)
            .expect("workspace entry")
    }

    fn listed_worktree_by_path<'a>(
        json: &'a serde_json::Value,
        path: &Path,
    ) -> &'a serde_json::Value {
        let path = path.to_string_lossy();
        json["worktrees"]
            .as_array()
            .expect("worktrees array")
            .iter()
            .find(|worktree| worktree["path"] == path.as_ref())
            .expect("worktree entry")
    }

    fn listed_thread_by_session_id<'a>(
        json: &'a serde_json::Value,
        session_id: &str,
    ) -> &'a serde_json::Value {
        json["threads"]
            .as_array()
            .expect("threads array")
            .iter()
            .find(|thread| thread["active_session_id"] == session_id)
            .expect("thread entry")
    }

    fn poisoned_session(key: &str) -> Session {
        Session {
            key: SessionKey::new(key).expect("session key"),
            kind: SessionKind::Main,
            status: SessionStatus::Idle,
            label: Some("poisoned".to_string()),
            model: "mock-model".to_string(),
            thinking: None,
            created_at: 1,
            updated_at: 2,
            archived_at: None,
            thread_binding: None,
            messages: vec![
                SessionMessage::structured(
                    SessionMessageRole::Tool,
                    vec![SessionContentBlock::ToolResult {
                        tool_use_id: "call_bad".to_string(),
                        content: serde_json::json!("bad"),
                        is_error: Some(false),
                    }],
                    1,
                    None,
                ),
                SessionMessage::structured(
                    SessionMessageRole::Assistant,
                    vec![SessionContentBlock::ToolUse {
                        id: "call_bad".to_string(),
                        provider_id: Some("fc_bad".to_string()),
                        name: "read_file".to_string(),
                        input: serde_json::json!({"path": "bad.txt"}),
                    }],
                    2,
                    None,
                ),
            ],
            memory: SessionMemory::default(),
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

    #[tokio::test]
    async fn workspaces_endpoint_returns_synthetic_general_workspace_for_unbound_threads() {
        let fixture = general_workspace_fixture();
        let registry = make_session_registry();
        let key = seed_session(&registry, "sess-general");

        let app = build_router(
            workspace_state(
                fixture.config.clone(),
                Arc::clone(&fixture.manager),
                Some(registry),
            ),
            None,
        );

        let workspaces = expect_ok_json(app.clone(), "GET", "/v1/workspaces").await;
        assert_eq!(workspaces["total"], 1);
        let general = listed_workspace_by_kind(&workspaces, "general");
        assert_eq!(general["id"], GENERAL_WORKSPACE_ID);
        assert_eq!(general["path"], "");
        assert!(general["repo"].is_null());

        let threads = expect_ok_json(
            app,
            "GET",
            &format!("/v1/workspaces/{GENERAL_WORKSPACE_ID}/threads"),
        )
        .await;
        assert_eq!(threads["total"], 1);
        let thread = &threads["threads"][0];
        assert_eq!(thread["active_session_id"], key.as_str());
        assert_eq!(thread["workspace_id"], GENERAL_WORKSPACE_ID);
        assert_eq!(thread["kind"], "general");
        assert!(thread["worktree_id"].is_null());
    }

    #[tokio::test]
    async fn workspaces_endpoint_returns_general_and_repo_backed_workspace_shapes() {
        let fixture = repo_workspace_fixture();
        let registry = make_session_registry();
        seed_session(&registry, "sess-repo");

        let app = build_router(
            workspace_state(
                fixture.config.clone(),
                Arc::clone(&fixture.manager),
                Some(registry),
            ),
            None,
        );

        let workspaces = expect_ok_json(app, "GET", "/v1/workspaces").await;
        assert_eq!(workspaces["total"], 2);

        let general = listed_workspace_by_kind(&workspaces, "general");
        assert_eq!(general["path"], "");
        assert!(general["repo"].is_null());

        let repository = listed_workspace_by_kind(&workspaces, "repository");
        assert_eq!(
            repository["path"],
            fixture.repo_root.to_string_lossy().as_ref()
        );
        assert_eq!(repository["repo"]["vcs"], "git");
        assert_eq!(
            repository["repo"]["root"],
            fixture.repo_root.to_string_lossy().as_ref()
        );
        assert_eq!(repository["repo"]["current_branch"], "main");
        assert_eq!(repository["repo"]["clean"], true);
        assert!(repository["repo"]["origin"].is_null());
        assert!(
            repository["last_opened_at"]
                .as_u64()
                .expect("repo last_opened_at")
                > 0
        );
    }

    #[tokio::test]
    async fn workspace_threads_endpoint_returns_thread_first_summaries_mapped_to_sessions() {
        let fixture = repo_workspace_fixture();
        let registry = make_session_registry();
        let key = seed_session(&registry, "sess-thread");
        record_session_messages(
            &registry,
            &key,
            &[
                (
                    SessionMessageRole::User,
                    "Implement the worktree read model",
                ),
                (
                    SessionMessageRole::Assistant,
                    "Backend read models are wired up",
                ),
            ],
        );

        let app = build_router(
            workspace_state(
                fixture.config.clone(),
                Arc::clone(&fixture.manager),
                Some(registry),
            ),
            None,
        );

        let workspaces = expect_ok_json(app.clone(), "GET", "/v1/workspaces").await;
        let repository = listed_workspace_by_kind(&workspaces, "repository");
        let repo_id = repository["id"].as_str().expect("repo id");

        let threads = expect_ok_json(
            app.clone(),
            "GET",
            &format!("/v1/workspaces/{repo_id}/threads"),
        )
        .await;
        assert_eq!(threads["total"], 1);
        let thread = &threads["threads"][0];
        assert_ne!(thread["id"], key.as_str());
        assert_eq!(thread["active_session_id"], key.as_str());
        assert_eq!(thread["workspace_id"], repo_id);
        assert_eq!(thread["kind"], "coding");
        assert_eq!(thread["title"], "Implement the worktree read model");
        assert_eq!(thread["preview"], "Backend read models are wired up");
        assert!(thread["worktree_id"].is_null());

        let sessions = expect_ok_json(app.clone(), "GET", "/v1/sessions").await;
        assert!(listed_session_keys(&sessions).contains(&key.to_string()));
        assert_eq!(
            listed_session(&sessions, &key)["preview"],
            thread["preview"]
        );

        let general_threads = expect_ok_json(
            app,
            "GET",
            &format!("/v1/workspaces/{GENERAL_WORKSPACE_ID}/threads"),
        )
        .await;
        assert_eq!(general_threads["total"], 0);
    }

    #[tokio::test]
    async fn workspace_worktrees_endpoint_returns_repo_backed_read_only_metadata() {
        let fixture = repo_workspace_fixture();
        let app = build_router(
            workspace_state(fixture.config.clone(), Arc::clone(&fixture.manager), None),
            None,
        );

        let workspaces = expect_ok_json(app.clone(), "GET", "/v1/workspaces").await;
        let repository = listed_workspace_by_kind(&workspaces, "repository");
        let repo_id = repository["id"].as_str().expect("repo id");

        let worktrees =
            expect_ok_json(app, "GET", &format!("/v1/workspaces/{repo_id}/worktrees")).await;
        assert_eq!(worktrees["total"], 2);

        let active = listed_worktree_by_path(&worktrees, &fixture.repo_root);
        assert_eq!(active["workspace_id"], repo_id);
        assert_eq!(active["status"], "active");
        assert_eq!(active["branch"], "main");
        assert_eq!(active["clean"], true);
        assert_eq!(active["ahead_count"], 0);
        assert_eq!(active["behind_count"], 0);

        let linked = listed_worktree_by_path(&worktrees, &fixture.linked_worktree);
        assert_eq!(linked["workspace_id"], repo_id);
        assert_eq!(linked["status"], "available");
        assert_eq!(linked["branch"], "feature/worktree");
        assert_eq!(linked["clean"], true);
        assert_eq!(linked["ahead_count"], 0);
        assert_eq!(linked["behind_count"], 0);
    }

    #[tokio::test]
    async fn workspace_endpoints_handle_detached_active_head() {
        let fixture = repo_workspace_fixture();
        run_git(&fixture.repo_root, &["checkout", "--detach"]);

        let app = build_router(
            workspace_state(fixture.config.clone(), Arc::clone(&fixture.manager), None),
            None,
        );

        let workspaces = expect_ok_json(app.clone(), "GET", "/v1/workspaces").await;
        let repository = listed_workspace_by_kind(&workspaces, "repository");
        let repo_id = repository["id"].as_str().expect("repo id");
        assert!(repository["repo"]["current_branch"]
            .as_str()
            .expect("current branch")
            .starts_with("detached@"));

        let worktrees =
            expect_ok_json(app, "GET", &format!("/v1/workspaces/{repo_id}/worktrees")).await;
        let active = listed_worktree_by_path(&worktrees, &fixture.repo_root);
        assert_eq!(active["workspace_id"], repo_id);
        assert_eq!(active["status"], "active");
        assert!(active["branch"]
            .as_str()
            .expect("detached branch")
            .starts_with("detached@"));
        assert!(active["base_ref"].is_null());
    }

    #[tokio::test]
    async fn workspace_threads_endpoint_returns_404_for_unknown_workspace() {
        let fixture = general_workspace_fixture();
        let app = build_router(
            workspace_state(fixture.config.clone(), Arc::clone(&fixture.manager), None),
            None,
        );

        let response = app
            .oneshot(authed_request(
                "GET",
                "/v1/workspaces/workspace-missing/threads",
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let json = response_json(response).await;

        assert_eq!(json["error"], "workspace not found: workspace-missing");
    }

    #[tokio::test]
    async fn workspace_worktrees_endpoint_returns_404_for_unknown_workspace() {
        let fixture = general_workspace_fixture();
        let app = build_router(
            workspace_state(fixture.config.clone(), Arc::clone(&fixture.manager), None),
            None,
        );

        let response = app
            .oneshot(authed_request(
                "GET",
                "/v1/workspaces/workspace-missing/worktrees",
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let json = response_json(response).await;

        assert_eq!(json["error"], "workspace not found: workspace-missing");
    }

    #[tokio::test]
    async fn open_workspace_endpoint_returns_repo_backed_summary_for_git_paths() {
        let fixture = repo_workspace_fixture();
        let app = build_router(
            workspace_state(fixture.config.clone(), Arc::clone(&fixture.manager), None),
            None,
        );

        let response = app
            .oneshot(authed_json_request(
                "POST",
                "/v1/workspaces/open",
                &format!(
                    "{{\"path\":\"{}\"}}",
                    fixture.linked_worktree.to_string_lossy()
                ),
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::CREATED);
        let json = response_json(response).await;

        assert_eq!(json["kind"], "repository");
        assert_eq!(json["path"], fixture.repo_root.to_string_lossy().as_ref());
        assert_eq!(json["repo"]["current_branch"], "feature/worktree");
    }

    #[tokio::test]
    async fn create_thread_route_binds_general_threads_even_when_repo_workspace_exists() {
        let fixture = repo_workspace_fixture();
        let registry = make_session_registry();
        let app = build_router(
            workspace_state(
                fixture.config.clone(),
                Arc::clone(&fixture.manager),
                Some(registry),
            ),
            None,
        );

        let response = app
            .clone()
            .oneshot(authed_json_request(
                "POST",
                "/v1/threads",
                &format!(
                    "{{\"workspace_id\":\"{}\",\"title\":\"General lane\"}}",
                    GENERAL_WORKSPACE_ID
                ),
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::CREATED);
        let created = response_json(response).await;
        let session_id = created["active_session_id"]
            .as_str()
            .expect("created session id")
            .to_string();

        let general_threads = expect_ok_json(
            app.clone(),
            "GET",
            &format!("/v1/workspaces/{GENERAL_WORKSPACE_ID}/threads"),
        )
        .await;
        let general_thread = listed_thread_by_session_id(&general_threads, &session_id);
        assert_eq!(general_thread["title"], "General lane");
        assert_eq!(general_thread["kind"], "general");

        let sessions = expect_ok_json(app, "GET", "/v1/sessions").await;
        assert_eq!(
            listed_session(
                &sessions,
                &SessionKey::new(&session_id).expect("created session key")
            )["thread_binding"]["workspace_id"],
            GENERAL_WORKSPACE_ID
        );
        assert_eq!(
            listed_session(
                &sessions,
                &SessionKey::new(&session_id).expect("created session key")
            )["thread_binding"]["execution_root"],
            fixture.repo_root.to_string_lossy().as_ref()
        );
    }

    #[tokio::test]
    async fn create_thread_route_rejects_unknown_explicit_model() {
        let fixture = repo_workspace_fixture();
        let registry = make_session_registry();
        let app = build_router(
            workspace_state(
                fixture.config.clone(),
                Arc::clone(&fixture.manager),
                Some(registry),
            ),
            None,
        );

        let response = app
            .oneshot(authed_json_request(
                "POST",
                "/v1/threads",
                &format!(
                    "{{\"workspace_id\":\"{}\",\"title\":\"Wrong model\",\"model\":\"definitely-missing-model\"}}",
                    GENERAL_WORKSPACE_ID
                ),
            ))
            .await
            .expect("response");

        let status = response.status();
        let json = response_json(response).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "json: {json}");
        assert_eq!(json["error"], "model not found: definitely-missing-model");
    }

    #[tokio::test]
    async fn create_thread_route_uses_explicit_workspace_path_for_off_catalog_repository() {
        let active_fixture = repo_workspace_fixture();
        let pinned_fixture = repo_workspace_fixture();
        let registry = make_session_registry();
        let app = build_router(
            workspace_state(
                active_fixture.config.clone(),
                Arc::clone(&active_fixture.manager),
                Some(registry),
            ),
            None,
        );

        let repo_id = crate::handlers::entity_ids::stable_entity_id(
            "workspace",
            &pinned_fixture.repo_root.to_string_lossy(),
        );
        let response = app
            .clone()
            .oneshot(authed_json_request(
                "POST",
                "/v1/threads",
                &format!(
                    "{{\"workspace_id\":\"{}\",\"workspace_path\":\"{}\",\"title\":\"Pinned repo thread\"}}",
                    repo_id,
                    pinned_fixture.repo_root.to_string_lossy()
                ),
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::CREATED);
        let created = response_json(response).await;
        let session_id = created["active_session_id"]
            .as_str()
            .expect("created session id")
            .to_string();

        let threads = expect_ok_json(
            app.clone(),
            "GET",
            &format!(
                "/v1/workspaces/{repo_id}/threads?workspace_path={}",
                pinned_fixture.repo_root.to_string_lossy()
            ),
        )
        .await;
        let created_thread = listed_thread_by_session_id(&threads, &session_id);
        assert_eq!(created_thread["title"], "Pinned repo thread");
        assert_eq!(created_thread["workspace_id"], repo_id);

        let sessions = expect_ok_json(app, "GET", "/v1/sessions").await;
        let binding = &listed_session(
            &sessions,
            &SessionKey::new(session_id).expect("created session key"),
        )["thread_binding"];
        assert_eq!(binding["workspace_id"], repo_id);
        assert_eq!(
            binding["execution_root"],
            pinned_fixture.repo_root.to_string_lossy().as_ref()
        );
    }

    #[tokio::test]
    async fn workspace_thread_routes_do_not_clone_legacy_sessions_into_explicit_workspace_path() {
        let active_fixture = repo_workspace_fixture();
        let pinned_fixture = repo_workspace_fixture();
        let registry = make_session_registry();
        let legacy_key = seed_session(&registry, "sess-legacy-thread");
        let app = build_router(
            workspace_state(
                active_fixture.config.clone(),
                Arc::clone(&active_fixture.manager),
                Some(registry),
            ),
            None,
        );

        let active_repo_id = crate::handlers::entity_ids::stable_entity_id(
            "workspace",
            &active_fixture.repo_root.to_string_lossy(),
        );
        let active_threads = expect_ok_json(
            app.clone(),
            "GET",
            &format!("/v1/workspaces/{active_repo_id}/threads"),
        )
        .await;
        assert_eq!(
            listed_thread_by_session_id(&active_threads, legacy_key.as_str())["workspace_id"],
            active_repo_id
        );

        let pinned_repo_id = crate::handlers::entity_ids::stable_entity_id(
            "workspace",
            &pinned_fixture.repo_root.to_string_lossy(),
        );
        let pinned_threads = expect_ok_json(
            app,
            "GET",
            &format!(
                "/v1/workspaces/{pinned_repo_id}/threads?workspace_path={}",
                pinned_fixture.repo_root.to_string_lossy()
            ),
        )
        .await;
        assert!(pinned_threads["threads"]
            .as_array()
            .expect("pinned threads")
            .iter()
            .all(|thread| thread["active_session_id"] != legacy_key.as_str()));
    }

    #[tokio::test]
    async fn create_thread_route_persists_worktree_binding_and_surfaces_it_in_read_models() {
        let fixture = repo_workspace_fixture();
        let registry = make_session_registry();
        let app = build_router(
            workspace_state(
                fixture.config.clone(),
                Arc::clone(&fixture.manager),
                Some(registry),
            ),
            None,
        );

        let workspaces = expect_ok_json(app.clone(), "GET", "/v1/workspaces").await;
        let repository = listed_workspace_by_kind(&workspaces, "repository");
        let repo_id = repository["id"].as_str().expect("repo id");
        let worktrees = expect_ok_json(
            app.clone(),
            "GET",
            &format!("/v1/workspaces/{repo_id}/worktrees"),
        )
        .await;
        let linked = listed_worktree_by_path(&worktrees, &fixture.linked_worktree);
        let worktree_id = linked["id"].as_str().expect("worktree id");

        let response = app
            .clone()
            .oneshot(authed_json_request(
                "POST",
                "/v1/threads",
                &format!(
                    "{{\"workspace_id\":\"{}\",\"title\":\"Lane thread\",\"worktree_id\":\"{}\"}}",
                    repo_id, worktree_id
                ),
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::CREATED);
        let created = response_json(response).await;
        let session_id = created["active_session_id"]
            .as_str()
            .expect("created session id")
            .to_string();

        let threads = expect_ok_json(
            app.clone(),
            "GET",
            &format!("/v1/workspaces/{repo_id}/threads"),
        )
        .await;
        let created_thread = listed_thread_by_session_id(&threads, &session_id);
        assert_eq!(created_thread["worktree_id"], worktree_id);
        assert_eq!(created_thread["title"], "Lane thread");

        let sessions = expect_ok_json(app, "GET", "/v1/sessions").await;
        let binding = &listed_session(
            &sessions,
            &SessionKey::new(session_id).expect("created session key"),
        )["thread_binding"];
        assert_eq!(binding["workspace_id"], repo_id);
        assert_eq!(
            binding["execution_root"],
            fixture.linked_worktree.to_string_lossy().as_ref()
        );
        assert_eq!(
            binding["worktree_path"],
            fixture.linked_worktree.to_string_lossy().as_ref()
        );
    }

    #[tokio::test]
    async fn worktree_routes_use_explicit_workspace_path_for_off_catalog_repository() {
        let active_fixture = repo_workspace_fixture();
        let pinned_fixture = repo_workspace_fixture();
        let app = build_router(
            workspace_state(
                active_fixture.config.clone(),
                Arc::clone(&active_fixture.manager),
                None,
            ),
            None,
        );

        let repo_id = crate::handlers::entity_ids::stable_entity_id(
            "workspace",
            &pinned_fixture.repo_root.to_string_lossy(),
        );
        let worktrees = expect_ok_json(
            app.clone(),
            "GET",
            &format!(
                "/v1/workspaces/{repo_id}/worktrees?workspace_path={}",
                pinned_fixture.repo_root.to_string_lossy()
            ),
        )
        .await;
        let linked = listed_worktree_by_path(&worktrees, &pinned_fixture.linked_worktree);
        assert_eq!(linked["workspace_id"], repo_id);

        let response = app
            .clone()
            .oneshot(authed_json_request(
                "POST",
                "/v1/worktrees",
                &format!(
                    "{{\"workspace_id\":\"{}\",\"workspace_path\":\"{}\",\"branch\":\"feature/off-catalog\",\"base_ref\":\"main\"}}",
                    repo_id,
                    pinned_fixture.repo_root.to_string_lossy()
                ),
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::CREATED);
        let created = response_json(response).await;
        assert_eq!(created["workspace_id"], repo_id);
        assert_eq!(created["branch"], "feature/off-catalog");
    }

    #[tokio::test]
    async fn worktree_mutation_routes_create_attach_archive_and_delete_lanes() {
        let fixture = repo_workspace_fixture();
        let registry = make_session_registry();
        let attached_key = seed_session(&registry, "sess-worktree-attach");
        let app = build_router(
            workspace_state(
                fixture.config.clone(),
                Arc::clone(&fixture.manager),
                Some(registry.clone()),
            ),
            None,
        );

        let workspaces = expect_ok_json(app.clone(), "GET", "/v1/workspaces").await;
        let repository = listed_workspace_by_kind(&workspaces, "repository");
        let repo_id = repository["id"].as_str().expect("repo id");

        let create_response = app
            .clone()
            .oneshot(authed_json_request(
                "POST",
                "/v1/worktrees",
                &format!(
                    "{{\"workspace_id\":\"{}\",\"branch\":\"feature/pr4-lifecycle\",\"base_ref\":\"main\"}}",
                    repo_id
                ),
            ))
            .await
            .expect("create response");
        assert_eq!(create_response.status(), StatusCode::CREATED);
        let created_worktree = response_json(create_response).await;
        let created_worktree_id = created_worktree["id"]
            .as_str()
            .expect("created worktree id");

        let attach_response = app
            .clone()
            .oneshot(authed_json_request(
                "POST",
                &format!("/v1/worktrees/{created_worktree_id}/attach-thread"),
                &format!(
                    "{{\"thread_id\":\"{}\"}}",
                    crate::handlers::entity_ids::stable_entity_id("thread", attached_key.as_str())
                ),
            ))
            .await
            .expect("attach response");
        assert_eq!(attach_response.status(), StatusCode::OK);

        let threads = expect_ok_json(
            app.clone(),
            "GET",
            &format!("/v1/workspaces/{repo_id}/threads"),
        )
        .await;
        let attached_thread = listed_thread_by_session_id(&threads, attached_key.as_str());
        assert_eq!(attached_thread["worktree_id"], created_worktree_id);

        let archive_response = app
            .clone()
            .oneshot(authed_request(
                "POST",
                &format!("/v1/worktrees/{created_worktree_id}/archive"),
            ))
            .await
            .expect("archive response");
        assert_eq!(archive_response.status(), StatusCode::OK);
        let archive_json = response_json(archive_response).await;
        assert_eq!(archive_json["archived_thread_count"], 1);

        let archived_sessions =
            expect_ok_json(app.clone(), "GET", "/v1/sessions?archived=only").await;
        assert!(listed_session_keys(&archived_sessions).contains(&attached_key.to_string()));

        let delete_response = app
            .clone()
            .oneshot(authed_request(
                "DELETE",
                &format!("/v1/worktrees/{created_worktree_id}"),
            ))
            .await
            .expect("delete response");
        assert_eq!(delete_response.status(), StatusCode::OK);
        let delete_json = response_json(delete_response).await;
        assert_eq!(delete_json["deleted"], true);

        let refreshed_worktrees =
            expect_ok_json(app, "GET", &format!("/v1/workspaces/{repo_id}/worktrees")).await;
        assert!(refreshed_worktrees["worktrees"]
            .as_array()
            .expect("refreshed worktrees")
            .iter()
            .all(|worktree| worktree["id"] != created_worktree_id));
    }

    #[tokio::test]
    async fn workspace_worktrees_endpoint_marks_configured_linked_worktree_as_active() {
        let fixture = repo_workspace_fixture();
        let config_toml = format!(
            "[workspace]\nroot = \"{}\"\n\n[tools]\nworking_dir = \"{}\"\n",
            fixture.linked_worktree.display(),
            fixture.linked_worktree.display()
        );
        let (_config_temp, config, manager) = temp_config_manager(&config_toml);
        let app = build_router(workspace_state(config, manager, None), None);

        let workspaces = expect_ok_json(app.clone(), "GET", "/v1/workspaces").await;
        let repository = listed_workspace_by_kind(&workspaces, "repository");
        let repo_id = repository["id"].as_str().expect("repo id");

        let worktrees =
            expect_ok_json(app, "GET", &format!("/v1/workspaces/{repo_id}/worktrees")).await;
        let active = listed_worktree_by_path(&worktrees, &fixture.linked_worktree);
        let repo_root = listed_worktree_by_path(&worktrees, &fixture.repo_root);
        assert_eq!(active["status"], "active");
        assert_eq!(repo_root["status"], "available");
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
            .add_node("build-node", "198.51.100.19", 8400)
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
                    node_name: "build-node".to_string(),
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
        assert!(text.contains("event: final_answer_delta\ndata: {\"text\":\"Mock response\"}"));
        assert!(text.contains("event: done\ndata: {\"response\":\"Mock response\"}"));
    }

    #[tokio::test]
    async fn session_message_sse_requires_bearer_token() {
        let registry = make_session_registry();
        let key = seed_session(&registry, "sess-sse-auth");
        let app = build_router(test_state_with_sessions(registry), None);
        let req = Request::builder()
            .method("POST")
            .uri(format!("/v1/sessions/{key}/messages"))
            .header("accept", "text/event-stream")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"message":"hello there"}"#))
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
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
        assert_archive_metadata(&json, false);
    }

    #[tokio::test]
    async fn list_sessions_defaults_to_active_only() {
        let registry = make_session_registry();
        let active = seed_session(&registry, "sess-active");
        let archived = seed_archived_session(&registry, "sess-archived");
        let app = build_router(test_state_with_sessions(registry), None);
        let resp = app
            .oneshot(authed_request("GET", "/v1/sessions"))
            .await
            .expect("response");

        assert_eq!(resp.status(), StatusCode::OK);
        let json = response_json(resp).await;
        assert_eq!(json["total"], 1);
        assert_eq!(listed_session_keys(&json), vec![active.to_string()]);
        assert!(!listed_session_keys(&json).contains(&archived.to_string()));
        assert_archive_metadata(listed_session(&json, &active), false);
    }

    #[tokio::test]
    async fn list_sessions_with_archived_all_includes_archived_sessions() {
        let registry = make_session_registry();
        let active = seed_session(&registry, "sess-active");
        let archived = seed_archived_session(&registry, "sess-archived");
        let app = build_router(test_state_with_sessions(registry), None);

        let response = app
            .oneshot(authed_request("GET", "/v1/sessions?archived=all"))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        let keys = listed_session_keys(&json);
        assert_eq!(json["total"], 2);
        assert!(keys.contains(&active.to_string()));
        assert!(keys.contains(&archived.to_string()));
        assert_archive_metadata(listed_session(&json, &active), false);
        assert_archive_metadata(listed_session(&json, &archived), true);
    }

    #[tokio::test]
    async fn list_sessions_with_archived_only_excludes_active_sessions() {
        let registry = make_session_registry();
        let active = seed_session(&registry, "sess-active");
        let archived = seed_archived_session(&registry, "sess-archived");
        let app = build_router(test_state_with_sessions(registry), None);

        let response = app
            .oneshot(authed_request("GET", "/v1/sessions?archived=only"))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["total"], 1);
        assert_eq!(listed_session_keys(&json), vec![archived.to_string()]);
        assert!(!listed_session_keys(&json).contains(&active.to_string()));
    }

    #[tokio::test]
    async fn invalid_archived_filter_returns_400() {
        let registry = make_session_registry();
        let app = build_router(test_state_with_sessions(registry), None);

        let response = app
            .oneshot(authed_request("GET", "/v1/sessions?archived=maybe"))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let json = response_json(response).await;
        assert_eq!(
            json["error"],
            "invalid archived filter 'maybe'; expected one of: active, all, only"
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
        assert_archive_metadata(&json, false);
    }

    #[tokio::test]
    async fn update_session_model_updates_stored_thread_model() {
        let registry = make_session_registry();
        let key = SessionKey::new("sess-update-model").expect("session key");
        registry
            .create(
                key.clone(),
                SessionKind::Main,
                SessionConfig {
                    label: Some("model update".to_string()),
                    model: "legacy-model".to_string(),
                    thinking: None,
                },
            )
            .expect("create session");
        registry
            .set_status(&key, SessionStatus::Idle)
            .expect("set idle");
        let app = build_router(test_state_with_sessions(registry.clone()), None);

        let response = app
            .oneshot(authed_json_request(
                "PUT",
                &format!("/v1/sessions/{key}/model"),
                r#"{"model":"mock-model"}"#,
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["model"], "mock-model");
        assert_eq!(
            registry.get_info(&key).expect("session").model,
            "mock-model"
        );
    }

    #[tokio::test]
    async fn update_session_model_resolves_from_snapshot_while_engine_is_busy() {
        let registry = make_session_registry();
        let key = SessionKey::new("sess-update-model-busy").expect("session key");
        registry
            .create(
                key.clone(),
                SessionKind::Main,
                SessionConfig {
                    label: Some("model update".to_string()),
                    model: "old-model".to_string(),
                    thinking: None,
                },
            )
            .expect("create session");
        registry
            .set_status(&key, SessionStatus::Idle)
            .expect("set idle");

        let mut state = test_state_with_engine(
            StubAppEngine::default()
                .with_active_model("old-model")
                .with_static_models(vec![
                    ModelInfoDto {
                        model_id: "old-model".to_string(),
                        provider: "test".to_string(),
                        auth_method: "none".to_string(),
                        display_name: None,
                        recommended: true,
                        thinking_levels: vec!["off".to_string()],
                    },
                    ModelInfoDto {
                        model_id: "new-model".to_string(),
                        provider: "test".to_string(),
                        auth_method: "none".to_string(),
                        display_name: None,
                        recommended: true,
                        thinking_levels: vec!["off".to_string(), "high".to_string()],
                    },
                ]),
        );
        state.session_registry = Some(registry.clone());
        let engine = Arc::clone(&state.app);
        let _busy_engine = engine.lock().await;
        let app = build_router(state, None);

        let response = tokio::time::timeout(
            std::time::Duration::from_millis(250),
            app.oneshot(authed_json_request(
                "PUT",
                &format!("/v1/sessions/{key}/model"),
                r#"{"model":"new-model"}"#,
            )),
        )
        .await
        .expect("session model update should not wait for the busy turn engine")
        .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["model"], "new-model");
        assert_eq!(registry.get_info(&key).expect("session").model, "new-model");
    }

    #[tokio::test]
    async fn update_session_model_clears_unsupported_thread_thinking() {
        let registry = make_session_registry();
        let key = SessionKey::new("sess-update-model-thinking-clear").expect("session key");
        registry
            .create(
                key.clone(),
                SessionKind::Main,
                SessionConfig {
                    label: Some("model update".to_string()),
                    model: "claude-sonnet-4-6".to_string(),
                    thinking: Some("adaptive".to_string()),
                },
            )
            .expect("create session");
        registry
            .set_status(&key, SessionStatus::Idle)
            .expect("set idle");
        let mut router = ModelRouter::new();
        router.register_provider_with_auth(
            Arc::new(StaticProvider {
                name: "anthropic",
                models: vec!["claude-sonnet-4-6"],
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
        let mut state = test_state_with_app(
            build_test_app(
                router,
                fx_config::FawxConfig::default(),
                None,
                test_runtime_info(),
            ),
            Vec::new(),
        );
        state.session_registry = Some(registry.clone());
        let app = build_router(state, None);

        let response = app
            .oneshot(authed_json_request(
                "PUT",
                &format!("/v1/sessions/{key}/model"),
                r#"{"model":"gpt-5.4"}"#,
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["model"], "gpt-5.4");
        assert!(json["thinking"].is_null());
        let info = registry.get_info(&key).expect("session");
        assert_eq!(info.model, "gpt-5.4");
        assert!(info.thinking.is_none());
    }

    #[tokio::test]
    async fn update_session_model_preserves_supported_thread_thinking() {
        let registry = make_session_registry();
        let key = SessionKey::new("sess-update-model-thinking-preserve").expect("session key");
        registry
            .create(
                key.clone(),
                SessionKind::Main,
                SessionConfig {
                    label: Some("model update".to_string()),
                    model: "claude-sonnet-4-6".to_string(),
                    thinking: Some("high".to_string()),
                },
            )
            .expect("create session");
        registry
            .set_status(&key, SessionStatus::Idle)
            .expect("set idle");
        let mut router = ModelRouter::new();
        router.register_provider_with_auth(
            Arc::new(StaticProvider {
                name: "anthropic",
                models: vec!["claude-sonnet-4-6"],
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
        let mut state = test_state_with_app(
            build_test_app(
                router,
                fx_config::FawxConfig::default(),
                None,
                test_runtime_info(),
            ),
            Vec::new(),
        );
        state.session_registry = Some(registry.clone());
        let app = build_router(state, None);

        let response = app
            .oneshot(authed_json_request(
                "PUT",
                &format!("/v1/sessions/{key}/model"),
                r#"{"model":"gpt-5.4"}"#,
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["model"], "gpt-5.4");
        assert_eq!(json["thinking"], "high");
        let info = registry.get_info(&key).expect("session");
        assert_eq!(info.model, "gpt-5.4");
        assert_eq!(info.thinking.as_deref(), Some("high"));
    }

    #[tokio::test]
    async fn update_session_thinking_updates_stored_thread_thinking() {
        let registry = make_session_registry();
        let key = seed_session(&registry, "sess-update-thinking");
        let app = build_router(test_state_with_sessions(registry.clone()), None);

        let response = app
            .oneshot(authed_json_request(
                "PUT",
                &format!("/v1/sessions/{key}/thinking"),
                r#"{"level":"off"}"#,
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["thinking"], "off");
        assert_eq!(
            registry
                .get_info(&key)
                .expect("session")
                .thinking
                .as_deref(),
            Some("off")
        );
    }

    #[tokio::test]
    async fn update_session_thinking_resolves_from_snapshot_while_engine_is_busy() {
        let registry = make_session_registry();
        let key = SessionKey::new("sess-update-thinking-busy").expect("session key");
        registry
            .create(
                key.clone(),
                SessionKind::Main,
                SessionConfig {
                    label: Some("thinking update".to_string()),
                    model: "busy-model".to_string(),
                    thinking: None,
                },
            )
            .expect("create session");
        registry
            .set_status(&key, SessionStatus::Idle)
            .expect("set idle");

        let mut state = test_state_with_engine(
            StubAppEngine::default()
                .with_active_model("busy-model")
                .with_static_models(vec![ModelInfoDto {
                    model_id: "busy-model".to_string(),
                    provider: "test".to_string(),
                    auth_method: "none".to_string(),
                    display_name: None,
                    recommended: true,
                    thinking_levels: vec!["off".to_string(), "high".to_string()],
                }]),
        );
        state.session_registry = Some(registry.clone());
        let engine = Arc::clone(&state.app);
        let _busy_engine = engine.lock().await;
        let app = build_router(state, None);

        let response = tokio::time::timeout(
            std::time::Duration::from_millis(250),
            app.oneshot(authed_json_request(
                "PUT",
                &format!("/v1/sessions/{key}/thinking"),
                r#"{"level":"high"}"#,
            )),
        )
        .await
        .expect("session thinking update should not wait for the busy turn engine")
        .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["thinking"], "high");
        assert_eq!(
            registry
                .get_info(&key)
                .expect("session")
                .thinking
                .as_deref(),
            Some("high")
        );
    }

    #[tokio::test]
    async fn update_session_thinking_rejects_unsupported_level() {
        let registry = make_session_registry();
        let key = seed_session(&registry, "sess-update-thinking-invalid");
        let app = build_router(test_state_with_sessions(registry), None);

        let response = app
            .oneshot(authed_json_request(
                "PUT",
                &format!("/v1/sessions/{key}/thinking"),
                r#"{"level":"high"}"#,
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let json = response_json(response).await;
        assert!(json["error"]
            .as_str()
            .expect("error")
            .contains("is not supported by model"));
    }

    #[tokio::test]
    async fn archive_route_archives_session_and_returns_success_payload() {
        let registry = make_session_registry();
        let key = seed_session(&registry, "sess-archive");
        let app = build_router(test_state_with_sessions(registry.clone()), None);

        let first = app
            .clone()
            .oneshot(authed_request(
                "POST",
                &format!("/v1/sessions/{key}/archive"),
            ))
            .await
            .expect("first response");

        assert_eq!(first.status(), StatusCode::OK);
        let first_json = response_json(first).await;
        assert_eq!(first_json["key"], key.as_str());
        assert_eq!(first_json["status"], "idle");
        assert_archive_metadata(&first_json, true);

        let second = app
            .oneshot(authed_request(
                "POST",
                &format!("/v1/sessions/{key}/archive"),
            ))
            .await
            .expect("second response");

        assert_eq!(second.status(), StatusCode::OK);
        let second_json = response_json(second).await;
        assert_eq!(second_json["key"], key.as_str());
        assert_archive_metadata(&second_json, true);
        assert!(registry.get_info(&key).expect("session info").is_archived());
    }

    #[tokio::test]
    async fn unarchive_route_restores_active_state_and_returns_success_payload() {
        let registry = make_session_registry();
        let key = seed_archived_session(&registry, "sess-unarchive");
        let app = build_router(test_state_with_sessions(registry.clone()), None);

        let first = app
            .clone()
            .oneshot(authed_request(
                "DELETE",
                &format!("/v1/sessions/{key}/archive"),
            ))
            .await
            .expect("first response");

        assert_eq!(first.status(), StatusCode::OK);
        let first_json = response_json(first).await;
        assert_eq!(first_json["key"], key.as_str());
        assert_eq!(first_json["status"], "idle");
        assert_archive_metadata(&first_json, false);

        let second = app
            .oneshot(authed_request(
                "DELETE",
                &format!("/v1/sessions/{key}/archive"),
            ))
            .await
            .expect("second response");

        assert_eq!(second.status(), StatusCode::OK);
        let second_json = response_json(second).await;
        assert_eq!(second_json["key"], key.as_str());
        assert_archive_metadata(&second_json, false);
        assert!(!registry.get_info(&key).expect("session info").is_archived());
    }

    #[tokio::test]
    async fn get_session_info_includes_archive_metadata_for_archived_session() {
        let registry = make_session_registry();
        let key = seed_archived_session(&registry, "sess-archived-info");
        let app = build_router(test_state_with_sessions(registry), None);

        let response = app
            .oneshot(authed_request("GET", &format!("/v1/sessions/{key}")))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["key"], key.as_str());
        assert_archive_metadata(&json, true);
    }

    #[tokio::test]
    async fn missing_session_on_archive_and_unarchive_returns_404() {
        let registry = make_session_registry();
        let app = build_router(test_state_with_sessions(registry), None);

        let archive = app
            .clone()
            .oneshot(authed_request("POST", "/v1/sessions/sess-missing/archive"))
            .await
            .expect("archive response");

        assert_eq!(archive.status(), StatusCode::NOT_FOUND);
        let archive_json = response_json(archive).await;
        assert_eq!(archive_json["error"], "session not found: sess-missing");

        let unarchive = app
            .oneshot(authed_request(
                "DELETE",
                "/v1/sessions/sess-missing/archive",
            ))
            .await
            .expect("unarchive response");

        assert_eq!(unarchive.status(), StatusCode::NOT_FOUND);
        let unarchive_json = response_json(unarchive).await;
        assert_eq!(unarchive_json["error"], "session not found: sess-missing");
    }

    #[tokio::test]
    async fn export_active_session_as_text() {
        let registry = make_session_registry();
        let key = seed_session(&registry, "sess-export-active-text");
        record_session_messages(
            &registry,
            &key,
            &[
                (SessionMessageRole::User, "First question"),
                (SessionMessageRole::Assistant, "First answer"),
            ],
        );
        let app = build_router(test_state_with_sessions(registry), None);

        let response = app
            .oneshot(authed_request("GET", &format!("/v1/sessions/{key}/export")))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .expect("content-type"),
            "text/plain; charset=utf-8"
        );
        let text = response_text(response).await;
        let question_index = text.find("First question").expect("question text");
        let answer_index = text.find("First answer").expect("answer text");
        assert!(text.contains(&format!("Session: {key}")));
        assert!(text.contains("Messages: 2"));
        assert!(question_index < answer_index);
    }

    #[tokio::test]
    async fn export_archived_session_as_text() {
        let registry = make_session_registry();
        let key = seed_archived_session(&registry, "sess-export-archived-text");
        record_session_messages(
            &registry,
            &key,
            &[
                (SessionMessageRole::User, "Archived question"),
                (SessionMessageRole::Assistant, "Archived answer"),
            ],
        );
        let app = build_router(test_state_with_sessions(registry), None);

        let response = app
            .oneshot(authed_request("GET", &format!("/v1/sessions/{key}/export")))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let text = response_text(response).await;
        assert!(text.contains(&format!("Session: {key}")));
        assert!(text.contains("Archived: yes"));
        assert!(text.contains("Archived question"));
        assert!(text.contains("Archived answer"));
    }

    #[tokio::test]
    async fn export_active_session_as_json() {
        let registry = make_session_registry();
        let key = seed_session(&registry, "sess-export-active-json");
        record_session_messages(
            &registry,
            &key,
            &[
                (SessionMessageRole::User, "Active json question"),
                (SessionMessageRole::Assistant, "Active json answer"),
            ],
        );
        let app = build_router(test_state_with_sessions(registry), None);

        let response = app
            .oneshot(authed_request(
                "GET",
                &format!("/v1/sessions/{key}/export?format=json"),
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["key"], key.as_str());
        assert_eq!(json["session"]["kind"], "main");
        assert_eq!(json["session"]["status"], "idle");
        assert_archive_metadata(&json["archive"], false);
        assert_eq!(json["total_messages"], 2);
        assert_eq!(
            json["messages"][0]["content"][0]["text"],
            "Active json question"
        );
        assert_eq!(
            json["messages"][1]["content"][0]["text"],
            "Active json answer"
        );
    }

    #[tokio::test]
    async fn export_archived_session_as_json() {
        let registry = make_session_registry();
        let key = seed_archived_session(&registry, "sess-export-archived-json");
        record_session_messages(
            &registry,
            &key,
            &[
                (SessionMessageRole::User, "Archived json question"),
                (SessionMessageRole::Assistant, "Archived json answer"),
            ],
        );
        let app = build_router(test_state_with_sessions(registry), None);

        let response = app
            .oneshot(authed_request(
                "GET",
                &format!("/v1/sessions/{key}/export?format=json"),
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["key"], key.as_str());
        assert_eq!(json["session"]["kind"], "main");
        assert_archive_metadata(&json["archive"], true);
        assert_eq!(json["total_messages"], 2);
        assert_eq!(
            json["messages"][0]["content"][0]["text"],
            "Archived json question"
        );
        assert_eq!(
            json["messages"][1]["content"][0]["text"],
            "Archived json answer"
        );
    }

    #[tokio::test]
    async fn invalid_export_format_returns_400() {
        let registry = make_session_registry();
        let key = seed_session(&registry, "sess-export-invalid-format");
        let app = build_router(test_state_with_sessions(registry), None);

        let response = app
            .oneshot(authed_request(
                "GET",
                &format!("/v1/sessions/{key}/export?format=markdown"),
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let json = response_json(response).await;
        assert_eq!(
            json["error"],
            "invalid export format 'markdown'; expected one of: text, json"
        );
    }

    #[tokio::test]
    async fn missing_session_export_returns_404() {
        let registry = make_session_registry();
        let app = build_router(test_state_with_sessions(registry), None);

        let response = app
            .oneshot(authed_request("GET", "/v1/sessions/sess-missing/export"))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let json = response_json(response).await;
        assert_eq!(json["error"], "session not found: sess-missing");
    }

    #[tokio::test]
    async fn archived_json_export_includes_archive_metadata() {
        let registry = make_session_registry();
        let key = seed_archived_session(&registry, "sess-export-archive-metadata");
        let app = build_router(test_state_with_sessions(registry), None);

        let response = app
            .oneshot(authed_request(
                "GET",
                &format!("/v1/sessions/{key}/export?format=json"),
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_archive_metadata(&json["archive"], true);
    }

    #[tokio::test]
    async fn export_preserves_stored_message_order() {
        let registry = make_session_registry();
        let key = seed_archived_session(&registry, "sess-export-message-order");
        record_session_messages(
            &registry,
            &key,
            &[
                (SessionMessageRole::User, "first message"),
                (SessionMessageRole::Assistant, "second message"),
                (SessionMessageRole::User, "third message"),
            ],
        );
        let app = build_router(test_state_with_sessions(registry), None);

        let response = app
            .oneshot(authed_request(
                "GET",
                &format!("/v1/sessions/{key}/export?format=json"),
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(
            exported_message_texts(&json),
            vec![
                "first message".to_string(),
                "second message".to_string(),
                "third message".to_string()
            ]
        );
    }

    #[tokio::test]
    async fn failed_turn_endpoint_returns_tool_chain_and_control_plane_traces() {
        let registry = make_session_registry();
        let key = seed_session(&registry, "sess-failed-diagnostic");
        registry
            .record_message_blocks(
                &key,
                SessionMessageRole::User,
                vec![SessionContentBlock::Text {
                    text: "Why did this fail?".to_string(),
                }],
                None,
            )
            .expect("record user");
        registry
            .record_message_blocks(
                &key,
                SessionMessageRole::Assistant,
                vec![SessionContentBlock::ToolUse {
                    id: "call-1".to_string(),
                    provider_id: Some("provider-call-1".to_string()),
                    name: "run_command".to_string(),
                    input: serde_json::json!({"command":"print-secret"}),
                }],
                None,
            )
            .expect("record tool use");
        registry
            .record_message_blocks(
                &key,
                SessionMessageRole::Tool,
                vec![SessionContentBlock::ToolResult {
                    tool_use_id: "call-1".to_string(),
                    content: serde_json::json!("existing session result"),
                    is_error: Some(true),
                }],
                None,
            )
            .expect("record tool result");
        let state = test_state_with_sessions(registry);
        let data_dir = state.data_dir.clone();
        persist_session_signals_for_test(
            &data_dir,
            key.as_str(),
            &[
                Signal::new(
                    LoopStep::Perceive,
                    SignalKind::Trace,
                    "resource-bearing request has no ready typed preflight route",
                    serde_json::json!({
                        "decision_kind": "preflight_route",
                        "decision": "no_ready_typed_route",
                        "fallback_mode": "public_web",
                    }),
                    2_001,
                )
                .with_id(1),
                Signal::new(
                    LoopStep::Act,
                    SignalKind::Retry,
                    "retrying tool 'run_command'",
                    serde_json::json!({
                        "decision_kind": "retry_policy",
                        "decision": "retry_allowed",
                        "retry_cause": "prior_failure",
                    }),
                    2_002,
                )
                .with_id(2),
                Signal::new(
                    LoopStep::Act,
                    SignalKind::Blocked,
                    "tool 'run_command' blocked: previous identical call failed permanently",
                    serde_json::json!({
                        "decision_kind": "tool_call_guardrail",
                        "decision": "blocked",
                        "source": "retry_policy",
                        "block_kind": "permanent_failure",
                        "failure_class": "permanent",
                    }),
                    2_003,
                )
                .with_id(3),
                failed_turn_terminal_signal(4, 2_004),
            ],
        );
        let app = build_router(state, None);

        let response = app
            .oneshot(authed_request(
                "GET",
                &format!("/v1/sessions/{key}/failed-turn?format=json"),
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["session_id"], key.as_str());
        assert_eq!(json["user_message"], "Why did this fail?");
        assert_eq!(json["tool_chain"][0]["kind"], "tool_use");
        assert_eq!(json["tool_chain"][0]["name"], "run_command");
        assert_eq!(json["tool_chain"][1]["kind"], "tool_result");
        assert!(json["decision_traces"]
            .as_array()
            .unwrap()
            .iter()
            .any(|trace| { trace["metadata"]["decision_kind"] == "preflight_route" }));
        assert!(json["decision_traces"]
            .as_array()
            .unwrap()
            .iter()
            .any(|trace| { trace["metadata"]["decision_kind"] == "retry_policy" }));
        assert!(json["decision_traces"]
            .as_array()
            .unwrap()
            .iter()
            .any(|trace| { trace["metadata"]["source"] == "retry_policy" }));
        assert_eq!(json["final_stop"]["result_kind"], "incomplete");
    }

    #[tokio::test]
    async fn failed_turn_endpoint_returns_404_for_successful_session() {
        let registry = make_session_registry();
        let key = seed_session(&registry, "sess-success-no-diagnostic");
        record_session_messages(
            &registry,
            &key,
            &[
                (SessionMessageRole::User, "hello"),
                (SessionMessageRole::Assistant, "hi"),
            ],
        );
        let state = test_state_with_sessions(registry);
        persist_session_signals_for_test(
            &state.data_dir,
            key.as_str(),
            &[complete_turn_terminal_signal(1, 2_000)],
        );
        let app = build_router(state, None);

        let response = app
            .oneshot(authed_request(
                "GET",
                &format!("/v1/sessions/{key}/failed-turn?format=json"),
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let json = response_json(response).await;
        let error = json["error"].as_str().expect("error text");
        assert!(error.contains(&format!(
            "failed-turn diagnostic not found for session: {key}"
        )));
        assert!(error.contains("signal data may be incomplete"));
    }

    #[tokio::test]
    async fn archive_export_lifecycle_restores_default_list_membership() {
        let registry = make_session_registry();
        let key = seed_export_session(&registry, "sess-archive-lifecycle");
        let app = build_router(test_state_with_sessions(registry), None);

        assert_list_membership(app.clone(), "/v1/sessions", &key, true).await;
        assert_archive_metadata(
            &expect_ok_json(app.clone(), "POST", &format!("/v1/sessions/{key}/archive")).await,
            true,
        );
        assert_list_membership(app.clone(), "/v1/sessions", &key, false).await;
        let archived_only = expect_ok_json(app.clone(), "GET", "/v1/sessions?archived=only").await;
        assert_eq!(listed_session_keys(&archived_only), vec![key.to_string()]);
        assert_archive_metadata(listed_session(&archived_only, &key), true);
        let info = expect_ok_json(app.clone(), "GET", &format!("/v1/sessions/{key}")).await;
        assert_archive_metadata(&info, true);
        let export = expect_ok_json(
            app.clone(),
            "GET",
            &format!("/v1/sessions/{key}/export?format=json"),
        )
        .await;
        assert_archive_metadata(&export["archive"], true);
        assert_eq!(
            exported_message_texts(&export),
            vec![
                "first message".to_string(),
                "second message".to_string(),
                "third message".to_string()
            ]
        );
        assert_archive_metadata(
            &expect_ok_json(
                app.clone(),
                "DELETE",
                &format!("/v1/sessions/{key}/archive"),
            )
            .await,
            false,
        );
        assert_list_membership(app, "/v1/sessions", &key, true).await;
    }

    #[tokio::test]
    async fn clear_and_delete_stay_distinct_after_archive_round_trip() {
        let registry = make_session_registry();
        let clear_key = seed_export_session(&registry, "sess-clear-contract");
        let delete_key = seed_session(&registry, "sess-delete-contract");
        let app = build_router(test_state_with_sessions(registry), None);

        let archive_uri = format!("/v1/sessions/{clear_key}/archive");
        expect_ok_json(app.clone(), "POST", &archive_uri).await;
        expect_ok_json(app.clone(), "DELETE", &archive_uri).await;
        let clear = app
            .clone()
            .oneshot(authed_request(
                "POST",
                &format!("/v1/sessions/{clear_key}/clear"),
            ))
            .await
            .expect("clear response");
        assert_eq!(clear.status(), StatusCode::OK);
        let cleared =
            expect_ok_json(app.clone(), "GET", &format!("/v1/sessions/{clear_key}")).await;
        assert_eq!(cleared["message_count"], 0);
        assert_archive_metadata(&cleared, false);
        assert_list_membership(app.clone(), "/v1/sessions", &clear_key, true).await;
        let delete = app
            .clone()
            .oneshot(authed_request(
                "DELETE",
                &format!("/v1/sessions/{delete_key}"),
            ))
            .await
            .expect("delete response");
        assert_eq!(delete.status(), StatusCode::OK);
        assert_list_membership(app.clone(), "/v1/sessions", &delete_key, false).await;
        let deleted = app
            .oneshot(authed_request("GET", &format!("/v1/sessions/{delete_key}")))
            .await
            .expect("deleted session response");
        assert_eq!(deleted.status(), StatusCode::NOT_FOUND);
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
    async fn context_endpoint_rejects_poisoned_stored_history_before_replay() {
        let key = "sess-poisoned-context";
        let registry = make_poisoned_session_registry(key);
        let app = build_router(test_state_with_sessions(registry), None);

        let response = app
            .oneshot(authed_request(
                "GET",
                &format!("/v1/sessions/{key}/context"),
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::CONFLICT);
        let json = response_json(response).await;
        assert_eq!(
            json["error"],
            format!(
                "corrupted session '{key}': invalid tool history: tool result 'call_bad' at message 0 block 0 has no matching earlier tool_use"
            )
        );
    }

    #[tokio::test]
    async fn send_message_rejects_poisoned_stored_history_before_provider_execution() {
        let key = SessionKey::new("sess-poisoned-send").expect("session key");
        let registry = make_poisoned_session_registry(key.as_str());
        let (app, app_state) =
            session_memory_test_router(registry, SessionMemory::default(), Some(key.clone()));

        let response = app
            .oneshot(authed_json_request(
                "POST",
                &format!("/v1/sessions/{key}/messages"),
                r#"{"message":"continue"}"#,
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::CONFLICT);
        let json = response_json(response).await;
        assert_eq!(
            json["error"],
            format!(
                "corrupted session '{key}': invalid tool history: tool result 'call_bad' at message 0 block 0 has no matching earlier tool_use"
            )
        );
        assert!(
            app_state
                .lock()
                .expect("state lock")
                .loaded_memories
                .is_empty(),
            "provider execution must not start for poisoned sessions"
        );
    }

    #[tokio::test]
    async fn send_message_binds_stored_session_model_for_turn() {
        let key = SessionKey::new("sess-model-scope").expect("session key");
        let registry = make_session_registry();
        registry
            .create(
                key.clone(),
                SessionKind::Main,
                SessionConfig {
                    label: Some("model scoped".to_string()),
                    model: "thread-model".to_string(),
                    thinking: None,
                },
            )
            .expect("create session");
        registry
            .set_status(&key, SessionStatus::Idle)
            .expect("set idle");
        let (app, app_state) =
            session_memory_test_router(registry, SessionMemory::default(), Some(key.clone()));

        let response = app
            .oneshot(authed_json_request(
                "POST",
                &format!("/v1/sessions/{key}/messages"),
                r#"{"message":"continue"}"#,
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["model"], "thread-model");
        let state = app_state.lock().expect("state lock");
        assert_eq!(state.processed_models, vec!["thread-model"]);
        assert_eq!(state.current_model, "mock-model");
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
    async fn get_session_messages_returns_turn_scoped_grouped_tool_history() {
        let registry = make_session_registry();
        let key = seed_session(&registry, "sess-grouped-history");
        registry
            .record_turn(
                &key,
                vec![
                    SessionMessage::structured(
                        SessionMessageRole::Assistant,
                        vec![
                            SessionContentBlock::ToolUse {
                                id: "call_1".to_string(),
                                provider_id: Some("fc_1".to_string()),
                                name: "read_file".to_string(),
                                input: serde_json::json!({"path": "README.md"}),
                            },
                            SessionContentBlock::ToolUse {
                                id: "call_2".to_string(),
                                provider_id: Some("fc_2".to_string()),
                                name: "list_dir".to_string(),
                                input: serde_json::json!({"path": "."}),
                            },
                        ],
                        1,
                        Some(21),
                    ),
                    SessionMessage::structured(
                        SessionMessageRole::Tool,
                        vec![
                            SessionContentBlock::ToolResult {
                                tool_use_id: "call_1".to_string(),
                                content: serde_json::json!("file contents"),
                                is_error: Some(false),
                            },
                            SessionContentBlock::ToolResult {
                                tool_use_id: "call_2".to_string(),
                                content: serde_json::json!(["Cargo.toml"]),
                                is_error: Some(false),
                            },
                        ],
                        2,
                        None,
                    ),
                    SessionMessage::text(SessionMessageRole::Assistant, "Done.", 3),
                ],
                SessionMemory::default(),
            )
            .expect("record turn");
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

        assert_eq!(json["total"], 3);
        assert_eq!(json["messages"][0]["role"], "assistant");
        assert_eq!(
            json["messages"][0]["content"]
                .as_array()
                .expect("tool uses")
                .len(),
            2
        );
        assert_eq!(json["messages"][0]["content"][0]["provider_id"], "fc_1");
        assert_eq!(json["messages"][0]["content"][1]["provider_id"], "fc_2");
        assert_eq!(json["messages"][0]["token_count"], 21);
        assert_eq!(json["messages"][1]["role"], "tool");
        assert_eq!(
            json["messages"][1]["content"]
                .as_array()
                .expect("tool results")
                .len(),
            2
        );
        assert_eq!(json["messages"][2]["role"], "assistant");
        assert_eq!(json["messages"][2]["content"][0]["text"], "Done.");
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
                    prompt_cache: Default::default(),
                    prompt_cache_affinity: Default::default(),
                }
            }
        }

        let registry = make_session_registry();
        let key = seed_session(&registry, "sess-orphan-tool");
        registry
            .set_model(&key, "capturing-model".to_string())
            .expect("set session model");
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
        assert!(text.contains("event: final_answer_delta\ndata: {\"text\":\"Mock response\"}"));
        assert!(text.contains("event: done\ndata: {\"response\":\"Mock response\"}"));
    }

    #[tokio::test]
    async fn second_session_stream_runs_while_first_session_turn_is_active() {
        let registry = make_session_registry();
        let first_key = seed_session(&registry, "sess-engine-first");
        let second_key = seed_session(&registry, "sess-engine-second");
        let (app, control) = blocking_session_turn_router(registry);

        let first_task = tokio::spawn(app.clone().oneshot(authed_sse_json_request(
            "POST",
            &format!("/v1/sessions/{first_key}/messages"),
            r#"{"message":"first"}"#,
        )));
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            control.wait_for_first_turn(),
        )
        .await
        .expect("first turn should start");

        let second_response = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            app.clone().oneshot(authed_sse_json_request(
                "POST",
                &format!("/v1/sessions/{second_key}/messages"),
                r#"{"message":"second"}"#,
            )),
        )
        .await
        .expect("second response should open while first is active")
        .expect("second response");
        assert_eq!(second_response.status(), StatusCode::OK);

        let second_body = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            second_response.into_body().collect(),
        )
        .await
        .expect("second stream should complete while first is active")
        .expect("second body");
        let second_text = String::from_utf8(second_body.to_bytes().to_vec()).expect("second utf8");
        assert!(!second_text.contains(r#""kind":"queued""#));
        assert!(second_text.contains("response for second"));
        assert_eq!(
            control.processed_inputs(),
            vec!["first".to_string(), "second".to_string()]
        );

        control.release_first_turn();
        let first_response = tokio::time::timeout(std::time::Duration::from_secs(1), first_task)
            .await
            .expect("first response should finish")
            .expect("first task")
            .expect("first response");
        assert_eq!(first_response.status(), StatusCode::OK);
        let first_body = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            first_response.into_body().collect(),
        )
        .await
        .expect("first stream should complete")
        .expect("first body");
        assert!(String::from_utf8(first_body.to_bytes().to_vec())
            .expect("first utf8")
            .contains("response for first"));
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
    async fn session_message_uses_bound_execution_root_instead_of_global_workspace_root() {
        let registry = make_session_registry();
        let key = seed_session(&registry, "sess-bound-root");
        let execution_root = TempDir::new().expect("tempdir");
        registry
            .set_thread_binding(
                &key,
                Some(SessionThreadBinding {
                    workspace_id: GENERAL_WORKSPACE_ID.to_string(),
                    execution_root: Some(execution_root.path().to_string_lossy().into_owned()),
                    worktree_path: None,
                }),
            )
            .expect("set thread binding");

        let (app, state) =
            session_memory_test_router(registry, SessionMemory::default(), Some(key.clone()));
        let req = Request::builder()
            .method("POST")
            .uri("/message")
            .header("authorization", format!("Bearer {TEST_TOKEN}"))
            .header("content-type", "application/json")
            .body(Body::from(format!(
                r#"{{"message":"hello bound root","session_id":"{}"}}"#,
                key.as_str()
            )))
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);

        let observed_roots = &state.lock().expect("state lock").loaded_execution_roots;
        assert_eq!(observed_roots.len(), 1);
        assert_eq!(observed_roots[0], execution_root.path());
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
                execution_root: Arc::new(fx_kernel::ExecutionRoot::new(std::env::temp_dir())),
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
                permission_prompt_state: None,
                ripcord_journal: Arc::new(fx_ripcord::RipcordJournal::new(
                    std::env::temp_dir().as_path(),
                )),
                improvement_provider: None,
                credential_store: None,
                token_broker: None,
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
        assert_eq!(json["models"][0]["thinking_levels"][0], "off");
        assert_eq!(json["models"][0]["thinking_levels"][1], "adaptive");
        assert_eq!(json["models"][1]["model_id"], "gpt-4o");
    }

    #[tokio::test]
    async fn list_models_prefers_dynamic_catalog_metadata() {
        let state = test_state_with_engine(
            StubAppEngine::default()
                .with_active_model("openai/gpt-5.4")
                .with_static_models(vec![ModelInfoDto {
                    model_id: "stale-model".to_string(),
                    provider: "openrouter".to_string(),
                    auth_method: "api_key".to_string(),
                    display_name: None,
                    recommended: true,
                    thinking_levels: vec!["off".to_string()],
                }])
                .with_dynamic_models(vec![ModelInfoDto {
                    model_id: "openai/gpt-5.4".to_string(),
                    provider: "openrouter".to_string(),
                    auth_method: "api_key".to_string(),
                    display_name: Some("GPT-5.4".to_string()),
                    recommended: false,
                    thinking_levels: vec![
                        "none".to_string(),
                        "low".to_string(),
                        "medium".to_string(),
                        "high".to_string(),
                        "xhigh".to_string(),
                    ],
                }]),
        );
        let app = build_router(state, None);

        let response = app
            .oneshot(authed_request("GET", "/v1/models"))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["models"].as_array().expect("models").len(), 1);
        assert_eq!(json["models"][0]["model_id"], "openai/gpt-5.4");
        assert_eq!(json["models"][0]["display_name"], "GPT-5.4");
        assert_eq!(json["models"][0]["recommended"], false);
        assert_eq!(json["models"][0]["thinking_levels"][0], "none");
        assert_eq!(json["models"][0]["thinking_levels"][4], "xhigh");
    }

    #[tokio::test]
    async fn list_models_publishes_post_refresh_active_model() {
        let state = test_state_with_engine(
            StubAppEngine::default()
                .with_active_model("accounts/fireworks/models/deepseek-v3p1")
                .with_dynamic_active_model("z-ai/glm-5.1")
                .with_dynamic_models(vec![ModelInfoDto {
                    model_id: "z-ai/glm-5.1".to_string(),
                    provider: "openrouter".to_string(),
                    auth_method: "api_key".to_string(),
                    display_name: Some("GLM 5.1".to_string()),
                    recommended: true,
                    thinking_levels: vec!["off".to_string()],
                }]),
        );
        let app = build_router(state, None);

        let response = app
            .oneshot(authed_request("GET", "/v1/models"))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["active_model"], "z-ai/glm-5.1");
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

    fn brave_search_settings_manifest_toml(fields_toml: &str) -> String {
        format!(
            r#"
name = "brave-search"
version = "1.0.0"
description = "Search the web"
author = "Fawx"
api_version = "host_api_v1"

[settings]
version = 1

{fields_toml}
"#
        )
    }

    #[tokio::test]
    async fn get_skill_settings_returns_schema_and_redacted_secret_status() {
        let temp = TempDir::new().expect("tempdir");
        let skills_dir = temp.path().join("skills").join("brave-search");
        std::fs::create_dir_all(&skills_dir).expect("create skills dir");
        std::fs::write(
            skills_dir.join("manifest.toml"),
            brave_search_settings_manifest_toml(
                r#"
[[settings.fields]]
key = "api_key"
label = "API Key"
type = "secret"
required = true

[[settings.fields]]
key = "region"
label = "Region"
type = "text"
"#,
            ),
        )
        .expect("write manifest");
        fx_auth::credential_store::EncryptedFileCredentialStore::open(temp.path())
            .expect("open skill store")
            .set_generic("skill:brave-search:api_key", "brv_secret_123")
            .expect("store secret");

        let mut config = fx_config::FawxConfig::default();
        config.general.data_dir = Some(temp.path().to_path_buf());
        let app = build_router(test_state_with_config(config, None, Vec::new()), None);

        let response = app
            .oneshot(authed_request("GET", "/v1/skills/brave-search/settings"))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["skill_name"], "brave-search");
        assert_eq!(json["schema"]["fields"][0]["key"], "api_key");
        assert_eq!(json["schema"]["fields"][0]["field_type"], "secret");
        assert!(json["schema"]["fields"][0].get("type").is_none());
        assert_eq!(json["values"][0]["is_secret"], true);
        assert_eq!(json["values"][0]["is_configured"], true);
        assert_eq!(json["values"][0]["value"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn get_skill_settings_reuses_shared_store_when_database_is_open() {
        let temp = TempDir::new().expect("tempdir");
        let skills_dir = temp.path().join("skills").join("brave-search");
        std::fs::create_dir_all(&skills_dir).expect("create skills dir");
        std::fs::write(
            skills_dir.join("manifest.toml"),
            brave_search_settings_manifest_toml(
                r#"
[[settings.fields]]
key = "api_key"
label = "API Key"
type = "secret"
required = true
"#,
            ),
        )
        .expect("write manifest");

        let store = Arc::new(
            fx_auth::credential_store::EncryptedFileCredentialStore::open(temp.path())
                .expect("open shared skill store"),
        );
        store
            .set_generic("skill:brave-search:api_key", "brv_secret_123")
            .expect("store secret");

        let mut config = fx_config::FawxConfig::default();
        config.general.data_dir = Some(temp.path().to_path_buf());
        let mut state = test_state_with_config(config, None, Vec::new());
        state.credential_store = Some(Arc::clone(&store));
        let app = build_router(state, None);

        let response = app
            .oneshot(authed_request("GET", "/v1/skills/brave-search/settings"))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["values"][0]["is_configured"], true);
    }

    #[tokio::test]
    async fn update_skill_settings_persists_without_restart() {
        let temp = TempDir::new().expect("tempdir");
        let skills_dir = temp.path().join("skills").join("brave-search");
        std::fs::create_dir_all(&skills_dir).expect("create skills dir");
        std::fs::write(
            skills_dir.join("manifest.toml"),
            brave_search_settings_manifest_toml(
                r#"
[[settings.fields]]
key = "api_key"
label = "API Key"
type = "secret"
required = true

[[settings.fields]]
key = "safesearch"
label = "Safe Search"
type = "boolean"
"#,
            ),
        )
        .expect("write manifest");

        let mut config = fx_config::FawxConfig::default();
        config.general.data_dir = Some(temp.path().to_path_buf());
        let app = build_router(test_state_with_config(config, None, Vec::new()), None);

        let response = app
            .oneshot(authed_json_request(
                "PUT",
                "/v1/skills/brave-search/settings",
                r#"{"values":[{"key":"api_key","value":"brv_secret_123"},{"key":"safesearch","value":"true"}]}"#,
            ))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["updated"], true);
        assert_eq!(json["settings"]["values"][1]["value"], "true");
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
                .uri("/v1/skills/brave-search/settings")
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
            session_runs: crate::state::SessionRunRegistry::default(),
            session_engines: crate::state::SessionEnginePool::default(),
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
            credential_store: None,
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

    mod observation_round_restriction_live_api {
        use super::*;
        use axum::http::StatusCode;
        use fx_kernel::act::{ToolExecutor, ToolExecutorError, ToolResult};
        use fx_kernel::budget::{BudgetConfig, BudgetTracker};
        use fx_kernel::cancellation::CancellationToken;
        use fx_kernel::context_manager::ContextCompactor;
        use fx_kernel::loop_engine::LoopEngine;
        use fx_llm::{
            CompletionProvider, CompletionRequest, CompletionResponse, CompletionStream,
            ModelRouter, ProviderError as LlmError,
        };
        use fx_session::{
            SessionConfig, SessionKey, SessionKind, SessionRegistry, SessionStatus, SessionStore,
        };
        use fx_subagent::{
            test_support::DisabledSubagentFactory, SubagentLimits, SubagentManager,
            SubagentManagerDeps,
        };
        use std::sync::Arc;

        fn live_test_runtime_info() -> Arc<std::sync::RwLock<RuntimeInfo>> {
            Arc::new(std::sync::RwLock::new(RuntimeInfo {
                active_model: String::new(),
                provider: String::new(),
                skills: Vec::new(),
                config_summary: ConfigSummary {
                    max_iterations: 3,
                    max_history: 20,
                    memory_enabled: false,
                    tool_invocations_remaining: 0,
                },
                authority: None,
                version: "test".to_string(),
            }))
        }

        fn live_make_session_registry() -> SessionRegistry {
            let storage = fx_storage::Storage::open_in_memory().expect("in-memory storage");
            SessionRegistry::new(SessionStore::new(storage)).expect("session registry")
        }

        fn live_seed_session(registry: &SessionRegistry, key: &str) -> SessionKey {
            let key = SessionKey::new(key).expect("session key");
            registry
                .create(
                    key.clone(),
                    SessionKind::Main,
                    SessionConfig {
                        label: Some(format!("label-{key}")),
                        model: "mock-model".to_string(),
                        thinking: None,
                    },
                )
                .expect("create session");
            registry
                .set_status(&key, SessionStatus::Idle)
                .expect("set idle");
            key
        }

        fn live_test_state_with_app(
            app: HeadlessApp,
            webhooks: Vec<Arc<WebhookChannel>>,
        ) -> HttpState {
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
                session_runs: crate::state::SessionRunRegistry::default(),
                session_engines: crate::state::SessionEnginePool::default(),
                start_time: Instant::now(),
                server_runtime: ServerRuntime::local(8400),
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
                credential_store: None,
                experiment_registry: {
                    let registry = ExperimentRegistry::new(std::env::temp_dir().as_path()).unwrap();
                    Arc::new(tokio::sync::Mutex::new(registry))
                },
                improvement_provider: None,
                telemetry: in_memory_telemetry(),
            }
        }

        fn synthesis_state(
            has_initial_value: bool,
        ) -> Arc<crate::handlers::synthesis::SynthesisState> {
            Arc::new(crate::handlers::synthesis::SynthesisState::new(
                has_initial_value,
            ))
        }

        fn live_build_test_app_with_engine(
            router: ModelRouter,
            loop_engine: LoopEngine,
            mut config: fx_config::FawxConfig,
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

            let subagent_manager = Arc::new(SubagentManager::new(SubagentManagerDeps {
                factory: Arc::new(DisabledSubagentFactory::new("disabled")),
                limits: SubagentLimits::default(),
            }));

            HeadlessApp::new(HeadlessAppDeps {
                loop_engine,
                router: Arc::new(std::sync::RwLock::new(router)),
                runtime_info: live_test_runtime_info(),
                config,
                execution_root: Arc::new(fx_kernel::ExecutionRoot::new(std::env::temp_dir())),
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
                permission_prompt_state: None,
                ripcord_journal: Arc::new(fx_ripcord::RipcordJournal::new(
                    std::env::temp_dir().as_path(),
                )),
                improvement_provider: None,
                credential_store: None,
                token_broker: None,
                experiment_registry: None,
            })
            .expect("test app")
        }

        #[derive(Debug, Clone, Copy)]
        enum ObservationScenario {
            DistinctReads,
            RepeatedReads,
        }

        #[derive(Debug)]
        struct ObservationRoundToolExecutor;

        #[async_trait]
        impl ToolExecutor for ObservationRoundToolExecutor {
            async fn execute_tools(
                &self,
                calls: &[fx_llm::ToolCall],
                _cancel: Option<&CancellationToken>,
            ) -> Result<Vec<ToolResult>, ToolExecutorError> {
                Ok(calls
                    .iter()
                    .map(|call| ToolResult {
                        tool_call_id: call.id.clone(),
                        tool_name: call.name.clone(),
                        success: true,
                        output: "ok".to_string(),
                        failure_class: None,
                    })
                    .collect())
            }

            fn tool_definitions(&self) -> Vec<fx_llm::ToolDefinition> {
                vec![fx_llm::ToolDefinition {
                    name: "read_file".to_string(),
                    description: "Read a file".to_string(),
                    parameters: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "path": { "type": "string" }
                        },
                        "required": ["path"]
                    }),
                }]
            }
        }

        #[derive(Debug)]
        struct ObservationRoundProvider {
            scenario: ObservationScenario,
            requests: Arc<std::sync::Mutex<Vec<CompletionRequest>>>,
        }

        impl ObservationRoundProvider {
            fn new(
                scenario: ObservationScenario,
                requests: Arc<std::sync::Mutex<Vec<CompletionRequest>>>,
            ) -> Self {
                Self { scenario, requests }
            }

            fn request_has_nudge(request: &CompletionRequest) -> bool {
                request.messages.iter().any(|message| {
                    message.role == fx_llm::MessageRole::System
                        && message.content.iter().any(|block| {
                            matches!(block, ContentBlock::Text { text } if text.contains("multiple tool rounds only gathering information"))
                        })
                })
            }

            fn tool_use_response(id: &str, path: &str) -> CompletionResponse {
                CompletionResponse {
                    content: Vec::new(),
                    tool_calls: vec![fx_llm::ToolCall {
                        id: id.to_string(),
                        name: "read_file".to_string(),
                        arguments: serde_json::json!({ "path": path }),
                    }],
                    usage: None,
                    stop_reason: Some("tool_use".to_string()),
                }
            }
        }

        #[async_trait]
        impl CompletionProvider for ObservationRoundProvider {
            async fn complete(
                &self,
                request: CompletionRequest,
            ) -> Result<CompletionResponse, LlmError> {
                let mut requests = self.requests.lock().expect("capture lock");
                let request_index = requests.len();
                requests.push(request.clone());
                drop(requests);

                if request.tools.is_empty() {
                    return Ok(mock_completion_response());
                }

                match (self.scenario, request_index) {
                    (ObservationScenario::DistinctReads, 0) => {
                        Ok(Self::tool_use_response("call-1", "a.txt"))
                    }
                    (ObservationScenario::DistinctReads, 1) => {
                        Ok(Self::tool_use_response("call-2", "b.txt"))
                    }
                    (ObservationScenario::DistinctReads, _) => Ok(mock_completion_response()),
                    (ObservationScenario::RepeatedReads, 0) => {
                        Ok(Self::tool_use_response("call-1", "a.txt"))
                    }
                    (ObservationScenario::RepeatedReads, 1) => {
                        Ok(Self::tool_use_response("call-2", "a.txt"))
                    }
                    (ObservationScenario::RepeatedReads, _) => {
                        if request_index >= 8 {
                            Ok(mock_completion_response())
                        } else {
                            Ok(Self::tool_use_response("call-3", "a.txt"))
                        }
                    }
                }
            }

            async fn complete_stream(
                &self,
                request: CompletionRequest,
            ) -> Result<CompletionStream, LlmError> {
                let response = self.complete(request).await?;
                let stream = futures::stream::once(async move {
                    Ok(fx_llm::StreamChunk {
                        delta_content: response.content.iter().find_map(|block| match block {
                            ContentBlock::Text { text } => Some(text.clone()),
                            _ => None,
                        }),
                        stop_reason: response.stop_reason.clone(),
                        ..Default::default()
                    })
                });
                Ok(Box::pin(stream))
            }

            fn name(&self) -> &str {
                "observation-rounds"
            }

            fn supported_models(&self) -> Vec<String> {
                vec!["mock-model".to_string()]
            }

            fn capabilities(&self) -> fx_llm::ProviderCapabilities {
                fx_llm::ProviderCapabilities {
                    supports_temperature: false,
                    requires_streaming: false,
                    prompt_cache: Default::default(),
                    prompt_cache_affinity: Default::default(),
                }
            }
        }

        fn observation_round_router(
            scenario: ObservationScenario,
            requests: Arc<std::sync::Mutex<Vec<CompletionRequest>>>,
        ) -> ModelRouter {
            let mut router = ModelRouter::new();
            router.register_provider(Box::new(ObservationRoundProvider::new(scenario, requests)));
            router.set_active("mock-model").expect("set active");
            router
        }

        struct LiveServerGuard(tokio::task::JoinHandle<()>);

        impl Drop for LiveServerGuard {
            fn drop(&mut self) {
                self.0.abort();
            }
        }

        async fn spawn_live_server(app: Router) -> (String, LiveServerGuard) {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind live server");
            let addr = listener.local_addr().expect("local addr");
            let base_url = format!("http://{addr}");
            let handle = tokio::spawn(async move {
                axum::serve(listener, app).await.ok();
            });
            (base_url, LiveServerGuard(handle))
        }

        async fn run_live_observation_scenario(
            scenario: ObservationScenario,
        ) -> (Vec<CompletionRequest>, serde_json::Value) {
            let captured = Arc::new(std::sync::Mutex::new(Vec::new()));
            let router = observation_round_router(scenario, Arc::clone(&captured));

            let mut config = fx_config::FawxConfig::default();
            config.model.default_model = Some("mock-model".to_string());
            let app = live_build_test_app_with_engine(
                router,
                LoopEngine::builder()
                    .budget(BudgetTracker::new(BudgetConfig::default(), 0, 0))
                    .context(ContextCompactor::new(2048, 256))
                    .max_iterations(8)
                    .tool_executor(Arc::new(ObservationRoundToolExecutor))
                    .synthesis_instruction("Summarize".to_string())
                    .build()
                    .expect("observation test engine"),
                config,
            );
            let registry = live_make_session_registry();
            let key = live_seed_session(&registry, "sess-observation-rounds");

            let mut state = live_test_state_with_app(app, Vec::new());
            state.session_registry = Some(registry);
            let app = build_router(state, None);

            let (base_url, _server) = spawn_live_server(app).await;
            let client = reqwest::Client::new();
            let response = client
                .post(format!("{base_url}/v1/sessions/{key}/messages"))
                .header("authorization", format!("Bearer {TEST_TOKEN}"))
                .header("content-type", "application/json")
                .json(&serde_json::json!({
                    "message": "research two files and summarize the differences"
                }))
                .send()
                .await
                .expect("send live request");
            assert_eq!(response.status(), StatusCode::OK);
            let body = response
                .json::<serde_json::Value>()
                .await
                .expect("json body");

            let requests = captured.lock().expect("capture lock").clone();
            (requests, body)
        }

        #[tokio::test]
        async fn live_headless_api_distinct_observation_rounds_keep_tools_available() {
            let (requests, body) =
                run_live_observation_scenario(ObservationScenario::DistinctReads).await;

            assert_eq!(body["result_kind"], "complete");
            assert!(
                requests
                    .iter()
                    .take(3)
                    .all(|request| !request.tools.is_empty()),
                "distinct observation rounds should keep the tool surface available"
            );
            assert!(
                requests
                    .iter()
                    .take(3)
                    .all(|request| !ObservationRoundProvider::request_has_nudge(request)),
                "distinct research should not receive the repeated-observation nudge"
            );
        }

        #[tokio::test]
        async fn live_headless_api_repeated_observation_rounds_block_duplicate_evidence() {
            let (requests, body) =
                run_live_observation_scenario(ObservationScenario::RepeatedReads).await;

            assert!(body["result_kind"] == "partial" || body["result_kind"] == "complete");
            let terminal_request = requests
                .iter()
                .skip(2)
                .find(|request| {
                    request.tools.is_empty()
                        && request.messages.iter().any(|message| {
                            message.content.iter().any(|block| {
                                matches!(block, ContentBlock::Text { text } if text.contains("already gathered the same evidence"))
                            })
                        })
                })
                .expect(
                    "exact duplicate observation should be blocked before the older observation nudge path",
                );
            assert!(
                !ObservationRoundProvider::request_has_nudge(terminal_request),
                "duplicate-evidence blocking should avoid reviving the older repeated-read nudge path"
            );
        }
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
                prompt_cache: Default::default(),
                prompt_cache_affinity: Default::default(),
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
                prompt_cache: Default::default(),
                prompt_cache_affinity: Default::default(),
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
                prompt_cache: Default::default(),
                prompt_cache_affinity: Default::default(),
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
            execution_root: Arc::new(fx_kernel::ExecutionRoot::new(std::env::temp_dir())),
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
            permission_prompt_state: None,
            ripcord_journal: Arc::new(fx_ripcord::RipcordJournal::new(
                std::env::temp_dir().as_path(),
            )),
            improvement_provider: None,
            credential_store: None,
            token_broker: None,
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
            None,
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
