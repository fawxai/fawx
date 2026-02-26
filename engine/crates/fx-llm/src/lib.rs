//! LLM provider abstractions and routing utilities.
//!
//! This crate currently contains:
//! - Legacy prompt-first provider routing (`generate`/`generate_streaming`)
//! - New provider-client abstractions for structured completion APIs
//!   (Anthropic + OpenAI-compatible)

use async_trait::async_trait;
use fx_core::error::LlmError;
use std::sync::Arc;

mod anthropic;
mod config;
mod fallback;
mod local;
pub mod model_catalog;
mod openai;
mod openai_responses;
mod provider;
mod router;
mod routing;
mod sse;
mod types;

pub use anthropic::AnthropicAuthMode;
pub use anthropic::AnthropicProvider;
pub use config::LocalModelConfig;
pub use fallback::{FallbackResult, FallbackRouter, ProviderHealth};
pub use local::LocalModel;
pub use model_catalog::{CatalogModel, ModelCatalog};
pub use openai::OpenAiProvider;
pub use openai_responses::OpenAiResponsesProvider;
pub use provider::{CompletionStream, LlmProvider as CompletionProvider, ProviderCapabilities};
pub use router::{LlmRouter, ModelInfo, ModelRouter, RouterError, RoutingStrategy};
pub use routing::{resolve_strategy, RoutingCondition, RoutingConfig, RoutingContext, RoutingRule};
pub use types::{
    CompletionRequest, CompletionResponse, ContentBlock, LlmError as ProviderError, Message,
    MessageRole, StreamChunk, ToolCall, ToolDefinition, ToolUseDelta, Usage,
};

/// Legacy prompt-generation provider trait.
///
/// This trait is used by the existing local/cloud router implementation.
/// The newer structured provider API is exposed as [`CompletionProvider`].
#[async_trait]
pub trait LlmProvider: Send + Sync + std::fmt::Debug {
    /// Generate a completion for the given prompt.
    async fn generate(&self, prompt: &str, max_tokens: u32) -> Result<String, LlmError>;

    /// Generate completion with streaming callback.
    async fn generate_streaming(
        &self,
        prompt: &str,
        max_tokens: u32,
        callback: Box<dyn Fn(String) + Send + 'static>,
    ) -> Result<String, LlmError>;

    /// Get the model name/identifier.
    fn model_name(&self) -> &str;
}

/// Type alias for boxed legacy prompt providers (dynamic dispatch).
pub type BoxedProvider = Box<dyn LlmProvider>;

/// Type alias for shared legacy prompt providers.
pub type SharedProvider = Arc<dyn LlmProvider>;

#[cfg(test)]
mod tests {
    use super::*;

    /// Mock legacy provider for testing.
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

        let callback = Box::new(|_chunk: String| {
            // noop for test
        });

        let result = provider
            .generate_streaming("test", 10, callback)
            .await
            .unwrap();
        assert_eq!(result, "Hello world");
    }
}
