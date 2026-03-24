//! Intent classification using LLM.

use super::metrics::IntentMetrics;
use super::parser::parse_intent_response;
use super::prompts::INTENT_SYSTEM_PROMPT;
use crate::claude::{AgentError, ClaudeClient, Message, Result};
use async_trait::async_trait;
use fx_core::types::{Intent, IntentCategory, UserInput};
use std::time::{Duration, Instant};
use tokio::time::timeout;
use tracing::{debug, info, warn};

#[cfg(test)]
use std::sync::Arc;
#[cfg(test)]
use tokio::sync::Mutex;

/// Fallback confidence score used when classification fails or times out.
const FALLBACK_CONFIDENCE: f32 = 0.3;

/// Configuration for the intent classifier.
///
/// Note: Token limits and model selection are controlled by the ClaudeClient configuration,
/// not by this classifier config.
#[derive(Debug, Clone)]
pub struct ClassifierConfig {
    /// Confidence threshold for accepting classifications (default: 0.7)
    /// Values >= threshold are accepted; values < threshold fall back to Conversation
    pub confidence_threshold: f32,
}

impl Default for ClassifierConfig {
    fn default() -> Self {
        Self {
            confidence_threshold: 0.7,
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
    metrics: IntentMetrics,
}

impl<C: LlmClassifier> IntentClassifier<C> {
    /// Create a new intent classifier.
    ///
    /// # Arguments
    /// * `config` - Classifier configuration
    /// * `llm` - LLM client implementing LlmClassifier trait
    pub fn new(config: ClassifierConfig, llm: C) -> Self {
        Self {
            config,
            llm,
            metrics: IntentMetrics::new(),
        }
    }

    /// Get a reference to the metrics for this classifier.
    ///
    /// Returns a clone of the metrics handle, which can be used to
    /// query metrics from other threads or components.
    pub fn metrics(&self) -> IntentMetrics {
        self.metrics.clone()
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

    /// Classify user input with a timeout for production robustness.
    ///
    /// Enforces a maximum time for classification to prevent hanging on slow LLM responses.
    /// On timeout, returns a Conversation intent with low confidence.
    ///
    /// # Arguments
    /// * `input` - User input to classify
    /// * `timeout_duration` - Maximum time to wait for classification
    ///
    /// # Returns
    /// Classified Intent, or a fallback Conversation intent on timeout
    pub async fn classify_with_timeout(
        &self,
        input: &UserInput,
        timeout_duration: Duration,
    ) -> Result<Intent> {
        let start_time = Instant::now();

        match timeout(timeout_duration, self.classify(input)).await {
            Ok(result) => result,
            Err(_) => {
                warn!(
                    "Classification timed out after {:?}, falling back to Conversation",
                    timeout_duration
                );

                // Record timeout fallback
                let latency = start_time.elapsed();
                self.metrics
                    .record_classification(FALLBACK_CONFIDENCE, latency, true);

                Ok(Intent {
                    category: IntentCategory::Conversation,
                    confidence: FALLBACK_CONFIDENCE,
                    entities: Default::default(),
                    raw_input: input.text.clone(),
                })
            }
        }
    }

    /// Internal method to classify messages and apply threshold logic.
    async fn classify_messages(&self, messages: &[Message], raw_input: &str) -> Result<Intent> {
        let start_time = Instant::now();

        // Call LLM
        let raw_response = self.llm.classify_raw(messages).await?;

        // Parse response
        let mut intent = parse_intent_response(&raw_response, raw_input)?;

        info!(
            "Raw classification: {:?} (confidence: {})",
            intent.category, intent.confidence
        );

        // Apply confidence threshold
        let was_fallback = if intent.confidence < self.config.confidence_threshold {
            warn!(
                "Confidence {} below threshold {}, overriding to Conversation",
                intent.confidence, self.config.confidence_threshold
            );
            intent.category = IntentCategory::Conversation;
            true
        } else {
            false
        };

        info!("Final classification: {:?}", intent.category);

        // Record metrics
        let latency = start_time.elapsed();
        self.metrics
            .record_classification(intent.confidence, latency, was_fallback);

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

    fn create_user_input(text: &str) -> UserInput {
        UserInput {
            text: text.to_string(),
            source: fx_core::types::InputSource::Text,
            timestamp: 1234567890,
            context_id: None,
            images: Vec::new(),
            documents: Vec::new(),
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
        assert_eq!(config.confidence_threshold, 0.7);
    }

    #[tokio::test]
    async fn test_classify_with_timeout_success() {
        let mock = MockClassifier::with_response(
            r#"{"category": "LaunchApp", "confidence": 0.95, "entities": {"app_name": "spotify"}}"#
                .to_string(),
        );
        let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

        let input = create_user_input("open spotify");
        let intent = classifier
            .classify_with_timeout(&input, Duration::from_secs(30))
            .await
            .unwrap();

        assert_eq!(intent.category, IntentCategory::LaunchApp);
        assert_eq!(intent.confidence, 0.95);
    }

    #[tokio::test]
    async fn test_classify_with_timeout_expires() {
        // Create a mock that will simulate a slow response
        struct SlowMockClassifier;

        #[async_trait]
        impl LlmClassifier for SlowMockClassifier {
            async fn classify_raw(&self, _messages: &[Message]) -> Result<String> {
                // Sleep longer than the timeout
                tokio::time::sleep(Duration::from_millis(200)).await;
                Ok(r#"{"category": "Search", "confidence": 0.9, "entities": {}}"#.to_string())
            }
        }

        let classifier = IntentClassifier::new(ClassifierConfig::default(), SlowMockClassifier);

        let input = create_user_input("search something");
        let intent = classifier
            .classify_with_timeout(&input, Duration::from_millis(50))
            .await
            .unwrap();

        // Should fallback to Conversation with low confidence on timeout
        assert_eq!(intent.category, IntentCategory::Conversation);
        assert_eq!(intent.confidence, 0.3);
        assert_eq!(intent.raw_input, "search something");
    }

    #[tokio::test]
    async fn test_classify_with_timeout_different_durations() {
        let mock = MockClassifier::with_response(
            r#"{"category": "Message", "confidence": 0.85, "entities": {}}"#.to_string(),
        );
        let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

        let input = create_user_input("text someone");

        // Test with very long timeout (should succeed)
        let intent = classifier
            .classify_with_timeout(&input, Duration::from_secs(60))
            .await
            .unwrap();

        assert_eq!(intent.category, IntentCategory::Message);
        assert_eq!(intent.confidence, 0.85);
    }

    #[tokio::test]
    async fn test_metrics_tracking_basic() {
        let mock = MockClassifier::with_response(
            r#"{"category": "LaunchApp", "confidence": 0.95, "entities": {"app_name": "spotify"}}"#
                .to_string(),
        );
        let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

        let input = create_user_input("open spotify");
        let _intent = classifier.classify(&input).await.unwrap();

        let snapshot = classifier.metrics().get_snapshot();
        assert_eq!(snapshot.total_classifications, 1);
        assert!((snapshot.average_confidence - 0.95).abs() < 0.01);
        assert_eq!(snapshot.fallback_count, 0);
    }

    #[tokio::test]
    async fn test_metrics_tracking_fallback() {
        let mock = MockClassifier::with_response(
            r#"{"category": "Question", "confidence": 0.6, "entities": {}}"#.to_string(),
        );

        let config = ClassifierConfig {
            confidence_threshold: 0.7,
        };
        let classifier = IntentClassifier::new(config, mock);

        let input = create_user_input("maybe a question?");
        let _intent = classifier.classify(&input).await.unwrap();

        let snapshot = classifier.metrics().get_snapshot();
        assert_eq!(snapshot.total_classifications, 1);
        assert!((snapshot.average_confidence - 0.6).abs() < 0.01);
        assert_eq!(snapshot.fallback_count, 1); // Below threshold
    }

    #[tokio::test]
    async fn test_metrics_tracking_multiple() {
        let mock = MockClassifier::new(vec![
            r#"{"category": "LaunchApp", "confidence": 0.9, "entities": {}}"#.to_string(),
            r#"{"category": "Message", "confidence": 0.8, "entities": {}}"#.to_string(),
            r#"{"category": "Search", "confidence": 0.6, "entities": {}}"#.to_string(),
        ]);

        let config = ClassifierConfig {
            confidence_threshold: 0.7,
        };
        let classifier = IntentClassifier::new(config, mock);

        let _intent1 = classifier
            .classify(&create_user_input("test1"))
            .await
            .unwrap();
        let _intent2 = classifier
            .classify(&create_user_input("test2"))
            .await
            .unwrap();
        let _intent3 = classifier
            .classify(&create_user_input("test3"))
            .await
            .unwrap();

        let snapshot = classifier.metrics().get_snapshot();
        assert_eq!(snapshot.total_classifications, 3);

        // Average: (0.9 + 0.8 + 0.6) / 3 = 0.766...
        assert!((snapshot.average_confidence - 0.7666).abs() < 0.01);

        // Only the third one (0.6) should be a fallback
        assert_eq!(snapshot.fallback_count, 1);
    }

    #[tokio::test]
    async fn test_metrics_tracking_timeout() {
        struct SlowMockClassifier;

        #[async_trait]
        impl LlmClassifier for SlowMockClassifier {
            async fn classify_raw(&self, _messages: &[Message]) -> Result<String> {
                tokio::time::sleep(Duration::from_millis(200)).await;
                Ok(r#"{"category": "Search", "confidence": 0.9, "entities": {}}"#.to_string())
            }
        }

        let classifier = IntentClassifier::new(ClassifierConfig::default(), SlowMockClassifier);

        let input = create_user_input("search something");
        let _intent = classifier
            .classify_with_timeout(&input, Duration::from_millis(50))
            .await
            .unwrap();

        let snapshot = classifier.metrics().get_snapshot();
        assert_eq!(snapshot.total_classifications, 1);
        assert!((snapshot.average_confidence - 0.3).abs() < 0.01); // Timeout fallback
        assert_eq!(snapshot.fallback_count, 1); // Timeout counts as fallback
    }

    #[tokio::test]
    async fn test_metrics_latency_tracking() {
        let mock = MockClassifier::with_response(
            r#"{"category": "LaunchApp", "confidence": 0.9, "entities": {}}"#.to_string(),
        );
        let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

        let input = create_user_input("open app");
        let _intent = classifier.classify(&input).await.unwrap();

        let snapshot = classifier.metrics().get_snapshot();
        assert_eq!(snapshot.total_classifications, 1);

        // Latency should be very small for mock classifier (< 10ms)
        assert!(snapshot.average_latency < Duration::from_millis(10));
    }

    #[tokio::test]
    async fn test_metrics_can_be_cloned() {
        let mock = MockClassifier::with_response(
            r#"{"category": "LaunchApp", "confidence": 0.9, "entities": {}}"#.to_string(),
        );
        let classifier = IntentClassifier::new(ClassifierConfig::default(), mock);

        let metrics1 = classifier.metrics();
        let metrics2 = classifier.metrics();

        let input = create_user_input("test");
        let _intent = classifier.classify(&input).await.unwrap();

        // Both clones should see the same metrics
        let snapshot1 = metrics1.get_snapshot();
        let snapshot2 = metrics2.get_snapshot();

        assert_eq!(snapshot1.total_classifications, 1);
        assert_eq!(snapshot2.total_classifications, 1);
        assert_eq!(snapshot1.average_confidence, snapshot2.average_confidence);
    }
}
