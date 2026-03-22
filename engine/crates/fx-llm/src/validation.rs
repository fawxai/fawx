use crate::types::{ContentBlock, LlmError, Message, MessageRole};
use std::collections::HashSet;

pub(crate) fn validate_tool_message_sequence(messages: &[Message]) -> Result<(), LlmError> {
    let mut seen_tool_calls = HashSet::new();

    for (message_index, message) in messages.iter().enumerate() {
        match message.role {
            MessageRole::Assistant => record_tool_uses(&message.content, &mut seen_tool_calls),
            MessageRole::Tool => {
                ensure_tool_results_have_matching_uses(
                    messages,
                    message_index,
                    &message.content,
                    &seen_tool_calls,
                )?;
            }
            MessageRole::System | MessageRole::User => {}
        }
    }

    Ok(())
}

fn record_tool_uses<'a>(blocks: &'a [ContentBlock], seen_tool_calls: &mut HashSet<&'a str>) {
    for block in blocks {
        if let ContentBlock::ToolUse { id, .. } = block {
            let trimmed = id.trim();
            if !trimmed.is_empty() {
                seen_tool_calls.insert(trimmed);
            }
        }
    }
}

fn ensure_tool_results_have_matching_uses(
    messages: &[Message],
    message_index: usize,
    blocks: &[ContentBlock],
    seen_tool_calls: &HashSet<&str>,
) -> Result<(), LlmError> {
    for block in blocks {
        if let ContentBlock::ToolResult { tool_use_id, .. } = block {
            let trimmed = tool_use_id.trim();
            if trimmed.is_empty() {
                continue;
            }
            if !seen_tool_calls.contains(trimmed) {
                return Err(LlmError::Request(format!(
                    "invalid tool continuation messages: tool result '{}' at message {} has no matching earlier assistant tool_use; tail={}",
                    trimmed,
                    message_index,
                    summarize_message_tail(messages),
                )));
            }
        }
    }

    Ok(())
}

fn summarize_message_tail(messages: &[Message]) -> String {
    let start = messages.len().saturating_sub(6);
    messages[start..]
        .iter()
        .enumerate()
        .map(|(offset, message)| summarize_message(start + offset, message))
        .collect::<Vec<_>>()
        .join(" | ")
}

fn summarize_message(index: usize, message: &Message) -> String {
    let blocks = message
        .content
        .iter()
        .map(summarize_content_block)
        .collect::<Vec<_>>()
        .join(",");
    format!("{index}:{:?}[{blocks}]", message.role)
}

fn summarize_content_block(block: &ContentBlock) -> String {
    match block {
        ContentBlock::Text { text } => {
            format!("text:{}", text.chars().take(24).collect::<String>())
        }
        ContentBlock::ToolUse {
            id,
            provider_id,
            name,
            ..
        } => format!(
            "tool_use:{name}:{id}:{}",
            provider_id.as_deref().unwrap_or("-")
        ),
        ContentBlock::ToolResult { tool_use_id, .. } => format!("tool_result:{tool_use_id}"),
        ContentBlock::Image { .. } => "image".to_string(),
    }
}
