use crate::signals::ControlPlaneDecisionKind;
use fx_llm::{CompletionResponse, ContentBlock, ToolCall, ToolDefinition};
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::HashMap;

pub(super) const MALFORMED_TOOL_CALL_FAILURE_TEXT: &str =
    "The model returned malformed tool-call markup that could not be safely normalized, so I stopped instead of guessing.";

const TOOL_CALL_OPEN_TAG: &str = "<tool_call>";
const TOOL_CALL_CLOSE_TAG: &str = "</tool_call>";
const ARG_KEY_OPEN_TAG: &str = "<arg_key>";
const ARG_KEY_CLOSE_TAG: &str = "</arg_key>";
const ARG_VALUE_OPEN_TAG: &str = "<arg_value>";
const ARG_VALUE_CLOSE_TAG: &str = "</arg_value>";
const TOOL_USE_STOP_REASON: &str = "tool_use";
const TOOL_CALL_MARKUP_TOKENS: [&str; 5] = [
    TOOL_CALL_OPEN_TAG,
    TOOL_CALL_CLOSE_TAG,
    ARG_KEY_OPEN_TAG,
    ARG_KEY_CLOSE_TAG,
    ARG_VALUE_OPEN_TAG,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArgumentSchemaKind {
    String,
    Boolean,
    Integer,
    Number,
    Array,
    Object,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct CompletionResponseNormalization {
    pub(super) response: CompletionResponse,
    pub(super) outcome: ToolCallNormalizationOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ToolCallNormalizationOutcome {
    None,
    Normalized {
        source: ToolCallNormalizationSource,
        tool_names: Vec<String>,
    },
    Rejected {
        source: ToolCallNormalizationSource,
        reason: ToolCallNormalizationRejectReason,
    },
}

impl ToolCallNormalizationOutcome {
    pub(super) const fn suppresses_buffered_text(&self) -> bool {
        !matches!(self, Self::None)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ToolCallNormalizationSource {
    RawMarkupText,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ToolCallNormalizationRejectReason {
    AmbiguousText,
    MalformedShape,
    UnknownTool,
    DuplicateArgumentKey,
    EmptyToolName,
    EmptyArgumentKey,
    InvalidArgumentValue,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(super) struct ToolCallNormalizationSignalMetadata {
    pub(super) decision_kind: ControlPlaneDecisionKind,
    pub(super) decision: &'static str,
    /// Legacy field retained for existing signal consumers; new diagnostic
    /// surfaces should prefer `decision_kind` and `decision`.
    pub(super) outcome: &'static str,
    pub(super) source: ToolCallNormalizationSource,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) tool_names: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) reason: Option<ToolCallNormalizationRejectReason>,
}

pub(super) fn normalize_completion_response(
    response: CompletionResponse,
    allowed_tools: &[ToolDefinition],
) -> CompletionResponseNormalization {
    if !response.tool_calls.is_empty() {
        return CompletionResponseNormalization {
            response,
            outcome: ToolCallNormalizationOutcome::None,
        };
    }

    let raw_text = response_text(&response.content);
    if !detect_tool_call_markup(&raw_text) {
        return CompletionResponseNormalization {
            response,
            outcome: ToolCallNormalizationOutcome::None,
        };
    }

    let source = ToolCallNormalizationSource::RawMarkupText;
    let outcome = match parse_tool_calls_from_markup(&raw_text, allowed_tools) {
        Ok(tool_calls) => {
            let tool_names = tool_calls.iter().map(|call| call.name.clone()).collect();
            let response = normalized_tool_call_response(response, tool_calls);
            return CompletionResponseNormalization {
                response,
                outcome: ToolCallNormalizationOutcome::Normalized { source, tool_names },
            };
        }
        Err(reason) => ToolCallNormalizationOutcome::Rejected { source, reason },
    };

    let response = rejected_markup_response(response);
    CompletionResponseNormalization { response, outcome }
}

fn normalized_tool_call_response(
    response: CompletionResponse,
    tool_calls: Vec<ToolCall>,
) -> CompletionResponse {
    let content = tool_calls
        .iter()
        .map(|call| ContentBlock::ToolUse {
            id: call.id.clone(),
            provider_id: None,
            name: call.name.clone(),
            input: call.arguments.clone(),
        })
        .collect();

    CompletionResponse {
        content,
        tool_calls,
        usage: response.usage,
        stop_reason: Some(TOOL_USE_STOP_REASON.to_string()),
    }
}

fn rejected_markup_response(response: CompletionResponse) -> CompletionResponse {
    CompletionResponse {
        content: vec![ContentBlock::Text {
            text: MALFORMED_TOOL_CALL_FAILURE_TEXT.to_string(),
        }],
        tool_calls: Vec::new(),
        usage: response.usage,
        stop_reason: Some("stop".to_string()),
    }
}

fn response_text(content: &[ContentBlock]) -> String {
    content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            ContentBlock::ToolUse { .. }
            | ContentBlock::ToolResult { .. }
            | ContentBlock::Image { .. }
            | ContentBlock::Document { .. } => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// Detection is only a cheap guard before the strict parser runs. The standalone
// value close tag is intentionally omitted because it is not enough signal by
// itself; the opening value tag already detects the known provider glitch.
fn detect_tool_call_markup(raw_text: &str) -> bool {
    let trimmed = raw_text.trim();
    TOOL_CALL_MARKUP_TOKENS
        .iter()
        .any(|token| trimmed.contains(token))
}

fn parse_tool_calls_from_markup(
    raw_text: &str,
    allowed_tools: &[ToolDefinition],
) -> Result<Vec<ToolCall>, ToolCallNormalizationRejectReason> {
    let trimmed = raw_text.trim();
    if !trimmed.starts_with(TOOL_CALL_OPEN_TAG) {
        return Err(ToolCallNormalizationRejectReason::AmbiguousText);
    }

    let allowed = allowed_tools
        .iter()
        .map(|tool| (tool.name.as_str(), tool))
        .collect::<HashMap<_, _>>();

    let mut calls = Vec::new();
    let mut remainder = trimmed;
    let mut index = 0usize;

    while !remainder.trim_start().is_empty() {
        remainder = remainder.trim_start();
        let Some(after_open) = remainder.strip_prefix(TOOL_CALL_OPEN_TAG) else {
            return Err(ToolCallNormalizationRejectReason::AmbiguousText);
        };

        let Some((tool_name_segment, mut call_body)) = after_open.split_once(ARG_KEY_OPEN_TAG)
        else {
            return Err(ToolCallNormalizationRejectReason::MalformedShape);
        };

        let tool_name = tool_name_segment.trim();
        if tool_name.is_empty() {
            return Err(ToolCallNormalizationRejectReason::EmptyToolName);
        }
        if tool_name.contains('<') || tool_name.contains('>') {
            return Err(ToolCallNormalizationRejectReason::MalformedShape);
        }
        let Some(tool_definition) = allowed.get(tool_name).copied() else {
            return Err(ToolCallNormalizationRejectReason::UnknownTool);
        };

        let mut arguments = Map::new();
        loop {
            let Some((arg_key_segment, after_key_close)) = call_body.split_once(ARG_KEY_CLOSE_TAG)
            else {
                return Err(ToolCallNormalizationRejectReason::MalformedShape);
            };
            let arg_key = arg_key_segment.trim();
            if arg_key.is_empty() {
                return Err(ToolCallNormalizationRejectReason::EmptyArgumentKey);
            }
            if arg_key.contains('<') || arg_key.contains('>') {
                return Err(ToolCallNormalizationRejectReason::MalformedShape);
            }
            if arguments.contains_key(arg_key) {
                return Err(ToolCallNormalizationRejectReason::DuplicateArgumentKey);
            }

            let Some(after_value_open) = after_key_close.strip_prefix(ARG_VALUE_OPEN_TAG) else {
                return Err(ToolCallNormalizationRejectReason::MalformedShape);
            };
            let Some((arg_value_segment, after_value_close)) =
                after_value_open.split_once(ARG_VALUE_CLOSE_TAG)
            else {
                return Err(ToolCallNormalizationRejectReason::MalformedShape);
            };

            // The malformed markup format has no escaping, so a literal closing
            // delimiter inside a value is ambiguous and rejected by the next
            // structural check instead of being guessed around.
            let arg_value =
                normalized_argument_value(&tool_definition.parameters, arg_key, arg_value_segment)?;
            arguments.insert(arg_key.to_string(), arg_value);
            call_body = after_value_close.trim_start();

            if let Some(after_close) = call_body.strip_prefix(TOOL_CALL_CLOSE_TAG) {
                let stable_id = stable_normalized_tool_call_id(
                    index,
                    tool_name,
                    &Value::Object(arguments.clone()),
                );
                calls.push(ToolCall {
                    id: stable_id,
                    name: tool_name.to_string(),
                    arguments: Value::Object(arguments),
                });
                remainder = after_close;
                index = index.saturating_add(1);
                break;
            }

            let Some(after_next_key) = call_body.strip_prefix(ARG_KEY_OPEN_TAG) else {
                return Err(ToolCallNormalizationRejectReason::MalformedShape);
            };
            call_body = after_next_key;
        }
    }

    if calls.is_empty() {
        return Err(ToolCallNormalizationRejectReason::MalformedShape);
    }

    Ok(calls)
}

fn normalized_argument_value(
    parameters_schema: &Value,
    arg_key: &str,
    raw_value: &str,
) -> Result<Value, ToolCallNormalizationRejectReason> {
    let kinds = argument_schema_kinds(parameters_schema, arg_key);
    if kinds.is_empty() || kinds.contains(&ArgumentSchemaKind::String) {
        // Unlike tool names and argument keys, values may intentionally contain
        // leading/trailing whitespace or JSON-looking text such as shell
        // commands `true` and `42`; preserve string-schema values exactly.
        return Ok(Value::String(raw_value.to_string()));
    }

    let parsed = serde_json::from_str::<Value>(raw_value.trim())
        .map_err(|_| ToolCallNormalizationRejectReason::InvalidArgumentValue)?;
    if kinds
        .iter()
        .any(|kind| value_matches_schema_kind(&parsed, *kind))
    {
        return Ok(parsed);
    }

    Err(ToolCallNormalizationRejectReason::InvalidArgumentValue)
}

fn argument_schema_kinds(parameters_schema: &Value, arg_key: &str) -> Vec<ArgumentSchemaKind> {
    parameters_schema
        .get("properties")
        .and_then(Value::as_object)
        .and_then(|properties| properties.get(arg_key))
        .and_then(|schema| schema.get("type"))
        .map(schema_type_kinds)
        .unwrap_or_default()
}

fn schema_type_kinds(schema_type: &Value) -> Vec<ArgumentSchemaKind> {
    match schema_type {
        Value::String(kind) => schema_type_kind(kind).into_iter().collect(),
        Value::Array(kinds) => kinds
            .iter()
            .filter_map(Value::as_str)
            .filter_map(schema_type_kind)
            .collect(),
        _ => Vec::new(),
    }
}

fn schema_type_kind(kind: &str) -> Option<ArgumentSchemaKind> {
    match kind {
        "string" => Some(ArgumentSchemaKind::String),
        "boolean" => Some(ArgumentSchemaKind::Boolean),
        "integer" => Some(ArgumentSchemaKind::Integer),
        "number" => Some(ArgumentSchemaKind::Number),
        "array" => Some(ArgumentSchemaKind::Array),
        "object" => Some(ArgumentSchemaKind::Object),
        _ => None,
    }
}

fn value_matches_schema_kind(value: &Value, kind: ArgumentSchemaKind) -> bool {
    match kind {
        ArgumentSchemaKind::String => value.is_string(),
        ArgumentSchemaKind::Boolean => value.is_boolean(),
        ArgumentSchemaKind::Integer => value.as_i64().is_some() || value.as_u64().is_some(),
        ArgumentSchemaKind::Number => value.is_number(),
        ArgumentSchemaKind::Array => value.is_array(),
        ArgumentSchemaKind::Object => value.is_object(),
    }
}

fn stable_normalized_tool_call_id(index: usize, tool_name: &str, arguments: &Value) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in format!("{tool_name}:{arguments}").bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("normalized-{index}-{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_response(text: &str) -> CompletionResponse {
        CompletionResponse {
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: None,
        }
    }

    fn run_command_allowed() -> Vec<ToolDefinition> {
        vec![ToolDefinition {
            name: "run_command".to_string(),
            description: "Execute a shell command".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" }
                },
                "required": ["command"]
            }),
        }]
    }

    fn typed_tool_allowed() -> Vec<ToolDefinition> {
        vec![ToolDefinition {
            name: "typed_tool".to_string(),
            description: "Accept typed values".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "enabled": { "type": "boolean" },
                    "count": { "type": "integer" },
                    "threshold": { "type": "number" },
                    "items": { "type": "array" },
                    "payload": { "type": "object" }
                },
                "required": ["enabled", "count", "threshold", "items", "payload"]
            }),
        }]
    }

    #[test]
    fn normalize_completion_response_repairs_known_raw_markup_shape() {
        let response = text_response(
            "<tool_call>run_command<arg_key>command</arg_key><arg_value>git status</arg_value></tool_call>",
        );

        let normalized = normalize_completion_response(response, &run_command_allowed());

        assert!(matches!(
            normalized.outcome,
            ToolCallNormalizationOutcome::Normalized { .. }
        ));
        assert_eq!(normalized.response.tool_calls.len(), 1);
        assert_eq!(normalized.response.tool_calls[0].name, "run_command");
        assert_eq!(
            normalized.response.tool_calls[0].arguments,
            serde_json::json!({"command": "git status"})
        );
        assert_eq!(normalized.response.stop_reason.as_deref(), Some("tool_use"));
        assert!(matches!(
            normalized.response.content.as_slice(),
            [ContentBlock::ToolUse { name, .. }] if name == "run_command"
        ));
    }

    #[test]
    fn normalize_completion_response_repairs_multiple_calls_and_preserves_values() {
        let response = text_response(
            "<tool_call>run_command<arg_key>command</arg_key><arg_value>printf '<ok>'\nnext</arg_value></tool_call>\n\
             <tool_call>run_command<arg_key>command</arg_key><arg_value>git diff --check</arg_value></tool_call>",
        );

        let normalized = normalize_completion_response(response, &run_command_allowed());

        assert!(matches!(
            normalized.outcome,
            ToolCallNormalizationOutcome::Normalized { .. }
        ));
        assert_eq!(normalized.response.tool_calls.len(), 2);
        assert_eq!(
            normalized.response.tool_calls[0].arguments,
            serde_json::json!({"command": "printf '<ok>'\nnext"})
        );
        assert_eq!(
            normalized.response.tool_calls[1].arguments,
            serde_json::json!({"command": "git diff --check"})
        );
        assert!(matches!(
            normalized.response.content.as_slice(),
            [
                ContentBlock::ToolUse { name: first, .. },
                ContentBlock::ToolUse { name: second, .. }
            ] if first == "run_command" && second == "run_command"
        ));
    }

    #[test]
    fn normalize_completion_response_parses_typed_argument_values_from_schema() {
        let response = text_response(
            "<tool_call>typed_tool\
             <arg_key>enabled</arg_key><arg_value>true</arg_value>\
             <arg_key>count</arg_key><arg_value>42</arg_value>\
             <arg_key>threshold</arg_key><arg_value>3.5</arg_value>\
             <arg_key>items</arg_key><arg_value>[\"a\",\"b\"]</arg_value>\
             <arg_key>payload</arg_key><arg_value>{\"mode\":\"safe\"}</arg_value>\
             </tool_call>",
        );

        let normalized = normalize_completion_response(response, &typed_tool_allowed());

        assert!(matches!(
            normalized.outcome,
            ToolCallNormalizationOutcome::Normalized { .. }
        ));
        assert_eq!(
            normalized.response.tool_calls[0].arguments,
            serde_json::json!({
                "enabled": true,
                "count": 42,
                "threshold": 3.5,
                "items": ["a", "b"],
                "payload": { "mode": "safe" }
            })
        );
    }

    #[test]
    fn normalize_completion_response_preserves_json_like_string_arguments() {
        let response = text_response(
            "<tool_call>run_command<arg_key>command</arg_key><arg_value>true</arg_value></tool_call>",
        );

        let normalized = normalize_completion_response(response, &run_command_allowed());

        assert!(matches!(
            normalized.outcome,
            ToolCallNormalizationOutcome::Normalized { .. }
        ));
        assert_eq!(
            normalized.response.tool_calls[0].arguments,
            serde_json::json!({"command": "true"})
        );
    }

    #[test]
    fn normalize_completion_response_rejects_mixed_text_markup() {
        let response = text_response(
            "I should inspect git first.\n<tool_call>run_command<arg_key>command</arg_key><arg_value>git status</arg_value></tool_call>",
        );

        let normalized = normalize_completion_response(response, &run_command_allowed());

        assert!(matches!(
            normalized.outcome,
            ToolCallNormalizationOutcome::Rejected {
                reason: ToolCallNormalizationRejectReason::AmbiguousText,
                ..
            }
        ));
        assert_eq!(normalized.response.tool_calls.len(), 0);
        assert_eq!(
            normalized.response.content,
            vec![ContentBlock::Text {
                text: MALFORMED_TOOL_CALL_FAILURE_TEXT.to_string()
            }]
        );
        assert_eq!(normalized.response.stop_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn normalize_completion_response_rejects_unknown_tools() {
        let response = text_response(
            "<tool_call>delete_everything<arg_key>path</arg_key><arg_value>/tmp</arg_value></tool_call>",
        );

        let normalized = normalize_completion_response(response, &run_command_allowed());

        assert!(matches!(
            normalized.outcome,
            ToolCallNormalizationOutcome::Rejected {
                reason: ToolCallNormalizationRejectReason::UnknownTool,
                ..
            }
        ));
    }

    #[test]
    fn normalize_completion_response_leaves_normal_text_alone() {
        let response = text_response("All set.");

        let normalized = normalize_completion_response(response.clone(), &run_command_allowed());

        assert_eq!(
            normalized,
            CompletionResponseNormalization {
                response,
                outcome: ToolCallNormalizationOutcome::None
            }
        );
    }

    #[test]
    fn normalize_completion_response_leaves_structured_tool_calls_unchanged() {
        let response = CompletionResponse {
            content: vec![ContentBlock::ToolUse {
                id: "call-1".to_string(),
                provider_id: None,
                name: "run_command".to_string(),
                input: serde_json::json!({"command": "git status"}),
            }],
            tool_calls: vec![ToolCall {
                id: "call-1".to_string(),
                name: "run_command".to_string(),
                arguments: serde_json::json!({"command": "git status"}),
            }],
            usage: None,
            stop_reason: Some("tool_use".to_string()),
        };

        let normalized = normalize_completion_response(response.clone(), &run_command_allowed());

        assert_eq!(
            normalized,
            CompletionResponseNormalization {
                response,
                outcome: ToolCallNormalizationOutcome::None
            }
        );
    }

    #[test]
    fn normalize_completion_response_rejects_duplicate_argument_keys() {
        let response = text_response(
            "<tool_call>run_command<arg_key>command</arg_key><arg_value>git status</arg_value><arg_key>command</arg_key><arg_value>git diff</arg_value></tool_call>",
        );

        let normalized = normalize_completion_response(response, &run_command_allowed());

        assert!(matches!(
            normalized.outcome,
            ToolCallNormalizationOutcome::Rejected {
                reason: ToolCallNormalizationRejectReason::DuplicateArgumentKey,
                ..
            }
        ));
    }

    #[test]
    fn normalize_completion_response_rejects_empty_tool_call_without_arguments() {
        let response = text_response("<tool_call>run_command</tool_call>");

        let normalized = normalize_completion_response(response, &run_command_allowed());

        assert!(matches!(
            normalized.outcome,
            ToolCallNormalizationOutcome::Rejected {
                reason: ToolCallNormalizationRejectReason::MalformedShape,
                ..
            }
        ));
    }

    #[test]
    fn normalize_completion_response_rejects_unescaped_value_close_delimiter() {
        let response = text_response(
            "<tool_call>run_command<arg_key>command</arg_key><arg_value>printf '</arg_value>'</arg_value></tool_call>",
        );

        let normalized = normalize_completion_response(response, &run_command_allowed());

        assert!(matches!(
            normalized.outcome,
            ToolCallNormalizationOutcome::Rejected {
                reason: ToolCallNormalizationRejectReason::MalformedShape,
                ..
            }
        ));
    }

    #[test]
    fn normalize_completion_response_rejects_invalid_typed_argument_values() {
        let response = text_response(
            "<tool_call>typed_tool<arg_key>enabled</arg_key><arg_value>truthy</arg_value></tool_call>",
        );

        let normalized = normalize_completion_response(response, &typed_tool_allowed());

        assert!(matches!(
            normalized.outcome,
            ToolCallNormalizationOutcome::Rejected {
                reason: ToolCallNormalizationRejectReason::InvalidArgumentValue,
                ..
            }
        ));
    }
}
