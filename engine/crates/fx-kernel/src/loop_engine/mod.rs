//! Agentic loop orchestrator.

use crate::act::{
    ActionContinuation, ActionNextStep, ActionResult, ActionTerminal, ContinuationToolScope,
    ExternalActionKind, FailureClass, ProceedUnderConstraints, TokenUsage, ToolCacheability,
    ToolCallClassification, ToolExecutionDiagnostics, ToolExecutor, ToolResult, TurnCommitment,
};
use crate::budget::{
    estimate_complexity, ActionCost, BudgetRemaining, BudgetState, BudgetTracker,
    TerminationConfig, ToolStrippingAfterNudge,
};
#[cfg(test)]
use crate::budget::{AllocationPlan, BudgetConfig};
use crate::cancellation::CancellationToken;
use crate::channels::ChannelRegistry;
use crate::context_manager::ContextCompactor;
use crate::loop_engine::tool_execution::tool_call_may_complete_external_action;

#[cfg(test)]
use crate::conversation_compactor::debug_assert_tool_pair_integrity;
use crate::conversation_compactor::{
    estimate_text_tokens, CompactionConfig, CompactionMemoryFlush, ConversationBudget,
};
use crate::decide::Decision;
use crate::input::{LoopCommand, LoopInputChannel};

use crate::perceive::{ProcessedPerception, TrimmingPolicy};
use crate::signal_summarizer::{ObservationPressure, SignalSummarizer, SignalSummaryContext};
use crate::signals::{
    ControlPlaneDecisionKind, LoopStep, Signal, SignalCollector, SignalKind,
    SignalToolClassification,
};
use crate::streaming::{
    ErrorCategory, Phase, StreamCallback, StreamEvent, StreamToolProgressClass,
    StreamToolProgressOutcome, TranscriptTurnPhase,
};
use crate::types::{
    Goal, IdentityContext, LoopError, PerceptionSnapshot, ReasoningContext, WorkingMemoryEntry,
};

use async_trait::async_trait;
#[cfg(test)]
use futures_util::StreamExt;
use fx_core::message::{InternalMessage, ProgressKind, StreamPhase};
use fx_core::runtime_info::RuntimeInfo;
use fx_core::tool_routing::{RouteAdvisory, ToolRoutingSummary};
#[cfg(test)]
use fx_core::types::{InputSource, ScreenState, UserInput};
use fx_decompose::{AggregationStrategy, ComplexityHint, DecompositionPlan, SubGoal};
#[cfg(test)]
use fx_decompose::{SubGoalOutcome, SubGoalResult};
use fx_llm::{
    emit_default_stream_response, CompletionRequest, CompletionResponse, ContentBlock, Message,
    MessageRole, PromptCacheAffinity, ProviderError, StreamCallback as ProviderStreamCallback,
    StreamChunk, ToolCall, ToolDefinition, ToolUseDelta, Usage,
};
use fx_memory::SignalSink;
use fx_session::SessionMemory;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(test)]
use fx_decompose::SubGoalContract;

mod bounded_local;
mod compaction;
mod continuation;
mod decomposition;
mod direct_inspection;
mod direct_utility;
mod preflight_route;
mod progress;
mod request;
mod retry;
mod streaming;
mod tool_call_normalization;
mod tool_execution;
mod turn_control;

#[cfg(test)]
use self::compaction::CompactionTier;
use self::compaction::{compacted_context_summary, CompactionScope};
#[cfg(test)]
use self::compaction::{
    has_compaction_marker, has_conversation_summary_marker, has_emergency_compaction_marker,
    marker_message_index, session_memory_message_index, summary_message_index,
};
#[cfg(test)]
use self::decomposition::{
    build_sub_goal_snapshot, child_max_iterations, should_halt_sub_goal_sequence,
    sub_goal_result_from_loop, successful_mutation_tool_names, successful_tool_names,
};
use self::decomposition::{
    estimate_plan_cost, is_decomposition_results_message, parse_decomposition_plan,
};
use self::preflight_route::{
    build_degraded_public_web_fallback_plan, build_route_plan, detect_route_resource, RoutePlan,
};
#[cfg(test)]
use self::request::{build_continuation_request, ContinuationRequestParams};
use self::request::{
    build_forced_synthesis_request, build_reasoning_messages, build_reasoning_request,
    build_simple_agent_messages, build_simple_agent_request, build_truncation_continuation_request,
    completion_request_to_prompt, ForcedSynthesisRequestParams, ReasoningRequestParams,
    RequestBuildContext, SimpleAgentRequestParams, SkillPromptSummary, ToolRequestConfig,
    TruncationContinuationRequestParams,
};
#[cfg(test)]
use self::request::{
    build_reasoning_system_prompt, build_reasoning_system_prompt_with_agent_preferences,
    build_reasoning_system_prompt_with_notify_guidance, build_tool_continuation_system_prompt,
    build_tool_continuation_system_prompt_with_notify_guidance, decompose_tool_definition,
    reasoning_user_prompt, tool_definitions_with_decompose,
};
#[cfg(test)]
use self::retry::same_call_failure_reason;
use self::retry::{RetryTracker, ToolCallKey};
use self::streaming::{StreamingRequestContext, TextStreamVisibility};
#[cfg(test)]
use self::tool_execution::ToolRoundOutcome;
use self::tool_execution::{
    blocked_tool_message, extract_tool_use_provider_ids, record_tool_round_messages,
};
use self::turn_control::{
    FinalResponseValidationFacts, FinalResponseValidationOutcome, FinalResponseViolation,
    TurnControlPlane,
};

#[cfg(test)]
use self::tool_execution::{
    append_tool_round_messages, build_tool_result_message, build_tool_use_assistant_message,
    evict_oldest_results, tool_synthesis_prompt, truncate_tool_results,
    TOOL_SYNTHESIS_MAX_OUTPUT_TOKENS,
};

#[cfg(test)]
use self::streaming::{
    finalize_stream_tool_calls, stream_tool_call_from_state, StreamResponseState,
    StreamToolCallState,
};

#[cfg(test)]
use crate::budget::{AllocationMode, BudgetAllocator, DepthMode};
#[cfg(test)]
use bounded_local::detect_turn_execution_profile;
use bounded_local::{
    bounded_local_phase_label, bounded_local_terminal_reason_label,
    bounded_local_terminal_reason_text, detect_turn_execution_profile_for_ownership,
    BoundedLocalPhase, BoundedLocalTerminalReason, TurnExecutionProfile,
};
use continuation::{
    commitment_tool_scope, final_response_turn_commitment, render_turn_commitment_directive,
    tool_continuation_artifact_write_target, tool_continuation_turn_commitment,
    turn_commitment_metadata,
};
#[cfg(test)]
use direct_inspection::DirectInspectionProfile;
use direct_inspection::{direct_inspection_profile_label, DirectInspectionOwnership};
#[cfg(test)]
use direct_utility::DirectUtilityProfile;
use direct_utility::{
    detect_direct_utility_profile, direct_utility_completion_response, direct_utility_directive,
    direct_utility_progress, direct_utility_tool_names,
};
use progress::json_string_arg;
#[cfg(test)]
use progress::{progress_for_tool_round, progress_for_turn_state_with_profile};

/// Dynamic scratchpad context provider for iteration-boundary refresh.
///
/// Implemented by the CLI layer to bridge `fx-scratchpad::Scratchpad` into the
/// kernel without a circular dependency. The loop engine calls these methods at
/// each iteration boundary so the model always sees up-to-date scratchpad state.
pub trait ScratchpadProvider: Send + Sync {
    /// Render current scratchpad state for prompt injection.
    fn render_for_context(&self) -> String;
    /// Compact scratchpad if it exceeds size thresholds.
    fn compact_if_needed(&self, current_iteration: u32);
}

impl std::fmt::Debug for dyn ScratchpadProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ScratchpadProvider")
    }
}

/// LLM provider trait used by the loop.
#[async_trait]
pub trait LlmProvider: Send + Sync + std::fmt::Debug {
    async fn generate(
        &self,
        prompt: &str,
        max_tokens: u32,
    ) -> Result<String, fx_core::error::LlmError>;

    async fn generate_streaming(
        &self,
        prompt: &str,
        max_tokens: u32,
        callback: Box<dyn Fn(String) + Send + 'static>,
    ) -> Result<String, fx_core::error::LlmError>;

    fn model_name(&self) -> &str;

    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, ProviderError> {
        let prompt = completion_request_to_prompt(&request);
        let max_tokens = request.max_tokens.unwrap_or(REASONING_MAX_OUTPUT_TOKENS);
        let generated = self
            .generate(&prompt, max_tokens)
            .await
            .map_err(|error| ProviderError::Provider(error.to_string()))?;

        Ok(CompletionResponse {
            content: vec![fx_llm::ContentBlock::Text { text: generated }],
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: None,
        })
    }

    async fn complete_stream(
        &self,
        request: CompletionRequest,
    ) -> Result<fx_llm::CompletionStream, ProviderError> {
        let response = self.complete(request).await?;
        let chunk = response_to_chunk(response);
        let stream =
            futures_util::stream::once(async move { Ok::<StreamChunk, ProviderError>(chunk) });
        Ok(Box::pin(stream))
    }

    async fn stream(
        &self,
        request: CompletionRequest,
        callback: ProviderStreamCallback,
    ) -> Result<CompletionResponse, ProviderError> {
        let response = self.complete(request).await?;
        emit_default_stream_response(&response, &callback);
        Ok(response)
    }
}

fn response_to_chunk(response: CompletionResponse) -> StreamChunk {
    let CompletionResponse {
        content,
        tool_calls,
        usage,
        stop_reason,
    } = response;
    let provider_item_ids = extract_tool_use_provider_ids(&content);

    let delta_content = content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            ContentBlock::Image { .. } => None,
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    let tool_use_deltas = tool_calls
        .into_iter()
        .map(|call| ToolUseDelta {
            provider_id: provider_item_ids.get(&call.id).cloned(),
            id: Some(call.id),
            name: Some(call.name),
            arguments_delta: Some(call.arguments.to_string()),
            arguments_done: true,
        })
        .collect();

    StreamChunk {
        delta_content: (!delta_content.is_empty()).then_some(delta_content),
        tool_use_deltas,
        usage,
        stop_reason,
    }
}

#[derive(Clone, Copy)]
struct CycleStream<'a> {
    callback: Option<&'a StreamCallback>,
}

impl<'a> CycleStream<'a> {
    fn disabled() -> Self {
        Self { callback: None }
    }

    fn enabled(callback: &'a StreamCallback) -> Self {
        Self {
            callback: Some(callback),
        }
    }

    fn emit(self, event: StreamEvent) {
        if let Some(callback) = self.callback {
            callback(event);
        }
    }

    fn emit_error(self, category: ErrorCategory, message: impl Into<String>, recoverable: bool) {
        self.emit(StreamEvent::Error {
            category,
            message: message.into(),
            recoverable,
        });
    }

    fn phase(self, phase: Phase) {
        self.emit(StreamEvent::PhaseChange { phase });
    }

    fn phase_boundary(self, phase: TranscriptTurnPhase) {
        self.emit(StreamEvent::TranscriptPhaseBoundary { phase });
    }

    fn tool_call_start(self, call: &ToolCall) {
        self.emit(StreamEvent::ToolCallStart {
            id: call.id.clone(),
            name: call.name.clone(),
        });
    }

    fn activity_start(self, id: &str, title: Option<String>) {
        self.emit(StreamEvent::ActivityStart {
            id: id.to_string(),
            title,
            kind: "tool_round".to_string(),
        });
    }

    fn activity_end(self, id: &str) {
        self.emit(StreamEvent::ActivityEnd { id: id.to_string() });
    }

    fn activity_tool_call_start(self, activity_id: &str, call: &ToolCall) {
        self.emit(StreamEvent::ActivityToolCallStart {
            activity_id: activity_id.to_string(),
            id: call.id.clone(),
            name: call.name.clone(),
        });
    }

    fn tool_call_complete(self, call: &ToolCall) {
        self.emit(StreamEvent::ToolCallComplete {
            id: call.id.clone(),
            name: call.name.clone(),
            arguments: call.arguments.to_string(),
        });
    }

    fn activity_tool_call_complete(self, activity_id: &str, call: &ToolCall) {
        self.emit(StreamEvent::ActivityToolCallComplete {
            activity_id: activity_id.to_string(),
            id: call.id.clone(),
            name: call.name.clone(),
            arguments: call.arguments.to_string(),
        });
    }

    fn tool_result(self, result: &ToolResult) {
        self.emit(StreamEvent::ToolResult {
            id: result.tool_call_id.clone(),
            tool_name: result.tool_name.clone(),
            output: result.output.clone(),
            is_error: !result.success,
        });
    }

    fn activity_tool_result(self, activity_id: &str, result: &ToolResult) {
        self.emit(StreamEvent::ActivityToolResult {
            activity_id: activity_id.to_string(),
            id: result.tool_call_id.clone(),
            tool_name: result.tool_name.clone(),
            output: result.output.clone(),
            is_error: !result.success,
        });
    }

    fn tool_progress(self, activity_id: Option<&str>, entry: &ToolProgressEntry) {
        self.emit(StreamEvent::ToolProgress {
            activity_id: activity_id.map(str::to_string),
            id: entry.call_id.clone(),
            tool_name: entry.tool_name.clone(),
            class: entry.class.into(),
            target: entry.target.clone(),
            advances_slot: entry.advances_slot.clone(),
            outcome: entry.outcome.into(),
        });
    }

    fn completed_summary(self, text: &str) {
        self.emit(StreamEvent::CompletedSummary {
            text: text.to_string(),
        });
    }

    fn tool_error(self, tool_name: &str, error: &str) {
        self.emit(StreamEvent::ToolError {
            tool_name: tool_name.to_string(),
            error: error.to_string(),
        });
    }

    fn notification(self, title: impl Into<String>, body: impl Into<String>) {
        self.emit(StreamEvent::Notification {
            title: title.into(),
            body: body.into(),
        });
    }

    fn done(self, response: &str) {
        self.emit(StreamEvent::Done {
            response: response.to_string(),
        });
    }

    fn final_answer(self, response: &str) {
        self.emit(StreamEvent::FinalAnswerDelta {
            text: response.to_string(),
        });
    }

    fn done_result(self, result: &LoopResult) {
        if let Some(response) = result.stream_done_response() {
            self.done(&response);
        }
    }
}

fn build_user_message(snapshot: &PerceptionSnapshot, user_message: &str) -> Message {
    match snapshot.user_input.as_ref() {
        Some(user_input) if !user_input.images.is_empty() || !user_input.documents.is_empty() => {
            Message::user_with_attachments(
                user_message,
                user_input.images.clone(),
                user_input.documents.clone(),
            )
        }
        _ => Message::user(user_message),
    }
}

fn normalize_turn_steer(text: String) -> Option<String> {
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn turn_steering_guidance_message(steer: &str) -> String {
    format!(
        "Mid-turn user steering guidance (current turn only; do not treat this as a new queued task): {steer}"
    )
}

/// Runtime loop status for `/loop` diagnostics.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LoopStatus {
    /// Iterations executed in the last loop invocation.
    pub iteration_count: u32,
    /// Maximum iterations permitted per invocation.
    pub max_iterations: u32,
    /// Total LLM calls consumed by the tracker.
    pub llm_calls_used: u32,
    /// Total tool invocations consumed by the tracker.
    pub tool_invocations_used: u32,
    /// Total tokens consumed by the tracker.
    pub tokens_used: u64,
    /// Total cost consumed by the tracker, in cents.
    pub cost_cents_used: u64,
    /// Remaining budget snapshot at query time.
    pub remaining: BudgetRemaining,
}

/// Result returned after running the loop engine.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LoopResult {
    /// Loop completed successfully.
    Complete {
        /// Final user-visible response.
        response: String,
        /// Iterations executed.
        iterations: u32,
        /// Total tokens consumed by this cycle.
        tokens_used: TokenUsage,
        /// Signals emitted during the cycle.
        signals: Vec<Signal>,
    },
    /// Loop exited because budget limits were reached.
    BudgetExhausted {
        /// Optional best-effort partial response text.
        partial_response: Option<String>,
        /// Iterations completed before exhaustion.
        iterations: u32,
        /// Signals emitted during the cycle.
        signals: Vec<Signal>,
    },
    /// Loop could not produce a usable terminal response, but may have partial progress.
    Incomplete {
        /// Optional best-effort partial response text.
        partial_response: Option<String>,
        /// Why the run is incomplete.
        reason: String,
        /// Iterations completed before the loop ended incomplete.
        iterations: u32,
        /// Signals emitted during the cycle.
        signals: Vec<Signal>,
    },
    /// Loop was stopped by the user (stop, abort, or Ctrl+C).
    UserStopped {
        /// Best-effort partial response text.
        partial_response: Option<String>,
        /// Iterations completed before the user stopped.
        iterations: u32,
        /// Signals emitted during the cycle.
        signals: Vec<Signal>,
    },
    /// Loop ended with a recoverable or non-recoverable runtime error.
    Error {
        /// Error message to surface to the caller.
        message: String,
        /// Whether retrying may succeed.
        recoverable: bool,
        /// Signals emitted during the cycle.
        signals: Vec<Signal>,
    },
}

impl LoopResult {
    pub fn signals(&self) -> &[Signal] {
        match self {
            Self::Complete { signals, .. }
            | Self::BudgetExhausted { signals, .. }
            | Self::Incomplete { signals, .. }
            | Self::UserStopped { signals, .. }
            | Self::Error { signals, .. } => signals,
        }
    }

    fn stream_done_response(&self) -> Option<String> {
        match self {
            Self::Complete { response, .. } => Some(response.clone()),
            Self::BudgetExhausted {
                partial_response, ..
            } => Some(
                partial_response
                    .clone()
                    .unwrap_or_else(|| "budget exhausted".to_string()),
            ),
            Self::Incomplete {
                partial_response, ..
            } => {
                // Incomplete turns without usable text should surface via the
                // typed engine_error path, not as a fabricated final assistant
                // answer. This keeps Swift from rendering placeholder failure
                // text as completed work.
                partial_response
                    .clone()
                    .filter(|text| !text.trim().is_empty())
            }
            Self::UserStopped {
                partial_response, ..
            } => Some(
                partial_response
                    .clone()
                    .filter(|text| !text.trim().is_empty())
                    .unwrap_or_else(|| "user stopped".to_string()),
            ),
            Self::Error { .. } => None,
        }
    }

    fn stream_final_answer_response(&self) -> Option<String> {
        match self {
            Self::Complete { response, .. } => Some(response.clone()),
            Self::BudgetExhausted {
                partial_response, ..
            }
            | Self::Incomplete {
                partial_response, ..
            } => partial_response
                .clone()
                .filter(|text| !text.trim().is_empty()),
            Self::UserStopped { .. } | Self::Error { .. } => None,
        }
    }

    fn stream_terminal_error(&self) -> Option<(ErrorCategory, String, bool)> {
        match self {
            Self::Incomplete {
                partial_response,
                reason,
                ..
            } if partial_response
                .as_ref()
                .is_none_or(|text| text.trim().is_empty()) =>
            {
                Some((ErrorCategory::System, reason.clone(), false))
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum ExecutionVisibility {
    #[default]
    Public,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RootTurnContract {
    deliverables: Vec<RootTurnDeliverable>,
    blocked_terminal_attempts: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RootTurnDeliverable {
    ResponseSection {
        label: String,
        normalized_label: String,
    },
    MutationWork {
        label: String,
        satisfied: bool,
    },
    ArtifactWrite {
        path: String,
        satisfied: bool,
    },
    ExternalAction {
        kind: RootTurnExternalActionKind,
        label: String,
        satisfied: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(clippy::enum_variant_names)]
enum RootTurnExternalActionKind {
    GitHubPrComment,
    GitHubIssueComment,
    GitHubPrReview,
    GitHubPrCreate,
    GitPush,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RootTurnCompletionBlock {
    missing_response_sections: Vec<String>,
    pending_mutation_work: Vec<String>,
    pending_artifact_paths: Vec<String>,
    pending_external_actions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RootTurnContractExtraction {
    contract: Option<RootTurnContract>,
    deliverable_block_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DeliverableSectionParse {
    labels: Vec<String>,
    block_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TaskContract {
    inputs: Vec<InputRequirement>,
    phase: TaskPhase,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InputRequirement {
    description: String,
    normalized_description: String,
    satisfied: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaskPhase {
    Gathering,
    Synthesizing,
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TaskContractExtraction {
    contract: Option<TaskContract>,
    visible_text: Option<String>,
}

/// Core orchestrator for the 7-step agentic loop.
///
/// Note: `LoopEngine` previously derived `Clone`, but context compaction
/// introduced a non-`Clone` cooldown tracker
/// (`compaction_last_iteration: Mutex<HashMap<CompactionScope, u32>>`).
/// `LoopInputChannel` also contains an `mpsc::Receiver`, which remains
/// non-`Clone`. No existing code clones `LoopEngine`, so this is a safe change.
pub struct LoopEngine {
    budget: BudgetTracker,
    context: ContextCompactor,
    tool_executor: Arc<dyn ToolExecutor>,
    max_iterations: u32,
    iteration_count: u32,
    synthesis_instruction: String,
    agent_preferences: Option<String>,
    memory_context: Option<String>,
    execution_context: Option<ExecutionContext>,
    session_memory: Arc<Mutex<SessionMemory>>,
    scratchpad_context: Option<String>,
    signals: SignalCollector,
    signal_store: Option<Box<dyn SignalSink>>,
    cancel_token: Option<CancellationToken>,
    input_channel: Option<LoopInputChannel>,
    user_stop_requested: bool,
    active_turn_steer: Option<String>,
    pending_flow_command: Option<LoopCommand>,
    event_bus: Option<fx_core::EventBus>,
    execution_visibility: ExecutionVisibility,
    compaction_config: CompactionConfig,
    conversation_budget: ConversationBudget,
    /// LLM for compaction-time memory extraction.
    compaction_llm: Option<Arc<dyn LlmProvider>>,
    memory_flush: Option<Arc<dyn CompactionMemoryFlush>>,
    compaction_last_iteration: Mutex<HashMap<CompactionScope, u32>>,
    /// Guards performance signal to fire only on the Normal→Low transition,
    /// not on every `perceive()` call while the budget stays Low.
    budget_low_signaled: bool,
    /// Consecutive iterations that included tool calls.
    /// Stored on `LoopEngine` because `perceive()` only has `&mut self`.
    /// Cycle-scoped; `prepare_cycle()` resets it, so child cycles start fresh.
    consecutive_tool_turns: u16,
    /// Cycle-scoped repetitive observation tracking for the observation-only gate.
    observation_round_tracker: ObservationRoundTracker,
    /// Cycle-scoped typed task-progress ledger for tool rounds.
    ///
    /// This is the control-plane primitive behind observation restrictions:
    /// methodical read-only work should stay allowed while it advances distinct
    /// evidence slots; repeated or ungrounded work accumulates pressure.
    turn_progress_ledger: TurnProgressLedger,
    /// Recent completed-cycle signals kept only for bounded prompt feedback.
    recent_signal_cycles: VecDeque<Vec<Signal>>,
    /// Latest reasoning input messages for graceful budget-exhausted synthesis.
    /// Stored on `LoopEngine` because `perceive()` only has `&mut self`.
    last_reasoning_messages: Vec<Message>,
    /// Last feedback advisory injected during the active cycle.
    last_signal_feedback_summary: Option<String>,
    /// Tool retry tracker for the current cycle.
    tool_retry_tracker: RetryTracker,
    /// Latest failed-tool signal ID for each retry signature in the current cycle.
    /// Bounded by the number of distinct tool signatures seen in a single cycle.
    last_tool_failure_signal_ids: HashMap<ToolCallKey, u64>,
    /// Circuit breaker for repeated tool failures across outer-loop iterations.
    repeated_tool_failure_tracker: RepeatedToolFailureTracker,
    /// Whether a successful `notify` tool call occurred during the current cycle.
    notify_called_this_cycle: bool,
    /// Whether this cycle already emitted the terminal answer over the typed
    /// final-answer stream. `done` carries text for lifecycle compatibility,
    /// but UI reducers need this explicit channel to keep answer text out of
    /// work-summary narration.
    final_answer_streamed_this_cycle: bool,
    /// Last high-level transcript phase emitted this cycle. This intentionally
    /// differs from low-level loop phases (`perceive`, `reason`, `act`) so UI
    /// clients can render transcript chunks without guessing from kernel steps.
    transcript_turn_phase: Option<TranscriptTurnPhase>,
    /// Whether this cycle currently has an active notification delivery channel.
    notify_tool_guidance_enabled: bool,
    /// Shared iteration counter for scratchpad age tracking.
    iteration_counter: Option<Arc<AtomicU32>>,
    /// Dynamic scratchpad provider for iteration-boundary context refresh.
    scratchpad_provider: Option<Arc<dyn ScratchpadProvider>>,
    /// Provider-specific tool output item identifiers keyed by stable tool call id.
    tool_call_provider_ids: HashMap<String, String>,
    /// Structured diagnostics captured for the latest executed tool results.
    pending_tool_result_diagnostics: HashMap<String, ToolExecutionDiagnostics>,
    /// Structured runtime signals emitted by loadable tools for the latest results.
    pending_tool_result_signals: HashMap<String, Vec<Signal>>,
    /// Mixed text emitted alongside tool calls before tool execution begins.
    pending_tool_response_text: Option<String>,
    /// Optional scoped tool surface for the next root reasoning pass.
    pending_tool_scope: Option<ContinuationToolScope>,
    /// Optional typed turn commitment for the next root reasoning pass.
    pending_turn_commitment: Option<TurnCommitment>,
    /// Active closure gate for a root-turn external action that must be
    /// completed before normal investigation can resume.
    pending_external_action_target: Option<RootTurnExternalActionKind>,
    /// Consecutive failed attempts at completing the pending external action.
    /// After reaching the configured threshold, the gate lifts so the agent can
    /// recover.
    pending_external_action_consecutive_failures: u8,
    /// Cycle-scoped retry pressure for final-response protocol violations.
    final_response_blocked_attempts: u8,
    /// Cycle-scoped marker that the provider attempted tool activity while the
    /// kernel was in final-response mode. The decide step can observe this
    /// violation before the terminal completion gate sees the response text.
    final_response_attempted_tool_activity: bool,
    /// Cycle-scoped marker that the final-response candidate ended with a
    /// non-terminal stop reason. Final output must be complete before the
    /// kernel can accept it as the turn's terminal answer.
    final_response_candidate_truncated: bool,
    /// Kernel-owned first-route plan for external-resource requests.
    preflight_route_plan: Option<RoutePlan>,
    /// Advisory-only cross-session route memories loaded from typed memory surfaces.
    route_advisories: Vec<RouteAdvisory>,
    /// Typed completion contract for the active root turn, when the prompt declares one.
    root_turn_contract: Option<RootTurnContract>,
    /// Active task-input lifecycle contract for the current root turn, when declared by the model.
    task_contract: Option<TaskContract>,
    /// Explicit artifact path requested by the user for this turn, if any.
    requested_artifact_target: Option<String>,
    /// Active gate requiring the next root pass to write the requested artifact first.
    pending_artifact_write_target: Option<String>,
    /// Last root-owned public progress update emitted during the current cycle.
    last_turn_state_progress: Option<(ProgressKind, String)>,
    /// Last ephemeral tool/activity progress update emitted during the current cycle.
    last_activity_progress: Option<(ProgressKind, String)>,
    /// Last public progress update actually emitted to the user.
    last_emitted_public_progress: Option<(ProgressKind, String)>,
    error_callback: Option<StreamCallback>,
    /// Extended thinking configuration forwarded to completion requests.
    thinking_config: Option<fx_llm::ThinkingConfig>,
    /// Runtime snapshot used to surface loaded skill summaries in prompts.
    runtime_info: Option<Arc<RwLock<RuntimeInfo>>>,
    /// Revision counter for cached runtime skill summaries.
    runtime_skill_prompt_revision: Option<Arc<AtomicU64>>,
    cached_runtime_skill_prompt_revision: u64,
    cached_runtime_skill_prompt_summaries: Vec<SkillPromptSummary>,
    /// Whether this runner may expose and honor the kernel-level decompose tool.
    decompose_enabled: bool,
    /// Whether experimental Graph-of-Thoughts decomposition is allowed.
    ///
    /// GoT is intentionally dormant by default. It remains available only to
    /// explicit internal experiments so ordinary agent runs keep one typed
    /// decomposition contract.
    graph_of_thoughts_enabled: bool,
    /// Root-turn ownership for direct-inspection classification during decomposition.
    direct_inspection_ownership: DirectInspectionOwnership,
    /// Turn-scoped routing profile for bounded local work vs. general tasks.
    turn_execution_profile: TurnExecutionProfile,
    /// Current phase for bounded local code-edit execution.
    bounded_local_phase: BoundedLocalPhase,
    /// Whether the bounded local workflow has already consumed its one recovery round.
    bounded_local_recovery_used: bool,
    /// Failed mutation targets to revisit during a bounded local recovery round.
    bounded_local_recovery_focus: Vec<String>,
    /// Kernel-authored terminal reason for bounded local runs, when they end before completion.
    bounded_local_terminal_reason: Option<BoundedLocalTerminalReason>,
    /// Registry of active input/output channels.
    channel_registry: ChannelRegistry,
}

impl std::fmt::Debug for LoopEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoopEngine")
            .field("max_iterations", &self.max_iterations)
            .field("iteration_count", &self.iteration_count)
            .field("memory_context", &self.memory_context)
            .field("execution_context", &self.execution_context)
            .field("session_memory", &"SessionMemory")
            .field("scratchpad_context", &self.scratchpad_context)
            .field("signal_store", &self.signal_store.is_some())
            .field("compaction_config", &self.compaction_config)
            .field("budget_low_signaled", &self.budget_low_signaled)
            .field("consecutive_tool_turns", &self.consecutive_tool_turns)
            .field("observation_round_tracker", &self.observation_round_tracker)
            .field("turn_progress_ledger", &self.turn_progress_ledger)
            .field("recent_signal_cycles", &self.recent_signal_cycles.len())
            .field(
                "last_signal_feedback_summary",
                &self.last_signal_feedback_summary,
            )
            .field("tool_retry_tracker", &self.tool_retry_tracker)
            .field(
                "last_tool_failure_signal_ids",
                &self.last_tool_failure_signal_ids.len(),
            )
            .field(
                "repeated_tool_failure_tracker",
                &self.repeated_tool_failure_tracker,
            )
            .field("notify_called_this_cycle", &self.notify_called_this_cycle)
            .field(
                "notify_tool_guidance_enabled",
                &self.notify_tool_guidance_enabled,
            )
            .field("pending_tool_scope", &self.pending_tool_scope)
            .field("pending_turn_commitment", &self.pending_turn_commitment)
            .field(
                "pending_external_action_target",
                &self.pending_external_action_target,
            )
            .field(
                "final_response_blocked_attempts",
                &self.final_response_blocked_attempts,
            )
            .field(
                "final_response_attempted_tool_activity",
                &self.final_response_attempted_tool_activity,
            )
            .field(
                "final_response_candidate_truncated",
                &self.final_response_candidate_truncated,
            )
            .field("preflight_route_plan", &self.preflight_route_plan)
            .field("route_advisories", &self.route_advisories.len())
            .field("root_turn_contract", &self.root_turn_contract)
            .field("task_contract", &self.task_contract)
            .field("requested_artifact_target", &self.requested_artifact_target)
            .field(
                "pending_artifact_write_target",
                &self.pending_artifact_write_target,
            )
            .field("last_turn_state_progress", &self.last_turn_state_progress)
            .field("last_activity_progress", &self.last_activity_progress)
            .field(
                "last_emitted_public_progress",
                &self.last_emitted_public_progress,
            )
            .field(
                "runtime_info",
                &self.runtime_info.as_ref().map(|_| "RuntimeInfo"),
            )
            .field(
                "runtime_skill_prompt_revision",
                &self
                    .runtime_skill_prompt_revision
                    .as_ref()
                    .map(|_| "AtomicU64"),
            )
            .field(
                "cached_runtime_skill_prompt_revision",
                &self.cached_runtime_skill_prompt_revision,
            )
            .field(
                "cached_runtime_skill_prompt_summaries",
                &self.cached_runtime_skill_prompt_summaries.len(),
            )
            .field(
                "direct_inspection_ownership",
                &self.direct_inspection_ownership,
            )
            .field("turn_execution_profile", &self.turn_execution_profile)
            .field("bounded_local_phase", &self.bounded_local_phase)
            .field(
                "bounded_local_recovery_used",
                &self.bounded_local_recovery_used,
            )
            .field(
                "bounded_local_recovery_focus",
                &self.bounded_local_recovery_focus,
            )
            .field(
                "bounded_local_terminal_reason",
                &self.bounded_local_terminal_reason,
            )
            .finish_non_exhaustive()
    }
}

struct ErrorCallbackGuard<'a> {
    engine: &'a mut LoopEngine,
    original: Option<StreamCallback>,
}

impl<'a> ErrorCallbackGuard<'a> {
    fn install(engine: &'a mut LoopEngine, replacement: Option<StreamCallback>) -> Self {
        let original = engine.error_callback.clone();
        if let Some(callback) = replacement {
            engine.error_callback = Some(callback);
        }
        Self { engine, original }
    }
}

impl std::ops::Deref for ErrorCallbackGuard<'_> {
    type Target = LoopEngine;

    fn deref(&self) -> &Self::Target {
        self.engine
    }
}

impl std::ops::DerefMut for ErrorCallbackGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.engine
    }
}

impl Drop for ErrorCallbackGuard<'_> {
    fn drop(&mut self) {
        self.engine.error_callback = self.original.take();
    }
}

#[derive(Default)]
#[must_use = "builder does nothing unless .build() is called"]
pub struct LoopEngineBuilder {
    budget: Option<BudgetTracker>,
    context: Option<ContextCompactor>,
    tool_executor: Option<Arc<dyn ToolExecutor>>,
    max_iterations: Option<u32>,
    synthesis_instruction: Option<String>,
    agent_preferences: Option<String>,
    compaction_config: Option<CompactionConfig>,
    compaction_llm: Option<Arc<dyn LlmProvider>>,
    memory_flush: Option<Arc<dyn CompactionMemoryFlush>>,
    event_bus: Option<fx_core::EventBus>,
    cancel_token: Option<CancellationToken>,
    input_channel: Option<LoopInputChannel>,
    memory_context: Option<String>,
    session_memory: Option<Arc<Mutex<SessionMemory>>>,
    route_advisories: Vec<RouteAdvisory>,
    scratchpad_context: Option<String>,
    signal_store: Option<Box<dyn SignalSink>>,
    iteration_counter: Option<Arc<AtomicU32>>,
    scratchpad_provider: Option<Arc<dyn ScratchpadProvider>>,
    error_callback: Option<StreamCallback>,
    thinking_config: Option<fx_llm::ThinkingConfig>,
    runtime_info: Option<Arc<RwLock<RuntimeInfo>>>,
    runtime_skill_prompt_revision: Option<Arc<AtomicU64>>,
    decompose_enabled: Option<bool>,
    graph_of_thoughts_enabled: Option<bool>,
    execution_visibility: ExecutionVisibility,
}

impl std::fmt::Debug for LoopEngineBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoopEngineBuilder")
            .field("budget", &self.budget)
            .field("context", &self.context)
            .field(
                "tool_executor",
                &self.tool_executor.as_ref().map(|_| "ToolExecutor"),
            )
            .field("max_iterations", &self.max_iterations)
            .field("synthesis_instruction", &self.synthesis_instruction)
            .field(
                "agent_preferences",
                &self.agent_preferences.as_ref().map(|_| "<configured>"),
            )
            .field("compaction_config", &self.compaction_config)
            .field(
                "compaction_llm",
                &self.compaction_llm.as_ref().map(|_| "LlmProvider"),
            )
            .field(
                "memory_flush",
                &self.memory_flush.as_ref().map(|_| "CompactionMemoryFlush"),
            )
            .field("event_bus", &self.event_bus)
            .field("cancel_token", &self.cancel_token)
            .field("input_channel", &self.input_channel)
            .field("memory_context", &self.memory_context)
            .field("route_advisories", &self.route_advisories.len())
            .field("scratchpad_context", &self.scratchpad_context)
            .field("signal_store", &self.signal_store.is_some())
            .field("iteration_counter", &self.iteration_counter)
            .field(
                "scratchpad_provider",
                &self
                    .scratchpad_provider
                    .as_ref()
                    .map(|_| "ScratchpadProvider"),
            )
            .field("thinking_config", &self.thinking_config)
            .field(
                "runtime_info",
                &self.runtime_info.as_ref().map(|_| "RuntimeInfo"),
            )
            .field(
                "runtime_skill_prompt_revision",
                &self
                    .runtime_skill_prompt_revision
                    .as_ref()
                    .map(|_| "AtomicU64"),
            )
            .finish_non_exhaustive()
    }
}

impl LoopEngineBuilder {
    pub fn budget(mut self, budget: BudgetTracker) -> Self {
        self.budget = Some(budget);
        self
    }

    pub fn context(mut self, context: ContextCompactor) -> Self {
        self.context = Some(context);
        self
    }

    pub fn max_iterations(mut self, max_iterations: u32) -> Self {
        self.max_iterations = Some(max_iterations);
        self
    }

    pub fn tool_executor(mut self, tool_executor: Arc<dyn ToolExecutor>) -> Self {
        self.tool_executor = Some(tool_executor);
        self
    }

    pub fn synthesis_instruction(mut self, synthesis_instruction: impl Into<String>) -> Self {
        self.synthesis_instruction = Some(synthesis_instruction.into());
        self
    }

    pub fn agent_preferences(mut self, agent_preferences: impl Into<String>) -> Self {
        let agent_preferences = agent_preferences.into();
        self.agent_preferences = if agent_preferences.trim().is_empty() {
            None
        } else {
            Some(agent_preferences)
        };
        self
    }

    pub fn compaction_config(mut self, compaction_config: CompactionConfig) -> Self {
        self.compaction_config = Some(compaction_config);
        self
    }

    pub fn compaction_llm(mut self, llm: Arc<dyn LlmProvider>) -> Self {
        self.compaction_llm = Some(llm);
        self
    }

    pub fn memory_flush(mut self, flush: Arc<dyn CompactionMemoryFlush>) -> Self {
        self.memory_flush = Some(flush);
        self
    }

    pub fn event_bus(mut self, event_bus: fx_core::EventBus) -> Self {
        self.event_bus = Some(event_bus);
        self
    }

    pub fn cancel_token(mut self, cancel_token: CancellationToken) -> Self {
        self.cancel_token = Some(cancel_token);
        self
    }

    pub fn input_channel(mut self, input_channel: LoopInputChannel) -> Self {
        self.input_channel = Some(input_channel);
        self
    }

    pub fn memory_context(mut self, memory_context: impl Into<String>) -> Self {
        self.memory_context = normalize_memory_context(memory_context.into());
        self
    }

    pub fn scratchpad_context(mut self, scratchpad_context: impl Into<String>) -> Self {
        let ctx = scratchpad_context.into();
        self.scratchpad_context = if ctx.trim().is_empty() {
            None
        } else {
            Some(ctx)
        };
        self
    }

    pub fn session_memory(mut self, session_memory: Arc<Mutex<SessionMemory>>) -> Self {
        self.session_memory = Some(session_memory);
        self
    }

    pub fn signal_store<S>(mut self, signal_store: S) -> Self
    where
        S: SignalSink + 'static,
    {
        self.signal_store = Some(Box::new(signal_store));
        self
    }

    pub fn route_advisories(mut self, advisories: Vec<RouteAdvisory>) -> Self {
        self.route_advisories = advisories;
        self
    }

    pub fn iteration_counter(mut self, counter: Arc<AtomicU32>) -> Self {
        self.iteration_counter = Some(counter);
        self
    }

    pub fn scratchpad_provider(mut self, provider: Arc<dyn ScratchpadProvider>) -> Self {
        self.scratchpad_provider = Some(provider);
        self
    }

    pub fn error_callback(mut self, cb: StreamCallback) -> Self {
        self.error_callback = Some(cb);
        self
    }

    pub fn thinking_config(mut self, config: fx_llm::ThinkingConfig) -> Self {
        self.thinking_config = Some(config);
        self
    }

    pub fn runtime_info(mut self, runtime_info: Arc<RwLock<RuntimeInfo>>) -> Self {
        self.runtime_info = Some(runtime_info);
        self
    }

    pub fn runtime_skill_prompt_revision(mut self, revision: Arc<AtomicU64>) -> Self {
        self.runtime_skill_prompt_revision = Some(revision);
        self
    }

    pub fn allow_decompose(mut self, enabled: bool) -> Self {
        self.decompose_enabled = Some(enabled);
        self
    }

    pub fn allow_graph_of_thoughts(mut self, enabled: bool) -> Self {
        self.graph_of_thoughts_enabled = Some(enabled);
        self
    }

    fn execution_visibility(mut self, visibility: ExecutionVisibility) -> Self {
        self.execution_visibility = visibility;
        self
    }

    pub fn build(self) -> Result<LoopEngine, LoopError> {
        let budget = required_builder_field(self.budget, "budget")?;
        let context = required_builder_field(self.context, "context")?;
        let tool_executor = required_builder_field(self.tool_executor, "tool_executor")?;
        let max_iterations = required_builder_field(self.max_iterations, "max_iterations")?.max(1);
        let synthesis_instruction =
            required_builder_field(self.synthesis_instruction, "synthesis_instruction")?;
        let compaction_llm_for_extraction = self.compaction_llm.as_ref().map(Arc::clone);
        let (compaction_config, conversation_budget) =
            build_compaction_components(self.compaction_config)?;
        let session_memory = self
            .session_memory
            .unwrap_or_else(|| default_session_memory(compaction_config.model_context_limit));
        configure_session_memory(&session_memory, compaction_config.model_context_limit);

        let mut engine = LoopEngine {
            budget,
            context,
            tool_executor,
            max_iterations,
            iteration_count: 0,
            synthesis_instruction,
            agent_preferences: self.agent_preferences,
            memory_context: self.memory_context,
            execution_context: None,
            session_memory,
            scratchpad_context: self.scratchpad_context,
            signals: SignalCollector::default(),
            signal_store: self.signal_store,
            cancel_token: self.cancel_token,
            input_channel: self.input_channel,
            user_stop_requested: false,
            active_turn_steer: None,
            pending_flow_command: None,
            event_bus: self.event_bus,
            execution_visibility: self.execution_visibility,
            compaction_config,
            conversation_budget,
            compaction_llm: compaction_llm_for_extraction,
            memory_flush: self.memory_flush,
            compaction_last_iteration: Mutex::new(HashMap::new()),
            budget_low_signaled: false,
            consecutive_tool_turns: 0,
            observation_round_tracker: ObservationRoundTracker::default(),
            turn_progress_ledger: TurnProgressLedger::default(),
            recent_signal_cycles: VecDeque::new(),
            last_reasoning_messages: Vec::new(),
            last_signal_feedback_summary: None,
            tool_retry_tracker: RetryTracker::default(),
            last_tool_failure_signal_ids: HashMap::with_capacity(8),
            repeated_tool_failure_tracker: RepeatedToolFailureTracker::default(),
            notify_called_this_cycle: false,
            final_answer_streamed_this_cycle: false,
            transcript_turn_phase: None,
            notify_tool_guidance_enabled: false,
            iteration_counter: self.iteration_counter,
            scratchpad_provider: self.scratchpad_provider,
            tool_call_provider_ids: HashMap::new(),
            pending_tool_result_diagnostics: HashMap::new(),
            pending_tool_result_signals: HashMap::new(),
            pending_tool_response_text: None,
            pending_tool_scope: None,
            pending_turn_commitment: None,
            pending_external_action_target: None,
            pending_external_action_consecutive_failures: 0,
            final_response_blocked_attempts: 0,
            final_response_attempted_tool_activity: false,
            final_response_candidate_truncated: false,
            preflight_route_plan: None,
            route_advisories: self.route_advisories,
            root_turn_contract: None,
            task_contract: None,
            requested_artifact_target: None,
            pending_artifact_write_target: None,
            last_turn_state_progress: None,
            last_activity_progress: None,
            last_emitted_public_progress: None,
            error_callback: self.error_callback,
            thinking_config: self.thinking_config,
            runtime_info: self.runtime_info,
            runtime_skill_prompt_revision: self.runtime_skill_prompt_revision,
            cached_runtime_skill_prompt_revision: 0,
            cached_runtime_skill_prompt_summaries: Vec::new(),
            decompose_enabled: self.decompose_enabled.unwrap_or(true),
            graph_of_thoughts_enabled: self.graph_of_thoughts_enabled.unwrap_or(false),
            direct_inspection_ownership: DirectInspectionOwnership::DetectFromTurn,
            turn_execution_profile: TurnExecutionProfile::Standard,
            bounded_local_phase: BoundedLocalPhase::Discovery,
            bounded_local_recovery_used: false,
            bounded_local_recovery_focus: Vec::new(),
            bounded_local_terminal_reason: None,
            channel_registry: ChannelRegistry::new(),
        };

        if engine.runtime_info.is_some() || engine.runtime_skill_prompt_revision.is_some() {
            engine.refresh_runtime_skill_prompt_summaries();
        }

        Ok(engine)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionContext {
    working_dir: String,
}

impl ExecutionContext {
    pub fn new(working_dir: impl Into<String>) -> Option<Self> {
        let working_dir = working_dir.into();
        let trimmed = working_dir.trim();
        (!trimmed.is_empty()).then(|| Self {
            working_dir: trimmed.to_string(),
        })
    }

    pub fn working_dir(&self) -> &str {
        &self.working_dir
    }
}

fn build_compaction_components(
    config: Option<CompactionConfig>,
) -> Result<(CompactionConfig, ConversationBudget), LoopError> {
    let compaction_config = config.unwrap_or_default();
    compaction_config.validate().map_err(|error| {
        loop_error(
            "init",
            &format!("invalid_compaction_config: {error}"),
            false,
        )
    })?;

    let conversation_budget = ConversationBudget::new(
        compaction_config.model_context_limit,
        compaction_config.slide_threshold,
        compaction_config.reserved_system_tokens,
    );
    Ok((compaction_config, conversation_budget))
}

fn truncate_prompt_text(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn required_builder_field<T>(value: Option<T>, field: &str) -> Result<T, LoopError> {
    value.ok_or_else(|| loop_error("init", &format!("missing_required_field: {field}"), false))
}

fn normalize_memory_context(memory_context: String) -> Option<String> {
    if memory_context.trim().is_empty() {
        None
    } else {
        Some(memory_context)
    }
}

fn default_session_memory(context_limit: usize) -> Arc<Mutex<SessionMemory>> {
    Arc::new(Mutex::new(SessionMemory::with_context_limit(context_limit)))
}

fn configure_session_memory(memory: &Arc<Mutex<SessionMemory>>, context_limit: usize) {
    let mut memory = memory
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    memory.set_context_limit(context_limit);
}

#[derive(Debug, Default, Clone)]
struct CycleState {
    tokens: TokenUsage,
}

#[derive(Debug, Default, Clone)]
struct ObservationRoundTracker {
    consecutive_observation_only_rounds: u16,
    repetitive_rounds: u16,
    seen_observation_fingerprints: HashSet<String>,
}

// Keep the cycle-scoped fingerprint set bounded. Once full, new fingerprints
// degrade to "seen" so extreme fan-out does not retain unbounded state.
const MAX_OBSERVATION_FINGERPRINTS_PER_CYCLE: usize = 256;

impl ObservationRoundTracker {
    fn pressure_rounds(&self) -> u16 {
        self.repetitive_rounds
    }

    fn record_observation_fingerprint(&mut self, fingerprint: String) -> bool {
        if self.seen_observation_fingerprints.contains(&fingerprint) {
            return true;
        }

        if self.seen_observation_fingerprints.len() >= MAX_OBSERVATION_FINGERPRINTS_PER_CYCLE {
            return true;
        }

        self.seen_observation_fingerprints.insert(fingerprint);
        false
    }
}

impl TurnProgressLedger {
    fn seed_explicit_evidence_slot(&mut self, label: impl Into<String>) -> Option<String> {
        let label = sanitize_task_contract_label(&label.into());
        let normalized = normalize_contract_label(&label);
        if normalized.is_empty() {
            return None;
        }
        let id = format!("evidence:required:{normalized}");
        self.explicit_evidence_slot_ids.insert(id.clone());
        self.evidence_slots
            .entry(id.clone())
            .or_insert_with(|| ProgressSlot {
                id: id.clone(),
                kind: ProgressSlotKind::Evidence,
                label,
                normalized_target: Some(normalized),
                explicit: true,
                status: ProgressSlotStatus::Open,
                attempts: 0,
                satisfied_by: Vec::new(),
            });
        Some(id)
    }

    fn seed_discovered_evidence_slot(&mut self, label: impl Into<String>) -> Option<String> {
        const MAX_DISCOVERED_EVIDENCE_SLOTS: usize = 96;

        // Labels are normalized into slot IDs so repeated references collapse
        // into one obligation. The reference extractor strips line suffixes
        // before calling this, so multiple `Foo.swift:line` hits become one
        // "read Foo.swift" follow-up instead of a slot per line.
        let label = sanitize_task_contract_label(&label.into());
        let normalized = normalize_contract_label(&label);
        if normalized.is_empty() {
            return None;
        }
        let id = format!("evidence:discovered:{normalized}");
        if self.evidence_slots.contains_key(&id) {
            return Some(id);
        }
        let discovered_count = self
            .evidence_slots
            .values()
            .filter(|slot| !slot.explicit)
            .count();
        if discovered_count >= MAX_DISCOVERED_EVIDENCE_SLOTS {
            return None;
        }
        self.evidence_slots.insert(
            id.clone(),
            ProgressSlot {
                id: id.clone(),
                kind: ProgressSlotKind::Evidence,
                label,
                normalized_target: Some(normalized),
                explicit: false,
                status: ProgressSlotStatus::Open,
                attempts: 0,
                satisfied_by: Vec::new(),
            },
        );
        Some(id)
    }

    fn seed_task_contract(&mut self, contract: &TaskContract) {
        for input in &contract.inputs {
            self.seed_explicit_evidence_slot(&input.description);
        }
    }

    fn has_explicit_evidence_slots(&self) -> bool {
        !self.explicit_evidence_slot_ids.is_empty()
    }

    fn has_open_discovered_evidence_slots(&self) -> bool {
        // Discovered slots are bounded by MAX_DISCOVERED_EVIDENCE_SLOTS, so a
        // scan keeps the ledger simpler than a counter that must stay in sync
        // with every status transition.
        self.evidence_slots
            .values()
            .any(|slot| !slot.explicit && slot.status == ProgressSlotStatus::Open)
    }

    fn open_explicit_evidence_slots(&self) -> Vec<String> {
        let mut labels = self
            .explicit_evidence_slot_ids
            .iter()
            .filter_map(|id| {
                self.evidence_slots.get(id).and_then(|slot| {
                    (slot.status != ProgressSlotStatus::Satisfied).then(|| slot.label.clone())
                })
            })
            .collect::<Vec<_>>();
        labels.sort();
        labels
    }

    fn matching_open_evidence_slot(&self, observation: &str) -> Option<String> {
        self.evidence_slots
            .values()
            .filter(|slot| slot.status != ProgressSlotStatus::Satisfied)
            .find(|slot| {
                slot.normalized_target
                    .as_deref()
                    .is_some_and(|target| evidence_target_matches_observation(target, observation))
            })
            .map(|slot| slot.id.clone())
    }

    fn slot_is_explicit(&self, slot_id: &str) -> bool {
        self.explicit_evidence_slot_ids.contains(slot_id)
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct TurnProgressLedger {
    evidence_slots: HashMap<String, ProgressSlot>,
    mutation_slots: HashMap<String, ProgressSlot>,
    explicit_evidence_slot_ids: HashSet<String>,
    generic_observation_scope_attempts: HashMap<String, u16>,
    tool_entries: Vec<ToolProgressEntry>,
    seen_retryable_failures: HashSet<String>,
    unproductive_observation_rounds: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProgressSlot {
    id: String,
    kind: ProgressSlotKind,
    label: String,
    normalized_target: Option<String>,
    explicit: bool,
    status: ProgressSlotStatus,
    attempts: u16,
    satisfied_by: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProgressSlotKind {
    Evidence,
    Mutation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProgressSlotStatus {
    Open,
    Satisfied,
    RetryableFailure,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ToolProgressEntry {
    call_id: String,
    tool_name: String,
    class: ToolProgressClass,
    target: Option<String>,
    advances_slot: Option<String>,
    outcome: ToolProgressOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolProgressClass {
    Observation,
    Mutation,
}

impl From<ToolProgressClass> for StreamToolProgressClass {
    fn from(value: ToolProgressClass) -> Self {
        match value {
            ToolProgressClass::Observation => Self::Observation,
            ToolProgressClass::Mutation => Self::Mutation,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolProgressOutcome {
    Advanced,
    Duplicate,
    RetryableFailure,
}

impl From<ToolProgressOutcome> for StreamToolProgressOutcome {
    fn from(value: ToolProgressOutcome) -> Self {
        match value {
            ToolProgressOutcome::Advanced => Self::Advanced,
            ToolProgressOutcome::Duplicate => Self::Duplicate,
            ToolProgressOutcome::RetryableFailure => Self::RetryableFailure,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RepeatedToolFailure {
    key: RepeatedToolFailureKey,
    summary: String,
    last_output: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RepeatedToolFailureKey {
    tool_name: String,
    failure_class: FailureClass,
    kind: RepeatedToolFailureKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RepeatedToolFailureKind {
    MalformedArgumentsJson,
    /// Hash of a normalized non-malformed tool error payload.
    OutputHash(u64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RepeatedToolFailureState {
    failure: RepeatedToolFailure,
    consecutive_failures: u16,
    guidance_injected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RepeatedToolFailureEvent {
    InjectGuidance(RepeatedToolFailureState),
    Trip(RepeatedToolFailureState),
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct RepeatedToolFailureTracker {
    /// Tracks one active failure family at a time. Alternating failure families
    /// intentionally reset the breaker so it only trips on repeated same-family
    /// loops rather than conflating broader instability patterns.
    active: Option<RepeatedToolFailureState>,
}

impl RepeatedToolFailureTracker {
    fn observe_action(
        &mut self,
        results: &[ToolResult],
        threshold: u16,
    ) -> Option<RepeatedToolFailureEvent> {
        let Some((failure, count)) = repeated_tool_failure_from_results(results) else {
            self.active = None;
            return None;
        };

        let mut state = match self.active.take() {
            Some(active) if active.failure.key == failure.key => RepeatedToolFailureState {
                failure,
                consecutive_failures: active.consecutive_failures.saturating_add(count),
                guidance_injected: active.guidance_injected,
            },
            _ => RepeatedToolFailureState {
                failure,
                consecutive_failures: count,
                guidance_injected: false,
            },
        };

        let event = if !state.guidance_injected && state.consecutive_failures >= threshold {
            state.guidance_injected = true;
            Some(RepeatedToolFailureEvent::InjectGuidance(state.clone()))
        } else if state.guidance_injected && state.consecutive_failures > threshold {
            Some(RepeatedToolFailureEvent::Trip(state.clone()))
        } else {
            None
        };

        self.active = Some(state);
        event
    }

    fn clear(&mut self) {
        self.active = None;
    }
}

#[derive(Debug, Clone)]
struct ToolRoundState {
    all_tool_results: Vec<ToolResult>,
    latest_tool_results: Vec<ToolResult>,
    current_calls: Vec<ToolCall>,
    pending_policy_deferred_calls: Vec<ToolCall>,
    continuation_messages: Vec<Message>,
    evidence_messages: Vec<Message>,
    pending_round_notices: Vec<String>,
    accumulated_text: Vec<String>,
    tokens_used: TokenUsage,
    observation_replan_attempted: bool,
    used_observation_tools: bool,
    used_mutation_tools: bool,
}

impl ToolRoundState {
    #[cfg(test)]
    fn new(calls: &[ToolCall], context_messages: &[Message], initial_text: Option<String>) -> Self {
        Self {
            all_tool_results: Vec::new(),
            latest_tool_results: Vec::new(),
            current_calls: calls.to_vec(),
            pending_policy_deferred_calls: Vec::new(),
            continuation_messages: context_messages.to_vec(),
            evidence_messages: Vec::new(),
            pending_round_notices: Vec::new(),
            accumulated_text: initial_text.into_iter().collect(),
            tokens_used: TokenUsage::default(),
            observation_replan_attempted: false,
            used_observation_tools: false,
            used_mutation_tools: false,
        }
    }

    fn new_empty_calls(context_messages: &[Message], initial_text: Option<String>) -> Self {
        Self {
            all_tool_results: Vec::new(),
            latest_tool_results: Vec::new(),
            current_calls: Vec::new(),
            pending_policy_deferred_calls: Vec::new(),
            continuation_messages: context_messages.to_vec(),
            evidence_messages: Vec::new(),
            pending_round_notices: Vec::new(),
            accumulated_text: initial_text.into_iter().collect(),
            tokens_used: TokenUsage::default(),
            observation_replan_attempted: false,
            used_observation_tools: false,
            used_mutation_tools: false,
        }
    }
}

#[derive(Debug)]
struct FollowUpDecomposeContext {
    prior_tool_results: Vec<ToolResult>,
    prior_tokens_used: TokenUsage,
    accumulated_text: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
struct ContextWindowStats {
    message_count: usize,
    token_count: usize,
    usage_ratio: f32,
}

impl ContextWindowStats {
    fn capture(engine: &LoopEngine, messages: &[Message]) -> Self {
        Self {
            message_count: messages.len(),
            token_count: ConversationBudget::estimate_tokens(messages),
            usage_ratio: engine.conversation_budget.usage_ratio(messages),
        }
    }
}

#[derive(Serialize)]
struct ContextOverflowSignalMetadata {
    scope: &'static str,
    messages_before: usize,
    messages_after: usize,
    messages_removed: usize,
    tokens_before: usize,
    tokens_after: usize,
    tokens_evicted: usize,
    usage_ratio_before: f32,
    usage_ratio_after: f32,
}

#[derive(Serialize)]
struct TimeoutSignalMetadata<'a> {
    decision_kind: ControlPlaneDecisionKind,
    tool: &'a str,
    tool_call_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure_class: Option<&'static str>,
    permanent: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    timeout_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    elapsed_ms: Option<u64>,
}

#[derive(Serialize)]
struct TurnStopSignalMetadata<'a> {
    decision_kind: ControlPlaneDecisionKind,
    decision: &'static str,
    failed: bool,
    result_kind: &'static str,
    stop_reason: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    iterations: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    recoverable: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct DecomposeToolArguments {
    #[serde(default)]
    sub_goals: Vec<DecomposeSubGoalArguments>,
    #[serde(default)]
    strategy: Option<AggregationStrategy>,
    #[serde(default)]
    reasoning_mode: Option<DecomposeReasoningMode>,
    #[serde(default)]
    got_branches: Option<usize>,
    #[serde(default)]
    got_criteria: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum DecomposeReasoningMode {
    Standard,
    GotChain,
    GotTree,
    GotGraph,
    GotConsensus,
}

#[derive(Debug, Deserialize)]
struct DecomposeSubGoalArguments {
    description: String,
    #[serde(default)]
    required_tools: Vec<String>,
    #[serde(default)]
    expected_output: Option<String>,
    #[serde(default)]
    complexity_hint: Option<ComplexityHint>,
}

impl From<DecomposeSubGoalArguments> for SubGoal {
    fn from(value: DecomposeSubGoalArguments) -> Self {
        SubGoal::with_definition_of_done(
            value.description,
            value.required_tools,
            value.expected_output.as_deref(),
            value.complexity_hint,
        )
    }
}

const REASONING_OUTPUT_TOKEN_HEURISTIC: u64 = 192;
const TOOL_SYNTHESIS_TOKEN_HEURISTIC: u64 = 320;
const REASONING_MAX_OUTPUT_TOKENS: u32 = 4096;
const TERMINAL_SYNTHESIS_MAX_OUTPUT_TOKENS: u32 = 8192;
const REASONING_TEMPERATURE: f32 = 0.2;
const MAX_CONTINUATION_ATTEMPTS: u32 = 3;
const DEFAULT_LLM_ACTION_COST_CENTS: u64 = 2;
const DECOMPOSE_TOOL_NAME: &str = "decompose";
const NOTIFY_TOOL_NAME: &str = "notify";
const NOTIFICATION_DEFAULT_TITLE: &str = "Fawx";
const DECOMPOSE_TOOL_DESCRIPTION: &str = "Break a complex task into 2-4 high-level sub-goals. Each sub-goal should be substantial enough to justify its own execution context. Do NOT create more than 5 sub-goals. Prefer fewer, broader goals over many narrow ones. Only use this for tasks that genuinely cannot be handled with direct tool calls.";
const MAX_SUB_GOALS: usize = 5;
const DECOMPOSITION_DEPTH_LIMIT_RESPONSE: &str =
    "I can't decompose this request further because the recursion depth limit was reached.";
const REASONING_SYSTEM_PROMPT: &str = "You are Fawx, a capable personal assistant. \
Answer the user directly and concisely. \
Never introduce yourself, greet the user, or add preamble; just answer. \
Use tools when you need information not already in the conversation \
(current time, file contents, directory listings, search results, memory, etc.). \
When the user's request relates to an available tool's purpose, prefer calling the tool \
over answering from general knowledge. \
While a tool-using turn is still in progress, assistant text immediately before tool calls \
is live work narration for the UI. Use that channel to give a concise, natural update about \
what you are checking, comparing, or trying to resolve next. Do not mention raw tool names, \
JSON, or implementation mechanics in that narration. \
After using tools, final answers should answer directly. In final answers, never narrate what \
tools you used, describe the process, or comment on tool output metadata. \
Do not hedge with qualifiers or reference tool mechanics. \
Avoid filler openers like \"I notice\", \"I can see that\", \"Based on the results\", \
\"It appears that\", \"Let me\", or \"I aim to\". Just answer the question. \
If the user makes a statement (not a question), acknowledge it naturally and briefly. \
If a tool call stores data (like memory_write), confirm the action in one short sentence. You are Fawx, a TUI-first agentic engine built in Rust. You were created by the Fawx team. Your architecture separates an immutable safety kernel from a loadable intelligence layer: the kernel enforces hard security boundaries that you cannot override at runtime. You are designed to be self-extending through a WASM plugin system. \
Your source code is at ~/fawx. Your config is at ~/.fawx/config.toml. \
Your data (conversations, memory) is at the data_dir set in config. \
Your conversation history is stored as JSONL files in the data directory. \
For multi-step tasks, use the decompose tool to break work into parallel sub-goals. \
Each sub-goal gets its own execution budget. \
Do not burn through your tool retry limit in a single sequential loop \
; decompose first, then execute. \
";

const SIMPLE_AGENT_SYSTEM_PROMPT: &str = "You are Fawx, a code-capable agent running in a real workspace. \
Answer by doing the requested work with the available tools. \
For requests to fix, resolve, implement, update, review, test, commit, push, or open a pull request, \
recommendations are not a resolution: inspect the workspace, make the concrete change, verify it when practical, \
and then report the outcome. \
Call tools whenever you need file contents, repository state, command output, or external action status. \
After a tool call, use the tool result as the source of truth and continue from it. \
Do not ask the user to confirm the obvious next step for an active implementation request. \
Stop only when the user-facing request is complete, impossible, or blocked by missing credentials/permissions/input. \
When blocked, state the exact blocker and the last concrete evidence. \
Keep final answers concise and specific.";

const SIMPLE_AGENT_FINAL_RESPONSE_DIRECTIVE: &str =
    "The tool-use budget for this turn is exhausted. \
Do not call more tools. Answer now from the gathered evidence. \
If the work is incomplete, report the concrete blocker and the evidence you gathered.";

const TERMINAL_SYNTHESIS_SYSTEM_PROMPT: &str = "You are Fawx in terminal answer mode. \
Tool execution is closed for this turn. \
You cannot call tools, request function calls, plan another tool step, or ask the runtime to continue execution. \
Use only the conversation text and observed tool-result evidence already provided in this request. \
If the evidence is incomplete, state the limitation directly and answer with the supported findings. \
Do not mention raw tool names, JSON, function calls, or provider mechanics. \
Do not write future-looking progress narration such as \"I will check\" or \"next I need to\". \
Produce the final user-facing answer now.";

const TOOL_CONTINUATION_DIRECTIVE: &str = "\n\nYou are continuing after one or more tool calls. \
Treat successful tool results as the primary evidence for your next response. \
If the existing tool results already answer the user's request, answer immediately instead of calling more tools. \
Only call another tool when the current results are missing critical information, are contradictory, or the user explicitly asked you to refresh/re-check something. \
If you do call more tools, first write one short live work narration update that states what the prior evidence showed and what specific gap you are closing next. \
Never repeat an identical successful tool call in the same cycle. Reuse the result you already have and answer from it.";

const TASK_CONTRACT_DECLARATION_DIRECTIVE: &str = "\n\nIf you are about to gather information with tools before you can answer, start that first tool-using response with an internal block exactly like:\nTask plan:\n- <required input 1>\n- <required input 2>\n\nList only the concrete inputs, files, documents, or records you still need. Do not list tools or implementation steps. After the block, request only the tools needed to satisfy those inputs.";

const NOTIFY_TOOL_GUIDANCE: &str = "\n\nYou have a `notify` tool that sends native OS notifications to the user. \
Use it when you complete a task that took multiple steps, have important results to share, or finish background work the user may not be watching. \
Do not use it for simple one-turn replies, trivial acknowledgements, or every tool completion. \
If you do not call `notify`, a generic notification may fire automatically for multi-step tasks when the app is not in focus. \
Prefer calling `notify` yourself when you can provide a more meaningful summary.";

const MEMORY_INSTRUCTION: &str = "\n\nYou have persistent memory across sessions. \
Use memory_write to save important facts about the user, their preferences, \
and project context. Use memory_read to recall specific details. \
Memories survive restart; write anything worth remembering. \
You lose all context between sessions. Your memory tools are how future-you \
understands what present-you built. Write what you wish past-you had left behind.";

const BUDGET_LOW_WRAP_UP_DIRECTIVE: &str = "You are running low on budget. \
Do not call any tools. Do not decompose. \
Summarize what you have accomplished and what remains undone. Be concise.";
const TERMINAL_SYNTHESIS_DIRECTIVE: &str = "\n\nTerminal response contract: answer now from the provided evidence only. Do not call tools or request further inspection.";
const BUDGET_EXHAUSTED_FALLBACK_RESPONSE: &str = "I reached my iteration limit.";
const TOOL_TURN_NUDGE: &str = "You've been working for several steps without responding. Share your progress with the user before continuing.";
const TOOL_ROUND_PROGRESS_NUDGE: &str = "You've been calling tools for several rounds without providing a response. Share your progress with the user now. If you have enough information to answer, do so immediately instead of calling more tools.";
const OBSERVATION_ONLY_TOOL_ROUND_NUDGE: &str = "You have spent multiple tool rounds only gathering information. Stop doing more read-only research unless it is absolutely necessary. If you have enough context, switch to implementation-side tools now. Otherwise, respond with what you learned, what remains blocked, and what input you need.";
const OBSERVATION_ONLY_MUTATION_REPLAN_DIRECTIVE: &str = "Read-only tool calls were blocked after repeated observation-only rounds. Do not request any more read-only tools. Use the remaining mutation/build/install tools now if you have enough context to proceed. If you still cannot proceed, answer with the current findings and the specific blocker.";
const OBSERVATION_ONLY_CALL_BLOCK_REASON: &str = "read-only inspection is disabled after repeated observation-only rounds; use a mutating/build/install step or answer with current findings";
const MALFORMED_TOOL_ARGUMENTS_ERROR_MARKER: &str = "arguments could not be parsed as valid JSON";
const DIRECT_INSPECTION_TASK_DIRECTIVE: &str = "\n\nThis turn is a direct local inspection request. Do not plan. Do not decompose. Use only the provided observation tools to inspect the explicit local path the user named. If the tool results answer the request, answer directly from that evidence. Do not broaden the task into repo research, code modification, testing, command execution, or web work.";
const DIRECT_INSPECTION_READ_LOCAL_PATH_PHASE_DIRECTIVE: &str = "\n\nDirect inspection focus: read_local_path.\nUse `read_file` to inspect the explicit local path the user requested. Do not call unrelated tools or reopen the task as general research.";
const BOUNDED_LOCAL_TASK_DIRECTIVE: &str = "\n\nThis turn is a bounded local workspace task. Do not use decompose. Do not reopen broad research. Prefer at most one read-only discovery pass, then move directly to the concrete local edit, write, command, or focused test needed to complete the task.";
const DIRECT_TOOL_TASK_DIRECTIVE: &str = "\n\nThis turn is a simple direct-tool request. Do not plan. Do not decompose. Use the one relevant utility tool immediately, then answer directly from its result. Do not call unrelated tools or do extra research unless the direct tool fails.";
const BOUNDED_LOCAL_DISCOVERY_PHASE_DIRECTIVE: &str = "\n\nBounded local workflow phase: discovery.\nOnly use local discovery tools (`search_text`, `read_file`, `list_directory`). Do not use `run_command` in this phase. For code-edit tasks, do not move on to mutation until you have grounded the edit target by reading the most relevant file directly. Gather only the context needed to identify and read that file, then move to the concrete code change.";
const BOUNDED_LOCAL_MUTATION_PHASE_DIRECTIVE: &str = "\n\nBounded local workflow phase: mutation.\nDo not do more discovery. Use `write_file` or `edit_file` now to make one concrete local code change. If you are blocked, state the precise blocker instead of reopening inspection.";
const BOUNDED_LOCAL_RECOVERY_PHASE_DIRECTIVE: &str = "\n\nBounded local workflow phase: recovery.\nThe first concrete edit attempt failed. Use at most one tiny targeted `read_file` or `search_text` step to gather the exact context needed for the retry, then go straight back to the edit. Do not call `run_command` or reopen broad inspection.";
const BOUNDED_LOCAL_VERIFICATION_PHASE_DIRECTIVE: &str = "\n\nBounded local workflow phase: verification.\nDo not reopen discovery. Use at most one focused verification step such as a targeted `run_command` test or a confirming `read_file`, then respond with the result.";
const BOUNDED_LOCAL_TERMINAL_PHASE_DIRECTIVE: &str = "\n\nBounded local workflow phase: terminal.\nDo not call any tools. Summarize what changed, what you verified, and what remains blocked.";
const BOUNDED_LOCAL_DISCOVERY_BLOCK_REASON: &str =
    "bounded local discovery only allows search_text, read_file, or list_directory before editing";
const BOUNDED_LOCAL_MUTATION_BLOCK_REASON: &str =
    "bounded local mutation requires a concrete write/edit step before more inspection or verification";
const BOUNDED_LOCAL_RECOVERY_BLOCK_REASON: &str =
    "bounded local recovery only allows one tiny targeted read/search pass after a failed edit attempt";
const BOUNDED_LOCAL_VERIFICATION_BLOCK_REASON: &str =
    "bounded local verification allows only one focused test/read after a code change";
const BOUNDED_LOCAL_MUTATION_NOOP_BLOCK_REASON: &str =
    "bounded local mutation requires a meaningful repo-relevant edit; noop or scratch writes do not count";
const BOUNDED_LOCAL_VERIFICATION_DISCOVERY_BLOCK_REASON: &str =
    "bounded local verification only allows focused confirmation commands; use read_file/search_text for repo inspection instead of shell discovery";
const TOOL_ERROR_RELAY_PREFIX: &str = "The following tools failed. Report these errors to the user before continuing with additional tool calls:";
const DECOMPOSITION_RESULTS_PREFIX: &str = "Task decomposition results:";

fn tool_error_relay_directive(failed_tools: &[(&str, &str)]) -> String {
    let details: Vec<String> = failed_tools
        .iter()
        .map(|(name, error)| format!("- Tool '{}' failed with: {}", name, error))
        .collect();
    format!("{}\n{}", TOOL_ERROR_RELAY_PREFIX, details.join("\n"))
}

pub(super) fn signal_metadata_value<T: Serialize>(metadata: T) -> serde_json::Value {
    serde_json::to_value(metadata).expect("signal metadata should serialize")
}

fn turn_stop_signal_metadata(result: &LoopResult) -> TurnStopSignalMetadata<'_> {
    match result {
        LoopResult::Complete { iterations, .. } => TurnStopSignalMetadata {
            decision_kind: ControlPlaneDecisionKind::TurnStop,
            decision: "completed",
            failed: false,
            result_kind: "complete",
            stop_reason: "complete",
            iterations: Some(*iterations),
            recoverable: None,
        },
        LoopResult::BudgetExhausted { iterations, .. } => TurnStopSignalMetadata {
            decision_kind: ControlPlaneDecisionKind::TurnStop,
            decision: "failed",
            failed: true,
            result_kind: "budget_exhausted",
            stop_reason: "budget_exhausted",
            iterations: Some(*iterations),
            recoverable: None,
        },
        LoopResult::Incomplete {
            reason, iterations, ..
        } => TurnStopSignalMetadata {
            decision_kind: ControlPlaneDecisionKind::TurnStop,
            decision: "failed",
            failed: true,
            result_kind: "incomplete",
            stop_reason: reason.as_str(),
            iterations: Some(*iterations),
            recoverable: None,
        },
        LoopResult::UserStopped { iterations, .. } => TurnStopSignalMetadata {
            decision_kind: ControlPlaneDecisionKind::TurnStop,
            decision: "stopped",
            failed: false,
            result_kind: "user_stopped",
            stop_reason: "user_stopped",
            iterations: Some(*iterations),
            recoverable: None,
        },
        LoopResult::Error {
            message,
            recoverable,
            ..
        } => TurnStopSignalMetadata {
            decision_kind: ControlPlaneDecisionKind::TurnStop,
            decision: "failed",
            failed: true,
            result_kind: "error",
            stop_reason: message.as_str(),
            iterations: None,
            recoverable: Some(*recoverable),
        },
    }
}

/// Maximum time to wait for a best-effort summary during emergency compaction.
const EMERGENCY_SUMMARY_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(500);

impl LoopEngine {
    /// Create a loop engine builder.
    pub fn builder() -> LoopEngineBuilder {
        LoopEngineBuilder::default()
    }

    /// Attach an fx-core event bus for inter-component progress events.
    pub fn set_event_bus(&mut self, bus: fx_core::EventBus) {
        self.event_bus = Some(bus);
    }

    fn public_event_bus(&self) -> Option<&fx_core::EventBus> {
        match self.execution_visibility {
            ExecutionVisibility::Public => self.event_bus.as_ref(),
            ExecutionVisibility::Internal => None,
        }
    }

    fn public_event_bus_clone(&self) -> Option<fx_core::EventBus> {
        match self.execution_visibility {
            ExecutionVisibility::Public => self.event_bus.clone(),
            ExecutionVisibility::Internal => None,
        }
    }

    /// Attach a cancellation token for cooperative cancellation.
    pub fn set_cancel_token(&mut self, token: CancellationToken) {
        self.cancel_token = Some(token);
    }

    /// Attach a user-input channel for bare-word commands.
    pub fn set_input_channel(&mut self, channel: LoopInputChannel) {
        self.input_channel = Some(channel);
    }

    pub fn set_synthesis_instruction(&mut self, instruction: String) -> Result<(), LoopError> {
        let trimmed = instruction.trim();
        if trimmed.is_empty() {
            return Err(loop_error(
                "configure",
                "synthesis instruction cannot be empty",
                true,
            ));
        }

        self.synthesis_instruction = trimmed.to_string();
        Ok(())
    }

    /// Set memory context for system prompt injection.
    pub fn set_memory_context(&mut self, context: String) {
        self.memory_context = normalize_memory_context(context);
    }

    pub fn set_execution_context(&mut self, working_dir: impl Into<String>) {
        self.execution_context = ExecutionContext::new(working_dir);
    }

    pub fn replace_session_memory(&self, memory: SessionMemory) -> SessionMemory {
        let mut replacement = memory;
        replacement.set_context_limit(self.compaction_config.model_context_limit);
        let mut stored = match self.session_memory.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        std::mem::replace(&mut *stored, replacement)
    }

    pub fn session_memory_snapshot(&self) -> SessionMemory {
        match self.session_memory.lock() {
            Ok(memory) => memory.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    pub fn set_route_advisories(&mut self, advisories: Vec<RouteAdvisory>) {
        self.route_advisories = advisories;
    }

    pub fn set_scratchpad_context(&mut self, context: String) {
        self.scratchpad_context = if context.trim().is_empty() {
            None
        } else {
            Some(context)
        };
    }

    /// Set the extended thinking configuration for completion requests.
    pub fn set_thinking_config(&mut self, config: Option<fx_llm::ThinkingConfig>) {
        self.thinking_config = config;
    }

    /// Attach runtime info so request builders can surface loaded skill summaries.
    pub fn set_runtime_info(&mut self, runtime_info: Arc<RwLock<RuntimeInfo>>) {
        self.runtime_info = Some(runtime_info);
        self.refresh_runtime_skill_prompt_summaries();
    }

    /// Clear the cached prompt summaries after mutating the runtime snapshot.
    pub fn invalidate_runtime_skill_prompt_cache(&mut self) {
        self.refresh_runtime_skill_prompt_summaries();
    }

    /// Return a reference to the channel registry.
    pub fn channel_registry(&self) -> &ChannelRegistry {
        &self.channel_registry
    }

    /// Return a mutable reference to the channel registry.
    pub fn channel_registry_mut(&mut self) -> &mut ChannelRegistry {
        &mut self.channel_registry
    }

    pub fn conversation_budget_ref(&self) -> &ConversationBudget {
        &self.conversation_budget
    }

    /// Update the context limit when the active model changes.
    /// Rebuilds the conversation budget from the updated config to prevent drift.
    pub fn update_context_limit(&mut self, new_limit: usize) {
        self.compaction_config.model_context_limit = new_limit;
        self.conversation_budget = ConversationBudget::new(
            self.compaction_config.model_context_limit,
            self.compaction_config.slide_threshold,
            self.compaction_config.reserved_system_tokens,
        );
        configure_session_memory(&self.session_memory, new_limit);
    }

    /// Synchronise the shared iteration counter and refresh scratchpad context.
    ///
    /// Called at each iteration boundary so `ScratchpadSkill` stamps entries
    /// with the correct iteration and the model sees up-to-date scratchpad
    /// state in the system prompt.
    fn refresh_iteration_state(&mut self) {
        if let Some(counter) = &self.iteration_counter {
            counter.store(self.iteration_count, Ordering::Relaxed);
        }
        if let Some(provider) = &self.scratchpad_provider {
            provider.compact_if_needed(self.iteration_count);
            let rendered = provider.render_for_context();
            self.set_scratchpad_context(rendered);
        }
    }

    fn request_build_context(&mut self) -> RequestBuildContext<'_> {
        self.sync_runtime_skill_prompt_summaries();
        let signal_feedback_summary = self.current_signal_feedback_summary();
        let skill_prompt_summaries = self.runtime_skill_prompt_summaries();
        RequestBuildContext::new(
            self.memory_context.as_deref(),
            self.scratchpad_context.as_deref(),
            self.thinking_config.clone(),
            self.notify_tool_guidance_enabled,
        )
        .with_execution_context(self.execution_context.as_ref())
        .with_agent_preferences(self.agent_preferences.as_deref())
        .with_skill_prompt_summaries(skill_prompt_summaries)
        .with_signal_feedback_summary(signal_feedback_summary)
        .with_prompt_cache_affinity(self.prompt_cache_affinity())
    }

    fn request_build_context_without_signal_feedback(&mut self) -> RequestBuildContext<'_> {
        self.sync_runtime_skill_prompt_summaries();
        let skill_prompt_summaries = self.runtime_skill_prompt_summaries();
        RequestBuildContext::new(
            self.memory_context.as_deref(),
            self.scratchpad_context.as_deref(),
            self.thinking_config.clone(),
            self.notify_tool_guidance_enabled,
        )
        .with_execution_context(self.execution_context.as_ref())
        .with_agent_preferences(self.agent_preferences.as_deref())
        .with_skill_prompt_summaries(skill_prompt_summaries)
        .with_prompt_cache_affinity(self.prompt_cache_affinity())
    }

    fn prompt_cache_affinity(&self) -> Option<PromptCacheAffinity> {
        let session_id = self.signal_store.as_ref()?.session_id()?;
        PromptCacheAffinity::new(format!("fawx:{session_id}"))
    }

    fn current_signal_feedback_summary(&mut self) -> Option<String> {
        if self.budget.state() == BudgetState::Low {
            return None;
        }

        let config = &self.budget.config().signal_feedback;
        let summary = SignalSummarizer::summarize(
            &self.signal_feedback_signals(usize::from(config.lookback_cycles)),
            SignalSummaryContext {
                budget_state: self.budget.state(),
                budget_remaining_percent: Some(self.budget.signal_feedback_remaining_percent()),
                observation_pressure: self.current_observation_pressure(),
            },
            config,
        )?;

        if self.iteration_count > 1
            && self.last_signal_feedback_summary.as_deref() == Some(summary.as_str())
        {
            return None;
        }

        self.last_signal_feedback_summary = Some(summary.clone());
        Some(summary)
    }

    fn signal_feedback_signals(&self, lookback_cycles: usize) -> Vec<Signal> {
        let current_signals = self.signals.signals();
        let recent_cycles = self
            .recent_signal_cycles
            .iter()
            .rev()
            .take(lookback_cycles)
            .collect::<Vec<_>>();
        let capacity =
            current_signals.len() + recent_cycles.iter().map(|cycle| cycle.len()).sum::<usize>();
        let mut signals = Vec::with_capacity(capacity);

        for cycle in recent_cycles.into_iter().rev() {
            signals.extend(cycle.iter().cloned());
        }
        signals.extend(current_signals.iter().cloned());
        signals
    }

    fn current_observation_pressure(&self) -> Option<ObservationPressure> {
        let termination = self.current_termination_config();
        let rounds_limit = ToolStrippingAfterNudge::from_config_value(
            termination.observation_only_round_strip_after_nudge,
        )
        .pressure_limit(termination.observation_only_round_nudge_after)?;

        (rounds_limit > 0).then_some(ObservationPressure {
            rounds_used: self.observation_round_tracker.pressure_rounds(),
            rounds_limit,
        })
    }

    fn record_signal_feedback_cycle(&mut self, signals: &[Signal]) {
        let lookback_cycles = usize::from(self.budget.config().signal_feedback.lookback_cycles);
        if lookback_cycles == 0 {
            self.recent_signal_cycles.clear();
            return;
        }
        if signals.is_empty() {
            return;
        }

        self.recent_signal_cycles.push_back(signals.to_vec());
        while self.recent_signal_cycles.len() > lookback_cycles {
            self.recent_signal_cycles.pop_front();
        }
    }

    fn sync_runtime_skill_prompt_summaries(&mut self) {
        if let Some(revision) = &self.runtime_skill_prompt_revision {
            let current = revision.load(Ordering::Acquire);
            if current != self.cached_runtime_skill_prompt_revision {
                self.refresh_runtime_skill_prompt_summaries();
            }
        }
    }

    fn runtime_skill_prompt_summaries(&self) -> &[SkillPromptSummary] {
        self.cached_runtime_skill_prompt_summaries.as_slice()
    }

    fn refresh_runtime_skill_prompt_summaries(&mut self) {
        self.cached_runtime_skill_prompt_summaries = self.collect_runtime_skill_prompt_summaries();
        self.cached_runtime_skill_prompt_revision = self
            .runtime_skill_prompt_revision
            .as_ref()
            .map(|revision| revision.load(Ordering::Acquire))
            .unwrap_or(0);
    }

    fn collect_runtime_skill_prompt_summaries(&self) -> Vec<SkillPromptSummary> {
        let Some(runtime_info) = &self.runtime_info else {
            return Vec::new();
        };

        let info = match runtime_info.read() {
            Ok(info) => info,
            Err(error) => {
                tracing::warn!(error = %error, "runtime info lock poisoned");
                return Vec::new();
            }
        };

        info.skills
            .iter()
            .filter_map(|skill| {
                let description = skill.description.as_deref()?.trim();
                if description.is_empty() || skill.name.trim().is_empty() {
                    return None;
                }
                Some(SkillPromptSummary::new(
                    skill.name.clone(),
                    description.to_string(),
                ))
            })
            .collect()
    }

    fn collect_runtime_routing_tools(&self) -> Vec<ToolRoutingSummary> {
        let Some(runtime_info) = &self.runtime_info else {
            return Vec::new();
        };

        let info = match runtime_info.read() {
            Ok(info) => info,
            Err(error) => {
                tracing::warn!(error = %error, "runtime info lock poisoned");
                return Vec::new();
            }
        };

        info.skills
            .iter()
            .flat_map(|skill| skill.routing_tools.clone())
            .collect()
    }

    fn apply_preflight_route_tool_surface(
        &self,
        tools: Vec<ToolDefinition>,
    ) -> Vec<ToolDefinition> {
        let Some(route_plan) = &self.preflight_route_plan else {
            return tools;
        };
        let active_route = route_plan.current_route();
        let allowed: HashSet<&str> = active_route.tool_names.iter().map(String::as_str).collect();
        let mut tool_map = tools
            .into_iter()
            .filter(|tool| allowed.contains(tool.name.as_str()))
            .map(|tool| (tool.name.clone(), tool))
            .collect::<HashMap<_, _>>();

        active_route
            .tool_names
            .iter()
            .filter_map(|tool_name| tool_map.remove(tool_name))
            .collect()
    }

    fn consume_preflight_route_plan(&mut self, reason: &str) {
        let Some(route_plan) = self.preflight_route_plan.take() else {
            return;
        };
        let active_route = route_plan.current_route().clone();
        self.emit_signal(
            LoopStep::Act,
            SignalKind::Trace,
            "consumed preflight route plan",
            serde_json::json!({
                "decision_kind": ControlPlaneDecisionKind::PreflightRoute,
                "decision": "consumed",
                "reason": reason,
                "resource": &route_plan.resource,
                "active_route": active_route,
            }),
        );
    }

    pub fn synthesis_instruction(&self) -> &str {
        &self.synthesis_instruction
    }

    /// Return status metrics for loop diagnostics.
    pub fn status(&self, current_time_ms: u64) -> LoopStatus {
        LoopStatus {
            iteration_count: self.iteration_count,
            max_iterations: self.max_iterations,
            llm_calls_used: self.budget.llm_calls_used(),
            tool_invocations_used: self.budget.tool_invocations_used(),
            tokens_used: self.budget.tokens_used(),
            cost_cents_used: self.budget.cost_cents_used(),
            remaining: self.budget.remaining(current_time_ms),
        }
    }

    fn emit_signal(
        &mut self,
        step: LoopStep,
        kind: SignalKind,
        message: impl Into<String>,
        metadata: serde_json::Value,
    ) -> u64 {
        self.emit_structured_signal(Signal::new(
            step,
            kind,
            message,
            metadata,
            current_time_ms(),
        ))
    }

    #[must_use = "capture the assigned signal ID when you need causal links; discard it explicitly otherwise"]
    fn emit_structured_signal(&mut self, signal: Signal) -> u64 {
        self.signals.emit(signal)
    }

    fn remember_tool_failure_signal(&mut self, call: &ToolCall, signal_id: u64) {
        self.last_tool_failure_signal_ids
            .insert(ToolCallKey::from_call(call), signal_id);
    }

    fn clear_tool_failure_signal(&mut self, call: &ToolCall) {
        self.last_tool_failure_signal_ids
            .remove(&ToolCallKey::from_call(call));
    }

    fn cause_id_for_tool_call(&self, call: &ToolCall) -> Option<u64> {
        self.last_tool_failure_signal_ids
            .get(&ToolCallKey::from_call(call))
            .copied()
    }

    fn finalize_result(&mut self, result: LoopResult) -> LoopResult {
        self.emit_turn_stop_signal(&result);
        self.emit_cache_stats_signal();
        let signals = self.signals.drain_all();
        self.persist_cycle_signals(&signals);
        self.record_signal_feedback_cycle(&signals);
        attach_signals(result, signals)
    }

    fn finalize_error_result(&mut self, error: &LoopError) {
        self.emit_signal(
            loop_step_for_error_stage(&error.stage),
            SignalKind::Blocked,
            "loop error",
            serde_json::json!({
                "stage": error.stage,
                "reason": error.reason,
                "recoverable": error.recoverable,
            }),
        );
        self.emit_turn_stop_signal(&LoopResult::Error {
            message: error.reason.clone(),
            recoverable: error.recoverable,
            signals: Vec::new(),
        });
        self.emit_cache_stats_signal();
        let signals = self.signals.drain_all();
        self.persist_cycle_signals(&signals);
        self.record_signal_feedback_cycle(&signals);
    }

    fn emit_turn_stop_signal(&mut self, result: &LoopResult) {
        self.emit_signal(
            LoopStep::Synthesize,
            SignalKind::Trace,
            "loop turn terminal status",
            signal_metadata_value(turn_stop_signal_metadata(result)),
        );
    }

    fn persist_cycle_signals(&self, signals: &[Signal]) {
        let Some(store) = self.signal_store.as_ref() else {
            return;
        };
        if signals.is_empty() {
            return;
        }

        let session_id = store.session_id().unwrap_or("unknown");

        if let Err(error) = store.append(signals).and_then(|()| store.flush()) {
            tracing::warn!(
                error = %error,
                signal_count = signals.len(),
                session_id,
                "signal store append failed; continuing without persistent signal history"
            );
        }
    }

    // Emit a user-visible error through the out-of-band error callback.
    // Used for errors outside the streaming cycle (compaction, background ops).
    fn emit_background_error(
        &self,
        category: ErrorCategory,
        message: impl Into<String>,
        recoverable: bool,
    ) {
        self.emit_stream_event(StreamEvent::Error {
            category,
            message: message.into(),
            recoverable,
        });
    }

    fn emit_stream_event(&self, event: StreamEvent) {
        if let Some(cb) = &self.error_callback {
            cb(event);
        }
    }

    fn emit_cache_stats_signal(&mut self) {
        let Some(stats) = self.tool_executor.cache_stats() else {
            return;
        };

        let total = stats.hits.saturating_add(stats.misses);
        let hit_rate = if total == 0 {
            0.0
        } else {
            stats.hits as f64 / total as f64
        };

        self.emit_signal(
            LoopStep::Act,
            SignalKind::Performance,
            "tool cache stats",
            serde_json::json!({
                "hits": stats.hits,
                "misses": stats.misses,
                "entries": stats.entries,
                "evictions": stats.evictions,
                "hit_rate": hit_rate,
            }),
        );
    }

    /// Run one full loop cycle.
    pub async fn run_cycle(
        &mut self,
        perception: PerceptionSnapshot,
        llm: &dyn LlmProvider,
    ) -> Result<LoopResult, LoopError> {
        self.run_cycle_streaming(perception, llm, None).await
    }

    pub async fn run_cycle_streaming(
        &mut self,
        perception: PerceptionSnapshot,
        llm: &dyn LlmProvider,
        stream_callback: Option<StreamCallback>,
    ) -> Result<LoopResult, LoopError> {
        let mut engine = ErrorCallbackGuard::install(self, stream_callback.clone());
        let result = engine
            .run_cycle_streaming_inner(perception, llm, stream_callback.as_ref())
            .await;
        if let Err(error) = &result {
            engine.finalize_error_result(error);
        }
        result
    }

    async fn run_cycle_streaming_inner(
        &mut self,
        perception: PerceptionSnapshot,
        llm: &dyn LlmProvider,
        stream_callback: Option<&StreamCallback>,
    ) -> Result<LoopResult, LoopError> {
        self.prepare_cycle();
        self.notify_tool_guidance_enabled = stream_callback.is_some();
        let mut state = CycleState::default();
        let stream = stream_callback.map_or_else(CycleStream::disabled, CycleStream::enabled);

        // Multi-pass: loops until model stops using tools.
        self.iteration_count = 1;
        self.refresh_iteration_state();

        if let Some(result) = self.budget_terminal(ActionCost::default(), None) {
            return Ok(self.finish_streaming_result(result, stream));
        }
        if let Some(result) = self.check_cancellation(None) {
            return Ok(self.finish_streaming_result(result, stream));
        }

        self.emit_loop_phase(stream, Phase::Perceive);
        let mut processed = self.perceive(&perception).await?;
        self.clear_turn_contract_control_plane();
        loop {
            let reason_cost = self.estimate_reasoning_cost(&processed);
            if let Some(result) = self.budget_terminal(reason_cost, None) {
                return Ok(self.finish_streaming_result(result, stream));
            }
            self.emit_loop_phase(stream, Phase::Reason);
            let reasoning_messages = build_simple_agent_messages(&processed);
            let response = self.reason_simple(&processed, llm, stream).await?;
            state
                .tokens
                .accumulate(response_usage_or_estimate(&response, &reasoning_messages));
            self.budget.record(&reason_cost);

            let response_text =
                normalize_response_text(&extract_readable_text(&extract_response_text(&response)));
            if let Some(result) = self.check_cancellation(
                (!response_text.trim().is_empty()).then_some(response_text.clone()),
            ) {
                return Ok(self.finish_streaming_result(result, stream));
            }

            if response.tool_calls.is_empty() {
                self.clear_tool_response_state();
                let followed_tool_round = self.consecutive_tool_turns > 0;
                let text = response_text;
                self.emit_loop_phase(
                    stream,
                    if followed_tool_round {
                        Phase::Synthesize
                    } else {
                        Phase::Act
                    },
                );
                self.emit_decision_signals(&Decision::Respond(text.clone()));
                if text.trim().is_empty() {
                    if followed_tool_round {
                        self.emit_signal(
                            LoopStep::Act,
                            SignalKind::Trace,
                            "tool continuation returned empty text",
                            serde_json::json!({ "iterations": self.iteration_count }),
                        );
                        self.emit_loop_phase(stream, Phase::Synthesize);
                        let final_response = self
                            .reason_simple_final_response(&processed, llm, stream)
                            .await?;
                        let final_messages = build_simple_agent_messages(&processed);
                        state.tokens.accumulate(response_usage_or_estimate(
                            &final_response,
                            &final_messages,
                        ));
                        let final_text = normalize_response_text(&extract_readable_text(
                            &extract_response_text(&final_response),
                        ));
                        self.emit_decision_signals(&Decision::Respond(final_text.clone()));
                        if !final_text.trim().is_empty() {
                            return Ok(self.finish_streaming_result(
                                LoopResult::Complete {
                                    response: final_text,
                                    iterations: self.iteration_count,
                                    tokens_used: state.tokens,
                                    signals: Vec::new(),
                                },
                                stream,
                            ));
                        }
                    }
                    self.consecutive_tool_turns = 0;
                    return Ok(self.finish_streaming_result(
                        LoopResult::Incomplete {
                            partial_response: None,
                            reason: "model returned no final response".to_string(),
                            iterations: self.iteration_count,
                            signals: Vec::new(),
                        },
                        stream,
                    ));
                }

                match self.guard_root_turn_terminal_completion(ActionTerminal::Complete {
                    response: text.clone(),
                }) {
                    ActionNextStep::Finish(ActionTerminal::Complete { response }) => {
                        self.consecutive_tool_turns = 0;
                        return Ok(self.finish_streaming_result(
                            LoopResult::Complete {
                                response,
                                iterations: self.iteration_count,
                                tokens_used: state.tokens,
                                signals: Vec::new(),
                            },
                            stream,
                        ));
                    }
                    ActionNextStep::Finish(ActionTerminal::Incomplete {
                        partial_response,
                        reason,
                    }) => {
                        self.consecutive_tool_turns = 0;
                        return Ok(self.finish_streaming_result(
                            LoopResult::Incomplete {
                                partial_response,
                                reason,
                                iterations: self.iteration_count,
                                signals: Vec::new(),
                            },
                            stream,
                        ));
                    }
                    ActionNextStep::Continue(continuation) => {
                        self.apply_pending_turn_commitment(&continuation, &[]);
                        append_continuation_context(&mut processed.context_window, &continuation);
                        self.consecutive_tool_turns = 0;
                        self.iteration_count += 1;
                        self.refresh_iteration_state();
                        continue;
                    }
                }
            }

            self.capture_tool_response_state(&response);
            let calls = response.tool_calls.clone();
            let tool_decision = Decision::UseTools(calls.clone());
            let action_cost = ActionCost {
                llm_calls: 0,
                tool_invocations: calls.len() as u32,
                tokens: 0,
                cost_cents: 0,
            };
            if let Some(result) = self.budget_terminal(action_cost, None) {
                if self.consecutive_tool_turns > 0 {
                    return Ok(self.finish_budget_exhausted(result, llm, stream).await);
                }
                return Ok(self.finish_streaming_result(result, stream));
            }
            self.budget.record(&action_cost);
            self.emit_decision_signals(&tool_decision);
            self.emit_loop_phase(stream, Phase::Act);
            let batch = self
                .execute_tool_calls_batch_with_stream(&calls, stream)
                .await?;
            self.emit_action_signals(&calls, &batch.results);
            self.consecutive_tool_turns = self.consecutive_tool_turns.saturating_add(1);

            let provider_ids = self.tool_call_provider_ids.clone();
            let mut evidence_messages = Vec::new();
            record_tool_round_messages(
                &mut processed.context_window,
                &mut evidence_messages,
                &calls,
                &provider_ids,
                &batch.results,
            )?;
            self.clear_tool_response_state();
            self.compact_simple_agent_context_if_needed(&mut processed)
                .await?;
            if !batch.blocked.is_empty() {
                let blocked_guidance = batch
                    .blocked
                    .iter()
                    .map(|blocked| {
                        blocked_tool_message(
                            &blocked.call.name,
                            &blocked.reason,
                            blocked.guidance.as_deref(),
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                processed.context_window.push(Message::system(format!(
                    "Blocked tool calls this round:\n{blocked_guidance}"
                )));
            }

            if let Some(result) = self.check_cancellation(None) {
                return Ok(self.finish_streaming_result(result, stream));
            }

            if !batch.blocked.is_empty() && batch.results.iter().all(|result| !result.success) {
                self.emit_signal(
                    LoopStep::Act,
                    SignalKind::Blocked,
                    "all requested tool calls were blocked",
                    serde_json::json!({"blocked_count": batch.blocked.len()}),
                );
                let final_reason_cost = self.estimate_reasoning_cost(&processed);
                if let Some(result) = self.budget_terminal(final_reason_cost, None) {
                    return Ok(self.finish_budget_exhausted(result, llm, stream).await);
                }
                self.emit_loop_phase(stream, Phase::Reason);
                let final_response = self
                    .reason_simple_final_response(&processed, llm, stream)
                    .await?;
                let final_messages = build_simple_agent_messages(&processed);
                state
                    .tokens
                    .accumulate(response_usage_or_estimate(&final_response, &final_messages));
                self.budget.record(&final_reason_cost);
                self.clear_tool_response_state();
                self.consecutive_tool_turns = 0;

                let raw = extract_response_text(&final_response);
                let text = normalize_response_text(&extract_readable_text(&raw));
                self.emit_decision_signals(&Decision::Respond(text.clone()));
                if text.trim().is_empty() {
                    return Ok(self.finish_streaming_result(
                        LoopResult::Incomplete {
                            partial_response: None,
                            reason: "all requested tool calls were blocked before a usable final response was produced".to_string(),
                            iterations: self.iteration_count,
                            signals: Vec::new(),
                        },
                        stream,
                    ));
                }
                return Ok(self.finish_streaming_result(
                    LoopResult::Complete {
                        response: text,
                        iterations: self.iteration_count,
                        tokens_used: state.tokens,
                        signals: Vec::new(),
                    },
                    stream,
                ));
            }

            // Tools were used. Check max before incrementing so the
            // reported iteration count is accurate (not inflated by 1).
            if self.iteration_count >= self.max_iterations {
                self.emit_signal(
                    LoopStep::Reason,
                    SignalKind::Trace,
                    "simple agent loop reached tool iteration limit; requesting final response",
                    serde_json::json!({
                        "iterations": self.iteration_count,
                        "max_iterations": self.max_iterations,
                    }),
                );
                self.emit_loop_phase(stream, Phase::Synthesize);
                let final_response = self
                    .reason_simple_final_response(&processed, llm, stream)
                    .await?;
                let final_messages = build_simple_agent_messages(&processed);
                state
                    .tokens
                    .accumulate(response_usage_or_estimate(&final_response, &final_messages));
                self.clear_tool_response_state();
                self.consecutive_tool_turns = 0;

                let text = normalize_response_text(&extract_readable_text(&extract_response_text(
                    &final_response,
                )));
                self.emit_decision_signals(&Decision::Respond(text.clone()));
                if text.trim().is_empty() {
                    return Ok(self.finish_streaming_result(
                        LoopResult::Incomplete {
                            partial_response: None,
                            reason: "iteration limit reached before a usable final response was produced".to_string(),
                            iterations: self.iteration_count,
                            signals: Vec::new(),
                        },
                        stream,
                    ));
                }

                return Ok(self.finish_streaming_result(
                    LoopResult::Complete {
                        response: text,
                        iterations: self.iteration_count,
                        tokens_used: state.tokens,
                        signals: Vec::new(),
                    },
                    stream,
                ));
            }
            self.iteration_count += 1;

            self.refresh_iteration_state();
        }
    }

    async fn compact_simple_agent_context_if_needed(
        &mut self,
        processed: &mut ProcessedPerception,
    ) -> Result<(), LoopError> {
        let compacted = self
            .compaction()
            .compact_if_needed(
                &processed.context_window,
                CompactionScope::ToolContinuation,
                self.iteration_count,
            )
            .await?;
        if let Cow::Owned(messages) = compacted {
            processed.context_window = messages;
        }
        Ok(())
    }

    /// Handle BudgetExhausted results with optional forced synthesis.
    async fn finish_budget_exhausted(
        &mut self,
        result: LoopResult,
        llm: &dyn LlmProvider,
        stream: CycleStream<'_>,
    ) -> LoopResult {
        let result = match result {
            LoopResult::BudgetExhausted {
                partial_response,
                iterations,
                signals,
            } => {
                let synthesized = if self.budget.config().termination.synthesize_on_exhaustion {
                    let reasoning_messages = std::mem::take(&mut self.last_reasoning_messages);
                    self.forced_synthesis_turn(llm, &reasoning_messages, stream)
                        .await
                } else {
                    None
                };
                if let Some(response) = synthesized.as_deref() {
                    if let Some(block) = self.root_turn_completion_block_for_response(response) {
                        let reason = Self::root_turn_incomplete_reason(&block);
                        let partial_response = Self::root_turn_contract_incomplete_response(
                            &block,
                            synthesized.as_deref(),
                        );
                        self.emit_signal(
                            LoopStep::Synthesize,
                            SignalKind::Friction,
                            "forced synthesis did not satisfy root turn completion contract",
                            serde_json::json!({
                                "missing_response_sections": &block.missing_response_sections,
                                "pending_mutation_work": &block.pending_mutation_work,
                                "pending_artifact_paths": &block.pending_artifact_paths,
                                "pending_external_actions": &block.pending_external_actions,
                            }),
                        );
                        return self.finish_streaming_result(
                            LoopResult::Incomplete {
                                partial_response,
                                reason,
                                iterations,
                                signals,
                            },
                            stream,
                        );
                    }
                }
                LoopResult::BudgetExhausted {
                    partial_response: Some(Self::resolve_budget_exhausted_response(
                        synthesized,
                        partial_response,
                    )),
                    iterations,
                    signals,
                }
            }
            other => other,
        };
        self.finish_streaming_result(result, stream)
    }

    async fn finish_iteration_limit(
        &mut self,
        llm: &dyn LlmProvider,
        synthesis_context: &[Message],
        tokens_used: TokenUsage,
        stream: CycleStream<'_>,
    ) -> LoopResult {
        let reason = "iteration limit reached before a usable final response was produced";
        let synthesized = self
            .forced_synthesis_turn(llm, synthesis_context, stream)
            .await;
        let result = if let Some(response) = synthesized.filter(|text| !text.trim().is_empty()) {
            if let Some(block) = self.root_turn_completion_block_for_response(&response) {
                let reason = Self::root_turn_incomplete_reason(&block);
                let partial_response =
                    Self::root_turn_contract_incomplete_response(&block, Some(&response));
                self.emit_signal(
                    LoopStep::Synthesize,
                    SignalKind::Friction,
                    "forced synthesis did not satisfy root turn completion contract",
                    serde_json::json!({
                        "missing_response_sections": &block.missing_response_sections,
                        "pending_mutation_work": &block.pending_mutation_work,
                        "pending_artifact_paths": &block.pending_artifact_paths,
                        "pending_external_actions": &block.pending_external_actions,
                    }),
                );
                LoopResult::Incomplete {
                    partial_response,
                    reason,
                    iterations: self.iteration_count,
                    signals: Vec::new(),
                }
            } else {
                LoopResult::Complete {
                    response,
                    iterations: self.iteration_count,
                    tokens_used,
                    signals: Vec::new(),
                }
            }
        } else {
            LoopResult::Incomplete {
                partial_response: None,
                reason: reason.to_string(),
                iterations: self.iteration_count,
                signals: Vec::new(),
            }
        };

        self.finish_streaming_result(result, stream)
    }

    fn emit_loop_phase(&mut self, stream: CycleStream<'_>, phase: Phase) {
        match phase {
            Phase::Perceive => {
                self.emit_transcript_phase_boundary(stream, TranscriptTurnPhase::CollectingWork)
            }
            Phase::Synthesize => {
                self.emit_transcript_phase_boundary(stream, TranscriptTurnPhase::Summarizing)
            }
            Phase::Reason | Phase::Act => {}
        }
        stream.phase(phase);
    }

    fn finish_streaming_result(
        &mut self,
        result: LoopResult,
        stream: CycleStream<'_>,
    ) -> LoopResult {
        self.maybe_emit_completion_notification(&result, stream);
        if let Some((category, message, recoverable)) = result.stream_terminal_error() {
            stream.emit_error(category, message, recoverable);
        }
        self.emit_completed_summary_if_available(stream);
        if let Some(response) = result.stream_final_answer_response() {
            self.emit_transcript_phase_boundary(stream, TranscriptTurnPhase::Finalizing);
            self.emit_terminal_final_answer_if_needed(stream, &response);
        }
        // `Completed` is the terminal transcript boundary, emitted from the
        // shared finish path so every outcome (complete, incomplete, or
        // cancelled) gives clients one last phase marker before `Done`.
        self.emit_transcript_phase_boundary(stream, TranscriptTurnPhase::Completed);
        stream.done_result(&result);
        self.finalize_result(result)
    }

    fn emit_transcript_phase_boundary(
        &mut self,
        stream: CycleStream<'_>,
        phase: TranscriptTurnPhase,
    ) {
        if self.transcript_turn_phase == Some(phase) {
            return;
        }

        if self
            .transcript_turn_phase
            .is_some_and(|current_phase| phase < current_phase)
        {
            tracing::debug!(
                current = ?self.transcript_turn_phase,
                requested = ?phase,
                "ignoring transcript phase boundary backtrack"
            );
            return;
        }

        self.transcript_turn_phase = Some(phase);
        stream.phase_boundary(phase);
    }

    fn emit_completed_summary_if_available(&mut self, stream: CycleStream<'_>) {
        let Some(summary) =
            completed_work_summary_from_progress_entries(&self.turn_progress_ledger.tool_entries)
        else {
            return;
        };
        self.emit_transcript_phase_boundary(stream, TranscriptTurnPhase::Summarizing);
        stream.completed_summary(&summary);
    }

    pub(super) fn mark_final_answer_streamed(&mut self) {
        self.final_answer_streamed_this_cycle = true;
    }

    fn emit_terminal_final_answer_if_needed(&mut self, stream: CycleStream<'_>, response: &str) {
        if self.final_answer_streamed_this_cycle || response.trim().is_empty() {
            return;
        }

        stream.final_answer(response);
        self.final_answer_streamed_this_cycle = true;
    }

    fn apply_decomposition_terminal_fallback(
        &self,
        terminal: ActionTerminal,
        last_context_message: Option<&Message>,
    ) -> ActionTerminal {
        match terminal {
            ActionTerminal::Complete { response } if response.trim().is_empty() => {
                let fallback = last_context_message
                    .map(message_content_to_text)
                    .filter(|text| is_decomposition_results_message(text));
                if let Some(response) = fallback {
                    ActionTerminal::Complete { response }
                } else {
                    ActionTerminal::Complete { response }
                }
            }
            other => other,
        }
    }

    fn loop_result_from_action_terminal(
        &self,
        terminal: ActionTerminal,
        tokens_used: TokenUsage,
    ) -> LoopResult {
        match terminal {
            ActionTerminal::Complete { response } => LoopResult::Complete {
                response,
                iterations: self.iteration_count,
                tokens_used,
                signals: Vec::new(),
            },
            ActionTerminal::Incomplete {
                partial_response,
                reason,
            } => LoopResult::Incomplete {
                partial_response,
                reason,
                iterations: self.iteration_count,
                signals: Vec::new(),
            },
        }
    }

    fn maybe_emit_completion_notification(&self, result: &LoopResult, stream: CycleStream<'_>) {
        let LoopResult::Complete { iterations, .. } = result else {
            return;
        };
        if *iterations <= 1 || self.notify_called_this_cycle {
            return;
        }

        stream.notification(
            NOTIFICATION_DEFAULT_TITLE,
            format!("Task complete ({iterations} steps)"),
        );
    }

    /// Drain the input channel into typed pending state.
    ///
    /// Flow commands are preserved for the next cancellation/control check while
    /// steering is latched independently so every model boundary can apply the
    /// latest user guidance for the rest of the current turn without swallowing
    /// Stop/Abort/Wait/Resume commands.
    fn drain_user_input_to_state(&mut self) {
        let Some(channel) = self.input_channel.as_mut() else {
            return;
        };
        let mut highest = self.pending_flow_command.take();
        let mut status_requested = false;
        let mut latest_steer: Option<String> = None;

        while let Some(cmd) = channel.try_recv() {
            match cmd {
                LoopCommand::Steer(text) => latest_steer = Some(text),
                LoopCommand::StatusQuery => status_requested = true,
                flow_cmd => highest = Some(prioritize_flow_command(highest, flow_cmd)),
            }
        }

        if let Some(steer) = latest_steer.and_then(normalize_turn_steer) {
            self.active_turn_steer = Some(steer);
        }
        self.pending_flow_command = highest;
        if status_requested {
            self.publish_system_status();
        }
    }

    /// Drain the input channel and return the highest-priority flow command.
    ///
    /// Priority ordering: `Abort` > `Stop` > `Wait/Resume` > `StatusQuery` > `Steer`.
    /// `StatusQuery` publishes an internal status message and does not alter loop flow.
    /// `Steer` stores the latest steer text for the current turn's model boundaries.
    fn check_user_input(&mut self) -> Option<LoopCommand> {
        self.drain_user_input_to_state();
        self.pending_flow_command.take()
    }

    fn apply_turn_steer_to_request(
        &mut self,
        request: &mut CompletionRequest,
        context: StreamingRequestContext,
    ) {
        self.drain_user_input_to_state();
        let Some(steer) = self.active_turn_steer.as_deref() else {
            return;
        };

        let directive = turn_steering_guidance_message(steer);
        tracing::info!(
            stage = context.stage(),
            steer_chars = steer.chars().count(),
            "applying current-turn steering guidance to model request"
        );
        request.messages.push(Message::user(directive));
    }

    fn publish_system_status(&self) {
        let Some(bus) = self.public_event_bus() else {
            return;
        };
        let status = self.status(current_time_ms());
        let message = format_system_status_message(&status);
        let _ = bus.publish(InternalMessage::SystemStatus { message });
    }

    /// Check both the cancellation token and input channel.
    fn check_cancellation(&mut self, partial: Option<String>) -> Option<LoopResult> {
        if self.user_stop_requested {
            self.user_stop_requested = false;
            return Some(self.user_stopped_result(partial, "user stopped", "input_channel"));
        }

        if self.cancellation_token_triggered() {
            return Some(self.user_stopped_result(partial, "user cancelled", "cancellation_token"));
        }

        if self.consume_stop_or_abort_command() {
            return Some(self.user_stopped_result(partial, "user stopped", "input_channel"));
        }

        None
    }

    fn user_stopped_result(
        &mut self,
        partial: Option<String>,
        message: &str,
        source: &str,
    ) -> LoopResult {
        self.emit_signal(
            LoopStep::Act,
            SignalKind::Blocked,
            message,
            serde_json::json!({ "source": source }),
        );
        LoopResult::UserStopped {
            partial_response: partial,
            iterations: self.iteration_count,
            signals: Vec::new(),
        }
    }

    fn consume_stop_or_abort_command(&mut self) -> bool {
        matches!(
            self.check_user_input(),
            Some(LoopCommand::Stop | LoopCommand::Abort)
        )
    }

    fn prepare_cycle(&mut self) {
        self.iteration_count = 0;
        if let Some(counter) = &self.iteration_counter {
            counter.store(0, Ordering::Relaxed);
        }
        self.budget.reset(current_time_ms());
        self.signals.clear();
        self.user_stop_requested = false;
        self.active_turn_steer = None;
        self.pending_flow_command = None;
        self.budget_low_signaled = false;
        self.consecutive_tool_turns = 0;
        self.observation_round_tracker = ObservationRoundTracker::default();
        self.turn_progress_ledger = TurnProgressLedger::default();
        self.last_signal_feedback_summary = None;
        self.last_reasoning_messages.clear();
        self.tool_retry_tracker.clear();
        self.last_tool_failure_signal_ids.clear();
        self.repeated_tool_failure_tracker.clear();
        self.notify_called_this_cycle = false;
        self.final_answer_streamed_this_cycle = false;
        self.transcript_turn_phase = None;
        self.notify_tool_guidance_enabled = false;
        self.tool_call_provider_ids.clear();
        self.pending_tool_result_diagnostics.clear();
        self.pending_tool_result_signals.clear();
        self.pending_tool_response_text = None;
        self.pending_tool_scope = None;
        self.pending_turn_commitment = None;
        self.pending_external_action_target = None;
        self.final_response_blocked_attempts = 0;
        self.final_response_attempted_tool_activity = false;
        self.final_response_candidate_truncated = false;
        self.preflight_route_plan = None;
        self.root_turn_contract = None;
        self.task_contract = None;
        self.requested_artifact_target = None;
        self.pending_artifact_write_target = None;
        self.last_turn_state_progress = None;
        self.last_activity_progress = None;
        self.last_emitted_public_progress = None;
        self.turn_execution_profile = TurnExecutionProfile::Standard;
        self.bounded_local_phase = BoundedLocalPhase::Discovery;
        self.bounded_local_recovery_used = false;
        self.bounded_local_recovery_focus.clear();
        self.bounded_local_terminal_reason = None;
        if let Some(token) = &self.cancel_token {
            token.reset();
        }
        self.tool_executor.clear_cache();
    }

    fn clear_turn_contract_control_plane(&mut self) {
        self.turn_progress_ledger = TurnProgressLedger::default();
        self.pending_tool_scope = None;
        self.pending_turn_commitment = None;
        self.pending_external_action_target = None;
        self.pending_external_action_consecutive_failures = 0;
        self.final_response_blocked_attempts = 0;
        self.final_response_attempted_tool_activity = false;
        self.final_response_candidate_truncated = false;
        self.root_turn_contract = None;
        self.task_contract = None;
        self.requested_artifact_target = None;
        self.pending_artifact_write_target = None;
        self.turn_execution_profile = TurnExecutionProfile::Standard;
        self.bounded_local_phase = BoundedLocalPhase::Discovery;
        self.bounded_local_recovery_used = false;
        self.bounded_local_recovery_focus.clear();
        self.bounded_local_terminal_reason = None;
    }

    fn update_tool_turns(&mut self, action: &ActionResult) {
        if action.has_tool_activity() {
            self.consecutive_tool_turns = self.consecutive_tool_turns.saturating_add(1);
        } else {
            self.consecutive_tool_turns = 0;
        }
    }

    fn apply_repeated_tool_failure_policy(
        &mut self,
        action: &mut ActionResult,
        action_partial: Option<&str>,
    ) -> Option<LoopResult> {
        if let ActionNextStep::Finish(terminal) = &action.next_step {
            if matches!(terminal, ActionTerminal::Complete { .. }) {
                self.repeated_tool_failure_tracker.clear();
            }
            return None;
        }

        let threshold = self.repeated_failure_streak_limit();
        let event = self
            .repeated_tool_failure_tracker
            .observe_action(&action.tool_results, threshold)?;

        match event {
            RepeatedToolFailureEvent::InjectGuidance(state) => {
                let directive = render_repeated_tool_failure_directive(&state);
                self.emit_signal(
                    LoopStep::Act,
                    SignalKind::Friction,
                    format!(
                        "injecting repeated failure guidance for '{}'",
                        state.failure.key.tool_name
                    ),
                    serde_json::json!({
                        "tool": state.failure.key.tool_name.as_str(),
                        "consecutive_failures": state.consecutive_failures,
                        "failure_summary": state.failure.summary.as_str(),
                    }),
                );
                if let ActionNextStep::Continue(continuation) = &mut action.next_step {
                    append_continuation_system_message(continuation, directive);
                }
                None
            }
            RepeatedToolFailureEvent::Trip(state) => {
                let reason = repeated_tool_failure_terminal_reason(&state);
                self.emit_signal(
                    LoopStep::Act,
                    SignalKind::Blocked,
                    &reason,
                    serde_json::json!({
                        "tool": state.failure.key.tool_name.as_str(),
                        "consecutive_failures": state.consecutive_failures,
                        "failure_summary": state.failure.summary.as_str(),
                    }),
                );
                Some(LoopResult::Incomplete {
                    partial_response: repeated_tool_failure_partial_response(
                        action_partial,
                        &state,
                    ),
                    reason,
                    iterations: self.iteration_count,
                    signals: Vec::new(),
                })
            }
        }
    }

    fn recorded_action_cost(&self, action: &ActionResult) -> Option<ActionCost> {
        (!action.has_tool_activity()).then(|| self.action_cost_from_result(action))
    }

    fn side_effect_tool_definitions(&self) -> Vec<ToolDefinition> {
        self.tool_executor
            .tool_definitions()
            .into_iter()
            .filter(|tool| {
                self.tool_executor.cacheability(&tool.name) == ToolCacheability::SideEffect
            })
            .collect()
    }

    fn apply_pending_tool_scope(&self, tools: Vec<ToolDefinition>) -> Vec<ToolDefinition> {
        if self.turn_execution_profile.owns_tool_surface() {
            return tools;
        }
        match self.pending_tool_scope.as_ref() {
            None | Some(ContinuationToolScope::Full) => tools,
            Some(ContinuationToolScope::NoTools) => Vec::new(),
            Some(ContinuationToolScope::MutationOnly) => tools
                .into_iter()
                .filter(|tool| {
                    self.tool_executor.cacheability(&tool.name) == ToolCacheability::SideEffect
                })
                .collect(),
            Some(ContinuationToolScope::Only(names)) => {
                let allowed: HashSet<&str> = names.iter().map(String::as_str).collect();
                tools
                    .into_iter()
                    .filter(|tool| allowed.contains(tool.name.as_str()))
                    .collect()
            }
        }
    }

    fn apply_pending_turn_commitment(
        &mut self,
        continuation: &ActionContinuation,
        tool_results: &[ToolResult],
    ) {
        let previous_commitment = self.pending_turn_commitment.clone();
        let previous_scope = self.pending_tool_scope.clone();
        let previous_artifact_target = self.pending_artifact_write_target.clone();
        let artifact_completed = previous_artifact_target
            .as_deref()
            .is_some_and(|target| artifact_write_completed(target, tool_results));
        let next_commitment = continuation
            .turn_commitment
            .clone()
            .or_else(|| previous_commitment.clone());
        let next_scope = if let Some(scope) = continuation.next_tool_scope.clone() {
            Some(scope)
        } else if continuation.turn_commitment.is_some() {
            commitment_tool_scope(next_commitment.as_ref())
        } else {
            commitment_tool_scope(next_commitment.as_ref()).or(previous_scope.clone())
        };
        let next_artifact_target = continuation.artifact_write_target.clone().or_else(|| {
            if artifact_completed {
                None
            } else {
                previous_artifact_target.clone()
            }
        });

        self.pending_turn_commitment = next_commitment;
        self.pending_tool_scope = next_scope;
        self.pending_artifact_write_target = next_artifact_target;

        let previous_finalize = matches!(
            previous_commitment,
            Some(TurnCommitment::FinalizeResponse(_))
        );
        let current_finalize = matches!(
            self.pending_turn_commitment,
            Some(TurnCommitment::FinalizeResponse(_))
        );
        if current_finalize != previous_finalize {
            self.final_response_blocked_attempts = 0;
            self.final_response_attempted_tool_activity = false;
            self.final_response_candidate_truncated = false;
        }

        if artifact_completed {
            self.emit_signal(
                LoopStep::Act,
                SignalKind::Success,
                "requested artifact write completed; releasing artifact gate",
                serde_json::json!({
                    "path": previous_artifact_target,
                }),
            );
        }

        if self.pending_turn_commitment != previous_commitment {
            if let Some(commitment) = &self.pending_turn_commitment {
                self.emit_signal(
                    LoopStep::Act,
                    SignalKind::Trace,
                    "continuation committed next turn state",
                    turn_commitment_metadata(commitment),
                );
            }
        } else if self.pending_turn_commitment.is_some() {
            self.emit_signal(
                LoopStep::Act,
                SignalKind::Trace,
                "continuation preserved committed next turn state",
                serde_json::json!({
                    "variant": "preserved",
                }),
            );
        }

        if self.pending_tool_scope != previous_scope {
            if let Some(scope) = &self.pending_tool_scope {
                let scope_metadata = match scope {
                    ContinuationToolScope::Full => serde_json::json!({
                        "mode": "full",
                    }),
                    ContinuationToolScope::NoTools => serde_json::json!({
                        "mode": "no_tools",
                    }),
                    ContinuationToolScope::MutationOnly => serde_json::json!({
                        "mode": "mutation_only",
                    }),
                    ContinuationToolScope::Only(names) => serde_json::json!({
                        "mode": "named",
                        "tools": names,
                    }),
                };
                self.emit_signal(
                    LoopStep::Act,
                    SignalKind::Trace,
                    "continuation constrained the next tool surface",
                    serde_json::json!({ "scope": scope_metadata }),
                );
            }
        } else if let Some(scope) = &self.pending_tool_scope {
            let scope_metadata = match scope {
                ContinuationToolScope::Full => serde_json::json!({
                    "mode": "full",
                }),
                ContinuationToolScope::NoTools => serde_json::json!({
                    "mode": "no_tools",
                }),
                ContinuationToolScope::MutationOnly => serde_json::json!({
                    "mode": "mutation_only",
                }),
                ContinuationToolScope::Only(names) => serde_json::json!({
                    "mode": "named",
                    "tools": names,
                }),
            };
            self.emit_signal(
                LoopStep::Act,
                SignalKind::Trace,
                "continuation preserved the next tool surface constraint",
                serde_json::json!({ "scope": scope_metadata }),
            );
        }

        if self.pending_artifact_write_target != previous_artifact_target {
            if let Some(path) = &self.pending_artifact_write_target {
                self.emit_signal(
                    LoopStep::Act,
                    SignalKind::Trace,
                    "continuation gated the next turn on an artifact write",
                    serde_json::json!({
                        "path": path,
                    }),
                );
            }
        } else if self.pending_artifact_write_target.is_some() {
            self.emit_signal(
                LoopStep::Act,
                SignalKind::Trace,
                "continuation preserved the artifact write gate",
                serde_json::json!({
                    "path": self.pending_artifact_write_target,
                }),
            );
        }
    }

    fn record_root_turn_contract_progress(&mut self, calls: &[ToolCall], results: &[ToolResult]) {
        let typed_completed_actions = completed_external_actions_from_diagnostics(
            results,
            &self.pending_tool_result_diagnostics,
        );
        let completed_mutation_work = mutation_work_completed(calls, results);
        let Some(contract) = self.root_turn_contract.as_mut() else {
            return;
        };

        let mut satisfied_paths = Vec::new();
        let mut satisfied_mutation_work = Vec::new();
        let mut satisfied_external_actions = Vec::new();
        let mut satisfied_external_action_kinds = Vec::new();
        for deliverable in &mut contract.deliverables {
            match deliverable {
                RootTurnDeliverable::MutationWork { label, satisfied } => {
                    if *satisfied || !completed_mutation_work {
                        continue;
                    }
                    *satisfied = true;
                    satisfied_mutation_work.push(label.clone());
                }
                RootTurnDeliverable::ArtifactWrite { path, satisfied } => {
                    if *satisfied || !artifact_write_completed(path, results) {
                        continue;
                    }
                    *satisfied = true;
                    satisfied_paths.push(path.clone());
                }
                RootTurnDeliverable::ExternalAction {
                    kind,
                    label,
                    satisfied,
                } => {
                    if *satisfied
                        || !external_action_completed(
                            *kind,
                            calls,
                            results,
                            &typed_completed_actions,
                        )
                    {
                        continue;
                    }
                    *satisfied = true;
                    satisfied_external_actions.push(label.clone());
                    satisfied_external_action_kinds.push(*kind);
                }
                RootTurnDeliverable::ResponseSection { .. } => {}
            }
        }

        if !satisfied_mutation_work.is_empty() {
            self.emit_signal(
                LoopStep::Act,
                SignalKind::Success,
                "root turn mutation deliverable satisfied",
                serde_json::json!({ "deliverables": satisfied_mutation_work }),
            );
        }
        if !satisfied_paths.is_empty() {
            self.emit_signal(
                LoopStep::Act,
                SignalKind::Success,
                "root turn artifact deliverable satisfied",
                serde_json::json!({ "paths": satisfied_paths }),
            );
        }
        if !satisfied_external_actions.is_empty() {
            self.clear_pending_external_action_target_if_satisfied(
                &satisfied_external_action_kinds,
            );
            self.emit_signal(
                LoopStep::Act,
                SignalKind::Success,
                "root turn external action deliverable satisfied",
                serde_json::json!({ "actions": satisfied_external_actions }),
            );
        }

        // Track consecutive failures against the pending external action gate.
        // If the agent attempted the action but it failed, increment the counter
        // so the gate can lift after repeated failures and allow recovery.
        if let Some(target_kind) = self.pending_external_action_target {
            if !satisfied_external_action_kinds.contains(&target_kind) {
                let attempted_and_failed =
                    calls.iter().zip(results.iter()).any(|(call, result)| {
                        tool_call_may_complete_external_action(call, target_kind) && !result.success
                    });
                if attempted_and_failed {
                    let threshold = self.external_action_gate_failure_lift_threshold();
                    self.pending_external_action_consecutive_failures = self
                        .pending_external_action_consecutive_failures
                        .saturating_add(1)
                        .min(threshold);
                    self.emit_signal(
                        LoopStep::Act,
                        SignalKind::Friction,
                        "pending external action attempt failed",
                        serde_json::json!({
                            "action": external_action_label(target_kind),
                            "consecutive_failures": self.pending_external_action_consecutive_failures,
                        }),
                    );
                }
            }
        }
    }

    fn record_task_contract_progress(&mut self, calls: &[ToolCall], results: &[ToolResult]) {
        let Some((satisfied_inputs, input_count, transitioned)) = ({
            let Some(contract) = self.task_contract.as_mut() else {
                return;
            };
            if contract.phase != TaskPhase::Gathering {
                return;
            }

            let observations = task_contract_observations(&*self.tool_executor, calls, results);
            let mut satisfied_inputs = Vec::new();
            for input in &mut contract.inputs {
                if input.satisfied
                    || !observations
                        .iter()
                        .any(|observation| task_contract_matches_observation(input, observation))
                {
                    continue;
                }
                input.satisfied = true;
                satisfied_inputs.push(input.description.clone());
            }

            let transitioned = contract.inputs.iter().all(|input| input.satisfied);
            if transitioned {
                contract.phase = TaskPhase::Synthesizing;
            }

            Some((satisfied_inputs, contract.inputs.len(), transitioned))
        }) else {
            return;
        };

        if !satisfied_inputs.is_empty() {
            self.emit_signal(
                LoopStep::Act,
                SignalKind::Success,
                "task contract input satisfied",
                serde_json::json!({ "inputs": satisfied_inputs }),
            );
        }

        if transitioned {
            self.emit_signal(
                LoopStep::Act,
                SignalKind::Trace,
                "task contract gathering complete; entering synthesizing phase",
                serde_json::json!({
                    "input_count": input_count,
                }),
            );
        }
    }

    fn mark_task_contract_complete(&mut self) {
        let should_emit = match self.task_contract.as_mut() {
            Some(contract) if contract.phase != TaskPhase::Complete => {
                contract.phase = TaskPhase::Complete;
                true
            }
            _ => false,
        };
        if !should_emit {
            return;
        }
        self.emit_signal(
            LoopStep::Act,
            SignalKind::Success,
            "task contract completed",
            serde_json::json!({}),
        );
    }

    fn finish_root_turn_terminal_response(&mut self, response: String) -> ActionNextStep {
        self.mark_task_contract_complete();
        ActionNextStep::Finish(ActionTerminal::Complete { response })
    }

    fn guard_root_turn_terminal_completion(&mut self, terminal: ActionTerminal) -> ActionNextStep {
        let ActionTerminal::Complete { response } = terminal else {
            return ActionNextStep::Finish(terminal);
        };
        if response.trim().is_empty() {
            let raw_response_chars = response.chars().count();
            let raw_response_preview = response.chars().take(160).collect::<String>();
            if self.active_finalize_response().is_some() {
                self.final_response_attempted_tool_activity = false;
                self.final_response_candidate_truncated = false;
            }
            self.emit_signal(
                LoopStep::Act,
                SignalKind::Friction,
                "empty terminal response rejected",
                serde_json::json!({
                    "reason": "model returned no visible assistant response",
                    "raw_response_chars": raw_response_chars,
                    "raw_response_preview": raw_response_preview,
                }),
            );
            return ActionNextStep::Finish(ActionTerminal::Incomplete {
                partial_response: None,
                reason: "model returned no visible assistant response".to_string(),
            });
        }
        if self.active_finalize_response().is_some() {
            let attempted_tool_activity =
                std::mem::take(&mut self.final_response_attempted_tool_activity);
            let response_truncated = std::mem::take(&mut self.final_response_candidate_truncated);
            match TurnControlPlane::validate_final_response(FinalResponseValidationFacts {
                attempted_tool_activity,
                response_truncated,
                response_text: Some(&response),
            }) {
                FinalResponseValidationOutcome::Accept => {}
                FinalResponseValidationOutcome::Retry(violation) => {
                    return self.block_finalize_response_completion(Some(response), violation);
                }
            }
        }
        let retry_limit = self.root_turn_completion_retry_limit();

        let Some(contract) = self.root_turn_contract.as_mut() else {
            return self.finish_root_turn_terminal_response(response);
        };
        let Some(block) = root_turn_completion_block(contract, &response) else {
            return self.finish_root_turn_terminal_response(response);
        };
        contract.blocked_terminal_attempts = contract.blocked_terminal_attempts.saturating_add(1);
        let blocked_attempts = contract.blocked_terminal_attempts;
        let retries_remaining = retry_limit.saturating_sub(blocked_attempts);

        if blocked_attempts >= retry_limit {
            let reason = Self::root_turn_incomplete_reason(&block);
            let partial_response =
                Self::root_turn_contract_incomplete_response(&block, Some(&response));
            self.emit_signal(
                LoopStep::Act,
                SignalKind::Friction,
                "root turn completion retry cap reached; ending incomplete",
                serde_json::json!({
                    "blocked_attempts": blocked_attempts,
                    "retry_limit": retry_limit,
                    "missing_response_sections": &block.missing_response_sections,
                    "pending_mutation_work": &block.pending_mutation_work,
                    "pending_artifact_paths": &block.pending_artifact_paths,
                    "pending_external_actions": &block.pending_external_actions,
                }),
            );
            return ActionNextStep::Finish(ActionTerminal::Incomplete {
                partial_response,
                reason,
            });
        }

        self.emit_signal(
            LoopStep::Act,
            SignalKind::Trace,
            "blocked terminal completion until root turn deliverables are satisfied",
            serde_json::json!({
                "blocked_attempts": blocked_attempts,
                "retry_limit": retry_limit,
                "retries_remaining": retries_remaining,
                "missing_response_sections": &block.missing_response_sections,
                "pending_mutation_work": &block.pending_mutation_work,
                "pending_artifact_paths": &block.pending_artifact_paths,
                "pending_external_actions": &block.pending_external_actions,
            }),
        );

        let mut context_messages = vec![Message::assistant(response.clone())];
        context_messages.push(Message::system(render_root_turn_retry_directive(
            &block,
            retries_remaining,
        )));
        let mut continuation = ActionContinuation::new(Some(response), None);
        if let Some(label) = block.pending_mutation_work.first().cloned() {
            let tool_scope = mutation_tool_scope(&self.tool_executor.tool_definitions());
            continuation = continuation
                .with_tool_scope(tool_scope.clone())
                .with_turn_commitment(TurnCommitment::ProceedUnderConstraints(
                    ProceedUnderConstraints {
                        goal: format!(
                            "Complete the pending root-turn mutation deliverable: {label}"
                        ),
                        success_target: Some(format!(
                            "Use mutation tools to complete this requested work now: {label}."
                        )),
                        unsupported_items: block
                            .missing_response_sections
                            .iter()
                            .map(|section| {
                                format!(
                                    "Response section still pending after mutation work: {section}"
                                )
                            })
                            .collect(),
                        assumptions: Vec::new(),
                        allowed_tools: Some(tool_scope),
                    },
                ));
        } else if let Some(path) = block.pending_artifact_paths.first().cloned() {
            let tool_scope = ContinuationToolScope::Only(vec!["write_file".to_string()]);
            continuation = continuation
                .with_tool_scope(tool_scope.clone())
                .with_artifact_write_target(path.clone())
                .with_turn_commitment(TurnCommitment::ProceedUnderConstraints(
                    ProceedUnderConstraints {
                        goal: format!("Write the pending root-turn artifact deliverable: {path}"),
                        success_target: Some(format!(
                            "Use write_file to create or update {path}, then continue to any remaining response deliverables."
                        )),
                        unsupported_items: block
                            .missing_response_sections
                            .iter()
                            .map(|label| {
                                format!("Response section still pending after artifact write: {label}")
                            })
                            .collect(),
                        assumptions: Vec::new(),
                        allowed_tools: Some(tool_scope),
                    },
                ));
        } else if let Some(action) = block.pending_external_actions.first().cloned() {
            let action_kind = self.first_pending_external_action_kind();
            if let Some(kind) = action_kind {
                self.pending_external_action_target = Some(kind);
            }
            let tool_scope_names = action_kind
                .map(|kind| {
                    available_external_action_tool_names(
                        &self.tool_executor.tool_definitions(),
                        kind,
                    )
                })
                .filter(|names| !names.is_empty())
                .unwrap_or_else(|| vec!["run_command".to_string()]);
            let tool_scope = ContinuationToolScope::Only(tool_scope_names);
            context_messages.push(Message::system(format!(
                "Use one of [{}] to complete the pending external action before producing the final response.",
                match &tool_scope {
                    ContinuationToolScope::Only(names) => names.join(", "),
                    _ => "run_command".to_string(),
                }
            )));
            continuation = continuation
                .with_tool_scope(tool_scope.clone())
                .with_turn_commitment(TurnCommitment::ProceedUnderConstraints(
                    ProceedUnderConstraints {
                        goal: format!("Complete the pending root-turn external action: {action}"),
                        success_target: Some(format!(
                            "Complete this external action now, then continue to any remaining response deliverables: {action}."
                        )),
                        unsupported_items: block
                            .missing_response_sections
                            .iter()
                            .map(|label| {
                                format!("Response section still pending after external action: {label}")
                            })
                            .collect(),
                        assumptions: Vec::new(),
                        allowed_tools: Some(tool_scope),
                    },
                ));
        } else {
            continuation = continuation.with_turn_commitment(final_response_turn_commitment(
                "required root-turn response sections were missing from the prior response",
                "Produce one consolidated final response that satisfies all remaining response sections.",
            ));
        }
        continuation = continuation.with_context_messages(context_messages);
        ActionNextStep::Continue(continuation)
    }

    fn active_finalize_response(&self) -> Option<&crate::act::FinalizeResponse> {
        match self.pending_turn_commitment.as_ref() {
            Some(TurnCommitment::FinalizeResponse(commitment)) => Some(commitment),
            _ => None,
        }
    }

    fn finalize_response_retry_limit(&self) -> u8 {
        self.root_turn_completion_retry_limit().max(1)
    }

    fn block_finalize_response_completion(
        &mut self,
        prior_response: Option<String>,
        violation: FinalResponseViolation,
    ) -> ActionNextStep {
        let retry_limit = self.finalize_response_retry_limit();
        self.final_response_blocked_attempts =
            self.final_response_blocked_attempts.saturating_add(1);
        let blocked_attempts = self.final_response_blocked_attempts;
        let retries_remaining = retry_limit.saturating_sub(blocked_attempts);
        let success_target = self
            .active_finalize_response()
            .map(|commitment| commitment.success_target.clone())
            .unwrap_or_else(|| "Produce one consolidated final answer.".to_string());
        let violation_label = final_response_violation_label(violation);

        if violation == FinalResponseViolation::ProgressOnlyResponse {
            tracing::info!(
                blocked_attempts,
                retry_limit,
                "progress-only final-response heuristic matched"
            );
            self.emit_signal(
                LoopStep::Act,
                SignalKind::Trace,
                "progress-only final-response heuristic matched",
                serde_json::json!({
                    "detector": "looks_like_progress_only_final_response",
                    "blocked_attempts": blocked_attempts,
                    "retry_limit": retry_limit,
                    "success_target": success_target,
                }),
            );
        }

        if blocked_attempts >= retry_limit {
            let partial_response =
                final_response_retry_cap_partial_response(prior_response.as_deref(), violation);
            self.emit_signal(
                LoopStep::Act,
                SignalKind::Friction,
                "final-response protocol retry cap reached; ending incomplete",
                serde_json::json!({
                    "violation": violation_label,
                    "blocked_attempts": blocked_attempts,
                    "retry_limit": retry_limit,
                    "success_target": success_target,
                }),
            );
            return ActionNextStep::Finish(ActionTerminal::Incomplete {
                partial_response: Some(partial_response),
                reason: format!(
                    "final response stayed non-terminal after {blocked_attempts} attempt(s): {violation_label}"
                ),
            });
        }

        self.emit_signal(
            LoopStep::Act,
            SignalKind::Blocked,
            "final-response protocol violation; retrying final answer",
            serde_json::json!({
                "violation": violation_label,
                "blocked_attempts": blocked_attempts,
                "retry_limit": retry_limit,
                "retries_remaining": retries_remaining,
                "success_target": success_target,
            }),
        );

        let mut context_messages = Vec::new();
        if let Some(response) = prior_response.as_ref() {
            context_messages.push(Message::assistant(response.clone()));
        }
        context_messages.push(Message::system(render_finalize_response_retry_directive(
            violation,
            retries_remaining,
            &success_target,
        )));

        ActionNextStep::Continue(
            ActionContinuation::new(None, None)
                .with_context_messages(context_messages)
                .with_tool_scope(ContinuationToolScope::NoTools)
                .with_turn_commitment(final_response_turn_commitment(
                    format!("final-response protocol violation: {violation_label}"),
                    &success_target,
                )),
        )
    }

    fn task_contract_blocks_tools(&self) -> bool {
        self.task_contract
            .as_ref()
            .is_some_and(|contract| contract.phase != TaskPhase::Gathering)
    }

    fn current_reasoning_tool_definitions(&self, should_strip_tools: bool) -> Vec<ToolDefinition> {
        let base = if self.task_contract_blocks_tools() {
            Vec::new()
        } else if should_strip_tools {
            let limited_tools = self.progress_limited_tool_definitions();
            tracing::info!(
                turns = self.consecutive_tool_turns,
                preserved_mutation_tools = !limited_tools.is_empty(),
                "limiting tools: agent exceeded nudge + grace threshold"
            );
            limited_tools
        } else {
            self.tool_executor.tool_definitions()
        };

        let scoped = self.apply_pending_tool_scope(base);
        let phased = self.apply_turn_execution_profile_tool_surface(scoped);
        let routed = self.apply_preflight_route_tool_surface(phased);
        let externally_gated = self.apply_pending_external_action_gate(routed);
        self.apply_pending_artifact_gate(externally_gated)
    }

    fn pending_turn_commitment_directive(&self) -> Option<String> {
        self.pending_turn_commitment
            .as_ref()
            .map(render_turn_commitment_directive)
    }

    fn root_turn_contract_directive(&self) -> Option<String> {
        self.root_turn_contract
            .as_ref()
            .map(render_root_turn_contract_directive)
    }

    fn task_contract_state_directive(&self) -> Option<String> {
        let mut sections = Vec::new();
        if let Some(contract) = self.task_contract.as_ref() {
            sections.push(render_task_contract_state_directive(contract));
        }
        if self.turn_progress_ledger.has_explicit_evidence_slots()
            || self
                .turn_progress_ledger
                .has_open_discovered_evidence_slots()
        {
            sections.push(render_turn_progress_ledger_directive(
                &self.turn_progress_ledger,
            ));
        }
        (!sections.is_empty()).then(|| sections.join("\n\n"))
    }

    fn task_contract_declaration_directive(
        &self,
        user_message: &str,
        tools_available: bool,
    ) -> Option<&'static str> {
        if tools_available
            && self.task_contract.is_none()
            && should_request_task_contract_declaration(user_message)
        {
            return Some(TASK_CONTRACT_DECLARATION_DIRECTIVE.trim_start_matches('\n'));
        }
        None
    }

    fn pending_artifact_write_directive(&self) -> Option<String> {
        self.pending_artifact_write_target.as_ref().map(|path| {
            format!(
                "Immediate next action: write the requested artifact to {path} using write_file. Do not do more observation, search, or shell inspection before attempting this write unless the write itself is blocked."
            )
        })
    }

    fn first_pending_external_action_kind(&self) -> Option<RootTurnExternalActionKind> {
        self.root_turn_contract
            .as_ref()?
            .deliverables
            .iter()
            .find_map(|deliverable| match deliverable {
                RootTurnDeliverable::ExternalAction {
                    kind,
                    satisfied: false,
                    ..
                } => Some(*kind),
                _ => None,
            })
    }

    fn pending_external_action_directive(&self) -> Option<String> {
        let action = self.pending_external_action_target?;
        let allowed_tools =
            available_external_action_tool_names(&self.tool_executor.tool_definitions(), action);
        if allowed_tools.is_empty() {
            Some(format!(
                "Immediate next action: complete the external action \"{}\". Do not run more inspection, diff, search, or status commands before attempting this action. Only use a tool call that can complete this action.",
                external_action_label(action)
            ))
        } else {
            Some(format!(
                "Immediate next action: complete the external action \"{}\". Do not run more inspection, diff, search, or status commands before attempting this action. Use one of [{}] to complete it now.",
                external_action_label(action),
                allowed_tools.join(", ")
            ))
        }
    }

    fn clear_pending_external_action_target_if_satisfied(
        &mut self,
        satisfied_kinds: &[RootTurnExternalActionKind],
    ) {
        if self
            .pending_external_action_target
            .is_some_and(|target| satisfied_kinds.contains(&target))
        {
            self.pending_external_action_target = None;
            self.pending_external_action_consecutive_failures = 0;
        }
    }

    fn root_turn_completion_retry_limit(&self) -> u8 {
        self.current_termination_config()
            .root_turn_completion_retry_limit
    }

    fn repeated_failure_streak_limit(&self) -> u16 {
        self.current_termination_config()
            .max_repeated_failure_streak
            .max(1)
    }

    fn external_action_gate_failure_lift_threshold(&self) -> u8 {
        self.current_termination_config()
            .external_action_gate_failure_lift_threshold
            .max(1)
    }

    fn current_termination_config(&self) -> Cow<'_, TerminationConfig> {
        let base = &self.budget.config().termination;
        match self
            .turn_execution_profile
            .tightened_termination_config(base)
        {
            Some(tightened) => Cow::Owned(tightened),
            None => Cow::Borrowed(base),
        }
    }

    fn apply_pending_artifact_gate(&self, tools: Vec<ToolDefinition>) -> Vec<ToolDefinition> {
        if self.pending_artifact_write_target.is_none() {
            return tools;
        }
        let write_tools: Vec<ToolDefinition> = tools
            .into_iter()
            .filter(|tool| tool.name == "write_file")
            .collect();
        if write_tools.is_empty() {
            self.apply_pending_tool_scope(self.tool_executor.tool_definitions())
        } else {
            write_tools
        }
    }

    fn apply_pending_external_action_gate(
        &self,
        tools: Vec<ToolDefinition>,
    ) -> Vec<ToolDefinition> {
        let Some(action) = self.pending_external_action_target else {
            return tools;
        };
        // After repeated failures, lift the gate so the agent can diagnose
        // and recover (e.g., fix git credentials before retrying push).
        if self.pending_external_action_consecutive_failures
            >= self.external_action_gate_failure_lift_threshold()
        {
            return tools;
        }
        let allowed_names = available_external_action_tool_names(&tools, action);
        let filter_allowed = |tools: Vec<ToolDefinition>| {
            tools
                .into_iter()
                .filter(|tool| allowed_names.iter().any(|name| name == &tool.name))
                .collect::<Vec<_>>()
        };
        let filtered = filter_allowed(tools);
        if filtered.is_empty() {
            let fallback_tools = self.tool_executor.tool_definitions();
            let fallback_allowed_names =
                available_external_action_tool_names(&fallback_tools, action);
            fallback_tools
                .into_iter()
                .filter(|tool| fallback_allowed_names.iter().any(|name| name == &tool.name))
                .collect()
        } else {
            filtered
        }
    }

    fn progress_limited_tool_definitions(&self) -> Vec<ToolDefinition> {
        let mutation_tools = self.side_effect_tool_definitions();
        if mutation_tools.is_empty() {
            Vec::new()
        } else {
            mutation_tools
        }
    }

    fn record_reasoning_cost(&mut self, reason_cost: ActionCost, state: &mut CycleState) {
        self.budget.record(&reason_cost);
        state
            .tokens
            .accumulate(reasoning_token_usage(reason_cost.tokens));
    }

    fn budget_terminal(
        &mut self,
        cost: ActionCost,
        partial_response: Option<String>,
    ) -> Option<LoopResult> {
        if self.budget.check_at(current_time_ms(), &cost).is_ok() {
            return None;
        }

        self.emit_signal(
            LoopStep::Act,
            SignalKind::Blocked,
            "budget exhausted",
            serde_json::json!({"iterations": self.iteration_count}),
        );

        Some(LoopResult::BudgetExhausted {
            partial_response,
            iterations: self.iteration_count,
            signals: Vec::new(),
        })
    }

    /// Make one final LLM call with tools stripped to synthesize findings.
    async fn forced_synthesis_turn(
        &mut self,
        llm: &dyn LlmProvider,
        messages: &[Message],
        stream: CycleStream<'_>,
    ) -> Option<String> {
        if !self.budget.config().termination.synthesize_on_exhaustion {
            tracing::debug!("skipping forced synthesis: synthesize_on_exhaustion disabled");
            return None;
        }

        let mut request = build_forced_synthesis_request(ForcedSynthesisRequestParams::new(
            messages,
            llm.model_name(),
            self.memory_context.as_deref(),
            self.scratchpad_context.as_deref(),
            self.execution_context.as_ref(),
            self.agent_preferences.as_deref(),
            self.notify_tool_guidance_enabled,
        ));
        request.cache_affinity = self.prompt_cache_affinity();

        let remaining_wall_ms = self
            .budget
            .remaining(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
            )
            .wall_time_ms;
        let timeout_ms = remaining_wall_ms.min(30_000).saturating_sub(2_000);
        if timeout_ms == 0 {
            tracing::warn!("skipping forced synthesis: insufficient wall time remaining");
            return None;
        }
        let timeout = std::time::Duration::from_millis(timeout_ms);

        let request_messages = request.messages.clone();
        let text_visibility = if self.root_turn_contract.is_some() {
            // Forced synthesis is a recovery path. If the root turn declared a
            // completion contract, this text is speculative until the kernel
            // validates the required sections, so do not publish it early.
            TextStreamVisibility::Hidden
        } else {
            TextStreamVisibility::Public
        };
        let synthesis = async {
            let response = self
                .request_completion(
                    llm,
                    request,
                    StreamingRequestContext::new(
                        LoopStep::Synthesize,
                        StreamPhase::Synthesize,
                        text_visibility,
                    ),
                    stream,
                )
                .await?;
            self.continue_truncated_response(
                response,
                &request_messages,
                llm,
                LoopStep::Synthesize,
                stream,
            )
            .await
        };

        match tokio::time::timeout(timeout, synthesis).await {
            Ok(Ok(response)) => {
                let text = response_text_segment(&response).unwrap_or_default();
                if text.trim().is_empty() {
                    None
                } else {
                    Some(text)
                }
            }
            Ok(Err(e)) => {
                tracing::warn!("forced synthesis turn failed: {e:?}");
                None
            }
            Err(_elapsed) => {
                tracing::warn!("forced synthesis turn timed out after {timeout_ms}ms");
                None
            }
        }
    }

    fn resolve_budget_exhausted_response(
        synthesized: Option<String>,
        partial_response: Option<String>,
    ) -> String {
        synthesized
            .or_else(|| partial_response.filter(|text| !text.trim().is_empty()))
            .unwrap_or_else(|| BUDGET_EXHAUSTED_FALLBACK_RESPONSE.to_string())
    }

    fn root_turn_incomplete_reason(block: &RootTurnCompletionBlock) -> String {
        let mut reason =
            String::from("required root-turn deliverables were still missing at turn end");
        if !block.missing_response_sections.is_empty() {
            reason.push_str("; missing response sections: ");
            reason.push_str(&block.missing_response_sections.join(", "));
        }
        if !block.pending_mutation_work.is_empty() {
            reason.push_str("; pending local mutation work: ");
            reason.push_str(&block.pending_mutation_work.join(", "));
        }
        if !block.pending_artifact_paths.is_empty() {
            reason.push_str("; pending artifact writes: ");
            reason.push_str(&block.pending_artifact_paths.join(", "));
        }
        if !block.pending_external_actions.is_empty() {
            reason.push_str("; pending external actions: ");
            reason.push_str(&block.pending_external_actions.join(", "));
        }
        reason
    }

    fn root_turn_contract_incomplete_response(
        block: &RootTurnCompletionBlock,
        candidate_response: Option<&str>,
    ) -> Option<String> {
        let has_side_effect_deliverable = !block.pending_mutation_work.is_empty()
            || !block.pending_artifact_paths.is_empty()
            || !block.pending_external_actions.is_empty();
        if !has_side_effect_deliverable {
            return None;
        }

        // The contract's `satisfied` flag gates tool scope, not speech.
        // When the loop must end incomplete, keep the model's visible response
        // available for context, but prefix it with the kernel-owned contract
        // failure so confident-but-wrong text is not presented as completed
        // work.
        let pending = root_turn_pending_deliverable_summary(block);
        let note = format!(
            "I could not complete this turn because required root-turn deliverable(s) are still pending: {pending}."
        );
        match candidate_response.and_then(meaningful_response_text) {
            Some(candidate) => Some(format!(
                "{note}\n\nModel response before the turn ended:\n{candidate}"
            )),
            None => Some(note),
        }
    }

    fn root_turn_completion_block_for_response(
        &self,
        response: &str,
    ) -> Option<RootTurnCompletionBlock> {
        self.root_turn_contract
            .as_ref()
            .and_then(|contract| root_turn_completion_block(contract, response))
    }

    /// Perceive step.
    async fn perceive(
        &mut self,
        snapshot: &PerceptionSnapshot,
    ) -> Result<ProcessedPerception, LoopError> {
        self.drain_user_input_to_state();
        let mut snapshot_with_steer = snapshot.clone();
        if let Some(live_steer) = self.active_turn_steer.clone() {
            snapshot_with_steer.steer_context = Some(live_steer);
        } else if let Some(snapshot_steer) = snapshot_with_steer
            .steer_context
            .take()
            .and_then(normalize_turn_steer)
        {
            self.active_turn_steer = Some(snapshot_steer.clone());
            snapshot_with_steer.steer_context = Some(snapshot_steer);
        }

        let user_message = extract_user_message(&snapshot_with_steer)?;
        self.emit_signal(
            LoopStep::Perceive,
            SignalKind::Trace,
            "processing user input",
            serde_json::json!({"input_length": user_message.len()}),
        );

        let mut context_window = snapshot_with_steer.conversation_history.clone();
        context_window.push(build_user_message(&snapshot_with_steer, &user_message));
        if let Some(memory_message) = self.session_memory_message() {
            let insert_pos = context_window
                .iter()
                .take_while(|message| matches!(message.role, MessageRole::System))
                .count();
            context_window.insert(insert_pos, memory_message);
        }

        let before_context_window = ContextWindowStats::capture(self, &context_window);
        let compacted_context = {
            let compaction = self.compaction();
            compaction
                .compact_if_needed(
                    &context_window,
                    CompactionScope::Perceive,
                    self.iteration_count,
                )
                .await?
        };
        if let Cow::Owned(messages) = compacted_context {
            self.tool_retry_tracker.notify_compaction();
            self.tool_executor.notify_compaction();
            self.emit_context_overflow_signal(
                CompactionScope::Perceive,
                before_context_window,
                &messages,
            );
            context_window = messages;
        }
        self.compaction()
            .ensure_within_hard_limit(CompactionScope::Perceive, &context_window)?;

        self.append_compacted_summary(&snapshot_with_steer, &user_message, &mut context_window);

        if self.budget.state() == BudgetState::Low {
            if !self.budget_low_signaled {
                self.emit_signal(
                    LoopStep::Perceive,
                    SignalKind::Performance,
                    "budget soft-ceiling reached, entering wrap-up mode",
                    serde_json::json!({"budget_state": "low"}),
                );
                self.budget_low_signaled = true;
            }
            context_window.push(Message::system(BUDGET_LOW_WRAP_UP_DIRECTIVE.to_string()));
        }

        let nudge_at = self.current_termination_config().nudge_after_tool_turns;
        if nudge_at > 0 && self.consecutive_tool_turns >= nudge_at {
            context_window.push(Message::system(TOOL_TURN_NUDGE.to_string()));
        }

        let processed = ProcessedPerception {
            user_message: user_message.clone(),
            images: snapshot_with_steer
                .user_input
                .as_ref()
                .map(|user_input| user_input.images.clone())
                .unwrap_or_default(),
            documents: snapshot_with_steer
                .user_input
                .as_ref()
                .map(|user_input| user_input.documents.clone())
                .unwrap_or_default(),
            context_window,
            active_goals: vec![format!("Help the user with: {user_message}")],
            budget_remaining: self.budget.remaining(snapshot_with_steer.timestamp_ms),
            steer_context: snapshot_with_steer.steer_context,
        };

        // Sync live tool_invocations_remaining into the runtime info snapshot.
        if let Some(ri) = &self.runtime_info {
            let remaining = self.budget.remaining(snapshot_with_steer.timestamp_ms);
            if let Ok(mut guard) = ri.write() {
                guard.config_summary.tool_invocations_remaining = remaining.tool_invocations;
            }
        }

        let available_tools = self.tool_executor.tool_definitions();
        let seeded_evidence = self.seed_turn_progress_from_user_message(&user_message);
        if !seeded_evidence.is_empty() {
            self.emit_signal(
                LoopStep::Perceive,
                SignalKind::Trace,
                "seeded explicit turn evidence slots",
                serde_json::json!({
                    "slots": seeded_evidence,
                }),
            );
        }
        self.turn_execution_profile = detect_turn_execution_profile_for_ownership(
            &user_message,
            &available_tools,
            self.direct_inspection_ownership,
        );
        self.bounded_local_phase = BoundedLocalPhase::Discovery;
        self.bounded_local_recovery_used = false;
        self.bounded_local_recovery_focus.clear();
        self.preflight_route_plan = None;
        match &self.turn_execution_profile {
            TurnExecutionProfile::BoundedLocal => {
                self.emit_signal(
                    LoopStep::Perceive,
                    SignalKind::Trace,
                    "selected bounded local execution profile",
                    serde_json::json!({
                        "profile": "bounded_local",
                        "phase": bounded_local_phase_label(self.bounded_local_phase),
                    }),
                );
            }
            TurnExecutionProfile::DirectInspection(profile) => {
                self.emit_signal(
                    LoopStep::Perceive,
                    SignalKind::Trace,
                    "selected direct inspection execution profile",
                    serde_json::json!({
                        "profile": "direct_inspection",
                        "inspection_profile": direct_inspection_profile_label(*profile),
                    }),
                );
            }
            TurnExecutionProfile::DirectUtility(profile) => {
                self.emit_signal(
                    LoopStep::Perceive,
                    SignalKind::Trace,
                    "selected direct utility execution profile",
                    serde_json::json!({
                        "profile": "direct_utility",
                        "tool_name": &profile.tool_name,
                    }),
                );
            }
            TurnExecutionProfile::DeterministicLocal(plan) => {
                self.emit_signal(
                    LoopStep::Perceive,
                    SignalKind::Trace,
                    "selected deterministic local execution profile",
                    serde_json::json!({
                        "profile": "deterministic_local",
                        "intent": plan.signal_metadata(),
                    }),
                );
            }
            TurnExecutionProfile::Standard => {}
        }
        if matches!(self.turn_execution_profile, TurnExecutionProfile::Standard) {
            if let Some(resource) = detect_route_resource(&user_message) {
                let runtime_routing_tools = self.collect_runtime_routing_tools();
                if let Some(route_plan) = build_route_plan(
                    &resource,
                    &available_tools,
                    &runtime_routing_tools,
                    &self.route_advisories,
                ) {
                    self.emit_signal(
                        LoopStep::Perceive,
                        SignalKind::Trace,
                        "planned preflight external resource route",
                        serde_json::json!({
                            "decision_kind": ControlPlaneDecisionKind::PreflightRoute,
                            "decision": "planned",
                            "resource": &route_plan.resource,
                            "primary_route": &route_plan.primary_route,
                            "fallback_routes": &route_plan.fallback_routes,
                            "requires_probe": route_plan.requires_probe,
                        }),
                    );
                    self.preflight_route_plan = Some(route_plan);
                } else {
                    let available_tool_names = available_tools
                        .iter()
                        .map(|tool| tool.name.clone())
                        .collect::<Vec<_>>();
                    let resource_kind = resource.kind();
                    let mut typed_route_tools = Vec::new();
                    let mut ready_typed_route_tools = Vec::new();
                    let mut unready_typed_route_tools = Vec::new();
                    for summary in runtime_routing_tools
                        .iter()
                        .filter(|summary| summary.metadata.resource_kinds.contains(&resource_kind))
                    {
                        typed_route_tools.push(summary.tool_name.clone());
                        if summary.readiness.available && summary.readiness.ready {
                            ready_typed_route_tools.push(summary.tool_name.clone());
                        } else {
                            unready_typed_route_tools.push(serde_json::json!({
                                "tool_name": &summary.tool_name,
                                "readiness_reason": &summary.readiness.readiness_reason,
                            }));
                        }
                    }
                    let degraded_plan =
                        build_degraded_public_web_fallback_plan(&resource, &available_tools);
                    let fallback_mode = if degraded_plan.is_some() {
                        "public_web"
                    } else {
                        "unconstrained"
                    };
                    self.emit_signal(
                        LoopStep::Perceive,
                        SignalKind::Trace,
                        "resource-bearing request has no ready typed preflight route",
                        serde_json::json!({
                            "decision_kind": ControlPlaneDecisionKind::PreflightRoute,
                            "decision": "no_ready_typed_route",
                            "resource": &resource,
                            "available_tools": available_tool_names,
                            "routing_tool_count": runtime_routing_tools.len(),
                            "typed_route_tools": typed_route_tools,
                            "ready_typed_route_tools": ready_typed_route_tools,
                            "unready_typed_route_tools": unready_typed_route_tools,
                            "fallback_mode": fallback_mode,
                            "fallback_route": degraded_plan.as_ref().map(|plan| &plan.primary_route),
                        }),
                    );
                    self.preflight_route_plan = degraded_plan;
                }
            }
        }
        self.requested_artifact_target = extract_requested_write_target(&user_message);
        let root_turn_contract = extract_root_turn_contract(&user_message);
        self.root_turn_contract = root_turn_contract.contract;
        if root_turn_contract.deliverable_block_count > 1 {
            self.emit_signal(
                LoopStep::Perceive,
                SignalKind::Friction,
                "multiple Deliverables blocks detected; using the first block only",
                serde_json::json!({
                    "deliverable_block_count": root_turn_contract.deliverable_block_count,
                }),
            );
        }
        if let Some(contract) = &self.root_turn_contract {
            let response_sections = contract
                .deliverables
                .iter()
                .filter_map(|deliverable| match deliverable {
                    RootTurnDeliverable::ResponseSection { label, .. } => Some(label.clone()),
                    RootTurnDeliverable::MutationWork { .. } => None,
                    RootTurnDeliverable::ArtifactWrite { .. } => None,
                    RootTurnDeliverable::ExternalAction { .. } => None,
                })
                .collect::<Vec<_>>();
            let mutation_work = contract
                .deliverables
                .iter()
                .filter_map(|deliverable| match deliverable {
                    RootTurnDeliverable::MutationWork { label, .. } => Some(label.clone()),
                    RootTurnDeliverable::ResponseSection { .. }
                    | RootTurnDeliverable::ArtifactWrite { .. }
                    | RootTurnDeliverable::ExternalAction { .. } => None,
                })
                .collect::<Vec<_>>();
            let artifact_paths = contract
                .deliverables
                .iter()
                .filter_map(|deliverable| match deliverable {
                    RootTurnDeliverable::ArtifactWrite { path, .. } => Some(path.clone()),
                    RootTurnDeliverable::MutationWork { .. } => None,
                    RootTurnDeliverable::ResponseSection { .. } => None,
                    RootTurnDeliverable::ExternalAction { .. } => None,
                })
                .collect::<Vec<_>>();
            let external_actions = contract
                .deliverables
                .iter()
                .filter_map(|deliverable| match deliverable {
                    RootTurnDeliverable::ExternalAction { label, .. } => Some(label.clone()),
                    RootTurnDeliverable::ResponseSection { .. }
                    | RootTurnDeliverable::MutationWork { .. }
                    | RootTurnDeliverable::ArtifactWrite { .. } => None,
                })
                .collect::<Vec<_>>();
            tracing::info!(
                deliverable_block_count = root_turn_contract.deliverable_block_count,
                response_sections = ?response_sections,
                mutation_work = ?mutation_work,
                artifact_paths = ?artifact_paths,
                external_actions = ?external_actions,
                "extracted root turn completion contract"
            );
            self.emit_signal(
                LoopStep::Perceive,
                SignalKind::Trace,
                "extracted root turn completion contract",
                serde_json::json!({
                    "deliverable_block_count": root_turn_contract.deliverable_block_count,
                    "response_sections": response_sections,
                    "mutation_work": mutation_work,
                    "artifact_paths": artifact_paths,
                    "external_actions": external_actions,
                }),
            );
        }
        self.last_reasoning_messages = build_reasoning_messages(&processed);

        Ok(processed)
    }

    fn seed_turn_progress_from_user_message(&mut self, user_message: &str) -> Vec<String> {
        extract_user_evidence_references(user_message)
            .into_iter()
            .filter_map(|reference| {
                self.turn_progress_ledger
                    .seed_explicit_evidence_slot(reference.clone())
                    .map(|_| reference)
            })
            .collect()
    }

    /// Reason step.
    async fn reason(
        &mut self,
        perception: &ProcessedPerception,
        llm: &dyn LlmProvider,
        stream: CycleStream<'_>,
    ) -> Result<CompletionResponse, LoopError> {
        self.maybe_publish_reason_progress(stream);
        if let TurnExecutionProfile::DeterministicLocal(plan) = &self.turn_execution_profile {
            return Ok(plan.completion_response());
        }
        if let TurnExecutionProfile::DirectUtility(profile) = &self.turn_execution_profile {
            let direct_tools = self.current_reasoning_tool_definitions(false);
            return Ok(direct_utility_completion_response(
                profile,
                &perception.user_message,
                &direct_tools,
            ));
        }
        if self.active_finalize_response().is_some() {
            return self.reason_final_response(perception, llm, stream).await;
        }
        let termination = self.current_termination_config();
        let tc = termination.as_ref();
        let should_strip_tools = tc.nudge_after_tool_turns > 0
            && ToolStrippingAfterNudge::from_config_value(tc.strip_tools_after_nudge).should_strip(
                tc.nudge_after_tool_turns,
                u32::from(self.consecutive_tool_turns),
            );
        let tools = self.current_reasoning_tool_definitions(should_strip_tools);
        let mut request = build_reasoning_request(ReasoningRequestParams::new(
            perception,
            llm.model_name(),
            ToolRequestConfig::new(tools, self.reasoning_decompose_enabled()),
            self.request_build_context(),
        ));
        if let Some(directive) = self.pending_turn_commitment_directive() {
            if let Some(system_prompt) = request.system_prompt.as_mut() {
                system_prompt.push_str("\n\nTurn commitment:\n");
                system_prompt.push_str(&directive);
            }
        }
        if let Some(directive) = self.root_turn_contract_directive() {
            if let Some(system_prompt) = request.system_prompt.as_mut() {
                system_prompt.push_str("\n\nRoot turn completion contract:\n");
                system_prompt.push_str(&directive);
            }
        }
        if let Some(directive) = self.pending_artifact_write_directive() {
            if let Some(system_prompt) = request.system_prompt.as_mut() {
                system_prompt.push_str("\n\nArtifact gate:\n");
                system_prompt.push_str(&directive);
            }
        }
        if let Some(directive) = self.pending_external_action_directive() {
            if let Some(system_prompt) = request.system_prompt.as_mut() {
                system_prompt.push_str("\n\nExternal action gate:\n");
                system_prompt.push_str(&directive);
            }
        }
        if let Some(directive) = self.task_contract_state_directive() {
            if let Some(system_prompt) = request.system_prompt.as_mut() {
                system_prompt.push_str("\n\nTask lifecycle contract:\n");
                system_prompt.push_str(&directive);
            }
        }
        if let Some(directive) = self.task_contract_declaration_directive(
            &perception.user_message,
            !request.tools.is_empty(),
        ) {
            if let Some(system_prompt) = request.system_prompt.as_mut() {
                system_prompt.push_str("\n\nTask lifecycle declaration:\n");
                system_prompt.push_str(directive);
            }
        }
        if let Some(directive) = self.turn_execution_profile_directive() {
            if let Some(system_prompt) = request.system_prompt.as_mut() {
                system_prompt.push_str(&directive);
            }
        }
        let reasoning_messages = request.messages.clone();
        let started = current_time_ms();
        let response = self
            .request_completion(
                llm,
                request,
                StreamingRequestContext::new(
                    LoopStep::Reason,
                    StreamPhase::Reason,
                    TextStreamVisibility::Preview,
                )
                .with_preview_final_commit(self.root_turn_contract.is_none()),
                stream,
            )
            .await?;

        let response = self
            .continue_truncated_response(
                response,
                &reasoning_messages,
                llm,
                LoopStep::Reason,
                stream,
            )
            .await?;
        let latency_ms = current_time_ms().saturating_sub(started);
        let usage = response.usage;
        self.emit_reason_trace_and_perf(latency_ms, usage.as_ref());
        Ok(response)
    }

    async fn reason_simple(
        &mut self,
        perception: &ProcessedPerception,
        llm: &dyn LlmProvider,
        stream: CycleStream<'_>,
    ) -> Result<CompletionResponse, LoopError> {
        self.maybe_publish_reason_progress(stream);
        let tools = if self.task_contract_blocks_tools() {
            Vec::new()
        } else {
            let tools = self
                .apply_turn_execution_profile_tool_surface(self.tool_executor.tool_definitions());
            let scoped = self.apply_pending_tool_scope(tools);
            self.apply_preflight_route_tool_surface(scoped)
        };
        let mut request = build_simple_agent_request(SimpleAgentRequestParams::new(
            perception,
            llm.model_name(),
            tools,
            self.request_build_context_without_signal_feedback(),
        ));
        request.cache_affinity = self.prompt_cache_affinity();
        let request_messages = request.messages.clone();
        let started = current_time_ms();
        let response = self
            .request_completion(
                llm,
                request,
                StreamingRequestContext::new(
                    LoopStep::Reason,
                    StreamPhase::Reason,
                    TextStreamVisibility::Preview,
                )
                .with_preview_final_commit(true),
                stream,
            )
            .await?;
        let response = self
            .continue_truncated_response(response, &request_messages, llm, LoopStep::Reason, stream)
            .await?;
        let latency_ms = current_time_ms().saturating_sub(started);
        let usage = response.usage;
        self.emit_reason_trace_and_perf(latency_ms, usage.as_ref());
        Ok(response)
    }

    async fn reason_simple_final_response(
        &mut self,
        perception: &ProcessedPerception,
        llm: &dyn LlmProvider,
        stream: CycleStream<'_>,
    ) -> Result<CompletionResponse, LoopError> {
        let mut request = build_simple_agent_request(
            SimpleAgentRequestParams::new(
                perception,
                llm.model_name(),
                Vec::new(),
                self.request_build_context_without_signal_feedback(),
            )
            .final_response(),
        );
        request.cache_affinity = self.prompt_cache_affinity();
        let request_messages = request.messages.clone();
        let started = current_time_ms();
        let response = self
            .request_completion(
                llm,
                request,
                StreamingRequestContext::new(
                    LoopStep::Reason,
                    StreamPhase::Synthesize,
                    TextStreamVisibility::Public,
                )
                .with_preview_final_commit(true),
                stream,
            )
            .await?;
        let response = self
            .continue_truncated_response(response, &request_messages, llm, LoopStep::Reason, stream)
            .await?;
        let latency_ms = current_time_ms().saturating_sub(started);
        let usage = response.usage;
        self.emit_reason_trace_and_perf(latency_ms, usage.as_ref());
        Ok(response)
    }

    async fn reason_final_response(
        &mut self,
        perception: &ProcessedPerception,
        llm: &dyn LlmProvider,
        stream: CycleStream<'_>,
    ) -> Result<CompletionResponse, LoopError> {
        let mut request = build_forced_synthesis_request(ForcedSynthesisRequestParams::new(
            &perception.context_window,
            llm.model_name(),
            self.memory_context.as_deref(),
            self.scratchpad_context.as_deref(),
            self.execution_context.as_ref(),
            self.agent_preferences.as_deref(),
            self.notify_tool_guidance_enabled,
        ));
        request.cache_affinity = self.prompt_cache_affinity();

        if let Some(directive) = self.pending_turn_commitment_directive() {
            if let Some(system_prompt) = request.system_prompt.as_mut() {
                system_prompt.push_str("\n\nTurn commitment:\n");
                system_prompt.push_str(&directive);
            }
        }
        if let Some(directive) = self.root_turn_contract_directive() {
            if let Some(system_prompt) = request.system_prompt.as_mut() {
                system_prompt.push_str("\n\nRoot turn completion contract:\n");
                system_prompt.push_str(&directive);
            }
        }
        if let Some(directive) = self.pending_artifact_write_directive() {
            if let Some(system_prompt) = request.system_prompt.as_mut() {
                system_prompt.push_str("\n\nArtifact gate:\n");
                system_prompt.push_str(&directive);
            }
        }
        if let Some(directive) = self.pending_external_action_directive() {
            if let Some(system_prompt) = request.system_prompt.as_mut() {
                system_prompt.push_str("\n\nExternal action gate:\n");
                system_prompt.push_str(&directive);
            }
        }
        if let Some(directive) = self.task_contract_state_directive() {
            if let Some(system_prompt) = request.system_prompt.as_mut() {
                system_prompt.push_str("\n\nTask lifecycle contract:\n");
                system_prompt.push_str(&directive);
            }
        }
        if let Some(directive) = self.turn_execution_profile_directive() {
            if let Some(system_prompt) = request.system_prompt.as_mut() {
                system_prompt.push_str(&directive);
            }
        }

        self.emit_signal(
            LoopStep::Synthesize,
            SignalKind::Trace,
            "final response synthesis request",
            serde_json::json!({"tool_count": request.tools.len()}),
        );

        let request_messages = request.messages.clone();
        let started = current_time_ms();
        let response = self
            .request_completion(
                llm,
                request,
                StreamingRequestContext::new(
                    LoopStep::Synthesize,
                    StreamPhase::Synthesize,
                    TextStreamVisibility::Hidden,
                ),
                stream,
            )
            .await?;
        let response = self
            .continue_truncated_response(
                response,
                &request_messages,
                llm,
                LoopStep::Synthesize,
                stream,
            )
            .await?;
        let latency_ms = current_time_ms().saturating_sub(started);
        let usage = response.usage;
        self.emit_reason_trace_and_perf(latency_ms, usage.as_ref());
        Ok(response)
    }

    fn session_memory_message(&self) -> Option<Message> {
        let memory_text = match self.session_memory.lock() {
            Ok(memory) => (!memory.is_empty()).then(|| memory.render()),
            Err(poisoned) => {
                let memory = poisoned.into_inner();
                (!memory.is_empty()).then(|| memory.render())
            }
        }?;
        Some(Message::system(memory_text))
    }

    fn emit_continuation_trace(&mut self, step: LoopStep, attempt: u32) {
        self.emit_signal(
            step,
            SignalKind::Trace,
            format!("response truncated, continuing ({attempt}/{MAX_CONTINUATION_ATTEMPTS})"),
            serde_json::json!({"attempt": attempt}),
        );
    }

    fn text_stream_visibility_for_step(step: LoopStep) -> TextStreamVisibility {
        match step {
            LoopStep::Reason => TextStreamVisibility::Preview,
            LoopStep::Act => TextStreamVisibility::Hidden,
            LoopStep::Perceive | LoopStep::Decide | LoopStep::Synthesize => {
                TextStreamVisibility::Public
            }
        }
    }

    fn continuation_budget_exhausted(&self, continuation_messages: &[Message]) -> bool {
        let cost = continuation_budget_cost_estimate(continuation_messages);
        self.budget.check_at(current_time_ms(), &cost).is_err()
    }

    fn terminal_finalization_continuation_reserve_applies(&self, step: LoopStep) -> bool {
        step == LoopStep::Synthesize
            && self.active_finalize_response().is_some()
            && !self.budget.wall_time_exceeded(current_time_ms())
    }

    fn emit_terminal_finalization_continuation_reserve_signal(
        &mut self,
        step: LoopStep,
        attempt: u32,
    ) {
        self.emit_signal(
            step,
            SignalKind::Trace,
            "terminal final response continuation using reserved budget",
            serde_json::json!({
                "decision_kind": ControlPlaneDecisionKind::BudgetGuardrail,
                "decision": "allowed",
                "reason": "terminal_finalization_reserve",
                "attempt": attempt,
            }),
        );
    }

    fn emit_continuation_budget_exhausted_signal(&mut self, step: LoopStep, attempt: u32) {
        self.emit_signal(
            step,
            SignalKind::Blocked,
            "continuation budget exhausted",
            serde_json::json!({
                "decision_kind": ControlPlaneDecisionKind::BudgetGuardrail,
                "decision": "blocked",
                "reason": "continuation_budget_exhausted",
                "attempt": attempt,
            }),
        );
    }

    fn record_continuation_budget(
        &mut self,
        response: &CompletionResponse,
        continuation_messages: &[Message],
    ) {
        let cost = continuation_budget_cost(response, continuation_messages);
        self.budget.record(&cost);
    }

    async fn request_truncated_continuation(
        &mut self,
        llm: &dyn LlmProvider,
        continuation_messages: &[Message],
        step: LoopStep,
        stream: CycleStream<'_>,
    ) -> Result<CompletionResponse, LoopError> {
        let continuation_tools = if self.task_contract_blocks_tools() {
            Vec::new()
        } else {
            let tools = self
                .apply_turn_execution_profile_tool_surface(self.tool_executor.tool_definitions());
            let scoped = self.apply_pending_tool_scope(tools);
            self.apply_preflight_route_tool_surface(scoped)
        };
        let mut request =
            build_truncation_continuation_request(TruncationContinuationRequestParams::new(
                llm.model_name(),
                continuation_messages,
                ToolRequestConfig::new(continuation_tools, self.effective_decompose_enabled()),
                self.request_build_context(),
                step,
            ));
        if let Some(directive) = self.turn_execution_profile_directive() {
            if let Some(system_prompt) = request.system_prompt.as_mut() {
                system_prompt.push_str(&directive);
            }
        }
        if let Some(directive) = self.task_contract_state_directive() {
            if let Some(system_prompt) = request.system_prompt.as_mut() {
                system_prompt.push_str("\n\nTask lifecycle contract:\n");
                system_prompt.push_str(&directive);
            }
        }
        let request_messages = request.messages.clone();
        let response = self
            .request_completion(
                llm,
                request,
                StreamingRequestContext::new(
                    step,
                    stream_phase_for_step(step),
                    Self::text_stream_visibility_for_step(step),
                ),
                stream,
            )
            .await?;
        self.record_continuation_budget(&response, &request_messages);
        Ok(response)
    }

    async fn continue_truncated_response(
        &mut self,
        initial_response: CompletionResponse,
        base_messages: &[Message],
        llm: &dyn LlmProvider,
        step: LoopStep,
        stream: CycleStream<'_>,
    ) -> Result<CompletionResponse, LoopError> {
        let mut attempts = 0;
        let mut full_text = extract_response_text(&initial_response);
        let mut combined = initial_response;

        while is_truncated(combined.stop_reason.as_deref()) && attempts < MAX_CONTINUATION_ATTEMPTS
        {
            attempts = attempts.saturating_add(1);
            let continuation_messages = build_continuation_messages(base_messages, &full_text);
            if self.continuation_budget_exhausted(&continuation_messages) {
                if self.terminal_finalization_continuation_reserve_applies(step) {
                    self.emit_terminal_finalization_continuation_reserve_signal(step, attempts);
                } else {
                    self.emit_continuation_budget_exhausted_signal(step, attempts);
                    combined.stop_reason = Some("continuation_budget_exhausted".to_string());
                    break;
                }
            }
            self.emit_continuation_trace(step, attempts);
            let continued = self
                .request_truncated_continuation(llm, &continuation_messages, step, stream)
                .await?;
            combined = merge_continuation_response(combined, continued, &mut full_text);
        }

        Ok(combined)
    }

    fn capture_tool_response_state(&mut self, response: &CompletionResponse) {
        self.tool_call_provider_ids = extract_tool_use_provider_ids(&response.content);
        let extracted = extract_task_contract_from_response(response);
        if self.task_contract.is_none() {
            if let Some(contract) = extracted.contract {
                self.turn_progress_ledger.seed_task_contract(&contract);
                self.task_contract = Some(contract);
            }
        }
        self.pending_tool_response_text =
            tool_response_final_text(response, extracted.visible_text);
    }

    fn clear_tool_response_state(&mut self) {
        self.tool_call_provider_ids.clear();
        self.pending_tool_response_text = None;
    }

    fn record_tool_round_response_state(
        &mut self,
        state: &mut ToolRoundState,
        response: &CompletionResponse,
    ) {
        self.tool_call_provider_ids = extract_tool_use_provider_ids(&response.content);
        let extracted = extract_task_contract_from_response(response);
        if self.task_contract.is_none() {
            if let Some(contract) = extracted.contract {
                self.turn_progress_ledger.seed_task_contract(&contract);
                self.task_contract = Some(contract);
            }
        }
        push_response_segment(
            &mut state.accumulated_text,
            tool_response_final_text(response, extracted.visible_text),
        );
    }

    /// Decide step.
    async fn decide(&mut self, response: &CompletionResponse) -> Result<Decision, LoopError> {
        if self.active_finalize_response().is_some() {
            let raw = extract_response_text(response);
            let text = normalize_response_text(&extract_readable_text(&raw));
            self.final_response_candidate_truncated =
                final_response_stop_reason_is_nonterminal(response.stop_reason.as_deref());
            if !response.tool_calls.is_empty() {
                self.final_response_attempted_tool_activity = true;
                self.emit_signal(
                    LoopStep::Decide,
                    SignalKind::Blocked,
                    "dropping tool calls from final response",
                    serde_json::json!({"tool_call_count": response.tool_calls.len()}),
                );
                self.clear_tool_response_state();
                let decision = Decision::Respond(text);
                self.emit_decision_signals(&decision);
                return Ok(decision);
            }
            if !text.trim().is_empty() {
                self.clear_tool_response_state();
                let decision = Decision::Respond(text);
                self.emit_decision_signals(&decision);
                return Ok(decision);
            }
        }

        // Decompose takes priority over all other tool calls in the same response.
        // Other tool calls are intentionally discarded — the sub-goals will re-invoke tools as needed.
        if let Some(decompose_call) = find_decompose_tool_call(&response.tool_calls) {
            if !self.effective_decompose_enabled() {
                self.emit_signal(
                    LoopStep::Decide,
                    SignalKind::Trace,
                    "dropping decompose tool call because decomposition is disabled",
                    serde_json::json!({"tool_call_id": decompose_call.id}),
                );
                let non_decompose_calls: Vec<ToolCall> = response
                    .tool_calls
                    .iter()
                    .filter(|call| call.name != DECOMPOSE_TOOL_NAME)
                    .cloned()
                    .collect();
                if !non_decompose_calls.is_empty() {
                    self.capture_tool_response_state(response);
                    let decision = Decision::UseTools(non_decompose_calls);
                    self.emit_decision_signals(&decision);
                    return Ok(decision);
                }
                self.clear_tool_response_state();
                let raw = extract_response_text(response);
                let text = extract_readable_text(&raw);
                let decision = Decision::Respond(normalize_response_text(&text));
                self.emit_decision_signals(&decision);
                return Ok(decision);
            }
            self.clear_tool_response_state();
            if response.tool_calls.len() > 1 {
                self.emit_signal(
                    LoopStep::Decide,
                    SignalKind::Trace,
                    "decompose takes precedence; dropping other tool calls",
                    serde_json::json!({"dropped_count": response.tool_calls.len() - 1}),
                );
            }
            let plan = parse_decomposition_plan(
                &decompose_call.arguments,
                self.graph_of_thoughts_enabled,
            )?;
            let decision = Decision::Decompose(plan);
            self.emit_decision_signals(&decision);
            return Ok(decision);
        }

        if !response.tool_calls.is_empty() {
            self.capture_tool_response_state(response);
            let decision = Decision::UseTools(response.tool_calls.clone());
            self.emit_decision_signals(&decision);
            return Ok(decision);
        }

        self.clear_tool_response_state();
        let raw = extract_response_text(response);
        let text = extract_readable_text(&raw);
        let decision = Decision::Respond(normalize_response_text(&text));
        self.emit_decision_signals(&decision);
        Ok(decision)
    }

    /// Act step.
    async fn act(
        &mut self,
        decision: &Decision,
        llm: &dyn LlmProvider,
        context_messages: &[Message],
        stream: CycleStream<'_>,
    ) -> Result<ActionResult, LoopError> {
        if self.active_finalize_response().is_some()
            && matches!(decision, Decision::UseTools(_) | Decision::Decompose(_))
        {
            return Ok(self.finalize_response_tool_activity_action_result(decision));
        }

        match decision {
            // Note: Clarify and Defer are not produced by decide() in the current
            // loop engine flow, but are kept for external callers (Decision is pub).
            Decision::Respond(text) | Decision::Clarify(text) | Decision::Defer(text) => {
                Ok(self.text_action_result(decision, text))
            }
            Decision::UseTools(calls) => {
                let action = self
                    .act_with_tools(decision, calls, llm, context_messages, stream)
                    .await?;
                self.emit_action_signals(calls, &action.tool_results);
                Ok(action)
            }
            Decision::Decompose(plan) => {
                if let Some(gate_result) = self
                    .evaluate_decompose_gates(plan, decision, llm, context_messages)
                    .await
                {
                    return gate_result;
                }
                self.execute_decomposition(decision, plan, llm, context_messages)
                    .await
            }
        }
    }

    fn finalize_response_tool_activity_action_result(
        &mut self,
        decision: &Decision,
    ) -> ActionResult {
        let next_step = self.block_finalize_response_completion(
            None,
            FinalResponseViolation::ToolActivityAttempted,
        );
        ActionResult {
            decision: decision.clone(),
            tool_results: Vec::new(),
            response_text: String::new(),
            tokens_used: TokenUsage::default(),
            next_step,
        }
    }

    /// Evaluate decompose gates in order:
    /// batch detection → complexity floor → cost gate → child budget floor.
    ///
    /// Returns `Some(Ok(..))` if a gate fires (short-circuits decomposition),
    /// `Some(Err(..))` on execution error, or `None` to proceed with normal decomposition.
    async fn evaluate_decompose_gates(
        &mut self,
        plan: &DecompositionPlan,
        decision: &Decision,
        llm: &dyn LlmProvider,
        context_messages: &[Message],
    ) -> Option<Result<ActionResult, LoopError>> {
        if plan.reasoning_mode.is_standard() {
            if self.is_batch_plan(plan) {
                if let Some(calls) = self.batch_to_tool_calls(plan) {
                    self.emit_signal(
                        LoopStep::Act,
                        SignalKind::Trace,
                        "decompose_batch_detected",
                        serde_json::json!({
                            "sub_goal_count": plan.sub_goals.len(),
                            "common_tool": &plan.sub_goals[0].required_tools[0],
                        }),
                    );
                    return Some(self.route_as_tool_calls(calls, llm, context_messages).await);
                }
            }

            if self.is_trivial_plan(plan) {
                if let Some(calls) = self.batch_to_tool_calls(plan) {
                    self.emit_signal(
                        LoopStep::Act,
                        SignalKind::Trace,
                        "decompose_complexity_floor",
                        serde_json::json!({ "sub_goal_count": plan.sub_goals.len() }),
                    );
                    return Some(self.route_as_tool_calls(calls, llm, context_messages).await);
                }
            }
        }

        if let Some(gated) = self.evaluate_cost_gate(plan, decision) {
            return Some(gated);
        }

        if self.budget.state() != BudgetState::Low
            && self.all_sub_goals_below_decomposition_budget_floor(plan)
        {
            self.emit_signal(
                LoopStep::Act,
                SignalKind::Blocked,
                "decompose_budget_floor_gate",
                serde_json::json!({
                    "decision_kind": ControlPlaneDecisionKind::BudgetGuardrail,
                    "decision": "blocked",
                    "reason": "all_sub_goals_below_budget_floor",
                }),
            );
            return Some(
                self.execute_decomposition(decision, plan, llm, context_messages)
                    .await,
            );
        }

        None
    }

    /// Convert plan sub-goals to tool calls and route through `act_with_tools`.
    async fn route_as_tool_calls(
        &mut self,
        calls: Vec<ToolCall>,
        llm: &dyn LlmProvider,
        context_messages: &[Message],
    ) -> Result<ActionResult, LoopError> {
        let decision = Decision::UseTools(calls);
        let calls_ref = match &decision {
            Decision::UseTools(c) => c,
            _ => unreachable!(),
        };
        // Break the indirect async recursion cycle between act_with_tools ->
        // follow-up decompose handling -> route_as_tool_calls -> act_with_tools.
        Box::pin(self.act_with_tools(
            &decision,
            calls_ref,
            llm,
            context_messages,
            CycleStream::disabled(),
        ))
        .await
    }

    /// Gate 3: reject if estimated cost exceeds 150% of remaining budget.
    fn evaluate_cost_gate(
        &mut self,
        plan: &DecompositionPlan,
        decision: &Decision,
    ) -> Option<Result<ActionResult, LoopError>> {
        let remaining = self.budget.remaining(current_time_ms());
        let estimated = estimate_plan_cost(plan);
        if estimated.cost_cents > remaining.cost_cents.saturating_mul(3) / 2 {
            self.emit_signal(
                LoopStep::Act,
                SignalKind::Blocked,
                "decompose_cost_gate",
                serde_json::json!({
                    "estimated_cost_cents": estimated.cost_cents,
                    "remaining_cost_cents": remaining.cost_cents,
                }),
            );
            let result = self.text_action_result(
                decision,
                &format!(
                    "Decomposition plan rejected: estimated cost ({} cents) exceeds \
                     150% of remaining budget ({} cents). Please reformulate a smaller plan.",
                    estimated.cost_cents, remaining.cost_cents
                ),
            );
            return Some(Ok(result));
        }
        None
    }

    /// Check whether all sub-goals use the same single tool (batch detection).
    fn is_batch_plan(&self, plan: &DecompositionPlan) -> bool {
        plan.sub_goals.len() > 1
            && plan.sub_goals.iter().all(|sg| sg.required_tools.len() == 1)
            && plan
                .sub_goals
                .iter()
                .map(|sg| &sg.required_tools[0])
                .collect::<HashSet<_>>()
                .len()
                == 1
    }

    /// Check whether every sub-goal is trivially simple (complexity floor).
    ///
    /// Only triggers for parallel strategies (sequential implies inter-dependencies).
    /// Requires every sub-goal to have exactly one tool — zero-tool sub-goals cannot
    /// be routed through `act_with_tools` (no registered "noop" tool).
    fn is_trivial_plan(&self, plan: &DecompositionPlan) -> bool {
        matches!(plan.strategy, AggregationStrategy::Parallel)
            && plan.sub_goals.len() > 1
            && plan.sub_goals.iter().all(|sg| {
                sg.required_tools.len() == 1
                    && sg
                        .complexity_hint
                        .unwrap_or_else(|| estimate_complexity(sg))
                        == ComplexityHint::Trivial
            })
    }

    /// Convert sub-goals into synthetic `ToolCall` structs.
    ///
    /// Each sub-goal becomes a single tool call using its first required tool.
    /// Sub-goals with no required tools are filtered out — callers (batch
    /// detection & complexity floor) guarantee at least one tool per sub-goal.
    fn batch_to_tool_calls(&self, plan: &DecompositionPlan) -> Option<Vec<ToolCall>> {
        let mut calls = Vec::new();
        for (index, sub_goal) in plan
            .sub_goals
            .iter()
            .enumerate()
            .filter(|(_, sg)| !sg.required_tools.is_empty())
        {
            let call_id = format!("decompose-gate-{index}");
            let request = crate::act::SubGoalToolRoutingRequest {
                description: sub_goal.description.clone(),
                required_tools: sub_goal.required_tools.clone(),
            };
            let call = self.tool_executor.route_sub_goal_call(&request, &call_id)?;
            calls.push(call);
        }

        if calls.is_empty() {
            None
        } else {
            Some(calls)
        }
    }

    fn emit_sub_goal_progress(&mut self, index: usize, total: usize, description: &str) {
        self.emit_signal(
            LoopStep::Act,
            SignalKind::Trace,
            format!("Sub-goal {}/{}: {description}", index + 1, total),
            serde_json::json!({
                "sub_goal_index": index,
                "total": total,
            }),
        );
        if let Some(bus) = self.public_event_bus() {
            let _ = bus.publish(fx_core::message::InternalMessage::SubGoalStarted {
                index,
                total,
                description: description.to_string(),
            });
        }
    }

    fn emit_sub_goal_skipped(&mut self, index: usize, total: usize, description: &str) {
        self.emit_signal(
            LoopStep::Act,
            SignalKind::Friction,
            format!("Sub-goal {}/{} skipped: {description}", index + 1, total),
            serde_json::json!({
                "sub_goal_index": index,
                "total": total,
                "reason": "below_budget_floor",
            }),
        );
    }

    fn emit_decomposition_truncation_signal(
        &mut self,
        original_sub_goals: usize,
        retained_sub_goals: usize,
    ) {
        self.emit_signal(
            LoopStep::Act,
            SignalKind::Friction,
            "decomposition plan truncated to max sub-goals",
            serde_json::json!({
                "original_sub_goals": original_sub_goals,
                "retained_sub_goals": retained_sub_goals,
                "max_sub_goals": MAX_SUB_GOALS,
            }),
        );
    }

    fn roll_up_sub_goal_signals(&mut self, signals: &[Signal]) {
        self.signals.import_signals(signals);
    }

    fn emit_reason_trace_and_perf(&mut self, latency_ms: u64, usage: Option<&fx_llm::Usage>) {
        let metadata = usage
            .map(usage_trace_metadata)
            .unwrap_or_else(|| serde_json::json!({"usage": "unavailable"}));
        self.emit_signal(
            LoopStep::Reason,
            SignalKind::Trace,
            "LLM call completed",
            metadata,
        );
        self.emit_signal(
            LoopStep::Reason,
            SignalKind::Performance,
            "LLM latency",
            serde_json::json!({"latency_ms": latency_ms}),
        );
    }

    fn emit_context_overflow_signal(
        &mut self,
        scope: CompactionScope,
        before: ContextWindowStats,
        after_messages: &[Message],
    ) {
        let after = ContextWindowStats::capture(self, after_messages);
        if after.message_count >= before.message_count && after.token_count >= before.token_count {
            return;
        }

        self.emit_signal(
            scope.loop_step(),
            SignalKind::ContextOverflow,
            "conversation context compacted",
            signal_metadata_value(ContextOverflowSignalMetadata {
                scope: scope.as_str(),
                messages_before: before.message_count,
                messages_after: after.message_count,
                messages_removed: before.message_count.saturating_sub(after.message_count),
                tokens_before: before.token_count,
                tokens_after: after.token_count,
                tokens_evicted: before.token_count.saturating_sub(after.token_count),
                usage_ratio_before: before.usage_ratio,
                usage_ratio_after: after.usage_ratio,
            }),
        );
    }

    fn emit_tool_round_trace_and_perf(
        &mut self,
        round: u32,
        tool_calls: usize,
        response: &CompletionResponse,
        latency_ms: u64,
    ) {
        let mut metadata = serde_json::json!({
            "round": round,
            "tool_calls": tool_calls,
            "follow_up_calls": response.tool_calls.len(),
        });
        if let Some(usage) = response.usage {
            let usage_metadata = usage_trace_metadata(&usage);
            if let Some(object) = usage_metadata.as_object() {
                for (key, value) in object {
                    metadata[key] = value.clone();
                }
            }
        } else {
            metadata["usage"] = serde_json::json!("unavailable");
        }
        self.emit_signal(
            LoopStep::Act,
            SignalKind::Trace,
            "tool continuation round",
            metadata,
        );
        self.emit_signal(
            LoopStep::Act,
            SignalKind::Performance,
            "tool continuation latency",
            serde_json::json!({"round": round, "latency_ms": latency_ms}),
        );
    }

    fn emit_decision_signals(&mut self, decision: &Decision) {
        let variant = decision_variant(decision);
        self.emit_signal(
            LoopStep::Decide,
            SignalKind::Decision,
            "decision made",
            serde_json::json!({"variant": variant}),
        );
        if let Decision::UseTools(calls) = decision {
            if calls.len() > 1 {
                let tools = calls
                    .iter()
                    .map(|call| call.name.clone())
                    .collect::<Vec<_>>();
                self.emit_signal(
                    LoopStep::Decide,
                    SignalKind::Trace,
                    "multiple tools selected",
                    serde_json::json!({"tools": tools}),
                );
            }
        }
        if let Decision::Decompose(plan) = decision {
            self.emit_signal(
                LoopStep::Decide,
                SignalKind::Trace,
                "task decomposition initiated",
                serde_json::json!({
                    "sub_goals": plan.sub_goals.len(),
                    "strategy": format!("{:?}", plan.strategy),
                }),
            );
        }
    }

    fn emit_action_signals(&mut self, calls: &[ToolCall], results: &[ToolResult]) {
        for result in results {
            let call = calls
                .iter()
                .find(|candidate| candidate.id == result.tool_call_id);
            let classification = call
                .map(|call| self.tool_executor.classify_call(call))
                .unwrap_or_else(
                    || match self.tool_executor.cacheability(&result.tool_name) {
                        ToolCacheability::SideEffect => ToolCallClassification::Mutation,
                        ToolCacheability::Cacheable | ToolCacheability::NeverCache => {
                            ToolCallClassification::Observation
                        }
                    },
                );
            let kind = if result.success {
                SignalKind::Success
            } else {
                SignalKind::Friction
            };
            let output_chars = result.output.chars().count();
            let truncated_output = if output_chars > 500 {
                let prefix = result.output.chars().take(500).collect::<String>();
                format!("{prefix}… ({} bytes total)", result.output.len())
            } else {
                result.output.clone()
            };
            let mut metadata = serde_json::json!({
                "success": result.success,
                "output": truncated_output,
                "failure_class": result.failure_classification().map(|class| class.as_str()),
                "permanent": result
                    .failure_classification()
                    .is_some_and(FailureClass::is_permanent),
                "classification": SignalToolClassification::from(classification),
            });
            let diagnostics = self
                .pending_tool_result_diagnostics
                .remove(&result.tool_call_id);
            if !result.success {
                metadata["decision_kind"] =
                    serde_json::json!(ControlPlaneDecisionKind::ToolFailure);
                metadata["decision"] = serde_json::json!("failed");
                metadata["tool"] = serde_json::json!(result.tool_name);
                metadata["tool_call_id"] = serde_json::json!(result.tool_call_id);
                if let Some(diagnostics) = diagnostics.as_ref() {
                    metadata["diagnostics"] = diagnostics.as_metadata_value();
                }
            }
            let timeout_duration_ms = self
                .tool_executor
                .concurrency_policy()
                .timeout_per_call
                .and_then(|timeout| u64::try_from(timeout.as_millis()).ok());
            let elapsed_ms = diagnostics
                .as_ref()
                .and_then(ToolExecutionDiagnostics::duration_ms);
            let timed_out = result.is_timeout()
                || diagnostics
                    .as_ref()
                    .is_some_and(ToolExecutionDiagnostics::timed_out);
            let mut signal = Signal::new(
                LoopStep::Act,
                kind,
                format!("tool {}", result.tool_name),
                metadata,
                current_time_ms(),
            );
            if let Some(duration_ms) = elapsed_ms {
                signal = signal.with_duration_ms(duration_ms);
            }
            let signal_id = self.emit_structured_signal(signal);
            if timed_out {
                let mut timeout_signal = Signal::new(
                    LoopStep::Act,
                    SignalKind::Timeout,
                    format!("tool '{}' timed out", result.tool_name),
                    signal_metadata_value(TimeoutSignalMetadata {
                        decision_kind: ControlPlaneDecisionKind::ToolFailure,
                        tool: &result.tool_name,
                        tool_call_id: &result.tool_call_id,
                        failure_class: result.failure_classification().map(FailureClass::as_str),
                        permanent: result
                            .failure_classification()
                            .is_some_and(FailureClass::is_permanent),
                        timeout_ms: timeout_duration_ms,
                        elapsed_ms,
                    }),
                    current_time_ms(),
                )
                .with_cause_id(signal_id);
                if let Some(duration_ms) = elapsed_ms {
                    timeout_signal = timeout_signal.with_duration_ms(duration_ms);
                }
                let _ = self.emit_structured_signal(timeout_signal);
            }
            if let Some(call) = call {
                if result.success {
                    self.clear_tool_failure_signal(call);
                } else {
                    self.remember_tool_failure_signal(call, signal_id);
                }
            }
            if let Some(signals) = self
                .pending_tool_result_signals
                .remove(&result.tool_call_id)
            {
                for signal in signals {
                    let signal = if signal.cause_id.is_some() {
                        signal
                    } else {
                        signal.with_cause_id(signal_id)
                    };
                    let _ = self.emit_structured_signal(signal);
                }
            }
        }
    }

    fn capture_tool_execution_diagnostics(&mut self, results: &[ToolResult]) {
        for result in results {
            if let Some(diagnostics) = self
                .tool_executor
                .take_execution_diagnostics(&result.tool_call_id)
            {
                self.pending_tool_result_diagnostics
                    .insert(result.tool_call_id.clone(), diagnostics);
            }
            if let Some(signals) = self
                .tool_executor
                .take_emitted_signals(&result.tool_call_id)
            {
                self.pending_tool_result_signals
                    .insert(result.tool_call_id.clone(), signals);
            }
        }
    }

    /// Emit observability signals summarizing the action result.
    fn emit_action_observations(&mut self, action: &ActionResult) {
        let has_tool_failure = action.tool_results.iter().any(|r| !r.success);
        let has_response = !action.response_text.trim().is_empty();
        let has_tools = !action.tool_results.is_empty();

        if has_tool_failure && has_response {
            let failed: Vec<&str> = action
                .tool_results
                .iter()
                .filter(|r| !r.success)
                .map(|r| r.tool_name.as_str())
                .collect();
            self.emit_signal(
                LoopStep::Act,
                SignalKind::Observation,
                "tool_failure_with_response",
                serde_json::json!({
                    "failed_tools": failed,
                    "response_len": action.response_text.len(),
                }),
            );
        }
        if !has_response && !has_tools {
            self.emit_signal(
                LoopStep::Act,
                SignalKind::Observation,
                "empty_response",
                serde_json::json!({}),
            );
        }
        if has_tools && !has_response {
            self.emit_signal(
                LoopStep::Act,
                SignalKind::Observation,
                "tool_only_turn",
                serde_json::json!({"tool_count": action.tool_results.len()}),
            );
        }
    }

    fn append_compacted_summary(
        &self,
        snapshot: &PerceptionSnapshot,
        user_message: &str,
        context_window: &mut Vec<Message>,
    ) {
        let synthetic_context = self.synthetic_context(snapshot, user_message);
        if !self.context.needs_compaction(&synthetic_context) {
            return;
        }

        let compacted = self
            .context
            .compact(synthetic_context, TrimmingPolicy::ByRelevance);
        if let Some(summary) = compacted_context_summary(&compacted) {
            context_window.push(Message::assistant(summary.to_string()));
        }
    }

    fn text_action_result(&self, decision: &Decision, text: &str) -> ActionResult {
        let response_text = normalize_response_text(text);
        ActionResult {
            decision: decision.clone(),
            tool_results: Vec::new(),
            response_text: response_text.clone(),
            tokens_used: TokenUsage::default(),
            next_step: ActionNextStep::Finish(ActionTerminal::Complete {
                response: response_text,
            }),
        }
    }

    fn cancellation_token_triggered(&self) -> bool {
        self.cancel_token
            .as_ref()
            .map(CancellationToken::is_cancelled)
            .unwrap_or(false)
    }

    fn tool_round_interrupted(&mut self) -> bool {
        if self.cancellation_token_triggered() {
            return true;
        }

        if self.consume_stop_or_abort_command() {
            self.user_stop_requested = true;
            return true;
        }

        false
    }

    fn cancelled_tool_action(
        &self,
        decision: &Decision,
        tool_results: Vec<ToolResult>,
        partial_response: Option<String>,
        tokens_used: TokenUsage,
    ) -> ActionResult {
        ActionResult {
            decision: decision.clone(),
            tool_results,
            response_text: String::new(),
            tokens_used,
            next_step: ActionNextStep::Continue(ActionContinuation::new(partial_response, None)),
        }
    }

    fn cancelled_tool_action_from_state(
        &self,
        decision: &Decision,
        state: ToolRoundState,
    ) -> ActionResult {
        let partial_response = summarize_tool_progress(&state.all_tool_results);
        self.cancelled_tool_action(
            decision,
            state.all_tool_results,
            partial_response,
            state.tokens_used,
        )
    }

    async fn handle_follow_up_decompose(
        &mut self,
        response: &CompletionResponse,
        llm: &dyn LlmProvider,
        context_messages: &[Message],
        context: FollowUpDecomposeContext,
    ) -> Result<ActionResult, LoopError> {
        let FollowUpDecomposeContext {
            prior_tool_results,
            prior_tokens_used,
            accumulated_text,
        } = context;
        let Some(decompose_call) = find_decompose_tool_call(&response.tool_calls) else {
            return Err(loop_error(
                "act",
                "follow-up decompose handler called without a decompose tool call",
                false,
            ));
        };
        let mut accumulated_text = accumulated_text;
        push_response_segment(&mut accumulated_text, response_text_segment(response));

        self.clear_tool_response_state();
        if response.tool_calls.len() > 1 {
            self.emit_signal(
                LoopStep::Act,
                SignalKind::Trace,
                "decompose takes precedence; dropping other tool calls",
                serde_json::json!({"dropped_count": response.tool_calls.len() - 1}),
            );
        }

        let plan =
            parse_decomposition_plan(&decompose_call.arguments, self.graph_of_thoughts_enabled)?;
        let decision = Decision::Decompose(plan.clone());
        self.emit_decision_signals(&decision);

        let mut action = if let Some(gate_result) = self
            .evaluate_decompose_gates(&plan, &decision, llm, context_messages)
            .await
        {
            gate_result?
        } else {
            self.execute_decomposition(&decision, &plan, llm, context_messages)
                .await?
        };

        if !prior_tool_results.is_empty() {
            let mut merged_tool_results = prior_tool_results;
            merged_tool_results.extend(action.tool_results);
            action.tool_results = merged_tool_results;
        }
        action.tokens_used.accumulate(prior_tokens_used);
        Ok(prepend_accumulated_text_to_action(
            action,
            &accumulated_text,
        ))
    }

    fn estimate_reasoning_cost(&self, perception: &ProcessedPerception) -> ActionCost {
        let context_tokens = perception
            .context_window
            .iter()
            .map(message_to_text)
            .map(|text| estimate_tokens(&text))
            .sum::<u64>();

        let goal_tokens = perception
            .active_goals
            .iter()
            .map(|goal| estimate_tokens(goal))
            .sum::<u64>();

        let input_tokens = context_tokens
            .saturating_add(goal_tokens)
            .saturating_add(estimate_tokens(&perception.user_message))
            .max(64);

        let output_tokens = REASONING_OUTPUT_TOKEN_HEURISTIC;

        ActionCost {
            llm_calls: 1,
            tool_invocations: 0,
            tokens: input_tokens.saturating_add(output_tokens),
            cost_cents: DEFAULT_LLM_ACTION_COST_CENTS,
        }
    }

    fn estimate_action_cost(&self, decision: &Decision) -> ActionCost {
        match decision {
            Decision::UseTools(calls) => ActionCost {
                llm_calls: 1,
                tool_invocations: calls.len() as u32,
                tokens: TOOL_SYNTHESIS_TOKEN_HEURISTIC,
                cost_cents: DEFAULT_LLM_ACTION_COST_CENTS,
            },
            Decision::Respond(_) | Decision::Clarify(_) | Decision::Defer(_) => {
                ActionCost::default()
            }
            Decision::Decompose(plan) => ActionCost {
                llm_calls: plan.sub_goals.len() as u32,
                tool_invocations: 0,
                tokens: TOOL_SYNTHESIS_TOKEN_HEURISTIC * plan.sub_goals.len() as u64,
                cost_cents: DEFAULT_LLM_ACTION_COST_CENTS * plan.sub_goals.len() as u64,
            },
        }
    }

    fn action_cost_from_result(&self, action: &ActionResult) -> ActionCost {
        ActionCost {
            llm_calls: if action.tokens_used.total_tokens() > 0 {
                1
            } else {
                0
            },
            tool_invocations: action.tool_results.len() as u32,
            tokens: action.tokens_used.total_tokens(),
            cost_cents: if action.tokens_used.total_tokens() > 0 {
                DEFAULT_LLM_ACTION_COST_CENTS
            } else if action.has_tool_activity() {
                1
            } else {
                0
            },
        }
    }

    fn synthetic_context(
        &self,
        snapshot: &PerceptionSnapshot,
        user_message: &str,
    ) -> ReasoningContext {
        ReasoningContext {
            perception: snapshot.clone(),
            working_memory: vec![WorkingMemoryEntry {
                key: "user_message".to_string(),
                value: user_message.to_string(),
                relevance: 1.0,
            }],
            relevant_episodic: Vec::new(),
            relevant_semantic: Vec::new(),
            active_procedures: Vec::new(),
            identity_context: IdentityContext {
                user_name: None,
                preferences: HashMap::new(),
                personality_traits: vec!["helpful".to_string(), "safe".to_string()],
            },
            goal: Goal::new(
                format!("Respond to user: {user_message}"),
                vec!["Provide a useful and safe response".to_string()],
                Some(self.max_iterations),
            ),
            depth: 0,
            parent_context: None,
        }
    }
}

fn find_decompose_tool_call(tool_calls: &[ToolCall]) -> Option<&ToolCall> {
    tool_calls
        .iter()
        .find(|call| call.name == DECOMPOSE_TOOL_NAME)
}

fn decision_variant(decision: &Decision) -> &'static str {
    match decision {
        Decision::Respond(_) => "Respond",
        Decision::UseTools(_) => "UseTools",
        Decision::Clarify(_) => "Clarify",
        Decision::Defer(_) => "Defer",
        Decision::Decompose(_) => "Decompose",
    }
}

fn attach_signals(result: LoopResult, signals: Vec<Signal>) -> LoopResult {
    match result {
        LoopResult::Complete {
            response,
            iterations,
            tokens_used,
            ..
        } => LoopResult::Complete {
            response,
            iterations,
            tokens_used,
            signals,
        },
        LoopResult::BudgetExhausted {
            partial_response,
            iterations,
            ..
        } => LoopResult::BudgetExhausted {
            partial_response,
            iterations,
            signals,
        },
        LoopResult::Incomplete {
            partial_response,
            reason,
            iterations,
            ..
        } => LoopResult::Incomplete {
            partial_response,
            reason,
            iterations,
            signals,
        },
        LoopResult::UserStopped {
            partial_response,
            iterations,
            ..
        } => LoopResult::UserStopped {
            partial_response,
            iterations,
            signals,
        },
        LoopResult::Error {
            message,
            recoverable,
            ..
        } => LoopResult::Error {
            message,
            recoverable,
            signals,
        },
    }
}

/// Evict oldest tool results until aggregate token count fits within `max_tokens`.
///
/// Evicted results are replaced with stubs preserving `tool_call_id` and `tool_name`.
/// If a single remaining result still exceeds the limit, it is truncated in-place.
fn extract_user_message(snapshot: &PerceptionSnapshot) -> Result<String, LoopError> {
    let user_message = snapshot
        .user_input
        .as_ref()
        .map(|input| input.text.trim().to_string())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| snapshot.screen.text_content.trim().to_string());

    if user_message.is_empty() {
        return Err(loop_error(
            "perceive",
            "no user message or screen text available for processing",
            true,
        ));
    }

    Ok(user_message)
}

fn prioritize_flow_command(current: Option<LoopCommand>, incoming: LoopCommand) -> LoopCommand {
    match current {
        None => incoming,
        Some(existing) if loop_command_priority(&existing) > loop_command_priority(&incoming) => {
            existing
        }
        Some(existing)
            if loop_command_priority(&existing) == loop_command_priority(&incoming)
                && !matches!(incoming, LoopCommand::Wait | LoopCommand::Resume) =>
        {
            existing
        }
        _ => incoming,
    }
}

fn loop_command_priority(command: &LoopCommand) -> u8 {
    match command {
        LoopCommand::Abort => 5,
        LoopCommand::Stop => 4,
        LoopCommand::Wait | LoopCommand::Resume => 3,
        LoopCommand::StatusQuery => 2,
        LoopCommand::Steer(_) => 1,
    }
}

fn format_system_status_message(status: &LoopStatus) -> String {
    format!(
        "status: iter={}/{} llm={} tools={} tokens={} cost_cents={} remaining(llm={},tools={},tokens={},cost_cents={})",
        status.iteration_count,
        status.max_iterations,
        status.llm_calls_used,
        status.tool_invocations_used,
        status.tokens_used,
        status.cost_cents_used,
        status.remaining.llm_calls,
        status.remaining.tool_invocations,
        status.remaining.tokens,
        status.remaining.cost_cents,
    )
}

fn build_continuation_messages(base_messages: &[Message], full_text: &str) -> Vec<Message> {
    let mut continuation_messages = base_messages.to_vec();
    if !full_text.trim().is_empty() {
        continuation_messages.push(Message::assistant(full_text.to_string()));
    }
    continuation_messages.push(Message::user(
        "Continue from exactly where you left off. Do not repeat prior text.",
    ));
    continuation_messages
}

fn stream_phase_for_step(step: LoopStep) -> StreamPhase {
    match step {
        LoopStep::Reason => StreamPhase::Reason,
        LoopStep::Act => StreamPhase::Synthesize,
        _ => StreamPhase::Synthesize,
    }
}

fn continuation_budget_cost_estimate(messages: &[Message]) -> ActionCost {
    let input_tokens = messages
        .iter()
        .map(message_to_text)
        .map(|text| estimate_tokens(&text))
        .sum::<u64>();

    ActionCost {
        llm_calls: 1,
        tool_invocations: 0,
        tokens: input_tokens.saturating_add(REASONING_OUTPUT_TOKEN_HEURISTIC),
        cost_cents: DEFAULT_LLM_ACTION_COST_CENTS,
    }
}

fn continuation_budget_cost(
    response: &CompletionResponse,
    continuation_messages: &[Message],
) -> ActionCost {
    let usage = response_usage_or_estimate(response, continuation_messages);
    ActionCost {
        llm_calls: 1,
        tool_invocations: 0,
        tokens: usage.total_tokens(),
        cost_cents: DEFAULT_LLM_ACTION_COST_CENTS,
    }
}

fn merge_continuation_response(
    previous: CompletionResponse,
    continued: CompletionResponse,
    full_text: &mut String,
) -> CompletionResponse {
    let new_text = extract_response_text(&continued);
    let deduped = trim_duplicate_seam(full_text, &new_text, 120, 80);
    full_text.push_str(&deduped);

    CompletionResponse {
        content: vec![ContentBlock::Text {
            text: full_text.clone(),
        }],
        tool_calls: merge_tool_calls(previous.tool_calls, continued.tool_calls),
        usage: merge_usage(previous.usage, continued.usage),
        stop_reason: continued.stop_reason,
    }
}

fn merge_tool_calls(previous: Vec<ToolCall>, continued: Vec<ToolCall>) -> Vec<ToolCall> {
    let mut merged = previous;
    for call in continued {
        if !tool_call_exists(&merged, &call) {
            merged.push(call);
        }
    }
    merged
}

fn tool_call_exists(existing: &[ToolCall], candidate: &ToolCall) -> bool {
    if !candidate.id.trim().is_empty() {
        return existing.iter().any(|call| call.id == candidate.id);
    }

    existing.iter().any(|call| {
        call.id.trim().is_empty()
            && call.name == candidate.name
            && call.arguments == candidate.arguments
    })
}

fn is_truncated(stop_reason: Option<&str>) -> bool {
    matches!(
        stop_reason.map(|s| s.to_ascii_lowercase()).as_deref(),
        Some("max_tokens" | "length" | "incomplete")
    )
}

fn merge_usage(left: Option<Usage>, right: Option<Usage>) -> Option<Usage> {
    if left.is_none() && right.is_none() {
        return None;
    }

    let left_in = left.as_ref().map(|u| u.input_tokens).unwrap_or(0);
    let left_out = left.as_ref().map(|u| u.output_tokens).unwrap_or(0);
    let left_cached = left.as_ref().map(|u| u.cached_input_tokens).unwrap_or(0);
    let left_cache_creation = left
        .as_ref()
        .map(|u| u.cache_creation_input_tokens)
        .unwrap_or(0);
    let right_in = right.as_ref().map(|u| u.input_tokens).unwrap_or(0);
    let right_out = right.as_ref().map(|u| u.output_tokens).unwrap_or(0);
    let right_cached = right.as_ref().map(|u| u.cached_input_tokens).unwrap_or(0);
    let right_cache_creation = right
        .as_ref()
        .map(|u| u.cache_creation_input_tokens)
        .unwrap_or(0);

    Some(Usage {
        input_tokens: left_in.saturating_add(right_in),
        output_tokens: left_out.saturating_add(right_out),
        cached_input_tokens: left_cached.saturating_add(right_cached),
        cache_creation_input_tokens: left_cache_creation.saturating_add(right_cache_creation),
    })
}

fn usage_trace_metadata(usage: &Usage) -> serde_json::Value {
    serde_json::json!({
        "input_tokens": usage.input_tokens,
        "output_tokens": usage.output_tokens,
        "cached_input_tokens": usage.cached_input_tokens,
        "cache_creation_input_tokens": usage.cache_creation_input_tokens,
        "total_tokens": usage.input_tokens.saturating_add(usage.output_tokens),
    })
}

fn trim_duplicate_seam(
    full_text: &str,
    new_text: &str,
    overlap_window: usize,
    min_overlap: usize,
) -> String {
    if full_text.is_empty() || new_text.is_empty() {
        return new_text.to_string();
    }

    let full_chars = full_text.chars().collect::<Vec<_>>();
    let new_chars = new_text.chars().collect::<Vec<_>>();
    let max_overlap = overlap_window.min(full_chars.len()).min(new_chars.len());
    if max_overlap < min_overlap {
        return new_text.to_string();
    }

    for overlap in (min_overlap..=max_overlap).rev() {
        let full_suffix = &full_chars[full_chars.len() - overlap..];
        let new_prefix = &new_chars[..overlap];
        if full_suffix == new_prefix {
            return new_chars[overlap..].iter().collect();
        }
    }

    new_text.to_string()
}

fn response_usage_or_estimate(
    response: &CompletionResponse,
    context_messages: &[Message],
) -> TokenUsage {
    if let Some(usage) = response.usage {
        return TokenUsage::from_llm_usage(usage);
    }

    let prompt_estimate: u64 = context_messages
        .iter()
        .flat_map(|m| &m.content)
        .map(|block| match block {
            ContentBlock::Text { text } => estimate_tokens(text),
            ContentBlock::ToolUse { input, .. } => estimate_tokens(&input.to_string()),
            ContentBlock::ToolResult { content, .. } => estimate_tokens(&content.to_string()),
            ContentBlock::Image { data, .. } => estimate_tokens(data),
            ContentBlock::Document { data, .. } => estimate_tokens(data),
        })
        .sum();
    let text = extract_response_text(response);
    TokenUsage {
        input_tokens: prompt_estimate,
        output_tokens: estimate_tokens(&text),
        ..Default::default()
    }
}

fn reasoning_token_usage(total_tokens: u64) -> TokenUsage {
    TokenUsage {
        input_tokens: total_tokens.saturating_mul(3) / 5,
        output_tokens: total_tokens.saturating_mul(2) / 5,
        ..Default::default()
    }
}

fn estimate_tokens(text: &str) -> u64 {
    estimate_text_tokens(text) as u64
}

fn message_to_text(message: &Message) -> String {
    let role = format!("{:?}", message.role);
    let content = message_content_to_text(message);

    format!("{role}: {content}")
}

fn message_content_to_text(message: &Message) -> String {
    message
        .content
        .iter()
        .map(|block| match block {
            fx_llm::ContentBlock::Text { text } => text.clone(),
            fx_llm::ContentBlock::ToolUse { name, input, .. } => {
                format!("[tool_use:{name}] {input}")
            }
            fx_llm::ContentBlock::ToolResult {
                tool_use_id,
                content,
            } => {
                format!("[tool_result:{tool_use_id}] {content}")
            }
            fx_llm::ContentBlock::Image { media_type, .. } => format!("[image:{media_type}]"),
            fx_llm::ContentBlock::Document {
                media_type,
                filename,
                ..
            } => filename
                .as_ref()
                .map(|filename| format!("[document:{media_type}:{filename}]"))
                .unwrap_or_else(|| format!("[document:{media_type}]")),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn extract_requested_write_target(user_message: &str) -> Option<String> {
    const PREFIXES: [&str; 4] = ["save it to ", "save to ", "write it to ", "write to "];
    let lower = user_message.to_lowercase();
    for prefix in PREFIXES {
        let Some(start) = lower.find(prefix) else {
            continue;
        };
        let raw = user_message[start + prefix.len()..]
            .split_whitespace()
            .next()?;
        let cleaned = raw
            .trim_matches(|c: char| matches!(c, '"' | '\'' | ')' | ']' | '>' | ',' | ';'))
            .trim_end_matches('.')
            .trim();
        if looks_like_artifact_path(cleaned) {
            return Some(cleaned.to_string());
        }
    }

    for (start, _) in lower.match_indices(" to ") {
        let prefix_context = lower[..start].trim_end();
        if !prefix_context_contains_recent_write_verb(prefix_context) {
            continue;
        }
        let raw = user_message[start + 4..].split_whitespace().next()?;
        let cleaned = raw
            .trim_matches(|c: char| matches!(c, '"' | '\'' | ')' | ']' | '>' | ',' | ';'))
            .trim_end_matches('.')
            .trim();
        if looks_like_artifact_path(cleaned) {
            return Some(cleaned.to_string());
        }
    }
    None
}

// Keep the fallback matcher precise: only recent standalone write verbs should
// activate it, so substring hits like "unsafe" and distant earlier verbs do
// not accidentally create an artifact gate.
fn prefix_context_contains_recent_write_verb(prefix_context: &str) -> bool {
    const VERBS: [&str; 4] = ["write", "save", "append", "create"];
    prefix_context
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .rev()
        .take(8)
        .any(|token| VERBS.contains(&token))
}

fn extract_requested_external_actions(user_message: &str) -> Vec<RootTurnExternalActionKind> {
    let mut actions = Vec::new();
    if requests_github_pr_comment(user_message) {
        actions.push(RootTurnExternalActionKind::GitHubPrComment);
    }
    if requests_github_issue_comment(user_message) {
        actions.push(RootTurnExternalActionKind::GitHubIssueComment);
    }
    if requests_github_pr_review(user_message) {
        actions.push(RootTurnExternalActionKind::GitHubPrReview);
    }
    if requests_github_pr_create(user_message) {
        actions.push(RootTurnExternalActionKind::GitHubPrCreate);
    }
    if requests_git_push(user_message) {
        actions.push(RootTurnExternalActionKind::GitPush);
    }
    actions
}

fn requests_local_mutation_work(user_message: &str) -> bool {
    let normalized = normalize_contract_label(user_message);
    if normalized.contains("do not edit")
        || normalized.contains("don t edit")
        || normalized.contains("without editing")
        || normalized.contains("only inspect")
        || normalized.contains("tell me what")
    {
        return false;
    }

    const MUTATION_REQUEST_VERBS: &[&str] = &[
        "add",
        "adds",
        "address",
        "addresses",
        "change",
        "changes",
        "delete",
        "deletes",
        "fix",
        "fixed",
        "fixes",
        "implement",
        "implemented",
        "implements",
        "patch",
        "refactor",
        "refactors",
        "remove",
        "removes",
        "rename",
        "renames",
        "resolve",
        "resolves",
        "update",
        "updates",
    ];
    const MUTATION_REQUEST_TARGETS: &[&str] = &[
        "branch", "bug", "bugs", "code", "crate", "file", "function", "harness", "issue", "issues",
        "module", "package", "pr", "repo", "review", "tests", "test",
    ];

    let has_mutation_verb = normalized
        .split_whitespace()
        .any(|token| MUTATION_REQUEST_VERBS.contains(&token));
    let target_tokens = normalized.split_whitespace().collect::<HashSet<_>>();
    let has_code_target = MUTATION_REQUEST_TARGETS
        .iter()
        .any(|target| target_tokens.contains(target));

    has_mutation_verb && has_code_target
}

fn requests_github_pr_comment(user_message: &str) -> bool {
    let normalized = normalize_contract_label(user_message);
    if normalized.contains("do not post")
        || normalized.contains("don t post")
        || normalized.contains("without posting")
    {
        return false;
    }

    let has_comment = normalized.contains(" comment") || normalized.contains("comment ");
    let has_posting_verb = ["post", "submit", "publish", "add", "leave"]
        .iter()
        .any(|verb| normalized.split_whitespace().any(|token| token == *verb));
    let has_pr_target = normalized.split_whitespace().any(|token| token == "pr")
        || normalized.contains("pull request");

    has_comment && has_posting_verb && has_pr_target
}

fn requests_github_issue_comment(user_message: &str) -> bool {
    let normalized = normalize_contract_label(user_message);
    if normalized.contains("do not post")
        || normalized.contains("don t post")
        || normalized.contains("without posting")
    {
        return false;
    }

    let has_comment = normalized.contains(" comment") || normalized.contains("comment ");
    let has_posting_verb = ["post", "submit", "publish", "add", "leave"]
        .iter()
        .any(|verb| normalized.split_whitespace().any(|token| token == *verb));
    let has_issue_target = normalized.split_whitespace().any(|token| token == "issue")
        || normalized.contains("github issue");
    let has_pr_target = normalized.split_whitespace().any(|token| token == "pr")
        || normalized.contains("pull request");

    has_comment && has_posting_verb && has_issue_target && !has_pr_target
}

fn requests_github_pr_review(user_message: &str) -> bool {
    let normalized = normalize_contract_label(user_message);
    if normalized.contains("do not approve")
        || normalized.contains("don t approve")
        || normalized.contains("without approving")
        || normalized.contains("without review")
    {
        return false;
    }

    let has_pr_target = normalized.split_whitespace().any(|token| token == "pr")
        || normalized.contains("pull request");
    let asks_for_review_submission = normalized.contains("approve")
        || normalized.contains("approval")
        || normalized.contains("request changes")
        || normalized.contains("submit review")
        || normalized.contains("post review")
        || normalized.contains("leave review");

    has_pr_target && asks_for_review_submission
}

fn requests_github_pr_create(user_message: &str) -> bool {
    let normalized = normalize_contract_label(user_message);
    if normalized.contains("do not open")
        || normalized.contains("don t open")
        || normalized.contains("without opening")
        || normalized.contains("without creating")
    {
        return false;
    }

    let has_pr_target = normalized.split_whitespace().any(|token| token == "pr")
        || normalized.contains("pull request");
    let asks_for_creation = normalized.contains("open")
        || normalized.contains("create")
        || normalized.contains("raise")
        || normalized.contains("submit");

    has_pr_target && asks_for_creation
}

fn requests_git_push(user_message: &str) -> bool {
    let normalized = normalize_contract_label(user_message);
    if normalized.contains("do not push")
        || normalized.contains("don t push")
        || normalized.contains("without pushing")
    {
        return false;
    }

    normalized.split_whitespace().any(|token| token == "push")
        && (normalized.contains(" git")
            || normalized.contains(" branch")
            || normalized.contains(" remote")
            || normalized.contains(" commit")
            || normalized.contains(" pr"))
}

fn external_action_label(kind: RootTurnExternalActionKind) -> &'static str {
    match kind {
        RootTurnExternalActionKind::GitHubPrComment => "Post a comment on the GitHub pull request",
        RootTurnExternalActionKind::GitHubIssueComment => "Post a comment on the GitHub issue",
        RootTurnExternalActionKind::GitHubPrReview => "Submit the GitHub pull request review",
        RootTurnExternalActionKind::GitHubPrCreate => "Open the GitHub pull request",
        RootTurnExternalActionKind::GitPush => "Push changes to the git remote",
    }
}

fn external_action_typed_tool_names(kind: RootTurnExternalActionKind) -> &'static [&'static str] {
    match kind {
        RootTurnExternalActionKind::GitHubPrComment => &["comment_pr", "github_pr_comment"],
        RootTurnExternalActionKind::GitHubIssueComment => {
            &["comment_issue", "github_issue_comment"]
        }
        RootTurnExternalActionKind::GitHubPrReview => &["review_pr", "github_pr_review"],
        RootTurnExternalActionKind::GitHubPrCreate => &["create_pr", "github_pr_create"],
        RootTurnExternalActionKind::GitPush => &["git_push"],
    }
}

fn external_action_fallback_tool_names(
    kind: RootTurnExternalActionKind,
) -> &'static [&'static str] {
    match kind {
        RootTurnExternalActionKind::GitHubPrComment
        | RootTurnExternalActionKind::GitHubIssueComment
        | RootTurnExternalActionKind::GitHubPrReview
        | RootTurnExternalActionKind::GitHubPrCreate
        | RootTurnExternalActionKind::GitPush => &["run_command"],
    }
}

fn external_action_tool_names(kind: RootTurnExternalActionKind) -> &'static [&'static str] {
    match kind {
        RootTurnExternalActionKind::GitHubPrComment => {
            &["comment_pr", "github_pr_comment", "run_command"]
        }
        RootTurnExternalActionKind::GitHubIssueComment => {
            &["comment_issue", "github_issue_comment", "run_command"]
        }
        RootTurnExternalActionKind::GitHubPrReview => {
            &["review_pr", "github_pr_review", "run_command"]
        }
        RootTurnExternalActionKind::GitHubPrCreate => {
            &["create_pr", "github_pr_create", "run_command"]
        }
        RootTurnExternalActionKind::GitPush => &["git_push", "run_command"],
    }
}

fn available_external_action_tool_names(
    available_tools: &[ToolDefinition],
    kind: RootTurnExternalActionKind,
) -> Vec<String> {
    let available_names: HashSet<&str> = available_tools
        .iter()
        .map(|tool| tool.name.as_str())
        .collect();

    let typed = external_action_typed_tool_names(kind)
        .iter()
        .filter(|name| available_names.contains(**name))
        .map(|name| (*name).to_string())
        .collect::<Vec<_>>();
    if !typed.is_empty() {
        return typed;
    }

    let fallback = external_action_fallback_tool_names(kind)
        .iter()
        .filter(|name| available_names.contains(**name))
        .map(|name| (*name).to_string())
        .collect::<Vec<_>>();
    if !fallback.is_empty() {
        return fallback;
    }

    external_action_tool_names_owned(kind)
}

fn external_action_tool_names_owned(kind: RootTurnExternalActionKind) -> Vec<String> {
    external_action_tool_names(kind)
        .iter()
        .map(|name| (*name).to_string())
        .collect()
}

fn external_action_completed(
    kind: RootTurnExternalActionKind,
    calls: &[ToolCall],
    results: &[ToolResult],
    typed_completed_actions: &HashSet<RootTurnExternalActionKind>,
) -> bool {
    if typed_completed_actions.contains(&kind) {
        return true;
    }

    match kind {
        RootTurnExternalActionKind::GitHubPrComment => {
            github_pr_comment_completed_by_successful_typed_tool(calls, results)
                || github_pr_comment_completed_by_legacy_shell_inference(calls, results)
        }
        RootTurnExternalActionKind::GitHubIssueComment => {
            github_issue_comment_completed_by_successful_typed_tool(calls, results)
        }
        RootTurnExternalActionKind::GitHubPrReview => {
            typed_completed_actions.contains(&RootTurnExternalActionKind::GitHubPrReview)
                || github_pr_review_completed_by_successful_typed_tool(calls, results)
        }
        RootTurnExternalActionKind::GitHubPrCreate => {
            github_pr_create_completed_by_successful_typed_tool(calls, results)
                || github_pr_create_completed_by_legacy_shell_inference(calls, results)
        }
        RootTurnExternalActionKind::GitPush => {
            typed_completed_actions.contains(&RootTurnExternalActionKind::GitPush)
                || git_push_completed_by_successful_typed_tool(calls, results)
        }
    }
}

fn completed_external_actions_from_diagnostics(
    results: &[ToolResult],
    diagnostics: &HashMap<String, ToolExecutionDiagnostics>,
) -> HashSet<RootTurnExternalActionKind> {
    let mut completed = HashSet::new();
    // Typed evidence only satisfies an external-action deliverable when the
    // producing tool result succeeded. A parsed `git push` command from a
    // rejected/failed shell result is useful diagnostic metadata, but it must
    // not close the user-visible contract.
    for result in results.iter().filter(|result| result.success) {
        let Some(diagnostics) = diagnostics.get(&result.tool_call_id) else {
            continue;
        };
        for evidence in diagnostics.external_actions() {
            let kind = match evidence.kind {
                ExternalActionKind::GithubPrComment => RootTurnExternalActionKind::GitHubPrComment,
                ExternalActionKind::GithubIssueComment => {
                    RootTurnExternalActionKind::GitHubIssueComment
                }
                ExternalActionKind::GithubPrReview => RootTurnExternalActionKind::GitHubPrReview,
                ExternalActionKind::GitPush => RootTurnExternalActionKind::GitPush,
            };
            completed.insert(kind);
        }
    }
    completed
}

fn github_pr_comment_completed_by_legacy_shell_inference(
    calls: &[ToolCall],
    results: &[ToolResult],
) -> bool {
    successful_call_evidence(calls, results).any(|(call, result)| {
        call.name == "github_pr_comment"
            || (call.name == "run_command"
                && run_command_posts_github_pr_comment(&call.arguments, &result.output))
    })
}

fn github_pr_comment_completed_by_successful_typed_tool(
    calls: &[ToolCall],
    results: &[ToolResult],
) -> bool {
    successful_call_evidence(calls, results)
        .any(|(call, _)| matches!(call.name.as_str(), "comment_pr" | "github_pr_comment"))
}

fn github_issue_comment_completed_by_successful_typed_tool(
    calls: &[ToolCall],
    results: &[ToolResult],
) -> bool {
    successful_call_evidence(calls, results)
        .any(|(call, _)| matches!(call.name.as_str(), "comment_issue" | "github_issue_comment"))
}

fn github_pr_review_completed_by_successful_typed_tool(
    calls: &[ToolCall],
    results: &[ToolResult],
) -> bool {
    successful_call_evidence(calls, results)
        .any(|(call, _)| matches!(call.name.as_str(), "review_pr" | "github_pr_review"))
}

fn github_pr_create_completed_by_successful_typed_tool(
    calls: &[ToolCall],
    results: &[ToolResult],
) -> bool {
    successful_call_evidence(calls, results)
        .any(|(call, _)| matches!(call.name.as_str(), "create_pr" | "github_pr_create"))
}

fn github_pr_create_completed_by_legacy_shell_inference(
    calls: &[ToolCall],
    results: &[ToolResult],
) -> bool {
    successful_call_evidence(calls, results).any(|(call, result)| {
        call.name == "run_command" && run_command_creates_github_pr(&call.arguments, &result.output)
    })
}

fn git_push_completed_by_successful_typed_tool(calls: &[ToolCall], results: &[ToolResult]) -> bool {
    successful_call_evidence(calls, results)
        .any(|(call, _)| matches!(call.name.as_str(), "git_push"))
}

fn successful_call_evidence<'a>(
    calls: &'a [ToolCall],
    results: &'a [ToolResult],
) -> impl Iterator<Item = (&'a ToolCall, &'a ToolResult)> {
    calls.iter().filter_map(|call| {
        results
            .iter()
            .find(|result| result.success && result.tool_call_id == call.id)
            .map(|result| (call, result))
    })
}

fn run_command_posts_github_pr_comment(arguments: &serde_json::Value, output: &str) -> bool {
    !crate::act::external_actions_from_run_command_arguments(arguments, output).is_empty()
}

fn run_command_creates_github_pr(arguments: &serde_json::Value, output: &str) -> bool {
    let output = output.to_ascii_lowercase();
    if !output.contains("github.com/") || !output.contains("/pull/") {
        return false;
    }

    normalize_contract_label(&arguments.to_string()).contains("gh pr create")
}

#[cfg(test)]
fn command_posts_github_pr_comment(command: &str) -> bool {
    command_posts_github_pr_comment_with_output(command, "")
}

#[cfg(test)]
fn command_posts_github_pr_comment_with_output(command: &str, output: &str) -> bool {
    !crate::act::external_actions_from_run_command(command, output).is_empty()
}

fn extract_task_contract_from_response(response: &CompletionResponse) -> TaskContractExtraction {
    let raw = extract_response_text(response);
    let readable = extract_readable_text(&raw);
    extract_task_contract_from_text(&readable)
}

fn tool_response_final_text(
    response: &CompletionResponse,
    visible_text: Option<String>,
) -> Option<String> {
    // Text emitted in the same model response as tool calls is a live narration
    // channel for clients. It may explain what the agent is checking next, but
    // it is not final answer content and must not be stitched into terminal
    // responses or partial-response fallbacks.
    response
        .tool_calls
        .is_empty()
        .then_some(visible_text)
        .flatten()
}

fn extract_task_contract_from_text(text: &str) -> TaskContractExtraction {
    let lines: Vec<&str> = text.lines().collect();
    let Some(header_index) = lines
        .iter()
        .position(|line| line.trim().eq_ignore_ascii_case("task plan:"))
    else {
        return TaskContractExtraction {
            contract: None,
            visible_text: meaningful_response_text(text),
        };
    };

    let mut descriptions = Vec::new();
    let mut end_index = header_index + 1;
    while end_index < lines.len() {
        let trimmed = lines[end_index].trim();
        if trimmed.is_empty() {
            end_index += 1;
            if !descriptions.is_empty() {
                break;
            }
            continue;
        }

        let Some(item) = parse_list_item(trimmed) else {
            break;
        };
        let description = sanitize_task_contract_label(item);
        if !description.is_empty() {
            descriptions.push(description);
        }
        end_index += 1;
    }

    if descriptions.is_empty() {
        return TaskContractExtraction {
            contract: None,
            visible_text: meaningful_response_text(text),
        };
    }

    let mut visible_lines = Vec::new();
    visible_lines.extend_from_slice(&lines[..header_index]);
    visible_lines.extend_from_slice(&lines[end_index..]);
    let visible_text = meaningful_response_text(&visible_lines.join("\n"));

    TaskContractExtraction {
        contract: Some(TaskContract {
            inputs: descriptions
                .into_iter()
                .map(|description| InputRequirement {
                    normalized_description: normalize_contract_label(&description),
                    description,
                    satisfied: false,
                })
                .collect(),
            phase: TaskPhase::Gathering,
        }),
        visible_text,
    }
}

fn extract_root_turn_contract(user_message: &str) -> RootTurnContractExtraction {
    let deliverable_parse = extract_deliverable_response_sections(user_message);
    let response_labels = deliverable_parse.labels;
    let mut deliverables = response_labels
        .into_iter()
        .map(|label| RootTurnDeliverable::ResponseSection {
            normalized_label: normalize_contract_label(&label),
            label,
        })
        .collect::<Vec<_>>();

    if requests_local_mutation_work(user_message) {
        deliverables.push(RootTurnDeliverable::MutationWork {
            label: "Complete the requested code or file changes".to_string(),
            satisfied: false,
        });
    }

    if let Some(path) = extract_requested_write_target(user_message) {
        if !deliverables.iter().any(|deliverable| {
            matches!(
                deliverable,
                RootTurnDeliverable::ArtifactWrite {
                    path: existing,
                    ..
                } if existing == &path
            )
        }) {
            deliverables.push(RootTurnDeliverable::ArtifactWrite {
                path,
                satisfied: false,
            });
        }
    }

    for action in extract_requested_external_actions(user_message) {
        if !deliverables.iter().any(|deliverable| {
            matches!(
                deliverable,
                RootTurnDeliverable::ExternalAction { kind, .. } if *kind == action
            )
        }) {
            deliverables.push(RootTurnDeliverable::ExternalAction {
                kind: action,
                label: external_action_label(action).to_string(),
                satisfied: false,
            });
        }
    }

    RootTurnContractExtraction {
        contract: (!deliverables.is_empty()).then_some(RootTurnContract {
            deliverables,
            blocked_terminal_attempts: 0,
        }),
        deliverable_block_count: deliverable_parse.block_count,
    }
}

// Only the first explicit `Deliverables:` block becomes the root-turn contract.
// Later blocks are ignored so the kernel has a single deterministic checklist.
fn extract_deliverable_response_sections(user_message: &str) -> DeliverableSectionParse {
    let mut lines = user_message.lines().peekable();
    let mut first_labels = None;
    let mut block_count = 0;
    while let Some(line) = lines.next() {
        if !line.trim().eq_ignore_ascii_case("deliverables:") {
            continue;
        }

        block_count += 1;
        let items = parse_deliverables_block(&mut lines);
        if first_labels.is_none() {
            first_labels = Some(items);
        }
    }
    DeliverableSectionParse {
        labels: first_labels
            .unwrap_or_default()
            .into_iter()
            .map(|item| sanitize_deliverable_label(&item))
            .filter(|label| !label.is_empty())
            .collect(),
        block_count,
    }
}

fn parse_deliverables_block<'a, I>(lines: &mut std::iter::Peekable<I>) -> Vec<String>
where
    I: Iterator<Item = &'a str>,
{
    let mut items = Vec::new();
    while let Some(next_line) = lines.peek() {
        let trimmed = next_line.trim();
        if trimmed.is_empty() {
            lines.next();
            if !items.is_empty() {
                break;
            }
            continue;
        }
        let Some(item) = parse_list_item(trimmed) else {
            break;
        };
        items.push(item.to_string());
        lines.next();
    }
    items
}

fn parse_list_item(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    for prefix in ["- ", "* ", "+ "] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return Some(rest.trim());
        }
    }

    let digit_count = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
    if digit_count == 0 {
        return None;
    }
    let suffix = trimmed[digit_count..].chars().next()?;
    if !matches!(suffix, '.' | ')') {
        return None;
    }
    Some(trimmed[digit_count + suffix.len_utf8()..].trim())
}

fn sanitize_deliverable_label(item: &str) -> String {
    item.trim()
        .trim_matches(|c: char| matches!(c, '*' | '_' | '`'))
        .trim_end_matches(':')
        .trim()
        .to_string()
}

fn sanitize_task_contract_label(item: &str) -> String {
    let trimmed = sanitize_deliverable_label(item);
    for prefix in ["[ ]", "[x]", "[X]"] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return rest.trim().to_string();
        }
    }
    trimmed
}

fn normalize_contract_label(text: &str) -> String {
    text.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

// Only ask for an internal task plan when the user has already named multiple
// concrete inputs. That keeps one-off tool turns out of unnecessary planning
// loops while preserving the structural contract for real context gathering.
fn should_request_task_contract_declaration(user_message: &str) -> bool {
    let mut references = HashSet::new();
    let tokens: Vec<&str> = user_message.split_whitespace().collect();

    for window in tokens.windows(2) {
        let [keyword, number] = window else {
            continue;
        };
        if normalize_contract_label(keyword) != "issue" {
            continue;
        }
        let cleaned = trim_task_contract_reference_token(number);
        if !cleaned.is_empty() && cleaned.chars().all(|ch| ch.is_ascii_digit()) {
            references.insert(format!("issue-{cleaned}"));
        }
    }

    for token in tokens {
        let cleaned = trim_task_contract_reference_token(token);
        if looks_like_task_contract_reference(cleaned) {
            references.insert(cleaned.to_ascii_lowercase());
        }
    }

    references.len() >= 2
}

fn extract_user_evidence_references(user_message: &str) -> Vec<String> {
    let tokens = user_message.split_whitespace().collect::<Vec<_>>();
    let mut references = Vec::new();
    let mut seen = HashSet::new();

    for index in 0..tokens.len() {
        let token = trim_task_contract_reference_token(tokens[index]);
        let normalized = normalize_contract_label(token);
        let next = tokens
            .get(index + 1)
            .map(|token| trim_task_contract_reference_token(token));
        let next_next = tokens
            .get(index + 2)
            .map(|token| trim_task_contract_reference_token(token));

        if matches!(normalized.as_str(), "pr" | "pull request") {
            if let Some(value) = next.and_then(normalize_numeric_reference) {
                push_unique_evidence_reference(&mut references, &mut seen, format!("pr {value}"));
            }
        } else if normalized == "pull" {
            let next_keyword = next.map(normalize_contract_label);
            if next_keyword.as_deref() == Some("request") {
                if let Some(value) = next_next.and_then(normalize_numeric_reference) {
                    push_unique_evidence_reference(
                        &mut references,
                        &mut seen,
                        format!("pr {value}"),
                    );
                }
            }
        } else if normalized == "issue" {
            if let Some(value) = next.and_then(normalize_numeric_reference) {
                push_unique_evidence_reference(
                    &mut references,
                    &mut seen,
                    format!("issue {value}"),
                );
            }
        }
    }

    for token in tokens {
        let cleaned = trim_task_contract_reference_token(token);
        if cleaned.contains("://") {
            push_unique_evidence_reference(&mut references, &mut seen, cleaned.to_string());
            continue;
        }
        if let Some(value) = cleaned
            .strip_prefix('#')
            .and_then(normalize_numeric_reference)
        {
            push_unique_evidence_reference(&mut references, &mut seen, format!("#{value}"));
            continue;
        }
        if looks_like_task_contract_reference(cleaned) {
            push_unique_evidence_reference(&mut references, &mut seen, cleaned.to_string());
        }
    }

    references
}

fn normalize_numeric_reference(token: &str) -> Option<String> {
    let cleaned = trim_task_contract_reference_token(token)
        .trim_start_matches('#')
        .trim();
    (!cleaned.is_empty() && cleaned.chars().all(|ch| ch.is_ascii_digit()))
        .then(|| cleaned.to_string())
}

fn push_unique_evidence_reference(
    references: &mut Vec<String>,
    seen: &mut HashSet<String>,
    reference: String,
) {
    let normalized = normalize_contract_label(&reference);
    if normalized.is_empty() || !seen.insert(normalized) {
        return;
    }
    references.push(reference);
}

fn trim_task_contract_reference_token(token: &str) -> &str {
    token
        .trim_matches(|c: char| {
            matches!(
                c,
                '`' | '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | ',' | ';' | ':'
            )
        })
        .trim_end_matches('.')
}

fn looks_like_task_contract_reference(token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    if let Some(number) = token.strip_prefix('#') {
        return !number.is_empty() && number.chars().all(|ch| ch.is_ascii_digit());
    }
    if token.contains('/') {
        return true;
    }
    let Some((stem, extension)) = token.rsplit_once('.') else {
        return false;
    };
    !stem.is_empty()
        && extension.len() >= 2
        && stem.chars().any(|ch| ch.is_ascii_alphanumeric())
        && extension.chars().all(|ch| ch.is_ascii_alphanumeric())
}

fn task_contract_observations(
    executor: &dyn ToolExecutor,
    calls: &[ToolCall],
    results: &[ToolResult],
) -> Vec<String> {
    let mut observations = Vec::new();
    let call_map: HashMap<&str, &ToolCall> =
        calls.iter().map(|call| (call.id.as_str(), call)).collect();

    for result in results.iter().filter(|result| result.success) {
        let Some(call) = call_map.get(result.tool_call_id.as_str()) else {
            continue;
        };
        if executor.classify_call(call) != ToolCallClassification::Observation {
            continue;
        }

        let normalized_observation =
            normalize_contract_label(&format!("{} {}", call.name, call.arguments));
        if !normalized_observation.is_empty() {
            observations.push(normalized_observation);
        }
    }

    observations
}

fn task_contract_matches_observation(input: &InputRequirement, observation: &str) -> bool {
    evidence_target_matches_observation(&input.normalized_description, observation)
}

fn evidence_target_matches_observation(target: &str, observation: &str) -> bool {
    if target.is_empty() || observation.is_empty() {
        return false;
    }
    if observation.contains(target) {
        return true;
    }

    let observation_tokens: HashSet<&str> = observation.split_whitespace().collect();
    target
        .split_whitespace()
        .all(|token| observation_tokens.contains(token))
}

fn extract_progress_evidence_references(text: &str) -> Vec<String> {
    const MAX_PROGRESS_EVIDENCE_REFERENCES: usize = 48;

    // This parser is intentionally simple because the call site scopes it to
    // reference-discovery outputs (`search_text`, grep/rg/find, diffs). Do not
    // run it over arbitrary command output; prose and code can contain path-like
    // strings that should not become follow-up obligations.
    let mut references = Vec::new();
    let mut seen = HashSet::new();
    for line in text.lines() {
        for token in line.split_whitespace() {
            let Some(reference) = sanitize_progress_evidence_reference_token(token) else {
                continue;
            };
            push_unique_evidence_reference(&mut references, &mut seen, reference);
            if references.len() >= MAX_PROGRESS_EVIDENCE_REFERENCES {
                return references;
            }
        }
    }
    references
}

fn sanitize_progress_evidence_reference_token(token: &str) -> Option<String> {
    let mut cleaned = token
        .trim_matches(|c: char| {
            matches!(
                c,
                '`' | '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | ',' | ';' | ':'
            )
        })
        .trim_start_matches('+')
        .trim_start_matches('-')
        .trim();
    cleaned = cleaned
        .strip_prefix("a/")
        .or_else(|| cleaned.strip_prefix("b/"))
        .unwrap_or(cleaned);
    cleaned = cleaned.strip_prefix("./").unwrap_or(cleaned);
    cleaned = strip_progress_reference_line_suffix(cleaned);

    if cleaned.len() < 3 || !looks_like_task_contract_reference(cleaned) {
        return None;
    }
    Some(cleaned.to_string())
}

fn strip_progress_reference_line_suffix(reference: &str) -> &str {
    if reference.contains("://") {
        return reference;
    }

    for (colon_index, _) in reference.match_indices(':') {
        let prefix = &reference[..colon_index];
        let line_segment = reference[colon_index + 1..]
            .split(':')
            .next()
            .unwrap_or_default();

        if !line_segment.is_empty()
            && line_segment.chars().all(|ch| ch.is_ascii_digit())
            && looks_like_task_contract_reference(prefix)
        {
            return prefix;
        }
    }

    reference
}

fn render_root_turn_contract_directive(contract: &RootTurnContract) -> String {
    let mut directive =
        String::from("Do not finish this turn until the root-turn deliverables are satisfied.\n");

    let response_sections = contract
        .deliverables
        .iter()
        .filter_map(|deliverable| match deliverable {
            RootTurnDeliverable::ResponseSection { label, .. } => Some(label.as_str()),
            RootTurnDeliverable::MutationWork { .. } => None,
            RootTurnDeliverable::ArtifactWrite { .. } => None,
            RootTurnDeliverable::ExternalAction { .. } => None,
        })
        .collect::<Vec<_>>();
    if !response_sections.is_empty() {
        directive.push_str(
            "Produce one consolidated final response with these explicit section headings:\n",
        );
        for label in response_sections {
            directive.push_str("- ");
            directive.push_str(label);
            directive.push('\n');
        }
    }

    let pending_mutation_work = contract
        .deliverables
        .iter()
        .filter_map(|deliverable| match deliverable {
            RootTurnDeliverable::MutationWork { label, satisfied } if !satisfied => {
                Some(label.as_str())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    if !pending_mutation_work.is_empty() {
        directive
            .push_str("Do not finish until these local mutation deliverables are completed:\n");
        for label in pending_mutation_work {
            directive.push_str("- ");
            directive.push_str(label);
            directive.push('\n');
        }
    }

    let pending_artifacts = contract
        .deliverables
        .iter()
        .filter_map(|deliverable| match deliverable {
            RootTurnDeliverable::ArtifactWrite { path, satisfied } if !satisfied => {
                Some(path.as_str())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    if !pending_artifacts.is_empty() {
        directive.push_str("Do not finish until these artifacts are written:\n");
        for path in pending_artifacts {
            directive.push_str("- ");
            directive.push_str(path);
            directive.push('\n');
        }
    }

    let pending_external_actions = contract
        .deliverables
        .iter()
        .filter_map(|deliverable| match deliverable {
            RootTurnDeliverable::ExternalAction {
                label, satisfied, ..
            } if !satisfied => Some(label.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    if !pending_external_actions.is_empty() {
        directive.push_str("Do not finish until these external actions are completed:\n");
        for action in pending_external_actions {
            directive.push_str("- ");
            directive.push_str(action);
            directive.push('\n');
        }
    }

    directive.push_str(
        "If a deliverable is still missing, continue the turn instead of stopping at a progress-only update.",
    );
    directive.trim_end().to_string()
}

fn render_task_contract_state_directive(contract: &TaskContract) -> String {
    let mut directive = match contract.phase {
        TaskPhase::Gathering => String::from(
            "You are operating under a declared task-input contract.\nPhase: gathering.\nOnly call tools needed to satisfy the remaining unchecked inputs. Do not broaden the search beyond this list unless a listed input is impossible to obtain.\n",
        ),
        TaskPhase::Synthesizing => String::from(
            "You are operating under a declared task-input contract.\nPhase: synthesizing.\nAll declared inputs are satisfied. Do not call more tools. Answer directly from the gathered evidence.\n",
        ),
        TaskPhase::Complete => String::from(
            "You are operating under a declared task-input contract.\nPhase: complete.\nDo not reopen tool gathering.\n",
        ),
    };

    directive.push_str("Declared inputs:\n");
    for input in &contract.inputs {
        directive.push_str(if input.satisfied { "- [x] " } else { "- [ ] " });
        directive.push_str(&input.description);
        directive.push('\n');
    }

    directive.trim_end().to_string()
}

fn render_turn_progress_ledger_directive(ledger: &TurnProgressLedger) -> String {
    let mut directive = String::from("You are operating under a turn evidence contract.\n");
    let mut explicit_slots = ledger
        .explicit_evidence_slot_ids
        .iter()
        .filter_map(|id| ledger.evidence_slots.get(id))
        .collect::<Vec<_>>();
    explicit_slots.sort_by(|left, right| left.label.cmp(&right.label));
    if !explicit_slots.is_empty() {
        directive.push_str("Required evidence slots:\n");
        for slot in explicit_slots {
            directive.push_str(if slot.status == ProgressSlotStatus::Satisfied {
                "- [x] "
            } else {
                "- [ ] "
            });
            directive.push_str(&slot.label);
            directive.push('\n');
        }
    }

    let open_explicit = ledger.open_explicit_evidence_slots();
    if ledger.has_explicit_evidence_slots() && open_explicit.is_empty() {
        directive.push_str(
            "All required evidence slots are satisfied. Do not broaden into unrelated read-only research; answer directly unless a later successful tool result discovered a concrete follow-up path that must be inspected.\n",
        );
    } else if !open_explicit.is_empty() {
        directive.push_str(
            "Only call read-only tools that can satisfy unchecked required evidence slots or inspect concrete evidence discovered by prior successful tools. Do not answer until unchecked required evidence is satisfied or explicitly impossible.\n",
        );
    } else {
        directive.push_str(
            "Successful search/listing results have discovered concrete follow-up evidence pointers. Treat those pointers as leads, not complete evidence.\n",
        );
    }

    let mut discovered_slots = ledger
        .evidence_slots
        .values()
        .filter(|slot| !slot.explicit && slot.status == ProgressSlotStatus::Open)
        .map(|slot| slot.label.as_str())
        .collect::<Vec<_>>();
    discovered_slots.sort();
    discovered_slots.truncate(12);
    if !discovered_slots.is_empty() {
        directive.push_str("Discovered follow-up evidence:\n");
        for label in discovered_slots {
            directive.push_str("- [ ] ");
            directive.push_str(label);
            directive.push('\n');
        }
        directive.push_str(
            "Inspect the most relevant discovered paths directly with read_file before answering; do not treat file:line search hits as full code evidence.\n",
        );
    }

    directive.trim_end().to_string()
}

fn root_turn_completion_block(
    contract: &RootTurnContract,
    response: &str,
) -> Option<RootTurnCompletionBlock> {
    let missing_response_sections = contract
        .deliverables
        .iter()
        .filter_map(|deliverable| match deliverable {
            RootTurnDeliverable::ResponseSection {
                label,
                normalized_label,
            } if !response_satisfies_required_section(response, normalized_label) => {
                Some(label.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let pending_artifact_paths = contract
        .deliverables
        .iter()
        .filter_map(|deliverable| match deliverable {
            RootTurnDeliverable::ArtifactWrite { path, satisfied } if !satisfied => {
                Some(path.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let pending_external_actions = contract
        .deliverables
        .iter()
        .filter_map(|deliverable| match deliverable {
            RootTurnDeliverable::ExternalAction {
                label, satisfied, ..
            } if !satisfied => Some(label.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let pending_mutation_work = contract
        .deliverables
        .iter()
        .filter_map(|deliverable| match deliverable {
            RootTurnDeliverable::MutationWork { label, satisfied } if !satisfied => {
                Some(label.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    if missing_response_sections.is_empty()
        && pending_mutation_work.is_empty()
        && pending_artifact_paths.is_empty()
        && pending_external_actions.is_empty()
    {
        None
    } else {
        Some(RootTurnCompletionBlock {
            missing_response_sections,
            pending_mutation_work,
            pending_artifact_paths,
            pending_external_actions,
        })
    }
}

fn root_turn_pending_deliverable_summary(block: &RootTurnCompletionBlock) -> String {
    let mut pending = Vec::new();
    pending.extend(
        block
            .missing_response_sections
            .iter()
            .map(|section| format!("response section `{section}`")),
    );
    pending.extend(
        block
            .pending_mutation_work
            .iter()
            .map(|work| format!("local mutation work `{work}`")),
    );
    pending.extend(
        block
            .pending_artifact_paths
            .iter()
            .map(|path| format!("artifact write `{path}`")),
    );
    pending.extend(
        block
            .pending_external_actions
            .iter()
            .map(|action| format!("external action `{action}`")),
    );

    if pending.is_empty() {
        "required deliverable(s)".to_string()
    } else {
        pending.join(", ")
    }
}

// Heading matching is intentionally lenient: exact normalized matches pass, and
// a required label may also be satisfied by a longer heading that starts with
// the same words, such as `Plan for Phase 2` for the deliverable `Plan`.
fn response_satisfies_required_section(response: &str, required_label: &str) -> bool {
    response.lines().any(|line| {
        let normalized = normalize_contract_label(
            line.trim()
                .trim_start_matches('#')
                .trim()
                .trim_matches(|c: char| matches!(c, '*' | '_' | '`'))
                .trim_end_matches(':')
                .trim(),
        );
        normalized == required_label
            || normalized
                .strip_prefix(required_label)
                .is_some_and(|rest| rest.starts_with(' '))
    })
}

fn render_root_turn_retry_directive(
    block: &RootTurnCompletionBlock,
    retries_remaining: u8,
) -> String {
    let mut directive = String::from(
        "The previous response is not terminal yet because required root-turn deliverables are still missing.\n",
    );
    if !block.missing_response_sections.is_empty() {
        directive.push_str("Missing response sections:\n");
        for label in &block.missing_response_sections {
            directive.push_str("- ");
            directive.push_str(label);
            directive.push('\n');
        }
    }
    if !block.pending_mutation_work.is_empty() {
        directive.push_str("Pending local mutation work:\n");
        for label in &block.pending_mutation_work {
            directive.push_str("- ");
            directive.push_str(label);
            directive.push('\n');
        }
    }
    if !block.pending_artifact_paths.is_empty() {
        directive.push_str("Pending artifact writes:\n");
        for path in &block.pending_artifact_paths {
            directive.push_str("- ");
            directive.push_str(path);
            directive.push('\n');
        }
    }
    if !block.pending_external_actions.is_empty() {
        directive.push_str("Pending external actions:\n");
        for action in &block.pending_external_actions {
            directive.push_str("- ");
            directive.push_str(action);
            directive.push('\n');
        }
    }
    directive.push_str(&format!(
        "Continue the same turn and produce one consolidated final response that satisfies all remaining deliverables. Remaining contract retries before the kernel falls back to the current response: {retries_remaining}.",
    ));
    directive.trim_end().to_string()
}

fn looks_like_artifact_path(path: &str) -> bool {
    !path.is_empty()
        && (path.contains('/') || path.starts_with("~/"))
        && path
            .rsplit('/')
            .next()
            .is_some_and(|segment| segment.contains('.'))
}

fn mutation_tool_scope(available_tools: &[ToolDefinition]) -> ContinuationToolScope {
    let mut names = available_tools
        .iter()
        .filter(|tool| {
            matches!(
                tool.name.as_str(),
                "edit_file" | "write_file" | "run_command" | "git_commit" | "git_push"
            )
        })
        .map(|tool| tool.name.clone())
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    if names.is_empty() {
        names.push("run_command".to_string());
    }
    ContinuationToolScope::Only(names)
}

fn mutation_work_completed(calls: &[ToolCall], results: &[ToolResult]) -> bool {
    // Root mutation deliverables are satisfied only by successful local file
    // writes. Broader side effects such as tests, commits, or pushes may be
    // required later, but they should not prove that requested code changes
    // actually happened.
    successful_call_evidence(calls, results)
        .any(|(call, _)| matches!(call.name.as_str(), "edit_file" | "write_file"))
}

fn artifact_write_completed(target: &str, tool_results: &[ToolResult]) -> bool {
    let candidates = artifact_path_candidates(target);
    tool_results.iter().any(|result| {
        result.success
            && result.tool_name == "write_file"
            && candidates
                .iter()
                .any(|candidate| result.output.contains(candidate))
    })
}

fn artifact_path_candidates(target: &str) -> Vec<String> {
    let mut candidates = vec![target.to_string()];
    if let Some(stripped) = target.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            candidates.push(format!("{home}/{stripped}"));
        }
    }
    candidates
}

// Retained for potential use in non-structured-tool contexts (e.g. plain-text LLM fallback).
#[allow(dead_code)]
fn available_tools_instructions(tool_definitions: &[ToolDefinition]) -> String {
    let tools = tool_definitions
        .iter()
        .map(|tool| format!("- {}: {}", tool.name, tool.description))
        .collect::<Vec<_>>()
        .join(
            "
",
        );

    format!(
        "Available tools:
{tools}"
    )
}
/// Extract human-readable text from JSON-shaped model output.
///
/// Safety net for models that return structured JSON instead of plain text
/// when no tool calls are present. Looks for common text-bearing keys;
/// falls back to the raw string when no match is found.
fn extract_readable_text(raw: &str) -> String {
    let trimmed = raw.trim();
    if !trimmed.starts_with('{') {
        return raw.to_string();
    }
    if let Ok(obj) = serde_json::from_str::<serde_json::Value>(trimmed) {
        for key in &["text", "response", "message", "content", "answer"] {
            if let Some(val) = obj.get(key).and_then(|v| v.as_str()) {
                return val.to_string();
            }
        }
    }
    raw.to_string()
}

fn extract_response_text(response: &CompletionResponse) -> String {
    response
        .content
        .iter()
        .filter_map(|block| match block {
            fx_llm::ContentBlock::Text { text } => Some(text.as_str()),
            ContentBlock::Image { .. } => None,
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn normalize_response_text(text: &str) -> String {
    text.trim().to_string()
}

fn final_response_violation_label(violation: FinalResponseViolation) -> &'static str {
    match violation {
        FinalResponseViolation::ToolActivityAttempted => "tool activity attempted",
        FinalResponseViolation::TruncatedResponse => "truncated response",
        FinalResponseViolation::EmptyResponse => "empty response",
        FinalResponseViolation::ProgressOnlyResponse => "progress-only response",
    }
}

fn final_response_retry_cap_partial_response(
    prior_response: Option<&str>,
    violation: FinalResponseViolation,
) -> String {
    if violation != FinalResponseViolation::TruncatedResponse {
        if let Some(response) = prior_response
            .map(normalize_response_text)
            .filter(|text| !text.trim().is_empty())
            .filter(|text| {
                matches!(
                    TurnControlPlane::validate_final_response(FinalResponseValidationFacts {
                        attempted_tool_activity: false,
                        response_truncated: false,
                        response_text: Some(text),
                    }),
                    FinalResponseValidationOutcome::Accept
                )
            })
            .filter(|text| !looks_like_tool_request_text(text))
        {
            return response;
        }
    }

    let violation_label = final_response_violation_label(violation);
    format!(
        "I could not produce a clean final answer because the final-response phase ended with {violation_label}. The turn is stopped here to avoid another tool loop. Please retry from this thread; the tool output gathered so far is preserved."
    )
}

fn looks_like_tool_request_text(text: &str) -> bool {
    let normalized = text.trim().to_ascii_lowercase();
    normalized.starts_with("tool request:")
        || normalized.starts_with("tool call:")
        || normalized.starts_with("function call:")
}

fn render_finalize_response_retry_directive(
    violation: FinalResponseViolation,
    retries_remaining: u8,
    success_target: &str,
) -> String {
    let violation_guidance = match violation {
        FinalResponseViolation::ToolActivityAttempted => {
            "The previous final-response attempt tried to use tools. Tools are not available in this phase."
        }
        FinalResponseViolation::TruncatedResponse => {
            "The previous final-response attempt was cut off before it reached a terminal answer."
        }
        FinalResponseViolation::EmptyResponse => {
            "The previous final-response attempt was empty."
        }
        FinalResponseViolation::ProgressOnlyResponse => {
            "The previous final-response attempt described future work instead of answering."
        }
    };

    format!(
        "{violation_guidance}\n\
         Final-response phase is terminal and final-only: do not call tools, do not plan more inspection, and do not say you need to read or check more before answering.\n\
         Success target: {success_target}\n\
         Retries remaining before the kernel ends this turn incomplete: {retries_remaining}."
    )
}

fn final_response_stop_reason_is_nonterminal(stop_reason: Option<&str>) -> bool {
    stop_reason.is_some_and(|reason| {
        let normalized = reason.trim().to_ascii_lowercase();
        normalized == "continuation_budget_exhausted" || is_truncated(Some(normalized.as_str()))
    })
}

fn meaningful_response_text(text: &str) -> Option<String> {
    let normalized = normalize_response_text(text);
    (!normalized.is_empty()).then_some(normalized)
}

fn response_text_segment(response: &CompletionResponse) -> Option<String> {
    let raw = extract_response_text(response);
    let readable = extract_readable_text(&raw);
    meaningful_response_text(&readable)
}

fn push_response_segment(segments: &mut Vec<String>, segment: Option<String>) {
    if let Some(segment) = segment {
        segments.push(segment);
    }
}

fn stitch_response_segments(segments: &[String], tail: Option<String>) -> String {
    segments
        .iter()
        .cloned()
        .chain(tail)
        .filter(|segment| !segment.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn stitched_response_text(segments: &[String], tail: Option<String>) -> Option<String> {
    meaningful_response_text(&stitch_response_segments(segments, tail))
}

fn prepend_accumulated_text_to_action(
    mut action: ActionResult,
    accumulated_text: &[String],
) -> ActionResult {
    if accumulated_text.is_empty() {
        return action;
    }
    if let Some(response_text) = meaningful_response_text(&action.response_text) {
        action.response_text = stitch_response_segments(accumulated_text, Some(response_text));
    }
    match &mut action.next_step {
        ActionNextStep::Continue(continuation) => {
            continuation.partial_response = continuation.partial_response.take().and_then(|text| {
                meaningful_response_text(&text)
                    .and_then(|text| stitched_response_text(accumulated_text, Some(text)))
            });
            continuation.context_message = continuation.context_message.take().and_then(|text| {
                meaningful_response_text(&text)
                    .and_then(|text| stitched_response_text(accumulated_text, Some(text)))
            });
        }
        ActionNextStep::Finish(ActionTerminal::Complete { response }) => {
            *response = stitch_response_segments(accumulated_text, Some(response.clone()));
        }
        ActionNextStep::Finish(ActionTerminal::Incomplete {
            partial_response, ..
        }) => {
            *partial_response = partial_response.take().and_then(|text| {
                meaningful_response_text(&text)
                    .and_then(|text| stitched_response_text(accumulated_text, Some(text)))
            });
        }
    }
    action
}

fn repeated_tool_failure_from_results(
    results: &[ToolResult],
) -> Option<(RepeatedToolFailure, u16)> {
    if results.is_empty() || results.iter().any(|result| result.success) {
        return None;
    }

    let mut failed = results.iter().filter(|result| !result.success);
    let mut failure = classify_repeated_tool_failure(failed.next()?);
    let mut count = 1u16;

    for result in failed {
        let next = classify_repeated_tool_failure(result);
        if next.key != failure.key {
            return None;
        }
        failure.summary = next.summary;
        failure.last_output = next.last_output;
        count = count.saturating_add(1);
    }

    Some((failure, count))
}

fn classify_repeated_tool_failure(result: &ToolResult) -> RepeatedToolFailure {
    let key = RepeatedToolFailureKey {
        tool_name: result.tool_name.clone(),
        failure_class: result
            .failure_classification()
            .unwrap_or(FailureClass::Unknown),
        kind: repeated_tool_failure_kind(&result.output),
    };
    let summary = repeated_tool_failure_summary(&key.kind, &result.output);
    RepeatedToolFailure {
        key,
        summary,
        last_output: truncate_prompt_text(result.output.trim(), 240),
    }
}

fn repeated_tool_failure_kind(output: &str) -> RepeatedToolFailureKind {
    if output.contains(MALFORMED_TOOL_ARGUMENTS_ERROR_MARKER) {
        RepeatedToolFailureKind::MalformedArgumentsJson
    } else {
        RepeatedToolFailureKind::OutputHash(hash_tool_failure_output(output))
    }
}

fn repeated_tool_failure_summary(kind: &RepeatedToolFailureKind, output: &str) -> String {
    match kind {
        RepeatedToolFailureKind::MalformedArgumentsJson => {
            MALFORMED_TOOL_ARGUMENTS_ERROR_MARKER.to_string()
        }
        RepeatedToolFailureKind::OutputHash(_) => truncate_prompt_text(output.trim(), 160),
    }
}

fn hash_tool_failure_output(text: &str) -> u64 {
    let normalized = normalize_tool_failure_output(text);
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    normalized.hash(&mut hasher);
    hasher.finish()
}

fn normalize_tool_failure_output(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut previous_was_space = false;
    let mut previous_was_digit = false;

    for ch in text.trim().chars() {
        if ch.is_whitespace() {
            if !previous_was_space && !normalized.is_empty() {
                normalized.push(' ');
            }
            previous_was_space = true;
            previous_was_digit = false;
            continue;
        }

        previous_was_space = false;
        if ch.is_ascii_digit() {
            if !previous_was_digit {
                normalized.push('0');
            }
            previous_was_digit = true;
            continue;
        }

        previous_was_digit = false;
        normalized.push(ch);
    }

    normalized
}

fn repeated_tool_failure_guidance(state: &RepeatedToolFailureState) -> String {
    match (
        state.failure.key.tool_name.as_str(),
        &state.failure.key.kind,
    ) {
        ("write_file", RepeatedToolFailureKind::MalformedArgumentsJson) => "Use an alternative approach: write smaller chunks, simplify the content so the arguments stay valid JSON, or switch to `run_command` with a heredoc if that is safer.".to_string(),
        (_, RepeatedToolFailureKind::MalformedArgumentsJson) => "Retry only if you can produce valid JSON arguments. Otherwise use a different tool shape or answer with the blocker instead of repeating the malformed call.".to_string(),
        ("run_command", _) => "Stop repeating the same command. Change the command shape, inspect the repo/files directly, or explain the blocker to the user.".to_string(),
        _ => "Do not repeat the same failing call. Try a different tool or narrower approach, or answer with what is blocked.".to_string(),
    }
}

fn render_repeated_tool_failure_directive(state: &RepeatedToolFailureState) -> String {
    format!(
        "Repeated tool failure circuit breaker: `{tool}` has failed {count} consecutive times with the same failure: {summary}. {guidance} If the same failure happens again, stop using tools and answer the user with what you attempted and why it failed.",
        tool = state.failure.key.tool_name,
        count = state.consecutive_failures,
        summary = state.failure.summary,
        guidance = repeated_tool_failure_guidance(state),
    )
}

fn repeated_tool_failure_terminal_reason(state: &RepeatedToolFailureState) -> String {
    format!(
        "repeated tool failure circuit breaker tripped after {count} consecutive `{tool}` failures ({summary})",
        count = state.consecutive_failures,
        tool = state.failure.key.tool_name,
        summary = state.failure.summary,
    )
}

fn repeated_tool_failure_partial_response(
    action_partial: Option<&str>,
    state: &RepeatedToolFailureState,
) -> Option<String> {
    let note = format!(
        "I stopped early because `{tool}` failed {count} consecutive times with the same error: {summary}. Last error: {last_error}",
        tool = state.failure.key.tool_name,
        count = state.consecutive_failures,
        summary = state.failure.summary,
        last_error = state.failure.last_output,
    );

    let mut segments = Vec::new();
    push_response_segment(
        &mut segments,
        action_partial.and_then(meaningful_response_text),
    );
    stitched_response_text(&segments, Some(note))
}

fn append_continuation_system_message(continuation: &mut ActionContinuation, message: String) {
    if continuation.context_messages.is_empty() {
        if let Some(context_message) = continuation.context_message.take() {
            continuation
                .context_messages
                .push(Message::assistant(context_message));
        }
    }
    continuation.context_messages.push(Message::system(message));
}

fn append_continuation_context(
    context_window: &mut Vec<Message>,
    continuation: &ActionContinuation,
) {
    if !continuation.context_messages.is_empty() {
        context_window.extend(continuation.context_messages.clone());
        return;
    }

    if let Some(context_message) = continuation.context_message.as_ref() {
        context_window.push(Message::assistant(context_message.clone()));
    }
}

fn continuation_commits_final_response(continuation: &ActionContinuation) -> bool {
    matches!(
        continuation.turn_commitment.as_ref(),
        Some(TurnCommitment::FinalizeResponse(_))
    )
}

fn action_commits_final_response(action: &ActionResult) -> bool {
    matches!(
        &action.next_step,
        ActionNextStep::Continue(continuation) if continuation_commits_final_response(continuation)
    )
}

fn action_is_terminal(action: &ActionResult) -> bool {
    matches!(action.next_step, ActionNextStep::Finish(_))
}

fn action_partial_response(action: &ActionResult) -> Option<String> {
    match &action.next_step {
        ActionNextStep::Finish(ActionTerminal::Complete { response }) => {
            meaningful_response_text(&action.response_text)
                .or_else(|| meaningful_response_text(response))
        }
        ActionNextStep::Finish(ActionTerminal::Incomplete {
            partial_response, ..
        }) => meaningful_response_text(&action.response_text).or_else(|| {
            partial_response
                .as_deref()
                .and_then(meaningful_response_text)
        }),
        ActionNextStep::Continue(continuation) => continuation
            .partial_response
            .as_deref()
            .and_then(meaningful_response_text),
    }
}

fn summarize_tool_progress(results: &[ToolResult]) -> Option<String> {
    // User-visible partial responses should explain blockers only. Successful
    // tool work belongs to typed activity events, not fallback assistant prose.
    let failures: Vec<_> = results
        .iter()
        .filter(|result| !result.success && !is_policy_deferred_tool_result(result))
        .collect();

    if failures.is_empty() {
        return None;
    }

    let mut parts = Vec::new();
    let latest = failures.last().expect("failures is non-empty");
    parts.push(format!(
        "latest blocker: {}",
        truncate_prompt_text(&latest.output, 160)
    ));

    Some(parts.join(". "))
}

fn completed_work_summary_from_progress_entries(entries: &[ToolProgressEntry]) -> Option<String> {
    if entries.is_empty() {
        return None;
    }

    let mut by_kind = BTreeMap::<(&'static str, &'static str), usize>::new();
    let mut failed = 0usize;
    let mut duplicate = 0usize;
    for entry in entries {
        *by_kind
            .entry(completed_work_tool_kind_labels(&entry.tool_name))
            .or_default() += 1;
        match entry.outcome {
            ToolProgressOutcome::RetryableFailure => failed = failed.saturating_add(1),
            ToolProgressOutcome::Duplicate => duplicate = duplicate.saturating_add(1),
            ToolProgressOutcome::Advanced => {}
        }
    }

    let mut pieces = by_kind
        .into_iter()
        .map(|((singular, plural), count)| {
            let label = if count == 1 { singular } else { plural };
            format!("{count} {label}")
        })
        .collect::<Vec<_>>();
    if failed > 0 {
        pieces.push(format!("{failed} failed"));
    }
    if duplicate > 0 {
        pieces.push(format!("{duplicate} repeated"));
    }
    Some(format!("Worked this turn: {}.", pieces.join(", ")))
}

fn completed_work_tool_kind_labels(name: &str) -> (&'static str, &'static str) {
    let normalized = name.to_ascii_lowercase();
    if normalized.contains("command") || normalized.contains("shell") {
        ("command", "commands")
    } else if normalized.contains("search") || normalized == "rg" || normalized == "grep" {
        ("search", "searches")
    } else if normalized.contains("edit")
        || normalized.contains("write")
        || normalized.contains("patch")
    {
        ("edit", "edits")
    } else if normalized.contains("read") || normalized.contains("file") || normalized == "ls" {
        ("file read", "file reads")
    } else {
        ("tool", "tools")
    }
}

fn is_policy_deferred_tool_result(result: &ToolResult) -> bool {
    !result.success && matches!(result.failure_class, Some(FailureClass::PolicyDeferred))
}

pub(super) fn loop_error(stage: &str, reason: &str, recoverable: bool) -> LoopError {
    LoopError {
        stage: stage.to_string(),
        reason: reason.to_string(),
        recoverable,
    }
}

fn loop_step_for_error_stage(stage: &str) -> LoopStep {
    match stage {
        "perceive" => LoopStep::Perceive,
        "reason" => LoopStep::Reason,
        "decide" => LoopStep::Decide,
        "act" => LoopStep::Act,
        "synthesize" => LoopStep::Synthesize,
        _ => LoopStep::Synthesize,
    }
}

fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
