use crate::pairing::{PairingCode, PairingError, PairingState};
use crate::server_runtime::ServerRuntime;
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
    Json(qr_pairing_response(&state.server_runtime))
}

fn qr_pairing_response(runtime: &ServerRuntime) -> QrPairingResponse {
    let (transport, same_network_only) = qr_transport(runtime.https_enabled);
    let host = runtime.host.clone();
    let port = runtime.port;
    let scheme_url = format!("fawx://connect?host={host}&port={port}&token=REDACTED");
    QrPairingResponse {
        scheme_url,
        display_host: host,
        port,
        transport: transport.to_string(),
        same_network_only,
    }
}

fn qr_transport(https_enabled: bool) -> (&'static str, bool) {
    if https_enabled {
        ("tailscale_https", false)
    } else {
        ("lan_http", true)
    }
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
    run_tailscale_cert_with(
        hostname,
        || std::env::var("HOME").map_err(|_| "HOME environment variable is not set".to_string()),
        run_tailscale_command,
    )
}

fn run_tailscale_cert_with<GetHome, RunCommand>(
    hostname: &str,
    get_home: GetHome,
    run_command: RunCommand,
) -> Result<TailscaleCertResponse, String>
where
    GetHome: FnOnce() -> Result<String, String>,
    RunCommand: FnOnce(&str, &str, &str) -> Result<TailscaleCommandResult, String>,
{
    validate_hostname(hostname)?;
    let home = get_home()?;
    let cert_dir = format!("{home}/.fawx/tls");
    std::fs::create_dir_all(&cert_dir).map_err(|e| format!("failed to create TLS dir: {e}"))?;

    let cert_path = format!("{cert_dir}/cert.pem");
    let key_path = format!("{cert_dir}/key.pem");
    let result = run_command(&cert_path, &key_path, hostname)?;
    if !result.success {
        return Err(tailscale_error_message(&result.stderr));
    }

    Ok(TailscaleCertResponse {
        success: true,
        hostname: hostname.to_string(),
        cert_path,
        key_path,
        https_enabled: true,
    })
}

#[derive(Debug)]
struct TailscaleCommandResult {
    success: bool,
    stderr: String,
}

fn run_tailscale_command(
    cert_path: &str,
    key_path: &str,
    hostname: &str,
) -> Result<TailscaleCommandResult, String> {
    let output = std::process::Command::new("tailscale")
        .args([
            "cert",
            "--cert-file",
            cert_path,
            "--key-file",
            key_path,
            "--",
            hostname,
        ])
        .output()
        .map_err(tailscale_command_error)?;
    Ok(TailscaleCommandResult {
        success: output.status.success(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
    })
}

fn tailscale_command_error(error: std::io::Error) -> String {
    if error.kind() == std::io::ErrorKind::NotFound {
        "Tailscale CLI not found. Install Tailscale and ensure 'tailscale' is in PATH.".to_string()
    } else {
        format!("failed to run tailscale cert: {error}")
    }
}

fn tailscale_error_message(stderr: &str) -> String {
    if stderr.contains("not logged in") {
        "Tailscale is not logged in. Run 'tailscale login' first.".to_string()
    } else if stderr.contains("HTTPS certificates are not available") {
        "HTTPS certificates are not enabled for this tailnet.".to_string()
    } else {
        "Tailscale certificate generation failed. Check 'tailscale status' for details.".to_string()
    }
}

fn validate_hostname(hostname: &str) -> Result<(), String> {
    if hostname.is_empty() || hostname.contains(' ') {
        return Err("Invalid hostname".to_string());
    }
    Ok(())
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
    use crate::server_runtime::{RestartController, ServerRuntime};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

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
    fn qr_pairing_uses_https_transport_when_https_enabled() {
        let response = qr_pairing_response(&test_runtime(true));
        assert_eq!(response.transport, "tailscale_https");
        assert!(!response.same_network_only);
    }

    #[test]
    fn qr_pairing_uses_lan_transport_when_https_disabled() {
        let response = qr_pairing_response(&test_runtime(false));
        assert_eq!(response.transport, "lan_http");
        assert!(response.same_network_only);
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

    #[test]
    fn run_tailscale_cert_returns_error_when_home_missing() {
        let result = run_tailscale_cert_with(
            "test.ts.net",
            || Err("HOME environment variable is not set".to_string()),
            |_, _, _| panic!("tailscale command should not run without HOME"),
        );
        assert_eq!(
            result.expect_err("missing HOME should fail"),
            "HOME environment variable is not set"
        );
    }

    #[test]
    fn run_tailscale_cert_returns_error_when_tailscale_missing() {
        let home = test_home_path();
        let result = run_tailscale_cert_with(
            "test.ts.net",
            || Ok(home.display().to_string()),
            |_, _, _| {
                Err(
                    "Tailscale CLI not found. Install Tailscale and ensure 'tailscale' is in PATH."
                        .to_string(),
                )
            },
        );
        cleanup_test_home(&home);
        assert_eq!(
            result.expect_err("missing tailscale should fail"),
            "Tailscale CLI not found. Install Tailscale and ensure 'tailscale' is in PATH."
        );
    }

    #[test]
    fn run_tailscale_cert_returns_actionable_error_for_login_failure() {
        let home = test_home_path();
        let result = run_tailscale_cert_with(
            "test.ts.net",
            || Ok(home.display().to_string()),
            |_, _, _| {
                Ok(TailscaleCommandResult {
                    success: false,
                    stderr: "not logged in".to_string(),
                })
            },
        );
        cleanup_test_home(&home);
        assert_eq!(
            result.expect_err("login failure should be mapped"),
            "Tailscale is not logged in. Run 'tailscale login' first."
        );
    }

    #[test]
    fn tailscale_error_message_maps_https_disabled_failure() {
        assert_eq!(
            tailscale_error_message("HTTPS certificates are not available for this tailnet"),
            "HTTPS certificates are not enabled for this tailnet."
        );
    }

    #[test]
    fn run_tailscale_cert_rejects_invalid_hostnames() {
        let result = run_tailscale_cert_with(
            "invalid host",
            || Ok(test_home_path().display().to_string()),
            |_, _, _| panic!("tailscale command should not run for invalid hostnames"),
        );
        assert_eq!(
            result.expect_err("invalid hostname should fail"),
            "Invalid hostname"
        );
    }

    fn test_runtime(https_enabled: bool) -> ServerRuntime {
        ServerRuntime::new(
            "test.ts.net",
            8400,
            https_enabled,
            RestartController::live(),
        )
    }

    fn test_home_path() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("fawx-pairing-tests-{nanos}"))
    }

    fn cleanup_test_home(home: &PathBuf) {
        let _ = std::fs::remove_dir_all(home);
    }
}
