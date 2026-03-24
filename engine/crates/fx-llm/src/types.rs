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

pub use fx_core::types::{DocumentAttachment, ImageAttachment};

/// Token budget for "high" thinking mode.
pub const THINKING_BUDGET_HIGH: u32 = 10_000;
/// Token budget for "adaptive" thinking mode.
pub const THINKING_BUDGET_ADAPTIVE: u32 = 5_000;
/// Token budget for "low" thinking mode.
pub const THINKING_BUDGET_LOW: u32 = 1_024;

/// Thinking/reasoning configuration for LLM requests.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ThinkingConfig {
    /// Anthropic Claude 4.6 adaptive thinking with effort parameter.
    Adaptive { effort: String },
    /// Anthropic Claude 4.5/older manual thinking with fixed token budget.
    Enabled { budget_tokens: u32 },
    /// OpenAI reasoning effort.
    Reasoning { effort: String },
    /// Thinking/reasoning disabled.
    Off,
}

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
    /// Extended thinking configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingConfig>,
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

    /// Create a user message with text and optional images.
    pub fn user_with_images(text: impl Into<String>, images: Vec<ImageAttachment>) -> Self {
        Self::user_with_attachments(text, images, Vec::new())
    }

    /// Create a user message with text and optional images/documents.
    pub fn user_with_attachments(
        text: impl Into<String>,
        images: Vec<ImageAttachment>,
        documents: Vec<DocumentAttachment>,
    ) -> Self {
        let mut content: Vec<ContentBlock> = images
            .into_iter()
            .map(|image| ContentBlock::Image {
                media_type: image.media_type,
                data: image.data,
            })
            .collect();
        content.extend(
            documents
                .into_iter()
                .map(|document| ContentBlock::Document {
                    media_type: document.media_type,
                    data: document.data,
                    filename: document.filename,
                }),
        );
        let text = text.into();
        if !text.is_empty() {
            content.push(ContentBlock::Text { text });
        }
        Self {
            role: MessageRole::User,
            content,
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
        /// Provider-specific output item identifier, when distinct from `id`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_id: Option<String>,
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
    /// Base64-encoded image content block for vision-capable models.
    Image {
        /// MIME type (e.g., "image/jpeg", "image/png").
        media_type: String,
        /// Base64-encoded image data (no data URI prefix).
        data: String,
    },
    /// Base64-encoded document content block.
    Document {
        /// MIME type (e.g., "application/pdf").
        media_type: String,
        /// Base64-encoded document data (no data URI prefix).
        data: String,
        /// Original filename when available.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
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
    /// Provider-specific output item identifier, when distinct from `id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    /// Tool/function name when provided.
    pub name: Option<String>,
    /// Incremental JSON argument text, when streamed as text deltas.
    pub arguments_delta: Option<String>,
    /// Whether this delta came from a `*.function_call_arguments.done` event.
    #[serde(default)]
    pub arguments_done: bool,
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
    fn user_with_images_creates_correct_blocks() {
        let message = Message::user_with_images(
            "describe this",
            vec![ImageAttachment {
                media_type: "image/png".to_string(),
                data: "abc123".to_string(),
            }],
        );

        assert_eq!(message.role, MessageRole::User);
        assert_eq!(message.content.len(), 2);
        assert_eq!(
            message.content[0],
            ContentBlock::Image {
                media_type: "image/png".to_string(),
                data: "abc123".to_string(),
            }
        );
        assert_eq!(
            message.content[1],
            ContentBlock::Text {
                text: "describe this".to_string(),
            }
        );
    }

    #[test]
    fn user_with_images_empty_text_omits_text_block() {
        let message = Message::user_with_images(
            "",
            vec![ImageAttachment {
                media_type: "image/jpeg".to_string(),
                data: "xyz789".to_string(),
            }],
        );

        assert_eq!(message.role, MessageRole::User);
        assert_eq!(message.content.len(), 1);
        assert_eq!(
            message.content[0],
            ContentBlock::Image {
                media_type: "image/jpeg".to_string(),
                data: "xyz789".to_string(),
            }
        );
    }

    #[test]
    fn user_with_attachments_includes_documents() {
        let message = Message::user_with_attachments(
            "summarize these",
            vec![ImageAttachment {
                media_type: "image/png".to_string(),
                data: "abc123".to_string(),
            }],
            vec![DocumentAttachment {
                media_type: "application/pdf".to_string(),
                data: "pdf123".to_string(),
                filename: Some("brief.pdf".to_string()),
            }],
        );

        assert_eq!(message.role, MessageRole::User);
        assert_eq!(message.content.len(), 3);
        assert_eq!(
            message.content[1],
            ContentBlock::Document {
                media_type: "application/pdf".to_string(),
                data: "pdf123".to_string(),
                filename: Some("brief.pdf".to_string()),
            }
        );
    }

    #[test]
    fn image_content_block_serde_round_trip() {
        let block = ContentBlock::Image {
            media_type: "image/png".to_string(),
            data: "abc123".to_string(),
        };
        let json = serde_json::to_string(&block).unwrap();
        let deserialized: ContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(block, deserialized);
    }

    #[test]
    fn document_content_block_serde_round_trip() {
        let block = ContentBlock::Document {
            media_type: "application/pdf".to_string(),
            data: "abc123".to_string(),
            filename: Some("brief.pdf".to_string()),
        };
        let json = serde_json::to_string(&block).unwrap();
        let deserialized: ContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(block, deserialized);
    }

    #[test]
    fn test_stream_chunk_default_is_empty() {
        let chunk = StreamChunk::default();
        assert!(chunk.delta_content.is_none());
        assert!(chunk.tool_use_deltas.is_empty());
        assert!(chunk.usage.is_none());
        assert!(chunk.stop_reason.is_none());
    }

    #[test]
    fn thinking_budget_constants_match_expected_values() {
        assert_eq!(THINKING_BUDGET_HIGH, 10_000);
        assert_eq!(THINKING_BUDGET_ADAPTIVE, 5_000);
        assert_eq!(THINKING_BUDGET_LOW, 1_024);
    }

    #[test]
    fn thinking_config_enabled_stores_budget() {
        let config = ThinkingConfig::Enabled {
            budget_tokens: THINKING_BUDGET_HIGH,
        };
        match config {
            ThinkingConfig::Enabled { budget_tokens } => assert_eq!(budget_tokens, 10_000),
            ThinkingConfig::Adaptive { .. }
            | ThinkingConfig::Reasoning { .. }
            | ThinkingConfig::Off => panic!("expected Enabled"),
        }
    }

    #[test]
    fn thinking_config_adaptive_stores_effort() {
        let config = ThinkingConfig::Adaptive {
            effort: "high".to_string(),
        };
        assert_eq!(
            config,
            ThinkingConfig::Adaptive {
                effort: "high".to_string(),
            }
        );
    }

    #[test]
    fn thinking_config_reasoning_stores_effort() {
        let config = ThinkingConfig::Reasoning {
            effort: "xhigh".to_string(),
        };
        assert_eq!(
            config,
            ThinkingConfig::Reasoning {
                effort: "xhigh".to_string(),
            }
        );
    }

    #[test]
    fn thinking_config_off_has_no_budget() {
        let config = ThinkingConfig::Off;
        assert_eq!(config, ThinkingConfig::Off);
    }

    #[test]
    fn thinking_config_completion_request_defaults_to_none() {
        let request = CompletionRequest {
            model: "test".to_string(),
            messages: vec![],
            tools: vec![],
            temperature: None,
            max_tokens: None,
            system_prompt: None,
            thinking: None,
        };
        assert!(request.thinking.is_none());
    }

    #[test]
    fn thinking_config_roundtrips_through_completion_request() {
        let thinking = Some(ThinkingConfig::Enabled {
            budget_tokens: THINKING_BUDGET_ADAPTIVE,
        });
        let request = CompletionRequest {
            model: "test".to_string(),
            messages: vec![],
            tools: vec![],
            temperature: None,
            max_tokens: None,
            system_prompt: None,
            thinking: thinking.clone(),
        };
        assert_eq!(request.thinking, thinking);
    }
}
