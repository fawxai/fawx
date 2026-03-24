//! Types for Claude API messages and responses.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A message in a conversation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message {
    /// Role of the message sender.
    pub role: Role,
    /// Content of the message.
    pub content: String,
}

impl Message {
    /// Create a new user message.
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }

    /// Create a new assistant message.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }

    /// Create a new system message.
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
        }
    }
}

/// Role of a message sender.
///
/// Note: In Claude's API, `System` messages are typically passed via the `system`
/// parameter rather than in the messages array. This variant is included for
/// completeness and potential future use cases.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// User message.
    User,
    /// Assistant (Claude) message.
    Assistant,
    /// System message (instructions).
    /// Note: Typically passed separately via the `system` parameter in Claude API.
    System,
}

/// A tool definition for Claude.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Tool {
    /// Name of the tool.
    pub name: String,
    /// Description of what the tool does.
    pub description: String,
    /// JSON Schema for the tool's input parameters.
    pub input_schema: Value,
}

impl Tool {
    /// Create a new tool definition.
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            input_schema,
        }
    }
}

/// Tool use requested by Claude.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolUse {
    /// Unique ID for this tool use.
    pub id: String,
    /// Name of the tool to use.
    pub name: String,
    /// Input parameters for the tool (JSON object).
    pub input: Value,
}

/// Result of a tool execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolResult {
    /// ID of the tool use this result corresponds to.
    pub tool_use_id: String,
    /// Result content (success or error message).
    pub content: String,
}

/// Response from Claude API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompletionResponse {
    /// Content blocks in the response.
    pub content: Vec<ContentBlock>,
    /// Reason the completion stopped.
    pub stop_reason: StopReason,
    /// Token usage statistics.
    pub usage: Usage,
}

/// A content block in a response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Text content.
    Text {
        /// The text content.
        text: String,
    },
    /// Tool use request.
    ToolUse {
        /// Unique ID for this tool use.
        id: String,
        /// Name of the tool.
        name: String,
        /// Input parameters.
        input: Value,
    },
}

impl ContentBlock {
    /// Extract text from a Text content block.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            ContentBlock::Text { text } => Some(text),
            _ => None,
        }
    }

    /// Extract tool use from a ToolUse content block.
    pub fn as_tool_use(&self) -> Option<ToolUse> {
        match self {
            ContentBlock::ToolUse { id, name, input } => Some(ToolUse {
                id: id.clone(),
                name: name.clone(),
                input: input.clone(),
            }),
            _ => None,
        }
    }
}

/// Reason why the completion stopped.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    /// Natural end of turn.
    EndTurn,
    /// Tool use requested.
    ToolUse,
    /// Maximum tokens reached.
    MaxTokens,
    /// Stop sequence encountered.
    StopSequence,
}

/// Token usage statistics.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Usage {
    /// Number of input tokens.
    pub input_tokens: u32,
    /// Number of output tokens.
    pub output_tokens: u32,
}

/// Event from streaming response.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    /// Content block started.
    ContentBlockStart,
    /// Content block delta (incremental text).
    ContentBlockDelta(String),
    /// Content block stopped.
    ContentBlockStop,
    /// Message completed.
    MessageStop,
}
