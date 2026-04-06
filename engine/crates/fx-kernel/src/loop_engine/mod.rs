//! Agentic loop orchestrator.

use crate::act::{
    ActionContinuation, ActionNextStep, ActionResult, ActionTerminal, ContinuationToolScope,
    FailureClass, TokenUsage, ToolCacheability, ToolCallClassification, ToolExecutionDiagnostics,
    ToolExecutor, ToolResult, TurnCommitment,
};
use crate::budget::{
    estimate_complexity, ActionCost, BudgetRemaining, BudgetState, BudgetTracker, TerminationConfig,
};
#[cfg(test)]
use crate::budget::{AllocationPlan, BudgetConfig};
use crate::cancellation::CancellationToken;
use crate::channels::ChannelRegistry;
use crate::context_manager::ContextCompactor;

#[cfg(test)]
use crate::conversation_compactor::debug_assert_tool_pair_integrity;
use crate::conversation_compactor::{
    estimate_text_tokens, CompactionConfig, CompactionMemoryFlush, ConversationBudget,
};
use crate::decide::Decision;
use crate::input::{LoopCommand, LoopInputChannel};

use crate::perceive::{ProcessedPerception, TrimmingPolicy};
use crate::signals::{LoopStep, Signal, SignalCollector, SignalKind};
use crate::streaming::{ErrorCategory, Phase, StreamCallback, StreamEvent};
use crate::types::{
    Goal, IdentityContext, LoopError, PerceptionSnapshot, ReasoningContext, WorkingMemoryEntry,
};

use async_trait::async_trait;
#[cfg(test)]
use futures_util::StreamExt;
use fx_core::message::{InternalMessage, ProgressKind, StreamPhase};
use fx_core::runtime_info::RuntimeInfo;
#[cfg(test)]
use fx_core::types::{InputSource, ScreenState, UserInput};
use fx_decompose::{AggregationStrategy, ComplexityHint, DecompositionPlan, SubGoal};
#[cfg(test)]
use fx_decompose::{SubGoalOutcome, SubGoalResult};
use fx_llm::{
    emit_default_stream_response, CompletionRequest, CompletionResponse, ContentBlock, Message,
    MessageRole, ProviderError, StreamCallback as ProviderStreamCallback, StreamChunk, ToolCall,
    ToolDefinition, ToolUseDelta, Usage,
};
use fx_session::SessionMemory;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
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
mod progress;
mod request;
mod retry;
mod streaming;
mod tool_execution;

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
    decomposition_results_all_skipped, estimate_plan_cost, is_decomposition_results_message,
    parse_decomposition_plan,
};
#[cfg(test)]
use self::request::{build_continuation_request, ContinuationRequestParams};
use self::request::{
    build_forced_synthesis_request, build_reasoning_messages, build_reasoning_request,
    build_truncation_continuation_request, completion_request_to_prompt,
    ForcedSynthesisRequestParams, ReasoningRequestParams, RequestBuildContext, SkillPromptSummary,
    ToolRequestConfig, TruncationContinuationRequestParams,
};
#[cfg(test)]
use self::request::{
    build_reasoning_system_prompt, build_reasoning_system_prompt_with_notify_guidance,
    build_tool_continuation_system_prompt,
    build_tool_continuation_system_prompt_with_notify_guidance, decompose_tool_definition,
    reasoning_user_prompt, tool_definitions_with_decompose,
};
#[cfg(test)]
use self::retry::same_call_failure_reason;
use self::retry::RetryTracker;
use self::streaming::{StreamingRequestContext, TextStreamVisibility};
use self::tool_execution::extract_tool_use_provider_ids;
#[cfg(test)]
use self::tool_execution::ToolRoundOutcome;

#[cfg(test)]
use self::tool_execution::{
    append_tool_round_messages, blocked_tool_message, build_tool_result_message,
    build_tool_use_assistant_message, evict_oldest_results, tool_synthesis_prompt,
    truncate_tool_results, TOOL_SYNTHESIS_MAX_OUTPUT_TOKENS,
};

#[cfg(test)]
use self::streaming::{
    finalize_stream_tool_calls, stream_tool_call_from_state, StreamResponseState,
    StreamToolCallState,
};

#[cfg(test)]
use crate::act::ProceedUnderConstraints;
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
    commitment_tool_scope, render_turn_commitment_directive,
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

    fn tool_call_start(self, call: &ToolCall) {
        self.emit(StreamEvent::ToolCallStart {
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

    fn tool_result(self, result: &ToolResult) {
        self.emit(StreamEvent::ToolResult {
            id: result.tool_call_id.clone(),
            tool_name: result.tool_name.clone(),
            output: result.output.clone(),
            is_error: !result.success,
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
            } => Some(
                partial_response
                    .clone()
                    .filter(|text| !text.trim().is_empty())
                    .unwrap_or_else(|| INCOMPLETE_FALLBACK_RESPONSE.to_string()),
            ),
            Self::UserStopped {
                partial_response, ..
            } => Some(
                partial_response
                    .clone()
                    .unwrap_or_else(|| "user stopped".to_string()),
            ),
            Self::Error { .. } => None,
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
    ArtifactWrite {
        path: String,
        satisfied: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RootTurnCompletionBlock {
    missing_response_sections: Vec<String>,
    pending_artifact_paths: Vec<String>,
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
    memory_context: Option<String>,
    session_memory: Arc<Mutex<SessionMemory>>,
    scratchpad_context: Option<String>,
    signals: SignalCollector,
    cancel_token: Option<CancellationToken>,
    input_channel: Option<LoopInputChannel>,
    user_stop_requested: bool,
    pending_steer: Option<String>,
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
    /// Latest reasoning input messages for graceful budget-exhausted synthesis.
    /// Stored on `LoopEngine` because `perceive()` only has `&mut self`.
    last_reasoning_messages: Vec<Message>,
    /// Tool retry tracker for the current cycle.
    tool_retry_tracker: RetryTracker,
    /// Circuit breaker for repeated tool failures across outer-loop iterations.
    repeated_tool_failure_tracker: RepeatedToolFailureTracker,
    /// Whether a successful `notify` tool call occurred during the current cycle.
    notify_called_this_cycle: bool,
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
    /// Mixed text emitted alongside tool calls before tool execution begins.
    pending_tool_response_text: Option<String>,
    /// Optional scoped tool surface for the next root reasoning pass.
    pending_tool_scope: Option<ContinuationToolScope>,
    /// Optional typed turn commitment for the next root reasoning pass.
    pending_turn_commitment: Option<TurnCommitment>,
    /// Typed completion contract for the active root turn, when the prompt declares one.
    root_turn_contract: Option<RootTurnContract>,
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
            .field("session_memory", &"SessionMemory")
            .field("scratchpad_context", &self.scratchpad_context)
            .field("compaction_config", &self.compaction_config)
            .field("budget_low_signaled", &self.budget_low_signaled)
            .field("consecutive_tool_turns", &self.consecutive_tool_turns)
            .field("observation_round_tracker", &self.observation_round_tracker)
            .field("tool_retry_tracker", &self.tool_retry_tracker)
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
            .field("root_turn_contract", &self.root_turn_contract)
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
    compaction_config: Option<CompactionConfig>,
    compaction_llm: Option<Arc<dyn LlmProvider>>,
    memory_flush: Option<Arc<dyn CompactionMemoryFlush>>,
    event_bus: Option<fx_core::EventBus>,
    cancel_token: Option<CancellationToken>,
    input_channel: Option<LoopInputChannel>,
    memory_context: Option<String>,
    session_memory: Option<Arc<Mutex<SessionMemory>>>,
    scratchpad_context: Option<String>,
    iteration_counter: Option<Arc<AtomicU32>>,
    scratchpad_provider: Option<Arc<dyn ScratchpadProvider>>,
    error_callback: Option<StreamCallback>,
    thinking_config: Option<fx_llm::ThinkingConfig>,
    runtime_info: Option<Arc<RwLock<RuntimeInfo>>>,
    runtime_skill_prompt_revision: Option<Arc<AtomicU64>>,
    decompose_enabled: Option<bool>,
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
            .field("scratchpad_context", &self.scratchpad_context)
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
            memory_context: self.memory_context,
            session_memory,
            scratchpad_context: self.scratchpad_context,
            signals: SignalCollector::default(),
            cancel_token: self.cancel_token,
            input_channel: self.input_channel,
            user_stop_requested: false,
            pending_steer: None,
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
            last_reasoning_messages: Vec::new(),
            tool_retry_tracker: RetryTracker::default(),
            repeated_tool_failure_tracker: RepeatedToolFailureTracker::default(),
            notify_called_this_cycle: false,
            notify_tool_guidance_enabled: false,
            iteration_counter: self.iteration_counter,
            scratchpad_provider: self.scratchpad_provider,
            tool_call_provider_ids: HashMap::new(),
            pending_tool_result_diagnostics: HashMap::new(),
            pending_tool_response_text: None,
            pending_tool_scope: None,
            pending_turn_commitment: None,
            root_turn_contract: None,
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
    repetitive_rounds: u16,
    seen_observation_fingerprints: HashSet<String>,
}

// Keep the cycle-scoped fingerprint set bounded. Once full, new fingerprints
// degrade to "seen" so extreme fan-out does not retain unbounded state.
const MAX_OBSERVATION_FINGERPRINTS_PER_CYCLE: usize = 256;

impl ObservationRoundTracker {
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
    current_calls: Vec<ToolCall>,
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
            current_calls: calls.to_vec(),
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
            current_calls: Vec::new(),
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

#[derive(Debug, Deserialize)]
struct DecomposeToolArguments {
    sub_goals: Vec<DecomposeSubGoalArguments>,
    #[serde(default)]
    strategy: Option<AggregationStrategy>,
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
After using tools, respond with the answer. Never narrate what tools you used, \
describe the process, or comment on tool output metadata. \
Never narrate your process, hedge with qualifiers, or reference tool mechanics. \
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

const TOOL_CONTINUATION_DIRECTIVE: &str = "\n\nYou are continuing after one or more tool calls. \
Treat successful tool results as the primary evidence for your next response. \
If the existing tool results already answer the user's request, answer immediately instead of calling more tools. \
Only call another tool when the current results are missing critical information, are contradictory, or the user explicitly asked you to refresh/re-check something. \
Never repeat an identical successful tool call in the same cycle. Reuse the result you already have and answer from it.";

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
const BUDGET_EXHAUSTED_SYNTHESIS_DIRECTIVE: &str = "\n\nYour tool budget is exhausted. Provide a final response summarizing what you've found and accomplished.";
const BUDGET_EXHAUSTED_FALLBACK_RESPONSE: &str = "I reached my iteration limit.";
const INCOMPLETE_FALLBACK_RESPONSE: &str = "I couldn't complete that run.";
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
        let skill_prompt_summaries = self.runtime_skill_prompt_summaries();
        RequestBuildContext::new(
            self.memory_context.as_deref(),
            self.scratchpad_context.as_deref(),
            self.thinking_config.clone(),
            self.notify_tool_guidance_enabled,
        )
        .with_skill_prompt_summaries(skill_prompt_summaries)
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
    ) {
        self.signals
            .emit_signal(step, kind, message, metadata, current_time_ms());
    }

    fn finalize_result(&mut self, result: LoopResult) -> LoopResult {
        self.emit_cache_stats_signal();
        let signals = self.signals.drain_all();
        attach_signals(result, signals)
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
        engine
            .run_cycle_streaming_inner(perception, llm, stream_callback.as_ref())
            .await
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

        stream.phase(Phase::Perceive);
        let mut processed = self.perceive(&perception).await?;
        let reason_cost = self.estimate_reasoning_cost(&processed);
        if let Some(result) = self.budget_terminal(reason_cost, None) {
            return Ok(self.finish_streaming_result(result, stream));
        }

        stream.phase(Phase::Reason);
        let response = self.reason(&processed, llm, stream).await?;
        self.record_reasoning_cost(reason_cost, &mut state);

        let mut decision = self.decide(&response).await?;
        if let Some(result) = self.budget_terminal(self.estimate_action_cost(&decision), None) {
            return Ok(self.finish_streaming_result(result, stream));
        }

        loop {
            stream.phase(Phase::Act);
            let mut action = self
                .act(&decision, llm, &processed.context_window, stream)
                .await?;

            let action_partial = action_partial_response(&action);

            if let Some(result) =
                self.apply_repeated_tool_failure_policy(&mut action, action_partial.as_deref())
            {
                return Ok(self.finish_streaming_result(result, stream));
            }

            state.tokens.accumulate(action.tokens_used);
            self.update_tool_turns(&action);

            if let Some(result) = self.check_cancellation(action_partial.clone()) {
                return Ok(self.finish_streaming_result(result, stream));
            }

            self.emit_action_observations(&action);

            let recorded_action_cost = self.recorded_action_cost(&action);
            if let Some(result) = self.budget_terminal(
                recorded_action_cost.unwrap_or_default(),
                action_partial.clone(),
            ) {
                return Ok(self.finish_budget_exhausted(result, llm, stream).await);
            }
            if let Some(action_cost) = recorded_action_cost {
                self.budget.record(&action_cost);
            }

            self.record_root_turn_contract_progress(&action.tool_results);

            let continuation = match action.next_step.clone() {
                ActionNextStep::Finish(terminal) => {
                    let terminal = self.apply_decomposition_terminal_fallback(
                        terminal,
                        processed.context_window.last(),
                    );
                    match self.guard_root_turn_terminal_completion(terminal) {
                        ActionNextStep::Finish(terminal) => {
                            return Ok(self.finish_streaming_result(
                                self.loop_result_from_action_terminal(terminal, state.tokens),
                                stream,
                            ));
                        }
                        ActionNextStep::Continue(continuation) => continuation,
                    }
                }
                ActionNextStep::Continue(continuation) => continuation,
            };

            if continuation
                .context_message
                .as_deref()
                .is_some_and(decomposition_results_all_skipped)
            {
                return Ok(self.finish_streaming_result(
                    LoopResult::Complete {
                        response: continuation
                            .context_message
                            .expect("checked decomposition context message"),
                        iterations: self.iteration_count,
                        tokens_used: state.tokens,
                        signals: Vec::new(),
                    },
                    stream,
                ));
            }

            self.apply_pending_turn_commitment(&continuation, &action.tool_results);

            // Tools were used. Check max before incrementing so the
            // reported iteration count is accurate (not inflated by 1).
            if self.iteration_count >= self.max_iterations {
                // Safety cap reached while the action still required follow-up.
                // Treat this as an incomplete terminal state rather than
                // inferring completion from any partial text.
                let result = LoopResult::Incomplete {
                    partial_response: action_partial.clone(),
                    reason: "iteration limit reached before a usable final response was produced"
                        .to_string(),
                    iterations: self.iteration_count,
                    signals: Vec::new(),
                };
                return Ok(self.finish_streaming_result(result, stream));
            }
            self.iteration_count += 1;

            self.refresh_iteration_state();

            // Append a summary of what happened to the context window so
            // the next reason() call sees the model's tool results. Without
            // this the model would be re-prompted with stale context.
            // NOTE: each continuation iteration adds one assistant message.
            // Bounded by max_iterations (default 10), so growth is small.
            //
            // We build a compact assistant message with the synthesis text
            // (which already summarizes tool outputs) rather than replaying
            // every tool call/result message, because act_with_tools may
            // have run multiple inner rounds with different call IDs that
            // don't map 1:1 to the original Decision::UseTools calls.
            append_continuation_context(&mut processed.context_window, &continuation);

            let reason_cost = self.estimate_reasoning_cost(&processed);
            if let Some(result) = self.budget_terminal(reason_cost, action_partial.clone()) {
                return Ok(self.finish_budget_exhausted(result, llm, stream).await);
            }

            // No re-perceive needed; context_window was updated in-place above.
            stream.phase(Phase::Reason);
            let response = self.reason(&processed, llm, stream).await?;
            self.record_reasoning_cost(reason_cost, &mut state);

            decision = self.decide(&response).await?;
            if let Some(result) = self.budget_terminal(self.estimate_action_cost(&decision), None) {
                return Ok(self.finish_streaming_result(result, stream));
            }

            // Loop back to act with new decision
        }
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
                    self.forced_synthesis_turn(llm, &reasoning_messages).await
                } else {
                    None
                };
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

    fn finish_streaming_result(
        &mut self,
        result: LoopResult,
        stream: CycleStream<'_>,
    ) -> LoopResult {
        self.maybe_emit_completion_notification(&result, stream);
        stream.done_result(&result);
        self.finalize_result(result)
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

    /// Drain the input channel and return the highest-priority flow command.
    ///
    /// Priority ordering: `Abort` > `Stop` > `Wait/Resume` > `StatusQuery` > `Steer`.
    /// `StatusQuery` publishes an internal status message and does not alter loop flow.
    /// `Steer` stores the latest steer text for the next perceive step.
    fn check_user_input(&mut self) -> Option<LoopCommand> {
        let channel = self.input_channel.as_mut()?;
        let mut highest: Option<LoopCommand> = None;
        let mut status_requested = false;
        let mut latest_steer: Option<String> = None;

        while let Some(cmd) = channel.try_recv() {
            match cmd {
                LoopCommand::Steer(text) => latest_steer = Some(text),
                LoopCommand::StatusQuery => status_requested = true,
                flow_cmd => highest = Some(prioritize_flow_command(highest, flow_cmd)),
            }
        }

        if let Some(steer) = latest_steer {
            self.pending_steer = Some(steer);
        }
        if status_requested {
            self.publish_system_status();
        }

        highest
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
        self.pending_steer = None;
        self.budget_low_signaled = false;
        self.consecutive_tool_turns = 0;
        self.observation_round_tracker = ObservationRoundTracker::default();
        self.last_reasoning_messages.clear();
        self.tool_retry_tracker.clear();
        self.repeated_tool_failure_tracker.clear();
        self.notify_called_this_cycle = false;
        self.notify_tool_guidance_enabled = false;
        self.tool_call_provider_ids.clear();
        self.pending_tool_result_diagnostics.clear();
        self.pending_tool_response_text = None;
        self.pending_tool_scope = None;
        self.pending_turn_commitment = None;
        self.root_turn_contract = None;
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
        if matches!(
            action.next_step,
            ActionNextStep::Finish(ActionTerminal::Complete { .. })
        ) {
            self.repeated_tool_failure_tracker.clear();
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

    fn record_root_turn_contract_progress(&mut self, tool_results: &[ToolResult]) {
        let Some(contract) = self.root_turn_contract.as_mut() else {
            return;
        };

        let mut satisfied_paths = Vec::new();
        for deliverable in &mut contract.deliverables {
            let RootTurnDeliverable::ArtifactWrite { path, satisfied } = deliverable else {
                continue;
            };
            if *satisfied || !artifact_write_completed(path, tool_results) {
                continue;
            }
            *satisfied = true;
            satisfied_paths.push(path.clone());
        }

        if !satisfied_paths.is_empty() {
            self.emit_signal(
                LoopStep::Act,
                SignalKind::Success,
                "root turn artifact deliverable satisfied",
                serde_json::json!({ "paths": satisfied_paths }),
            );
        }
    }

    fn guard_root_turn_terminal_completion(&mut self, terminal: ActionTerminal) -> ActionNextStep {
        let ActionTerminal::Complete { response } = terminal else {
            return ActionNextStep::Finish(terminal);
        };
        let retry_limit = self.root_turn_completion_retry_limit();

        let Some(contract) = self.root_turn_contract.as_mut() else {
            return ActionNextStep::Finish(ActionTerminal::Complete { response });
        };

        let Some(block) = root_turn_completion_block(contract, &response) else {
            return ActionNextStep::Finish(ActionTerminal::Complete { response });
        };

        let allow_incomplete_terminal = contract.blocked_terminal_attempts >= retry_limit;
        let blocked_attempts = if allow_incomplete_terminal {
            contract.blocked_terminal_attempts
        } else {
            contract.blocked_terminal_attempts =
                contract.blocked_terminal_attempts.saturating_add(1);
            contract.blocked_terminal_attempts
        };
        let retries_remaining = retry_limit.saturating_sub(blocked_attempts);

        if allow_incomplete_terminal {
            self.emit_signal(
                LoopStep::Act,
                SignalKind::Friction,
                "root turn completion retry cap reached; allowing incomplete terminal response",
                serde_json::json!({
                    "blocked_attempts": blocked_attempts,
                    "retry_limit": retry_limit,
                    "missing_response_sections": &block.missing_response_sections,
                    "pending_artifact_paths": &block.pending_artifact_paths,
                }),
            );
            return ActionNextStep::Finish(ActionTerminal::Complete { response });
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
                "pending_artifact_paths": &block.pending_artifact_paths,
            }),
        );

        let mut context_messages = vec![Message::assistant(response.clone())];
        context_messages.push(Message::system(render_root_turn_retry_directive(
            &block,
            retries_remaining,
        )));
        ActionNextStep::Continue(
            ActionContinuation::new(Some(response), None).with_context_messages(context_messages),
        )
    }

    fn current_reasoning_tool_definitions(&self, should_strip_tools: bool) -> Vec<ToolDefinition> {
        let base = if should_strip_tools {
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
        self.apply_pending_artifact_gate(phased)
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

    fn pending_artifact_write_directive(&self) -> Option<String> {
        self.pending_artifact_write_target.as_ref().map(|path| {
            format!(
                "Immediate next action: write the requested artifact to {path} using write_file. Do not do more observation, search, or shell inspection before attempting this write unless the write itself is blocked."
            )
        })
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
        &self,
        llm: &dyn LlmProvider,
        messages: &[Message],
    ) -> Option<String> {
        if !self.budget.config().termination.synthesize_on_exhaustion {
            tracing::debug!("skipping forced synthesis: synthesize_on_exhaustion disabled");
            return None;
        }

        let request = build_forced_synthesis_request(ForcedSynthesisRequestParams::new(
            messages,
            llm.model_name(),
            self.memory_context.as_deref(),
            self.scratchpad_context.as_deref(),
            self.notify_tool_guidance_enabled,
        ));

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

        match tokio::time::timeout(timeout, llm.complete(request)).await {
            Ok(Ok(response)) => {
                let text: String = response
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("");
                if text.trim().is_empty() {
                    None
                } else {
                    Some(text)
                }
            }
            Ok(Err(e)) => {
                tracing::warn!("forced synthesis turn failed: {e}");
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

    /// Perceive step.
    async fn perceive(
        &mut self,
        snapshot: &PerceptionSnapshot,
    ) -> Result<ProcessedPerception, LoopError> {
        let mut snapshot_with_steer = snapshot.clone();
        snapshot_with_steer.steer_context = self.pending_steer.take();

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
        self.turn_execution_profile = detect_turn_execution_profile_for_ownership(
            &user_message,
            &self.tool_executor.tool_definitions(),
            self.direct_inspection_ownership,
        );
        self.bounded_local_phase = BoundedLocalPhase::Discovery;
        self.bounded_local_recovery_used = false;
        self.bounded_local_recovery_focus.clear();
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
            TurnExecutionProfile::Standard => {}
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
                    RootTurnDeliverable::ArtifactWrite { .. } => None,
                })
                .collect::<Vec<_>>();
            let artifact_paths = contract
                .deliverables
                .iter()
                .filter_map(|deliverable| match deliverable {
                    RootTurnDeliverable::ArtifactWrite { path, .. } => Some(path.clone()),
                    RootTurnDeliverable::ResponseSection { .. } => None,
                })
                .collect::<Vec<_>>();
            tracing::info!(
                deliverable_block_count = root_turn_contract.deliverable_block_count,
                response_sections = ?response_sections,
                artifact_paths = ?artifact_paths,
                "extracted root turn completion contract"
            );
            self.emit_signal(
                LoopStep::Perceive,
                SignalKind::Trace,
                "extracted root turn completion contract",
                serde_json::json!({
                    "deliverable_block_count": root_turn_contract.deliverable_block_count,
                    "response_sections": response_sections,
                    "artifact_paths": artifact_paths,
                }),
            );
        }
        self.last_reasoning_messages = build_reasoning_messages(&processed);

        Ok(processed)
    }

    /// Reason step.
    async fn reason(
        &mut self,
        perception: &ProcessedPerception,
        llm: &dyn LlmProvider,
        stream: CycleStream<'_>,
    ) -> Result<CompletionResponse, LoopError> {
        self.maybe_publish_reason_progress(stream);
        if let TurnExecutionProfile::DirectUtility(profile) = &self.turn_execution_profile {
            let direct_tools = self.current_reasoning_tool_definitions(false);
            return Ok(direct_utility_completion_response(
                profile,
                &perception.user_message,
                &direct_tools,
            ));
        }
        let termination = self.current_termination_config();
        let tc = termination.as_ref();
        let should_strip_tools = tc.nudge_after_tool_turns > 0
            && self.consecutive_tool_turns
                >= tc
                    .nudge_after_tool_turns
                    .saturating_add(tc.strip_tools_after_nudge);
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
                    "reason",
                    StreamPhase::Reason,
                    TextStreamVisibility::Public,
                ),
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
            LoopStep::Reason => TextStreamVisibility::Public,
            LoopStep::Act => TextStreamVisibility::Hidden,
            _ => TextStreamVisibility::Public,
        }
    }

    fn ensure_continuation_budget(
        &self,
        continuation_messages: &[Message],
        step: LoopStep,
    ) -> Result<(), LoopError> {
        let cost = continuation_budget_cost_estimate(continuation_messages);
        self.budget
            .check_at(current_time_ms(), &cost)
            .map_err(|_| loop_error(step_stage(step), "continuation budget exhausted", true))
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
        self.ensure_continuation_budget(continuation_messages, step)?;
        let continuation_tools =
            self.apply_turn_execution_profile_tool_surface(self.tool_executor.tool_definitions());
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
        let request_messages = request.messages.clone();
        let response = self
            .request_completion(
                llm,
                request,
                StreamingRequestContext::new(
                    step_stage(step),
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
            self.emit_continuation_trace(step, attempts);
            let continuation_messages = build_continuation_messages(base_messages, &full_text);
            let continued = self
                .request_truncated_continuation(llm, &continuation_messages, step, stream)
                .await?;
            combined = merge_continuation_response(combined, continued, &mut full_text);
        }

        Ok(combined)
    }

    fn capture_tool_response_state(&mut self, response: &CompletionResponse) {
        self.tool_call_provider_ids = extract_tool_use_provider_ids(&response.content);
        self.pending_tool_response_text = response_text_segment(response);
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
        push_response_segment(&mut state.accumulated_text, response_text_segment(response));
    }

    /// Decide step.
    async fn decide(&mut self, response: &CompletionResponse) -> Result<Decision, LoopError> {
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
            let plan = parse_decomposition_plan(&decompose_call.arguments)?;
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

    /// Evaluate decompose gates in order: batch detection → complexity floor → cost gate.
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

        self.evaluate_cost_gate(plan, decision)
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
            .map(|u| {
                serde_json::json!({
                    "input_tokens": u.input_tokens,
                    "output_tokens": u.output_tokens,
                })
            })
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
            metadata["input_tokens"] = serde_json::json!(usage.input_tokens);
            metadata["output_tokens"] = serde_json::json!(usage.output_tokens);
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
            let classification = calls
                .iter()
                .find(|call| call.id == result.tool_call_id)
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
                "classification": tool_call_classification_label(classification),
            });
            let diagnostics = self
                .pending_tool_result_diagnostics
                .remove(&result.tool_call_id);
            if !result.success && result.tool_name == "run_command" {
                if let Some(diagnostics) = diagnostics {
                    metadata["diagnostics"] = diagnostics.as_metadata_value();
                }
            }
            self.emit_signal(
                LoopStep::Act,
                kind,
                format!("tool {}", result.tool_name),
                metadata,
            );
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
        let partial_response = stitched_response_text(
            &state.accumulated_text,
            summarize_tool_progress(&state.all_tool_results),
        );
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

        let plan = parse_decomposition_plan(&decompose_call.arguments)?;
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

fn tool_call_classification_label(classification: ToolCallClassification) -> &'static str {
    match classification {
        ToolCallClassification::Observation => "observation",
        ToolCallClassification::Mutation => "mutation",
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

fn step_stage(step: LoopStep) -> &'static str {
    match step {
        LoopStep::Reason => "reason",
        LoopStep::Act => "act",
        _ => "act",
    }
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
    let right_in = right.as_ref().map(|u| u.input_tokens).unwrap_or(0);
    let right_out = right.as_ref().map(|u| u.output_tokens).unwrap_or(0);

    Some(Usage {
        input_tokens: left_in.saturating_add(right_in),
        output_tokens: left_out.saturating_add(right_out),
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
        return TokenUsage {
            input_tokens: u64::from(usage.input_tokens),
            output_tokens: u64::from(usage.output_tokens),
        };
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
    }
}

fn reasoning_token_usage(total_tokens: u64) -> TokenUsage {
    TokenUsage {
        input_tokens: total_tokens.saturating_mul(3) / 5,
        output_tokens: total_tokens.saturating_mul(2) / 5,
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
            fx_llm::ContentBlock::ToolUse { name, .. } => format!("[tool_use:{name}]"),
            fx_llm::ContentBlock::ToolResult { tool_use_id, .. } => {
                format!("[tool_result:{tool_use_id}]")
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

fn extract_root_turn_contract(user_message: &str) -> RootTurnContractExtraction {
    let deliverable_parse = extract_deliverable_response_sections(user_message);
    let mut deliverables = deliverable_parse
        .labels
        .into_iter()
        .map(|label| RootTurnDeliverable::ResponseSection {
            normalized_label: normalize_contract_label(&label),
            label,
        })
        .collect::<Vec<_>>();

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

fn render_root_turn_contract_directive(contract: &RootTurnContract) -> String {
    let mut directive =
        String::from("Do not finish this turn until the root-turn deliverables are satisfied.\n");

    let response_sections = contract
        .deliverables
        .iter()
        .filter_map(|deliverable| match deliverable {
            RootTurnDeliverable::ResponseSection { label, .. } => Some(label.as_str()),
            RootTurnDeliverable::ArtifactWrite { .. } => None,
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

    directive.push_str(
        "If a deliverable is still missing, continue the turn instead of stopping at a progress-only update.",
    );
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

    if missing_response_sections.is_empty() && pending_artifact_paths.is_empty() {
        None
    } else {
        Some(RootTurnCompletionBlock {
            missing_response_sections,
            pending_artifact_paths,
        })
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
    if !block.pending_artifact_paths.is_empty() {
        directive.push_str("Pending artifact writes:\n");
        for path in &block.pending_artifact_paths {
            directive.push_str("- ");
            directive.push_str(path);
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
                .as_ref()
                .and_then(|text| meaningful_response_text(text))
        }),
        ActionNextStep::Continue(continuation) => continuation
            .partial_response
            .as_ref()
            .and_then(|text| meaningful_response_text(text)),
    }
}

fn summarize_tool_progress(results: &[ToolResult]) -> Option<String> {
    let successes: Vec<_> = results.iter().filter(|result| result.success).collect();
    let failures: Vec<_> = results.iter().filter(|result| !result.success).collect();

    if successes.is_empty() && failures.is_empty() {
        return None;
    }

    let mut parts = Vec::new();
    if !successes.is_empty() {
        let names = successes
            .iter()
            .map(|result| result.tool_name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        parts.push(format!("completed tool work: {names}"));
    }
    if !failures.is_empty() {
        let latest = failures.last().expect("failures is non-empty");
        parts.push(format!(
            "latest blocker: {}",
            truncate_prompt_text(&latest.output, 160)
        ));
    }

    Some(parts.join(". "))
}

pub(super) fn loop_error(stage: &str, reason: &str, recoverable: bool) -> LoopError {
    LoopError {
        stage: stage.to_string(),
        reason: reason.to_string(),
        recoverable,
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
