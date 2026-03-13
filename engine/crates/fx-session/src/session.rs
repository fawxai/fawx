//! Individual session state and conversation management.

use crate::types::{
    MessageRole, SessionConfig, SessionInfo, SessionKey, SessionKind, SessionStatus,
};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// A single conversation message within a session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionMessage {
    /// Who produced this message.
    pub role: MessageRole,
    /// Message content.
    pub content: String,
    /// Unix epoch seconds when the message was recorded.
    pub timestamp: u64,
}

/// Persistent session state: metadata + conversation history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Unique identifier.
    pub key: SessionKey,
    /// Session classification.
    pub kind: SessionKind,
    /// Current lifecycle status.
    pub status: SessionStatus,
    /// Optional human-readable label.
    pub label: Option<String>,
    /// Model identifier.
    pub model: String,
    /// Unix epoch seconds at creation.
    pub created_at: u64,
    /// Unix epoch seconds of last activity.
    pub updated_at: u64,
    /// Ordered conversation messages.
    pub messages: Vec<SessionMessage>,
}

impl Session {
    /// Create a new session with the given parameters.
    pub fn new(key: SessionKey, kind: SessionKind, config: SessionConfig) -> Self {
        let now = current_epoch_secs();
        Self {
            key,
            kind,
            status: SessionStatus::Active,
            label: config.label,
            model: config.model,
            created_at: now,
            updated_at: now,
            messages: Vec::new(),
        }
    }

    /// Append a message and update the timestamp.
    pub fn add_message(&mut self, role: MessageRole, content: impl Into<String>) {
        let now = current_epoch_secs();
        self.messages.push(SessionMessage {
            role,
            content: content.into(),
            timestamp: now,
        });
        self.updated_at = now;
    }

    /// Remove all recorded messages and update the timestamp.
    pub fn clear_messages(&mut self) {
        self.messages.clear();
        self.updated_at = current_epoch_secs();
    }

    /// Return the most recent `limit` messages (or all if fewer exist).
    pub fn recent_messages(&self, limit: usize) -> &[SessionMessage] {
        let start = self.messages.len().saturating_sub(limit);
        &self.messages[start..]
    }

    /// Build a summary `SessionInfo` snapshot.
    pub fn info(&self) -> SessionInfo {
        SessionInfo {
            key: self.key.clone(),
            kind: self.kind,
            status: self.status,
            label: self.label.clone(),
            model: self.model.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
            message_count: self.messages.len(),
        }
    }
}

fn current_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> SessionConfig {
        SessionConfig {
            label: Some("test-session".to_string()),
            model: "gpt-4".to_string(),
        }
    }

    #[test]
    fn new_session_starts_active_with_no_messages() {
        let session = Session::new(
            SessionKey::new("s1").unwrap(),
            SessionKind::Main,
            test_config(),
        );
        assert_eq!(session.status, SessionStatus::Active);
        assert!(session.messages.is_empty());
        assert_eq!(session.label.as_deref(), Some("test-session"));
        assert_eq!(session.model, "gpt-4");
    }

    #[test]
    fn add_message_increments_count_and_updates_timestamp() {
        let mut session = Session::new(
            SessionKey::new("s2").unwrap(),
            SessionKind::Subagent,
            test_config(),
        );
        let before = session.updated_at;
        session.add_message(MessageRole::User, "hello");
        assert_eq!(session.messages.len(), 1);
        assert!(session.updated_at >= before);
        assert_eq!(session.messages[0].role, MessageRole::User);
        assert_eq!(session.messages[0].content, "hello");
    }

    #[test]
    fn recent_messages_returns_tail_slice() {
        let mut session = Session::new(
            SessionKey::new("s3").unwrap(),
            SessionKind::Main,
            test_config(),
        );
        for i in 0..10 {
            session.add_message(MessageRole::User, format!("msg-{i}"));
        }
        let recent = session.recent_messages(3);
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].content, "msg-7");
        assert_eq!(recent[2].content, "msg-9");
    }

    #[test]
    fn recent_messages_returns_all_when_limit_exceeds_count() {
        let mut session = Session::new(
            SessionKey::new("s4").unwrap(),
            SessionKind::Main,
            test_config(),
        );
        session.add_message(MessageRole::User, "only one");
        let recent = session.recent_messages(100);
        assert_eq!(recent.len(), 1);
    }

    #[test]
    fn info_snapshot_reflects_session_state() {
        let mut session = Session::new(
            SessionKey::new("s5").unwrap(),
            SessionKind::Channel,
            test_config(),
        );
        session.add_message(MessageRole::User, "hi");
        session.add_message(MessageRole::Assistant, "hello");
        let info = session.info();
        assert_eq!(info.key, SessionKey::new("s5").unwrap());
        assert_eq!(info.kind, SessionKind::Channel);
        assert_eq!(info.message_count, 2);
    }

    #[test]
    fn session_round_trips_through_json() {
        let mut session = Session::new(
            SessionKey::new("rt").unwrap(),
            SessionKind::Cron,
            SessionConfig {
                label: None,
                model: "claude".to_string(),
            },
        );
        session.add_message(MessageRole::System, "init");
        let json = serde_json::to_string(&session).expect("serialize");
        let restored: Session = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored.key, session.key);
        assert_eq!(restored.kind, session.kind);
        assert_eq!(restored.messages.len(), 1);
        assert_eq!(restored.messages[0].content, "init");
    }
}
