//! Configuration for Claude API client.

use super::error::{AgentError, Result};
use serde::{Deserialize, Serialize};

/// Configuration for Claude API client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeConfig {
    /// API key for Claude API.
    pub api_key: String,
    /// Model to use (default: "claude-sonnet-4-5").
    pub model: String,
    /// Maximum tokens to generate.
    pub max_tokens: u32,
    /// Base URL for the API (default: "https://api.anthropic.com").
    pub base_url: String,
}

impl ClaudeConfig {
    /// Create a new configuration with the given API key.
    pub fn new(api_key: impl Into<String>) -> Result<Self> {
        let api_key = api_key.into();
        if api_key.is_empty() {
            return Err(AgentError::Config("API key cannot be empty".to_string()));
        }

        Ok(Self {
            api_key,
            model: "claude-sonnet-4-5".to_string(),
            max_tokens: 4096,
            base_url: "https://api.anthropic.com".to_string(),
        })
    }

    /// Set the model to use.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    /// Set the maximum tokens to generate.
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    /// Set the base URL for the API.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Validate the configuration.
    pub fn validate(&self) -> Result<()> {
        if self.api_key.is_empty() {
            return Err(AgentError::Config("API key cannot be empty".to_string()));
        }
        if self.model.is_empty() {
            return Err(AgentError::Config("Model cannot be empty".to_string()));
        }
        if self.max_tokens == 0 {
            return Err(AgentError::Config(
                "Max tokens must be greater than 0".to_string(),
            ));
        }
        if self.base_url.is_empty() {
            return Err(AgentError::Config("Base URL cannot be empty".to_string()));
        }
        Ok(())
    }
}

impl Default for ClaudeConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            model: "claude-sonnet-4-5".to_string(),
            max_tokens: 4096,
            base_url: "https://api.anthropic.com".to_string(),
        }
    }
}
