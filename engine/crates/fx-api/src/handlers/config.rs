use crate::state::HttpState;
use crate::types::ErrorBody;
use axum::extract::{Json, Query, State};
use axum::http::StatusCode;
use fx_tools::ConfigSetRequest;
use std::collections::HashMap;

pub async fn handle_config_get(
    State(state): State<HttpState>,
    query: Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorBody>)> {
    let section = query.get("section").map(|s| s.as_str()).unwrap_or("all");
    let app = state.app.lock().await;
    let mgr = app.config_manager().ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                error: "config manager not available".to_string(),
            }),
        )
    })?;
    let guard = mgr.lock().map_err(|error| {
        tracing::error!(error = %error, "config manager lock failed");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody {
                error: "internal_error".to_string(),
            }),
        )
    })?;
    let value = guard
        .get(section)
        .map_err(|error| (StatusCode::BAD_REQUEST, Json(ErrorBody { error })))?;
    Ok(Json(value))
}

pub async fn handle_config_set(
    State(state): State<HttpState>,
    Json(request): Json<ConfigSetRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorBody>)> {
    let app = state.app.lock().await;
    let mgr = app.config_manager().ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                error: "config manager not available".to_string(),
            }),
        )
    })?;
    let mut guard = mgr.lock().map_err(|error| {
        tracing::error!(error = %error, "config manager lock failed");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody {
                error: "internal_error".to_string(),
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
