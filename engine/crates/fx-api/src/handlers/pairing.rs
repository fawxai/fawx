use crate::pairing::{PairingCode, PairingError, PairingState};
use crate::state::HttpState;
use crate::types::ErrorBody;
use axum::extract::{Json, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

const DEFAULT_DEVICE_NAME: &str = "Unnamed device";

use super::HandlerResult;

#[derive(Debug, Deserialize)]
pub struct GeneratePairRequest {
    #[serde(default)]
    pub ttl_seconds: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct ExchangePairRequest {
    pub code: String,
    #[serde(default)]
    pub device_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ExchangePairResponse {
    pub token: String,
    pub device_id: String,
}

pub async fn handle_generate_pair(
    State(state): State<HttpState>,
    request: Option<Json<GeneratePairRequest>>,
) -> Json<PairingCode> {
    let mut pairing = state.pairing.lock().await;
    Json(generate_pair_code(&mut pairing, request))
}

fn generate_pair_code(
    pairing: &mut PairingState,
    request: Option<Json<GeneratePairRequest>>,
) -> PairingCode {
    match request.and_then(|Json(request)| request.ttl_seconds) {
        Some(ttl_seconds) => pairing.generate_with_ttl(ttl_seconds),
        None => pairing.generate(),
    }
}

pub async fn handle_exchange_pair(
    State(state): State<HttpState>,
    Json(request): Json<ExchangePairRequest>,
) -> HandlerResult<Json<ExchangePairResponse>> {
    exchange_pairing_code(&state, &request.code).await?;
    let device_name = requested_device_name(request.device_name.as_deref());
    let (token, device_id) = create_and_persist_device(&state, &device_name)
        .await
        .map_err(internal_error)?;
    Ok(Json(ExchangePairResponse { token, device_id }))
}

async fn exchange_pairing_code(state: &HttpState, code: &str) -> HandlerResult<()> {
    let mut pairing = state.pairing.lock().await;
    pairing.exchange(code).map_err(pairing_error_response)
}

async fn create_and_persist_device(
    state: &HttpState,
    device_name: &str,
) -> anyhow::Result<(String, String)> {
    let mut devices = state.devices.lock().await;
    let (token, device) = devices.create_device(device_name);
    if let Some(path) = state.devices_path.as_deref() {
        if let Err(error) = devices.save(path) {
            let _ = devices.revoke(&device.id);
            return Err(error);
        }
    }
    Ok((token, device.id))
}

fn requested_device_name(device_name: Option<&str>) -> String {
    device_name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(DEFAULT_DEVICE_NAME)
        .to_string()
}

fn pairing_error_response(error: PairingError) -> (StatusCode, Json<ErrorBody>) {
    match error {
        PairingError::InvalidCode => error_response(StatusCode::BAD_REQUEST, "invalid_code"),
        PairingError::Expired => error_response(StatusCode::GONE, "expired"),
        PairingError::TooManyAttempts => {
            error_response(StatusCode::TOO_MANY_REQUESTS, "too_many_attempts")
        }
    }
}

fn internal_error(error: anyhow::Error) -> (StatusCode, Json<ErrorBody>) {
    tracing::error!(error = %error, "pairing exchange failed");
    error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
}

fn error_response(status: StatusCode, error: &str) -> (StatusCode, Json<ErrorBody>) {
    (
        status,
        Json(ErrorBody {
            error: error.to_string(),
        }),
    )
}
