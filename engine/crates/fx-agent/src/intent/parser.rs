//! Intent response parsing.

use crate::claude::AgentError;
use fx_core::types::{Intent, IntentCategory};
use serde::Deserialize;
use std::collections::HashMap;
use tracing::warn;

/// Maximum length for entity values (characters). Values exceeding this are truncated.
const MAX_ENTITY_VALUE_LENGTH: usize = 1024;
/// Marker appended to truncated entity values
const TRUNCATION_MARKER: &str = "...";

/// Raw JSON response from Claude for intent classification.
///
/// Note: Extra top-level fields are ignored by serde, but extra keys
/// within the `entities` HashMap are preserved since it's a HashMap<String, String>.
#[derive(Debug, Deserialize)]
struct IntentResponse {
    category: String,
    confidence: f32,
    #[serde(default)]
    entities: HashMap<String, String>,
}

/// Truncate entity value if it exceeds maximum length.
///
/// Values longer than MAX_ENTITY_VALUE_LENGTH are truncated to exactly
/// MAX_ENTITY_VALUE_LENGTH characters, with the truncation marker appended.
/// This prevents malicious or accidental extremely long entity values from consuming
/// excessive memory or causing downstream issues.
///
/// # Arguments
/// * `value` - Entity value to truncate
///
/// # Returns
/// Truncated value if needed (exactly MAX_ENTITY_VALUE_LENGTH chars), otherwise original value
fn truncate_entity_value(value: String) -> String {
    let char_count = value.chars().count();
    if char_count > MAX_ENTITY_VALUE_LENGTH {
        let max_chars = MAX_ENTITY_VALUE_LENGTH.saturating_sub(TRUNCATION_MARKER.len());
        let truncated: String = value.chars().take(max_chars).collect();
        warn!(
            "Entity value exceeds {} chars, truncated from {} to {}",
            MAX_ENTITY_VALUE_LENGTH, char_count, MAX_ENTITY_VALUE_LENGTH
        );
        format!("{}{}", truncated, TRUNCATION_MARKER)
    } else {
        value
    }
}

/// Parse Claude's JSON response into an Intent.
///
/// Handles malformed JSON gracefully by falling back to Conversation with low confidence.
///
/// # Arguments
/// * `raw` - Raw JSON string from Claude
/// * `original_input` - Original user input text (for fallback)
///
/// # Returns
/// Parsed Intent or error if parsing fails completely
pub fn parse_intent_response(raw: &str, original_input: &str) -> Result<Intent, AgentError> {
    // Try to parse JSON
    let response: IntentResponse = match serde_json::from_str(raw) {
        Ok(r) => r,
        Err(e) => {
            warn!(
                "Failed to parse intent JSON: {}. Raw response: {}. Falling back to Conversation.",
                e, raw
            );
            // Graceful fallback to Conversation
            return Ok(Intent {
                category: IntentCategory::Conversation,
                confidence: 0.3,
                entities: HashMap::new(),
                raw_input: original_input.to_string(),
            });
        }
    };

    // Validate confidence is finite and in valid range
    let confidence = if response.confidence.is_finite() {
        let clamped = response.confidence.clamp(0.0, 1.0);
        if clamped != response.confidence {
            warn!(
                "Confidence {} out of range, clamped to {}",
                response.confidence, clamped
            );
        }
        clamped
    } else {
        warn!(
            "Invalid confidence {} (NaN or infinity), defaulting to 0.3",
            response.confidence
        );
        0.3
    };

    // Map category string to enum
    let category = category_from_str(&response.category);

    // Truncate entity values that exceed maximum length
    let entities = response
        .entities
        .into_iter()
        .map(|(key, value)| (key, truncate_entity_value(value)))
        .collect();

    Ok(Intent {
        category,
        confidence,
        entities,
        raw_input: original_input.to_string(),
    })
}

/// Convert category string to IntentCategory enum.
///
/// Case-insensitive matching with normalization (removes spaces, underscores, hyphens).
/// Unknown categories default to Conversation.
///
/// # Arguments
/// * `s` - Category string from JSON response
///
/// # Returns
/// Corresponding IntentCategory enum variant
///
/// # Examples
/// ```
/// # use fx_agent::category_from_str;
/// # use fx_core::types::IntentCategory;
/// assert_eq!(category_from_str("LaunchApp"), IntentCategory::LaunchApp);
/// assert_eq!(category_from_str("launch_app"), IntentCategory::LaunchApp);
/// assert_eq!(category_from_str("launch-app"), IntentCategory::LaunchApp);
/// assert_eq!(category_from_str("launch app"), IntentCategory::LaunchApp);
/// ```
pub fn category_from_str(s: &str) -> IntentCategory {
    // Normalize: lowercase + remove delimiters (spaces, underscores, hyphens)
    let normalized = s
        .to_lowercase()
        .chars()
        .filter(|c| !matches!(c, ' ' | '_' | '-'))
        .collect::<String>();

    match normalized.as_str() {
        "launchapp" => IntentCategory::LaunchApp,
        "search" => IntentCategory::Search,
        "navigate" => IntentCategory::Navigate,
        "message" => IntentCategory::Message,
        "calendar" => IntentCategory::Calendar,
        "settings" => IntentCategory::Settings,
        "question" => IntentCategory::Question,
        "complextask" => IntentCategory::ComplexTask,
        "conversation" => IntentCategory::Conversation,
        unknown => {
            warn!("Unknown category '{}', defaulting to Conversation", unknown);
            IntentCategory::Conversation
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_json() {
        let raw = r#"{
            "category": "LaunchApp",
            "confidence": 0.95,
            "entities": {"app_name": "spotify"}
        }"#;

        let intent = parse_intent_response(raw, "open spotify").unwrap();
        assert_eq!(intent.category, IntentCategory::LaunchApp);
        assert_eq!(intent.confidence, 0.95);
        assert_eq!(intent.entities.get("app_name").unwrap(), "spotify");
        assert_eq!(intent.raw_input, "open spotify");
    }

    #[test]
    fn test_parse_malformed_json() {
        let raw = "not valid json at all";
        let intent = parse_intent_response(raw, "test input").unwrap();

        // Should fallback to Conversation with low confidence
        assert_eq!(intent.category, IntentCategory::Conversation);
        assert_eq!(intent.confidence, 0.3);
        assert_eq!(intent.raw_input, "test input");
    }

    #[test]
    fn test_parse_missing_entities() {
        let raw = r#"{
            "category": "Conversation",
            "confidence": 0.8
        }"#;

        let intent = parse_intent_response(raw, "hey").unwrap();
        assert_eq!(intent.category, IntentCategory::Conversation);
        assert_eq!(intent.confidence, 0.8);
        assert!(intent.entities.is_empty());
    }

    #[test]
    fn test_confidence_clamping() {
        // Test confidence > 1.0
        let raw = r#"{
            "category": "Question",
            "confidence": 1.5
        }"#;

        let intent = parse_intent_response(raw, "test").unwrap();
        assert_eq!(intent.confidence, 1.0);

        // Test confidence < 0.0
        let raw = r#"{
            "category": "Question",
            "confidence": -0.5
        }"#;

        let intent = parse_intent_response(raw, "test").unwrap();
        assert_eq!(intent.confidence, 0.0);
    }

    #[test]
    fn test_category_from_str_case_insensitive() {
        assert_eq!(category_from_str("LaunchApp"), IntentCategory::LaunchApp);
        assert_eq!(category_from_str("launchapp"), IntentCategory::LaunchApp);
        assert_eq!(category_from_str("LAUNCHAPP"), IntentCategory::LaunchApp);
        assert_eq!(category_from_str("LaUnChApP"), IntentCategory::LaunchApp);
    }

    #[test]
    fn test_category_from_str_all_categories() {
        assert_eq!(category_from_str("LaunchApp"), IntentCategory::LaunchApp);
        assert_eq!(category_from_str("Search"), IntentCategory::Search);
        assert_eq!(category_from_str("Navigate"), IntentCategory::Navigate);
        assert_eq!(category_from_str("Message"), IntentCategory::Message);
        assert_eq!(category_from_str("Calendar"), IntentCategory::Calendar);
        assert_eq!(category_from_str("Settings"), IntentCategory::Settings);
        assert_eq!(category_from_str("Question"), IntentCategory::Question);
        assert_eq!(
            category_from_str("ComplexTask"),
            IntentCategory::ComplexTask
        );
        assert_eq!(
            category_from_str("Conversation"),
            IntentCategory::Conversation
        );
    }

    #[test]
    fn test_category_from_str_unknown() {
        assert_eq!(
            category_from_str("UnknownCategory"),
            IntentCategory::Conversation
        );
        assert_eq!(category_from_str(""), IntentCategory::Conversation);
        assert_eq!(category_from_str("xyz"), IntentCategory::Conversation);
    }

    #[test]
    fn test_category_from_str_with_delimiters() {
        // Underscores
        assert_eq!(category_from_str("launch_app"), IntentCategory::LaunchApp);
        assert_eq!(
            category_from_str("complex_task"),
            IntentCategory::ComplexTask
        );

        // Hyphens
        assert_eq!(category_from_str("launch-app"), IntentCategory::LaunchApp);
        assert_eq!(
            category_from_str("complex-task"),
            IntentCategory::ComplexTask
        );

        // Spaces
        assert_eq!(category_from_str("launch app"), IntentCategory::LaunchApp);
        assert_eq!(
            category_from_str("complex task"),
            IntentCategory::ComplexTask
        );

        // Mixed
        assert_eq!(category_from_str("launch_App"), IntentCategory::LaunchApp);
        assert_eq!(
            category_from_str("Complex-Task"),
            IntentCategory::ComplexTask
        );
    }

    #[test]
    fn test_confidence_nan_handling() {
        let raw = r#"{
            "category": "Question",
            "confidence": null
        }"#;

        // This will fail to parse confidence as f32, so serde will error
        // The malformed JSON fallback will catch it
        let intent = parse_intent_response(raw, "test").unwrap();
        assert_eq!(intent.category, IntentCategory::Conversation);
        assert_eq!(intent.confidence, 0.3);
    }
}
