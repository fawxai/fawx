//! Internal message types for inter-crate communication.
//!
//! These types are used for communication between different Fawx components.

use serde::{Deserialize, Serialize};

/// Which LLM phase is streaming.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StreamPhase {
    Reason,
    Synthesize,
}

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

    /// Streaming started for an LLM phase.
    StreamingStarted {
        /// Which phase is currently streaming.
        phase: StreamPhase,
    },

    /// Streaming text delta for an LLM phase.
    StreamDelta {
        /// Incremental text emitted by the model.
        delta: String,
        /// Which phase emitted the delta.
        phase: StreamPhase,
    },

    /// Streaming finished for an LLM phase.
    StreamingFinished {
        /// Which phase finished streaming.
        phase: StreamPhase,
    },

    /// A tool call is about to be executed.
    ToolUse {
        /// Tool call identifier.
        call_id: String,
        /// Tool/function name.
        name: String,
        /// Structured arguments.
        arguments: serde_json::Value,
    },

    /// A tool execution result is available.
    ToolResult {
        /// Tool call identifier.
        call_id: String,
        /// Tool/function name.
        name: String,
        /// Whether the tool call succeeded.
        success: bool,
        /// Human-readable output.
        content: String,
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

    #[test]
    fn streaming_started_roundtrip_serde() {
        let msg = InternalMessage::StreamingStarted {
            phase: StreamPhase::Reason,
        };
        let json = serde_json::to_string(&msg).expect("serialize");
        let decoded: InternalMessage = serde_json::from_str(&json).expect("deserialize");
        assert!(matches!(
            decoded,
            InternalMessage::StreamingStarted {
                phase: StreamPhase::Reason
            }
        ));
    }

    #[test]
    fn stream_delta_roundtrip_serde() {
        let msg = InternalMessage::StreamDelta {
            delta: "delta".to_string(),
            phase: StreamPhase::Synthesize,
        };
        let json = serde_json::to_string(&msg).expect("serialize");
        let decoded: InternalMessage = serde_json::from_str(&json).expect("deserialize");
        assert!(matches!(
            decoded,
            InternalMessage::StreamDelta {
                delta,
                phase: StreamPhase::Synthesize
            } if delta == "delta"
        ));
    }

    #[test]
    fn streaming_finished_roundtrip_serde() {
        let msg = InternalMessage::StreamingFinished {
            phase: StreamPhase::Reason,
        };
        let json = serde_json::to_string(&msg).expect("serialize");
        let decoded: InternalMessage = serde_json::from_str(&json).expect("deserialize");
        assert!(matches!(
            decoded,
            InternalMessage::StreamingFinished {
                phase: StreamPhase::Reason
            }
        ));
    }

    #[test]
    fn tool_use_roundtrip_serde() {
        let msg = InternalMessage::ToolUse {
            call_id: "call-1".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": "src/main.rs"}),
        };
        let json = serde_json::to_string(&msg).expect("serialize");
        let decoded: InternalMessage = serde_json::from_str(&json).expect("deserialize");
        assert!(matches!(
            decoded,
            InternalMessage::ToolUse {
                call_id,
                name,
                arguments
            } if call_id == "call-1"
                && name == "read_file"
                && arguments == serde_json::json!({"path": "src/main.rs"})
        ));
    }

    #[test]
    fn tool_result_roundtrip_serde() {
        let msg = InternalMessage::ToolResult {
            call_id: "call-1".to_string(),
            name: "read_file".to_string(),
            success: false,
            content: "fn main() {}".to_string(),
        };
        let json = serde_json::to_string(&msg).expect("serialize");
        let decoded: InternalMessage = serde_json::from_str(&json).expect("deserialize");
        assert!(matches!(
            decoded,
            InternalMessage::ToolResult {
                call_id,
                name,
                success,
                content
            } if call_id == "call-1"
                && name == "read_file"
                && !success
                && content == "fn main() {}"
        ));
    }
}
