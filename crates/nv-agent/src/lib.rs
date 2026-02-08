//! Agent reasoning loop and orchestration.
//!
//! This crate implements the core agent logic: perception → cognition → action.
//! It orchestrates intent classification, action planning, and execution.

pub mod claude;
pub mod history;
pub mod intent;
pub mod plan_builder;
pub mod retry;
pub mod tools;

use nv_core::types::{ActionPlan, UserInput};

pub use claude::{
    AgentError, ClaudeClient, ClaudeConfig, CompletionResponse, ContentBlock, Message, Result,
    Role, StopReason, StreamEvent, Tool, ToolResult, ToolUse, Usage,
};
pub use history::ConversationHistory;
pub use intent::{
    category_from_str, parse_intent_response, ClassifierConfig, IntentClassifier, LlmClassifier,
    INTENT_SYSTEM_PROMPT,
};
pub use plan_builder::PlanBuilder;
pub use retry::{calculate_delay, should_retry, with_retry, RetryPolicy};
pub use tools::nova_action_tools;

/// Main agent orchestrator.
///
/// Coordinates the perception → cognition → action loop.
pub struct Agent {
    // Placeholder - will be populated in later epics
}

impl Agent {
    /// Create a new agent instance.
    pub fn new() -> Self {
        Self {}
    }

    /// Process user input and generate a response.
    pub async fn process(&self, _input: UserInput) -> nv_core::error::Result<ActionPlan> {
        // Placeholder implementation
        todo!("Agent processing will be implemented in Epic 4")
    }
}

impl Default for Agent {
    fn default() -> Self {
        Self::new()
    }
}
