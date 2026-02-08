//! Configuration management for Nova.
//!
//! Handles loading and validating configuration from JSON files.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::error::{CoreError, Result};

/// Main configuration structure for Nova.
///
/// This configuration is loaded from a JSON5 file (currently using JSON for simplicity)
/// and defines all system-level settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Path to the local LLM model file (GGUF format)
    pub model_path: PathBuf,

    /// Path to the API key storage file (encrypted)
    pub api_key_path: PathBuf,

    /// Logging level (trace, debug, info, warn, error)
    pub log_level: String,

    /// Path to the encrypted storage directory
    pub storage_path: PathBuf,

    /// Path to the action policy file
    pub policy_path: PathBuf,
}

impl Config {
    /// Load configuration from a JSON file.
    ///
    /// # Arguments
    /// * `path` - Path to the configuration file
    ///
    /// # Returns
    /// * `Result<Config>` - Loaded configuration or error
    ///
    /// # Example
    /// ```no_run
    /// use nv_core::Config;
    /// let config = Config::load("config.json").unwrap();
    /// ```
    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let content = std::fs::read_to_string(path.as_ref())
            .map_err(|e| CoreError::ConfigLoad(e.to_string()))?;

        let config: Config =
            serde_json::from_str(&content).map_err(|e| CoreError::ConfigParse(e.to_string()))?;

        Ok(config)
    }

    /// Validate the configuration.
    ///
    /// Checks that all required paths exist and are accessible.
    pub fn validate(&self) -> Result<()> {
        // Validation logic placeholder
        // In a real implementation, check that paths are accessible
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_deserialize() {
        let json = r#"{
            "model_path": "/path/to/model.gguf",
            "api_key_path": "/path/to/keys.enc",
            "log_level": "info",
            "storage_path": "/path/to/storage",
            "policy_path": "/path/to/policy.json"
        }"#;

        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.log_level, "info");
    }
}
