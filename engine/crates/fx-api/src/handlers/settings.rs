use crate::state::HttpState;
use crate::types::{ErrorBody, SetModelRequest, SetThinkingRequest};
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};

pub async fn handle_list_models(State(state): State<HttpState>) -> Json<Value> {
    let app = state.app.lock().await;
    let models = app.available_models();
    let active_model = app.active_model().to_string();
    Json(json!({
        "active_model": active_model,
        "models": models,
    }))
}

pub async fn handle_set_model(
    State(state): State<HttpState>,
    Json(request): Json<SetModelRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorBody>)> {
    let mut app = state.app.lock().await;
    let previous_model = app.active_model().to_string();
    let active_model = app.set_active_model(&request.model).map_err(bad_request)?;
    Ok(Json(json!({
        "previous_model": previous_model,
        "active_model": active_model,
    })))
}

pub async fn handle_get_thinking(State(state): State<HttpState>) -> Json<Value> {
    let app = state.app.lock().await;
    Json(json!(app.thinking_level()))
}

pub async fn handle_set_thinking(
    State(state): State<HttpState>,
    Json(request): Json<SetThinkingRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorBody>)> {
    let mut app = state.app.lock().await;
    let previous = app.thinking_level();
    let updated = app
        .set_thinking_level(&request.level)
        .map_err(bad_request)?;
    Ok(Json(json!({
        "previous_level": previous.level,
        "level": updated.level,
        "budget_tokens": updated.budget_tokens,
    })))
}

pub async fn handle_list_skills(State(state): State<HttpState>) -> Json<Value> {
    let app = state.app.lock().await;
    let skills = app.skill_summaries();
    let total = skills.len();
    Json(json!({
        "skills": skills,
        "total": total,
    }))
}

pub async fn handle_list_auth(State(state): State<HttpState>) -> Json<Value> {
    let app = state.app.lock().await;
    Json(json!({
        "providers": app.auth_provider_statuses(),
    }))
}

fn bad_request(error: anyhow::Error) -> (StatusCode, Json<ErrorBody>) {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorBody {
            error: error.to_string(),
        }),
    )
}
