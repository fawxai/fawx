use crate::pairing::{PairingCode, PairingError, PairingState};
use crate::state::HttpState;
use crate::types::{ErrorBody, QrPairingResponse, TailscaleCertRequest, TailscaleCertResponse};
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

pub async fn handle_qr_pairing(State(state): State<HttpState>) -> Json<QrPairingResponse> {
    let runtime = &state.server_runtime;
    let host = runtime.host.clone();
    let port = runtime.port;
    let transport = if runtime.https_enabled {
        "tailscale_https"
    } else {
        "lan_http"
    };
    let scheme_url = format!("fawx://connect?host={host}&port={port}&token=REDACTED");
    Json(QrPairingResponse {
        scheme_url,
        display_host: host,
        port,
        transport: transport.to_string(),
        same_network_only: !runtime.https_enabled,
    })
}

pub async fn handle_tailscale_cert(
    Json(request): Json<TailscaleCertRequest>,
) -> HandlerResult<Json<TailscaleCertResponse>> {
    let hostname = request.hostname;
    let result = tokio::task::spawn_blocking({
        let hostname = hostname.clone();
        move || run_tailscale_cert(&hostname)
    })
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody {
                error: e.to_string(),
            }),
        )
    })?
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody { error: e }),
        )
    })?;
    Ok(Json(result))
}

fn run_tailscale_cert(hostname: &str) -> Result<TailscaleCertResponse, String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let cert_dir = format!("{home}/.fawx/tls");
    std::fs::create_dir_all(&cert_dir).map_err(|e| format!("failed to create TLS dir: {e}"))?;
    let cert_path = format!("{cert_dir}/cert.pem");
    let key_path = format!("{cert_dir}/key.pem");
    let output = std::process::Command::new("tailscale")
        .args([
            "cert",
            "--cert-file",
            &cert_path,
            "--key-file",
            &key_path,
            hostname,
        ])
        .output()
        .map_err(|e| format!("failed to run tailscale cert: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("tailscale cert failed: {stderr}"));
    }
    Ok(TailscaleCertResponse {
        success: true,
        hostname: hostname.to_string(),
        cert_path,
        key_path,
        https_enabled: true,
    })
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

#[cfg(test)]
mod phase4_tests {
    use super::*;

    #[test]
    fn qr_response_serializes() {
        let r = QrPairingResponse {
            scheme_url: "fawx://connect?host=test&port=8400&token=REDACTED".into(),
            display_host: "test.ts.net".into(),
            port: 8400,
            transport: "tailscale_https".into(),
            same_network_only: false,
        };
        let json = serde_json::to_value(r).unwrap();
        assert_eq!(json["transport"], "tailscale_https");
        assert_eq!(json["same_network_only"], false);
    }

    #[test]
    fn cert_response_serializes() {
        let r = TailscaleCertResponse {
            success: true,
            hostname: "test.ts.net".into(),
            cert_path: "/tmp/cert.pem".into(),
            key_path: "/tmp/key.pem".into(),
            https_enabled: true,
        };
        let json = serde_json::to_value(r).unwrap();
        assert_eq!(json["success"], true);
    }
}
