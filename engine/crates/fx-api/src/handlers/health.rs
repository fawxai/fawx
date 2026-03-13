use crate::config_redaction;
use crate::engine::AppEngine;
use crate::state::HttpState;
use crate::types::{HealthResponse, StatusResponse};
use axum::extract::State;
use axum::Json;

pub fn sanitize_config(value: serde_json::Value) -> serde_json::Value {
    config_redaction::sanitize_json(value)
}

pub fn sanitized_status_config(app: &dyn AppEngine) -> Option<serde_json::Value> {
    let manager = app.config_manager()?;
    let guard = manager.lock().ok()?;
    let config = guard.get("all").ok()?;
    Some(sanitize_config(config))
}

pub async fn handle_health(State(state): State<HttpState>) -> Json<HealthResponse> {
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

pub async fn handle_status(State(state): State<HttpState>) -> Json<StatusResponse> {
    let app = state.app.lock().await;
    let model = app.active_model().to_string();
    let config = sanitized_status_config(&*app);

    Json(StatusResponse {
        status: "ok",
        model,
        skills: Vec::new(),
        memory_entries: 0,
        tailscale_ip: state.tailscale_ip.clone(),
        config,
    })
}
