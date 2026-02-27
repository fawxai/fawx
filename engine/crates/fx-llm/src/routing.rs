//! Smart routing rules for LLM provider selection.
//!
//! This module provides intelligent routing logic that selects between local
//! and cloud LLM providers based on configurable rules and runtime context.

use crate::router::RoutingStrategy;

/// Configuration for routing decisions.
///
/// The router evaluates rules in order and returns the strategy of the first
/// matching rule. If no rules match, the default strategy is used.
#[derive(Debug, Clone)]
pub struct RoutingConfig {
    /// Default strategy when no rules match
    pub default_strategy: RoutingStrategy,
    /// Ordered list of routing rules (first match wins)
    pub rules: Vec<RoutingRule>,
}

impl RoutingConfig {
    /// Create a new routing configuration with defaults.
    ///
    /// Default rules:
    /// - ComplexTask intents → CloudOnly (need powerful models)
    /// - Conversation intents → LocalFirst (fast, private)
    /// - Low confidence (<0.6) → CloudFirst (cloud models more reliable)
    pub fn new_with_defaults() -> Self {
        Self {
            default_strategy: RoutingStrategy::LocalFirst,
            rules: vec![
                RoutingRule {
                    condition: RoutingCondition::IntentCategory("ComplexTask".to_string()),
                    strategy: RoutingStrategy::CloudOnly,
                },
                RoutingRule {
                    condition: RoutingCondition::IntentCategory("Conversation".to_string()),
                    strategy: RoutingStrategy::LocalFirst,
                },
                RoutingRule {
                    condition: RoutingCondition::ConfidenceBelow(0.6),
                    strategy: RoutingStrategy::CloudFirst,
                },
            ],
        }
    }

    /// Create a minimal configuration with just a default strategy.
    pub fn new_simple(default: RoutingStrategy) -> Self {
        Self {
            default_strategy: default,
            rules: Vec::new(),
        }
    }
}

// Manual Default impl because we need to initialize with specific routing rules
// rather than just default field values.
impl Default for RoutingConfig {
    fn default() -> Self {
        Self::new_with_defaults()
    }
}

/// A routing rule that maps a condition to a strategy.
#[derive(Debug, Clone)]
pub struct RoutingRule {
    /// Condition that must be met for this rule to apply
    pub condition: RoutingCondition,
    /// Strategy to use if condition matches
    pub strategy: RoutingStrategy,
}

/// Conditions that trigger specific routing strategies.
#[derive(Debug, Clone, PartialEq)]
pub enum RoutingCondition {
    /// Route by intent category (e.g., "Conversation", "ComplexTask")
    IntentCategory(String),
    /// Route when confidence is below threshold (0.0-1.0)
    ConfidenceBelow(f32),
    /// Route when input length exceeds threshold (in characters)
    InputLengthAbove(usize),
    /// Always match (useful for catch-all rules)
    Always,
}

impl RoutingCondition {
    /// Check if this condition matches the given context.
    pub fn matches(&self, context: &RoutingContext) -> bool {
        match self {
            RoutingCondition::IntentCategory(category) => {
                context.intent_category.as_ref() == Some(category)
            }
            RoutingCondition::ConfidenceBelow(threshold) => context
                .confidence
                .map(|conf| conf < *threshold)
                .unwrap_or(false),
            RoutingCondition::InputLengthAbove(threshold) => context.input_length > *threshold,
            RoutingCondition::Always => true,
        }
    }
}

/// Runtime context for routing decisions.
///
/// Contains metadata about the current request that routing rules can use
/// to make intelligent decisions about provider selection.
#[derive(Debug, Clone)]
pub struct RoutingContext {
    /// Intent category (e.g., "Conversation", "ComplexTask", "Question")
    pub intent_category: Option<String>,
    /// Confidence score from intent classifier (0.0-1.0)
    pub confidence: Option<f32>,
    /// Length of input text in characters
    pub input_length: usize,
}

impl RoutingContext {
    /// Create a new routing context from prompt text.
    pub fn from_prompt(prompt: &str) -> Self {
        Self {
            intent_category: None,
            confidence: None,
            input_length: prompt.len(),
        }
    }

    /// Create a routing context with full metadata.
    pub fn new(
        intent_category: Option<String>,
        confidence: Option<f32>,
        input_length: usize,
    ) -> Self {
        Self {
            intent_category,
            confidence,
            input_length,
        }
    }

    /// Set the intent category.
    pub fn with_intent(mut self, category: impl Into<String>) -> Self {
        self.intent_category = Some(category.into());
        self
    }

    /// Set the confidence score.
    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = Some(confidence);
        self
    }
}

/// Resolve which routing strategy to use based on config and context.
///
/// Evaluates rules in order and returns the strategy of the first matching rule.
/// If no rules match, returns the default strategy.
///
/// # Arguments
/// * `config` - Routing configuration with rules and default
/// * `context` - Runtime context about the current request
///
/// # Returns
/// The selected routing strategy
///
/// # Examples
/// ```
/// use fx_llm::{RoutingConfig, RoutingContext, resolve_strategy, RoutingStrategy};
///
/// // Use default config with built-in rules
/// let config = RoutingConfig::default();
/// let context = RoutingContext::from_prompt("Hello, how are you?")
///     .with_intent("Conversation")
///     .with_confidence(0.9);
///
/// let strategy = resolve_strategy(&config, &context);
/// assert_eq!(strategy, RoutingStrategy::LocalFirst); // Conversation → LocalFirst
///
/// // Low confidence triggers CloudFirst
/// let context = RoutingContext::from_prompt("What is quantum entanglement?")
///     .with_intent("Question")
///     .with_confidence(0.5);
/// let strategy = resolve_strategy(&config, &context);
/// assert_eq!(strategy, RoutingStrategy::CloudFirst); // Low confidence → CloudFirst
/// ```
pub fn resolve_strategy(config: &RoutingConfig, context: &RoutingContext) -> RoutingStrategy {
    for rule in &config.rules {
        if rule.condition.matches(context) {
            return rule.strategy;
        }
    }
    config.default_strategy
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_has_rules() {
        let config = RoutingConfig::default();
        assert_eq!(config.default_strategy, RoutingStrategy::LocalFirst);
        assert!(!config.rules.is_empty());
    }

    #[test]
    fn test_simple_config() {
        let config = RoutingConfig::new_simple(RoutingStrategy::CloudOnly);
        assert_eq!(config.default_strategy, RoutingStrategy::CloudOnly);
        assert!(config.rules.is_empty());
    }

    #[test]
    fn test_resolve_strategy_default_when_no_rules() {
        let config = RoutingConfig::new_simple(RoutingStrategy::LocalOnly);
        let context = RoutingContext::from_prompt("test");

        let strategy = resolve_strategy(&config, &context);
        assert_eq!(strategy, RoutingStrategy::LocalOnly);
    }

    #[test]
    fn test_intent_category_condition_matches() {
        let condition = RoutingCondition::IntentCategory("Conversation".to_string());
        let context = RoutingContext::new(Some("Conversation".to_string()), None, 100);

        assert!(condition.matches(&context));
    }

    #[test]
    fn test_intent_category_condition_no_match() {
        let condition = RoutingCondition::IntentCategory("Conversation".to_string());
        let context = RoutingContext::new(Some("ComplexTask".to_string()), None, 100);

        assert!(!condition.matches(&context));
    }

    #[test]
    fn test_intent_category_condition_none() {
        let condition = RoutingCondition::IntentCategory("Conversation".to_string());
        let context = RoutingContext::new(None, None, 100);

        assert!(!condition.matches(&context));
    }

    #[test]
    fn test_confidence_below_triggers_cloud() {
        let condition = RoutingCondition::ConfidenceBelow(0.6);
        let context = RoutingContext::new(None, Some(0.5), 100);

        assert!(condition.matches(&context));
    }

    #[test]
    fn test_confidence_above_no_match() {
        let condition = RoutingCondition::ConfidenceBelow(0.6);
        let context = RoutingContext::new(None, Some(0.8), 100);

        assert!(!condition.matches(&context));
    }

    #[test]
    fn test_confidence_none_no_match() {
        let condition = RoutingCondition::ConfidenceBelow(0.6);
        let context = RoutingContext::new(None, None, 100);

        assert!(!condition.matches(&context));
    }

    #[test]
    fn test_input_length_above_triggers_cloud() {
        let condition = RoutingCondition::InputLengthAbove(1000);
        let context = RoutingContext::new(None, None, 1500);

        assert!(condition.matches(&context));
    }

    #[test]
    fn test_input_length_below_no_match() {
        let condition = RoutingCondition::InputLengthAbove(1000);
        let context = RoutingContext::new(None, None, 500);

        assert!(!condition.matches(&context));
    }

    #[test]
    fn test_always_condition() {
        let condition = RoutingCondition::Always;
        let context = RoutingContext::new(None, None, 100);

        assert!(condition.matches(&context));
    }

    #[test]
    fn test_first_matching_rule_wins() {
        let config = RoutingConfig {
            default_strategy: RoutingStrategy::LocalFirst,
            rules: vec![
                RoutingRule {
                    condition: RoutingCondition::IntentCategory("Conversation".to_string()),
                    strategy: RoutingStrategy::LocalOnly,
                },
                RoutingRule {
                    condition: RoutingCondition::Always,
                    strategy: RoutingStrategy::CloudOnly,
                },
            ],
        };

        let context = RoutingContext::new(Some("Conversation".to_string()), None, 100);
        let strategy = resolve_strategy(&config, &context);

        // First rule matches, should use LocalOnly
        assert_eq!(strategy, RoutingStrategy::LocalOnly);
    }

    #[test]
    fn test_multiple_conditions_evaluated_correctly() {
        let config = RoutingConfig {
            default_strategy: RoutingStrategy::LocalFirst,
            rules: vec![
                RoutingRule {
                    condition: RoutingCondition::IntentCategory("ComplexTask".to_string()),
                    strategy: RoutingStrategy::CloudOnly,
                },
                RoutingRule {
                    condition: RoutingCondition::ConfidenceBelow(0.6),
                    strategy: RoutingStrategy::CloudFirst,
                },
                RoutingRule {
                    condition: RoutingCondition::InputLengthAbove(2000),
                    strategy: RoutingStrategy::CloudFirst,
                },
            ],
        };

        // Test intent category match
        let context1 = RoutingContext::new(Some("ComplexTask".to_string()), Some(0.9), 100);
        assert_eq!(
            resolve_strategy(&config, &context1),
            RoutingStrategy::CloudOnly
        );

        // Test low confidence match
        let context2 = RoutingContext::new(Some("Question".to_string()), Some(0.5), 100);
        assert_eq!(
            resolve_strategy(&config, &context2),
            RoutingStrategy::CloudFirst
        );

        // Test long input match
        let context3 = RoutingContext::new(None, Some(0.9), 2500);
        assert_eq!(
            resolve_strategy(&config, &context3),
            RoutingStrategy::CloudFirst
        );

        // Test no match, use default
        let context4 = RoutingContext::new(Some("SimpleTask".to_string()), Some(0.9), 100);
        assert_eq!(
            resolve_strategy(&config, &context4),
            RoutingStrategy::LocalFirst
        );
    }

    #[test]
    fn test_routing_context_builder() {
        let context = RoutingContext::from_prompt("Hello world")
            .with_intent("Conversation")
            .with_confidence(0.85);

        assert_eq!(context.intent_category, Some("Conversation".to_string()));
        assert_eq!(context.confidence, Some(0.85));
        assert_eq!(context.input_length, 11); // "Hello world" length
    }
}
