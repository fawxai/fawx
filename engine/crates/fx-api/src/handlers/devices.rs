use crate::devices::{DeviceStore, DeviceToken};
use crate::state::HttpState;
use crate::types::ErrorBody;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;
use std::path::Path as FsPath;

use super::HandlerResult;

#[derive(Debug, Serialize)]
pub struct ListDevicesResponse {
    pub devices: Vec<crate::devices::DeviceInfo>,
}

#[derive(Debug, Serialize)]
pub struct RevokeDeviceResponse {
    pub revoked: bool,
    pub device_id: String,
    pub device_name: String,
}

impl From<DeviceToken> for RevokeDeviceResponse {
    fn from(device: DeviceToken) -> Self {
        Self {
            revoked: true,
            device_id: device.id,
            device_name: device.device_name,
        }
    }
}

pub async fn handle_list_devices(State(state): State<HttpState>) -> Json<ListDevicesResponse> {
    let devices = state.devices.lock().await;
    Json(ListDevicesResponse {
        devices: devices.list_device_info(),
    })
}

pub async fn handle_delete_device(
    State(state): State<HttpState>,
    Path(device_id): Path<String>,
) -> HandlerResult<Json<RevokeDeviceResponse>> {
    let response = revoke_device(&state, &device_id)
        .await
        .map_err(internal_error)?
        .ok_or_else(device_not_found_response)?;
    Ok(Json(response))
}

async fn revoke_device(
    state: &HttpState,
    device_id: &str,
) -> anyhow::Result<Option<RevokeDeviceResponse>> {
    let mut devices = state.devices.lock().await;
    let Some(device) = devices.revoke(device_id) else {
        return Ok(None);
    };
    persist_revocation(&mut devices, state.devices_path.as_deref(), &device)?;
    Ok(Some(device.into()))
}

fn persist_revocation(
    devices: &mut DeviceStore,
    path: Option<&FsPath>,
    device: &DeviceToken,
) -> anyhow::Result<()> {
    let Some(path) = path else {
        return Ok(());
    };
    if let Err(error) = devices.save(path) {
        devices.restore_device(device.clone());
        return Err(error);
    }
    Ok(())
}

fn device_not_found_response() -> (StatusCode, Json<ErrorBody>) {
    error_response(StatusCode::NOT_FOUND, "device not found")
}

fn internal_error(error: anyhow::Error) -> (StatusCode, Json<ErrorBody>) {
    tracing::error!(error = %error, "device revocation failed");
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
