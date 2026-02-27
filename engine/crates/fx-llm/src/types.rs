//! Shared request/response types for provider-backed LLM clients.
//!
//! These types are provider-agnostic and model the common shape needed to talk to
//! Anthropic and OpenAI-compatible APIs.
//!
//! Migration note: `fx-agent/src/claude/types.rs` currently defines Claude-specific
//! equivalents. This module is the new cross-provider target, and fx-agent can
//! migrate onto these types in a follow-up refactor.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

/// A model completion request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompletionRequest {
    /// Target model identifier.
    pub model: String,
    /// Conversation messages.
    pub messages: Vec<Message>,
    /// Available tool definitions for this request.
    #[serde(default)]
    pub tools: Vec<ToolDefinition>,
    /// Sampling temperature.
    pub temperature: Option<f32>,
    /// Maximum number of output tokens to generate.
    pub max_tokens: Option<u32>,
    /// Optional top-level system prompt.
    pub system_prompt: Option<String>,
}

/// A model completion response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompletionResponse {
    /// Content blocks returned by the provider.
    pub content: Vec<ContentBlock>,
    /// Tool calls requested by the provider.
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    /// Optional token usage information.
    pub usage: Option<Usage>,
    /// Provider stop reason, when supplied.
    pub stop_reason: Option<String>,
}

/// A single conversation message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message {
    /// Message role.
    pub role: MessageRole,
    /// Structured content blocks.
    #[serde(default)]
    pub content: Vec<ContentBlock>,
}

impl Message {
    /// Create a user text message.
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            content: vec![ContentBlock::Text { text: text.into() }],
        }
    }

    /// Create an assistant text message.
    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: vec![ContentBlock::Text { text: text.into() }],
        }
    }

    /// Create a system text message.
    pub fn system(text: impl Into<String>) -> Self {
        Self {
            role: MessageRole::System,
            content: vec![ContentBlock::Text { text: text.into() }],
        }
    }
}

/// Role of a message sender.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    /// System instruction message.
    System,
    /// User message.
    User,
    /// Assistant/model message.
    Assistant,
    /// Tool message.
    Tool,
}

/// Structured content within a message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Text content block.
    Text {
        /// Text payload.
        text: String,
    },
    /// Tool use request block.
    ToolUse {
        /// Tool call identifier.
        id: String,
        /// Tool/function name.
        name: String,
        /// Structured input arguments.
        input: Value,
    },
    /// Tool execution result block.
    ToolResult {
        /// Tool call identifier this result belongs to.
        tool_use_id: String,
        /// Tool output content.
        content: Value,
    },
}

/// Definition of a callable tool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolDefinition {
    /// Tool/function name.
    pub name: String,
    /// Tool description for model selection.
    pub description: String,
    /// JSON Schema parameters object.
    pub parameters: Value,
}

/// Tool call extracted from provider responses.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCall {
    /// Tool call identifier.
    pub id: String,
    /// Tool/function name.
    pub name: String,
    /// Parsed JSON arguments.
    pub arguments: Value,
}

/// Token usage accounting.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Usage {
    /// Input tokens consumed by prompt/context.
    pub input_tokens: u32,
    /// Output tokens produced by generation.
    pub output_tokens: u32,
}

/// Incremental stream update from a provider.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct StreamChunk {
    /// Incremental text delta.
    pub delta_content: Option<String>,
    /// Incremental tool-use deltas.
    #[serde(default)]
    pub tool_use_deltas: Vec<ToolUseDelta>,
    /// Incremental usage update.
    pub usage: Option<Usage>,
    /// Optional stop reason surfaced during streaming.
    pub stop_reason: Option<String>,
}

/// Incremental tool call data from streaming APIs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolUseDelta {
    /// Tool call identifier when provided.
    pub id: Option<String>,
    /// Tool/function name when provided.
    pub name: Option<String>,
    /// Incremental JSON argument text, when streamed as text deltas.
    pub arguments_delta: Option<String>,
}

/// Errors produced by provider adapters.
#[derive(Debug, Clone, Serialize, Deserialize, Error, PartialEq)]
pub enum LlmError {
    /// Request configuration was invalid.
    #[error("configuration error: {0}")]
    Config(String),

    /// HTTP request failed.
    #[error("request failed: {0}")]
    Request(String),

    /// Authentication failed.
    #[error("authentication failed: {0}")]
    Authentication(String),

    /// Request was rate limited.
    #[error("rate limited: {0}")]
    RateLimited(String),

    /// Response payload could not be understood.
    #[error("invalid response: {0}")]
    InvalidResponse(String),

    /// Serialization/deserialization failed.
    #[error("serialization error: {0}")]
    Serialization(String),

    /// Streaming payload parsing failed.
    #[error("streaming error: {0}")]
    Streaming(String),

    /// Requested model is unsupported by this provider.
    #[error("unsupported model: {0}")]
    UnsupportedModel(String),

    /// Catch-all provider error.
    #[error("provider error: {0}")]
    Provider(String),
}

impl From<serde_json::Error> for LlmError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serialization(value.to_string())
    }
}

impl From<reqwest::Error> for LlmError {
    fn from(value: reqwest::Error) -> Self {
        if value.is_timeout() {
            return Self::Request(format!("timeout: {value}"));
        }

        Self::Request(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_helpers_create_text_blocks() {
        let user = Message::user("hello");
        let assistant = Message::assistant("world");
        let system = Message::system("rules");

        assert!(matches!(user.role, MessageRole::User));
        assert!(matches!(assistant.role, MessageRole::Assistant));
        assert!(matches!(system.role, MessageRole::System));

        assert_eq!(user.content.len(), 1);
        assert_eq!(assistant.content.len(), 1);
        assert_eq!(system.content.len(), 1);
    }

    #[test]
    fn test_stream_chunk_default_is_empty() {
        let chunk = StreamChunk::default();
        assert!(chunk.delta_content.is_none());
        assert!(chunk.tool_use_deltas.is_empty());
        assert!(chunk.usage.is_none());
        assert!(chunk.stop_reason.is_none());
    }
}
