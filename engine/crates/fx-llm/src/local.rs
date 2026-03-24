//! Local LLM provider implementation.
//!
//! Stub implementation for on-device inference. The `llama-cpp-sys` FFI
//! dependency was removed during open-source extraction; this module
//! preserves the public API surface so downstream crates compile, but
//! all inference calls return an error at runtime.

use async_trait::async_trait;
use fx_core::error::LlmError;
use tracing::{debug, warn};

use crate::{LlmProvider, LocalModelConfig};

/// Local LLM provider (stub).
///
/// Inference is not yet available; all `generate` calls return an error.
#[derive(Debug)]
pub struct LocalModel {
    config: LocalModelConfig,
}

impl LocalModel {
    /// Create a new LocalModel instance.
    ///
    /// # Arguments
    /// * `config` - Validated configuration for the model
    ///
    /// # Returns
    /// A new LocalModel instance, or an error if initialization fails
    ///
    /// # Errors
    /// - `LlmError::Model`: Configuration is invalid
    /// - `LlmError::Inference`: Model file doesn't exist or can't be loaded
    pub fn new(config: LocalModelConfig) -> Result<Self, LlmError> {
        if config.context_size == 0 {
            return Err(LlmError::Model("context_size must be > 0".to_string()));
        }

        if !config.model_path.exists() {
            warn!("Model file does not exist: {}", config.model_path.display());
            return Err(LlmError::Model(format!(
                "Model file not found: {}",
                config.model_path.display()
            )));
        }

        debug!("LocalModel created (stub); inference will fail at runtime");

        Ok(Self { config })
    }

    /// Stub inference method.
    #[allow(dead_code)]
    fn infer_internal(&self, _prompt: &str, _max_tokens: u32) -> Result<String, LlmError> {
        Err(LlmError::Model(
            "local inference not available; llama-cpp backend was removed".to_string(),
        ))
    }
}

#[async_trait]
impl LlmProvider for LocalModel {
    async fn generate(&self, prompt: &str, max_tokens: u32) -> Result<String, LlmError> {
        debug!(
            "LocalModel::generate called with prompt length: {}, max_tokens: {}",
            prompt.len(),
            max_tokens
        );

        tokio::task::spawn_blocking(move || {
            Err(LlmError::Model(
                "local inference not available; llama-cpp backend was removed".to_string(),
            ))
        })
        .await
        .map_err(|e| LlmError::Inference(format!("Task join error: {}", e)))?
    }

    async fn generate_streaming(
        &self,
        prompt: &str,
        max_tokens: u32,
        callback: Box<dyn Fn(String) + Send + 'static>,
    ) -> Result<String, LlmError> {
        debug!(
            "LocalModel::generate_streaming called with prompt length: {}",
            prompt.len()
        );

        let result = self.generate(prompt, max_tokens).await?;
        callback(result.clone());
        Ok(result)
    }

    fn model_name(&self) -> &str {
        self.config
            .model_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_local_model_new_missing_file() {
        let config = LocalModelConfig::new(
            PathBuf::from("/nonexistent/model.gguf"),
            2048,
            0.7,
            0.95,
            512,
        )
        .unwrap();

        let result = LocalModel::new(config);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), LlmError::Model(_)));
    }

    #[test]
    fn test_model_name_extraction() {
        let temp_dir = std::env::temp_dir();
        let model_path = temp_dir.join("test-model.gguf");
        std::fs::write(&model_path, b"fake model").unwrap();

        let config = LocalModelConfig::new(model_path.clone(), 2048, 0.7, 0.95, 512).unwrap();
        let model = LocalModel::new(config).unwrap();

        assert_eq!(model.model_name(), "test-model.gguf");

        std::fs::remove_file(&model_path).ok();
    }

    #[tokio::test]
    async fn test_generate_returns_error() {
        let temp_dir = std::env::temp_dir();
        let model_path = temp_dir.join("test-model-2.gguf");
        std::fs::write(&model_path, b"fake model").unwrap();

        let config = LocalModelConfig::new(model_path.clone(), 2048, 0.7, 0.95, 512).unwrap();
        let model = LocalModel::new(config).unwrap();

        let result = model.generate("test prompt", 10).await;
        assert!(result.is_err());

        std::fs::remove_file(&model_path).ok();
    }

    #[tokio::test]
    async fn test_streaming_falls_back_to_generate() {
        let temp_dir = std::env::temp_dir();
        let model_path = temp_dir.join("test-model-streaming.gguf");
        std::fs::write(&model_path, b"fake model").unwrap();

        let config = LocalModelConfig::new(model_path.clone(), 2048, 0.7, 0.95, 512).unwrap();
        let model = LocalModel::new(config).unwrap();

        let callback = Box::new(|_chunk: String| {});
        let result = model.generate_streaming("test", 10, callback).await;
        assert!(result.is_err());

        std::fs::remove_file(&model_path).ok();
    }
}
