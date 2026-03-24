use crate::engine::ConfigManagerHandle;
use crate::state::HttpState;
use crate::types::ErrorBody;
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use super::HandlerResult;

/// In-memory version tracking for optimistic concurrency.
pub struct SynthesisState {
    version: AtomicU64,
    updated_at: Mutex<Option<u64>>,
}

impl SynthesisState {
    pub fn new(has_initial_value: bool) -> Self {
        let (version, updated_at) = if has_initial_value {
            (1, Some(unix_now()))
        } else {
            (0, None)
        };
        Self {
            version: AtomicU64::new(version),
            updated_at: Mutex::new(updated_at),
        }
    }

    pub fn current_version(&self) -> u64 {
        self.version.load(Ordering::SeqCst)
    }

    pub fn bump(&self) -> u64 {
        let new_version = self.version.fetch_add(1, Ordering::SeqCst) + 1;
        if let Ok(mut guard) = self.updated_at.lock() {
            *guard = Some(unix_now());
        }
        new_version
    }

    pub fn bump_cleared(&self) -> u64 {
        let new_version = self.version.fetch_add(1, Ordering::SeqCst) + 1;
        if let Ok(mut guard) = self.updated_at.lock() {
            *guard = None;
        }
        new_version
    }

    pub fn updated_at(&self) -> Option<u64> {
        self.updated_at.lock().ok().and_then(|guard| *guard)
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[derive(Debug, Clone, Serialize)]
pub struct SynthesisResponse {
    pub synthesis: Option<String>,
    pub updated_at: Option<u64>,
    pub source: &'static str,
    pub version: u64,
    pub max_length: usize,
}

#[derive(Debug, Deserialize)]
pub struct SetSynthesisRequest {
    pub synthesis: String,
    #[serde(default)]
    pub version: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SetSynthesisResponse {
    pub updated: bool,
    pub synthesis: String,
    pub updated_at: u64,
    pub version: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClearSynthesisResponse {
    pub cleared: bool,
    pub version: u64,
}

pub async fn handle_get_synthesis(
    State(state): State<HttpState>,
) -> HandlerResult<Json<SynthesisResponse>> {
    let synthesis = read_synthesis(&state).await?;
    Ok(Json(SynthesisResponse {
        synthesis,
        updated_at: state.synthesis.updated_at(),
        source: "config",
        version: state.synthesis.current_version(),
        max_length: fx_config::MAX_SYNTHESIS_INSTRUCTION_LENGTH,
    }))
}

pub async fn handle_set_synthesis(
    State(state): State<HttpState>,
    Json(request): Json<SetSynthesisRequest>,
) -> HandlerResult<Json<SetSynthesisResponse>> {
    validate_synthesis_value(&request.synthesis)?;
    check_version_match(&state, request.version)?;
    update_synthesis(&state, &request.synthesis).await?;

    let new_version = state.synthesis.bump();
    let updated_at = state.synthesis.updated_at().unwrap_or_else(unix_now);
    Ok(Json(SetSynthesisResponse {
        updated: true,
        synthesis: request.synthesis,
        updated_at,
        version: new_version,
    }))
}

pub async fn handle_clear_synthesis(
    State(state): State<HttpState>,
) -> HandlerResult<Json<ClearSynthesisResponse>> {
    clear_synthesis(&state).await?;
    let new_version = state.synthesis.bump_cleared();
    Ok(Json(ClearSynthesisResponse {
        cleared: true,
        version: new_version,
    }))
}

async fn read_synthesis(state: &HttpState) -> HandlerResult<Option<String>> {
    let manager = config_manager_handle(state).await?;
    let guard = manager.lock().map_err(config_lock_error)?;
    Ok(guard.config().model.synthesis_instruction.clone())
}

async fn update_synthesis(state: &HttpState, value: &str) -> HandlerResult<()> {
    let manager = config_manager_handle(state).await?;
    let mut guard = manager.lock().map_err(config_lock_error)?;
    guard
        .set("model.synthesis_instruction", value)
        .map_err(bad_request)
}

async fn clear_synthesis(state: &HttpState) -> HandlerResult<()> {
    let manager = config_manager_handle(state).await?;
    let mut guard = manager.lock().map_err(config_lock_error)?;
    guard
        .clear("model.synthesis_instruction")
        .map_err(bad_request)
}

async fn config_manager_handle(state: &HttpState) -> HandlerResult<ConfigManagerHandle> {
    let app = state.app.lock().await;
    app.config_manager().ok_or_else(config_manager_missing)
}

fn validate_synthesis_value(value: &str) -> Result<(), (StatusCode, Json<ErrorBody>)> {
    fx_config::validate_synthesis_instruction(value)
        .map_err(|error| validation_error(StatusCode::UNPROCESSABLE_ENTITY, error))
}

fn check_version_match(
    state: &HttpState,
    requested: Option<u64>,
) -> Result<(), (StatusCode, Json<ErrorBody>)> {
    if let Some(expected) = requested {
        let current = state.synthesis.current_version();
        // Note: version check and subsequent write are not atomic. Concurrent PUTs
        // with the same version could both pass. Acceptable for single-user engine.
        if current != expected {
            return Err(validation_error(
                StatusCode::CONFLICT,
                format!("Version mismatch: expected {current}, got {expected}"),
            ));
        }
    }
    Ok(())
}

fn config_manager_missing() -> (StatusCode, Json<ErrorBody>) {
    validation_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "Config manager not available".to_string(),
    )
}

fn config_lock_error<T>(_: std::sync::PoisonError<T>) -> (StatusCode, Json<ErrorBody>) {
    validation_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "Config lock poisoned".to_string(),
    )
}

fn bad_request(msg: String) -> (StatusCode, Json<ErrorBody>) {
    validation_error(StatusCode::BAD_REQUEST, msg)
}

fn validation_error(status: StatusCode, error: String) -> (StatusCode, Json<ErrorBody>) {
    (status, Json(ErrorBody { error }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthesis_response_serializes_with_value() {
        let response = SynthesisResponse {
            synthesis: Some("Be concise".into()),
            updated_at: Some(1_700_000_000),
            source: "config",
            version: 1,
            max_length: 500,
        };

        let json = serde_json::to_value(response).unwrap();
        assert_eq!(json["synthesis"], "Be concise");
        assert_eq!(json["version"], 1);
        assert_eq!(json["max_length"], 500);
    }

    #[test]
    fn synthesis_response_serializes_null_when_unset() {
        let response = SynthesisResponse {
            synthesis: None,
            updated_at: None,
            source: "config",
            version: 0,
            max_length: 500,
        };

        let json = serde_json::to_value(response).unwrap();
        assert!(json["synthesis"].is_null());
        assert!(json["updated_at"].is_null());
    }

    #[test]
    fn set_request_deserializes_with_version() {
        let json = r#"{"synthesis": "test", "version": 3}"#;
        let request: SetSynthesisRequest = serde_json::from_str(json).unwrap();

        assert_eq!(request.synthesis, "test");
        assert_eq!(request.version, Some(3));
    }

    #[test]
    fn set_request_deserializes_without_version() {
        let json = r#"{"synthesis": "test"}"#;
        let request: SetSynthesisRequest = serde_json::from_str(json).unwrap();

        assert_eq!(request.version, None);
    }

    #[test]
    fn clear_response_serializes() {
        let response = ClearSynthesisResponse {
            cleared: true,
            version: 5,
        };

        let json = serde_json::to_value(response).unwrap();
        assert_eq!(json["cleared"], true);
        assert_eq!(json["version"], 5);
    }

    #[test]
    fn state_starts_at_zero_when_unset() {
        let state = SynthesisState::new(false);

        assert_eq!(state.current_version(), 0);
        assert!(state.updated_at().is_none());
    }

    #[test]
    fn state_starts_at_one_when_set() {
        let state = SynthesisState::new(true);

        assert_eq!(state.current_version(), 1);
        assert!(state.updated_at().is_some());
    }

    #[test]
    fn bump_increments_version() {
        let state = SynthesisState::new(false);

        assert_eq!(state.bump(), 1);
        assert_eq!(state.bump(), 2);
        assert_eq!(state.current_version(), 2);
    }

    #[test]
    fn bump_cleared_resets_timestamp() {
        let state = SynthesisState::new(true);

        assert!(state.updated_at().is_some());
        assert_eq!(state.bump_cleared(), 2);
        assert_eq!(state.current_version(), 2);
        assert!(state.updated_at().is_none());
    }

    #[test]
    fn set_response_serializes() {
        let response = SetSynthesisResponse {
            updated: true,
            synthesis: "new value".into(),
            updated_at: 1_700_000_000,
            version: 2,
        };

        let json = serde_json::to_value(response).unwrap();
        assert_eq!(json["updated"], true);
        assert_eq!(json["synthesis"], "new value");
    }
}
