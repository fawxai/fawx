//! Individual session state and conversation management.

use crate::types::{
    MessageRole, SessionConfig, SessionInfo, SessionKey, SessionKind, SessionStatus,
};
use fx_llm::{ContentBlock, Message, Usage};
use serde::de::Deserializer;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_id: Option<String>,
        name: String,
        input: Value,
    },
    /// Tool output associated with a prior tool invocation.
    ToolResult {
        tool_use_id: String,
        content: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
    /// Persisted image attachment data for client-side rendering.
    Image {
        media_type: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        data: Option<String>,
    },
    /// Persisted document attachment data for client-side rendering.
    Document {
        media_type: String,
        data: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
    },
}

impl From<ContentBlock> for SessionContentBlock {
    fn from(block: ContentBlock) -> Self {
        match block {
            ContentBlock::Text { text } => Self::Text { text },
            ContentBlock::ToolUse {
                id,
                provider_id,
                name,
                input,
            } => Self::ToolUse {
                id,
                provider_id,
                name,
                input,
            },
            ContentBlock::ToolResult {
                tool_use_id,
                content,
            } => Self::ToolResult {
                tool_use_id,
                content,
                is_error: None,
            },
            ContentBlock::Image { media_type, data } => Self::Image {
                media_type,
                data: Some(data),
            },
            ContentBlock::Document {
                media_type,
                data,
                filename,
            } => Self::Document {
                media_type,
                data,
                filename,
            },
        }
    }
}

impl From<SessionContentBlock> for ContentBlock {
    fn from(block: SessionContentBlock) -> Self {
        match block {
            SessionContentBlock::Text { text } => Self::Text { text },
            SessionContentBlock::ToolUse {
                id,
                provider_id,
                name,
                input,
            } => Self::ToolUse {
                id,
                provider_id,
                name,
                input,
            },
            SessionContentBlock::ToolResult {
                tool_use_id,
                content,
                ..
            } => Self::ToolResult {
                tool_use_id,
                content,
            },
            // Image and document payloads are persisted in session history for
            // client-side rendering, but replayed as text markers in LLM context
            // to avoid re-sending large binary data on subsequent turns.
            SessionContentBlock::Image { media_type, .. } => Self::Text {
                text: format!("[image:{media_type}]"),
            },
            SessionContentBlock::Document {
                media_type,
                filename,
                ..
            } => Self::Text {
                text: filename
                    .map(|filename| format!("[document:{media_type}:{filename}]"))
                    .unwrap_or_else(|| format!("[document:{media_type}]")),
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
            content: normalized_llm_content(&self.content),
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

/// Errors raised when session history violates tool ordering invariants.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SessionHistoryError {
    #[error(
        "invalid tool history: tool result '{tool_use_id}' at message {message_index} block {block_index} has no matching earlier tool_use"
    )]
    ToolResultBeforeToolUse {
        tool_use_id: String,
        message_index: usize,
        block_index: usize,
    },
}

const DEFAULT_SESSION_MEMORY_MAX_ITEMS: usize = 40;
const DEFAULT_SESSION_MEMORY_MAX_TOKENS: usize = 4_000;

/// Compute the session memory token cap for a given model context window.
#[must_use]
pub fn max_memory_tokens(context_limit: usize) -> usize {
    (context_limit / 50).clamp(2_000, 8_000)
}

/// Compute the per-list session memory item cap for a given model context window.
#[must_use]
pub fn max_memory_items(context_limit: usize) -> usize {
    (context_limit / 5_000).clamp(20, 80)
}

fn default_session_memory_max_items() -> usize {
    DEFAULT_SESSION_MEMORY_MAX_ITEMS
}

fn default_session_memory_max_tokens() -> usize {
    DEFAULT_SESSION_MEMORY_MAX_TOKENS
}

/// Persistent session memory that survives conversation compaction.
/// Contains key facts the agent extracted about the session's purpose and state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMemory {
    /// What this session is about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// Current state of work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_state: Option<String>,
    /// Key decisions made during this session.
    #[serde(default)]
    pub key_decisions: Vec<String>,
    /// Files actively being worked on.
    #[serde(default)]
    pub active_files: Vec<String>,
    /// Custom context the agent wants to remember.
    #[serde(default)]
    pub custom_context: Vec<String>,
    /// Unix epoch seconds of last update.
    #[serde(default)]
    pub last_updated: u64,
    /// Runtime-only list cap applied to tracked memory collections.
    #[serde(skip, default = "default_session_memory_max_items")]
    max_items: usize,
    /// Runtime-only token cap applied to rendered session memory.
    #[serde(skip, default = "default_session_memory_max_tokens")]
    max_tokens: usize,
}

impl Default for SessionMemory {
    fn default() -> Self {
        Self {
            project: None,
            current_state: None,
            key_decisions: Vec::new(),
            active_files: Vec::new(),
            custom_context: Vec::new(),
            last_updated: 0,
            max_items: DEFAULT_SESSION_MEMORY_MAX_ITEMS,
            max_tokens: DEFAULT_SESSION_MEMORY_MAX_TOKENS,
        }
    }
}

impl PartialEq for SessionMemory {
    fn eq(&self, other: &Self) -> bool {
        self.project == other.project
            && self.current_state == other.current_state
            && self.key_decisions == other.key_decisions
            && self.active_files == other.active_files
            && self.custom_context == other.custom_context
            && self.last_updated == other.last_updated
    }
}

impl Eq for SessionMemory {}

impl SessionMemory {
    /// Create empty session memory configured for a specific model context window.
    #[must_use]
    pub fn with_context_limit(context_limit: usize) -> Self {
        let mut memory = Self::default();
        memory.set_context_limit(context_limit);
        memory
    }

    /// Recompute runtime caps for a new model context window.
    pub fn set_context_limit(&mut self, context_limit: usize) {
        self.max_items = max_memory_items(context_limit);
        self.max_tokens = max_memory_tokens(context_limit);
        self.trim_to_item_cap();
    }

    /// Return the current per-list item cap.
    #[must_use]
    pub fn item_cap(&self) -> usize {
        self.max_items
    }

    /// Return the current token cap.
    #[must_use]
    pub fn token_cap(&self) -> usize {
        self.max_tokens
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.project.is_none()
            && self.current_state.is_none()
            && self.key_decisions.is_empty()
            && self.active_files.is_empty()
            && self.custom_context.is_empty()
    }

    /// Estimated token count for the rendered memory block.
    #[must_use]
    pub fn estimated_tokens(&self) -> usize {
        let text = self.render();
        if text.is_empty() {
            return 0;
        }
        text.chars()
            .count()
            .div_ceil(4)
            .max(text.split_whitespace().count())
            .max(1)
    }

    /// Apply an update from the agent's tool call.
    /// Returns `Err(String)` with a user-facing message if the update
    /// would exceed the session memory token cap.
    pub fn apply_update(&mut self, update: SessionMemoryUpdate) -> Result<(), String> {
        let mut candidate = self.clone();
        if let Some(project) = update.project {
            candidate.project = Some(project);
        }
        if let Some(state) = update.current_state {
            candidate.current_state = Some(state);
        }
        if let Some(decisions) = update.key_decisions {
            append_capped_items(&mut candidate.key_decisions, decisions, candidate.max_items);
        }
        if let Some(files) = update.active_files {
            candidate.active_files = files;
            trim_oldest_items(&mut candidate.active_files, candidate.max_items);
        }
        if let Some(context) = update.custom_context {
            append_capped_items(&mut candidate.custom_context, context, candidate.max_items);
        }
        let estimated_tokens = candidate.estimated_tokens();
        if estimated_tokens > candidate.max_tokens {
            return Err(format!(
                "Session memory would exceed {} token cap ({} estimated). Be more concise.",
                candidate.max_tokens, estimated_tokens
            ));
        }
        candidate.last_updated = current_epoch_secs();
        *self = candidate;
        Ok(())
    }

    /// Render the memory as a human-readable text block.
    pub fn render(&self) -> String {
        if self.is_empty() {
            return String::new();
        }
        let mut lines = vec!["[Session Memory]".to_string()];
        if let Some(project) = &self.project {
            lines.push(format!("Project: {project}"));
        }
        if let Some(state) = &self.current_state {
            lines.push(format!("Current state: {state}"));
        }
        push_session_memory_items(&mut lines, "Key decisions:", &self.key_decisions);
        push_session_memory_items(&mut lines, "Active files:", &self.active_files);
        push_session_memory_items(&mut lines, "Context:", &self.custom_context);
        lines.join("\n")
    }

    fn trim_to_item_cap(&mut self) {
        trim_oldest_items(&mut self.key_decisions, self.max_items);
        trim_oldest_items(&mut self.active_files, self.max_items);
        trim_oldest_items(&mut self.custom_context, self.max_items);
    }
}

/// Partial update to session memory from the agent's tool call.
#[derive(Debug, Clone, Deserialize)]
pub struct SessionMemoryUpdate {
    pub project: Option<String>,
    pub current_state: Option<String>,
    pub key_decisions: Option<Vec<String>>,
    pub active_files: Option<Vec<String>>,
    pub custom_context: Option<Vec<String>>,
}

fn append_capped_items(items: &mut Vec<String>, incoming: Vec<String>, max_items: usize) {
    items.extend(incoming);
    trim_oldest_items(items, max_items);
}

fn trim_oldest_items(items: &mut Vec<String>, max_items: usize) {
    let excess = items.len().saturating_sub(max_items);
    if excess > 0 {
        items.drain(..excess);
    }
}

fn push_session_memory_items(lines: &mut Vec<String>, heading: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    lines.push(heading.to_string());
    for item in items {
        lines.push(format!("- {item}"));
    }
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
    /// Unix epoch seconds when the session was archived, if archived.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<u64>,
    /// Ordered conversation messages.
    pub messages: Vec<SessionMessage>,
    /// Persistent memory that survives compaction.
    #[serde(default)]
    pub memory: SessionMemory,
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
            archived_at: None,
            messages: Vec::new(),
            memory: SessionMemory::default(),
        }
    }

    /// Append a message and update the timestamp.
    pub fn add_message(
        &mut self,
        role: MessageRole,
        content: impl Into<String>,
    ) -> Result<(), SessionHistoryError> {
        self.add_message_blocks(
            role,
            vec![SessionContentBlock::Text {
                text: content.into(),
            }],
            None,
        )
    }

    /// Append a structured message and update the timestamp.
    pub fn add_message_blocks(
        &mut self,
        role: MessageRole,
        content: Vec<SessionContentBlock>,
        token_count: Option<u32>,
    ) -> Result<(), SessionHistoryError> {
        let now = current_epoch_secs();
        self.extend_messages([SessionMessage::structured(role, content, now, token_count)])
    }

    /// Append already-constructed messages and update the timestamp once.
    pub fn extend_messages(
        &mut self,
        messages: impl IntoIterator<Item = SessionMessage>,
    ) -> Result<(), SessionHistoryError> {
        let messages = messages.into_iter().collect::<Vec<_>>();
        if messages.is_empty() {
            return Ok(());
        }

        let mut seen_tool_uses = HashSet::new();
        validate_tool_message_order_with_seen(
            self.messages.iter().enumerate(),
            &mut seen_tool_uses,
        )?;
        validate_tool_message_order_with_seen(
            messages
                .iter()
                .enumerate()
                .map(|(offset, message)| (self.messages.len() + offset, message)),
            &mut seen_tool_uses,
        )?;

        self.messages.extend(messages);
        self.updated_at = current_epoch_secs();
        Ok(())
    }

    pub fn set_memory(&mut self, memory: SessionMemory) {
        self.memory = memory;
        self.updated_at = current_epoch_secs();
    }

    /// Remove all recorded messages and update the timestamp.
    pub fn clear_messages(&mut self) {
        self.messages.clear();
        self.updated_at = current_epoch_secs();
    }

    pub fn is_archived(&self) -> bool {
        self.archived_at.is_some()
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
            archived_at: self.archived_at,
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

    pub fn validate_history(&self) -> Result<(), SessionHistoryError> {
        validate_tool_message_order(&self.messages)
    }
}

/// Validate that each stored `ToolResult` references a matching earlier `ToolUse`.
pub fn validate_tool_message_order(messages: &[SessionMessage]) -> Result<(), SessionHistoryError> {
    let mut seen_tool_uses = HashSet::new();
    validate_tool_message_order_with_seen(messages.iter().enumerate(), &mut seen_tool_uses)
}

/// Drop tool-call blocks that cannot be replayed safely on a later turn.
pub fn prune_unresolved_tool_history(messages: &[SessionMessage]) -> Vec<SessionMessage> {
    let mut state = ReplaySafeToolHistory::new(messages);
    messages
        .iter()
        .filter_map(|message| prune_message_for_replay(message, &mut state))
        .collect()
}

fn validate_tool_message_order_with_seen<'a>(
    messages: impl IntoIterator<Item = (usize, &'a SessionMessage)>,
    seen_tool_uses: &mut HashSet<String>,
) -> Result<(), SessionHistoryError> {
    for (message_index, message) in messages {
        for (block_index, block) in message.content.iter().enumerate() {
            match block {
                SessionContentBlock::ToolUse { id, .. } => {
                    let trimmed = id.trim();
                    if !trimmed.is_empty() {
                        seen_tool_uses.insert(trimmed.to_string());
                    }
                }
                SessionContentBlock::ToolResult { tool_use_id, .. } => {
                    let trimmed = tool_use_id.trim();
                    if !trimmed.is_empty() && !seen_tool_uses.contains(trimmed) {
                        return Err(SessionHistoryError::ToolResultBeforeToolUse {
                            tool_use_id: trimmed.to_string(),
                            message_index,
                            block_index,
                        });
                    }
                }
                SessionContentBlock::Text { .. }
                | SessionContentBlock::Image { .. }
                | SessionContentBlock::Document { .. } => {}
            }
        }
    }

    Ok(())
}

struct ReplaySafeToolHistory {
    remaining_tool_results: HashMap<String, usize>,
    seen_tool_uses: HashSet<String>,
}

impl ReplaySafeToolHistory {
    fn new(messages: &[SessionMessage]) -> Self {
        Self {
            remaining_tool_results: remaining_tool_result_counts(messages),
            seen_tool_uses: HashSet::new(),
        }
    }

    fn keep_tool_use(&mut self, tool_use_id: &str) -> bool {
        let trimmed = tool_use_id.trim();
        if trimmed.is_empty() {
            return true;
        }

        let keep = self
            .remaining_tool_results
            .get(trimmed)
            .copied()
            .unwrap_or_default()
            > 0;
        if keep {
            self.seen_tool_uses.insert(trimmed.to_string());
        }
        keep
    }

    fn keep_tool_result(&mut self, tool_use_id: &str) -> bool {
        let trimmed = tool_use_id.trim();
        if trimmed.is_empty() {
            return true;
        }

        let keep = self.seen_tool_uses.contains(trimmed);
        decrement_remaining_tool_result(&mut self.remaining_tool_results, trimmed);
        keep
    }
}

fn remaining_tool_result_counts(messages: &[SessionMessage]) -> HashMap<String, usize> {
    let mut remaining = HashMap::new();
    for message in messages {
        for block in &message.content {
            if let SessionContentBlock::ToolResult { tool_use_id, .. } = block {
                let trimmed = tool_use_id.trim();
                if !trimmed.is_empty() {
                    *remaining.entry(trimmed.to_string()).or_default() += 1;
                }
            }
        }
    }
    remaining
}

fn prune_message_for_replay(
    message: &SessionMessage,
    state: &mut ReplaySafeToolHistory,
) -> Option<SessionMessage> {
    let content = message
        .content
        .iter()
        .filter_map(|block| prune_block_for_replay(block, state))
        .collect::<Vec<_>>();
    (!content.is_empty()).then_some(SessionMessage {
        role: message.role,
        content,
        timestamp: message.timestamp,
        token_count: message.token_count,
        input_token_count: message.input_token_count,
        output_token_count: message.output_token_count,
    })
}

fn prune_block_for_replay(
    block: &SessionContentBlock,
    state: &mut ReplaySafeToolHistory,
) -> Option<SessionContentBlock> {
    match block {
        SessionContentBlock::ToolUse { id, .. } => state.keep_tool_use(id).then(|| block.clone()),
        SessionContentBlock::ToolResult { tool_use_id, .. } => {
            state.keep_tool_result(tool_use_id).then(|| block.clone())
        }
        SessionContentBlock::Text { .. }
        | SessionContentBlock::Image { .. }
        | SessionContentBlock::Document { .. } => Some(block.clone()),
    }
}

fn decrement_remaining_tool_result(
    remaining_tool_results: &mut HashMap<String, usize>,
    tool_use_id: &str,
) {
    let Some(count) = remaining_tool_results.get(tool_use_id).copied() else {
        return;
    };

    if count <= 1 {
        remaining_tool_results.remove(tool_use_id);
    } else {
        remaining_tool_results.insert(tool_use_id.to_string(), count - 1);
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

fn normalized_llm_content(blocks: &[SessionContentBlock]) -> Vec<ContentBlock> {
    let mut normalized = Vec::with_capacity(blocks.len());
    let mut tool_use_indices: HashMap<String, usize> = HashMap::new();

    for block in blocks.iter().cloned() {
        match block {
            SessionContentBlock::ToolUse {
                id,
                provider_id,
                name,
                input,
            } => {
                if let Some(existing_index) = tool_use_indices.get(&id).copied() {
                    let should_replace = matches!(
                        normalized.get(existing_index),
                        Some(ContentBlock::ToolUse {
                            provider_id: existing_provider_id,
                            ..
                        }) if existing_provider_id.is_none() && provider_id.is_some()
                    );
                    if should_replace {
                        normalized[existing_index] = ContentBlock::ToolUse {
                            id,
                            provider_id,
                            name,
                            input,
                        };
                    }
                    continue;
                }

                tool_use_indices.insert(id.clone(), normalized.len());
                normalized.push(ContentBlock::ToolUse {
                    id,
                    provider_id,
                    name,
                    input,
                });
            }
            other => normalized.push(other.into()),
        }
    }

    normalized
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
        SessionContentBlock::ToolUse {
            id, name, input, ..
        } => {
            if options.include_tool_use_id {
                format!("[tool_use:{name}#{id}] {input}")
            } else {
                format!("[tool_use:{name}] {input}")
            }
        }
        SessionContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => {
            if is_error == &Some(true) {
                format!("[tool_result:{tool_use_id} error] {content}")
            } else {
                format!("[tool_result:{tool_use_id}] {content}")
            }
        }
        SessionContentBlock::Image { media_type, .. } => format!("[image:{media_type}]"),
        SessionContentBlock::Document {
            media_type,
            filename,
            ..
        } => filename
            .as_ref()
            .map(|filename| format!("[document:{media_type}:{filename}]"))
            .unwrap_or_else(|| format!("[document:{media_type}]")),
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

    fn memory_update() -> SessionMemoryUpdate {
        SessionMemoryUpdate {
            project: None,
            current_state: None,
            key_decisions: None,
            active_files: None,
            custom_context: None,
        }
    }

    #[test]
    fn session_memory_default_is_empty_and_uses_default_caps() {
        let memory = SessionMemory::default();

        assert!(memory.is_empty());
        assert_eq!(memory.token_cap(), DEFAULT_SESSION_MEMORY_MAX_TOKENS);
        assert_eq!(memory.item_cap(), DEFAULT_SESSION_MEMORY_MAX_ITEMS);
    }

    #[test]
    fn max_memory_tokens_scales_with_context_limit() {
        assert_eq!(max_memory_tokens(32_000), 2_000);
        assert_eq!(max_memory_tokens(200_000), 4_000);
        assert_eq!(max_memory_tokens(300_000), 6_000);
    }

    #[test]
    fn max_memory_items_scales_with_context_limit() {
        assert_eq!(max_memory_items(32_000), 20);
        assert_eq!(max_memory_items(200_000), 40);
        assert_eq!(max_memory_items(300_000), 60);
        assert_eq!(max_memory_items(500_000), 80);
    }

    #[test]
    fn max_memory_tokens_clamps_at_boundaries() {
        assert_eq!(max_memory_tokens(16_000), 2_000);
        assert_eq!(max_memory_tokens(500_000), 8_000);
    }

    #[test]
    fn session_memory_round_trip() {
        let memory = SessionMemory {
            project: Some("Phase 3".to_string()),
            current_state: Some("wiring tests".to_string()),
            key_decisions: vec!["use a shared arc".to_string()],
            active_files: vec!["engine/crates/fx-session/src/session.rs".to_string()],
            custom_context: vec!["keep it concise".to_string()],
            last_updated: 123,
            ..SessionMemory::default()
        };

        let json = serde_json::to_string(&memory).expect("serialize memory");
        let restored: SessionMemory = serde_json::from_str(&json).expect("deserialize memory");

        assert_eq!(restored, memory);
    }

    #[test]
    fn session_memory_serializes_empty_collections_as_empty_arrays() {
        let value = serde_json::to_value(SessionMemory::default()).expect("serialize memory");

        assert_eq!(
            value,
            json!({
                "key_decisions": [],
                "active_files": [],
                "custom_context": [],
                "last_updated": 0
            })
        );
    }

    #[test]
    fn session_backward_compat_defaults_memory_when_missing() {
        let session = Session::new(
            SessionKey::new("compat").unwrap(),
            SessionKind::Main,
            test_config(),
        );
        let mut value = serde_json::to_value(&session).expect("serialize session");
        let Some(object) = value.as_object_mut() else {
            panic!("session json should be an object");
        };
        object.remove("memory");

        let restored: Session = serde_json::from_value(value).expect("deserialize session");

        assert!(restored.memory.is_empty());
        assert_eq!(
            restored.memory.token_cap(),
            DEFAULT_SESSION_MEMORY_MAX_TOKENS
        );
        assert_eq!(restored.memory.item_cap(), DEFAULT_SESSION_MEMORY_MAX_ITEMS);
    }

    #[test]
    fn session_backward_compat_defaults_archive_metadata_when_missing() {
        let session = Session::new(
            SessionKey::new("legacy-archive").unwrap(),
            SessionKind::Main,
            test_config(),
        );
        let mut value = serde_json::to_value(&session).expect("serialize session");
        let Some(object) = value.as_object_mut() else {
            panic!("session json should be an object");
        };
        object.remove("archived_at");

        let restored: Session = serde_json::from_value(value).expect("deserialize session");

        assert!(restored.archived_at.is_none());
        assert!(!restored.is_archived());
    }

    #[test]
    fn apply_update_overwrites_project_and_state() {
        let mut memory = SessionMemory::default();
        let mut initial = memory_update();
        initial.project = Some("first".to_string());
        initial.current_state = Some("planning".to_string());
        memory.apply_update(initial).expect("initial update");

        let mut replacement = memory_update();
        replacement.project = Some("second".to_string());
        replacement.current_state = Some("coding".to_string());
        memory
            .apply_update(replacement)
            .expect("replacement update");

        assert_eq!(memory.project.as_deref(), Some("second"));
        assert_eq!(memory.current_state.as_deref(), Some("coding"));
    }

    #[test]
    fn apply_update_appends_decisions_and_context() {
        let mut memory = SessionMemory::default();
        let mut first = memory_update();
        first.key_decisions = Some(vec!["decide one".to_string()]);
        first.custom_context = Some(vec!["context one".to_string()]);
        memory.apply_update(first).expect("first update");

        let mut second = memory_update();
        second.key_decisions = Some(vec!["decide two".to_string()]);
        second.custom_context = Some(vec!["context two".to_string()]);
        memory.apply_update(second).expect("second update");

        assert_eq!(
            memory.key_decisions,
            vec!["decide one".to_string(), "decide two".to_string()]
        );
        assert_eq!(
            memory.custom_context,
            vec!["context one".to_string(), "context two".to_string()]
        );
    }

    #[test]
    fn apply_update_replaces_active_files() {
        let mut memory = SessionMemory::default();
        let mut first = memory_update();
        first.active_files = Some(vec!["a.rs".to_string(), "b.rs".to_string()]);
        memory.apply_update(first).expect("first files");

        let mut second = memory_update();
        second.active_files = Some(vec!["c.rs".to_string()]);
        memory.apply_update(second).expect("second files");

        assert_eq!(memory.active_files, vec!["c.rs".to_string()]);
    }

    #[test]
    fn apply_update_caps_lists_for_context_limit() {
        let mut memory = SessionMemory::with_context_limit(32_000);
        let mut update = memory_update();
        update.key_decisions = Some((0..25).map(|i| format!("decision-{i}")).collect());
        update.active_files = Some((0..22).map(|i| format!("file-{i}.rs")).collect());
        update.custom_context = Some((0..22).map(|i| format!("context-{i}")).collect());
        memory.apply_update(update).expect("capped update");

        assert_eq!(memory.key_decisions.len(), 20);
        assert_eq!(memory.active_files.len(), 20);
        assert_eq!(memory.custom_context.len(), 20);
        assert_eq!(
            memory.key_decisions.first().map(String::as_str),
            Some("decision-5")
        );
        assert_eq!(
            memory.active_files.first().map(String::as_str),
            Some("file-2.rs")
        );
        assert_eq!(
            memory.custom_context.first().map(String::as_str),
            Some("context-2")
        );
    }

    #[test]
    fn session_memory_estimated_tokens_is_nonzero_when_nonempty() {
        let memory = SessionMemory {
            project: Some("session memory".to_string()),
            ..SessionMemory::default()
        };

        assert!(memory.estimated_tokens() > 0);
    }

    #[test]
    fn session_memory_rejects_oversized_updates() {
        let mut memory = SessionMemory::default();
        let mut update = memory_update();
        update.project = Some("x".repeat(DEFAULT_SESSION_MEMORY_MAX_TOKENS * 8));

        let error = memory
            .apply_update(update)
            .expect_err("oversized memory should fail");

        assert!(error.contains("token cap"));
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
        assert!(session.memory.is_empty());
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
        session
            .add_message(MessageRole::User, "hello")
            .expect("add message");
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
            session
                .add_message(MessageRole::User, format!("msg-{i}"))
                .expect("add message");
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
        session
            .add_message(MessageRole::User, "only one")
            .expect("add message");
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
        session
            .add_message(MessageRole::User, "hi")
            .expect("add user");
        session
            .add_message(MessageRole::Assistant, "hello")
            .expect("add assistant");
        let info = session.info();
        assert_eq!(info.key, SessionKey::new("s5").unwrap());
        assert_eq!(info.kind, SessionKind::Channel);
        assert_eq!(info.title.as_deref(), Some("hi"));
        assert_eq!(info.preview.as_deref(), Some("hello"));
        assert!(!info.is_archived());
        assert_eq!(info.message_count, 2);
    }

    #[test]
    fn session_info_title_from_first_user_message() {
        let mut session = Session::new(
            SessionKey::new("s6").unwrap(),
            SessionKind::Main,
            test_config(),
        );
        session
            .add_message(MessageRole::Assistant, "system ready")
            .expect("add assistant");
        session
            .add_message(MessageRole::User, "first user title")
            .expect("add first user");
        session
            .add_message(MessageRole::User, "second user title")
            .expect("add second user");

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
        session
            .add_message(MessageRole::User, "hello")
            .expect("add user");
        session
            .add_message(MessageRole::Assistant, "latest preview")
            .expect("add assistant");

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
        session.archived_at = Some(321);
        session
            .add_message(MessageRole::System, "init")
            .expect("add message");
        let json = serde_json::to_string(&session).expect("serialize");
        let restored: Session = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored.key, session.key);
        assert_eq!(restored.kind, session.kind);
        assert_eq!(restored.archived_at, session.archived_at);
        assert!(restored.is_archived());
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
                    provider_id: Some("fc_1".to_string()),
                    name: "read_file".to_string(),
                    input: json!({"path": "README.md"}),
                },
                SessionContentBlock::Image {
                    media_type: "image/png".to_string(),
                    data: Some("abc123".to_string()),
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
                    provider_id: Some("fc_1".to_string()),
                    name: "search".to_string(),
                    input: json!({"q": "weather"}),
                },
                SessionContentBlock::ToolResult {
                    tool_use_id: "call_1".to_string(),
                    content: json!("sunny"),
                    is_error: None,
                },
                SessionContentBlock::Image {
                    media_type: "image/png".to_string(),
                    data: Some("abc123".to_string()),
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
                    provider_id: Some("fc_1".to_string()),
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
    fn session_message_to_llm_message_deduplicates_tool_use_blocks_preferring_provider_id() {
        let message = SessionMessage::structured(
            MessageRole::Assistant,
            vec![
                SessionContentBlock::ToolUse {
                    id: "call_1".to_string(),
                    provider_id: None,
                    name: "search".to_string(),
                    input: json!({"q": "weather"}),
                },
                SessionContentBlock::ToolUse {
                    id: "call_1".to_string(),
                    provider_id: Some("fc_1".to_string()),
                    name: "search".to_string(),
                    input: json!({"q": "weather"}),
                },
                SessionContentBlock::ToolResult {
                    tool_use_id: "call_1".to_string(),
                    content: json!("sunny"),
                    is_error: None,
                },
            ],
            123,
            Some(5),
        );

        let llm_message = message.to_llm_message();

        assert_eq!(
            llm_message.content,
            vec![
                ContentBlock::ToolUse {
                    id: "call_1".to_string(),
                    provider_id: Some("fc_1".to_string()),
                    name: "search".to_string(),
                    input: json!({"q": "weather"}),
                },
                ContentBlock::ToolResult {
                    tool_use_id: "call_1".to_string(),
                    content: json!("sunny"),
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

        session
            .extend_messages([
                SessionMessage::text(MessageRole::User, "first", 1),
                SessionMessage::text(MessageRole::Assistant, "second", 2),
            ])
            .expect("extend messages");

        assert_eq!(session.messages.len(), 2);
        assert_eq!(session.messages[0].render_text(), "first");
        assert_eq!(session.messages[1].render_text(), "second");
        assert!(session.updated_at > 0);
    }

    #[test]
    fn validate_tool_message_order_rejects_result_before_matching_tool_use() {
        let messages = vec![
            SessionMessage::structured(
                MessageRole::Tool,
                vec![SessionContentBlock::ToolResult {
                    tool_use_id: "call_1".to_string(),
                    content: json!("missing"),
                    is_error: Some(false),
                }],
                1,
                None,
            ),
            SessionMessage::structured(
                MessageRole::Assistant,
                vec![SessionContentBlock::ToolUse {
                    id: "call_1".to_string(),
                    provider_id: Some("fc_1".to_string()),
                    name: "read_file".to_string(),
                    input: json!({"path": "README.md"}),
                }],
                2,
                None,
            ),
        ];

        assert_eq!(
            validate_tool_message_order(&messages),
            Err(SessionHistoryError::ToolResultBeforeToolUse {
                tool_use_id: "call_1".to_string(),
                message_index: 0,
                block_index: 0,
            })
        );
    }

    #[test]
    fn extend_messages_rejects_tool_result_before_matching_tool_use() {
        let mut session = Session::new(
            SessionKey::new("invalid-tool-order").unwrap(),
            SessionKind::Main,
            test_config(),
        );

        let error = session
            .extend_messages([
                SessionMessage::structured(
                    MessageRole::Tool,
                    vec![SessionContentBlock::ToolResult {
                        tool_use_id: "call_1".to_string(),
                        content: json!("missing"),
                        is_error: Some(false),
                    }],
                    1,
                    None,
                ),
                SessionMessage::structured(
                    MessageRole::Assistant,
                    vec![SessionContentBlock::ToolUse {
                        id: "call_1".to_string(),
                        provider_id: Some("fc_1".to_string()),
                        name: "read_file".to_string(),
                        input: json!({"path": "README.md"}),
                    }],
                    2,
                    None,
                ),
            ])
            .expect_err("invalid tool ordering should fail");

        assert_eq!(
            error,
            SessionHistoryError::ToolResultBeforeToolUse {
                tool_use_id: "call_1".to_string(),
                message_index: 0,
                block_index: 0,
            }
        );
        assert!(session.messages.is_empty());
    }

    #[test]
    fn prune_unresolved_tool_history_drops_half_resolved_tool_use() {
        let messages = vec![
            SessionMessage::text(MessageRole::User, "update the readme", 1),
            SessionMessage::structured(
                MessageRole::Assistant,
                vec![
                    SessionContentBlock::ToolUse {
                        id: "call_resolved".to_string(),
                        provider_id: Some("fc_resolved".to_string()),
                        name: "read_file".to_string(),
                        input: json!({"path": "README.md"}),
                    },
                    SessionContentBlock::ToolUse {
                        id: "call_orphan".to_string(),
                        provider_id: Some("fc_orphan".to_string()),
                        name: "git_status".to_string(),
                        input: json!({}),
                    },
                ],
                2,
                None,
            ),
            SessionMessage::structured(
                MessageRole::Tool,
                vec![SessionContentBlock::ToolResult {
                    tool_use_id: "call_resolved".to_string(),
                    content: json!("updated"),
                    is_error: Some(false),
                }],
                3,
                None,
            ),
            SessionMessage::text(MessageRole::Assistant, "Updated README.md.", 4),
        ];

        let pruned = prune_unresolved_tool_history(&messages);

        assert_eq!(pruned.len(), 4);
        assert!(matches!(
            pruned[1].content.as_slice(),
            [SessionContentBlock::ToolUse { id, provider_id, .. }]
                if id == "call_resolved"
                    && provider_id.as_deref() == Some("fc_resolved")
        ));
        assert!(matches!(
            pruned[2].content.as_slice(),
            [SessionContentBlock::ToolResult { tool_use_id, .. }]
                if tool_use_id == "call_resolved"
        ));
        assert!(validate_tool_message_order(&pruned).is_ok());
        assert!(!pruned
            .iter()
            .flat_map(|message| &message.content)
            .any(|block| matches!(
                block,
                SessionContentBlock::ToolUse { id, .. } if id == "call_orphan"
            )));
    }

    #[test]
    fn prune_unresolved_tool_history_drops_orphaned_tool_result() {
        let messages = vec![
            SessionMessage::text(MessageRole::User, "what changed?", 1),
            SessionMessage::structured(
                MessageRole::Tool,
                vec![SessionContentBlock::ToolResult {
                    tool_use_id: "call_orphan".to_string(),
                    content: json!("stale"),
                    is_error: Some(false),
                }],
                2,
                None,
            ),
            SessionMessage::text(MessageRole::Assistant, "Nothing yet.", 3),
        ];

        let pruned = prune_unresolved_tool_history(&messages);

        assert_eq!(pruned.len(), 2);
        assert!(pruned
            .iter()
            .all(|message| message.role != MessageRole::Tool));
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
    fn session_message_deserializes_tool_result_without_is_error() {
        let json = r#"{"role":"tool","content":[{"type":"tool_result","tool_use_id":"call_1","content":"hello"}],"timestamp":123}"#;

        let restored: SessionMessage = serde_json::from_str(json).expect("deserialize");

        assert_eq!(
            restored.content,
            vec![SessionContentBlock::ToolResult {
                tool_use_id: "call_1".to_string(),
                content: json!("hello"),
                is_error: None,
            }]
        );
    }

    #[test]
    fn session_message_deserializes_legacy_image_without_data() {
        let json = r#"{"role":"user","content":[{"type":"image","media_type":"image/png"}],"timestamp":123}"#;

        let restored: SessionMessage = serde_json::from_str(json).expect("deserialize");

        assert_eq!(
            restored.content,
            vec![SessionContentBlock::Image {
                media_type: "image/png".to_string(),
                data: None,
            }]
        );
        assert_eq!(restored.render_text(), "[image:image/png]");
    }

    #[test]
    fn session_content_block_converts_to_and_from_llm_content() {
        let blocks = vec![
            ContentBlock::Text {
                text: "hello".to_string(),
            },
            ContentBlock::ToolUse {
                id: "call_1".to_string(),
                provider_id: Some("fc_1".to_string()),
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
