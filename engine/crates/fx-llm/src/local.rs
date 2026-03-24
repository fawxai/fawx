//! Local LLM provider implementation using llama.cpp.

use async_trait::async_trait;
use fx_core::error::LlmError;
use tracing::{debug, warn};

use crate::{LlmProvider, LocalModelConfig};

/// Local LLM provider using llama.cpp for on-device inference.
///
/// This struct wraps the unsafe llama-cpp-sys FFI bindings and provides
/// a safe, async Rust API.
#[derive(Debug)]
pub struct LocalModel {
    config: LocalModelConfig,
    // Future: Add actual llama.cpp context handle
    // context: Option<*mut llama_context>,
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
        // Validate config (already validated in LocalModelConfig::new, but double-check)
        if config.context_size == 0 {
            return Err(LlmError::Model("context_size must be > 0".to_string()));
        }

        // Check model file exists
        if !config.model_path.exists() {
            warn!("Model file does not exist: {}", config.model_path.display());
            return Err(LlmError::Model(format!(
                "Model file not found: {}",
                config.model_path.display()
            )));
        }

        #[cfg(not(feature = "llama-cpp"))]
        {
            debug!("LocalModel created without llama-cpp feature; inference will fail at runtime");
        }

        Ok(Self { config })
    }

    /// Internal method to perform actual inference.
    ///
    /// This is where llama.cpp FFI calls would happen.
    #[allow(dead_code)]
    #[cfg(feature = "llama-cpp")]
    fn infer_internal(&self, _prompt: &str, _max_tokens: u32) -> Result<String, LlmError> {
        // Future: Actual llama.cpp inference
        // 1. Tokenize prompt
        // 2. Run inference loop
        // 3. Decode tokens to string
        // 4. Return result

        Err(LlmError::Inference(
            "llama.cpp integration not yet implemented".to_string(),
        ))
    }

    #[allow(dead_code)]
    #[cfg(not(feature = "llama-cpp"))]
    fn infer_internal(&self, _prompt: &str, _max_tokens: u32) -> Result<String, LlmError> {
        Err(LlmError::Model(
            "llama-cpp feature not enabled; cannot perform local inference".to_string(),
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

        // Run inference in blocking task (llama.cpp is CPU-bound)
        let _config = self.config.clone();
        let _prompt = prompt.to_string();

        tokio::task::spawn_blocking(move || {
            // Placeholder: would call self.infer_internal here
            // For now, return error since feature is not enabled
            #[cfg(feature = "llama-cpp")]
            {
                Err(LlmError::Inference(
                    "llama.cpp integration not yet implemented".to_string(),
                ))
            }

            #[cfg(not(feature = "llama-cpp"))]
            {
                Err(LlmError::Model(
                    "llama-cpp feature not enabled; cannot perform local inference".to_string(),
                ))
            }
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

        // For now, fall back to non-streaming and call callback once
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
        // Create a temp file for testing
        let temp_dir = std::env::temp_dir();
        let model_path = temp_dir.join("test-model.gguf");
        std::fs::write(&model_path, b"fake model").unwrap();

        let config = LocalModelConfig::new(model_path.clone(), 2048, 0.7, 0.95, 512).unwrap();
        let model = LocalModel::new(config).unwrap();

        assert_eq!(model.model_name(), "test-model.gguf");

        // Cleanup
        std::fs::remove_file(&model_path).ok();
    }

    #[tokio::test]
    async fn test_generate_without_feature() {
        // Create a temp file
        let temp_dir = std::env::temp_dir();
        let model_path = temp_dir.join("test-model-2.gguf");
        std::fs::write(&model_path, b"fake model").unwrap();

        let config = LocalModelConfig::new(model_path.clone(), 2048, 0.7, 0.95, 512).unwrap();
        let model = LocalModel::new(config).unwrap();

        let result = model.generate("test prompt", 10).await;

        // Without llama-cpp feature, should error
        #[cfg(not(feature = "llama-cpp"))]
        assert!(result.is_err());

        // Cleanup
        std::fs::remove_file(&model_path).ok();
    }

    #[tokio::test]
    async fn test_streaming_callback_signature() {
        // This test verifies the streaming API signature works correctly
        // without requiring actual model inference

        let temp_dir = std::env::temp_dir();
        let model_path = temp_dir.join("test-model-streaming.gguf");
        std::fs::write(&model_path, b"fake model").unwrap();

        let config = LocalModelConfig::new(model_path.clone(), 2048, 0.7, 0.95, 512).unwrap();
        let model = LocalModel::new(config).unwrap();

        // Verify callback accepts owned String (not &str)
        let callback = Box::new(|chunk: String| {
            // In real use, this would send chunk to a channel/stream
            assert!(!chunk.is_empty() || chunk.is_empty()); // Always true, just to use chunk
        });

        let result = model.generate_streaming("test", 10, callback).await;

        // Without llama-cpp feature, generate() fails, so streaming also fails
        #[cfg(not(feature = "llama-cpp"))]
        assert!(result.is_err());

        // Cleanup
        std::fs::remove_file(&model_path).ok();
    }
}
