use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use fx_fleet::{
    current_time_ms, FleetHeartbeat, FleetManager, FleetRegistrationRequest,
    FleetRegistrationResponse, FleetTaskResult, NodeCapability, NodeStatus, WorkerState,
};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::warn;

#[derive(Clone)]
struct FleetState {
    manager: Arc<Mutex<FleetManager>>,
}

pub fn fleet_router(manager: Arc<Mutex<FleetManager>>) -> Router {
    let state = FleetState { manager };
    Router::new()
        .route("/fleet/register", post(handle_fleet_register))
        .route("/fleet/heartbeat", post(handle_fleet_heartbeat))
        .route("/fleet/result", post(handle_fleet_result))
        .with_state(state)
}

async fn handle_fleet_register(
    State(state): State<FleetState>,
    Json(request): Json<FleetRegistrationRequest>,
) -> impl IntoResponse {
    let mut manager = state.manager.lock().await;
    let Some(node_id) = manager.verify_bearer(&request.bearer_token) else {
        return registration_response(StatusCode::UNAUTHORIZED, "", false, "invalid bearer token");
    };
    let capabilities = request
        .capabilities
        .iter()
        .map(|capability| NodeCapability::from(capability.as_str()))
        .collect();
    match manager.register_worker(&node_id, capabilities, current_time_ms()) {
        Ok(node) => registration_response(StatusCode::OK, &node.node_id, true, "registered"),
        Err(_) => registration_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &node_id,
            false,
            "registration failed",
        ),
    }
}

async fn handle_fleet_heartbeat(
    State(state): State<FleetState>,
    headers: HeaderMap,
    Json(heartbeat): Json<FleetHeartbeat>,
) -> impl IntoResponse {
    let Some(node_id) = authenticated_node_id(&state.manager, &headers).await else {
        return StatusCode::UNAUTHORIZED;
    };
    if node_id != heartbeat.node_id {
        return StatusCode::UNAUTHORIZED;
    }
    match state.manager.lock().await.record_worker_heartbeat(
        &node_id,
        node_status(&heartbeat.status),
        current_time_ms(),
    ) {
        Ok(()) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn handle_fleet_result(
    State(state): State<FleetState>,
    headers: HeaderMap,
    Json(_result): Json<FleetTaskResult>,
) -> impl IntoResponse {
    let Some(node_id) = authenticated_node_id(&state.manager, &headers).await else {
        return StatusCode::UNAUTHORIZED;
    };
    match state
        .manager
        .lock()
        .await
        .mark_result_received(&node_id, current_time_ms())
    {
        Ok(()) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn registration_response(
    status: StatusCode,
    node_id: &str,
    accepted: bool,
    message: &str,
) -> (StatusCode, Json<FleetRegistrationResponse>) {
    let response = FleetRegistrationResponse {
        node_id: node_id.to_string(),
        accepted,
        message: message.to_string(),
    };
    (status, Json(response))
}

async fn authenticated_node_id(
    manager: &Arc<Mutex<FleetManager>>,
    headers: &HeaderMap,
) -> Option<String> {
    let bearer = bearer_token(headers)?;
    manager.lock().await.verify_bearer(&bearer)
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let header = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    header.strip_prefix("Bearer ").map(str::to_owned)
}

fn node_status(status: &WorkerState) -> NodeStatus {
    match status {
        WorkerState::Idle => NodeStatus::Online,
        WorkerState::Busy => NodeStatus::Busy,
        WorkerState::ShuttingDown => NodeStatus::Offline,
        _ => unknown_worker_state_status(status),
    }
}

fn unknown_worker_state_status(state: &impl std::fmt::Debug) -> NodeStatus {
    warn!(?state, "unknown WorkerState variant, treating as busy");
    NodeStatus::Busy
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use fx_fleet::{FleetTaskStatus, FleetToken};
    use http_body_util::BodyExt;
    use serde::de::DeserializeOwned;
    use tower::ServiceExt;

    struct TestFleet {
        _temp_dir: tempfile::TempDir,
        manager: Arc<Mutex<FleetManager>>,
        token: FleetToken,
    }

    impl TestFleet {
        fn node_id(&self) -> &str {
            &self.token.node_id
        }
    }

    fn build_test_fleet() -> TestFleet {
        let temp_dir = tempfile::TempDir::new().expect("tempdir should create");
        let mut manager = FleetManager::init(temp_dir.path()).expect("fleet should initialize");
        let token = manager
            .add_node("build-node", "198.51.100.19", 8400)
            .expect("node should add");
        TestFleet {
            _temp_dir: temp_dir,
            manager: Arc::new(Mutex::new(manager)),
            token,
        }
    }

    fn registration_request(token: &str) -> FleetRegistrationRequest {
        FleetRegistrationRequest {
            node_name: "build-node".to_string(),
            bearer_token: token.to_string(),
            capabilities: vec!["agentic_loop".to_string(), "macos-aarch64".to_string()],
            rust_version: Some("1.85.0".to_string()),
            os: Some("macos".to_string()),
            cpus: Some(8),
            ram_gb: None,
        }
    }

    fn heartbeat_request(node_id: &str, status: WorkerState) -> FleetHeartbeat {
        FleetHeartbeat {
            node_id: node_id.to_string(),
            status,
            current_task: Some("exp-001".to_string()),
        }
    }

    fn result_request() -> FleetTaskResult {
        FleetTaskResult {
            task_id: "exp-001".to_string(),
            status: FleetTaskStatus::Complete,
            candidate_patch: None,
            evaluation: None,
            build_log: None,
            error: None,
            duration_ms: 100,
        }
    }

    async fn response_json<T>(response: axum::response::Response) -> T
    where
        T: DeserializeOwned,
    {
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body should collect")
            .to_bytes();
        serde_json::from_slice(&bytes).expect("body should deserialize")
    }

    #[derive(Clone, Default)]
    struct LogBuffer(Arc<std::sync::Mutex<Vec<u8>>>);

    impl LogBuffer {
        fn contents(&self) -> String {
            String::from_utf8(self.0.lock().expect("log buffer lock").clone())
                .expect("log buffer should be utf8")
        }
    }

    struct LogWriter(Arc<std::sync::Mutex<Vec<u8>>>);

    impl std::io::Write for LogWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0
                .lock()
                .expect("log writer lock")
                .extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogBuffer {
        type Writer = LogWriter;

        fn make_writer(&'a self) -> Self::Writer {
            LogWriter(self.0.clone())
        }
    }

    fn with_warn_logs<T>(action: impl FnOnce() -> T) -> (T, String) {
        let logs = LogBuffer::default();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .without_time()
            .with_max_level(tracing::Level::WARN)
            .with_writer(logs.clone())
            .finish();
        let result = tracing::subscriber::with_default(subscriber, action);
        (result, logs.contents())
    }

    #[test]
    fn fleet_node_status_maps_worker_states() {
        assert_eq!(node_status(&WorkerState::Idle), NodeStatus::Online);
        assert_eq!(node_status(&WorkerState::Busy), NodeStatus::Busy);
        assert_eq!(node_status(&WorkerState::ShuttingDown), NodeStatus::Offline);
    }

    #[test]
    fn unknown_worker_state_logs_warning_and_maps_to_busy() {
        let (status, logs) = with_warn_logs(|| unknown_worker_state_status(&"future-state"));

        assert_eq!(status, NodeStatus::Busy);
        assert!(logs.contains("unknown WorkerState variant, treating as busy"));
        assert!(logs.contains("future-state"));
    }

    #[tokio::test]
    async fn fleet_register_endpoint_accepts_valid_token() {
        let fleet = build_test_fleet();
        let app = fleet_router(fleet.manager.clone());
        let request = Request::builder()
            .method("POST")
            .uri("/fleet/register")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::to_vec(&registration_request(&fleet.token.secret))
                    .expect("request should serialize"),
            ))
            .expect("request should build");

        let response = app.oneshot(request).await.expect("route should respond");
        assert_eq!(response.status(), StatusCode::OK);
        let body: FleetRegistrationResponse = response_json(response).await;

        assert!(body.accepted);
        assert_eq!(body.node_id, fleet.node_id());
        let manager = fleet.manager.lock().await;
        let node = manager
            .list_nodes()
            .into_iter()
            .find(|node| node.node_id == fleet.node_id())
            .expect("node should be present");
        assert_eq!(node.status, NodeStatus::Online);
        assert!(node.last_heartbeat_ms > 0);
        assert!(node.capabilities.contains(&NodeCapability::AgenticLoop));
    }

    #[tokio::test]
    async fn fleet_register_endpoint_rejects_invalid_token() {
        let fleet = build_test_fleet();
        let app = fleet_router(fleet.manager.clone());
        let request = Request::builder()
            .method("POST")
            .uri("/fleet/register")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::to_vec(&registration_request("invalid-token"))
                    .expect("request should serialize"),
            ))
            .expect("request should build");

        let response = app.oneshot(request).await.expect("route should respond");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body: FleetRegistrationResponse = response_json(response).await;

        assert!(!body.accepted);
        assert!(body.node_id.is_empty());
    }

    #[tokio::test]
    async fn fleet_heartbeat_endpoint_updates_node() {
        let fleet = build_test_fleet();
        let app = fleet_router(fleet.manager.clone());
        let request = Request::builder()
            .method("POST")
            .uri("/fleet/heartbeat")
            .header(header::CONTENT_TYPE, "application/json")
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", fleet.token.secret),
            )
            .body(Body::from(
                serde_json::to_vec(&heartbeat_request(fleet.node_id(), WorkerState::Busy))
                    .expect("request should serialize"),
            ))
            .expect("request should build");

        let response = app.oneshot(request).await.expect("route should respond");
        assert_eq!(response.status(), StatusCode::OK);
        let manager = fleet.manager.lock().await;
        let node = manager
            .list_nodes()
            .into_iter()
            .find(|node| node.node_id == fleet.node_id())
            .expect("node should be present");

        assert_eq!(node.status, NodeStatus::Busy);
        assert!(node.last_heartbeat_ms > 0);
    }

    #[tokio::test]
    async fn fleet_heartbeat_endpoint_rejects_mismatched_node_id() {
        let fleet = build_test_fleet();
        {
            let mut manager = fleet.manager.lock().await;
            manager
                .register_worker(fleet.node_id(), vec![NodeCapability::AgenticLoop], 1)
                .expect("worker should register");
        }
        let app = fleet_router(fleet.manager.clone());
        let request = Request::builder()
            .method("POST")
            .uri("/fleet/heartbeat")
            .header(header::CONTENT_TYPE, "application/json")
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", fleet.token.secret),
            )
            .body(Body::from(
                serde_json::to_vec(&heartbeat_request("different-node", WorkerState::Busy))
                    .expect("request should serialize"),
            ))
            .expect("request should build");

        let response = app.oneshot(request).await.expect("route should respond");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn fleet_result_endpoint_rejects_invalid_auth() {
        let fleet = build_test_fleet();
        let app = fleet_router(fleet.manager.clone());
        let request = Request::builder()
            .method("POST")
            .uri("/fleet/result")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, "Bearer invalid-token")
            .body(Body::from(
                serde_json::to_vec(&result_request()).expect("request should serialize"),
            ))
            .expect("request should build");

        let response = app.oneshot(request).await.expect("route should respond");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn fleet_result_endpoint_marks_worker_online() {
        let fleet = build_test_fleet();
        {
            let mut manager = fleet.manager.lock().await;
            manager
                .record_worker_heartbeat(fleet.node_id(), NodeStatus::Busy, 1)
                .expect("heartbeat should persist");
        }
        let app = fleet_router(fleet.manager.clone());
        let request = Request::builder()
            .method("POST")
            .uri("/fleet/result")
            .header(header::CONTENT_TYPE, "application/json")
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", fleet.token.secret),
            )
            .body(Body::from(
                serde_json::to_vec(&result_request()).expect("request should serialize"),
            ))
            .expect("request should build");

        let response = app.oneshot(request).await.expect("route should respond");
        assert_eq!(response.status(), StatusCode::OK);
        let manager = fleet.manager.lock().await;
        let node = manager
            .list_nodes()
            .into_iter()
            .find(|node| node.node_id == fleet.node_id())
            .expect("node should be present");

        assert_eq!(node.status, NodeStatus::Online);
        assert!(node.last_heartbeat_ms > 1);
    }
}
