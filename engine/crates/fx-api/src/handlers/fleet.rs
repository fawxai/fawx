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
