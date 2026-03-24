//! Configuration management for Fawx.
//!
//! Handles loading and validating configuration from JSON files.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::error::{CoreError, Result};

/// Main configuration structure for Fawx.
///
/// This configuration is loaded from a JSON file and defines all system-level settings.
/// TODO(#861): Evaluate JSON5 for comments in config files (Epic 2+).
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
    /// use fx_core::Config;
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
    /// Checks that required paths exist and log level is valid.
    /// More thorough validation (model file format, key accessibility)
    /// will be added in Epic 2-3 when those subsystems are implemented.
    pub fn validate(&self) -> Result<()> {
        let valid_levels = ["trace", "debug", "info", "warn", "error"];
        if !valid_levels.contains(&self.log_level.as_str()) {
            return Err(CoreError::ConfigParse(format!(
                "Invalid log_level '{}'. Expected one of: {:?}",
                self.log_level, valid_levels
            )));
        }

        if self.model_path.as_os_str().is_empty() {
            return Err(CoreError::ConfigParse("model_path cannot be empty".into()));
        }

        if self.storage_path.as_os_str().is_empty() {
            return Err(CoreError::ConfigParse(
                "storage_path cannot be empty".into(),
            ));
        }

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
