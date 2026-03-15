use crate::launchagent::{self, LaunchAgentConfig, LaunchAgentError};
use crate::state::HttpState;
use crate::types::ErrorBody;
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use super::HandlerResult;

#[derive(Debug, Serialize)]
pub struct LaunchAgentStatusResponse {
    pub installed: bool,
    pub loaded: bool,
}

#[derive(Debug, Deserialize)]
pub struct LaunchAgentInstallRequest {
    #[serde(default = "default_auto_start")]
    pub auto_start: bool,
}

fn default_auto_start() -> bool {
    true
}

#[derive(Debug, Serialize)]
pub struct LaunchAgentInstallResponse {
    pub installed: bool,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct LaunchAgentUninstallResponse {
    pub uninstalled: bool,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct LaunchAgentReloadResponse {
    pub reloaded: bool,
    pub message: String,
}

// GET /v1/launchagent/status
pub async fn handle_launchagent_status() -> Json<LaunchAgentStatusResponse> {
    let status = tokio::task::spawn_blocking(launchagent::status)
        .await
        .unwrap_or_else(|_| launchagent::status());
    Json(LaunchAgentStatusResponse {
        installed: status.installed,
        loaded: status.loaded,
    })
}

// POST /v1/launchagent/install
pub async fn handle_launchagent_install(
    State(state): State<HttpState>,
    Json(request): Json<LaunchAgentInstallRequest>,
) -> HandlerResult<Json<LaunchAgentInstallResponse>> {
    let runtime = state.server_runtime.clone();
    let config = LaunchAgentConfig {
        server_binary_path: std::env::current_exe().unwrap_or_default(),
        port: runtime.port,
        data_dir: state.data_dir.clone(),
        log_path: default_log_path(),
        auto_start: request.auto_start,
        keep_alive: true,
    };
    tokio::task::spawn_blocking(move || launchagent::install(&config))
        .await
        .map_err(|e| join_error(e.to_string()))?
        .map_err(agent_error)?;
    Ok(Json(LaunchAgentInstallResponse {
        installed: true,
        message: "LaunchAgent installed successfully.".to_string(),
    }))
}

// POST /v1/launchagent/uninstall
pub async fn handle_launchagent_uninstall() -> HandlerResult<Json<LaunchAgentUninstallResponse>> {
    tokio::task::spawn_blocking(launchagent::uninstall)
        .await
        .map_err(|e| join_error(e.to_string()))?
        .map_err(agent_error)?;
    Ok(Json(LaunchAgentUninstallResponse {
        uninstalled: true,
        message: "LaunchAgent uninstalled.".to_string(),
    }))
}

// POST /v1/launchagent/reload
pub async fn handle_launchagent_reload(
    State(state): State<HttpState>,
) -> HandlerResult<Json<LaunchAgentReloadResponse>> {
    let runtime = state.server_runtime.clone();
    let config = LaunchAgentConfig {
        server_binary_path: std::env::current_exe().unwrap_or_default(),
        port: runtime.port,
        data_dir: state.data_dir.clone(),
        log_path: default_log_path(),
        auto_start: true,
        keep_alive: true,
    };
    tokio::task::spawn_blocking(move || launchagent::reload(&config))
        .await
        .map_err(|e| join_error(e.to_string()))?
        .map_err(agent_error)?;
    Ok(Json(LaunchAgentReloadResponse {
        reloaded: true,
        message: "LaunchAgent reloaded.".to_string(),
    }))
}

fn default_log_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    std::path::PathBuf::from(home).join("Library/Logs/Fawx/server.log")
}

fn agent_error(error: LaunchAgentError) -> (StatusCode, Json<ErrorBody>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorBody {
            error: error.to_string(),
        }),
    )
}

fn join_error(msg: String) -> (StatusCode, Json<ErrorBody>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorBody { error: msg }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_response_serializes() {
        let r = LaunchAgentStatusResponse {
            installed: true,
            loaded: true,
        };
        let json = serde_json::to_value(r).unwrap();
        assert_eq!(json["installed"], true);
    }

    #[test]
    fn install_response_serializes() {
        let r = LaunchAgentInstallResponse {
            installed: true,
            message: "ok".into(),
        };
        let json = serde_json::to_value(r).unwrap();
        assert_eq!(json["installed"], true);
    }
}
