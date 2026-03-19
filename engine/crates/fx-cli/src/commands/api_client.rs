use super::runtime_layout::RuntimeLayout;
use crate::auth_store::AuthStore;
use anyhow::Context;
use serde::Deserialize;
use std::{path::Path, time::Duration};

const API_REQUEST_TIMEOUT_SECONDS: u64 = 2;
const HTTP_BEARER_PROVIDER: &str = "http_bearer";

#[derive(Debug, Deserialize)]
pub(crate) struct ErrorResponse {
    pub(crate) error: String,
}

pub(crate) fn http_client() -> anyhow::Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(API_REQUEST_TIMEOUT_SECONDS))
        .build()
        .context("failed to build HTTP client")
}

pub(crate) fn bearer_token(layout: &RuntimeLayout) -> anyhow::Result<String> {
    auth_store_bearer_token(&layout.data_dir)
        .or_else(|| config_bearer_token(layout))
        .ok_or_else(|| anyhow::anyhow!(missing_auth_message()))
}

fn auth_store_bearer_token(data_dir: &Path) -> Option<String> {
    AuthStore::open(data_dir)
        .ok()
        .and_then(|store| store.get_provider_token(HTTP_BEARER_PROVIDER).ok())
        .flatten()
        .and_then(|token| normalized_token(token.as_str()))
}

fn config_bearer_token(layout: &RuntimeLayout) -> Option<String> {
    layout
        .config
        .http
        .bearer_token
        .as_deref()
        .and_then(normalized_token)
}

fn normalized_token(token: &str) -> Option<String> {
    let trimmed = token.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

pub(crate) fn request_error(error: reqwest::Error) -> anyhow::Error {
    if error.is_connect() {
        anyhow::anyhow!(server_not_running_message())
    } else {
        anyhow::Error::new(error)
    }
}

pub(crate) async fn api_error_message(response: reqwest::Response) -> String {
    let status = response.status();
    match response.json::<ErrorResponse>().await {
        Ok(body) if !body.error.trim().is_empty() => body.error,
        _ => format!("request failed with status {status}"),
    }
}

pub(crate) fn current_unix_seconds() -> u64 {
    fx_api::time_util::current_time_seconds()
}

pub(crate) fn server_not_running_message() -> &'static str {
    "Fawx server is not running. Start it with `fawx serve --http`"
}

pub(crate) fn missing_auth_message() -> &'static str {
    "No authentication configured. Run `fawx setup` first."
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_config::FawxConfig;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn test_layout(data_dir: &Path, config_token: Option<&str>) -> RuntimeLayout {
        let mut config = FawxConfig::default();
        config.http.bearer_token = config_token.map(str::to_string);
        RuntimeLayout {
            data_dir: data_dir.to_path_buf(),
            config_path: data_dir.join("config.toml"),
            storage_dir: data_dir.join("storage"),
            audit_log_path: data_dir.join("audit.log"),
            auth_db_path: data_dir.join("auth.db"),
            logs_dir: data_dir.join("logs"),
            skills_dir: data_dir.join("skills"),
            trusted_keys_dir: data_dir.join("trusted_keys"),
            embedding_model_dir: data_dir.join("models"),
            pid_file: data_dir.join("fawx.pid"),
            memory_json_path: data_dir.join("memory.json"),
            sessions_dir: data_dir.join("sessions"),
            security_baseline_path: data_dir.join("security-baseline.json"),
            repo_root: PathBuf::from("."),
            http_port: 8400,
            config,
        }
    }

    #[test]
    fn bearer_token_prefers_auth_store_token_over_config() {
        let dir = tempdir().expect("tempdir");
        let store = AuthStore::open(dir.path()).expect("auth store");
        store
            .store_provider_token(HTTP_BEARER_PROVIDER, " stored-token ")
            .expect("store token");
        let layout = test_layout(dir.path(), Some(" config-token "));

        let token = bearer_token(&layout).expect("bearer token");

        assert_eq!(token, "stored-token");
    }

    #[test]
    fn bearer_token_falls_back_to_config_when_store_has_no_token() {
        let dir = tempdir().expect("tempdir");
        let layout = test_layout(dir.path(), Some(" config-token "));

        let token = bearer_token(&layout).expect("bearer token");

        assert_eq!(token, "config-token");
    }

    #[test]
    fn bearer_token_falls_back_to_config_when_auth_store_cannot_open() {
        let dir = tempdir().expect("tempdir");
        let file_path = dir.path().join("not-a-directory");
        std::fs::write(&file_path, "x").expect("test file");
        let layout = test_layout(&file_path, Some(" config-token "));

        let token = bearer_token(&layout).expect("bearer token");

        assert_eq!(token, "config-token");
    }

    #[test]
    fn bearer_token_errors_when_no_authentication_is_configured() {
        let dir = tempdir().expect("tempdir");
        let layout = test_layout(dir.path(), None);

        let error = bearer_token(&layout).expect_err("missing auth should error");

        assert_eq!(error.to_string(), missing_auth_message());
    }
}
