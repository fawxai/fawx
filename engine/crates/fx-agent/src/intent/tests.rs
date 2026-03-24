//! Comprehensive test suite for intent classification.

use super::classifier::{ClassifierConfig, IntentClassifier, MockClassifier};
use super::prompts::INTENT_SYSTEM_PROMPT;
use fx_core::types::{InputSource, IntentCategory, UserInput};

fn create_user_input(text: &str) -> UserInput {
    UserInput {
        text: text.to_string(),
        source: InputSource::Text,
        timestamp: 1234567890,
        context_id: None,
        images: Vec::new(),
        documents: Vec::new(),
    }
}

// ============================================================================
// Intent Category Classification Tests (9 categories)
// ============================================================================

#[tokio::test]
async fn test_launch_app_intent() {
    let mock = MockClassifier::with_response(
        r#"{"category": "LaunchApp", "confidence": 0.95, "entities": {"app_name": "spotify"}}"#
            .to_string(),
    );
    let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

    let input = create_user_input("open spotify");
    let intent = classifier.classify(&input).await.unwrap();

    assert_eq!(intent.category, IntentCategory::LaunchApp);
    assert_eq!(intent.confidence, 0.95);
    assert_eq!(intent.entities.get("app_name").unwrap(), "spotify");
    assert_eq!(intent.raw_input, "open spotify");
}

#[tokio::test]
async fn test_search_intent() {
    let mock = MockClassifier::with_response(
        r#"{"category": "Search", "confidence": 0.9, "entities": {"query": "restaurants", "location": "nearby"}}"#
            .to_string(),
    );
    let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

    let input = create_user_input("find restaurants nearby");
    let intent = classifier.classify(&input).await.unwrap();

    assert_eq!(intent.category, IntentCategory::Search);
    assert_eq!(intent.confidence, 0.9);
    assert_eq!(intent.entities.get("query").unwrap(), "restaurants");
    assert_eq!(intent.entities.get("location").unwrap(), "nearby");
}

#[tokio::test]
async fn test_navigate_intent() {
    let mock = MockClassifier::with_response(
        r#"{"category": "Navigate", "confidence": 0.92, "entities": {"destination": "coffee shop"}}"#
            .to_string(),
    );
    let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

    let input = create_user_input("navigate to coffee shop");
    let intent = classifier.classify(&input).await.unwrap();

    assert_eq!(intent.category, IntentCategory::Navigate);
    assert_eq!(intent.entities.get("destination").unwrap(), "coffee shop");
}

#[tokio::test]
async fn test_message_intent() {
    let mock = MockClassifier::with_response(
        r#"{"category": "Message", "confidence": 0.9, "entities": {"contact": "mom", "message": "happy birthday", "channel": "text"}}"#
            .to_string(),
    );
    let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

    let input = create_user_input("text mom happy birthday");
    let intent = classifier.classify(&input).await.unwrap();

    assert_eq!(intent.category, IntentCategory::Message);
    assert_eq!(intent.entities.get("contact").unwrap(), "mom");
    assert_eq!(intent.entities.get("message").unwrap(), "happy birthday");
}

#[tokio::test]
async fn test_calendar_intent() {
    let mock = MockClassifier::with_response(
        r#"{"category": "Calendar", "confidence": 0.93, "entities": {"action": "alarm", "time": "7am"}}"#
            .to_string(),
    );
    let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

    let input = create_user_input("set alarm for 7am");
    let intent = classifier.classify(&input).await.unwrap();

    assert_eq!(intent.category, IntentCategory::Calendar);
    assert_eq!(intent.entities.get("action").unwrap(), "alarm");
    assert_eq!(intent.entities.get("time").unwrap(), "7am");
}

#[tokio::test]
async fn test_settings_intent() {
    let mock = MockClassifier::with_response(
        r#"{"category": "Settings", "confidence": 0.94, "entities": {"setting": "bluetooth", "value": "on"}}"#
            .to_string(),
    );
    let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

    let input = create_user_input("turn on bluetooth");
    let intent = classifier.classify(&input).await.unwrap();

    assert_eq!(intent.category, IntentCategory::Settings);
    assert_eq!(intent.entities.get("setting").unwrap(), "bluetooth");
    assert_eq!(intent.entities.get("value").unwrap(), "on");
}

#[tokio::test]
async fn test_question_intent() {
    let mock = MockClassifier::with_response(
        r#"{"category": "Question", "confidence": 0.95, "entities": {"query": "weather"}}"#
            .to_string(),
    );
    let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

    let input = create_user_input("what's the weather");
    let intent = classifier.classify(&input).await.unwrap();

    assert_eq!(intent.category, IntentCategory::Question);
    assert_eq!(intent.entities.get("query").unwrap(), "weather");
}

#[tokio::test]
async fn test_complex_task_intent() {
    let mock = MockClassifier::with_response(
        r#"{"category": "ComplexTask", "confidence": 0.88, "entities": {"tasks": "flight,hotel", "timeframe": "next week"}}"#
            .to_string(),
    );
    let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

    let input = create_user_input("book a flight and hotel for next week");
    let intent = classifier.classify(&input).await.unwrap();

    assert_eq!(intent.category, IntentCategory::ComplexTask);
    assert_eq!(intent.entities.get("tasks").unwrap(), "flight,hotel");
    assert_eq!(intent.entities.get("timeframe").unwrap(), "next week");
}

#[tokio::test]
async fn test_conversation_intent() {
    let mock = MockClassifier::with_response(
        r#"{"category": "Conversation", "confidence": 0.85, "entities": {}}"#.to_string(),
    );
    let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

    let input = create_user_input("hey how's it going");
    let intent = classifier.classify(&input).await.unwrap();

    assert_eq!(intent.category, IntentCategory::Conversation);
    assert!(intent.entities.is_empty());
}

// ============================================================================
// Confidence Threshold Tests
// ============================================================================

#[tokio::test]
async fn test_low_confidence_fallback() {
    let mock = MockClassifier::with_response(
        r#"{"category": "Question", "confidence": 0.4, "entities": {}}"#.to_string(),
    );
    let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

    let input = create_user_input("unclear input");
    let intent = classifier.classify(&input).await.unwrap();

    // Should fall back to Conversation due to low confidence
    assert_eq!(intent.category, IntentCategory::Conversation);
    assert_eq!(intent.confidence, 0.4);
}

#[tokio::test]
async fn test_confidence_exactly_at_threshold() {
    let mock = MockClassifier::with_response(
        r#"{"category": "Search", "confidence": 0.7, "entities": {}}"#.to_string(),
    );

    let config = ClassifierConfig {
        confidence_threshold: 0.7,
    };
    let classifier = IntentClassifier::new(config, mock);

    let input = create_user_input("search something");
    let intent = classifier.classify(&input).await.unwrap();

    // >= threshold, should NOT be overridden
    assert_eq!(intent.category, IntentCategory::Search);
    assert_eq!(intent.confidence, 0.7);
}

#[tokio::test]
async fn test_confidence_just_below_threshold() {
    let mock = MockClassifier::with_response(
        r#"{"category": "Navigate", "confidence": 0.69, "entities": {}}"#.to_string(),
    );

    let config = ClassifierConfig {
        confidence_threshold: 0.7,
    };
    let classifier = IntentClassifier::new(config, mock);

    let input = create_user_input("maybe navigate");
    let intent = classifier.classify(&input).await.unwrap();

    // < threshold, should be overridden
    assert_eq!(intent.category, IntentCategory::Conversation);
    assert_eq!(intent.confidence, 0.69);
}

#[tokio::test]
async fn test_custom_confidence_threshold() {
    let mock = MockClassifier::with_response(
        r#"{"category": "LaunchApp", "confidence": 0.75, "entities": {}}"#.to_string(),
    );

    let config = ClassifierConfig {
        confidence_threshold: 0.8, // Higher threshold
    };
    let classifier = IntentClassifier::new(config, mock);

    let input = create_user_input("open app");
    let intent = classifier.classify(&input).await.unwrap();

    // 0.75 < 0.8, should be overridden
    assert_eq!(intent.category, IntentCategory::Conversation);
}

// ============================================================================
// Malformed JSON Handling
// ============================================================================

#[tokio::test]
async fn test_malformed_json_fallback() {
    let mock = MockClassifier::with_response("not valid json at all".to_string());
    let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

    let input = create_user_input("some input");
    let intent = classifier.classify(&input).await.unwrap();

    // Should gracefully fall back to Conversation
    assert_eq!(intent.category, IntentCategory::Conversation);
    assert_eq!(intent.confidence, 0.3);
    assert_eq!(intent.raw_input, "some input");
}

#[tokio::test]
async fn test_incomplete_json() {
    let mock = MockClassifier::with_response(r#"{"category": "Question""#.to_string());
    let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

    let input = create_user_input("test");
    let intent = classifier.classify(&input).await.unwrap();

    assert_eq!(intent.category, IntentCategory::Conversation);
    assert_eq!(intent.confidence, 0.3);
}

// ============================================================================
// Entity Extraction Tests
// ============================================================================

#[tokio::test]
async fn test_entity_extraction_app_name() {
    let mock = MockClassifier::with_response(
        r#"{"category": "LaunchApp", "confidence": 0.95, "entities": {"app_name": "gmail"}}"#
            .to_string(),
    );
    let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

    let input = create_user_input("open gmail");
    let intent = classifier.classify(&input).await.unwrap();

    assert_eq!(intent.entities.get("app_name").unwrap(), "gmail");
}

#[tokio::test]
async fn test_entity_extraction_contact_and_message() {
    let mock = MockClassifier::with_response(
        r#"{"category": "Message", "confidence": 0.9, "entities": {"contact": "alice", "message": "see you soon"}}"#
            .to_string(),
    );
    let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

    let input = create_user_input("text alice see you soon");
    let intent = classifier.classify(&input).await.unwrap();

    assert_eq!(intent.entities.get("contact").unwrap(), "alice");
    assert_eq!(intent.entities.get("message").unwrap(), "see you soon");
}

#[tokio::test]
async fn test_entity_extraction_time() {
    let mock = MockClassifier::with_response(
        r#"{"category": "Calendar", "confidence": 0.92, "entities": {"action": "reminder", "time": "3pm", "task": "call john"}}"#
            .to_string(),
    );
    let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

    let input = create_user_input("remind me to call john at 3pm");
    let intent = classifier.classify(&input).await.unwrap();

    assert_eq!(intent.entities.get("time").unwrap(), "3pm");
    assert_eq!(intent.entities.get("task").unwrap(), "call john");
}

#[tokio::test]
async fn test_entity_extraction_location() {
    let mock = MockClassifier::with_response(
        r#"{"category": "Search", "confidence": 0.88, "entities": {"query": "coffee", "location": "downtown"}}"#
            .to_string(),
    );
    let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

    let input = create_user_input("find coffee downtown");
    let intent = classifier.classify(&input).await.unwrap();

    assert_eq!(intent.entities.get("query").unwrap(), "coffee");
    assert_eq!(intent.entities.get("location").unwrap(), "downtown");
}

// ============================================================================
// Conversation Context Tests
// ============================================================================

#[tokio::test]
async fn test_classify_with_context() {
    use crate::claude::Message;

    let mock = MockClassifier::with_response(
        r#"{"category": "Conversation", "confidence": 0.9, "entities": {}}"#.to_string(),
    );
    let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

    let history = vec![
        Message::user("hello"),
        Message::assistant("Hi! How can I help?"),
    ];

    let input = create_user_input("thanks");
    let intent = classifier
        .classify_with_context(&input, &history)
        .await
        .unwrap();

    assert_eq!(intent.category, IntentCategory::Conversation);
}

#[tokio::test]
async fn test_classify_with_empty_context() {
    let mock = MockClassifier::with_response(
        r#"{"category": "Question", "confidence": 0.85, "entities": {}}"#.to_string(),
    );
    let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

    let history = vec![];
    let input = create_user_input("what time is it");
    let intent = classifier
        .classify_with_context(&input, &history)
        .await
        .unwrap();

    assert_eq!(intent.category, IntentCategory::Question);
}

// ============================================================================
// Empty Input Handling
// ============================================================================

#[tokio::test]
async fn test_empty_input() {
    let mock = MockClassifier::with_response(
        r#"{"category": "Conversation", "confidence": 0.5, "entities": {}}"#.to_string(),
    );
    let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

    let input = create_user_input("");
    let intent = classifier.classify(&input).await.unwrap();

    // Empty input should be classified as Conversation with low confidence
    assert_eq!(intent.category, IntentCategory::Conversation);
    assert_eq!(intent.raw_input, "");
}

// ============================================================================
// System Prompt Tests
// ============================================================================

#[test]
fn test_system_prompt_contains_all_categories() {
    // Verify the system prompt documents all 9 categories
    assert!(INTENT_SYSTEM_PROMPT.contains("LaunchApp"));
    assert!(INTENT_SYSTEM_PROMPT.contains("Search"));
    assert!(INTENT_SYSTEM_PROMPT.contains("Navigate"));
    assert!(INTENT_SYSTEM_PROMPT.contains("Message"));
    assert!(INTENT_SYSTEM_PROMPT.contains("Calendar"));
    assert!(INTENT_SYSTEM_PROMPT.contains("Settings"));
    assert!(INTENT_SYSTEM_PROMPT.contains("Question"));
    assert!(INTENT_SYSTEM_PROMPT.contains("ComplexTask"));
    assert!(INTENT_SYSTEM_PROMPT.contains("Conversation"));
}

#[test]
fn test_system_prompt_describes_confidence_scoring() {
    // Verify confidence scoring guidelines are present
    assert!(INTENT_SYSTEM_PROMPT.contains("0.9"));
    assert!(INTENT_SYSTEM_PROMPT.contains("0.7"));
    assert!(INTENT_SYSTEM_PROMPT.contains("0.5"));
    assert!(INTENT_SYSTEM_PROMPT.contains("confidence"));
}

#[test]
fn test_system_prompt_has_json_format() {
    // Verify it instructs JSON response format
    assert!(INTENT_SYSTEM_PROMPT.contains("JSON"));
    assert!(INTENT_SYSTEM_PROMPT.contains("category"));
    assert!(INTENT_SYSTEM_PROMPT.contains("entities"));
}

// ============================================================================
// Configuration Tests
// ============================================================================

#[test]
fn test_classifier_config_defaults() {
    let config = ClassifierConfig::default();
    assert_eq!(config.confidence_threshold, 0.7);
}

#[test]
fn test_classifier_config_custom() {
    let config = ClassifierConfig {
        confidence_threshold: 0.8,
    };
    assert_eq!(config.confidence_threshold, 0.8);
}

// ============================================================================
// MockClassifier Tests
// ============================================================================

#[tokio::test]
async fn test_mock_classifier_single_response() {
    use crate::claude::Message;
    use crate::intent::classifier::LlmClassifier;

    let mock = MockClassifier::with_response(r#"{"test": "response"}"#.to_string());

    // Should return the same response multiple times
    let resp1 = mock.classify_raw(&[Message::user("test")]).await.unwrap();
    let resp2 = mock.classify_raw(&[Message::user("test")]).await.unwrap();

    assert_eq!(resp1, r#"{"test": "response"}"#);
    assert_eq!(resp2, r#"{"test": "response"}"#);
}

#[tokio::test]
async fn test_mock_classifier_multiple_responses() {
    use crate::claude::Message;
    use crate::intent::classifier::LlmClassifier;

    let mock = MockClassifier::new(vec![
        r#"{"response": 1}"#.to_string(),
        r#"{"response": 2}"#.to_string(),
    ]);

    let resp1 = mock.classify_raw(&[Message::user("test")]).await.unwrap();
    let resp2 = mock.classify_raw(&[Message::user("test")]).await.unwrap();

    assert_eq!(resp1, r#"{"response": 1}"#);
    assert_eq!(resp2, r#"{"response": 2}"#);
}

// ============================================================================
// Unicode Entity Tests (Issue #148)
// ============================================================================

#[tokio::test]
async fn test_unicode_emoji_in_app_name() {
    let mock = MockClassifier::with_response(
        r#"{"category": "LaunchApp", "confidence": 0.95, "entities": {"app_name": "📱 Phone"}}"#
            .to_string(),
    );
    let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

    let input = create_user_input("open 📱 app");
    let intent = classifier.classify(&input).await.unwrap();

    assert_eq!(intent.category, IntentCategory::LaunchApp);
    assert_eq!(intent.entities.get("app_name").unwrap(), "📱 Phone");
}

#[tokio::test]
async fn test_unicode_emoji_in_message() {
    let mock = MockClassifier::with_response(
        r#"{"category": "Message", "confidence": 0.92, "entities": {"contact": "Alice", "message": "Happy birthday! 🎉🎂"}}"#
            .to_string(),
    );
    let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

    let input = create_user_input("text Alice Happy birthday! 🎉🎂");
    let intent = classifier.classify(&input).await.unwrap();

    assert_eq!(intent.category, IntentCategory::Message);
    assert_eq!(
        intent.entities.get("message").unwrap(),
        "Happy birthday! 🎉🎂"
    );
}

#[tokio::test]
async fn test_unicode_cjk_characters_in_contact() {
    let mock = MockClassifier::with_response(
        r#"{"category": "Message", "confidence": 0.93, "entities": {"contact": "李明", "message": "你好"}}"#
            .to_string(),
    );
    let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

    let input = create_user_input("text 李明 你好");
    let intent = classifier.classify(&input).await.unwrap();

    assert_eq!(intent.category, IntentCategory::Message);
    assert_eq!(intent.entities.get("contact").unwrap(), "李明");
    assert_eq!(intent.entities.get("message").unwrap(), "你好");
}

#[tokio::test]
async fn test_unicode_arabic_in_query() {
    let mock = MockClassifier::with_response(
        r#"{"category": "Search", "confidence": 0.90, "entities": {"query": "مطعم", "location": "دبي"}}"#
            .to_string(),
    );
    let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

    let input = create_user_input("find مطعم in دبي");
    let intent = classifier.classify(&input).await.unwrap();

    assert_eq!(intent.category, IntentCategory::Search);
    assert_eq!(intent.entities.get("query").unwrap(), "مطعم");
    assert_eq!(intent.entities.get("location").unwrap(), "دبي");
}

#[tokio::test]
async fn test_unicode_cyrillic_in_destination() {
    let mock = MockClassifier::with_response(
        r#"{"category": "Navigate", "confidence": 0.91, "entities": {"destination": "Москва"}}"#
            .to_string(),
    );
    let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

    let input = create_user_input("navigate to Москва");
    let intent = classifier.classify(&input).await.unwrap();

    assert_eq!(intent.category, IntentCategory::Navigate);
    assert_eq!(intent.entities.get("destination").unwrap(), "Москва");
}

#[tokio::test]
async fn test_unicode_mixed_scripts() {
    let mock = MockClassifier::with_response(
        r#"{"category": "Message", "confidence": 0.88, "entities": {"contact": "José García", "message": "Hello! 안녕하세요 👋"}}"#
            .to_string(),
    );
    let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

    let input = create_user_input("text José García Hello! 안녕하세요 👋");
    let intent = classifier.classify(&input).await.unwrap();

    assert_eq!(intent.category, IntentCategory::Message);
    assert_eq!(intent.entities.get("contact").unwrap(), "José García");
    assert_eq!(
        intent.entities.get("message").unwrap(),
        "Hello! 안녕하세요 👋"
    );
}

#[tokio::test]
async fn test_unicode_hebrew_in_task() {
    let mock = MockClassifier::with_response(
        r#"{"category": "Calendar", "confidence": 0.89, "entities": {"action": "reminder", "task": "לקנות חלב"}}"#
            .to_string(),
    );
    let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

    let input = create_user_input("remind me to לקנות חלב");
    let intent = classifier.classify(&input).await.unwrap();

    assert_eq!(intent.category, IntentCategory::Calendar);
    assert_eq!(intent.entities.get("task").unwrap(), "לקנות חלב");
}

#[tokio::test]
async fn test_unicode_japanese_in_app_name() {
    let mock = MockClassifier::with_response(
        r#"{"category": "LaunchApp", "confidence": 0.94, "entities": {"app_name": "天気予報"}}"#
            .to_string(),
    );
    let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

    let input = create_user_input("open 天気予報");
    let intent = classifier.classify(&input).await.unwrap();

    assert_eq!(intent.category, IntentCategory::LaunchApp);
    assert_eq!(intent.entities.get("app_name").unwrap(), "天気予報");
}

// ============================================================================
// JSON Extra Fields Tests (Issue #149)
// ============================================================================

#[tokio::test]
async fn test_json_with_extra_fields() {
    // LLM returns extra fields that aren't in the struct
    let mock = MockClassifier::with_response(
        r#"{"category": "LaunchApp", "confidence": 0.95, "entities": {"app_name": "spotify"}, "extra_field": "ignored", "another_extra": 42}"#
            .to_string(),
    );
    let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

    let input = create_user_input("open spotify");
    let intent = classifier.classify(&input).await.unwrap();

    // Should successfully parse, ignoring extra fields
    assert_eq!(intent.category, IntentCategory::LaunchApp);
    assert_eq!(intent.confidence, 0.95);
    assert_eq!(intent.entities.get("app_name").unwrap(), "spotify");
    // Extra fields should be silently ignored by serde
}

#[tokio::test]
async fn test_json_with_nested_extra_objects() {
    let mock = MockClassifier::with_response(
        r#"{"category": "Message", "confidence": 0.9, "entities": {"contact": "alice"}, "metadata": {"timestamp": 123, "source": "test"}}"#
            .to_string(),
    );
    let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

    let input = create_user_input("text alice");
    let intent = classifier.classify(&input).await.unwrap();

    // Should successfully parse core fields
    assert_eq!(intent.category, IntentCategory::Message);
    assert_eq!(intent.confidence, 0.9);
    assert_eq!(intent.entities.get("contact").unwrap(), "alice");
}

#[tokio::test]
async fn test_json_with_extra_array_field() {
    let mock = MockClassifier::with_response(
        r#"{"category": "Search", "confidence": 0.88, "entities": {"query": "restaurants"}, "suggestions": ["pizza", "sushi", "burgers"]}"#
            .to_string(),
    );
    let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

    let input = create_user_input("find restaurants");
    let intent = classifier.classify(&input).await.unwrap();

    assert_eq!(intent.category, IntentCategory::Search);
    assert_eq!(intent.confidence, 0.88);
    assert_eq!(intent.entities.get("query").unwrap(), "restaurants");
}

#[tokio::test]
async fn test_json_with_unexpected_null_extra_field() {
    let mock = MockClassifier::with_response(
        r#"{"category": "Question", "confidence": 0.85, "entities": {}, "reasoning": null}"#
            .to_string(),
    );
    let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

    let input = create_user_input("what time is it");
    let intent = classifier.classify(&input).await.unwrap();

    assert_eq!(intent.category, IntentCategory::Question);
    assert_eq!(intent.confidence, 0.85);
}

#[tokio::test]
async fn test_json_extra_entities_field_format() {
    // Test case where entities has additional unexpected structure
    let mock = MockClassifier::with_response(
        r#"{"category": "Navigate", "confidence": 0.92, "entities": {"destination": "home", "extra_key": "extra_value"}, "debug_info": "test"}"#
            .to_string(),
    );
    let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

    let input = create_user_input("navigate home");
    let intent = classifier.classify(&input).await.unwrap();

    assert_eq!(intent.category, IntentCategory::Navigate);
    assert_eq!(intent.entities.get("destination").unwrap(), "home");
    // Extra entity keys are preserved in the HashMap
    assert_eq!(intent.entities.get("extra_key").unwrap(), "extra_value");
}

// ============================================================================
// Entity Value Truncation Tests (Issue #150)
// ============================================================================

#[tokio::test]
async fn test_entity_value_empty_string() {
    let mock = MockClassifier::with_response(
        r#"{"category": "Message", "confidence": 0.9, "entities": {"message": ""}}"#.to_string(),
    );
    let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

    let input = create_user_input("send message");
    let intent = classifier.classify(&input).await.unwrap();

    // Empty string should remain empty
    assert_eq!(intent.entities.get("message").unwrap(), "");
}

#[tokio::test]
async fn test_entity_value_within_limit() {
    let short_message = "This is a normal message";
    let mock = MockClassifier::with_response(format!(
        r#"{{"category": "Message", "confidence": 0.9, "entities": {{"message": "{}"}}}}"#,
        short_message
    ));
    let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

    let input = create_user_input("send message");
    let intent = classifier.classify(&input).await.unwrap();

    // Should not be truncated
    assert_eq!(intent.entities.get("message").unwrap(), short_message);
    assert!(!intent.entities.get("message").unwrap().ends_with("..."));
}

#[tokio::test]
async fn test_entity_value_exactly_at_limit() {
    // Create a string exactly 1024 characters
    let exactly_limit = "a".repeat(1024);
    let mock = MockClassifier::with_response(format!(
        r#"{{"category": "Message", "confidence": 0.9, "entities": {{"message": "{}"}}}}"#,
        exactly_limit
    ));
    let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

    let input = create_user_input("send message");
    let intent = classifier.classify(&input).await.unwrap();

    // Should not be truncated (exactly at limit)
    assert_eq!(intent.entities.get("message").unwrap().len(), 1024);
    assert!(!intent.entities.get("message").unwrap().ends_with("..."));
}

#[tokio::test]
async fn test_entity_value_exceeds_limit_truncated() {
    // Create a string that exceeds 1024 characters
    let long_message = "a".repeat(2000);
    let mock = MockClassifier::with_response(format!(
        r#"{{"category": "Message", "confidence": 0.9, "entities": {{"message": "{}"}}}}"#,
        long_message
    ));
    let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

    let input = create_user_input("send message");
    let intent = classifier.classify(&input).await.unwrap();

    let truncated = intent.entities.get("message").unwrap();

    // Should be truncated to exactly 1024 chars total (including "...")
    assert!(truncated.ends_with("..."));
    assert_eq!(truncated.len(), 1024); // Exactly 1024 chars total
}

#[tokio::test]
async fn test_entity_value_truncation_with_unicode() {
    // Test truncation with multi-byte Unicode characters
    let long_unicode = "🎉".repeat(1500); // Each emoji is 4 bytes, but counts as 1 char
    let mock = MockClassifier::with_response(format!(
        r#"{{"category": "Message", "confidence": 0.9, "entities": {{"message": "{}"}}}}"#,
        long_unicode
    ));
    let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

    let input = create_user_input("send message");
    let intent = classifier.classify(&input).await.unwrap();

    let truncated = intent.entities.get("message").unwrap();

    // Should be truncated based on character count, not byte count
    assert!(truncated.ends_with("..."));
    assert_eq!(truncated.chars().count(), 1024); // Exactly 1024 chars total
}

#[tokio::test]
async fn test_multiple_entity_values_truncation() {
    // Test that multiple entities are independently truncated
    let long_contact = "a".repeat(1500);
    let long_message = "b".repeat(1500);

    let mock = MockClassifier::with_response(format!(
        r#"{{"category": "Message", "confidence": 0.9, "entities": {{"contact": "{}", "message": "{}"}}}}"#,
        long_contact, long_message
    ));
    let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

    let input = create_user_input("send message");
    let intent = classifier.classify(&input).await.unwrap();

    let contact = intent.entities.get("contact").unwrap();
    let message = intent.entities.get("message").unwrap();

    // Both should be truncated independently to exactly 1024 chars
    assert!(contact.ends_with("..."));
    assert!(message.ends_with("..."));
    assert_eq!(contact.len(), 1024);
    assert_eq!(message.len(), 1024);
}

#[tokio::test]
async fn test_entity_truncation_preserves_other_entities() {
    let short_contact = "Alice";
    let long_message = "x".repeat(2000);

    let mock = MockClassifier::with_response(format!(
        r#"{{"category": "Message", "confidence": 0.9, "entities": {{"contact": "{}", "message": "{}"}}}}"#,
        short_contact, long_message
    ));
    let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

    let input = create_user_input("send message");
    let intent = classifier.classify(&input).await.unwrap();

    // Short entity should be unchanged
    assert_eq!(intent.entities.get("contact").unwrap(), short_contact);
    assert!(!intent.entities.get("contact").unwrap().ends_with("..."));

    // Long entity should be truncated to exactly 1024 chars
    let message = intent.entities.get("message").unwrap();
    assert!(message.ends_with("..."));
    assert_eq!(message.len(), 1024);
}
