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

/// Filter for including active and/or archived sessions in registry listings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SessionArchiveFilter {
    /// Return active sessions only.
    #[default]
    ActiveOnly,
    /// Return both active and archived sessions.
    All,
    /// Return archived sessions only.
    ArchivedOnly,
}

impl SessionArchiveFilter {
    pub fn matches(self, is_archived: bool) -> bool {
        match self {
            Self::ActiveOnly => !is_archived,
            Self::All => true,
            Self::ArchivedOnly => is_archived,
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
    /// Title derived from the first user message.
    #[serde(default)]
    pub title: Option<String>,
    /// Preview of the most recent message.
    #[serde(default)]
    pub preview: Option<String>,
    /// Model identifier used by this session.
    pub model: String,
    /// Unix epoch seconds when session was created.
    pub created_at: u64,
    /// Unix epoch seconds of last activity.
    pub updated_at: u64,
    /// Unix epoch seconds when the session was archived, if archived.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<u64>,
    /// Number of messages in the conversation.
    pub message_count: usize,
}

impl SessionInfo {
    pub fn is_archived(&self) -> bool {
        self.archived_at.is_some()
    }
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
    /// Tool execution result associated with a prior assistant tool-use.
    Tool,
}

impl fmt::Display for MessageRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::User => f.write_str("user"),
            Self::Assistant => f.write_str("assistant"),
            Self::System => f.write_str("system"),
            Self::Tool => f.write_str("tool"),
        }
    }
}

impl From<MessageRole> for fx_llm::MessageRole {
    fn from(value: MessageRole) -> Self {
        match value {
            MessageRole::User => Self::User,
            MessageRole::Assistant => Self::Assistant,
            MessageRole::System => Self::System,
            MessageRole::Tool => Self::Tool,
        }
    }
}

impl From<fx_llm::MessageRole> for MessageRole {
    fn from(value: fx_llm::MessageRole) -> Self {
        match value {
            fx_llm::MessageRole::User => Self::User,
            fx_llm::MessageRole::Assistant => Self::Assistant,
            fx_llm::MessageRole::System => Self::System,
            fx_llm::MessageRole::Tool => Self::Tool,
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

    fn sample_session_info(key: &str, archived_at: Option<u64>) -> SessionInfo {
        SessionInfo {
            key: SessionKey::new(key).expect("session key"),
            kind: SessionKind::Main,
            status: SessionStatus::Active,
            label: Some("primary".to_string()),
            title: Some("Hello world".to_string()),
            preview: Some("Latest message".to_string()),
            model: "gpt-4".to_string(),
            created_at: 1000,
            updated_at: 2000,
            archived_at,
            message_count: 5,
        }
    }

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
        assert_eq!(MessageRole::Tool.to_string(), "tool");
    }

    #[test]
    fn message_role_converts_into_llm_role() {
        assert_eq!(
            fx_llm::MessageRole::from(MessageRole::User),
            fx_llm::MessageRole::User
        );
        assert_eq!(
            fx_llm::MessageRole::from(MessageRole::Assistant),
            fx_llm::MessageRole::Assistant
        );
        assert_eq!(
            fx_llm::MessageRole::from(MessageRole::System),
            fx_llm::MessageRole::System
        );
        assert_eq!(
            fx_llm::MessageRole::from(MessageRole::Tool),
            fx_llm::MessageRole::Tool
        );
    }

    #[test]
    fn message_role_converts_from_llm_role() {
        assert_eq!(
            MessageRole::from(fx_llm::MessageRole::User),
            MessageRole::User
        );
        assert_eq!(
            MessageRole::from(fx_llm::MessageRole::Assistant),
            MessageRole::Assistant
        );
        assert_eq!(
            MessageRole::from(fx_llm::MessageRole::System),
            MessageRole::System
        );
        assert_eq!(
            MessageRole::from(fx_llm::MessageRole::Tool),
            MessageRole::Tool
        );
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
        let info = sample_session_info("sess-1", Some(3000));
        let json = serde_json::to_string(&info).expect("serialize");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse");
        assert_eq!(parsed["kind"], "main");
        assert_eq!(parsed["status"], "active");
        assert_eq!(parsed["title"], "Hello world");
        assert_eq!(parsed["preview"], "Latest message");
        assert_eq!(parsed["archived_at"], 3000);
        assert_eq!(parsed["message_count"], 5);
    }

    #[test]
    fn archived_session_metadata_round_trips_through_json() {
        let info = SessionInfo {
            key: SessionKey::new("sess-rt").unwrap(),
            kind: SessionKind::Subagent,
            status: SessionStatus::Completed,
            label: None,
            title: None,
            preview: None,
            model: "claude-3".to_string(),
            created_at: 100,
            updated_at: 200,
            archived_at: Some(1234),
            message_count: 10,
        };
        let json = serde_json::to_string(&info).expect("serialize");
        let restored: SessionInfo = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored.key, info.key);
        assert_eq!(restored.kind, info.kind);
        assert_eq!(restored.status, info.status);
        assert_eq!(restored.title, info.title);
        assert_eq!(restored.preview, info.preview);
        assert_eq!(restored.model, info.model);
        assert_eq!(restored.archived_at, info.archived_at);
        assert!(restored.is_archived());
    }

    #[test]
    fn legacy_active_session_deserializes_with_no_archive_timestamp() {
        let json = r#"{
            "key":"sess-legacy",
            "kind":"main",
            "status":"idle",
            "label":null,
            "model":"gpt-4",
            "created_at":1,
            "updated_at":2,
            "message_count":0
        }"#;

        let info: SessionInfo = serde_json::from_str(json).expect("deserialize legacy");

        assert!(info.title.is_none());
        assert!(info.preview.is_none());
        assert!(info.archived_at.is_none());
        assert!(!info.is_archived());
    }

    #[test]
    fn is_archived_reports_metadata_presence() {
        let active = sample_session_info("sess-active", None);
        let archived = sample_session_info("sess-archived", Some(42));

        assert!(!active.is_archived());
        assert!(archived.is_archived());
    }

    #[test]
    fn archive_filter_defaults_to_active_only() {
        assert_eq!(
            SessionArchiveFilter::default(),
            SessionArchiveFilter::ActiveOnly
        );
    }

    #[test]
    fn archive_filter_matches_expected_archive_states() {
        assert!(SessionArchiveFilter::ActiveOnly.matches(false));
        assert!(!SessionArchiveFilter::ActiveOnly.matches(true));
        assert!(SessionArchiveFilter::All.matches(false));
        assert!(SessionArchiveFilter::All.matches(true));
        assert!(!SessionArchiveFilter::ArchivedOnly.matches(false));
        assert!(SessionArchiveFilter::ArchivedOnly.matches(true));
    }
}
