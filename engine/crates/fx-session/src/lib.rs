//! Multi-session management for Fawx.
//!
//! Provides a session registry that tracks concurrent conversations,
//! persists them to storage (redb), and exposes tool-friendly operations
//! for listing, inspecting, and messaging across sessions.

pub mod registry;
pub mod session;
pub mod store;
pub mod types;

pub use registry::{SessionError, SessionRegistry};
pub use session::{
    max_memory_items, max_memory_tokens, prune_unresolved_tool_history, render_content_blocks,
    render_content_blocks_with_options, validate_tool_message_order, ContentRenderOptions, Session,
    SessionContentBlock, SessionHistoryError, SessionMemory, SessionMemoryUpdate, SessionMessage,
};
pub use store::SessionStore;
pub use types::{
    InvalidSessionKey, MessageRole, SessionArchiveFilter, SessionConfig, SessionInfo, SessionKey,
    SessionKind, SessionStatus,
};
