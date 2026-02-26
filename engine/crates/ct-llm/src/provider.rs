//! Provider abstraction for model-completion backends.

use async_trait::async_trait;
use futures::Stream;
use std::pin::Pin;

use crate::types::{CompletionRequest, CompletionResponse, LlmError, StreamChunk};

/// Streaming response type for completion APIs.
pub type CompletionStream = Pin<Box<dyn Stream<Item = Result<StreamChunk, LlmError>> + Send>>;

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

    /// Provider name for logging/routing.
    fn name(&self) -> &str;

    /// Models supported by this provider.
    fn supported_models(&self) -> Vec<String>;
}
