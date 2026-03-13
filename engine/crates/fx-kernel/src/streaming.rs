use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Categories of user-visible errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCategory {
    /// LLM provider error (rate limit, auth, timeout).
    Provider,
    /// Tool execution failed and was not retried.
    ToolExecution,
    /// Channel/surface error (send failure, parse error).
    Channel,
    /// Compaction or memory operation failed.
    Memory,
    /// Configuration or system error.
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Perceive,
    Reason,
    Act,
    Synthesize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StreamEvent {
    TextDelta {
        text: String,
    },
    ToolCallStart {
        id: String,
        name: String,
    },
    ToolCallComplete {
        id: String,
        name: String,
        arguments: String,
    },
    ToolResult {
        id: String,
        output: String,
        is_error: bool,
    },
    PhaseChange {
        phase: Phase,
    },
    Done {
        response: String,
    },
    /// A user-visible error occurred during execution.
    Error {
        /// Machine-readable error category.
        category: ErrorCategory,
        /// Human-readable message for the user.
        message: String,
        /// Whether the engine continues (true) or stops (false).
        recoverable: bool,
    },
}

pub type StreamCallback = Arc<dyn Fn(StreamEvent) + Send + Sync>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_event_serializes_correctly() {
        let event = StreamEvent::Error {
            category: ErrorCategory::Provider,
            message: "rate limit exceeded".to_string(),
            recoverable: true,
        };

        let json = serde_json::to_string(&event).unwrap();
        let deserialized: StreamEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, deserialized);
    }

    #[test]
    fn error_category_serializes_as_snake_case() {
        let json = serde_json::to_string(&ErrorCategory::ToolExecution).unwrap();
        assert_eq!(json, "\"tool_execution\"");
    }

    #[test]
    fn all_error_categories_roundtrip() {
        for category in [
            ErrorCategory::Provider,
            ErrorCategory::ToolExecution,
            ErrorCategory::Channel,
            ErrorCategory::Memory,
            ErrorCategory::System,
        ] {
            let json = serde_json::to_string(&category).unwrap();
            let back: ErrorCategory = serde_json::from_str(&json).unwrap();
            assert_eq!(category, back);
        }
    }
}
