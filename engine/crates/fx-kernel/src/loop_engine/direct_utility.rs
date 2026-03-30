use super::{
    DIRECT_CURRENT_TIME_PHASE_DIRECTIVE, DIRECT_TOOL_TASK_DIRECTIVE, DIRECT_WEATHER_PHASE_DIRECTIVE,
};
use crate::act::ToolResult;
use fx_core::message::ProgressKind;
use fx_llm::{CompletionResponse, ContentBlock, ToolCall, ToolDefinition};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DirectUtilityProfile {
    Weather,
    CurrentTime,
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

pub(super) fn direct_utility_tool_names(profile: &DirectUtilityProfile) -> &'static [&'static str] {
    match profile {
        DirectUtilityProfile::Weather => &["weather"],
        DirectUtilityProfile::CurrentTime => &["current_time"],
    }
}

pub(super) fn direct_utility_directive(profile: &DirectUtilityProfile) -> String {
    match profile {
        DirectUtilityProfile::Weather => {
            format!("{DIRECT_TOOL_TASK_DIRECTIVE}{DIRECT_WEATHER_PHASE_DIRECTIVE}")
        }
        DirectUtilityProfile::CurrentTime => {
            format!("{DIRECT_TOOL_TASK_DIRECTIVE}{DIRECT_CURRENT_TIME_PHASE_DIRECTIVE}")
        }
    }
}

pub(super) fn direct_utility_block_reason(profile: &DirectUtilityProfile) -> &'static str {
    match profile {
        DirectUtilityProfile::Weather | DirectUtilityProfile::CurrentTime => {
            "direct utility turns only allow their profile-owned tool surface"
        }
    }
}

pub(super) fn direct_utility_progress(profile: &DirectUtilityProfile) -> (ProgressKind, String) {
    match profile {
        DirectUtilityProfile::Weather => (
            ProgressKind::Researching,
            "Checking the weather...".to_string(),
        ),
        DirectUtilityProfile::CurrentTime => (
            ProgressKind::Researching,
            "Checking the current time...".to_string(),
        ),
    }
}

pub(super) fn direct_utility_completion_response(
    profile: DirectUtilityProfile,
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
    profile: DirectUtilityProfile,
    tool_results: &[ToolResult],
) -> String {
    if let Some(result) = tool_results
        .iter()
        .rev()
        .find(|result| result.success && !result.output.trim().is_empty())
    {
        return extract_direct_utility_message(&result.output);
    }

    let prefix = match profile {
        DirectUtilityProfile::Weather => "I couldn't get the weather right now",
        DirectUtilityProfile::CurrentTime => "I couldn't get the current time right now",
    };

    if let Some(result) = tool_results
        .iter()
        .rev()
        .find(|result| !result.output.trim().is_empty())
    {
        format!(
            "{prefix}: {}",
            extract_direct_utility_message(&result.output)
        )
    } else {
        prefix.to_string()
    }
}

fn build_direct_utility_call(
    profile: DirectUtilityProfile,
    user_message: &str,
    available_tools: &[ToolDefinition],
) -> Result<ToolCall, String> {
    match profile {
        DirectUtilityProfile::Weather => {
            let location = extract_weather_location(user_message).ok_or_else(|| {
                "Please tell me the city or location you want weather for.".to_string()
            })?;
            let weather_tool = available_tools
                .iter()
                .find(|tool| tool.name == "weather")
                .ok_or_else(|| "Weather is not available in this session.".to_string())?;
            let location_key = weather_location_key(weather_tool).ok_or_else(|| {
                "Weather is not exposed with a direct location schema.".to_string()
            })?;
            let mut arguments = serde_json::Map::new();
            arguments.insert(location_key, serde_json::Value::String(location));
            Ok(ToolCall {
                id: "direct-weather-1".to_string(),
                name: "weather".to_string(),
                arguments: serde_json::Value::Object(arguments),
            })
        }
        DirectUtilityProfile::CurrentTime => Ok(ToolCall {
            id: "direct-current-time-1".to_string(),
            name: "current_time".to_string(),
            arguments: serde_json::json!({}),
        }),
    }
}

fn direct_utility_profile_for_tool(
    tool: &ToolDefinition,
    lower_user_message: &str,
) -> Option<DirectUtilityProfile> {
    let metadata = direct_utility_metadata(&tool.parameters)?;
    if !metadata.enabled
        || metadata.trigger_patterns.is_empty()
        || !metadata
            .trigger_patterns
            .iter()
            .any(|pattern| lower_user_message.contains(pattern))
    {
        return None;
    }

    match metadata.profile.as_deref() {
        Some("weather") if weather_location_key(tool).is_some() => {
            Some(DirectUtilityProfile::Weather)
        }
        Some("current_time") if schema_accepts_empty_object(&tool.parameters) => {
            Some(DirectUtilityProfile::CurrentTime)
        }
        _ => None,
    }
}

fn extract_weather_location(user_message: &str) -> Option<String> {
    let lower = user_message.to_lowercase();
    let markers = [
        "weather in ",
        "forecast in ",
        "weather for ",
        "forecast for ",
    ];
    for marker in markers {
        if let Some(index) = lower.find(marker) {
            let start = index + marker.len();
            let tail = user_message.get(start..)?.trim();
            let cleaned = tail
                .trim_matches(|c: char| matches!(c, '?' | '.' | '!' | '"' | '\''))
                .trim();
            if !cleaned.is_empty() {
                return Some(cleaned.to_string());
            }
        }
    }
    None
}

fn weather_location_key(tool: &ToolDefinition) -> Option<String> {
    let required = required_property_names(&tool.parameters);
    if !required.contains("location") {
        return None;
    }
    property_accepts_string(&tool.parameters, "location").then(|| "location".to_string())
}

fn schema_accepts_empty_object(schema: &serde_json::Value) -> bool {
    schema.get("type").and_then(serde_json::Value::as_str) == Some("object")
        && required_property_names(schema).is_empty()
}

fn property_accepts_string(schema: &serde_json::Value, property: &str) -> bool {
    let Some(definition) = schema
        .get("properties")
        .and_then(|value| value.get(property))
    else {
        return false;
    };
    match definition.get("type") {
        Some(serde_json::Value::String(value)) => value == "string",
        Some(serde_json::Value::Array(values)) => {
            values.iter().any(|value| value.as_str() == Some("string"))
        }
        _ => false,
    }
}

fn required_property_names(schema: &serde_json::Value) -> HashSet<&str> {
    schema
        .get("required")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DirectUtilityMetadata {
    enabled: bool,
    profile: Option<String>,
    trigger_patterns: Vec<String>,
}

fn direct_utility_metadata(schema: &serde_json::Value) -> Option<DirectUtilityMetadata> {
    let metadata = schema.get("x-fawx-direct-utility")?;
    let enabled = metadata
        .get("enabled")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if !enabled {
        return None;
    }

    let profile = metadata
        .get("profile")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let trigger_patterns = metadata
        .get("trigger_patterns")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_lowercase())
        .collect::<Vec<_>>();

    Some(DirectUtilityMetadata {
        enabled,
        profile,
        trigger_patterns,
    })
}

fn extract_direct_utility_message(output: &str) -> String {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    if let Ok(json) = serde_json::from_str::<serde_json::Value>(trimmed) {
        for key in ["error", "message", "output", "text"] {
            if let Some(value) = json.get(key).and_then(serde_json::Value::as_str) {
                let value = value.trim();
                if !value.is_empty() {
                    return value.to_string();
                }
            }
        }
    }

    trimmed.to_string()
}
