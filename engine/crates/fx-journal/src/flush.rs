//! Pre-compaction memory flush — persists evicted messages to journal.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fx_kernel::conversation_compactor::{CompactionFlushError, CompactionMemoryFlush};
use fx_llm::{ContentBlock, Message, MessageRole};

use crate::journal::Journal;

const MAX_CONTENT_BYTES: usize = 4_000;
const MAX_TOOL_RESULT_BYTES: usize = 500;

/// Flushes evicted conversation content to the journal before compaction.
#[derive(Debug)]
pub struct JournalCompactionFlush {
    journal: Arc<Mutex<Journal>>,
}

impl JournalCompactionFlush {
    /// Create a new flush backed by the given journal.
    pub fn new(journal: Arc<Mutex<Journal>>) -> Self {
        Self { journal }
    }
}

#[async_trait]
impl CompactionMemoryFlush for JournalCompactionFlush {
    async fn flush(
        &self,
        evicted: &[Message],
        scope_label: &str,
    ) -> Result<(), CompactionFlushError> {
        if evicted.is_empty() {
            return Ok(());
        }

        let content = format_evicted_messages(evicted);
        let truncated = truncate_content(&content);

        let context_text = format!(
            "Auto-flushed during {} compaction. {} messages evicted.",
            scope_label,
            evicted.len(),
        );

        let mut journal = self
            .journal
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        journal
            .write(
                truncated,
                vec!["compaction-flush".to_string(), "auto".to_string()],
                "session-memory".to_string(),
                Some(context_text),
            )
            .map_err(|err| CompactionFlushError::FlushFailed {
                reason: err.to_string(),
            })?;

        Ok(())
    }
}

fn truncate_content(content: &str) -> String {
    if content.len() <= MAX_CONTENT_BYTES {
        return content.to_string();
    }

    let truncate_at = truncate_at_char_boundary(content, MAX_CONTENT_BYTES);
    format!("{}...[truncated]", &content[..truncate_at])
}

fn truncate_at_char_boundary(content: &str, max_bytes: usize) -> usize {
    content
        .char_indices()
        .map(|(index, _)| index)
        .take_while(|&index| index <= max_bytes)
        .last()
        .unwrap_or(0)
}

fn format_evicted_messages(messages: &[Message]) -> String {
    messages
        .iter()
        .map(format_single_message)
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_single_message(msg: &Message) -> String {
    let role = match &msg.role {
        MessageRole::System => "system",
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::Tool => "tool",
    };
    let text = msg
        .content
        .iter()
        .filter_map(format_content_block)
        .collect::<Vec<_>>()
        .join(" ");
    format!("{role}: {text}")
}

fn format_content_block(block: &ContentBlock) -> Option<String> {
    match block {
        ContentBlock::Text { text } => Some(text.clone()),
        ContentBlock::ToolUse { name, input, .. } => Some(format!("[tool:{name}] {input}")),
        ContentBlock::ToolResult { content, .. } => {
            let s = content.to_string();
            if s.len() > MAX_TOOL_RESULT_BYTES {
                let truncate_at = truncate_at_char_boundary(&s, MAX_TOOL_RESULT_BYTES);
                Some(format!("{}...", &s[..truncate_at]))
            } else {
                Some(s)
            }
        }
        ContentBlock::Image { .. } => Some("[image]".to_string()),
        ContentBlock::Document { filename, .. } => Some(
            filename
                .as_ref()
                .map(|filename| format!("[document:{filename}]"))
                .unwrap_or_else(|| "[document]".to_string()),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_journal(tmp: &TempDir) -> Arc<Mutex<Journal>> {
        let path = tmp.path().join("journal.jsonl");
        Arc::new(Mutex::new(Journal::load(path).unwrap()))
    }

    #[tokio::test]
    async fn flush_writes_entry_with_correct_tags() {
        let tmp = TempDir::new().unwrap();
        let journal = test_journal(&tmp);
        let flush = JournalCompactionFlush::new(Arc::clone(&journal));

        let messages = vec![Message::user("hello world"), Message::assistant("hi there")];

        flush.flush(&messages, "perceive").await.unwrap();

        let j = journal.lock().unwrap();
        let entries = j.list(None);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].tags, vec!["compaction-flush", "auto"]);
        assert_eq!(entries[0].applies_to, "session-memory");
        assert!(entries[0].lesson.contains("user: hello world"));
        assert!(entries[0].lesson.contains("assistant: hi there"));
    }

    #[tokio::test]
    async fn flush_handles_empty_eviction() {
        let tmp = TempDir::new().unwrap();
        let journal = test_journal(&tmp);
        let flush = JournalCompactionFlush::new(Arc::clone(&journal));

        flush.flush(&[], "perceive").await.unwrap();

        let j = journal.lock().unwrap();
        assert_eq!(j.count(), 0);
    }

    #[tokio::test]
    async fn flush_truncates_large_content() {
        let tmp = TempDir::new().unwrap();
        let journal = test_journal(&tmp);
        let flush = JournalCompactionFlush::new(Arc::clone(&journal));

        let big = "x".repeat(5_000);
        let messages = vec![Message::user(big)];

        flush.flush(&messages, "perceive").await.unwrap();

        let j = journal.lock().unwrap();
        let entries = j.list(None);
        assert!(entries[0].lesson.len() <= MAX_CONTENT_BYTES + "...[truncated]".len());
        assert!(entries[0].lesson.ends_with("...[truncated]"));
    }

    #[tokio::test]
    async fn flush_handles_multibyte_truncation() {
        let tmp = TempDir::new().unwrap();
        let journal = test_journal(&tmp);
        let flush = JournalCompactionFlush::new(Arc::clone(&journal));

        let emoji_content = "🦝".repeat(2_000);
        let messages = vec![Message::user(emoji_content)];

        flush.flush(&messages, "perceive").await.unwrap();

        let j = journal.lock().unwrap();
        let entries = j.list(None);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].lesson.ends_with("...[truncated]"));
        let truncated_prefix = entries[0]
            .lesson
            .strip_suffix("...[truncated]")
            .expect("expected truncated suffix");
        assert!(entries[0].lesson.is_char_boundary(truncated_prefix.len()));
    }

    #[tokio::test]
    async fn flush_formats_roles_correctly() {
        let tmp = TempDir::new().unwrap();
        let journal = test_journal(&tmp);
        let flush = JournalCompactionFlush::new(Arc::clone(&journal));

        let messages = vec![
            Message::system("sys prompt"),
            Message::user("question"),
            Message::assistant("answer"),
        ];

        flush.flush(&messages, "perceive").await.unwrap();

        let j = journal.lock().unwrap();
        let lesson = &j.list(None)[0].lesson;
        assert!(lesson.contains("system: sys prompt"));
        assert!(lesson.contains("user: question"));
        assert!(lesson.contains("assistant: answer"));
    }

    #[tokio::test]
    async fn flush_truncates_large_tool_results() {
        let tmp = TempDir::new().unwrap();
        let journal = test_journal(&tmp);
        let flush = JournalCompactionFlush::new(Arc::clone(&journal));

        let big_result = "🦝".repeat(400);
        let messages = vec![Message {
            role: MessageRole::Tool,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "t1".to_string(),
                content: serde_json::json!(big_result),
            }],
        }];

        flush.flush(&messages, "perceive").await.unwrap();

        let j = journal.lock().unwrap();
        let lesson = &j.list(None)[0].lesson;
        assert!(lesson.len() < 1_000);
        assert!(lesson.contains("..."));
    }
}
