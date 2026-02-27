//! Configuration for local LLM models.

use fx_core::error::LlmError;
use std::path::PathBuf;

/// Configuration for local LLM inference.
#[derive(Debug, Clone)]
pub struct LocalModelConfig {
    /// Path to the GGUF model file
    pub model_path: PathBuf,

    /// Context window size (number of tokens)
    pub context_size: u32,

    /// Temperature for sampling (0.0 = deterministic, 1.0+ = creative)
    pub temperature: f32,

    /// Top-p (nucleus) sampling threshold
    pub top_p: f32,

    /// Maximum tokens to generate per request
    pub max_tokens: u32,
}

impl LocalModelConfig {
    /// Create a new configuration with validation.
    ///
    /// # Errors
    /// Returns `LlmError::Model` if:
    /// - model_path is empty
    /// - context_size is 0
    /// - temperature is negative
    /// - top_p is not in [0.0, 1.0]
    /// - max_tokens is 0
    pub fn new(
        model_path: PathBuf,
        context_size: u32,
        temperature: f32,
        top_p: f32,
        max_tokens: u32,
    ) -> Result<Self, LlmError> {
        // Validate model path
        if model_path.as_os_str().is_empty() {
            return Err(LlmError::Model("model_path cannot be empty".to_string()));
        }

        // Validate context_size
        if context_size == 0 {
            return Err(LlmError::Model("context_size must be > 0".to_string()));
        }

        // Validate temperature
        if temperature < 0.0 {
            return Err(LlmError::Model("temperature must be >= 0.0".to_string()));
        }

        // Validate top_p
        if !(0.0..=1.0).contains(&top_p) {
            return Err(LlmError::Model(
                "top_p must be between 0.0 and 1.0".to_string(),
            ));
        }

        // Validate max_tokens
        if max_tokens == 0 {
            return Err(LlmError::Model("max_tokens must be > 0".to_string()));
        }

        Ok(Self {
            model_path,
            context_size,
            temperature,
            top_p,
            max_tokens,
        })
    }

    /// Create a default configuration for testing (requires valid model path).
    pub fn default_with_path(model_path: PathBuf) -> Result<Self, LlmError> {
        Self::new(
            model_path, 2048, // 2K context
            0.7,  // Moderate creativity
            0.95, // High diversity
            512,  // Reasonable default
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_config() {
        let config =
            LocalModelConfig::new(PathBuf::from("/models/test.gguf"), 2048, 0.7, 0.95, 512);
        assert!(config.is_ok());
        let config = config.unwrap();
        assert_eq!(config.context_size, 2048);
        assert_eq!(config.temperature, 0.7);
    }

    #[test]
    fn test_empty_path_fails() {
        let result = LocalModelConfig::new(PathBuf::from(""), 2048, 0.7, 0.95, 512);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), LlmError::Model(_)));
    }

    #[test]
    fn test_zero_context_fails() {
        let result = LocalModelConfig::new(PathBuf::from("/test"), 0, 0.7, 0.95, 512);
        assert!(result.is_err());
    }

    #[test]
    fn test_negative_temperature_fails() {
        let result = LocalModelConfig::new(PathBuf::from("/test"), 2048, -0.1, 0.95, 512);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_top_p_fails() {
        let result = LocalModelConfig::new(PathBuf::from("/test"), 2048, 0.7, 1.5, 512);
        assert!(result.is_err());

        let result = LocalModelConfig::new(PathBuf::from("/test"), 2048, 0.7, -0.1, 512);
        assert!(result.is_err());
    }

    #[test]
    fn test_zero_max_tokens_fails() {
        let result = LocalModelConfig::new(PathBuf::from("/test"), 2048, 0.7, 0.95, 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_default_with_path() {
        let config = LocalModelConfig::default_with_path(PathBuf::from("/models/test.gguf"));
        assert!(config.is_ok());
        let config = config.unwrap();
        assert_eq!(config.context_size, 2048);
        assert_eq!(config.max_tokens, 512);
    }
}
