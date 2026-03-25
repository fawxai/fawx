use super::HandlerResult;
use crate::state::HttpState;
use crate::types::ErrorBody;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use fx_fleet::{
    current_time_ms, FleetError, FleetHttpClient, FleetManager, FleetTaskRequest, FleetTaskType,
    NodeCapability, NodeInfo, NodeStatus,
};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

use fx_fleet::DEFAULT_STALE_THRESHOLD_MS;

static TASK_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize)]
pub struct FleetOverviewResponse {
    pub total_nodes: usize,
    pub healthy_nodes: usize,
    pub degraded_nodes: usize,
    pub offline_nodes: usize,
    pub active_tasks: usize,
    pub queued_tasks: usize,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct FleetNodeDto {
    pub id: String,
    pub name: String,
    pub status: String,
    pub last_seen_at: u64,
    pub active_tasks: usize,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FleetNodesResponse {
    pub nodes: Vec<FleetNodeDto>,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct FleetNodeDetailResponse {
    pub id: String,
    pub name: String,
    pub status: String,
    pub last_seen_at: u64,
    pub active_tasks: usize,
    pub queued_tasks: usize,
    pub capabilities: Vec<String>,
    pub endpoint: String,
    pub registered_at: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RemoveNodeResponse {
    pub id: String,
    pub removed: bool,
}

#[derive(Debug, Deserialize)]
pub struct DispatchTaskRequest {
    /// Task description. Currently unused (stub), validated by deserialization.
    #[allow(dead_code)]
    pub task: String,
    /// Priority level. Currently unused (stub).
    #[allow(dead_code)]
    #[serde(default = "default_priority")]
    pub priority: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DispatchTaskResponse {
    pub accepted: bool,
    pub node_id: String,
    pub task_id: String,
    pub status: String,
}

pub async fn handle_fleet_overview(
    State(state): State<HttpState>,
) -> HandlerResult<Json<FleetOverviewResponse>> {
    let manager = require_fleet_manager(&state)?;
    let manager = manager.lock().await;
    let now_ms = current_time_ms();
    Ok(Json(build_overview_response(manager.list_nodes(), now_ms)))
}

pub async fn handle_fleet_nodes(
    State(state): State<HttpState>,
) -> HandlerResult<Json<FleetNodesResponse>> {
    let manager = require_fleet_manager(&state)?;
    let manager = manager.lock().await;
    let now_ms = current_time_ms();
    let mut nodes = build_node_dtos(manager.list_nodes(), now_ms);
    nodes.sort_by(|left, right| left.name.cmp(&right.name));
    let total = nodes.len();
    Ok(Json(FleetNodesResponse { nodes, total }))
}

pub async fn handle_fleet_node_detail(
    State(state): State<HttpState>,
    Path(node_id): Path<String>,
) -> HandlerResult<Json<FleetNodeDetailResponse>> {
    let manager = require_fleet_manager(&state)?;
    let manager = manager.lock().await;
    let now_ms = current_time_ms();
    let node = find_node(&manager, &node_id).ok_or_else(node_not_found_response)?;
    Ok(Json(build_node_detail_response(&node, now_ms)))
}

pub async fn handle_remove_fleet_node(
    State(state): State<HttpState>,
    Path(node_id): Path<String>,
) -> HandlerResult<Json<RemoveNodeResponse>> {
    let manager = require_fleet_manager(&state)?;
    let mut manager = manager.lock().await;
    let node_name = find_node_name(&manager, &node_id).ok_or_else(node_not_found_response)?;
    manager.remove_node(&node_name).map_err(remove_node_error)?;
    Ok(Json(RemoveNodeResponse {
        id: node_id,
        removed: true,
    }))
}

pub async fn handle_dispatch_task(
    State(state): State<HttpState>,
    Path(node_id): Path<String>,
    Json(request): Json<DispatchTaskRequest>,
) -> HandlerResult<Json<DispatchTaskResponse>> {
    let manager = require_fleet_manager(&state)?;
    let manager = manager.lock().await;
    let node = find_node(&manager, &node_id).ok_or_else(node_not_found_response)?;
    let endpoint = node.endpoint.clone();
    let bearer = node
        .auth_token
        .clone()
        .ok_or_else(|| error_response(StatusCode::BAD_REQUEST, "Node has no auth token"))?;
    drop(manager);

    let task_id = generate_task_id();
    let fleet_request = build_fleet_task_request(&task_id, &request);
    let client = FleetHttpClient::new(std::time::Duration::from_secs(30));
    client
        .send_task(&endpoint, &bearer, &fleet_request)
        .await
        .map_err(|error| {
            tracing::warn!(node_id = %node_id, error = %error, "fleet task dispatch failed");
            error_response(
                StatusCode::BAD_GATEWAY,
                &format!("Task dispatch failed: {error}"),
            )
        })?;

    Ok(Json(DispatchTaskResponse {
        accepted: true,
        node_id,
        task_id,
        status: "dispatched".to_string(),
    }))
}

fn generate_task_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let counter = TASK_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format_task_id(nanos, counter)
}

fn format_task_id(nanos: u128, counter: u64) -> String {
    format!("task_{nanos:x}_{counter:x}")
}

fn build_fleet_task_request(task_id: &str, request: &DispatchTaskRequest) -> FleetTaskRequest {
    FleetTaskRequest {
        task_id: task_id.to_string(),
        task_type: FleetTaskType::GenerateAndEvaluate,
        // Dashboard dispatch runs against the worker's existing local checkout,
        // so there is no remote repository URL to clone for this stub task.
        repo_url: String::new(),
        // Likewise, the worker uses its current checkout instead of a requested
        // branch until fleet dispatch grows repo-aware execution.
        branch: String::new(),
        git_token: None,
        signal: serde_json::json!({"description": request.task}),
        config: serde_json::json!({"priority": request.priority}),
        chain_history: vec![],
        scope: vec![],
    }
}

fn default_priority() -> String {
    "normal".to_string()
}

fn build_overview_response(nodes: Vec<&NodeInfo>, now_ms: u64) -> FleetOverviewResponse {
    let mut healthy_nodes = 0;
    let mut degraded_nodes = 0;
    let mut offline_nodes = 0;
    for node in nodes.iter().copied() {
        match node_health_status(&effective_node_status(node, now_ms)) {
            "healthy" => healthy_nodes += 1,
            "degraded" => degraded_nodes += 1,
            _ => offline_nodes += 1,
        }
    }
    FleetOverviewResponse {
        total_nodes: nodes.len(),
        healthy_nodes,
        degraded_nodes,
        offline_nodes,
        active_tasks: 0,
        queued_tasks: 0,
        updated_at: now_ms / 1000,
    }
}

fn build_node_dtos(nodes: Vec<&NodeInfo>, now_ms: u64) -> Vec<FleetNodeDto> {
    nodes
        .into_iter()
        .map(|node| FleetNodeDto {
            id: node.node_id.clone(),
            name: node.name.clone(),
            status: node_health_status(&effective_node_status(node, now_ms)).to_string(),
            last_seen_at: node.last_heartbeat_ms / 1000,
            active_tasks: 0,
            capabilities: capability_strings(&node.capabilities),
        })
        .collect()
}

fn build_node_detail_response(node: &NodeInfo, now_ms: u64) -> FleetNodeDetailResponse {
    FleetNodeDetailResponse {
        id: node.node_id.clone(),
        name: node.name.clone(),
        status: node_health_status(&effective_node_status(node, now_ms)).to_string(),
        last_seen_at: node.last_heartbeat_ms / 1000,
        active_tasks: 0,
        queued_tasks: 0,
        capabilities: capability_strings(&node.capabilities),
        endpoint: node.endpoint.clone(),
        registered_at: node.registered_at_ms / 1000,
    }
}

fn capability_strings(capabilities: &[NodeCapability]) -> Vec<String> {
    capabilities.iter().map(capability_str).collect()
}

fn find_node(manager: &FleetManager, node_id: &str) -> Option<NodeInfo> {
    manager
        .list_nodes()
        .into_iter()
        .find(|node| node.node_id == node_id)
        .cloned()
}

fn find_node_name(manager: &FleetManager, node_id: &str) -> Option<String> {
    find_node(manager, node_id).map(|node| node.name)
}

fn effective_node_status(node: &NodeInfo, now_ms: u64) -> NodeStatus {
    if node_is_stale(node, now_ms) {
        NodeStatus::Stale
    } else {
        node.status.clone()
    }
}

fn node_is_stale(node: &NodeInfo, now_ms: u64) -> bool {
    matches!(node.status, NodeStatus::Online | NodeStatus::Busy)
        && now_ms.saturating_sub(node.last_heartbeat_ms) > DEFAULT_STALE_THRESHOLD_MS
}

fn require_fleet_manager(state: &HttpState) -> HandlerResult<Arc<Mutex<FleetManager>>> {
    state
        .fleet_manager
        .clone()
        .ok_or_else(|| error_response(StatusCode::SERVICE_UNAVAILABLE, "Fleet not initialized"))
}

fn node_not_found_response() -> (StatusCode, Json<ErrorBody>) {
    error_response(StatusCode::NOT_FOUND, "node not found")
}

fn remove_node_error(error: FleetError) -> (StatusCode, Json<ErrorBody>) {
    tracing::error!(error = %error, "fleet node removal failed");
    error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
}

fn error_response(status: StatusCode, message: &str) -> (StatusCode, Json<ErrorBody>) {
    (
        status,
        Json(ErrorBody {
            error: message.to_string(),
        }),
    )
}

fn node_health_status(status: &NodeStatus) -> &'static str {
    match status {
        NodeStatus::Online | NodeStatus::Busy => "healthy",
        NodeStatus::Stale => "degraded",
        NodeStatus::Offline => "offline",
    }
}

fn capability_str(capability: &NodeCapability) -> String {
    match capability {
        NodeCapability::AgenticLoop => "agentic_loop".to_string(),
        NodeCapability::SkillBuild => "skill_build".to_string(),
        NodeCapability::SkillExecute => "skill_execute".to_string(),
        NodeCapability::Network => "network".to_string(),
        NodeCapability::GpuCompute => "gpu_compute".to_string(),
        NodeCapability::Custom(name) => name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn overview_response_serializes() {
        let response = FleetOverviewResponse {
            total_nodes: 3,
            healthy_nodes: 2,
            degraded_nodes: 1,
            offline_nodes: 0,
            active_tasks: 0,
            queued_tasks: 0,
            updated_at: 1_742_000_000,
        };

        let json = serde_json::to_value(response).expect("overview response should serialize");

        assert_eq!(
            json,
            json!({
                "total_nodes": 3,
                "healthy_nodes": 2,
                "degraded_nodes": 1,
                "offline_nodes": 0,
                "active_tasks": 0,
                "queued_tasks": 0,
                "updated_at": 1_742_000_000,
            })
        );
    }

    #[test]
    fn node_dto_serializes() {
        let response = FleetNodeDto {
            id: "node-1".to_string(),
            name: "Worker Node A".to_string(),
            status: "healthy".to_string(),
            last_seen_at: 1_742_000_100,
            active_tasks: 0,
            capabilities: vec!["agentic_loop".to_string(), "network".to_string()],
        };

        let json = serde_json::to_value(response).expect("node dto should serialize");

        assert_eq!(
            json,
            json!({
                "id": "node-1",
                "name": "Worker Node A",
                "status": "healthy",
                "last_seen_at": 1_742_000_100,
                "active_tasks": 0,
                "capabilities": ["agentic_loop", "network"],
            })
        );
    }

    #[test]
    fn node_health_status_maps_correctly() {
        assert_eq!(node_health_status(&NodeStatus::Online), "healthy");
        assert_eq!(node_health_status(&NodeStatus::Busy), "healthy");
        assert_eq!(node_health_status(&NodeStatus::Stale), "degraded");
        assert_eq!(node_health_status(&NodeStatus::Offline), "offline");
    }

    #[test]
    fn capability_str_maps_all_variants() {
        assert_eq!(capability_str(&NodeCapability::AgenticLoop), "agentic_loop");
        assert_eq!(capability_str(&NodeCapability::SkillBuild), "skill_build");
        assert_eq!(
            capability_str(&NodeCapability::SkillExecute),
            "skill_execute"
        );
        assert_eq!(capability_str(&NodeCapability::Network), "network");
        assert_eq!(capability_str(&NodeCapability::GpuCompute), "gpu_compute");
        assert_eq!(
            capability_str(&NodeCapability::Custom("custom-cap".to_string())),
            "custom-cap"
        );
    }

    #[test]
    fn dispatch_request_deserializes() {
        let request: DispatchTaskRequest = serde_json::from_value(json!({
            "task": "Run task",
            "priority": "high",
        }))
        .expect("dispatch request should deserialize");

        assert_eq!(request.task, "Run task");
        assert_eq!(request.priority, "high");
    }

    #[test]
    fn dispatch_request_default_priority() {
        let request: DispatchTaskRequest = serde_json::from_value(json!({
            "task": "Run task",
        }))
        .expect("dispatch request should deserialize");

        assert_eq!(request.task, "Run task");
        assert_eq!(request.priority, "normal");
    }

    #[test]
    fn effective_status_marks_old_busy_nodes_degraded() {
        let node = NodeInfo {
            node_id: "node-1".to_string(),
            name: "Worker Node A".to_string(),
            endpoint: "https://127.0.0.1:8400".to_string(),
            auth_token: None,
            capabilities: vec![NodeCapability::AgenticLoop],
            status: NodeStatus::Busy,
            last_heartbeat_ms: 1_000,
            registered_at_ms: 2_000,
            address: None,
            ssh_user: None,
            ssh_key: None,
        };

        assert_eq!(effective_node_status(&node, 62_000), NodeStatus::Stale);
    }

    #[test]
    fn node_not_found_response_returns_404() {
        let (status, body) = node_not_found_response();
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body.0.error, "node not found");
    }

    #[test]
    fn error_response_returns_correct_status() {
        let (status, body) =
            error_response(StatusCode::SERVICE_UNAVAILABLE, "Fleet not initialized");
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body.0.error, "Fleet not initialized");
    }

    #[test]
    fn dispatch_response_serializes() {
        let response = DispatchTaskResponse {
            accepted: true,
            node_id: "node-1".to_string(),
            task_id: "task_abc".to_string(),
            status: "dispatched".to_string(),
        };
        let json = serde_json::to_value(response).expect("serialize");
        assert_eq!(json["accepted"], true);
        assert_eq!(json["task_id"], "task_abc");
        assert_eq!(json["status"], "dispatched");
    }

    #[test]
    fn build_fleet_task_request_maps_fields() {
        let request = DispatchTaskRequest {
            task: "Run experiment".to_string(),
            priority: "high".to_string(),
        };
        let fleet_req = build_fleet_task_request("task_123", &request);
        assert_eq!(fleet_req.task_id, "task_123");
        assert!(fleet_req.repo_url.is_empty());
        assert!(fleet_req.branch.is_empty());
        assert_eq!(fleet_req.signal["description"], "Run experiment");
        assert_eq!(fleet_req.config["priority"], "high");
    }

    #[test]
    fn format_task_id_uses_counter_suffix() {
        assert_eq!(format_task_id(0xabc, 0x1), "task_abc_1");
        assert_ne!(format_task_id(0xabc, 0x1), format_task_id(0xabc, 0x2));
    }
}
