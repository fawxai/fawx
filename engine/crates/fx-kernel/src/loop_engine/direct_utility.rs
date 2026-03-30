use super::DIRECT_TOOL_TASK_DIRECTIVE;
use crate::act::ToolResult;
use fx_core::message::ProgressKind;
use fx_llm::{CompletionResponse, ContentBlock, ToolCall, ToolDefinition};
use serde_json::{Map, Value};

const DIRECT_UTILITY_BLOCK_REASON: &str =
    "direct utility turns only allow their profile-owned tool surface";
const DIRECT_UTILITY_PROGRESS_KIND: ProgressKind = ProgressKind::Researching;
const ARGUMENT_FILLERS: [&str; 5] = ["in ", "for ", "at ", "to ", "about "];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DirectUtilityProfile {
    pub(super) tool_name: String,
    focus_label: String,
    trigger_patterns: Vec<String>,
    progress_kind: ProgressKind,
    progress_message: String,
    invocation: DirectUtilityInvocation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DirectUtilityInvocation {
    EmptyObject,
    SingleRequiredString {
        parameter_name: String,
        prompt_label: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DirectUtilityMetadata {
    trigger_patterns: Vec<String>,
}

impl DirectUtilityProfile {
    #[cfg(test)]
    pub(crate) fn test_empty_object(
        tool_name: &str,
        description: &str,
        trigger_patterns: &[&str],
    ) -> Self {
        build_direct_utility_profile(
            &tool_definition(
                tool_name,
                description,
                serde_json::json!({"type":"object","properties":{}}),
            ),
            trigger_patterns
                .iter()
                .map(|value| value.to_string())
                .collect(),
            DirectUtilityInvocation::EmptyObject,
        )
    }

    #[cfg(test)]
    pub(crate) fn test_single_required_string(
        tool_name: &str,
        description: &str,
        parameter_name: &str,
        prompt_label: &str,
        trigger_patterns: &[&str],
    ) -> Self {
        build_direct_utility_profile(
            &tool_definition(
                tool_name,
                description,
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        parameter_name: {
                            "type": "string"
                        }
                    },
                    "required": [parameter_name]
                }),
            ),
            trigger_patterns
                .iter()
                .map(|value| value.to_string())
                .collect(),
            DirectUtilityInvocation::SingleRequiredString {
                parameter_name: parameter_name.to_string(),
                prompt_label: prompt_label.to_string(),
            },
        )
    }
}

pub(super) fn detect_direct_utility_profile(
    user_message: &str,
    available_tools: &[ToolDefinition],
) -> Option<DirectUtilityProfile> {
    let lower = user_message.to_lowercase();
    available_tools
        .iter()
        .find_map(|tool| direct_utility_profile_for_tool(tool, &lower))
}

pub(super) fn direct_utility_tool_names(profile: &DirectUtilityProfile) -> Vec<String> {
    vec![profile.tool_name.clone()]
}

pub(super) fn direct_utility_directive(profile: &DirectUtilityProfile) -> String {
    format!(
        "{DIRECT_TOOL_TASK_DIRECTIVE}\n\nDirect tool focus: {}.\nCall `{}` now using its declared schema and answer directly from that result. Do not call other tools unless `{}` fails or cannot answer the request.",
        profile.tool_name, profile.tool_name, profile.tool_name
    )
}

pub(super) fn direct_utility_block_reason(_profile: &DirectUtilityProfile) -> &'static str {
    DIRECT_UTILITY_BLOCK_REASON
}

pub(super) fn direct_utility_progress(profile: &DirectUtilityProfile) -> (ProgressKind, String) {
    (profile.progress_kind, profile.progress_message.clone())
}

pub(super) fn direct_utility_completion_response(
    profile: &DirectUtilityProfile,
    user_message: &str,
    available_tools: &[ToolDefinition],
) -> CompletionResponse {
    match build_direct_utility_call(profile, user_message, available_tools) {
        Ok(call) => CompletionResponse {
            content: Vec::new(),
            tool_calls: vec![call],
            usage: None,
            stop_reason: Some("tool_use".to_string()),
        },
        Err(message) => CompletionResponse {
            content: vec![ContentBlock::Text { text: message }],
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: None,
        },
    }
}

pub(super) fn direct_utility_terminal_response(
    profile: &DirectUtilityProfile,
    tool_results: &[ToolResult],
) -> String {
    if let Some(result) = latest_successful_result(tool_results) {
        return extract_direct_utility_message(&result.output);
    }

    let prefix = format!("I couldn't get {} right now", profile.focus_label);
    if let Some(result) = latest_non_empty_result(tool_results) {
        format!(
            "{prefix}: {}",
            extract_direct_utility_message(&result.output)
        )
    } else {
        prefix
    }
}

pub(super) fn is_structured_tool_schema(schema: &Value) -> bool {
    if schema.get("type").and_then(Value::as_str) != Some("object") {
        return false;
    }
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return false;
    };
    !is_legacy_input_wrapper_schema(schema, properties)
}

fn build_direct_utility_call(
    profile: &DirectUtilityProfile,
    user_message: &str,
    available_tools: &[ToolDefinition],
) -> Result<ToolCall, String> {
    let arguments = direct_utility_arguments(profile, user_message)?;
    let tool = direct_utility_tool_definition(profile, available_tools)?;
    Ok(ToolCall {
        id: direct_utility_call_id(&profile.tool_name),
        name: tool.name.clone(),
        arguments,
    })
}

fn direct_utility_arguments(
    profile: &DirectUtilityProfile,
    user_message: &str,
) -> Result<Value, String> {
    match &profile.invocation {
        DirectUtilityInvocation::EmptyObject => Ok(Value::Object(Map::new())),
        DirectUtilityInvocation::SingleRequiredString {
            parameter_name,
            prompt_label,
        } => {
            let argument = extract_direct_string_argument(user_message, &profile.trigger_patterns)
                .ok_or_else(|| format!("Please tell me the {prompt_label}."))?;
            let mut object = Map::new();
            object.insert(parameter_name.clone(), Value::String(argument));
            Ok(Value::Object(object))
        }
    }
}

fn direct_utility_tool_definition<'a>(
    profile: &DirectUtilityProfile,
    available_tools: &'a [ToolDefinition],
) -> Result<&'a ToolDefinition, String> {
    available_tools
        .iter()
        .find(|tool| tool.name == profile.tool_name)
        .ok_or_else(|| format!("{} is not available in this session.", profile.focus_label))
}

fn direct_utility_call_id(tool_name: &str) -> String {
    format!("direct-{}-1", tool_name.replace('_', "-"))
}

fn latest_successful_result(tool_results: &[ToolResult]) -> Option<&ToolResult> {
    tool_results
        .iter()
        .rev()
        .find(|result| result.success && !result.output.trim().is_empty())
}

fn latest_non_empty_result(tool_results: &[ToolResult]) -> Option<&ToolResult> {
    tool_results
        .iter()
        .rev()
        .find(|result| !result.output.trim().is_empty())
}

fn direct_utility_profile_for_tool(
    tool: &ToolDefinition,
    lower_user_message: &str,
) -> Option<DirectUtilityProfile> {
    let metadata = direct_utility_metadata(&tool.parameters)?;
    if !matches_trigger_patterns(lower_user_message, &metadata.trigger_patterns) {
        return None;
    }
    let invocation = direct_utility_invocation(&tool.parameters)?;
    Some(build_direct_utility_profile(
        tool,
        metadata.trigger_patterns,
        invocation,
    ))
}

fn matches_trigger_patterns(lower_user_message: &str, trigger_patterns: &[String]) -> bool {
    !trigger_patterns.is_empty()
        && trigger_patterns
            .iter()
            .any(|pattern| lower_user_message.contains(pattern))
}

fn build_direct_utility_profile(
    tool: &ToolDefinition,
    trigger_patterns: Vec<String>,
    invocation: DirectUtilityInvocation,
) -> DirectUtilityProfile {
    let focus_label = direct_utility_focus_label(tool);
    DirectUtilityProfile {
        tool_name: tool.name.clone(),
        progress_kind: DIRECT_UTILITY_PROGRESS_KIND,
        progress_message: format!("Checking {}...", focus_label),
        focus_label,
        trigger_patterns,
        invocation,
    }
}

fn direct_utility_focus_label(tool: &ToolDefinition) -> String {
    focus_label_from_description(&tool.description).unwrap_or_else(|| tool.name.replace('_', " "))
}

fn focus_label_from_description(description: &str) -> Option<String> {
    let trimmed = description.trim().trim_end_matches('.');
    let focus = strip_focus_prefix(trimmed)?;
    Some(truncate_focus_phrase(focus))
}

fn strip_focus_prefix(description: &str) -> Option<&str> {
    ["Get ", "Fetch ", "Check "]
        .into_iter()
        .find_map(|prefix| description.strip_prefix(prefix))
}

fn truncate_focus_phrase(text: &str) -> String {
    let lower = text.to_lowercase();
    for separator in [" for ", " with ", " using ", " from "] {
        if let Some(index) = lower.find(separator) {
            return text[..index].trim().to_string();
        }
    }
    text.trim().to_string()
}

fn direct_utility_invocation(schema: &Value) -> Option<DirectUtilityInvocation> {
    if !is_structured_tool_schema(schema) {
        return None;
    }
    if required_property_names(schema).is_empty() {
        return Some(DirectUtilityInvocation::EmptyObject);
    }
    single_required_string_invocation(schema)
}

fn single_required_string_invocation(schema: &Value) -> Option<DirectUtilityInvocation> {
    let required = required_property_names(schema);
    if required.len() != 1 {
        return None;
    }
    let parameter_name = required[0].clone();
    if !property_accepts_string(schema, &parameter_name) {
        return None;
    }
    Some(DirectUtilityInvocation::SingleRequiredString {
        prompt_label: property_prompt_label(schema, &parameter_name),
        parameter_name,
    })
}

fn property_prompt_label(schema: &Value, parameter_name: &str) -> String {
    property_description(schema, parameter_name)
        .and_then(normalize_prompt_label)
        .unwrap_or_else(|| parameter_name.replace('_', " "))
}

fn property_description<'a>(schema: &'a Value, parameter_name: &str) -> Option<&'a str> {
    schema
        .get("properties")
        .and_then(|value| value.get(parameter_name))
        .and_then(|value| value.get("description"))
        .and_then(Value::as_str)
}

fn normalize_prompt_label(description: &str) -> Option<String> {
    let prefix = description
        .split(['(', '['])
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let lower = prefix.to_lowercase();
    for separator in [" to ", " for ", " with ", " in "] {
        if let Some(index) = lower.find(separator) {
            return Some(lower[..index].trim().to_string());
        }
    }
    Some(lower)
}

fn extract_direct_string_argument(
    user_message: &str,
    trigger_patterns: &[String],
) -> Option<String> {
    let lower = user_message.to_lowercase();
    trigger_patterns.iter().find_map(|pattern| {
        let index = lower.find(pattern)?;
        let start = index + pattern.len();
        let tail = user_message.get(start..)?;
        normalize_direct_argument(tail)
    })
}

fn normalize_direct_argument(tail: &str) -> Option<String> {
    let trimmed = tail.trim_start_matches([':', ',', '-', ' ']);
    let without_fillers = strip_argument_fillers(trimmed);
    let cleaned = without_fillers
        .trim_matches(|ch: char| matches!(ch, '?' | '.' | '!' | '"' | '\''))
        .trim();
    (!cleaned.is_empty()).then(|| cleaned.to_string())
}

fn strip_argument_fillers(mut tail: &str) -> &str {
    loop {
        let stripped = ARGUMENT_FILLERS
            .iter()
            .find_map(|filler| tail.strip_prefix(filler))
            .unwrap_or(tail);
        if stripped == tail {
            return tail;
        }
        tail = stripped.trim_start();
    }
}

fn property_accepts_string(schema: &Value, property: &str) -> bool {
    let Some(definition) = schema
        .get("properties")
        .and_then(|value| value.get(property))
    else {
        return false;
    };
    match definition.get("type") {
        Some(Value::String(value)) => value == "string",
        Some(Value::Array(values)) => values.iter().any(|value| value.as_str() == Some("string")),
        _ => false,
    }
}

fn required_property_names(schema: &Value) -> Vec<String> {
    schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect()
}

fn is_legacy_input_wrapper_schema(
    schema: &Value,
    properties: &serde_json::Map<String, Value>,
) -> bool {
    properties.len() == 1
        && properties.contains_key("input")
        && required_property_names(schema) == ["input".to_string()]
        && property_accepts_string(schema, "input")
}

fn direct_utility_metadata(schema: &Value) -> Option<DirectUtilityMetadata> {
    let metadata = schema.get("x-fawx-direct-utility")?;
    let enabled = metadata
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !enabled {
        return None;
    }
    let trigger_patterns = metadata
        .get("trigger_patterns")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_lowercase())
        .collect::<Vec<_>>();
    Some(DirectUtilityMetadata { trigger_patterns })
}

fn extract_direct_utility_message(output: &str) -> String {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Ok(json) = serde_json::from_str::<Value>(trimmed) {
        for key in ["error", "message", "output", "text"] {
            if let Some(value) = json.get(key).and_then(Value::as_str) {
                let value = value.trim();
                if !value.is_empty() {
                    return value.to_string();
                }
            }
        }
    }
    trimmed.to_string()
}

#[cfg(test)]
fn tool_definition(name: &str, description: &str, parameters: Value) -> ToolDefinition {
    ToolDefinition {
        name: name.to_string(),
        description: description.to_string(),
        parameters,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_direct_utility_profile_builds_profile_from_metadata() {
        let profile = detect_direct_utility_profile(
            "What time is it right now?",
            &[tool_definition(
                "current_time",
                "Get the current time",
                serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": [],
                    "x-fawx-direct-utility": {
                        "enabled": true,
                        "trigger_patterns": ["what time", "time is it"]
                    }
                }),
            )],
        )
        .expect("profile");

        assert_eq!(profile.tool_name, "current_time");
        assert_eq!(profile.focus_label, "the current time");
        assert_eq!(
            profile.trigger_patterns,
            vec!["what time".to_string(), "time is it".to_string()]
        );
        assert_eq!(profile.progress_kind, ProgressKind::Researching);
        assert_eq!(profile.progress_message, "Checking the current time...");
    }

    #[test]
    fn legacy_wrapped_schema_fails_structured_schema_validation() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "input": {
                    "type": "string",
                    "description": "JSON input for the WASM skill"
                }
            },
            "required": ["input"]
        });

        assert!(!is_structured_tool_schema(&schema));
    }

    #[test]
    fn direct_utility_does_not_activate_for_legacy_wrapper_with_metadata() {
        let tool = tool_definition(
            "weather",
            "Get the weather for a location",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "input": {
                        "type": "string",
                        "description": "JSON input for the WASM skill"
                    }
                },
                "required": ["input"],
                "x-fawx-direct-utility": {
                    "enabled": true,
                    "trigger_patterns": ["weather", "forecast"]
                }
            }),
        );

        assert!(detect_direct_utility_profile("What's the weather in Miami?", &[tool]).is_none());
    }

    #[test]
    fn direct_utility_builds_single_required_string_call_from_visible_schema() {
        let tool = tool_definition(
            "weather",
            "Get the weather for a location",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "location": {
                        "type": "string",
                        "description": "Location to check"
                    }
                },
                "required": ["location"],
                "x-fawx-direct-utility": {
                    "enabled": true,
                    "trigger_patterns": ["weather", "forecast"]
                }
            }),
        );
        let profile = detect_direct_utility_profile(
            "What's the weather in Denver?",
            std::slice::from_ref(&tool),
        )
        .expect("profile");

        let response =
            direct_utility_completion_response(&profile, "What's the weather in Denver?", &[tool]);
        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].name, "weather");
        assert_eq!(
            response.tool_calls[0].arguments,
            serde_json::json!({"location":"Denver"})
        );
    }
}
