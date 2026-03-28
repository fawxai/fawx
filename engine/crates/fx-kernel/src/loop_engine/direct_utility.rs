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

// TODO(#1646): replace these hardcoded message heuristics with direct-utility
// manifest metadata so the kernel routes only from declared tool contracts.
pub(super) fn detect_direct_utility_profile(
    user_message: &str,
    available_tools: &[ToolDefinition],
) -> Option<DirectUtilityProfile> {
    let lower = user_message.to_lowercase();

    if looks_like_weather_question(&lower) {
        let weather_tool = available_tools.iter().find(|tool| tool.name == "weather")?;
        weather_location_key(weather_tool)?;
        return Some(DirectUtilityProfile::Weather);
    }

    if looks_like_current_time_question(&lower)
        && available_tools.iter().any(|tool| {
            tool.name == "current_time" && schema_accepts_empty_object(&tool.parameters)
        })
    {
        return Some(DirectUtilityProfile::CurrentTime);
    }

    None
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

fn looks_like_weather_question(lower: &str) -> bool {
    let mentions_weather = lower.contains("weather") || lower.contains("forecast");
    let questionish = lower.contains('?')
        || lower.starts_with("what")
        || lower.starts_with("how")
        || lower.starts_with("is it")
        || lower.starts_with("tell me");
    mentions_weather && questionish
}

fn looks_like_current_time_question(lower: &str) -> bool {
    lower.contains("current time")
        || lower.contains("what time")
        || lower.contains("what's the time")
        || lower.contains("whats the time")
        || lower.contains("time is it")
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
