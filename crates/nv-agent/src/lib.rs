//! Agent reasoning loop and orchestration.
//!
//! This crate implements the core agent logic: perception → cognition → action.
//! It orchestrates intent classification, action planning, and execution.

use nv_core::types::{ActionPlan, UserInput};

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
