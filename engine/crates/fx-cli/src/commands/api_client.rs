use super::runtime_layout::RuntimeLayout;
use anyhow::Context;
use serde::Deserialize;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const API_REQUEST_TIMEOUT_SECONDS: u64 = 2;

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

pub(crate) fn bearer_token(layout: &RuntimeLayout) -> anyhow::Result<&str> {
    layout
        .config
        .http
        .bearer_token
        .as_deref()
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!(missing_auth_message()))
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
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

pub(crate) fn server_not_running_message() -> &'static str {
    "Fawx server is not running. Start it with `fawx serve --http`"
}

pub(crate) fn missing_auth_message() -> &'static str {
    "No authentication configured. Run `fawx setup` first."
}
