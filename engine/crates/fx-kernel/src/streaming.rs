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

impl ErrorCategory {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Provider => "provider",
            Self::ToolExecution => "tool_execution",
            Self::Channel => "channel",
            Self::Memory => "memory",
            Self::System => "system",
        }
    }
}

impl std::fmt::Display for ErrorCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Perceive,
    Reason,
    Act,
    Synthesize,
}

// `Eq` is intentionally omitted because `ContextCompacted` carries `usage_ratio: f64`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StreamEvent {
    TextDelta {
        text: String,
    },
    Notification {
        title: String,
        body: String,
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
    ToolError {
        tool_name: String,
        error: String,
    },
    PermissionPrompt(crate::permission_prompt::PermissionPrompt),
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
    /// Compaction completed and the conversation context was reduced.
    ContextCompacted {
        /// Which compaction tier fired.
        tier: String,
        /// Number of messages removed from the conversation window.
        messages_removed: usize,
        /// Token estimate before compaction.
        tokens_before: usize,
        /// Token estimate after compaction.
        tokens_after: usize,
        /// Current context usage ratio (0.0-1.0).
        usage_ratio: f64,
    },
}

pub type StreamCallback = Arc<dyn Fn(StreamEvent) + Send + Sync>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_event_serializes_correctly() {
        let event = StreamEvent::Notification {
            title: "Fawx".to_string(),
            body: "Task complete".to_string(),
        };

        let json = serde_json::to_string(&event).unwrap();
        let deserialized: StreamEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, deserialized);
    }

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
    fn tool_error_event_serializes_correctly() {
        let event = StreamEvent::ToolError {
            tool_name: "read_file".to_string(),
            error: "permission denied".to_string(),
        };

        let json = serde_json::to_string(&event).unwrap();
        let deserialized: StreamEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, deserialized);
    }

    #[test]
    fn context_compacted_event_serializes_correctly() {
        let event = StreamEvent::ContextCompacted {
            tier: "slide".to_string(),
            messages_removed: 12,
            tokens_before: 5_100,
            tokens_after: 2_900,
            usage_ratio: 0.42,
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
    fn error_category_display_uses_snake_case() {
        assert_eq!(ErrorCategory::ToolExecution.to_string(), "tool_execution");
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
