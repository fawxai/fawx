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
pub use session::{Session, SessionMessage};
pub use store::SessionStore;
pub use types::{
    InvalidSessionKey, MessageRole, SessionConfig, SessionInfo, SessionKey, SessionKind,
    SessionStatus,
};
