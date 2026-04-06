//! LLM provider abstractions and routing utilities.
//!
//! This crate currently contains:
//! - Legacy prompt-first provider routing (`generate`/`generate_streaming`)
//! - New provider-client abstractions for structured completion APIs
//!   (Anthropic + OpenAI-compatible)

use async_trait::async_trait;
use fx_core::error::LlmError;
use std::collections::HashSet;
use std::sync::Arc;

mod anthropic;
mod config;
mod document;
mod fallback;
mod local;
pub mod model_catalog;
mod openai;
mod openai_common;
mod openai_responses;
mod provider;
mod router;
mod routing;
mod sse;
pub mod streaming;
pub use thinking::{default_thinking_level, thinking_config_for_model};

#[cfg(test)]
mod test_helpers;
pub mod thinking;
mod types;
mod validation;

pub use anthropic::AnthropicAuthMode;
pub use anthropic::AnthropicProvider;
pub use config::LocalModelConfig;
pub use fallback::{FallbackResult, FallbackRouter, ProviderHealth};
pub use local::LocalModel;
pub use model_catalog::{CatalogModel, ModelCatalog};
pub use openai::OpenAiProvider;
pub use openai_responses::OpenAiResponsesProvider;
pub use provider::{
    default_loop_response_classification, default_loop_truncation_resume_messages,
    null_loop_harness, resolve_loop_harness_from_profiles, CompletionStream,
    LlmProvider as CompletionProvider, LoopBufferedCompletionStrategy, LoopHarness, LoopModelMatch,
    LoopModelProfile, LoopPromptOverlayContext, LoopResponseClassification,
    LoopResponseTextClassification, LoopStreamingRecoveryStrategy, LoopTextDeltaMode,
    ProviderCapabilities, ProviderCatalogFilters, StaticLoopModelProfile,
};
pub use router::{
    fetch_available_models_from_catalog, LlmRouter, ModelInfo, ModelRouter, ProviderCatalogEntry,
    RouterError, RoutingStrategy,
};
pub use routing::{resolve_strategy, RoutingCondition, RoutingConfig, RoutingContext, RoutingRule};
pub use streaming::{completion_text, emit_default_stream_response, StreamCallback, StreamEvent};
pub use types::{
    CompletionRequest, CompletionResponse, ContentBlock, DocumentAttachment, ImageAttachment,
    LlmError as ProviderError, Message, MessageRole, StreamChunk, ThinkingConfig, ToolCall,
    ToolDefinition, ToolUseDelta, Usage, THINKING_BUDGET_ADAPTIVE, THINKING_BUDGET_HIGH,
    THINKING_BUDGET_LOW,
};

/// Trim a conversation history to the most recent `max_history` messages,
/// dropping the oldest messages first.
pub fn trim_conversation_history(history: &mut Vec<Message>, max_history: usize) {
    if history.len() <= max_history {
        return;
    }

    let keep_from = history.len().saturating_sub(max_history);
    let mut trimmed = history.split_off(keep_from);
    normalize_trimmed_tool_history(&mut trimmed);
    *history = trimmed;
}

fn normalize_trimmed_tool_history(history: &mut Vec<Message>) {
    let tool_result_ids = history
        .iter()
        .flat_map(|message| {
            message.content.iter().filter_map(|block| match block {
                ContentBlock::ToolResult { tool_use_id, .. } => Some(tool_use_id.clone()),
                _ => None,
            })
        })
        .collect::<HashSet<_>>();

    let mut seen_tool_use_ids = HashSet::new();

    for message in history.iter_mut() {
        match message.role {
            MessageRole::Assistant => {
                message.content.retain(|block| match block {
                    ContentBlock::ToolUse { id, .. } => {
                        let keep = tool_result_ids.contains(id);
                        if keep {
                            seen_tool_use_ids.insert(id.clone());
                        }
                        keep
                    }
                    ContentBlock::Text { .. }
                    | ContentBlock::Image { .. }
                    | ContentBlock::Document { .. } => true,
                    ContentBlock::ToolResult { .. } => false,
                });
            }
            MessageRole::Tool => {
                message.content.retain(|block| match block {
                    ContentBlock::ToolResult { tool_use_id, .. } => {
                        seen_tool_use_ids.contains(tool_use_id)
                    }
                    ContentBlock::Text { .. }
                    | ContentBlock::Image { .. }
                    | ContentBlock::Document { .. } => true,
                    ContentBlock::ToolUse { .. } => false,
                });
            }
            MessageRole::System | MessageRole::User => {}
        }
    }

    history.retain(|message| !message.content.is_empty());
}

/// Legacy prompt-generation provider trait.
///
/// This trait is used by the existing local/cloud router implementation.
/// The newer structured provider API is exposed as [`CompletionProvider`].
#[async_trait]
pub trait LlmProvider: Send + Sync + std::fmt::Debug {
    /// Generate a completion for the given prompt.
    async fn generate(&self, prompt: &str, max_tokens: u32) -> Result<String, LlmError>;

    /// Generate completion with streaming callback.
    async fn generate_streaming(
        &self,
        prompt: &str,
        max_tokens: u32,
        callback: Box<dyn Fn(String) + Send + 'static>,
    ) -> Result<String, LlmError>;

    /// Get the model name/identifier.
    fn model_name(&self) -> &str;
}

/// Type alias for boxed legacy prompt providers (dynamic dispatch).
pub type BoxedProvider = Box<dyn LlmProvider>;

/// Type alias for shared legacy prompt providers.
pub type SharedProvider = Arc<dyn LlmProvider>;

/// Normalize raw tool-call arguments: empty/whitespace-only strings become `"{}"`.
///
/// LLM providers sometimes return empty strings for zero-parameter tool calls.
/// All consumers expect valid JSON (`Value::Object({})`), so we normalize here.
pub(crate) fn normalize_tool_arguments(raw: &str) -> &str {
    if raw.trim().is_empty() {
        "{}"
    } else {
        raw
    }
}

const RAW_TOOL_ARGUMENTS_KEY: &str = "__fawx_raw_args";
const RAW_TOOL_ARGUMENTS_ERROR_KEY: &str = "__fawx_raw_args_error";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolArgumentsParseError {
    message: String,
}

impl ToolArgumentsParseError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ToolArgumentsParseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ToolArgumentsParseError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MalformedToolArguments<'a> {
    pub raw: &'a str,
    pub error: &'a str,
}

/// Parse tool call arguments into a JSON object, with a safe fallback.
///
/// If parsing fails, wraps the raw string as `{"__fawx_raw_args": "..."}` so
/// the value remains a valid JSON object (providers require this) and the
/// original string is preserved for debugging. The `__fawx_raw_args` key is
/// prefixed to avoid collisions with legitimate tool parameter names.
pub fn parse_tool_arguments_object(raw: &str) -> serde_json::Value {
    match try_parse_tool_arguments_object(raw) {
        Ok(arguments) => arguments,
        Err(error) => {
            tracing::warn!(
                error = %error,
                "tool arguments JSON parse failed after repair attempts; wrapping as malformed tool arguments"
            );
            serde_json::json!({
                RAW_TOOL_ARGUMENTS_KEY: raw,
                RAW_TOOL_ARGUMENTS_ERROR_KEY: error.to_string(),
            })
        }
    }
}

pub fn try_parse_tool_arguments_object(
    raw: &str,
) -> Result<serde_json::Value, ToolArgumentsParseError> {
    let normalized = normalize_tool_arguments(raw);
    parse_tool_arguments_json(normalized)
}

pub fn malformed_tool_arguments(
    arguments: &serde_json::Value,
) -> Option<MalformedToolArguments<'_>> {
    let raw = arguments
        .get(RAW_TOOL_ARGUMENTS_KEY)
        .and_then(serde_json::Value::as_str)?;
    let error = arguments
        .get(RAW_TOOL_ARGUMENTS_ERROR_KEY)
        .and_then(serde_json::Value::as_str)
        .unwrap_or("arguments could not be parsed as valid JSON");
    Some(MalformedToolArguments { raw, error })
}

fn parse_tool_arguments_json(raw: &str) -> Result<serde_json::Value, ToolArgumentsParseError> {
    match serde_json::from_str(raw) {
        Ok(arguments) => Ok(arguments),
        Err(original_error) => {
            let repaired = repair_tool_arguments_json(raw);
            if repaired == raw {
                return Err(ToolArgumentsParseError::new(original_error.to_string()));
            }

            match serde_json::from_str(&repaired) {
                Ok(arguments) => {
                    tracing::warn!(
                        error = %original_error,
                        "tool arguments JSON parse failed; repaired common string escaping issues"
                    );
                    Ok(arguments)
                }
                Err(repaired_error) => Err(ToolArgumentsParseError::new(format!(
                    "{original_error}; automatic repair failed: {repaired_error}"
                ))),
            }
        }
    }
}

fn repair_tool_arguments_json(raw: &str) -> String {
    let mut repaired = String::with_capacity(raw.len() + 16);
    let mut chars = raw.chars().peekable();
    let mut in_string = false;
    let mut in_escape = false;

    while let Some(ch) = chars.next() {
        if !in_string {
            match ch {
                '"' => {
                    in_string = true;
                    repaired.push(ch);
                }
                ',' if next_non_whitespace_is_closing(chars.clone()) => {}
                _ => repaired.push(ch),
            }
            continue;
        }

        if in_escape {
            repaired.push(ch);
            in_escape = false;
            continue;
        }

        match ch {
            '\\' => {
                if escaped_quote_should_remain_literal(chars.clone()) {
                    repaired.push('\\');
                    repaired.push('\\');
                } else if starts_valid_json_escape(chars.clone()) {
                    repaired.push(ch);
                    in_escape = true;
                } else {
                    repaired.push('\\');
                    repaired.push('\\');
                }
            }
            '"' => {
                if string_quote_closes(chars.clone()) {
                    in_string = false;
                    repaired.push(ch);
                } else {
                    repaired.push('\\');
                    repaired.push('"');
                }
            }
            '\n' => repaired.push_str("\\n"),
            '\r' => repaired.push_str("\\r"),
            '\t' => repaired.push_str("\\t"),
            other if other.is_control() => {
                use std::fmt::Write as _;
                let _ = write!(repaired, "\\u{:04x}", other as u32);
            }
            other => repaired.push(other),
        }
    }

    repaired
}

fn escaped_quote_should_remain_literal(
    mut chars: std::iter::Peekable<std::str::Chars<'_>>,
) -> bool {
    matches!(chars.next(), Some('"')) && !string_quote_closes(chars)
}

fn starts_valid_json_escape(mut chars: std::iter::Peekable<std::str::Chars<'_>>) -> bool {
    match chars.next() {
        Some('"') | Some('\\') | Some('/') | Some('b') | Some('f') | Some('n') | Some('r')
        | Some('t') => true,
        Some('u') => {
            let hex_digits = chars.by_ref().take(4).collect::<Vec<_>>();
            hex_digits.len() == 4 && hex_digits.iter().all(|digit| digit.is_ascii_hexdigit())
        }
        _ => false,
    }
}

fn string_quote_closes(mut chars: std::iter::Peekable<std::str::Chars<'_>>) -> bool {
    while let Some(ch) = chars.next() {
        if ch.is_whitespace() {
            continue;
        }
        return matches!(ch, ',' | '}' | ']' | ':');
    }
    true
}

fn next_non_whitespace_is_closing(chars: std::iter::Peekable<std::str::Chars<'_>>) -> bool {
    chars
        .skip_while(|ch| ch.is_whitespace())
        .next()
        .is_some_and(|ch| matches!(ch, '}' | ']'))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mock legacy provider for testing.
    #[derive(Debug)]
    struct MockProvider {
        name: String,
        response: String,
    }

    #[async_trait]
    impl LlmProvider for MockProvider {
        async fn generate(&self, _prompt: &str, _max_tokens: u32) -> Result<String, LlmError> {
            Ok(self.response.clone())
        }

        async fn generate_streaming(
            &self,
            _prompt: &str,
            _max_tokens: u32,
            callback: Box<dyn Fn(String) + Send + 'static>,
        ) -> Result<String, LlmError> {
            for word in self.response.split_whitespace() {
                callback(word.to_string());
            }
            Ok(self.response.clone())
        }

        fn model_name(&self) -> &str {
            &self.name
        }
    }

    #[tokio::test]
    async fn test_mock_provider_generate() {
        let provider = MockProvider {
            name: "test-model".to_string(),
            response: "Hello world".to_string(),
        };

        let result = provider.generate("test", 10).await.unwrap();
        assert_eq!(result, "Hello world");
        assert_eq!(provider.model_name(), "test-model");
    }

    #[tokio::test]
    async fn test_mock_provider_streaming() {
        let provider = MockProvider {
            name: "test-model".to_string(),
            response: "Hello world".to_string(),
        };

        let callback = Box::new(|_chunk: String| {
            // noop for test
        });

        let result = provider
            .generate_streaming("test", 10, callback)
            .await
            .unwrap();
        assert_eq!(result, "Hello world");
    }

    #[test]
    fn repair_tool_arguments_json_preserves_code_escapes() {
        let raw = "{\n  \"path\": \"main.rs\",\n  \"content\": \"let pattern = r\"\\d+\";\nlet msg = \"she said \\\"hello\\\"\";\n\"\n}";
        let repaired = repair_tool_arguments_json(raw);
        assert!(
            repaired.contains("\\\\\\\"hello\\\\\\\""),
            "repaired JSON should preserve literal escaped quotes: {repaired}"
        );

        let parsed: serde_json::Value =
            serde_json::from_str(&repaired).expect("repaired JSON should parse");
        assert_eq!(
            parsed["content"],
            "let pattern = r\"\\d+\";\nlet msg = \"she said \\\"hello\\\"\";\n"
        );
    }

    #[test]
    fn trim_conversation_history_drops_oldest() {
        let mut history = vec![
            Message::user("first"),
            Message::assistant("second"),
            Message::user("third"),
            Message::assistant("fourth"),
            Message::user("fifth"),
        ];
        trim_conversation_history(&mut history, 3);
        assert_eq!(history.len(), 3);
        assert_eq!(history[0], Message::user("third"));
        assert_eq!(history[2], Message::user("fifth"));
    }

    #[test]
    fn trim_conversation_history_noop_when_under_limit() {
        let mut history = vec![Message::user("only")];
        trim_conversation_history(&mut history, 10);
        assert_eq!(history.len(), 1);
    }

    #[test]
    fn trim_conversation_history_noop_when_at_limit() {
        let mut history = vec![Message::user("a"), Message::user("b")];
        trim_conversation_history(&mut history, 2);
        assert_eq!(history.len(), 2);
    }

    #[test]
    fn trim_conversation_history_drops_orphaned_tool_result_at_window_start() {
        let mut history = vec![
            Message::user("older prompt"),
            Message {
                role: MessageRole::Assistant,
                content: vec![ContentBlock::ToolUse {
                    id: "call_1".to_string(),
                    provider_id: Some("fc_1".to_string()),
                    name: "lookup".to_string(),
                    input: serde_json::json!({"q": "weather"}),
                }],
            },
            Message {
                role: MessageRole::Tool,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "call_1".to_string(),
                    content: serde_json::json!("first result"),
                }],
            },
            Message::assistant("summary"),
            Message::user("latest prompt"),
        ];

        trim_conversation_history(&mut history, 3);

        assert_eq!(history.len(), 2);
        assert_eq!(history[0], Message::assistant("summary"));
        assert_eq!(history[1], Message::user("latest prompt"));
    }

    #[test]
    fn trim_conversation_history_keeps_complete_tool_round_when_it_fits() {
        let mut history = vec![
            Message::user("older prompt"),
            Message {
                role: MessageRole::Assistant,
                content: vec![ContentBlock::ToolUse {
                    id: "call_1".to_string(),
                    provider_id: Some("fc_1".to_string()),
                    name: "lookup".to_string(),
                    input: serde_json::json!({"q": "weather"}),
                }],
            },
            Message {
                role: MessageRole::Tool,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "call_1".to_string(),
                    content: serde_json::json!("first result"),
                }],
            },
        ];

        trim_conversation_history(&mut history, 2);

        assert_eq!(history.len(), 2);
        assert!(matches!(
            &history[0].content[0],
            ContentBlock::ToolUse { id, .. } if id == "call_1"
        ));
        assert!(matches!(
            &history[1].content[0],
            ContentBlock::ToolResult { tool_use_id, .. } if tool_use_id == "call_1"
        ));
    }
}
