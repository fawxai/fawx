//! LLM provider abstraction (local + cloud).
//!
//! Provides unified interface for both local LLM inference (llama.cpp)
//! and cloud LLM APIs (Claude), with automatic routing based on strategy.

use async_trait::async_trait;
use ct_core::error::LlmError;
use std::sync::Arc;

mod config;
mod fallback;
mod local;
mod router;
mod routing;

pub use config::LocalModelConfig;
pub use fallback::{FallbackResult, FallbackRouter, ProviderHealth};
pub use local::LocalModel;
pub use router::{LlmRouter, RoutingStrategy};
pub use routing::{resolve_strategy, RoutingCondition, RoutingConfig, RoutingContext, RoutingRule};

/// LLM provider trait.
///
/// Abstraction over local and cloud LLM providers.
#[async_trait]
pub trait LlmProvider: Send + Sync + std::fmt::Debug {
    /// Generate a completion for the given prompt.
    ///
    /// # Arguments
    /// * `prompt` - The input text to generate from
    /// * `max_tokens` - Maximum number of tokens to generate
    ///
    /// # Returns
    /// Generated text or an error
    async fn generate(&self, prompt: &str, max_tokens: u32) -> Result<String, LlmError>;

    /// Generate completion with streaming callback.
    ///
    /// # Arguments
    /// * `prompt` - The input text to generate from
    /// * `max_tokens` - Maximum number of tokens to generate
    /// * `callback` - Called for each generated token chunk (receives owned String)
    ///
    /// # Returns
    /// Full generated text or an error
    async fn generate_streaming(
        &self,
        prompt: &str,
        max_tokens: u32,
        callback: Box<dyn Fn(String) + Send + 'static>,
    ) -> Result<String, LlmError>;

    /// Get the model name/identifier.
    fn model_name(&self) -> &str;
}

/// Type alias for boxed LLM providers (for dynamic dispatch).
pub type BoxedProvider = Box<dyn LlmProvider>;

/// Type alias for Arc-wrapped providers (for shared ownership).
pub type SharedProvider = Arc<dyn LlmProvider>;

#[cfg(test)]
mod tests {
    use super::*;

    /// Mock provider for testing
    #[derive(Debug)]
    struct MockProvider {
        name: String,
        response: String,
    }

    #[async_trait]
    impl LlmProvider for MockProvider {
        async fn generate(&self, _prompt: &str, _max_tokens: u32) -> Result<String, LlmError> {
            Ok(self.response.clone())
        }

        async fn generate_streaming(
            &self,
            _prompt: &str,
            _max_tokens: u32,
            callback: Box<dyn Fn(String) + Send + 'static>,
        ) -> Result<String, LlmError> {
            // Simulate streaming by calling callback for each word
            for word in self.response.split_whitespace() {
                callback(word.to_string());
            }
            Ok(self.response.clone())
        }

        fn model_name(&self) -> &str {
            &self.name
        }
    }

    #[tokio::test]
    async fn test_mock_provider_generate() {
        let provider = MockProvider {
            name: "test-model".to_string(),
            response: "Hello world".to_string(),
        };

        let result = provider.generate("test", 10).await.unwrap();
        assert_eq!(result, "Hello world");
        assert_eq!(provider.model_name(), "test-model");
    }

    #[tokio::test]
    async fn test_mock_provider_streaming() {
        let provider = MockProvider {
            name: "test-model".to_string(),
            response: "Hello world".to_string(),
        };

        // Use a simple non-mutating callback
        let callback = Box::new(|_chunk: String| {
            // In a real scenario, this would send chunks to a stream
        });

        let result = provider
            .generate_streaming("test", 10, callback)
            .await
            .unwrap();
        assert_eq!(result, "Hello world");
    }
}
