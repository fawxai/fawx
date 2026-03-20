//! Individual session state and conversation management.

use crate::types::{
    MessageRole, SessionConfig, SessionInfo, SessionKey, SessionKind, SessionStatus,
};
use fx_llm::{ContentBlock, Message, Usage};
use serde::de::Deserializer;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

/// A structured content block stored in session history.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionContentBlock {
    /// Plain text content.
    Text { text: String },
    /// Tool invocation requested by the assistant.
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    /// Tool output associated with a prior tool invocation.
    ToolResult { tool_use_id: String, content: Value },
    /// Marker indicating an image was part of the message.
    Image { media_type: String },
}

impl From<ContentBlock> for SessionContentBlock {
    fn from(block: ContentBlock) -> Self {
        match block {
            ContentBlock::Text { text } => Self::Text { text },
            ContentBlock::ToolUse { id, name, input } => Self::ToolUse { id, name, input },
            ContentBlock::ToolResult {
                tool_use_id,
                content,
            } => Self::ToolResult {
                tool_use_id,
                content,
            },
            ContentBlock::Image { media_type, .. } => Self::Image { media_type },
        }
    }
}

impl From<SessionContentBlock> for ContentBlock {
    fn from(block: SessionContentBlock) -> Self {
        match block {
            SessionContentBlock::Text { text } => Self::Text { text },
            SessionContentBlock::ToolUse { id, name, input } => Self::ToolUse { id, name, input },
            SessionContentBlock::ToolResult {
                tool_use_id,
                content,
            } => Self::ToolResult {
                tool_use_id,
                content,
            },
            // Image payloads are intentionally not persisted; replay them as
            // a readable marker so later turns retain the fact that vision was used.
            SessionContentBlock::Image { media_type } => Self::Text {
                text: format!("[image:{media_type}]"),
            },
        }
    }
}

/// A single conversation message within a session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionMessage {
    /// Who produced this message.
    pub role: MessageRole,
    /// Message content.
    #[serde(deserialize_with = "deserialize_content")]
    pub content: Vec<SessionContentBlock>,
    /// Unix epoch seconds when the message was recorded.
    pub timestamp: u64,
    /// Tokens consumed to produce this message, when known.
    #[serde(default)]
    pub token_count: Option<u32>,
    /// Input tokens consumed by prompt/context, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_token_count: Option<u32>,
    /// Output tokens produced by generation, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_token_count: Option<u32>,
}

impl SessionMessage {
    /// Build a text-only session message.
    pub fn text(role: MessageRole, content: impl Into<String>, timestamp: u64) -> Self {
        Self {
            role,
            content: vec![SessionContentBlock::Text {
                text: content.into(),
            }],
            timestamp,
            token_count: None,
            input_token_count: None,
            output_token_count: None,
        }
    }

    /// Build a structured session message.
    pub fn structured(
        role: MessageRole,
        content: Vec<SessionContentBlock>,
        timestamp: u64,
        token_count: Option<u32>,
    ) -> Self {
        Self {
            role,
            content,
            timestamp,
            token_count,
            input_token_count: None,
            output_token_count: None,
        }
    }

    /// Build a structured session message with split token accounting.
    pub fn structured_with_usage(
        role: MessageRole,
        content: Vec<SessionContentBlock>,
        timestamp: u64,
        usage: Option<Usage>,
    ) -> Self {
        Self {
            role,
            content,
            timestamp,
            token_count: usage.map(total_token_count),
            input_token_count: usage.map(|usage| usage.input_tokens),
            output_token_count: usage.map(|usage| usage.output_tokens),
        }
    }

    /// Convert the stored message into an LLM history message.
    pub fn to_llm_message(&self) -> Message {
        Message {
            role: self.role.into(),
            content: self.content.clone().into_iter().map(Into::into).collect(),
        }
    }

    /// Return a readable text representation of the structured content.
    pub fn render_text(&self) -> String {
        render_content_blocks(&self.content)
    }

    /// Return the combined token count when available.
    pub fn total_token_count(&self) -> Option<u32> {
        self.token_count.or_else(|| {
            Some(
                self.input_token_count?
                    .saturating_add(self.output_token_count?),
            )
        })
    }
}

/// Formatting controls for rendered session content.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ContentRenderOptions {
    pub include_tool_use_id: bool,
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
        self.add_message_blocks(
            role,
            vec![SessionContentBlock::Text {
                text: content.into(),
            }],
            None,
        );
    }

    /// Append a structured message and update the timestamp.
    pub fn add_message_blocks(
        &mut self,
        role: MessageRole,
        content: Vec<SessionContentBlock>,
        token_count: Option<u32>,
    ) {
        let now = current_epoch_secs();
        self.messages
            .push(SessionMessage::structured(role, content, now, token_count));
        self.updated_at = now;
    }

    /// Append already-constructed messages and update the timestamp once.
    pub fn extend_messages(&mut self, messages: impl IntoIterator<Item = SessionMessage>) {
        let mut appended_any = false;
        for message in messages {
            self.messages.push(message);
            appended_any = true;
        }
        if appended_any {
            self.updated_at = current_epoch_secs();
        }
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
            title: self.compute_title(),
            preview: self.compute_preview(),
            model: self.model.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
            message_count: self.messages.len(),
        }
    }

    fn compute_title(&self) -> Option<String> {
        self.messages
            .iter()
            .find(|message| message.role == MessageRole::User)
            .map(|message| truncate_text(&message.render_text(), 80))
    }

    fn compute_preview(&self) -> Option<String> {
        self.messages
            .last()
            .map(|message| truncate_text(&message.render_text(), 120))
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ContentField {
    Blocks(Vec<SessionContentBlock>),
    LegacyText(String),
}

fn deserialize_content<'de, D>(deserializer: D) -> Result<Vec<SessionContentBlock>, D::Error>
where
    D: Deserializer<'de>,
{
    match ContentField::deserialize(deserializer)? {
        ContentField::Blocks(blocks) => Ok(blocks),
        ContentField::LegacyText(text) => Ok(vec![SessionContentBlock::Text { text }]),
    }
}

pub fn render_content_blocks(blocks: &[SessionContentBlock]) -> String {
    render_content_blocks_with_options(blocks, ContentRenderOptions::default())
}

/// Render structured content blocks into readable text with configurable formatting.
pub fn render_content_blocks_with_options(
    blocks: &[SessionContentBlock],
    options: ContentRenderOptions,
) -> String {
    blocks
        .iter()
        .map(|block| render_content_block_with_options(block, options))
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_content_block_with_options(
    block: &SessionContentBlock,
    options: ContentRenderOptions,
) -> String {
    match block {
        SessionContentBlock::Text { text } => text.clone(),
        SessionContentBlock::ToolUse { id, name, input } => {
            if options.include_tool_use_id {
                format!("[tool_use:{name}#{id}] {input}")
            } else {
                format!("[tool_use:{name}] {input}")
            }
        }
        SessionContentBlock::ToolResult {
            tool_use_id,
            content,
        } => format!("[tool_result:{tool_use_id}] {content}"),
        SessionContentBlock::Image { media_type } => format!("[image:{media_type}]"),
    }
}

fn total_token_count(usage: Usage) -> u32 {
    usage.input_tokens.saturating_add(usage.output_tokens)
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    let mut chars = trimmed.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}…")
    } else {
        trimmed.to_string()
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
    use serde_json::json;

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
        assert_eq!(
            session.messages[0].content,
            vec![SessionContentBlock::Text {
                text: "hello".to_string()
            }]
        );
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
        assert_eq!(recent[0].render_text(), "msg-7");
        assert_eq!(recent[2].render_text(), "msg-9");
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
        assert_eq!(info.title.as_deref(), Some("hi"));
        assert_eq!(info.preview.as_deref(), Some("hello"));
        assert_eq!(info.message_count, 2);
    }

    #[test]
    fn session_info_title_from_first_user_message() {
        let mut session = Session::new(
            SessionKey::new("s6").unwrap(),
            SessionKind::Main,
            test_config(),
        );
        session.add_message(MessageRole::Assistant, "system ready");
        session.add_message(MessageRole::User, "first user title");
        session.add_message(MessageRole::User, "second user title");

        let info = session.info();

        assert_eq!(info.title.as_deref(), Some("first user title"));
    }

    #[test]
    fn session_info_preview_from_last_message() {
        let mut session = Session::new(
            SessionKey::new("s7").unwrap(),
            SessionKind::Main,
            test_config(),
        );
        session.add_message(MessageRole::User, "hello");
        session.add_message(MessageRole::Assistant, "latest preview");

        let info = session.info();

        assert_eq!(info.preview.as_deref(), Some("latest preview"));
    }

    #[test]
    fn session_info_returns_none_when_no_messages_exist() {
        let session = Session::new(
            SessionKey::new("s8").unwrap(),
            SessionKind::Main,
            test_config(),
        );

        let info = session.info();

        assert!(info.title.is_none());
        assert!(info.preview.is_none());
    }

    #[test]
    fn truncate_text_handles_multibyte_characters() {
        let text = "🙂".repeat(81);

        let truncated = truncate_text(&text, 80);

        assert_eq!(truncated, format!("{}…", "🙂".repeat(80)));
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
        assert_eq!(restored.messages[0].render_text(), "init");
    }

    #[test]
    fn session_message_round_trips_mixed_structured_content() {
        let message = SessionMessage::structured_with_usage(
            MessageRole::Assistant,
            vec![
                SessionContentBlock::Text {
                    text: "Let me check.".to_string(),
                },
                SessionContentBlock::ToolUse {
                    id: "call_1".to_string(),
                    name: "read_file".to_string(),
                    input: json!({"path": "README.md"}),
                },
                SessionContentBlock::Image {
                    media_type: "image/png".to_string(),
                },
            ],
            123,
            Some(Usage {
                input_tokens: 17,
                output_tokens: 25,
            }),
        );

        let json = serde_json::to_string(&message).expect("serialize");
        let restored: SessionMessage = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(restored, message);
    }

    #[test]
    fn session_message_to_llm_message_preserves_structured_content() {
        let message = SessionMessage::structured(
            MessageRole::Assistant,
            vec![
                SessionContentBlock::Text {
                    text: "hello".to_string(),
                },
                SessionContentBlock::ToolUse {
                    id: "call_1".to_string(),
                    name: "search".to_string(),
                    input: json!({"q": "weather"}),
                },
                SessionContentBlock::ToolResult {
                    tool_use_id: "call_1".to_string(),
                    content: json!("sunny"),
                },
                SessionContentBlock::Image {
                    media_type: "image/png".to_string(),
                },
            ],
            123,
            Some(5),
        );

        let llm_message = message.to_llm_message();

        assert_eq!(llm_message.role, MessageRole::Assistant.into());
        assert_eq!(
            llm_message.content,
            vec![
                ContentBlock::Text {
                    text: "hello".to_string(),
                },
                ContentBlock::ToolUse {
                    id: "call_1".to_string(),
                    name: "search".to_string(),
                    input: json!({"q": "weather"}),
                },
                ContentBlock::ToolResult {
                    tool_use_id: "call_1".to_string(),
                    content: json!("sunny"),
                },
                ContentBlock::Text {
                    text: "[image:image/png]".to_string(),
                },
            ]
        );
    }

    #[test]
    fn extend_messages_appends_messages_and_updates_timestamp() {
        let mut session = Session::new(
            SessionKey::new("extend").unwrap(),
            SessionKind::Main,
            test_config(),
        );
        session.updated_at = 0;

        session.extend_messages([
            SessionMessage::text(MessageRole::User, "first", 1),
            SessionMessage::text(MessageRole::Assistant, "second", 2),
        ]);

        assert_eq!(session.messages.len(), 2);
        assert_eq!(session.messages[0].render_text(), "first");
        assert_eq!(session.messages[1].render_text(), "second");
        assert!(session.updated_at > 0);
    }

    #[test]
    fn session_message_deserializes_legacy_string_content() {
        let json = r#"{"role":"user","content":"hello","timestamp":123}"#;

        let restored: SessionMessage = serde_json::from_str(json).expect("deserialize");

        assert_eq!(
            restored.content,
            vec![SessionContentBlock::Text {
                text: "hello".to_string()
            }]
        );
        assert_eq!(restored.token_count, None);
    }

    #[test]
    fn session_content_block_converts_to_and_from_llm_content() {
        let blocks = vec![
            ContentBlock::Text {
                text: "hello".to_string(),
            },
            ContentBlock::ToolUse {
                id: "call_1".to_string(),
                name: "search".to_string(),
                input: json!({"q": "weather"}),
            },
            ContentBlock::ToolResult {
                tool_use_id: "call_1".to_string(),
                content: json!("sunny"),
            },
            ContentBlock::Image {
                media_type: "image/jpeg".to_string(),
                data: "ZmFrZQ==".to_string(),
            },
        ];

        let stored = blocks
            .clone()
            .into_iter()
            .map(SessionContentBlock::from)
            .collect::<Vec<_>>();
        let restored = stored
            .iter()
            .cloned()
            .map(ContentBlock::from)
            .collect::<Vec<_>>();

        assert_eq!(
            stored[0],
            SessionContentBlock::Text {
                text: "hello".to_string()
            }
        );
        assert_eq!(restored[0], blocks[0]);
        assert_eq!(restored[1], blocks[1]);
        assert_eq!(restored[2], blocks[2]);
        assert_eq!(
            restored[3],
            ContentBlock::Text {
                text: "[image:image/jpeg]".to_string()
            }
        );
    }

    #[test]
    fn token_count_round_trips_and_defaults_for_legacy_messages() {
        let message = SessionMessage::structured_with_usage(
            MessageRole::Assistant,
            vec![SessionContentBlock::Text {
                text: "usage".to_string(),
            }],
            123,
            Some(Usage {
                input_tokens: 44,
                output_tokens: 55,
            }),
        );

        let json = serde_json::to_string(&message).expect("serialize");
        let restored: SessionMessage = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored.token_count, Some(99));
        assert_eq!(restored.input_token_count, Some(44));
        assert_eq!(restored.output_token_count, Some(55));

        let legacy: SessionMessage =
            serde_json::from_str(r#"{"role":"assistant","content":"old","timestamp":1}"#)
                .expect("legacy deserialize");
        assert_eq!(legacy.token_count, None);
        assert_eq!(legacy.input_token_count, None);
        assert_eq!(legacy.output_token_count, None);
    }
}
