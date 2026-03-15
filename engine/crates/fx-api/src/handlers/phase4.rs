use crate::server_runtime::RestartAction;
use crate::state::HttpState;
use crate::types::{
    ErrorBody, ServerRestartResponse, ServerStatusResponse, SetupAuthStatus, SetupStatusResponse,
    SetupTailscaleStatus,
};
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use fx_config::FawxConfig;
use serde_json::Value;
use std::io::ErrorKind;
use std::process::Command;

use super::HandlerResult;

const LOCAL_MODE: &str = "local";
const READY_STATUS: &str = "ok";

pub async fn handle_setup_status(State(state): State<HttpState>) -> Json<SetupStatusResponse> {
    let auth = setup_auth_status(&state).await;
    let has_valid_config = has_valid_config(&state);
    Json(SetupStatusResponse {
        mode: LOCAL_MODE.to_string(),
        setup_complete: has_valid_config && auth.bearer_token_present,
        has_valid_config,
        server_running: true,
        auth,
        tailscale: detect_tailscale_status(),
    })
}

pub async fn handle_server_status(State(state): State<HttpState>) -> Json<ServerStatusResponse> {
    let runtime = &state.server_runtime;
    Json(ServerStatusResponse {
        status: READY_STATUS.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_seconds: state.start_time.elapsed().as_secs(),
        pid: std::process::id(),
        host: runtime.host.clone(),
        port: runtime.port,
        https_enabled: runtime.https_enabled,
    })
}

pub async fn handle_server_restart(
    State(state): State<HttpState>,
) -> HandlerResult<Json<ServerRestartResponse>> {
    let action = state
        .server_runtime
        .request_restart()
        .map_err(restart_error)?;
    Ok(Json(server_restart_response(action)))
}

async fn setup_auth_status(state: &HttpState) -> SetupAuthStatus {
    let app = state.app.lock().await;
    SetupAuthStatus {
        bearer_token_present: !state.bearer_token.is_empty(),
        providers_configured: app
            .auth_provider_statuses()
            .into_iter()
            .map(|status| status.provider)
            .collect(),
    }
}

fn has_valid_config(state: &HttpState) -> bool {
    let config_path = state.data_dir.join("config.toml");
    config_path.is_file() && FawxConfig::load(&state.data_dir).is_ok()
}

fn detect_tailscale_status() -> SetupTailscaleStatus {
    match probe_tailscale_status() {
        TailscaleProbe::NotInstalled => SetupTailscaleStatus::default(),
        TailscaleProbe::Unavailable => SetupTailscaleStatus {
            installed: true,
            ..SetupTailscaleStatus::default()
        },
        TailscaleProbe::Ready(json) => build_tailscale_status(&json),
    }
}

fn build_tailscale_status(json: &Value) -> SetupTailscaleStatus {
    let backend_state = json
        .get("BackendState")
        .and_then(Value::as_str)
        .unwrap_or_default();
    SetupTailscaleStatus {
        installed: true,
        running: is_tailscale_running(backend_state),
        logged_in: backend_state == "Running",
        hostname: tailscale_hostname(json),
        cert_ready: tailscale_cert_ready(json),
    }
}

fn is_tailscale_running(backend_state: &str) -> bool {
    !backend_state.is_empty() && backend_state != "NoState"
}

fn tailscale_hostname(json: &Value) -> Option<String> {
    let hostname = json.pointer("/Self/DNSName").and_then(Value::as_str)?;
    let hostname = hostname.trim_end_matches('.');
    if hostname.is_empty() {
        None
    } else {
        Some(hostname.to_string())
    }
}

fn tailscale_cert_ready(json: &Value) -> bool {
    json.get("CertDomains")
        .and_then(Value::as_array)
        .is_some_and(|domains| !domains.is_empty())
}

fn probe_tailscale_status() -> TailscaleProbe {
    match Command::new("tailscale")
        .args(["status", "--json"])
        .output()
    {
        Err(error) if error.kind() == ErrorKind::NotFound => TailscaleProbe::NotInstalled,
        Err(_) => TailscaleProbe::Unavailable,
        Ok(output) if !output.status.success() => TailscaleProbe::Unavailable,
        Ok(output) => match serde_json::from_slice::<Value>(&output.stdout) {
            Ok(json) => TailscaleProbe::Ready(json),
            Err(_) => TailscaleProbe::Unavailable,
        },
    }
}

fn server_restart_response(action: RestartAction) -> ServerRestartResponse {
    ServerRestartResponse {
        accepted: true,
        restart_via: action.restart_via.to_string(),
        message: action.message.to_string(),
    }
}

fn restart_error(error: String) -> (StatusCode, Json<ErrorBody>) {
    tracing::error!(error = %error, "server restart request failed");
    (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorBody { error }))
}

enum TailscaleProbe {
    NotInstalled,
    Unavailable,
    Ready(Value),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_status_response_serializes_expected_shape() {
        let response = SetupStatusResponse {
            mode: "local".to_string(),
            setup_complete: true,
            has_valid_config: true,
            server_running: true,
            auth: SetupAuthStatus {
                bearer_token_present: true,
                providers_configured: vec!["anthropic".to_string()],
            },
            tailscale: SetupTailscaleStatus {
                installed: true,
                running: true,
                logged_in: true,
                hostname: Some("joes-mac.tail1234.ts.net".to_string()),
                cert_ready: true,
            },
        };

        let json = serde_json::to_value(response).expect("serialize");

        assert_eq!(json["mode"], "local");
        assert_eq!(json["setup_complete"], true);
        assert_eq!(json["auth"]["providers_configured"][0], "anthropic");
        assert_eq!(json["tailscale"]["hostname"], "joes-mac.tail1234.ts.net");
    }

    #[test]
    fn server_status_response_serializes_expected_shape() {
        let response = ServerStatusResponse {
            status: "ok".to_string(),
            version: "1.2.3".to_string(),
            uptime_seconds: 42,
            pid: 1234,
            host: "127.0.0.1".to_string(),
            port: 8400,
            https_enabled: false,
        };

        let json = serde_json::to_value(response).expect("serialize");

        assert_eq!(json["status"], "ok");
        assert_eq!(json["version"], "1.2.3");
        assert_eq!(json["pid"], 1234);
        assert_eq!(json["port"], 8400);
    }

    #[test]
    fn server_restart_response_serializes_expected_shape() {
        let response = ServerRestartResponse {
            accepted: true,
            restart_via: "launchagent_keepalive".to_string(),
            message: "Server restart requested.".to_string(),
        };

        let json = serde_json::to_value(response).expect("serialize");

        assert_eq!(json["accepted"], true);
        assert_eq!(json["restart_via"], "launchagent_keepalive");
        assert_eq!(json["message"], "Server restart requested.");
    }
}
