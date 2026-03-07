//! Cross-turn conversation deduplication.
//!
//! Scans conversation history for duplicate content blocks and replaces
//! earlier occurrences with a reference marker `[see turn N]`. System
//! messages and recent turns are never deduplicated.

use fx_llm::{ContentBlock, Message, MessageRole};
use std::collections::HashMap;

/// Configuration for the deduplication pass.
#[derive(Debug, Clone)]
pub struct DeduplicationConfig {
    /// Whether dedup is enabled at all.
    pub enabled: bool,
    /// Minimum content length to consider for dedup.
    pub min_length: usize,
    /// Number of recent messages to always preserve intact.
    pub preserve_recent: usize,
}

impl Default for DeduplicationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            min_length: 100,
            preserve_recent: 2,
        }
    }
}

/// Report of deduplication results.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeduplicationReport {
    /// Number of duplicate content blocks replaced.
    pub duplicates_found: usize,
    /// Estimated bytes saved by dedup replacements.
    pub bytes_saved: usize,
}

/// Run the deduplication pass over a conversation.
///
/// Returns a report of what was deduplicated. Messages are modified in place.
pub fn dedup_conversation(
    messages: &mut [Message],
    config: &DeduplicationConfig,
) -> DeduplicationReport {
    if !config.enabled || messages.is_empty() {
        return DeduplicationReport::default();
    }

    let eligible_count = messages.len().saturating_sub(config.preserve_recent);
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut report = DeduplicationReport::default();

    record_first_occurrences(messages, eligible_count, config.min_length, &mut seen);
    replace_duplicates(messages, eligible_count, config, &seen, &mut report);

    report
}

/// Record first occurrence turn index for each content string.
fn record_first_occurrences(
    messages: &[Message],
    eligible_count: usize,
    min_length: usize,
    seen: &mut HashMap<String, usize>,
) {
    for (turn, message) in messages.iter().enumerate() {
        if message.role == MessageRole::System {
            continue;
        }
        for block in &message.content {
            if let Some(text) = extract_text(block) {
                if text.len() >= min_length && !seen.contains_key(text) {
                    seen.insert(text.to_owned(), turn);
                }
            }
        }
        if turn + 1 >= eligible_count {
            break;
        }
    }
}

/// Replace duplicate content blocks with reference markers.
fn replace_duplicates(
    messages: &mut [Message],
    eligible_count: usize,
    config: &DeduplicationConfig,
    seen: &HashMap<String, usize>,
    report: &mut DeduplicationReport,
) {
    for (turn, message) in messages.iter_mut().enumerate().take(eligible_count) {
        if message.role == MessageRole::System {
            continue;
        }
        replace_blocks_in_message(message, turn, config.min_length, seen, report);
    }
}

/// Replace duplicate blocks within a single message.
fn replace_blocks_in_message(
    message: &mut Message,
    turn: usize,
    min_length: usize,
    seen: &HashMap<String, usize>,
    report: &mut DeduplicationReport,
) {
    for block in &mut message.content {
        let text = match extract_text(block) {
            Some(t) if t.len() >= min_length => t.to_owned(),
            _ => continue,
        };
        let first_seen = match seen.get(&text) {
            Some(&idx) => idx,
            None => continue,
        };
        if first_seen == turn {
            continue; // This is the first occurrence — keep it
        }
        let original_len = text.len();
        let marker = format!("[see turn {first_seen}]");
        *block = ContentBlock::Text { text: marker };
        report.duplicates_found += 1;
        report.bytes_saved += original_len;
    }
}

/// Extract text content from a content block, if it's a text block.
fn extract_text(block: &ContentBlock) -> Option<&str> {
    match block {
        ContentBlock::Text { text } => Some(text.as_str()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_enabled() -> DeduplicationConfig {
        DeduplicationConfig {
            enabled: true,
            min_length: 100,
            preserve_recent: 2,
        }
    }

    fn long_text(prefix: &str) -> String {
        format!("{prefix}{}", "x".repeat(150))
    }

    #[test]
    fn dedup_removes_exact_duplicates() {
        let repeated = long_text("duplicate content ");
        let mut messages = vec![
            Message::user(repeated.clone()),
            Message::assistant("short reply"),
            Message::user(repeated.clone()),
            Message::assistant("another reply"),
            // last 2 are preserved
            Message::user("final question"),
            Message::assistant("final answer"),
        ];
        let report = dedup_conversation(&mut messages, &config_enabled());

        assert!(report.duplicates_found >= 1);
        // The second occurrence (index 2) should be replaced
        let block_text = extract_text(&messages[2].content[0]).unwrap();
        assert!(
            block_text.starts_with("[see turn"),
            "expected reference marker, got: {block_text}"
        );
        // First occurrence (index 0) should be kept intact
        let first_text = extract_text(&messages[0].content[0]).unwrap();
        assert_eq!(first_text, repeated);
    }

    #[test]
    fn dedup_preserves_system_messages() {
        let system_text = long_text("system instructions ");
        let mut messages = vec![
            Message::system(system_text.clone()),
            Message::user(system_text.clone()),
            Message::assistant("response"),
            // last 2 preserved
            Message::user("q"),
            Message::assistant("a"),
        ];
        let _report = dedup_conversation(&mut messages, &config_enabled());

        // System message must never be touched
        let sys_text = extract_text(&messages[0].content[0]).unwrap();
        assert_eq!(sys_text, system_text);
    }

    #[test]
    fn dedup_preserves_recent_turns() {
        let repeated = long_text("same content ");
        let mut messages = vec![
            Message::user(repeated.clone()),
            Message::assistant("first reply"),
            // These are the last 2 — must be preserved
            Message::user(repeated.clone()),
            Message::assistant(repeated.clone()),
        ];
        let report = dedup_conversation(&mut messages, &config_enabled());

        // Last 2 messages should be untouched
        let last_user = extract_text(&messages[2].content[0]).unwrap();
        assert_eq!(last_user, repeated);
        let last_asst = extract_text(&messages[3].content[0]).unwrap();
        assert_eq!(last_asst, repeated);
        assert_eq!(report.duplicates_found, 0);
    }

    #[test]
    fn dedup_skips_short_content() {
        let short = "short"; // < 100 chars
        let mut messages = vec![
            Message::user(short),
            Message::assistant(short),
            Message::user(short),
            Message::assistant(short),
            Message::user("q"),
            Message::assistant("a"),
        ];
        let report = dedup_conversation(&mut messages, &config_enabled());

        assert_eq!(report.duplicates_found, 0);
        for msg in &messages[..4] {
            let text = extract_text(&msg.content[0]).unwrap();
            assert_eq!(text, short);
        }
    }

    #[test]
    fn dedup_report_counts_savings() {
        let repeated = long_text("repeated block ");
        let original_len = repeated.len();
        let mut messages = vec![
            Message::user(repeated.clone()),
            Message::assistant(repeated.clone()),
            Message::user(repeated.clone()),
            // last 2 preserved
            Message::user("q"),
            Message::assistant("a"),
        ];
        let report = dedup_conversation(&mut messages, &config_enabled());

        assert_eq!(report.duplicates_found, 2);
        assert_eq!(report.bytes_saved, original_len * 2);
    }

    #[test]
    fn dedup_disabled_by_default() {
        let repeated = long_text("content ");
        let original = repeated.clone();
        let mut messages = vec![
            Message::user(repeated.clone()),
            Message::assistant(repeated.clone()),
            Message::user("q"),
            Message::assistant("a"),
        ];
        let config = DeduplicationConfig::default();
        assert!(!config.enabled);

        let report = dedup_conversation(&mut messages, &config);

        assert_eq!(report.duplicates_found, 0);
        let text = extract_text(&messages[0].content[0]).unwrap();
        assert_eq!(text, original);
    }
}
