//! Core types for session management.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Error when constructing a [`SessionKey`] with invalid input.
#[derive(Debug, thiserror::Error)]
#[error("session key must not be empty or whitespace-only")]
pub struct InvalidSessionKey;

/// Unique identifier for a session (UUID or label).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionKey(pub(crate) String);

impl fmt::Display for SessionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl SessionKey {
    /// Create a new session key from a string.
    ///
    /// Returns an error if `key` is empty or whitespace-only.
    pub fn new(key: impl Into<String>) -> Result<Self, InvalidSessionKey> {
        let s = key.into();
        if s.trim().is_empty() {
            return Err(InvalidSessionKey);
        }
        Ok(Self(s))
    }

    /// Returns the inner key string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Classification of session origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionKind {
    /// Primary user conversation.
    Main,
    /// Spawned by a parent session.
    Subagent,
    /// Per-channel session (Telegram, Matrix, etc.).
    Channel,
    /// Scheduled/cron task.
    Cron,
}

impl fmt::Display for SessionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Main => f.write_str("main"),
            Self::Subagent => f.write_str("subagent"),
            Self::Channel => f.write_str("channel"),
            Self::Cron => f.write_str("cron"),
        }
    }
}

/// Current lifecycle status of a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    /// Session is actively processing.
    Active,
    /// Session exists but has no pending work.
    Idle,
    /// Session finished its task.
    Completed,
    /// Session terminated due to an error.
    Failed,
    /// Session is temporarily suspended.
    Paused,
}

impl fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Active => f.write_str("active"),
            Self::Idle => f.write_str("idle"),
            Self::Completed => f.write_str("completed"),
            Self::Failed => f.write_str("failed"),
            Self::Paused => f.write_str("paused"),
        }
    }
}

/// Summary metadata for a session (returned by list operations).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    /// Unique key identifying this session.
    pub key: SessionKey,
    /// What kind of session this is.
    pub kind: SessionKind,
    /// Current lifecycle status.
    pub status: SessionStatus,
    /// Optional human-readable label.
    pub label: Option<String>,
    /// Model identifier used by this session.
    pub model: String,
    /// Unix epoch seconds when session was created.
    pub created_at: u64,
    /// Unix epoch seconds of last activity.
    pub updated_at: u64,
    /// Number of messages in the conversation.
    pub message_count: usize,
}

/// Role of a message in a conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    /// Message from the user.
    User,
    /// Message from the assistant/model.
    Assistant,
    /// System-level instruction or context.
    System,
}

impl fmt::Display for MessageRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::User => f.write_str("user"),
            Self::Assistant => f.write_str("assistant"),
            Self::System => f.write_str("system"),
        }
    }
}

/// Configuration for creating a new session.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Optional label for the session.
    pub label: Option<String>,
    /// Model to use for this session.
    pub model: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_key_display_shows_inner_value() {
        let key = SessionKey::new("abc-123").unwrap();
        assert_eq!(key.to_string(), "abc-123");
    }

    #[test]
    fn session_key_rejects_empty() {
        assert!(SessionKey::new("").is_err());
    }

    #[test]
    fn session_key_rejects_whitespace_only() {
        assert!(SessionKey::new("   ").is_err());
        assert!(SessionKey::new("\t\n").is_err());
    }

    #[test]
    fn session_kind_display_matches_variant() {
        assert_eq!(SessionKind::Main.to_string(), "main");
        assert_eq!(SessionKind::Subagent.to_string(), "subagent");
        assert_eq!(SessionKind::Channel.to_string(), "channel");
        assert_eq!(SessionKind::Cron.to_string(), "cron");
    }

    #[test]
    fn session_status_display_matches_variant() {
        assert_eq!(SessionStatus::Active.to_string(), "active");
        assert_eq!(SessionStatus::Idle.to_string(), "idle");
        assert_eq!(SessionStatus::Completed.to_string(), "completed");
        assert_eq!(SessionStatus::Failed.to_string(), "failed");
        assert_eq!(SessionStatus::Paused.to_string(), "paused");
    }

    #[test]
    fn message_role_display_matches_variant() {
        assert_eq!(MessageRole::User.to_string(), "user");
        assert_eq!(MessageRole::Assistant.to_string(), "assistant");
        assert_eq!(MessageRole::System.to_string(), "system");
    }

    /// Regression test: the inner field of `SessionKey` must not be
    /// directly constructible from outside the crate, which would allow
    /// bypassing `SessionKey::new()` validation (e.g., empty keys).
    #[test]
    fn session_key_as_str_returns_inner_value() {
        let key = SessionKey::new("my-session").unwrap();
        assert_eq!(key.as_str(), "my-session");
    }

    #[test]
    fn session_key_equality() {
        let a = SessionKey::new("test").unwrap();
        let b = SessionKey::new("test").unwrap();
        let c = SessionKey::new("other").unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn session_info_serializes_to_json() {
        let info = SessionInfo {
            key: SessionKey::new("sess-1").unwrap(),
            kind: SessionKind::Main,
            status: SessionStatus::Active,
            label: Some("primary".to_string()),
            model: "gpt-4".to_string(),
            created_at: 1000,
            updated_at: 2000,
            message_count: 5,
        };
        let json = serde_json::to_string(&info).expect("serialize");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse");
        assert_eq!(parsed["kind"], "main");
        assert_eq!(parsed["status"], "active");
        assert_eq!(parsed["message_count"], 5);
    }

    #[test]
    fn session_info_round_trips_through_json() {
        let info = SessionInfo {
            key: SessionKey::new("sess-rt").unwrap(),
            kind: SessionKind::Subagent,
            status: SessionStatus::Completed,
            label: None,
            model: "claude-3".to_string(),
            created_at: 100,
            updated_at: 200,
            message_count: 10,
        };
        let json = serde_json::to_string(&info).expect("serialize");
        let restored: SessionInfo = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored.key, info.key);
        assert_eq!(restored.kind, info.kind);
        assert_eq!(restored.status, info.status);
        assert_eq!(restored.model, info.model);
    }
}
