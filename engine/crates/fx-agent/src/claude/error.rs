//! Error types for Claude API client.

use thiserror::Error;

/// Errors that can occur when interacting with Claude API.
#[derive(Error, Debug)]
pub enum AgentError {
    /// API request failed (network, connection issues).
    #[error("API request failed: {0}")]
    ApiRequest(String),

    /// Invalid response from API (malformed JSON, unexpected structure).
    #[error("Invalid API response: {0}")]
    InvalidResponse(String),

    /// Request timed out.
    #[error("Request timed out: {0}")]
    Timeout(String),

    /// Rate limit exceeded (HTTP 429).
    #[error("Rate limit exceeded: {0}")]
    RateLimit(String),

    /// Authentication failed (HTTP 401).
    #[error("Authentication failed: {0}")]
    Auth(String),

    /// Bad request (HTTP 400).
    #[error("Bad request: {0}")]
    BadRequest(String),

    /// Server error (HTTP 5xx).
    #[error("Server error: {0}")]
    ServerError(String),

    /// Configuration error (missing API key, invalid config).
    #[error("Configuration error: {0}")]
    Config(String),

    /// Tool execution error.
    #[error("Tool execution error: {0}")]
    ToolExecution(String),

    /// JSON serialization/deserialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// HTTP client error.
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// Error from fx-core.
    #[error("Core error: {0}")]
    Core(String),
}

impl From<fx_core::error::LlmError> for AgentError {
    fn from(err: fx_core::error::LlmError) -> Self {
        AgentError::Core(err.to_string())
    }
}

/// Result type alias using `AgentError`.
pub type Result<T> = std::result::Result<T, AgentError>;
