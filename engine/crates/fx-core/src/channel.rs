//! Channel trait — abstract input/output source for the agentic loop.
//!
//! Channels feed user messages into the kernel and receive responses.
//! Examples: TUI, HTTP API, Telegram bot, Discord bot, webhook.
//!
//! This trait lives in fx-core (not fx-kernel) so that skills and external
//! crates can reference it without circular dependencies.

use crate::types::InputSource;
use std::fmt;

/// Routing context for response delivery.
///
/// Channels extract what they need from this structure. Channels that do not
/// require routing metadata can ignore it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResponseContext {
    /// Channel-specific routing key (for example, a Telegram `chat_id`).
    pub routing_key: Option<String>,
    /// Original message identifier for reply threading.
    pub reply_to: Option<String>,
}

/// A channel represents an input/output source for the agentic loop.
///
/// Channels feed user messages into the kernel and receive responses.
/// Examples: TUI, HTTP API, Telegram bot, Discord bot, webhook.
pub trait Channel: Send + Sync {
    /// Unique identifier for this channel (e.g., "tui", "http", "telegram").
    fn id(&self) -> &str;

    /// Human-readable name (e.g., "Terminal UI", "HTTP API").
    fn name(&self) -> &str;

    /// The [`InputSource`] type for messages from this channel.
    fn input_source(&self) -> InputSource;

    /// Whether this channel is currently connected/active.
    fn is_active(&self) -> bool;

    /// Send a response back through this channel.
    ///
    /// This is the return path — when the kernel produces a response to a
    /// message that originated from this channel, it calls `send_response()`
    /// to deliver it.
    ///
    /// Channels that don't handle their own output (e.g., TUI, which renders
    /// directly) can return `Ok(())` as a no-op.
    fn send_response(&self, message: &str, context: &ResponseContext) -> Result<(), ChannelError>;
}

/// Errors from channel operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelError {
    /// Channel is not connected/active.
    NotConnected,
    /// Failed to deliver message.
    DeliveryFailed(String),
    /// Channel doesn't support outbound messages.
    NotSupported,
    /// No channel found for the given input source.
    NotFound(String),
}

impl fmt::Display for ChannelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotConnected => write!(f, "channel is not connected"),
            Self::DeliveryFailed(msg) => write!(f, "delivery failed: {msg}"),
            Self::NotSupported => write!(f, "channel does not support outbound messages"),
            Self::NotFound(source) => write!(f, "no channel found for source: {source}"),
        }
    }
}

impl std::error::Error for ChannelError {}
