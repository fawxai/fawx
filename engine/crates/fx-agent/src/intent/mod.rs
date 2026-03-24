//! Intent classification module.
//!
//! Classifies user input into intent categories using an LLM.
//!
//! # Overview
//!
//! This module provides intent classification for the Fawx agent. It uses Claude
//! to analyze user input and classify it into one of 9 intent categories:
//!
//! - **LaunchApp**: Launch an application
//! - **Search**: Search for information
//! - **Navigate**: Navigate to a location
//! - **Message**: Send a message
//! - **Calendar**: Calendar/scheduling actions
//! - **Settings**: Modify device settings
//! - **Question**: Answer a question
//! - **ComplexTask**: Multi-step complex task
//! - **Conversation**: Conversational input
//!
//! # Example
//!
//! ```no_run
//! use fx_agent::intent::{IntentClassifier, ClassifierConfig, LlmClassifier};
//! use fx_agent::{ClaudeClient, ClaudeConfig};
//! use fx_core::types::{UserInput, InputSource};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create Claude client (implements LlmClassifier)
//! let api_key = std::env::var("ANTHROPIC_API_KEY")?;
//! let config = ClaudeConfig::new(api_key)?;
//! let claude_client = ClaudeClient::new(config)?;
//!
//! // Create classifier
//! let classifier = IntentClassifier::new(
//!     ClassifierConfig::default(),
//!     claude_client,
//! );
//!
//! // Classify user input
//! let input = UserInput {
//!     text: "open spotify".to_string(),
//!     source: InputSource::Voice,
//!     timestamp: 1234567890,
//!     context_id: None,
//!     images: Vec::new(),
//! };
//!
//! let intent = classifier.classify(&input).await?;
//! println!("Category: {:?}, Confidence: {}", intent.category, intent.confidence);
//! # Ok(())
//! # }
//! ```

pub mod classifier;
pub mod metrics;
pub mod parser;
pub mod prompts;

#[cfg(test)]
mod tests;

// Re-export key types
pub use classifier::{ClassifierConfig, IntentClassifier, LlmClassifier};
pub use metrics::{IntentMetrics, MetricsSnapshot};
pub use parser::{category_from_str, parse_intent_response};
pub use prompts::INTENT_SYSTEM_PROMPT;

// Re-export MockClassifier for testing
#[cfg(test)]
pub use classifier::MockClassifier;
