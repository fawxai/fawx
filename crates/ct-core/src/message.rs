//! Internal message types for inter-crate communication.
//!
//! These types are used for communication between different Citros components.

use serde::{Deserialize, Serialize};

/// Internal message sent between crates via the event bus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InternalMessage {
    /// Agent has classified user input
    IntentClassified {
        /// The classified intent
        intent: String,
        /// Classification confidence (0.0 - 1.0)
        confidence: f32,
    },

    /// Action plan generated
    PlanGenerated {
        /// Plan ID for tracking
        plan_id: String,
        /// Number of steps in the plan
        step_count: usize,
    },

    /// Action execution started
    ActionStarted {
        /// Action ID
        action_id: String,
        /// Action description
        description: String,
    },

    /// Action execution completed
    ActionCompleted {
        /// Action ID
        action_id: String,
        /// Whether action succeeded
        success: bool,
    },

    /// System status update
    SystemStatus {
        /// Status message
        message: String,
    },
}
