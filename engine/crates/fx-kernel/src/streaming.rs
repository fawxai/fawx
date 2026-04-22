use fx_core::message::ProgressKind;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptTurnPhase {
    /// The turn is collecting visible work: narration, tool calls, and tool results.
    CollectingWork,
    /// The turn is executing tool calls and emitting activity progress to clients.
    ExecutingTools,
    /// The turn is synthesizing completed work into a summary.
    Summarizing,
    /// The turn is streaming or preparing the final assistant answer.
    Finalizing,
    /// The turn has reached its terminal stream result. Emitted immediately before `Done`.
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamToolProgressClass {
    Observation,
    Mutation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamToolProgressOutcome {
    Advanced,
    Duplicate,
    RetryableFailure,
}

// `Eq` is intentionally omitted because `ContextCompacted` carries `usage_ratio: f64`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StreamEvent {
    /// Speculative answer text that may be cleared if the same model turn resolves
    /// to tool use. Clients may render this like live answer text, then demote it
    /// to working narration on `TextReset`.
    TextPreviewDelta {
        text: String,
    },
    /// Working narration for the turn activity log. This is committed work
    /// narration, not speculative final-answer text.
    WorkingNarrationDelta {
        text: String,
        /// True when the narration is synthesized from typed tool progress and
        /// should be represented by the adjacent tool card instead of preserved
        /// as assistant voiceover prose.
        voiceover_suppressed: bool,
    },
    /// Clear speculative text emitted for the current visible response.
    TextReset,
    /// Legacy final-answer text delta. New live streams should emit
    /// `FinalAnswerDelta` instead so clients do not infer intent from phase.
    TextDelta {
        text: String,
    },
    /// Final assistant answer text.
    FinalAnswerDelta {
        text: String,
    },
    Progress {
        kind: ProgressKind,
        message: String,
    },
    Notification {
        title: String,
        body: String,
    },
    ToolCallStart {
        id: String,
        name: String,
    },
    ActivityStart {
        id: String,
        title: Option<String>,
        kind: String,
    },
    ActivityEnd {
        id: String,
    },
    ActivityToolCallStart {
        activity_id: String,
        id: String,
        name: String,
    },
    ToolCallComplete {
        id: String,
        name: String,
        arguments: String,
    },
    ActivityToolCallComplete {
        activity_id: String,
        id: String,
        name: String,
        arguments: String,
    },
    ToolResult {
        id: String,
        tool_name: String,
        output: String,
        is_error: bool,
    },
    ActivityToolResult {
        activity_id: String,
        id: String,
        tool_name: String,
        output: String,
        is_error: bool,
    },
    /// Semantic progress produced by the turn progress ledger for a completed
    /// tool call. This lets clients render "what was accomplished" from typed
    /// state instead of inferring it from raw tool names or prose.
    ToolProgress {
        activity_id: Option<String>,
        id: String,
        tool_name: String,
        class: StreamToolProgressClass,
        target: Option<String>,
        advances_slot: Option<String>,
        outcome: StreamToolProgressOutcome,
    },
    /// Kernel-authored summary of completed work for the current turn.
    /// Clients should render this as the completed-work phase instead of
    /// fabricating summaries from local transcript state.
    ///
    /// This is a single completed summary payload, not a text-delta stream.
    /// The final assistant answer remains the streamed user-facing response.
    CompletedSummary {
        text: String,
    },
    ToolError {
        tool_name: String,
        error: String,
    },
    PermissionPrompt(crate::permission_prompt::PermissionPrompt),
    PhaseChange {
        phase: Phase,
    },
    /// High-level transcript lifecycle boundary for clients that render a turn
    /// as ordered chunks instead of inferring state from low-level loop phases.
    TranscriptPhaseBoundary {
        phase: TranscriptTurnPhase,
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
    fn progress_event_serializes_correctly() {
        let event = StreamEvent::Progress {
            kind: ProgressKind::Researching,
            message: "Researching the request.".to_string(),
        };

        let json = serde_json::to_string(&event).unwrap();
        let deserialized: StreamEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, deserialized);
    }

    #[test]
    fn transcript_phase_boundary_event_serializes_correctly() {
        let event = StreamEvent::TranscriptPhaseBoundary {
            phase: TranscriptTurnPhase::Finalizing,
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("TranscriptPhaseBoundary"));
        assert!(json.contains("finalizing"));
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
    fn tool_progress_event_serializes_correctly() {
        let event = StreamEvent::ToolProgress {
            activity_id: Some("tool-round-call-1".to_string()),
            id: "call-1".to_string(),
            tool_name: "read_file".to_string(),
            class: StreamToolProgressClass::Observation,
            target: Some("PR 1834".to_string()),
            advances_slot: Some("evidence:required:pr:1834".to_string()),
            outcome: StreamToolProgressOutcome::Advanced,
        };

        let json = serde_json::to_string(&event).unwrap();
        let deserialized: StreamEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, deserialized);
    }

    #[test]
    fn completed_summary_event_serializes_correctly() {
        let event = StreamEvent::CompletedSummary {
            text: "Worked this turn: 1 file read.".to_string(),
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("CompletedSummary"));
        assert!(json.contains("Worked this turn"));
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
