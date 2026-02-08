//! Intent response parsing.

use crate::claude::AgentError;
use nv_core::types::{Intent, IntentCategory};
use serde::Deserialize;
use std::collections::HashMap;
use tracing::warn;

/// Raw JSON response from Claude for intent classification.
#[derive(Debug, Deserialize)]
struct IntentResponse {
    category: String,
    confidence: f32,
    #[serde(default)]
    entities: HashMap<String, String>,
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

    // Validate confidence is in valid range
    let confidence = response.confidence.clamp(0.0, 1.0);
    if confidence != response.confidence {
        warn!(
            "Confidence {} out of range, clamped to {}",
            response.confidence, confidence
        );
    }

    // Map category string to enum
    let category = category_from_str(&response.category);

    Ok(Intent {
        category,
        confidence,
        entities: response.entities,
        raw_input: original_input.to_string(),
    })
}

/// Convert category string to IntentCategory enum.
///
/// Case-insensitive matching. Unknown categories default to Conversation.
///
/// # Arguments
/// * `s` - Category string from JSON response
///
/// # Returns
/// Corresponding IntentCategory enum variant
pub fn category_from_str(s: &str) -> IntentCategory {
    match s.to_lowercase().as_str() {
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
}
