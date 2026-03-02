//! Internal message types for inter-crate communication.
//!
//! These types are used for communication between different Fawx components.

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

    /// A sub-goal has started execution within a decomposition plan.
    SubGoalStarted {
        /// Zero-based index within the plan.
        index: usize,
        /// Total sub-goals in the plan.
        total: usize,
        /// Sub-goal description.
        description: String,
    },

    /// A sub-goal has finished execution.
    SubGoalCompleted {
        /// Zero-based index within the plan.
        index: usize,
        /// Total sub-goals in the plan.
        total: usize,
        /// Whether the sub-goal succeeded.
        success: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sub_goal_started_roundtrip_serde() {
        let msg = InternalMessage::SubGoalStarted {
            index: 0,
            total: 3,
            description: "Summarize findings".to_string(),
        };
        let json = serde_json::to_string(&msg).expect("serialize");
        let decoded: InternalMessage = serde_json::from_str(&json).expect("deserialize");
        assert!(matches!(
            decoded,
            InternalMessage::SubGoalStarted {
                index: 0,
                total: 3,
                ..
            }
        ));
    }

    #[test]
    fn sub_goal_completed_roundtrip_serde() {
        let msg = InternalMessage::SubGoalCompleted {
            index: 1,
            total: 2,
            success: true,
        };
        let json = serde_json::to_string(&msg).expect("serialize");
        let decoded: InternalMessage = serde_json::from_str(&json).expect("deserialize");
        assert!(matches!(
            decoded,
            InternalMessage::SubGoalCompleted {
                index: 1,
                total: 2,
                success: true
            }
        ));
    }
}
