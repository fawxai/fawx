use crate::config_redaction;
use crate::engine::ConfigManagerHandle;
use crate::state::HttpState;
use crate::types::{HealthResponse, StatusResponse};
use axum::extract::State;
use axum::Json;

pub fn sanitize_config(value: serde_json::Value) -> serde_json::Value {
    config_redaction::sanitize_json(value)
}

pub fn sanitized_status_config(manager: Option<&ConfigManagerHandle>) -> Option<serde_json::Value> {
    let guard = manager?.lock().ok()?;
    let config = guard.get("all").ok()?;
    Some(sanitize_config(config))
}

pub async fn handle_health(State(state): State<HttpState>) -> Json<HealthResponse> {
    let uptime = state.start_time.elapsed().as_secs();
    let snap = state.shared.read().await;
    Json(HealthResponse {
        status: "ok",
        model: snap.active_model,
        uptime_seconds: uptime,
        skills_loaded: 0,
        https_enabled: state.server_runtime.https_enabled,
    })
}

pub async fn handle_status(State(state): State<HttpState>) -> Json<StatusResponse> {
    let snap = state.shared.read().await;
    let config = sanitized_status_config(state.config_manager.as_ref());

    Json(StatusResponse {
        status: "ok",
        model: snap.active_model,
        skills: Vec::new(),
        memory_entries: 0,
        tailscale_ip: state.tailscale_ip.clone(),
        config,
    })
}
