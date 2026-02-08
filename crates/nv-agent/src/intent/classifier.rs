//! Intent classification using LLM.

use super::parser::parse_intent_response;
use super::prompts::INTENT_SYSTEM_PROMPT;
use crate::claude::{AgentError, ClaudeClient, Message, Result};
use async_trait::async_trait;
use nv_core::types::{Intent, IntentCategory, UserInput};
use tracing::{debug, info, warn};

#[cfg(test)]
use std::sync::Arc;
#[cfg(test)]
use tokio::sync::Mutex;

/// Configuration for the intent classifier.
#[derive(Debug, Clone)]
pub struct ClassifierConfig {
    /// Model to use for classification (default: "claude-sonnet-4-5")
    pub model: String,

    /// Confidence threshold for accepting classifications (default: 0.7)
    /// If confidence < threshold, override to Conversation category
    pub confidence_threshold: f32,

    /// Maximum tokens in classification response (default: 256)
    pub max_tokens: u32,
}

impl Default for ClassifierConfig {
    fn default() -> Self {
        Self {
            model: "claude-sonnet-4-5".to_string(),
            confidence_threshold: 0.7,
            max_tokens: 256,
        }
    }
}

/// Trait for LLM-based intent classification.
///
/// Abstraction to allow mocking in tests while using real ClaudeClient in production.
#[async_trait]
pub trait LlmClassifier: Send + Sync {
    /// Classify raw messages and return JSON response string.
    ///
    /// # Arguments
    /// * `messages` - Conversation messages including system prompt and user input
    ///
    /// # Returns
    /// Raw JSON string from the LLM
    async fn classify_raw(&self, messages: &[Message]) -> Result<String>;
}

/// Implementation of LlmClassifier for ClaudeClient.
#[async_trait]
impl LlmClassifier for ClaudeClient {
    async fn classify_raw(&self, messages: &[Message]) -> Result<String> {
        let response = self.complete(messages, None).await?;

        // Extract text from content blocks
        let text = response
            .content
            .iter()
            .filter_map(|block| {
                if let crate::claude::ContentBlock::Text { text } = block {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(" ");

        if text.is_empty() {
            return Err(AgentError::InvalidResponse(
                "No text content in response".to_string(),
            ));
        }

        Ok(text)
    }
}

/// Intent classifier that uses an LLM to classify user input.
pub struct IntentClassifier<C: LlmClassifier> {
    config: ClassifierConfig,
    llm: C,
}

impl<C: LlmClassifier> IntentClassifier<C> {
    /// Create a new intent classifier.
    ///
    /// # Arguments
    /// * `config` - Classifier configuration
    /// * `llm` - LLM client implementing LlmClassifier trait
    pub fn new(config: ClassifierConfig, llm: C) -> Self {
        Self { config, llm }
    }

    /// Classify user input into an intent.
    ///
    /// # Arguments
    /// * `input` - User input to classify
    ///
    /// # Returns
    /// Classified Intent with category, confidence, and entities
    pub async fn classify(&self, input: &UserInput) -> Result<Intent> {
        debug!("Classifying input: {}", input.text);

        let messages = vec![
            Message::system(INTENT_SYSTEM_PROMPT),
            Message::user(&input.text),
        ];

        self.classify_messages(&messages, &input.text).await
    }

    /// Classify user input with conversation history for context.
    ///
    /// # Arguments
    /// * `input` - User input to classify
    /// * `history` - Previous conversation messages for context
    ///
    /// # Returns
    /// Classified Intent with category, confidence, and entities
    pub async fn classify_with_context(
        &self,
        input: &UserInput,
        history: &[Message],
    ) -> Result<Intent> {
        debug!(
            "Classifying input with {} history messages: {}",
            history.len(),
            input.text
        );

        let mut messages = vec![Message::system(INTENT_SYSTEM_PROMPT)];
        messages.extend_from_slice(history);
        messages.push(Message::user(&input.text));

        self.classify_messages(&messages, &input.text).await
    }

    /// Internal method to classify messages and apply threshold logic.
    async fn classify_messages(&self, messages: &[Message], raw_input: &str) -> Result<Intent> {
        // Call LLM
        let raw_response = self.llm.classify_raw(messages).await?;

        // Parse response
        let mut intent = parse_intent_response(&raw_response, raw_input)?;

        info!(
            "Raw classification: {:?} (confidence: {})",
            intent.category, intent.confidence
        );

        // Apply confidence threshold
        if intent.confidence < self.config.confidence_threshold {
            warn!(
                "Confidence {} below threshold {}, overriding to Conversation",
                intent.confidence, self.config.confidence_threshold
            );
            intent.category = IntentCategory::Conversation;
        }

        info!("Final classification: {:?}", intent.category);

        Ok(intent)
    }
}

/// Mock classifier for testing that returns predefined responses.
#[cfg(test)]
pub struct MockClassifier {
    responses: Arc<Mutex<Vec<String>>>,
}

#[cfg(test)]
impl MockClassifier {
    /// Create a new mock classifier with predefined responses.
    ///
    /// Responses are consumed in order (FIFO).
    pub fn new(responses: Vec<String>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(responses)),
        }
    }

    /// Create a mock that returns a single response repeatedly.
    pub fn with_response(response: String) -> Self {
        Self::new(vec![response])
    }
}

#[cfg(test)]
#[async_trait]
impl LlmClassifier for MockClassifier {
    async fn classify_raw(&self, _messages: &[Message]) -> Result<String> {
        let mut responses = self.responses.lock().await;
        if responses.is_empty() {
            // If we run out of responses, return the last one again
            return Err(AgentError::InvalidResponse(
                "No more mock responses".to_string(),
            ));
        }
        if responses.len() == 1 {
            // Return the same response repeatedly
            Ok(responses[0].clone())
        } else {
            // Return and remove first response
            Ok(responses.remove(0))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    fn create_user_input(text: &str) -> UserInput {
        UserInput {
            text: text.to_string(),
            source: nv_core::types::InputSource::Text,
            timestamp: 1234567890,
            context_id: None,
        }
    }

    #[tokio::test]
    async fn test_classify_launch_app() {
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
    }

    #[tokio::test]
    async fn test_classify_message() {
        let mock = MockClassifier::with_response(
            r#"{"category": "Message", "confidence": 0.9, "entities": {"contact": "mom", "message": "happy birthday"}}"#
                .to_string(),
        );
        let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

        let input = create_user_input("text mom happy birthday");
        let intent = classifier.classify(&input).await.unwrap();

        assert_eq!(intent.category, IntentCategory::Message);
        assert_eq!(intent.confidence, 0.9);
        assert_eq!(intent.entities.get("contact").unwrap(), "mom");
    }

    #[tokio::test]
    async fn test_confidence_threshold_override() {
        let mock = MockClassifier::with_response(
            r#"{"category": "Question", "confidence": 0.6, "entities": {}}"#.to_string(),
        );

        let config = ClassifierConfig {
            confidence_threshold: 0.7,
            ..Default::default()
        };
        let classifier = IntentClassifier::new(config, mock);

        let input = create_user_input("maybe a question?");
        let intent = classifier.classify(&input).await.unwrap();

        // Should be overridden to Conversation
        assert_eq!(intent.category, IntentCategory::Conversation);
        assert_eq!(intent.confidence, 0.6); // Confidence preserved
    }

    #[tokio::test]
    async fn test_confidence_exactly_at_threshold() {
        let mock = MockClassifier::with_response(
            r#"{"category": "Search", "confidence": 0.7, "entities": {}}"#.to_string(),
        );

        let config = ClassifierConfig {
            confidence_threshold: 0.7,
            ..Default::default()
        };
        let classifier = IntentClassifier::new(config, mock);

        let input = create_user_input("search for something");
        let intent = classifier.classify(&input).await.unwrap();

        // Should NOT be overridden (>= threshold)
        assert_eq!(intent.category, IntentCategory::Search);
    }

    #[tokio::test]
    async fn test_classify_with_context() {
        let mock = MockClassifier::with_response(
            r#"{"category": "Conversation", "confidence": 0.85, "entities": {}}"#.to_string(),
        );
        let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

        let history = vec![
            Message::user("hello"),
            Message::assistant("Hi! How can I help you?"),
        ];

        let input = create_user_input("thanks");
        let intent = classifier
            .classify_with_context(&input, &history)
            .await
            .unwrap();

        assert_eq!(intent.category, IntentCategory::Conversation);
    }

    #[tokio::test]
    async fn test_malformed_json_fallback() {
        let mock = MockClassifier::with_response("not valid json".to_string());
        let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

        let input = create_user_input("some input");
        let intent = classifier.classify(&input).await.unwrap();

        // Should fallback to Conversation
        assert_eq!(intent.category, IntentCategory::Conversation);
        assert_eq!(intent.confidence, 0.3);
    }

    #[tokio::test]
    async fn test_config_defaults() {
        let config = ClassifierConfig::default();
        assert_eq!(config.model, "claude-sonnet-4-5");
        assert_eq!(config.confidence_threshold, 0.7);
        assert_eq!(config.max_tokens, 256);
    }
}
