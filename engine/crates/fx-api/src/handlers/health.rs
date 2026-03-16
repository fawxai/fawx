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
    let uptime = state.start_time.elapsed().as_secs();
    // Try to read model, but don't block if app is busy (e.g., during experiments).
    // The health endpoint must always respond quickly so the GUI stays connected.
    let model = match state.app.try_lock() {
        Ok(app) => app.active_model().to_string(),
        Err(_) => "(busy)".to_string(),
    };
    Json(HealthResponse {
        status: "ok",
        model,
        uptime_seconds: uptime,
        skills_loaded: 0,
    })
}

pub async fn handle_status(State(state): State<HttpState>) -> Json<StatusResponse> {
    // Try non-blocking lock so status endpoint stays responsive during long operations.
    let (model, config) = match state.app.try_lock() {
        Ok(app) => {
            let model = app.active_model().to_string();
            let config = sanitized_status_config(&*app);
            (model, config)
        }
        Err(_) => ("(busy)".to_string(), None),
    };

    Json(StatusResponse {
        status: "ok",
        model,
        skills: Vec::new(),
        memory_entries: 0,
        tailscale_ip: state.tailscale_ip.clone(),
        config,
    })
}
