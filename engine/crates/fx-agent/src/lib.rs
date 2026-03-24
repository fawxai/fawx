//! Agent reasoning loop and orchestration.
//!
//! This crate implements the core agent logic: perception → cognition → action.
//! It orchestrates intent classification, action planning, and execution.

pub mod auth;
pub mod claude;
pub mod history;
pub mod intent;
pub mod plan_builder;
pub mod retry;
pub mod skill_tools;
pub mod tools;

use fx_core::types::{ActionPlan, UserInput};

pub use auth::{AuthBackend, AuthCredentials, AuthRouter, AuthStrategy};
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
pub use skill_tools::{format_skills_context, skill_tools};
pub use tools::fawx_action_tools;

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
    pub async fn process(&self, _input: UserInput) -> fx_core::error::Result<ActionPlan> {
        // Placeholder implementation
        todo!("Agent processing will be implemented in Epic 4")
    }
}

impl Default for Agent {
    fn default() -> Self {
        Self::new()
    }
}
