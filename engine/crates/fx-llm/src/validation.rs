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
        ContentBlock::Document { .. } => "document".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::validate_tool_message_sequence;
    use crate::types::{ContentBlock, LlmError, Message, MessageRole};

    fn user_message() -> Message {
        Message {
            role: MessageRole::User,
            content: vec![ContentBlock::Text {
                text: "hello".to_string(),
            }],
        }
    }

    fn assistant_tool_use(id: &str) -> Message {
        Message {
            role: MessageRole::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: id.to_string(),
                provider_id: None,
                name: "read".to_string(),
                input: serde_json::json!({"path": "/tmp/a"}),
            }],
        }
    }

    fn tool_message(tool_use_id: &str) -> Message {
        Message {
            role: MessageRole::Tool,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: tool_use_id.to_string(),
                content: serde_json::json!("ok"),
            }],
        }
    }

    #[test]
    fn validate_accepts_valid_tool_sequence() {
        let messages = vec![
            user_message(),
            assistant_tool_use("call-1"),
            tool_message("call-1"),
        ];

        assert!(validate_tool_message_sequence(&messages).is_ok());
    }

    #[test]
    fn validate_rejects_orphaned_tool_result() {
        let messages = vec![user_message(), tool_message("call-1")];

        let message = match validate_tool_message_sequence(&messages) {
            Err(LlmError::Request(message)) => message,
            Err(other) => panic!("expected request error, got {other:?}"),
            Ok(()) => panic!("expected orphaned tool result to fail"),
        };

        assert!(message.contains("invalid tool continuation"));
    }

    #[test]
    fn validate_accepts_empty_messages() {
        assert!(validate_tool_message_sequence(&[]).is_ok());
    }

    #[test]
    fn validate_ignores_empty_tool_use_ids() {
        let messages = vec![user_message(), assistant_tool_use(""), tool_message("")];

        assert!(validate_tool_message_sequence(&messages).is_ok());
    }
}
