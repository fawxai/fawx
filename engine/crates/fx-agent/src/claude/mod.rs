//! Claude API client module.
//!
//! Provides a client for interacting with the Claude API, including support
//! for message completions, streaming responses, and tool use.

pub mod client;
pub mod config;
pub mod error;
pub mod types;

pub use client::ClaudeClient;
pub use config::ClaudeConfig;
pub use error::{AgentError, Result};
pub use types::{
    CompletionResponse, ContentBlock, Message, Role, StopReason, StreamEvent, Tool, ToolResult,
    ToolUse, Usage,
};
