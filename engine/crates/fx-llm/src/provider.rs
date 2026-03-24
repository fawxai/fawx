//! Provider abstraction for model-completion backends.

use async_trait::async_trait;
use futures::Stream;
use std::pin::Pin;

use crate::streaming::{emit_default_stream_response, StreamCallback};
use crate::types::{CompletionRequest, CompletionResponse, LlmError, StreamChunk};

/// Streaming response type for completion APIs.
pub type CompletionStream = Pin<Box<dyn Stream<Item = Result<StreamChunk, LlmError>> + Send>>;

/// Static capabilities for a provider backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderCapabilities {
    /// Whether this backend accepts a `temperature` request parameter.
    pub supports_temperature: bool,
    /// Whether this backend requires streaming to be used.
    pub requires_streaming: bool,
}

/// Shared provider interface for cloud LLM adapters.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Send a completion request and return the full response.
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, LlmError>;

    /// Send a completion request and return a stream of incremental chunks.
    async fn complete_stream(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionStream, LlmError>;

    /// Send a completion request and emit normalized stream events.
    async fn stream(
        &self,
        request: CompletionRequest,
        callback: StreamCallback,
    ) -> Result<CompletionResponse, LlmError> {
        let response = self.complete(request).await?;
        emit_default_stream_response(&response, &callback);
        Ok(response)
    }

    /// Provider name for logging/routing.
    fn name(&self) -> &str;

    /// Models supported by this provider.
    fn supported_models(&self) -> Vec<String>;

    /// Fetch available models dynamically from the provider API.
    ///
    /// Returns model IDs the current credential has access to. Providers
    /// without a dynamic catalog override fall back to their static support list.
    async fn list_models(&self) -> Result<Vec<String>, LlmError> {
        Ok(self.supported_models())
    }

    /// Provider feature support contract.
    fn capabilities(&self) -> ProviderCapabilities;
}
