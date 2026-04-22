use super::bounded_local::{
    bounded_local_terminal_partial_response, partition_by_bounded_local_phase_semantics,
    BoundedLocalTerminalReason, TurnExecutionProfile,
};
use super::compaction::CompactionScope;
use super::request::{build_continuation_request, ContinuationRequestParams, ToolRequestConfig};
use super::retry::{
    mutation_guard_terminal_for_round, partition_by_mutation_guard_policy,
    partition_by_retry_policy, partition_by_same_batch_mutation_guard_policy, BlockedToolCall,
    BlockedToolSource, MutationGuardBlockKind, MutationGuardTerminal, RetryBlockKind,
};
use super::streaming::{StreamingRequestContext, TextStreamVisibility};
use super::turn_control::{
    FinalAnswerDecision, ObservationFollowUpDecision, ObservationFollowUpFacts,
    PendingToolCallState, ToolContinuationFacts, ToolEvidenceTerminalDecision,
    ToolEvidenceTerminalFacts, TurnControlPlane,
};
use super::{
    continuation_budget_cost, continuation_budget_cost_estimate, current_time_ms,
    estimate_text_tokens, estimate_tokens, extract_progress_evidence_references,
    final_response_turn_commitment, find_decompose_tool_call, loop_error, meaningful_response_text,
    normalize_contract_label, render_repeated_tool_failure_directive,
    repeated_tool_failure_partial_response, repeated_tool_failure_terminal_reason,
    response_text_segment, signal_metadata_value, stitch_response_segments, stitched_response_text,
    summarize_tool_progress, tool_continuation_artifact_write_target,
    tool_continuation_turn_commitment, tool_error_relay_directive, ContextWindowStats, CycleStream,
    FollowUpDecomposeContext, LlmProvider, LoopEngine, ProgressSlot, ProgressSlotKind,
    ProgressSlotStatus, RepeatedToolFailureEvent, RepeatedToolFailureState, ToolProgressClass,
    ToolProgressEntry, ToolProgressOutcome, ToolRoundState, NOTIFY_TOOL_NAME,
    OBSERVATION_ONLY_CALL_BLOCK_REASON, OBSERVATION_ONLY_MUTATION_REPLAN_DIRECTIVE,
    OBSERVATION_ONLY_TOOL_ROUND_NUDGE, TOOL_ROUND_PROGRESS_NUDGE,
};
use crate::act::{
    ActionContinuation, ActionNextStep, ActionResult, ActionTerminal, ContinuationToolScope,
    FailureClass, TokenUsage, ToolCacheability, ToolCallClassification, ToolExecutor, ToolResult,
};
use crate::budget::{
    truncate_tool_result, ActionCost, BudgetState, ToolStrippingAfterNudge,
    DEFAULT_TOOL_INVOCATION_COST_CENTS,
};
use crate::decide::Decision;
use crate::signals::{ControlPlaneDecisionKind, LoopStep, Signal, SignalKind};
use crate::streaming::{ErrorCategory, Phase, TranscriptTurnPhase};
use crate::types::LoopError;
use fx_core::message::{InternalMessage, StreamPhase, ToolRoundCall, ToolRoundResult};
use fx_llm::{
    CompletionRequest, CompletionResponse, ContentBlock, Message, MessageRole, ToolCall,
    ToolDefinition,
};
use std::borrow::Cow;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

pub(super) const TOOL_SYNTHESIS_MAX_OUTPUT_TOKENS: u32 = 1024;
const DIRECT_INSPECTION_EMPTY_SUMMARY_RESPONSE: &str =
    "Inspection completed but produced no summary.";
const SPAWN_AGENT_TOOL_NAME: &str = "spawn_agent";
const SUBAGENT_STATUS_TOOL_NAME: &str = "subagent_status";
const SUBAGENT_HARVEST_WAIT_TIMEOUT_SECONDS: u64 = 600;
const MIN_TOOL_LOOP_HARD_ITERATIONS: u32 = 3;
const ROOT_SYNTHESIS_FROM_TOOL_EVIDENCE_DIRECTIVE: &str = concat!(
    "Tool transaction controller: committed observed tool evidence back to root synthesis. ",
    "Do not treat this as terminal tool chatter. Produce the final answer from the observed ",
    "tool results without requesting additional tools."
);
const SUBAGENT_HARVEST_FINAL_SYNTHESIS_DIRECTIVE: &str = concat!(
    "Kernel orchestration completed pending subagent waits and appended their terminal results. ",
    "Produce the final answer from the observed evidence now. Do not request more tools."
);

struct PreparedToolCalls {
    allowed: Vec<ToolCall>,
    blocked: Vec<BlockedToolCall>,
}

impl PreparedToolCalls {
    fn new(allowed: Vec<ToolCall>, blocked: Vec<BlockedToolCall>) -> Self {
        Self { allowed, blocked }
    }

    fn filtered(mut self, allowed: Vec<ToolCall>, blocked: Vec<BlockedToolCall>) -> Self {
        self.allowed = allowed;
        self.blocked.extend(blocked);
        self
    }
}

#[derive(Debug)]
pub(super) struct ToolExecutionBatch {
    pub(super) results: Vec<ToolResult>,
    pub(super) blocked: Vec<BlockedToolCall>,
}

#[derive(Debug)]
pub(super) enum ToolRoundOutcome {
    Cancelled,
    /// Budget soft-ceiling crossed after tool execution; skip LLM continuation.
    BudgetLow(ToolLoopExhaustionReason),
    /// A terminal execution profile can answer immediately from tool output.
    ProfileAnswered(String),
    /// Deterministic mutation guardrails stopped same-turn variant churn.
    MutationGuardTerminal(MutationGuardTerminal),
    /// The model requested only tool calls that already succeeded this turn.
    DuplicateSuccessTerminal,
    /// Repeated blocked/failed tool attempts reached the hard terminal guard.
    RepeatedFailureTerminal(RepeatedToolFailureState),
    /// Bounded-local phase machine reached a typed terminal blocker.
    BoundedLocalTerminal(BoundedLocalTerminalReason),
    /// Repeated observation-only rounds were blocked and could not be replanned.
    ObservationRestricted,
    /// Repeated observation-only rounds were blocked; request one mutation-only follow-up.
    ObservationRestrictedReplan,
    Response(CompletionResponse),
}

enum ToolLoopStep {
    Continue(ToolRoundState),
    Break(ToolRoundState, ToolLoopExhaustionReason),
    Return(Box<ActionResult>),
}

#[allow(clippy::large_enum_variant)]
enum ToolLoopExit {
    Exhausted {
        state: ToolRoundState,
        reason: ToolLoopExhaustionReason,
    },
    Return(Box<ActionResult>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ToolLoopExhaustionReason {
    BudgetLow,
    ToolRoundBudgetExhausted,
    ContinuationBudgetExhausted,
    HardIterationLimit,
}

impl ToolLoopExhaustionReason {
    fn signal_reason(self) -> &'static str {
        match self {
            Self::BudgetLow => "budget_low",
            Self::ToolRoundBudgetExhausted | Self::ContinuationBudgetExhausted => {
                "budget_exhausted"
            }
            Self::HardIterationLimit => "iteration_limit",
        }
    }

    fn signal_message(self) -> &'static str {
        match self {
            Self::BudgetLow => {
                "tool transaction reached the soft budget boundary; continuing root synthesis"
            }
            Self::ToolRoundBudgetExhausted => {
                "tool transaction hit the tool dispatch budget boundary; continuing root synthesis"
            }
            Self::ContinuationBudgetExhausted => {
                "tool transaction hit the continuation budget boundary; continuing root synthesis"
            }
            Self::HardIterationLimit => {
                "tool transaction hit the hard iteration ceiling; continuing root synthesis"
            }
        }
    }

    fn incomplete_reason(self) -> &'static str {
        match self {
            Self::BudgetLow => "tool transaction stopped at the soft budget boundary",
            Self::ToolRoundBudgetExhausted => {
                "tool transaction stopped before dispatching pending tools due to budget"
            }
            Self::ContinuationBudgetExhausted => {
                "tool transaction stopped before continuation due to budget"
            }
            Self::HardIterationLimit => "tool transaction stopped at the hard iteration ceiling",
        }
    }

    fn pending_tool_call_state(self, pending_calls: &[ToolCall]) -> Option<PendingToolCallState> {
        (!pending_calls.is_empty()).then_some(PendingToolCallState::ResourceBoundary)
    }
}

struct ExecutedToolRound {
    calls: Vec<ToolCall>,
    results: Vec<ToolResult>,
    blocked: Vec<BlockedToolCall>,
    has_tool_errors: bool,
    started_at_ms: u64,
}

struct SerializedToolRoundCalls {
    execute: Vec<ToolCall>,
    deferred: Vec<ToolCall>,
    deferred_message: Option<String>,
}

struct ToolRoundContinuationRequest<'a> {
    round: u32,
    llm: &'a dyn LlmProvider,
    continuation_tools: Vec<ToolDefinition>,
    calls_count: usize,
    started_at_ms: u64,
    stream: CycleStream<'a>,
}

struct ToolContinuationPayload {
    response_text: String,
    response: String,
    tokens_used: TokenUsage,
    next_tool_scope: Option<ContinuationToolScope>,
    context_messages: Vec<Message>,
    can_finish: bool,
}

struct ToolEvidenceTerminalParts {
    tool_results: Vec<ToolResult>,
    pending_calls: Vec<ToolCall>,
    evidence_messages: Vec<Message>,
    tokens_used: TokenUsage,
}

#[derive(serde::Serialize)]
struct RetrySignalMetadata<'a> {
    decision_kind: ControlPlaneDecisionKind,
    decision: &'static str,
    retry_cause: &'static str,
    tool: &'a str,
    tool_call_id: &'a str,
    attempt: u16,
    prior_failures: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure_class: Option<&'static str>,
    permanent: bool,
    cycle_total_failures: u16,
}

#[derive(serde::Serialize)]
struct BlockedToolSignalMetadata<'a> {
    decision_kind: ControlPlaneDecisionKind,
    decision: &'static str,
    source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    block_kind: Option<&'static str>,
    tool: &'a str,
    reason: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure_class: Option<&'static str>,
    permanent: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    guidance: Option<&'a str>,
    signature_failures: u16,
    cycle_total_failures: u16,
}

impl LoopEngine {
    pub(super) fn publish_tool_use(&self, call: &ToolCall) {
        let Some(bus) = self.public_event_bus() else {
            return;
        };
        let _ = bus.publish(InternalMessage::ToolUse {
            call_id: call.id.clone(),
            provider_id: self.tool_call_provider_ids.get(&call.id).cloned(),
            name: call.name.clone(),
            arguments: call.arguments.clone(),
        });
    }

    pub(super) fn publish_tool_round(
        &mut self,
        calls: &[ToolCall],
        results: &[ToolResult],
        progress_entries: &[ToolProgressEntry],
        stream: CycleStream<'_>,
    ) {
        let activity_id = tool_round_activity_id(calls);
        if let Some(activity_id) = activity_id.as_deref() {
            self.emit_transcript_phase_boundary(stream, TranscriptTurnPhase::ExecutingTools);
            stream.activity_start(activity_id, tool_round_activity_title(calls));
        }

        for call in calls {
            if let Some(activity_id) = activity_id.as_deref() {
                stream.activity_tool_call_start(activity_id, call);
            }
            stream.tool_call_start(call);
            if let Some(activity_id) = activity_id.as_deref() {
                stream.activity_tool_call_complete(activity_id, call);
            }
            stream.tool_call_complete(call);
            self.publish_tool_use(call);
        }

        let progress_by_call_id: HashMap<&str, &ToolProgressEntry> = progress_entries
            .iter()
            .map(|entry| (entry.call_id.as_str(), entry))
            .collect();
        let mut emitted_progress_call_ids = HashSet::new();
        for result in results {
            if let Some(activity_id) = activity_id.as_deref() {
                stream.activity_tool_result(activity_id, result);
            }
            stream.tool_result(result);
            if let Some(entry) = progress_by_call_id.get(result.tool_call_id.as_str()) {
                stream.tool_progress(activity_id.as_deref(), entry);
                emitted_progress_call_ids.insert(result.tool_call_id.as_str());
            }
            self.publish_tool_result(result);
        }
        for entry in progress_entries {
            if !emitted_progress_call_ids.contains(entry.call_id.as_str()) {
                stream.tool_progress(activity_id.as_deref(), entry);
            }
        }

        if let Some(activity_id) = activity_id.as_deref() {
            stream.activity_end(activity_id);
        }

        let Some(bus) = self.public_event_bus() else {
            return;
        };
        let _ = bus.publish(InternalMessage::ToolRound {
            calls: calls
                .iter()
                .map(|call| ToolRoundCall {
                    call_id: call.id.clone(),
                    provider_id: self.tool_call_provider_ids.get(&call.id).cloned(),
                    name: call.name.clone(),
                    arguments: call.arguments.clone(),
                })
                .collect(),
            results: results
                .iter()
                .map(|result| ToolRoundResult {
                    call_id: result.tool_call_id.clone(),
                    name: result.tool_name.clone(),
                    success: result.success,
                    content: result.output.clone(),
                })
                .collect(),
        });
    }

    pub(super) fn emit_tool_errors(&self, results: &[ToolResult], stream: CycleStream<'_>) -> bool {
        let mut has_errors = false;
        for result in results.iter().filter(|result| !result.success) {
            has_errors = true;
            stream.tool_error(&result.tool_name, &result.output);
        }
        has_errors
    }

    pub(super) fn publish_tool_result(&mut self, result: &ToolResult) {
        if result.success && result.tool_name == NOTIFY_TOOL_NAME {
            self.notify_called_this_cycle = true;
        }
        let Some(bus) = self.public_event_bus() else {
            return;
        };
        let _ = bus.publish(InternalMessage::ToolResult {
            call_id: result.tool_call_id.clone(),
            name: result.tool_name.clone(),
            success: result.success,
            content: result.output.clone(),
        });
    }

    pub(super) fn record_tool_execution_cost(
        &mut self,
        calls: &[ToolCall],
        results: &[ToolResult],
        blocked_count: usize,
    ) {
        let mut signature_failures: HashMap<String, u16> = HashMap::new();
        let result_map: HashMap<&str, &ToolResult> = results
            .iter()
            .map(|result| (result.tool_call_id.as_str(), result))
            .collect();
        let mut tool_invocations = blocked_count as u32;
        let mut cost_cents = blocked_count as u64;

        for call in calls {
            let Some(result) = result_map.get(call.id.as_str()) else {
                continue;
            };
            tool_invocations = tool_invocations.saturating_add(1);
            cost_cents = cost_cents.saturating_add(tool_result_cost_cents(
                self.tool_retry_tracker.consecutive_failures_for(call),
                call,
                result,
                &mut signature_failures,
            ));
        }

        self.budget.record(&ActionCost {
            llm_calls: 0,
            tool_invocations,
            tokens: 0,
            cost_cents,
        });
    }

    pub(super) fn record_successful_tool_classifications(
        &self,
        state: &mut ToolRoundState,
        calls: &[ToolCall],
        results: &[ToolResult],
    ) {
        for result in results.iter().filter(|result| result.success) {
            let classification = calls
                .iter()
                .find(|call| call.id == result.tool_call_id)
                .map(|call| self.tool_executor.classify_call(call))
                .unwrap_or_else(|| {
                    classification_for_tool_name(self.tool_executor.as_ref(), result)
                });
            match classification {
                ToolCallClassification::Observation => state.used_observation_tools = true,
                ToolCallClassification::Orchestration => {}
                ToolCallClassification::Mutation => state.used_mutation_tools = true,
            }
            if state.used_observation_tools && state.used_mutation_tools {
                break;
            }
        }
    }

    #[cfg(test)]
    pub(super) async fn execute_tool_calls(
        &mut self,
        calls: &[ToolCall],
    ) -> Result<Vec<ToolResult>, LoopError> {
        Ok(self
            .execute_tool_calls_batch_with_stream(calls, CycleStream::disabled())
            .await?
            .results)
    }

    pub(super) async fn execute_tool_calls_batch_with_stream(
        &mut self,
        calls: &[ToolCall],
        stream: CycleStream<'_>,
    ) -> Result<ToolExecutionBatch, LoopError> {
        let prepared = self.prepare_tool_calls_for_execution(calls);
        self.emit_blocked_tool_errors(&prepared.blocked, stream);
        let mut results = self
            .execute_allowed_tool_calls(&prepared.allowed, stream)
            .await?;
        // Emit retry-attempt signals before mutating tracker state so the
        // signal reflects "this execution was a retry of prior failures."
        self.emit_retry_attempt_signals(&prepared.allowed, &results);
        self.capture_tool_execution_diagnostics(&results);
        self.record_tool_execution_cost(&prepared.allowed, &results, prepared.blocked.len());
        self.tool_retry_tracker.record_results(
            &prepared.allowed,
            &results,
            self.tool_executor.as_ref(),
        );
        self.tool_retry_tracker.record_mutation_results(
            &prepared.allowed,
            &results,
            self.tool_executor.as_ref(),
        );
        results.extend(build_blocked_tool_results(&prepared.blocked));
        Ok(ToolExecutionBatch {
            results: reorder_results_by_calls(calls, results),
            blocked: prepared.blocked,
        })
    }

    fn emit_retry_attempt_signals(&mut self, allowed: &[ToolCall], results: &[ToolResult]) {
        let executed_call_ids = results
            .iter()
            .map(|result| result.tool_call_id.as_str())
            .collect::<HashSet<_>>();

        for call in allowed {
            if !executed_call_ids.contains(call.id.as_str()) {
                continue;
            }
            let Some(retry_attempt) = self.tool_retry_tracker.retry_attempt_for(call) else {
                continue;
            };

            let mut signal = Signal::new(
                LoopStep::Act,
                SignalKind::Retry,
                format!("retrying tool '{}'", call.name),
                signal_metadata_value(RetrySignalMetadata {
                    decision_kind: ControlPlaneDecisionKind::RetryPolicy,
                    decision: "retry_allowed",
                    retry_cause: "prior_failure",
                    tool: &call.name,
                    tool_call_id: &call.id,
                    attempt: retry_attempt.attempt,
                    prior_failures: retry_attempt.prior_failures,
                    failure_class: retry_attempt.failure_class.map(|class| class.as_str()),
                    permanent: retry_attempt
                        .failure_class
                        .is_some_and(FailureClass::is_permanent),
                    cycle_total_failures: self.tool_retry_tracker.cycle_total_failures(),
                }),
                current_time_ms(),
            );
            if let Some(cause_id) = self.cause_id_for_tool_call(call) {
                signal = signal.with_cause_id(cause_id);
            }
            let _ = self.emit_structured_signal(signal);
        }
    }

    fn prepare_tool_calls_for_execution(&self, calls: &[ToolCall]) -> PreparedToolCalls {
        let retry_policy = self.budget.config().retry_policy();
        let (allowed, blocked) =
            partition_by_retry_policy(calls, &self.tool_retry_tracker, &retry_policy);
        let prepared = PreparedToolCalls::new(allowed, blocked);
        let prepared = self.filter_calls_by_profile_tool_names(prepared);
        let prepared = self.filter_calls_by_bounded_local_semantics(prepared);
        let prepared = self.filter_calls_by_observation_controls(prepared);
        self.filter_calls_by_mutation_guards(prepared)
    }

    fn filter_calls_by_profile_tool_names(&self, prepared: PreparedToolCalls) -> PreparedToolCalls {
        let (Some(allowed_names), Some(reason)) = (
            self.turn_execution_profile_tool_names(),
            self.turn_execution_profile_block_reason(),
        ) else {
            return prepared;
        };
        let (allowed, blocked) =
            partition_by_allowed_tool_names(&prepared.allowed, &allowed_names, reason);
        prepared.filtered(allowed, blocked)
    }

    fn filter_calls_by_bounded_local_semantics(
        &self,
        prepared: PreparedToolCalls,
    ) -> PreparedToolCalls {
        if !matches!(
            &self.turn_execution_profile,
            TurnExecutionProfile::BoundedLocal
        ) {
            return prepared;
        }
        let artifact_target = self
            .pending_artifact_write_target
            .as_deref()
            .or(self.requested_artifact_target.as_deref());
        let (allowed, blocked) = partition_by_bounded_local_phase_semantics(
            &prepared.allowed,
            self.bounded_local_phase,
            artifact_target,
        );
        prepared.filtered(allowed, blocked)
    }

    fn filter_calls_by_observation_controls(
        &self,
        prepared: PreparedToolCalls,
    ) -> PreparedToolCalls {
        if !self
            .turn_execution_profile
            .uses_standard_observation_controls()
            || !self.observation_only_call_restriction_active()
        {
            return prepared;
        }
        let (allowed, blocked) = partition_by_call_classification(
            &prepared.allowed,
            self.tool_executor.as_ref(),
            ToolCallClassification::Mutation,
            OBSERVATION_ONLY_CALL_BLOCK_REASON,
        );
        prepared.filtered(allowed, blocked)
    }

    fn filter_calls_by_mutation_guards(&self, prepared: PreparedToolCalls) -> PreparedToolCalls {
        let (allowed, blocked) = partition_by_mutation_guard_policy(
            &prepared.allowed,
            &self.tool_retry_tracker,
            self.tool_executor.as_ref(),
        );
        prepared.filtered(allowed, blocked)
    }

    pub(super) fn emit_blocked_tool_errors(
        &mut self,
        blocked: &[BlockedToolCall],
        stream: CycleStream<'_>,
    ) {
        for blocked_call in blocked {
            let call = &blocked_call.call;
            let signature_failures = self.tool_retry_tracker.consecutive_failures_for(call);
            let mut signal = Signal::new(
                LoopStep::Act,
                SignalKind::Blocked,
                format!("tool '{}' blocked: {}", call.name, blocked_call.reason),
                signal_metadata_value(BlockedToolSignalMetadata {
                    decision_kind: ControlPlaneDecisionKind::ToolCallGuardrail,
                    decision: "blocked",
                    source: blocked_call.source.as_str(),
                    block_kind: blocked_call.source.block_kind(),
                    tool: &call.name,
                    reason: &blocked_call.reason,
                    failure_class: blocked_call.failure_class.map(|class| class.as_str()),
                    permanent: blocked_call
                        .failure_class
                        .is_some_and(FailureClass::is_permanent),
                    guidance: blocked_call.guidance.as_deref(),
                    signature_failures,
                    cycle_total_failures: self.tool_retry_tracker.cycle_total_failures(),
                }),
                current_time_ms(),
            );
            if let Some(cause_id) = self.cause_id_for_tool_call(call) {
                signal = signal.with_cause_id(cause_id);
            }
            let _ = self.emit_structured_signal(signal);
            stream.emit_error(
                ErrorCategory::ToolExecution,
                blocked_tool_message(
                    &call.name,
                    &blocked_call.reason,
                    blocked_call.guidance.as_deref(),
                ),
                true,
            );
        }
    }

    pub(super) async fn execute_allowed_tool_calls(
        &mut self,
        allowed: &[ToolCall],
        stream: CycleStream<'_>,
    ) -> Result<Vec<ToolResult>, LoopError> {
        if allowed.is_empty() {
            return Ok(Vec::new());
        }

        let mut malformed_results = Vec::new();
        let valid = collect_valid_tool_calls(allowed, &mut malformed_results);
        let max_bytes = self.budget.config().max_tool_result_bytes;
        let executed = self
            .tool_executor
            .execute_tools(&valid, self.cancel_token.as_ref())
            .await
            .map_err(|error| {
                stream.emit_error(
                    ErrorCategory::ToolExecution,
                    tool_execution_failure_message(allowed, &error.message),
                    error.recoverable,
                );
                loop_error(
                    "act",
                    &format!("tool execution failed: {}", error.message),
                    error.recoverable,
                )
            })?;
        let mut results = truncate_tool_results(executed, max_bytes);
        results.append(&mut malformed_results);
        Ok(results)
    }

    pub(super) async fn act_with_tools(
        &mut self,
        decision: &Decision,
        calls: &[ToolCall],
        llm: &dyn LlmProvider,
        context_messages: &[Message],
        stream: CycleStream<'_>,
    ) -> Result<ActionResult, LoopError> {
        let state = self.prepare_tool_action_state(calls, context_messages);
        if self.budget.state() == BudgetState::Low {
            return Ok(self.budget_low_blocked_result(
                decision,
                "tool dispatch",
                &state.accumulated_text,
            ));
        }

        match self.run_tool_loop(decision, llm, state, stream).await? {
            ToolLoopExit::Exhausted { state, reason } => {
                debug_assert!(
                    state.pending_round_notices.is_empty(),
                    "pending_round_notices not flushed"
                );
                self.finish_tool_loop_on_exhaustion(decision, state, reason, llm, stream)
                    .await
            }
            ToolLoopExit::Return(action) => Ok(*action),
        }
    }

    fn prepare_tool_action_state(
        &mut self,
        calls: &[ToolCall],
        context_messages: &[Message],
    ) -> ToolRoundState {
        self.pending_tool_result_diagnostics.clear();
        let initial_text = self.pending_tool_response_text.take();
        let mut state = ToolRoundState::new_empty_calls(context_messages, initial_text);
        self.stage_tool_calls_for_round(&mut state, calls.to_vec());
        state
    }

    fn tool_loop_budget_low(&mut self, round: u32) -> bool {
        if self.budget.state() != BudgetState::Low {
            return false;
        }
        self.emit_budget_low_break_signal(round);
        true
    }

    async fn run_tool_loop(
        &mut self,
        decision: &Decision,
        llm: &dyn LlmProvider,
        mut state: ToolRoundState,
        stream: CycleStream<'_>,
    ) -> Result<ToolLoopExit, LoopError> {
        let mut round = 0;
        let hard_iteration_limit = self.max_iterations.max(MIN_TOOL_LOOP_HARD_ITERATIONS);
        loop {
            if self.tool_round_interrupted() {
                return Ok(ToolLoopExit::Return(Box::new(
                    self.cancelled_tool_action_from_state(decision, state),
                )));
            }
            if round >= hard_iteration_limit {
                self.emit_signal(
                    LoopStep::Act,
                    SignalKind::Blocked,
                    format!("tool loop reached hard iteration ceiling before round {round}"),
                    serde_json::json!({
                        "decision_kind": ControlPlaneDecisionKind::BudgetGuardrail,
                        "decision": "blocked",
                        "reason": "iteration_limit",
                        "round": round,
                        "max_iterations": self.max_iterations,
                        "hard_iteration_limit": hard_iteration_limit,
                        "pending_tool_count": state.current_calls.len(),
                    }),
                );
                return Ok(ToolLoopExit::Exhausted {
                    state,
                    reason: ToolLoopExhaustionReason::HardIterationLimit,
                });
            }
            if self.tool_loop_budget_low(round) {
                return Ok(ToolLoopExit::Exhausted {
                    state,
                    reason: ToolLoopExhaustionReason::BudgetLow,
                });
            }
            if self.tool_round_budget_exhausted(round, &state) {
                return Ok(ToolLoopExit::Exhausted {
                    state,
                    reason: ToolLoopExhaustionReason::ToolRoundBudgetExhausted,
                });
            }

            match self
                .run_tool_loop_round(decision, llm, state, round, stream)
                .await?
            {
                ToolLoopStep::Continue(next_state) => {
                    state = next_state;
                    round = round.saturating_add(1);
                }
                ToolLoopStep::Break(next_state, reason) => {
                    return Ok(ToolLoopExit::Exhausted {
                        state: next_state,
                        reason,
                    });
                }
                ToolLoopStep::Return(action) => return Ok(ToolLoopExit::Return(action)),
            }
        }
    }

    fn tool_round_budget_exhausted(&mut self, round: u32, state: &ToolRoundState) -> bool {
        let tool_invocations = state.current_calls.len() as u32;
        let cost = ActionCost {
            llm_calls: 0,
            tool_invocations,
            tokens: 0,
            cost_cents: u64::from(tool_invocations)
                .saturating_mul(DEFAULT_TOOL_INVOCATION_COST_CENTS),
        };
        if self.budget.check(&cost).is_ok() {
            return false;
        }

        self.emit_signal(
            LoopStep::Act,
            SignalKind::Blocked,
            format!("budget exhausted before tool round {round}"),
            serde_json::json!({
                "decision_kind": ControlPlaneDecisionKind::BudgetGuardrail,
                "decision": "blocked",
                "reason": "budget_exhausted",
                "round": round,
                "pending_tool_count": state.current_calls.len(),
            }),
        );
        true
    }

    async fn run_tool_loop_round(
        &mut self,
        decision: &Decision,
        llm: &dyn LlmProvider,
        mut state: ToolRoundState,
        round: u32,
        stream: CycleStream<'_>,
    ) -> Result<ToolLoopStep, LoopError> {
        let continuation_tools =
            self.apply_tool_round_progress_policy(round, &mut state.continuation_messages);
        let outcome = self
            .execute_tool_round(round + 1, llm, &mut state, continuation_tools, stream)
            .await?;
        self.handle_tool_round_outcome(decision, llm, state, outcome, stream)
            .await
    }

    async fn handle_tool_round_outcome(
        &mut self,
        decision: &Decision,
        llm: &dyn LlmProvider,
        state: ToolRoundState,
        outcome: ToolRoundOutcome,
        stream: CycleStream<'_>,
    ) -> Result<ToolLoopStep, LoopError> {
        match outcome {
            ToolRoundOutcome::Cancelled => Ok(ToolLoopStep::Return(Box::new(
                self.cancelled_tool_action_from_state(decision, state),
            ))),
            ToolRoundOutcome::BudgetLow(reason) => Ok(ToolLoopStep::Break(state, reason)),
            ToolRoundOutcome::ProfileAnswered(response) => Ok(ToolLoopStep::Return(Box::new(
                self.direct_utility_action_result(decision, state, response),
            ))),
            ToolRoundOutcome::MutationGuardTerminal(terminal) => Ok(ToolLoopStep::Return(
                Box::new(self.mutation_guard_terminal_action(decision, state, terminal)),
            )),
            ToolRoundOutcome::DuplicateSuccessTerminal => Ok(ToolLoopStep::Return(Box::new(
                self.duplicate_success_terminal_action(decision, state),
            ))),
            ToolRoundOutcome::RepeatedFailureTerminal(failure_state) => {
                Ok(ToolLoopStep::Return(Box::new(
                    self.repeated_tool_failure_terminal_action(decision, state, failure_state),
                )))
            }
            ToolRoundOutcome::BoundedLocalTerminal(reason) => Ok(ToolLoopStep::Return(Box::new(
                self.bounded_local_terminal_action(decision, state, reason),
            ))),
            ToolRoundOutcome::ObservationRestricted => Ok(ToolLoopStep::Return(Box::new(
                self.observation_restricted_action(decision, state, None),
            ))),
            ToolRoundOutcome::ObservationRestrictedReplan => {
                self.handle_observation_restricted_replan(decision, llm, state, stream)
                    .await
            }
            ToolRoundOutcome::Response(response) => {
                self.handle_tool_round_response(decision, llm, state, response, stream)
                    .await
            }
        }
    }

    fn direct_utility_action_result(
        &self,
        decision: &Decision,
        state: ToolRoundState,
        response: String,
    ) -> ActionResult {
        let response = stitch_response_segments(&state.accumulated_text, Some(response));
        ActionResult {
            decision: decision.clone(),
            tool_results: state.all_tool_results,
            response_text: response.clone(),
            tokens_used: state.tokens_used,
            next_step: ActionNextStep::Finish(ActionTerminal::Complete { response }),
        }
    }

    fn bounded_local_terminal_action(
        &mut self,
        decision: &Decision,
        state: ToolRoundState,
        reason: BoundedLocalTerminalReason,
    ) -> ActionResult {
        let partial_response = stitched_response_text(
            &state.accumulated_text,
            Some(bounded_local_terminal_partial_response(
                reason,
                &state.all_tool_results,
            )),
        );
        self.bounded_local_terminal_action_result(
            decision,
            state.all_tool_results,
            partial_response,
            state.tokens_used,
            reason,
        )
    }

    fn observation_restricted_action(
        &self,
        decision: &Decision,
        state: ToolRoundState,
        tail: Option<String>,
    ) -> ActionResult {
        let tail_partial = tail.and_then(|text| meaningful_response_text(&text));
        if let (ObservationFollowUpDecision::AllowSynthesis, Some(response)) = (
            TurnControlPlane::decide_observation_follow_up(ObservationFollowUpFacts {
                turn_execution_profile: &self.turn_execution_profile,
                used_observation_tools: state.used_observation_tools,
                used_mutation_tools: state.used_mutation_tools,
                observation_pressure_saturated: self.observation_only_call_restriction_active(),
                mutation_tools_available: !self.side_effect_tool_definitions().is_empty(),
                artifact_write_pending: self.artifact_write_pending(),
            }),
            tail_partial.clone(),
        ) {
            return ActionResult {
                decision: decision.clone(),
                tool_results: state.all_tool_results,
                response_text: response.clone(),
                tokens_used: state.tokens_used,
                next_step: ActionNextStep::Finish(ActionTerminal::Complete { response }),
            };
        }
        let partial = tail_partial.or_else(|| summarize_tool_progress(&state.all_tool_results));
        self.incomplete_action_result(
            decision,
            state.all_tool_results,
            partial,
            OBSERVATION_ONLY_CALL_BLOCK_REASON,
            state.tokens_used,
        )
    }

    async fn handle_observation_restricted_replan(
        &mut self,
        decision: &Decision,
        llm: &dyn LlmProvider,
        mut state: ToolRoundState,
        stream: CycleStream<'_>,
    ) -> Result<ToolLoopStep, LoopError> {
        let mutation_tools = self.side_effect_tool_definitions();
        if mutation_tools.is_empty() {
            return Ok(ToolLoopStep::Return(Box::new(
                self.observation_restricted_action(decision, state, None),
            )));
        }

        let response = self
            .request_observation_restricted_replan(llm, &mut state, mutation_tools, stream)
            .await?;
        if response.tool_calls.is_empty() {
            return self
                .finish_observation_restricted_replan(decision, llm, state, response, stream)
                .await;
        }
        self.handle_follow_up_tool_calls(llm, state, response).await
    }

    async fn request_observation_restricted_replan(
        &mut self,
        llm: &dyn LlmProvider,
        state: &mut ToolRoundState,
        continuation_tools: Vec<ToolDefinition>,
        stream: CycleStream<'_>,
    ) -> Result<CompletionResponse, LoopError> {
        self.emit_loop_phase(stream, Phase::Synthesize);
        let response = self
            .request_tool_continuation(
                llm,
                &state.continuation_messages,
                continuation_tools,
                &mut state.tokens_used,
                stream,
            )
            .await?;
        self.record_continuation_cost(&response, &state.continuation_messages);
        Ok(response)
    }

    async fn finish_observation_restricted_replan(
        &mut self,
        decision: &Decision,
        llm: &dyn LlmProvider,
        state: ToolRoundState,
        response: CompletionResponse,
        stream: CycleStream<'_>,
    ) -> Result<ToolLoopStep, LoopError> {
        let response = self
            .continue_truncated_response(
                response,
                &state.continuation_messages,
                llm,
                LoopStep::Act,
                stream,
            )
            .await?;
        Ok(ToolLoopStep::Return(Box::new(
            self.observation_restricted_action(decision, state, response_text_segment(&response)),
        )))
    }

    async fn handle_tool_round_response(
        &mut self,
        decision: &Decision,
        llm: &dyn LlmProvider,
        state: ToolRoundState,
        response: CompletionResponse,
        stream: CycleStream<'_>,
    ) -> Result<ToolLoopStep, LoopError> {
        if !response.tool_calls.is_empty() {
            return self.handle_follow_up_tool_calls(llm, state, response).await;
        }

        let response = self
            .continue_truncated_response(
                response,
                &state.continuation_messages,
                llm,
                LoopStep::Act,
                stream,
            )
            .await?;
        let next_tool_scope = self.continuation_tool_scope_for_round(&state);
        let action = self
            .finalize_tool_response(decision, state, &response, llm, stream, next_tool_scope)
            .await?;
        Ok(ToolLoopStep::Return(Box::new(action)))
    }

    async fn handle_follow_up_tool_calls(
        &mut self,
        llm: &dyn LlmProvider,
        mut state: ToolRoundState,
        response: CompletionResponse,
    ) -> Result<ToolLoopStep, LoopError> {
        if find_decompose_tool_call(&response.tool_calls).is_some() {
            let ToolRoundState {
                all_tool_results,
                accumulated_text,
                continuation_messages,
                tokens_used,
                ..
            } = state;
            let context = FollowUpDecomposeContext {
                prior_tool_results: all_tool_results,
                prior_tokens_used: tokens_used,
                accumulated_text,
            };
            let action = self
                .handle_follow_up_decompose(&response, llm, &continuation_messages, context)
                .await?;
            return Ok(ToolLoopStep::Return(Box::new(action)));
        }

        self.record_tool_round_response_state(&mut state, &response);
        self.stage_tool_calls_for_round(&mut state, response.tool_calls);
        Ok(ToolLoopStep::Continue(state))
    }

    fn repeated_tool_failure_terminal_action(
        &mut self,
        decision: &Decision,
        state: ToolRoundState,
        failure_state: RepeatedToolFailureState,
    ) -> ActionResult {
        self.repeated_tool_failure_terminal_action_from_parts(
            decision,
            state.all_tool_results,
            &state.accumulated_text,
            state.tokens_used,
            failure_state,
        )
    }

    fn duplicate_success_terminal_action(
        &mut self,
        decision: &Decision,
        state: ToolRoundState,
    ) -> ActionResult {
        self.tool_evidence_terminal_action(
            decision,
            state,
            Some(ContinuationToolScope::NoTools),
            Some(Vec::new()),
            None,
            "duplicate_success",
            "duplicate successful tool request reused existing evidence; continuing root synthesis",
            "duplicate successful tool request did not add new evidence",
        )
    }

    fn repeated_tool_failure_terminal_action_from_parts(
        &mut self,
        decision: &Decision,
        tool_results: Vec<ToolResult>,
        accumulated_text: &[String],
        tokens_used: TokenUsage,
        failure_state: RepeatedToolFailureState,
    ) -> ActionResult {
        let reason = repeated_tool_failure_terminal_reason(&failure_state);
        self.emit_signal(
            LoopStep::Act,
            SignalKind::Blocked,
            &reason,
            serde_json::json!({
                "tool": failure_state.failure.key.tool_name.as_str(),
                "consecutive_failures": failure_state.consecutive_failures,
                "failure_summary": failure_state.failure.summary.as_str(),
            }),
        );
        let partial_seed = stitched_response_text(accumulated_text, None);
        let partial_response =
            repeated_tool_failure_partial_response(partial_seed.as_deref(), &failure_state);
        ActionResult {
            decision: decision.clone(),
            tool_results,
            response_text: partial_response.clone().unwrap_or_default(),
            tokens_used,
            next_step: ActionNextStep::Finish(ActionTerminal::Incomplete {
                partial_response,
                reason,
            }),
        }
    }

    async fn finish_tool_loop_on_exhaustion(
        &mut self,
        decision: &Decision,
        state: ToolRoundState,
        reason: ToolLoopExhaustionReason,
        _llm: &dyn LlmProvider,
        _stream: CycleStream<'_>,
    ) -> Result<ActionResult, LoopError> {
        let next_tool_scope = self.continuation_tool_scope_for_round(&state);
        let pending_tool_call_state = reason.pending_tool_call_state(&state.current_calls);
        Ok(self.tool_evidence_terminal_action(
            decision,
            state,
            next_tool_scope,
            None,
            pending_tool_call_state,
            reason.signal_reason(),
            reason.signal_message(),
            reason.incomplete_reason(),
        ))
    }

    async fn finalize_tool_response(
        &mut self,
        decision: &Decision,
        mut state: ToolRoundState,
        response: &CompletionResponse,
        llm: &dyn LlmProvider,
        stream: CycleStream<'_>,
        next_tool_scope: Option<ContinuationToolScope>,
    ) -> Result<ActionResult, LoopError> {
        let pending_subagents = pending_subagent_ids(&state.all_tool_results);
        if !pending_subagents.is_empty() {
            let remaining_subagents = self.harvest_pending_subagents(&mut state, stream).await?;
            if !remaining_subagents.is_empty() {
                return Ok(self.pending_subagent_wait_timeout_action_result(
                    decision,
                    state,
                    remaining_subagents,
                ));
            }
        }

        let mut response = response.clone();
        if !pending_subagents.is_empty() {
            state.continuation_messages.push(Message::system(
                SUBAGENT_HARVEST_FINAL_SYNTHESIS_DIRECTIVE.to_string(),
            ));
            self.emit_loop_phase(stream, Phase::Synthesize);
            let synthesis = self
                .request_tool_continuation(
                    llm,
                    &state.continuation_messages,
                    Vec::new(),
                    &mut state.tokens_used,
                    stream,
                )
                .await?;
            self.record_continuation_cost(&synthesis, &state.continuation_messages);
            if !synthesis.tool_calls.is_empty() {
                let partial = summarize_tool_progress(&state.all_tool_results);
                return Ok(self.incomplete_action_result(
                    decision,
                    state.all_tool_results,
                    partial,
                    "final synthesis after subagent harvest attempted tool activity",
                    state.tokens_used,
                ));
            }
            response = self
                .continue_truncated_response(
                    synthesis,
                    &state.continuation_messages,
                    llm,
                    LoopStep::Act,
                    stream,
                )
                .await?;
        }

        if has_policy_deferred_tool_results(&state.all_tool_results) {
            let ToolRoundState {
                all_tool_results,
                evidence_messages,
                accumulated_text,
                tokens_used,
                ..
            } = state;
            return Ok(self.policy_deferred_continuation_action_result(
                decision,
                all_tool_results,
                accumulated_text,
                evidence_messages,
                tokens_used,
                next_tool_scope,
            ));
        }

        let current_round_text = response_text_segment(&response);
        let response_text =
            stitch_response_segments(&state.accumulated_text, current_round_text.clone());
        self.emit_empty_tool_response_signal_if_needed(&response_text, &state);
        if current_round_text.is_some() {
            return Ok(self.tool_continuation_action(
                decision,
                state,
                response_text,
                next_tool_scope,
            ));
        }
        if self.turn_execution_profile.allows_synthesis_fallback() {
            return self
                .synthesize_tool_fallback(decision, state, llm, stream, next_tool_scope)
                .await;
        }
        Ok(self.incomplete_tool_continuation_action(decision, state))
    }

    fn emit_empty_tool_response_signal_if_needed(
        &mut self,
        response_text: &str,
        state: &ToolRoundState,
    ) {
        if !response_text.is_empty() {
            return;
        }
        self.emit_signal(
            LoopStep::Act,
            SignalKind::Trace,
            "tool continuation returned empty text",
            serde_json::json!({
                "tool_count": state.all_tool_results.len(),
            }),
        );
    }

    fn tool_continuation_action(
        &self,
        decision: &Decision,
        state: ToolRoundState,
        response_text: String,
        next_tool_scope: Option<ContinuationToolScope>,
    ) -> ActionResult {
        let response = meaningful_response_text(&response_text)
            .expect("stitched response should be meaningful when the current round has text");
        let can_finish =
            self.tool_continuation_text_should_finish(&state, next_tool_scope.as_ref());
        let ToolRoundState {
            all_tool_results,
            evidence_messages,
            tokens_used,
            ..
        } = state;
        self.tool_continuation_action_result(
            decision,
            all_tool_results,
            ToolContinuationPayload {
                response_text,
                response,
                tokens_used,
                next_tool_scope,
                context_messages: evidence_messages,
                can_finish,
            },
        )
    }

    fn incomplete_tool_continuation_action(
        &mut self,
        decision: &Decision,
        state: ToolRoundState,
    ) -> ActionResult {
        if has_policy_deferred_tool_results(&state.all_tool_results) {
            let next_tool_scope = self.continuation_tool_scope_for_round(&state);
            let ToolRoundState {
                all_tool_results,
                evidence_messages,
                accumulated_text,
                tokens_used,
                ..
            } = state;
            return self.policy_deferred_continuation_action_result(
                decision,
                all_tool_results,
                accumulated_text,
                evidence_messages,
                tokens_used,
                next_tool_scope,
            );
        }

        let next_tool_scope = self.continuation_tool_scope_for_round(&state);
        self.tool_evidence_terminal_action(
            decision,
            state,
            next_tool_scope,
            Some(Vec::new()),
            None,
            "empty_tool_continuation",
            "empty tool continuation committed evidence to root synthesis",
            "tool continuation did not produce a usable final response",
        )
    }

    async fn synthesize_tool_fallback(
        &mut self,
        decision: &Decision,
        state: ToolRoundState,
        llm: &dyn LlmProvider,
        stream: CycleStream<'_>,
        next_tool_scope: Option<ContinuationToolScope>,
    ) -> Result<ActionResult, LoopError> {
        let ToolRoundState {
            all_tool_results,
            accumulated_text,
            evidence_messages,
            mut tokens_used,
            used_mutation_tools,
            ..
        } = state;
        let max_tokens = self.budget.config().max_synthesis_tokens;
        let evicted = evict_oldest_results(all_tool_results, max_tokens);
        let synthesis_prompt = tool_synthesis_prompt(&evicted, &self.synthesis_instruction);
        self.emit_loop_phase(stream, Phase::Synthesize);
        let synthesis_response = self
            .generate_tool_summary_response(
                &synthesis_prompt,
                llm,
                stream,
                TextStreamVisibility::Hidden,
            )
            .await?;
        let llm_text = response_text_segment(&synthesis_response).unwrap_or_default();
        tokens_used.accumulate(synthesis_usage(&synthesis_prompt, &llm_text));
        let synthesized_text = meaningful_response_text(&llm_text);
        let response_text = stitch_response_segments(&accumulated_text, synthesized_text.clone());
        let final_response = meaningful_response_text(&response_text);
        let can_finish = self.tool_continuation_text_should_finish_from_mutation_state(
            used_mutation_tools,
            next_tool_scope.as_ref(),
        );
        Ok(match synthesized_text {
            Some(_) => self.tool_continuation_action_result(
                decision,
                evicted,
                ToolContinuationPayload {
                    response_text,
                    response: final_response
                        .expect("stitched response should be meaningful when synthesis has text"),
                    tokens_used,
                    next_tool_scope,
                    context_messages: evidence_messages,
                    can_finish,
                },
            ),
            None if self
                .turn_execution_profile
                .direct_inspection_profile()
                .is_some() =>
            {
                self.direct_inspection_empty_summary_action_result(decision, evicted, tokens_used)
            }
            None if has_policy_deferred_tool_results(&evicted) => self
                .policy_deferred_continuation_action_result(
                    decision,
                    evicted,
                    accumulated_text,
                    evidence_messages,
                    tokens_used,
                    next_tool_scope,
                ),
            None => self.tool_evidence_terminal_action_from_parts(
                decision,
                ToolEvidenceTerminalParts {
                    tool_results: evicted,
                    pending_calls: Vec::new(),
                    evidence_messages,
                    tokens_used,
                },
                next_tool_scope,
                None,
                "empty_tool_synthesis",
                "empty tool synthesis committed evidence to root synthesis",
                "tool synthesis did not produce a usable final response",
            ),
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn tool_evidence_terminal_action(
        &mut self,
        decision: &Decision,
        state: ToolRoundState,
        next_tool_scope: Option<ContinuationToolScope>,
        pending_calls_override: Option<Vec<ToolCall>>,
        pending_tool_call_state: Option<PendingToolCallState>,
        reason: &'static str,
        signal_message: &'static str,
        incomplete_reason: &'static str,
    ) -> ActionResult {
        let pending_calls = pending_calls_override.unwrap_or_else(|| state.current_calls.clone());
        let ToolRoundState {
            all_tool_results,
            evidence_messages,
            tokens_used,
            ..
        } = state;
        self.tool_evidence_terminal_action_from_parts(
            decision,
            ToolEvidenceTerminalParts {
                tool_results: all_tool_results,
                pending_calls,
                evidence_messages,
                tokens_used,
            },
            next_tool_scope,
            pending_tool_call_state,
            reason,
            signal_message,
            incomplete_reason,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn tool_evidence_terminal_action_from_parts(
        &mut self,
        decision: &Decision,
        parts: ToolEvidenceTerminalParts,
        next_tool_scope: Option<ContinuationToolScope>,
        pending_tool_call_state: Option<PendingToolCallState>,
        reason: &'static str,
        signal_message: &'static str,
        incomplete_reason: &'static str,
    ) -> ActionResult {
        let ToolEvidenceTerminalParts {
            tool_results,
            pending_calls,
            evidence_messages,
            tokens_used,
        } = parts;
        let pending_tool_calls = pending_tool_call_state.unwrap_or({
            if pending_calls.is_empty() {
                PendingToolCallState::None
            } else {
                PendingToolCallState::UnresolvedIntent
            }
        });
        match TurnControlPlane::decide_tool_evidence_terminal(ToolEvidenceTerminalFacts {
            has_tool_results: !tool_results.is_empty(),
            has_policy_deferred_results: has_policy_deferred_tool_results(&tool_results),
            pending_tool_calls,
            direct_inspection: self
                .turn_execution_profile
                .direct_inspection_profile()
                .is_some(),
        }) {
            ToolEvidenceTerminalDecision::CompleteDirectInspectionEmpty => self
                .direct_inspection_empty_summary_action_result(decision, tool_results, tokens_used),
            ToolEvidenceTerminalDecision::ContinuePolicyDeferred => self
                .policy_deferred_continuation_action_result(
                    decision,
                    tool_results,
                    Vec::new(),
                    evidence_messages,
                    tokens_used,
                    next_tool_scope,
                ),
            ToolEvidenceTerminalDecision::ContinueRootSynthesis => self
                .root_synthesis_continuation_action_result(
                    decision,
                    tool_results,
                    pending_calls,
                    evidence_messages,
                    tokens_used,
                    next_tool_scope,
                    reason,
                    signal_message,
                ),
            ToolEvidenceTerminalDecision::IncompletePendingToolCalls => {
                let pending_tool_names = pending_calls
                    .iter()
                    .map(|call| call.name.as_str())
                    .collect::<Vec<_>>();
                self.emit_signal(
                    LoopStep::Act,
                    SignalKind::Blocked,
                    "tool transaction reached its boundary with unresolved tool requests",
                    serde_json::json!({
                        "decision_kind": ControlPlaneDecisionKind::ToolRoundGuardrail,
                        "decision": "incomplete_pending_tool_requests",
                        "reason": reason,
                        "tool_result_count": tool_results.len(),
                        "pending_tool_count": pending_calls.len(),
                        "pending_tools": pending_tool_names,
                    }),
                );
                self.incomplete_action_result(
                    decision,
                    tool_results,
                    None,
                    "tool transaction reached its boundary with pending tool requests",
                    tokens_used,
                )
            }
            ToolEvidenceTerminalDecision::Incomplete => self.incomplete_action_result(
                decision,
                tool_results,
                None,
                incomplete_reason,
                tokens_used,
            ),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn root_synthesis_continuation_action_result(
        &mut self,
        decision: &Decision,
        tool_results: Vec<ToolResult>,
        pending_calls: Vec<ToolCall>,
        mut context_messages: Vec<Message>,
        tokens_used: TokenUsage,
        _next_tool_scope: Option<ContinuationToolScope>,
        reason: &'static str,
        signal_message: &'static str,
    ) -> ActionResult {
        let pending_tool_names = pending_calls
            .iter()
            .map(|call| call.name.as_str())
            .collect::<Vec<_>>();
        self.emit_signal(
            LoopStep::Act,
            SignalKind::Trace,
            signal_message,
            serde_json::json!({
                "decision_kind": ControlPlaneDecisionKind::ToolRoundGuardrail,
                "decision": "continue_root_synthesis",
                "reason": reason,
                "tool_result_count": tool_results.len(),
                "pending_tool_count": pending_calls.len(),
                "pending_tools": pending_tool_names,
            }),
        );

        let mut directive = ROOT_SYNTHESIS_FROM_TOOL_EVIDENCE_DIRECTIVE.to_string();
        if !pending_calls.is_empty() {
            directive.push_str(" Pending tool requests were not executed because the inner tool transaction reached its boundary: ");
            directive.push_str(&pending_calls.len().to_string());
            directive
                .push_str(" request(s) remained pending. Do not claim these pending requests ran.");
        }
        context_messages.push(Message::system(directive));

        let continuation = ActionContinuation::new(None, None)
            .with_context_messages(context_messages)
            .with_tool_scope(ContinuationToolScope::NoTools)
            .with_turn_commitment(final_response_turn_commitment(
                "tool transaction reached its boundary with usable evidence",
                "Produce the final answer from observed tool evidence without calling more tools.",
            ));
        ActionResult {
            decision: decision.clone(),
            tool_results,
            response_text: String::new(),
            tokens_used,
            next_step: ActionNextStep::Continue(continuation),
        }
    }

    fn policy_deferred_continuation_action_result(
        &self,
        decision: &Decision,
        tool_results: Vec<ToolResult>,
        _accumulated_text: Vec<String>,
        context_messages: Vec<Message>,
        tokens_used: TokenUsage,
        next_tool_scope: Option<ContinuationToolScope>,
    ) -> ActionResult {
        let continuation =
            ActionContinuation::new(None, None).with_context_messages(context_messages);
        let continuation =
            self.apply_tool_continuation_contracts(decision, continuation, next_tool_scope);

        ActionResult {
            decision: decision.clone(),
            tool_results,
            response_text: String::new(),
            tokens_used,
            next_step: ActionNextStep::Continue(continuation),
        }
    }

    async fn harvest_pending_subagents(
        &mut self,
        state: &mut ToolRoundState,
        stream: CycleStream<'_>,
    ) -> Result<Vec<String>, LoopError> {
        let pending_subagents = pending_subagent_ids(&state.all_tool_results);
        if pending_subagents.is_empty() {
            return Ok(Vec::new());
        }

        self.emit_signal(
            LoopStep::Act,
            SignalKind::Trace,
            "harvesting pending subagent results",
            serde_json::json!({
                "decision_kind": ControlPlaneDecisionKind::ToolRoundGuardrail,
                "decision": "harvest_pending_subagents",
                "pending_subagents": &pending_subagents,
                "timeout_seconds": SUBAGENT_HARVEST_WAIT_TIMEOUT_SECONDS,
            }),
        );

        let calls = pending_subagents
            .iter()
            .enumerate()
            .map(|(index, id)| subagent_wait_tool_call(index, id))
            .collect::<Vec<_>>();
        let started_at_ms = current_time_ms();
        let batch = self
            .execute_tool_calls_batch_with_stream(&calls, stream)
            .await?;
        let executed = ExecutedToolRound {
            calls,
            has_tool_errors: batch.results.iter().any(|result| !result.success),
            results: batch.results,
            blocked: batch.blocked,
            started_at_ms,
        };
        let progress_entries = self.record_executed_tool_round(state, &executed)?;
        self.publish_tool_round(
            &executed.calls,
            &executed.results,
            &progress_entries,
            stream,
        );
        self.emit_tool_errors(&executed.results, stream);

        Ok(pending_subagent_ids(&state.all_tool_results))
    }

    fn pending_subagent_wait_timeout_action_result(
        &mut self,
        decision: &Decision,
        state: ToolRoundState,
        pending_subagents: Vec<String>,
    ) -> ActionResult {
        self.emit_signal(
            LoopStep::Act,
            SignalKind::Blocked,
            "pending subagent wait timed out",
            serde_json::json!({
                "decision_kind": ControlPlaneDecisionKind::ToolRoundGuardrail,
                "decision": "blocked",
                "guardrail": "pending_subagent_wait",
                "pending_subagents": &pending_subagents,
            }),
        );
        let partial = summarize_tool_progress(&state.all_tool_results);
        self.incomplete_action_result(
            decision,
            state.all_tool_results,
            partial,
            "pending subagent results did not finish before the orchestration wait deadline",
            state.tokens_used,
        )
    }

    fn tool_continuation_action_result(
        &self,
        decision: &Decision,
        tool_results: Vec<ToolResult>,
        payload: ToolContinuationPayload,
    ) -> ActionResult {
        let ToolContinuationPayload {
            response_text,
            response,
            tokens_used,
            next_tool_scope,
            context_messages,
            can_finish,
        } = payload;
        if can_finish {
            return ActionResult {
                decision: decision.clone(),
                tool_results,
                response_text,
                tokens_used,
                next_step: ActionNextStep::Finish(ActionTerminal::Complete { response }),
            };
        }

        let continuation = if context_messages.is_empty() {
            ActionContinuation::new(None, Some(response.clone()))
        } else {
            ActionContinuation::new(None, None).with_context_messages(context_messages)
        };
        let continuation =
            self.apply_tool_continuation_contracts(decision, continuation, next_tool_scope);
        ActionResult {
            decision: decision.clone(),
            tool_results,
            response_text,
            tokens_used,
            next_step: ActionNextStep::Continue(continuation),
        }
    }

    fn tool_continuation_text_should_finish(
        &self,
        state: &ToolRoundState,
        next_tool_scope: Option<&ContinuationToolScope>,
    ) -> bool {
        self.tool_continuation_text_should_finish_from_mutation_state(
            state.used_mutation_tools,
            next_tool_scope,
        )
    }

    fn tool_continuation_text_should_finish_from_mutation_state(
        &self,
        used_mutation_tools: bool,
        next_tool_scope: Option<&ContinuationToolScope>,
    ) -> bool {
        matches!(
            TurnControlPlane::decide_tool_continuation(ToolContinuationFacts {
                turn_execution_profile: &self.turn_execution_profile,
                task_phase: self.task_contract.as_ref().map(|contract| contract.phase),
                next_tool_scope,
                used_mutation_tools,
                observation_pressure_saturated: self.observation_only_call_restriction_active(),
                mutation_tools_available: !self.side_effect_tool_definitions().is_empty(),
                artifact_write_pending: self.artifact_write_pending(),
            }),
            FinalAnswerDecision::Accept
        )
    }

    fn apply_tool_continuation_contracts(
        &self,
        decision: &Decision,
        continuation: ActionContinuation,
        next_tool_scope: Option<ContinuationToolScope>,
    ) -> ActionContinuation {
        let turn_commitment = tool_continuation_turn_commitment(decision, next_tool_scope.as_ref());
        let artifact_write_target = tool_continuation_artifact_write_target(
            self.requested_artifact_target.as_deref(),
            next_tool_scope.as_ref(),
        );
        let continuation = match next_tool_scope {
            Some(scope) => continuation.with_tool_scope(scope),
            None => continuation,
        };
        let continuation = match artifact_write_target {
            Some(path) => continuation.with_artifact_write_target(path),
            None => continuation,
        };
        match turn_commitment {
            Some(commitment) => continuation.with_turn_commitment(commitment),
            None => continuation,
        }
    }

    fn direct_inspection_empty_summary_action_result(
        &self,
        decision: &Decision,
        tool_results: Vec<ToolResult>,
        tokens_used: TokenUsage,
    ) -> ActionResult {
        let response = DIRECT_INSPECTION_EMPTY_SUMMARY_RESPONSE.to_string();
        ActionResult {
            decision: decision.clone(),
            tool_results,
            response_text: response.clone(),
            tokens_used,
            next_step: ActionNextStep::Finish(ActionTerminal::Complete { response }),
        }
    }

    fn mutation_guard_terminal_action(
        &mut self,
        decision: &Decision,
        state: ToolRoundState,
        terminal: MutationGuardTerminal,
    ) -> ActionResult {
        let partial_response = summarize_tool_progress(&state.all_tool_results);
        self.emit_signal(
            LoopStep::Act,
            SignalKind::Blocked,
            &terminal.reason,
            serde_json::json!({
                "decision_kind": ControlPlaneDecisionKind::MutationGuardrail,
                "decision": "terminal",
                "guardrail": "mutation_family_terminal",
                "family": terminal.family,
                "failure_class": terminal.failure_class.map(FailureClass::as_str),
            }),
        );
        self.incomplete_action_result(
            decision,
            state.all_tool_results,
            partial_response,
            &terminal.reason,
            state.tokens_used,
        )
    }

    fn incomplete_action_result(
        &self,
        decision: &Decision,
        tool_results: Vec<ToolResult>,
        partial_response: Option<String>,
        reason: &str,
        tokens_used: TokenUsage,
    ) -> ActionResult {
        ActionResult {
            decision: decision.clone(),
            tool_results,
            response_text: String::new(),
            tokens_used,
            next_step: ActionNextStep::Finish(ActionTerminal::Incomplete {
                partial_response,
                reason: reason.to_string(),
            }),
        }
    }

    fn bounded_local_terminal_action_result(
        &mut self,
        decision: &Decision,
        tool_results: Vec<ToolResult>,
        partial_response: Option<String>,
        tokens_used: TokenUsage,
        reason: BoundedLocalTerminalReason,
    ) -> ActionResult {
        let reason_text = super::bounded_local_terminal_reason_text(reason);
        self.emit_signal(
            LoopStep::Act,
            SignalKind::Blocked,
            reason_text,
            serde_json::json!({
                "profile": "bounded_local",
                "terminal_reason": super::bounded_local_terminal_reason_label(reason),
            }),
        );
        self.incomplete_action_result(
            decision,
            tool_results,
            partial_response,
            reason_text,
            tokens_used,
        )
    }

    #[cfg(test)]
    pub(super) async fn generate_tool_summary(
        &mut self,
        synthesis_prompt: &str,
        llm: &dyn LlmProvider,
        stream: CycleStream<'_>,
        text_visibility: TextStreamVisibility,
    ) -> Result<String, LoopError> {
        let response = self
            .generate_tool_summary_response(synthesis_prompt, llm, stream, text_visibility)
            .await?;
        Ok(response_text_segment(&response).unwrap_or_default())
    }

    async fn generate_tool_summary_response(
        &mut self,
        synthesis_prompt: &str,
        llm: &dyn LlmProvider,
        stream: CycleStream<'_>,
        text_visibility: TextStreamVisibility,
    ) -> Result<CompletionResponse, LoopError> {
        let messages = vec![Message::user(synthesis_prompt.to_string())];
        let request = CompletionRequest {
            model: llm.model_name().to_string(),
            messages: messages.clone(),
            tools: Vec::new(),
            temperature: None,
            max_tokens: Some(TOOL_SYNTHESIS_MAX_OUTPUT_TOKENS),
            system_prompt: Some(synthesis_prompt.to_string()),
            prompt_cache: Default::default(),
            cache_affinity: None,
            thinking: None,
        };
        let response = self
            .request_completion(
                llm,
                request,
                StreamingRequestContext::new(
                    LoopStep::Act,
                    StreamPhase::Synthesize,
                    text_visibility,
                ),
                stream,
            )
            .await?;
        self.continue_truncated_response(response, &messages, llm, LoopStep::Act, stream)
            .await
    }

    #[cfg(test)]
    pub(super) fn apply_fan_out_cap(
        &mut self,
        calls: &[ToolCall],
    ) -> (Vec<ToolCall>, Vec<ToolCall>) {
        self.apply_fan_out_cap_owned(calls.to_vec())
    }

    fn apply_fan_out_cap_owned(
        &mut self,
        mut calls: Vec<ToolCall>,
    ) -> (Vec<ToolCall>, Vec<ToolCall>) {
        let max_fan_out = self.budget.config().max_fan_out;
        if calls.len() <= max_fan_out {
            return (calls, Vec::new());
        }

        let total = calls.len();
        let deferred = calls.split_off(max_fan_out);
        let execute = calls;
        let deferred_names: Vec<&str> = deferred.iter().map(|call| call.name.as_str()).collect();
        self.emit_signal(
            LoopStep::Act,
            SignalKind::Friction,
            format!(
                "fan-out cap: executing {}/{}, deferring: {}",
                max_fan_out,
                total,
                deferred_names.join(", ")
            ),
            serde_json::json!({
                "decision_kind": ControlPlaneDecisionKind::ToolBatchGuardrail,
                "decision": "deferred",
                "guardrail": "fan_out_cap",
                "executed": max_fan_out,
                "total": total,
                "deferred_tools": deferred_names,
            }),
        );
        (execute, deferred)
    }

    fn stage_tool_calls_for_round(&mut self, state: &mut ToolRoundState, calls: Vec<ToolCall>) {
        let (deduped, blocked_duplicates) =
            partition_by_same_batch_mutation_guard_policy(&calls, self.tool_executor.as_ref());
        self.record_preblocked_tool_calls(state, &blocked_duplicates);

        let total_calls = deduped.len();
        let (capped, fan_out_deferred) = self.apply_fan_out_cap_owned(deduped);
        if !fan_out_deferred.is_empty() {
            self.append_deferred_tool_results(state, &fan_out_deferred, total_calls);
        }

        let serialized = self.apply_mutation_serialization_policy(capped);
        state.current_calls = serialized.execute;
        if !serialized.deferred.is_empty() {
            let message = serialized
                .deferred_message
                .expect("serialized mutation deferrals require a notice");
            self.append_deferred_tool_results_with_message(
                state,
                &serialized.deferred,
                message.clone(),
            );
            state.pending_round_notices.push(message);
        }
    }

    fn record_preblocked_tool_calls(
        &mut self,
        state: &mut ToolRoundState,
        blocked: &[BlockedToolCall],
    ) {
        if blocked.is_empty() {
            return;
        }

        self.emit_blocked_tool_errors(blocked, CycleStream::disabled());
        let blocked_calls = blocked
            .iter()
            .map(|blocked_call| blocked_call.call.clone())
            .collect::<Vec<_>>();
        let blocked_results = build_blocked_tool_results(blocked);
        // These blocked calls/results are synthesized from the same source in lockstep.
        // A mismatch here indicates an internal bug, not an expected blocked-tool outcome.
        record_tool_round_messages(
            &mut state.continuation_messages,
            &mut state.evidence_messages,
            &blocked_calls,
            &self.tool_call_provider_ids,
            &blocked_results,
        )
        .expect("blocked tool calls/results should always align");
        self.append_retry_block_guidance(state, blocked);
        state.all_tool_results.extend(blocked_results);
    }

    fn apply_mutation_serialization_policy(
        &mut self,
        calls: Vec<ToolCall>,
    ) -> SerializedToolRoundCalls {
        if calls.len() <= 1 {
            return SerializedToolRoundCalls {
                execute: calls,
                deferred: Vec::new(),
                deferred_message: None,
            };
        }

        let mut first_mutation_index = None;
        let mut has_trailing_observations = false;
        for (index, call) in calls.iter().enumerate() {
            match self.tool_executor.classify_call(call) {
                ToolCallClassification::Observation => {
                    if first_mutation_index.is_some() {
                        has_trailing_observations = true;
                    }
                }
                ToolCallClassification::Orchestration => {}
                ToolCallClassification::Mutation => {
                    first_mutation_index.get_or_insert(index);
                }
            }
        }
        let Some(first_mutation_index) = first_mutation_index else {
            return SerializedToolRoundCalls {
                execute: calls,
                deferred: Vec::new(),
                deferred_message: None,
            };
        };

        if first_mutation_index > 0 {
            let mut execute = calls;
            let deferred = execute.split_off(first_mutation_index);
            let deferred_names: Vec<String> =
                deferred.iter().map(|call| call.name.clone()).collect();
            let deferred_names_csv = deferred_names.join(", ");
            self.emit_signal(
                LoopStep::Act,
                SignalKind::Friction,
                format!(
                    "mutation barrier: ran {} leading observation call(s), deferred remaining call(s): {}",
                    execute.len(),
                    deferred_names_csv
                ),
                serde_json::json!({
                    "decision_kind": ControlPlaneDecisionKind::MutationGuardrail,
                    "decision": "deferred",
                    "guardrail": "mutation_barrier",
                    "executed_observations": execute.len(),
                    "deferred_calls": deferred_names.clone(),
                }),
            );
            return SerializedToolRoundCalls {
                execute,
                deferred,
                deferred_message: Some(format!(
                    "Calls from the first mutation onward were deferred until after you review the observation results: {}. Re-request in your next turn if still needed.",
                    deferred_names.join(", ")
                )),
            };
        }

        let total_calls = calls.len();
        let mut execute = calls;
        let deferred = execute.split_off(1);
        if deferred.is_empty() {
            return SerializedToolRoundCalls {
                execute,
                deferred,
                deferred_message: None,
            };
        }

        let deferred_names: Vec<String> = deferred.iter().map(|call| call.name.clone()).collect();
        let deferred_names_csv = deferred_names.join(", ");
        self.emit_signal(
            LoopStep::Act,
            SignalKind::Friction,
            format!(
                "mutation serialization: executing first mutation, deferring following call(s): {}",
                deferred_names_csv
            ),
            serde_json::json!({
                "decision_kind": ControlPlaneDecisionKind::MutationGuardrail,
                "decision": "deferred",
                "guardrail": "mutation_serialization",
                "executed_mutations": 1,
                "total_calls": total_calls,
                "deferred_calls": deferred_names.clone(),
            }),
        );
        SerializedToolRoundCalls {
            execute,
            deferred,
            deferred_message: Some(if has_trailing_observations {
                format!(
                    "Calls after the first mutation were deferred until after you review the latest mutation result: {}. Re-request in your next turn if still needed.",
                    deferred_names.join(", ")
                )
            } else {
                format!(
                    "Mutation-capable tool calls are executed one at a time. Deferred until after you review the latest mutation result: {}. Re-request in your next turn if still needed.",
                    deferred_names.join(", ")
                )
            }),
        }
    }

    fn append_deferred_tool_results(
        &self,
        state: &mut ToolRoundState,
        deferred: &[ToolCall],
        total: usize,
    ) {
        let executed = total.saturating_sub(deferred.len());
        let names: Vec<&str> = deferred.iter().map(|call| call.name.as_str()).collect();
        let message = format!(
            "Tool calls deferred (budget: {executed}/{total}): {}. \
             Re-request in your next turn if still needed.",
            names.join(", ")
        );
        self.append_deferred_tool_results_with_message(state, deferred, message);
    }

    fn append_deferred_tool_results_with_message(
        &self,
        state: &mut ToolRoundState,
        deferred: &[ToolCall],
        message: String,
    ) {
        for call in deferred {
            state.pending_policy_deferred_calls.push(call.clone());
            state.all_tool_results.push(ToolResult {
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                success: false,
                output: message.clone(),
                failure_class: Some(FailureClass::PolicyDeferred),
            });
        }
    }

    pub(super) fn budget_low_blocked_result(
        &mut self,
        decision: &Decision,
        action_name: &str,
        accumulated_text: &[String],
    ) -> ActionResult {
        self.emit_signal(
            LoopStep::Act,
            SignalKind::Blocked,
            format!("{action_name} blocked: budget is low, wrapping up"),
            serde_json::json!({
                "decision_kind": ControlPlaneDecisionKind::BudgetGuardrail,
                "decision": "blocked",
                "reason": "budget_soft_ceiling",
            }),
        );
        let response = stitch_response_segments(
            accumulated_text,
            Some(format!(
                "{action_name} was not executed because the budget soft-ceiling was reached. Summarizing what has been accomplished so far."
            )),
        );
        self.text_action_result(decision, &response)
    }

    pub(super) fn record_continuation_cost(
        &mut self,
        response: &CompletionResponse,
        context_messages: &[Message],
    ) {
        let cost = continuation_budget_cost(response, context_messages);
        self.budget.record(&cost);
    }

    async fn compact_tool_continuation(
        &mut self,
        round: u32,
        messages: &mut Vec<Message>,
    ) -> Result<(), LoopError> {
        let before = ContextWindowStats::capture(self, messages);
        let compacted = {
            let compaction = self.compaction();
            compaction
                .compact_if_needed(messages, CompactionScope::ToolContinuation, round)
                .await?
        };
        if let Cow::Owned(compacted_messages) = compacted {
            self.emit_context_overflow_signal(
                CompactionScope::ToolContinuation,
                before,
                &compacted_messages,
            );
            *messages = compacted_messages;
        }
        self.compaction()
            .ensure_within_hard_limit(CompactionScope::ToolContinuation, messages)
    }

    fn emit_budget_low_break_signal(&mut self, round: u32) {
        self.emit_signal(
            LoopStep::Act,
            SignalKind::Blocked,
            format!("budget soft-ceiling reached during tool round {round}, breaking loop"),
            serde_json::json!({
                "decision_kind": ControlPlaneDecisionKind::BudgetGuardrail,
                "decision": "blocked",
                "reason": "budget_soft_ceiling",
                "round": round,
            }),
        );
    }

    pub(super) async fn execute_tool_round(
        &mut self,
        round: u32,
        llm: &dyn LlmProvider,
        state: &mut ToolRoundState,
        continuation_tools: Vec<ToolDefinition>,
        stream: CycleStream<'_>,
    ) -> Result<ToolRoundOutcome, LoopError> {
        if let Some(outcome) = self
            .maybe_handle_observation_only_round(round, state, stream)
            .await?
        {
            return Ok(outcome);
        }

        let executed = self.run_tool_round_calls(round, state, stream).await?;
        let mutation_guard_terminal = mutation_guard_terminal_for_round(
            &self.tool_retry_tracker,
            &executed.calls,
            &executed.results,
            &executed.blocked,
            self.tool_executor.as_ref(),
        );
        let request = ToolRoundContinuationRequest {
            round,
            llm,
            continuation_tools,
            calls_count: executed.calls.len(),
            started_at_ms: executed.started_at_ms,
            stream,
        };
        let progress_entries = self.record_executed_tool_round(state, &executed)?;
        self.publish_tool_round(
            &executed.calls,
            &executed.results,
            &progress_entries,
            stream,
        );
        self.emit_tool_errors(&executed.results, stream);
        if duplicate_success_terminal_round(&executed) {
            return Ok(ToolRoundOutcome::DuplicateSuccessTerminal);
        }
        if let Some(outcome) = self.repeated_tool_failure_outcome_after_round(state) {
            return Ok(outcome);
        }
        if let Some(terminal) = mutation_guard_terminal {
            return Ok(ToolRoundOutcome::MutationGuardTerminal(terminal));
        }
        if let Some(outcome) = self.round_terminal_outcome(state, stream) {
            return Ok(outcome);
        }
        if let Some(outcome) = self
            .prepare_round_continuation(round, state, stream)
            .await?
        {
            return Ok(outcome);
        }

        self.request_tool_round_response(state, request).await
    }

    fn repeated_tool_failure_outcome_after_round(
        &mut self,
        state: &mut ToolRoundState,
    ) -> Option<ToolRoundOutcome> {
        let event = self.repeated_tool_failure_tracker.observe_action(
            &state.latest_tool_results,
            self.repeated_failure_streak_limit(),
        )?;

        match event {
            RepeatedToolFailureEvent::InjectGuidance(failure_state) => {
                let directive = render_repeated_tool_failure_directive(&failure_state);
                self.emit_signal(
                    LoopStep::Act,
                    SignalKind::Friction,
                    format!(
                        "injecting repeated failure guidance for '{}'",
                        failure_state.failure.key.tool_name
                    ),
                    serde_json::json!({
                        "tool": failure_state.failure.key.tool_name.as_str(),
                        "consecutive_failures": failure_state.consecutive_failures,
                        "failure_summary": failure_state.failure.summary.as_str(),
                    }),
                );
                state.continuation_messages.push(Message::system(directive));
                None
            }
            RepeatedToolFailureEvent::Trip(failure_state) => {
                Some(ToolRoundOutcome::RepeatedFailureTerminal(failure_state))
            }
        }
    }

    async fn maybe_handle_observation_only_round(
        &mut self,
        round: u32,
        state: &mut ToolRoundState,
        stream: CycleStream<'_>,
    ) -> Result<Option<ToolRoundOutcome>, LoopError> {
        if !self
            .turn_execution_profile
            .uses_standard_observation_controls()
            || !self.observation_only_call_restriction_active()
            || !calls_are_all_classification(
                &state.current_calls,
                self.tool_executor.as_ref(),
                ToolCallClassification::Observation,
            )
        {
            return Ok(None);
        }

        let blocked =
            build_uniform_blocked_calls(&state.current_calls, OBSERVATION_ONLY_CALL_BLOCK_REASON);
        self.emit_blocked_tool_errors(&blocked, stream);
        let blocked_results = build_blocked_tool_results(&blocked);
        state.used_observation_tools = true;
        record_tool_round_messages(
            &mut state.continuation_messages,
            &mut state.evidence_messages,
            &state.current_calls,
            &self.tool_call_provider_ids,
            &blocked_results,
        )?;
        state.all_tool_results.extend(blocked_results);
        self.emit_observation_only_block_signal(round, &state.current_calls);
        if !state.observation_replan_attempted {
            state.observation_replan_attempted = true;
            state
                .continuation_messages
                .push(Message::system(OBSERVATION_ONLY_MUTATION_REPLAN_DIRECTIVE));
            self.compact_tool_continuation(round, &mut state.continuation_messages)
                .await?;
            self.last_reasoning_messages = state.continuation_messages.clone();
            return Ok(Some(ToolRoundOutcome::ObservationRestrictedReplan));
        }
        Ok(Some(ToolRoundOutcome::ObservationRestricted))
    }

    fn emit_observation_only_block_signal(&mut self, round: u32, calls: &[ToolCall]) {
        self.emit_signal(
            LoopStep::Act,
            SignalKind::Blocked,
            "observation-only rounds forced to wrap up",
            serde_json::json!({
                "decision_kind": ControlPlaneDecisionKind::ToolRoundGuardrail,
                "decision": "blocked",
                "guardrail": "observation_only_rounds",
                "round": round,
                "blocked_calls": calls.iter().map(|call| call.name.as_str()).collect::<Vec<_>>(),
            }),
        );
    }

    async fn run_tool_round_calls(
        &mut self,
        round: u32,
        state: &ToolRoundState,
        stream: CycleStream<'_>,
    ) -> Result<ExecutedToolRound, LoopError> {
        let started_at_ms = current_time_ms();
        let calls = state.current_calls.clone();
        self.maybe_publish_tool_round_progress(round as usize, &calls, stream);
        let batch = self
            .execute_tool_calls_batch_with_stream(&calls, stream)
            .await?;
        let has_tool_errors = batch.results.iter().any(|result| !result.success);
        Ok(ExecutedToolRound {
            calls,
            results: batch.results,
            blocked: batch.blocked,
            has_tool_errors,
            started_at_ms,
        })
    }

    fn record_executed_tool_round(
        &mut self,
        state: &mut ToolRoundState,
        executed: &ExecutedToolRound,
    ) -> Result<Vec<ToolProgressEntry>, LoopError> {
        let ExecutedToolRound {
            calls,
            results,
            blocked,
            has_tool_errors,
            ..
        } = executed;
        self.record_successful_tool_classifications(state, calls, results);
        self.record_root_turn_contract_progress(calls, results);
        self.record_task_contract_progress(calls, results);
        self.record_tool_round_result_bytes(results);
        self.record_round_messages(state, calls, results, blocked, *has_tool_errors)?;
        state.current_calls.clear();
        state.latest_tool_results = results.clone();
        resolve_fulfilled_policy_deferred_calls(state, calls, results);
        let progress_entries = self.record_tool_round_progress(calls, results);
        self.advance_bounded_local_phase_after_tool_round(calls, results);
        self.update_preflight_route_after_tool_round(state, calls, results);
        state.all_tool_results.extend(results.iter().cloned());
        Ok(progress_entries)
    }

    fn record_tool_round_result_bytes(&mut self, results: &[ToolResult]) {
        let round_result_bytes: usize = results.iter().map(|result| result.output.len()).sum();
        self.budget.record_result_bytes(round_result_bytes);
    }

    fn record_round_messages(
        &self,
        state: &mut ToolRoundState,
        calls: &[ToolCall],
        results: &[ToolResult],
        blocked: &[BlockedToolCall],
        has_tool_errors: bool,
    ) -> Result<(), LoopError> {
        record_tool_round_messages(
            &mut state.continuation_messages,
            &mut state.evidence_messages,
            calls,
            &self.tool_call_provider_ids,
            results,
        )?;
        self.append_pending_round_notices(state);
        self.append_retry_block_guidance(state, blocked);
        if has_tool_errors {
            self.append_tool_error_relay(state, results);
        }
        Ok(())
    }

    fn update_preflight_route_after_tool_round(
        &mut self,
        state: &mut ToolRoundState,
        calls: &[ToolCall],
        results: &[ToolResult],
    ) {
        if calls.is_empty() {
            return;
        }

        enum RouteResolution {
            Clear(&'static str),
            Reroute {
                resource: serde_json::Value,
                failure_class: crate::act::FailureClass,
                failed_route: super::preflight_route::PlannedRoute,
                failed_tools: Vec<String>,
                next_route: super::preflight_route::PlannedRoute,
            },
            Exhausted {
                resource: serde_json::Value,
                failure_class: crate::act::FailureClass,
                failed_route: super::preflight_route::PlannedRoute,
                failed_tools: Vec<String>,
            },
        }

        let resolution = {
            let Some(route_plan) = self.preflight_route_plan.as_mut() else {
                return;
            };
            let current_route = route_plan.current_route().clone();
            let allowed_tools: HashSet<&str> = current_route
                .tool_names
                .iter()
                .map(String::as_str)
                .collect();
            let route_results = results
                .iter()
                .filter(|result| allowed_tools.contains(result.tool_name.as_str()))
                .collect::<Vec<_>>();
            if route_results.is_empty() {
                return;
            }
            if route_results.iter().any(|result| result.success) {
                RouteResolution::Clear("route_succeeded")
            } else {
                let Some(failure_class) = preflight_route_failure_class(&route_results) else {
                    return self
                        .consume_preflight_route_plan("route_failed_without_reroute_signal");
                };
                if !failure_class.is_reroute_relevant() {
                    return self.consume_preflight_route_plan("route_failed_without_reroute_class");
                }
                let failed_tools = route_results
                    .iter()
                    .map(|result| result.tool_name.clone())
                    .collect::<Vec<_>>();
                let resource = serde_json::to_value(&route_plan.resource)
                    .unwrap_or_else(|_| serde_json::json!({"kind": "unknown"}));
                if let Some(next_route) = route_plan.advance_to_reroute(failure_class) {
                    RouteResolution::Reroute {
                        resource,
                        failure_class,
                        failed_route: current_route,
                        failed_tools,
                        next_route,
                    }
                } else {
                    RouteResolution::Exhausted {
                        resource,
                        failure_class,
                        failed_route: current_route,
                        failed_tools,
                    }
                }
            }
        };

        match resolution {
            RouteResolution::Clear(reason) => self.consume_preflight_route_plan(reason),
            RouteResolution::Reroute {
                resource,
                failure_class,
                failed_route,
                failed_tools,
                next_route,
            } => {
                self.emit_signal(
                    LoopStep::Act,
                    SignalKind::Trace,
                    "rerouted preflight external resource route",
                    serde_json::json!({
                        "decision_kind": ControlPlaneDecisionKind::PreflightRoute,
                        "decision": "rerouted",
                        "resource": resource,
                        "failure_class": failure_class.as_str(),
                        "failed_route": failed_route,
                        "failed_tools": failed_tools,
                        "next_route": next_route,
                    }),
                );
                state.continuation_messages.push(Message::system(
                    preflight_route_reroute_directive(&failed_route, &next_route, failure_class),
                ));
            }
            RouteResolution::Exhausted {
                resource,
                failure_class,
                failed_route,
                failed_tools,
            } => {
                self.emit_signal(
                    LoopStep::Act,
                    SignalKind::Trace,
                    "preflight external resource route exhausted",
                    serde_json::json!({
                        "decision_kind": ControlPlaneDecisionKind::PreflightRoute,
                        "decision": "exhausted",
                        "resource": resource,
                        "failure_class": failure_class.as_str(),
                        "failed_route": failed_route,
                        "failed_tools": failed_tools,
                    }),
                );
                self.consume_preflight_route_plan("route_chain_exhausted");
            }
        }
    }

    fn append_tool_error_relay(&self, state: &mut ToolRoundState, results: &[ToolResult]) {
        let failed: Vec<(&str, &str)> = results
            .iter()
            .filter(|result| !result.success)
            .map(|result| (result.tool_name.as_str(), result.output.as_str()))
            .collect();
        state
            .continuation_messages
            .push(Message::system(tool_error_relay_directive(&failed)));
    }

    fn append_retry_block_guidance(&self, state: &mut ToolRoundState, blocked: &[BlockedToolCall]) {
        let mut directives = blocked
            .iter()
            .filter_map(render_block_directive)
            .collect::<Vec<_>>();
        directives.sort();
        directives.dedup();
        if directives.is_empty() {
            return;
        }

        let body = directives
            .into_iter()
            .map(|directive| format!("- {directive}"))
            .collect::<Vec<_>>()
            .join("\n");
        state
            .continuation_messages
            .push(Message::system(format!("Tool guardrails:\n{body}")));
    }

    fn append_pending_round_notices(&self, state: &mut ToolRoundState) {
        for notice in state.pending_round_notices.drain(..) {
            let notice = Message::system(notice);
            state.continuation_messages.push(notice.clone());
            state.evidence_messages.push(notice);
        }
    }

    fn round_terminal_outcome(
        &mut self,
        state: &ToolRoundState,
        stream: CycleStream<'_>,
    ) -> Option<ToolRoundOutcome> {
        if let Some(reason) = self.bounded_local_terminal_reason.take() {
            self.last_reasoning_messages = state.continuation_messages.clone();
            self.expire_activity_progress(stream);
            return Some(ToolRoundOutcome::BoundedLocalTerminal(reason));
        }
        if let TurnExecutionProfile::DirectUtility(profile) = &self.turn_execution_profile {
            let response = super::direct_utility::direct_utility_terminal_response(
                profile,
                &state.all_tool_results,
            );
            self.last_reasoning_messages = state.continuation_messages.clone();
            self.expire_activity_progress(stream);
            return Some(ToolRoundOutcome::ProfileAnswered(response));
        }
        if let TurnExecutionProfile::DeterministicLocal(plan) = &self.turn_execution_profile {
            let response = plan.terminal_response(&state.all_tool_results);
            self.last_reasoning_messages = state.continuation_messages.clone();
            self.expire_activity_progress(stream);
            return Some(ToolRoundOutcome::ProfileAnswered(response));
        }
        None
    }

    async fn prepare_round_continuation(
        &mut self,
        round: u32,
        state: &mut ToolRoundState,
        stream: CycleStream<'_>,
    ) -> Result<Option<ToolRoundOutcome>, LoopError> {
        self.compact_tool_continuation(round, &mut state.continuation_messages)
            .await?;
        self.last_reasoning_messages = state.continuation_messages.clone();
        self.expire_activity_progress(stream);
        if self.cancellation_token_triggered() {
            return Ok(Some(ToolRoundOutcome::Cancelled));
        }
        if self.budget.state() == BudgetState::Low {
            self.emit_budget_low_break_signal(round);
            return Ok(Some(ToolRoundOutcome::BudgetLow(
                ToolLoopExhaustionReason::BudgetLow,
            )));
        }
        if self
            .budget
            .check(&continuation_budget_cost_estimate(
                &state.continuation_messages,
            ))
            .is_err()
        {
            self.emit_signal(
                LoopStep::Act,
                SignalKind::Blocked,
                format!("budget exhausted before tool continuation round {round}"),
                serde_json::json!({
                    "decision_kind": ControlPlaneDecisionKind::BudgetGuardrail,
                    "decision": "blocked",
                    "reason": "budget_exhausted",
                    "round": round,
                }),
            );
            return Ok(Some(ToolRoundOutcome::BudgetLow(
                ToolLoopExhaustionReason::ContinuationBudgetExhausted,
            )));
        }
        Ok(None)
    }

    async fn request_tool_round_response(
        &mut self,
        state: &mut ToolRoundState,
        request: ToolRoundContinuationRequest<'_>,
    ) -> Result<ToolRoundOutcome, LoopError> {
        let pending_subagents = pending_subagent_ids(&state.all_tool_results);
        let continuation_tools = if !pending_subagents.is_empty() {
            tools_named(
                self.tool_executor.tool_definitions(),
                &[SUBAGENT_STATUS_TOOL_NAME],
            )
        } else if self.task_contract_blocks_tools() {
            Vec::new()
        } else {
            request.continuation_tools
        };
        self.emit_loop_phase(request.stream, Phase::Synthesize);
        let response = self
            .request_tool_continuation(
                request.llm,
                &state.continuation_messages,
                continuation_tools,
                &mut state.tokens_used,
                request.stream,
            )
            .await?;
        self.record_continuation_cost(&response, &state.continuation_messages);
        self.emit_tool_round_trace_and_perf(
            request.round,
            request.calls_count,
            &response,
            current_time_ms().saturating_sub(request.started_at_ms),
        );
        if self.cancellation_token_triggered() {
            return Ok(ToolRoundOutcome::Cancelled);
        }
        Ok(ToolRoundOutcome::Response(response))
    }

    pub(super) fn apply_tool_round_progress_policy(
        &self,
        round: u32,
        continuation_messages: &mut Vec<Message>,
    ) -> Vec<ToolDefinition> {
        if self.task_contract_blocks_tools() {
            return Vec::new();
        }

        let termination = self.current_termination_config();
        let config = termination.as_ref();
        let tool_nudge = u32::from(config.tool_round_nudge_after);
        let tool_stripping =
            ToolStrippingAfterNudge::from_config_value(config.tool_round_strip_after_nudge);
        let observation_nudge = u32::from(config.observation_only_round_nudge_after);
        let observation_stripping = ToolStrippingAfterNudge::from_config_value(
            config.observation_only_round_strip_after_nudge,
        );
        let observation_rounds = u32::from(self.observation_round_tracker.pressure_rounds());
        let all_tools = self.tool_executor.tool_definitions();
        let profile_owns_surface = self.turn_execution_profile.owns_tool_surface();

        if !profile_owns_surface && observation_nudge > 0 && observation_rounds == observation_nudge
        {
            continuation_messages.push(Message::system(
                OBSERVATION_ONLY_TOOL_ROUND_NUDGE.to_string(),
            ));
        }
        if tool_nudge > 0 && round == tool_nudge {
            continuation_messages.push(Message::system(TOOL_ROUND_PROGRESS_NUDGE.to_string()));
        }
        if !profile_owns_surface
            && observation_nudge > 0
            && observation_stripping.should_strip(
                config.observation_only_round_nudge_after,
                observation_rounds,
            )
        {
            return self.side_effect_tool_definitions();
        }
        if tool_nudge > 0 && tool_stripping.should_strip(config.tool_round_nudge_after, round) {
            self.progress_limited_tool_definitions()
        } else {
            all_tools
        }
    }

    pub(super) fn continuation_tool_scope_for_round(
        &self,
        state: &ToolRoundState,
    ) -> Option<ContinuationToolScope> {
        match TurnControlPlane::decide_observation_follow_up(ObservationFollowUpFacts {
            turn_execution_profile: &self.turn_execution_profile,
            used_observation_tools: state.used_observation_tools,
            used_mutation_tools: state.used_mutation_tools,
            observation_pressure_saturated: self.observation_only_call_restriction_active(),
            mutation_tools_available: !self.side_effect_tool_definitions().is_empty(),
            artifact_write_pending: self.artifact_write_pending(),
        }) {
            ObservationFollowUpDecision::RequireMutationOnly => {
                Some(ContinuationToolScope::MutationOnly)
            }
            ObservationFollowUpDecision::Continue | ObservationFollowUpDecision::AllowSynthesis => {
                None
            }
        }
    }

    fn observation_only_call_restriction_active(&self) -> bool {
        let termination = self.current_termination_config();
        let config = termination.as_ref();
        let nudge_threshold = u32::from(config.observation_only_round_nudge_after);
        nudge_threshold > 0
            && ToolStrippingAfterNudge::from_config_value(
                config.observation_only_round_strip_after_nudge,
            )
            .should_strip(
                config.observation_only_round_nudge_after,
                u32::from(self.observation_round_tracker.pressure_rounds()),
            )
    }

    fn artifact_write_pending(&self) -> bool {
        self.pending_artifact_write_target.is_some() || self.requested_artifact_target.is_some()
    }

    #[cfg(test)]
    pub(super) fn record_tool_round_kind(&mut self, calls: &[ToolCall]) {
        let _ = self.record_tool_round_progress(calls, &[]);
    }

    pub(super) fn record_tool_round_progress(
        &mut self,
        calls: &[ToolCall],
        results: &[ToolResult],
    ) -> Vec<ToolProgressEntry> {
        let start_index = self.turn_progress_ledger.tool_entries.len();
        let results_by_call_id: HashMap<&str, &ToolResult> = results
            .iter()
            .map(|result| (result.tool_call_id.as_str(), result))
            .collect();
        let mut saw_observation_call = false;
        let mut saw_mutation_call = false;
        let mut saw_new_fingerprint = false;
        let mut saw_productive_observation = false;
        let mut saw_unproductive_observation = false;
        self.turn_progress_ledger.unproductive_observation_rounds = self
            .turn_progress_ledger
            .unproductive_observation_rounds
            .max(self.observation_round_tracker.repetitive_rounds);

        for call in calls {
            match self.tool_executor.classify_call(call) {
                ToolCallClassification::Observation => {
                    saw_observation_call = true;
                    let fingerprint = observation_tool_fingerprint(call);
                    let seen_before = self
                        .observation_round_tracker
                        .record_observation_fingerprint(fingerprint.clone());
                    saw_new_fingerprint |= !seen_before;
                    let outcome = self.record_observation_tool_progress(
                        call,
                        results_by_call_id.get(call.id.as_str()).copied(),
                        fingerprint,
                        seen_before,
                    );
                    match outcome {
                        ToolProgressOutcome::Advanced | ToolProgressOutcome::RetryableFailure => {
                            saw_productive_observation = true;
                        }
                        ToolProgressOutcome::Duplicate => {
                            saw_unproductive_observation = true;
                        }
                    }
                }
                ToolCallClassification::Orchestration => {}
                ToolCallClassification::Mutation => {
                    saw_mutation_call = true;
                    self.record_mutation_tool_progress(
                        call,
                        results_by_call_id.get(call.id.as_str()).copied(),
                    );
                }
            }
        }

        if saw_observation_call && !saw_mutation_call {
            self.observation_round_tracker
                .consecutive_observation_only_rounds = self
                .observation_round_tracker
                .consecutive_observation_only_rounds
                .saturating_add(1);
        } else {
            self.observation_round_tracker
                .consecutive_observation_only_rounds = 0;
        }

        // The observation gate is driven by task progress, not the fact that a
        // model is still reading. Distinct successful observations and first
        // failures both count as progress because they advance evidence or
        // identify a retryable path. Repeated/ungrounded observation-only
        // rounds accumulate pressure.
        if !saw_observation_call || saw_mutation_call || saw_productive_observation {
            self.turn_progress_ledger.unproductive_observation_rounds = 0;
        } else if saw_unproductive_observation || !saw_new_fingerprint {
            self.turn_progress_ledger.unproductive_observation_rounds = self
                .turn_progress_ledger
                .unproductive_observation_rounds
                .saturating_add(1);
        } else {
            self.turn_progress_ledger.unproductive_observation_rounds = 0;
        }

        self.observation_round_tracker.repetitive_rounds =
            self.turn_progress_ledger.unproductive_observation_rounds;

        self.turn_progress_ledger.tool_entries[start_index..].to_vec()
    }

    fn record_observation_tool_progress(
        &mut self,
        call: &ToolCall,
        result: Option<&ToolResult>,
        fingerprint: String,
        seen_before: bool,
    ) -> ToolProgressOutcome {
        let observation_text = tool_progress_observation_text(call, result);
        let matched_slot_id = self
            .turn_progress_ledger
            .matching_open_evidence_slot(&observation_text);
        let semantic_scope = observation_tool_semantic_scope(call);
        let semantic_scope_slot_id = semantic_scope
            .as_deref()
            .and_then(discovered_evidence_slot_id);
        let fingerprint_slot_id = discovered_evidence_slot_id(&fingerprint);
        let progress_slot_id = matched_slot_id
            .clone()
            .or_else(|| semantic_scope_slot_id.clone())
            .or_else(|| fingerprint_slot_id.clone());
        let existing_status = self
            .turn_progress_ledger
            .evidence_slots
            .get(progress_slot_id.as_deref().unwrap_or_default())
            .map(|slot| slot.status);
        let discovered_references = result
            .filter(|result| result.success)
            .filter(|result| should_extract_progress_evidence_references(call, result))
            .map(|result| extract_progress_evidence_references(&result.output))
            .unwrap_or_default();
        let discovered_new_evidence = discovered_references
            .iter()
            .any(|reference| !self.turn_progress_ledger_has_evidence_label(reference));
        // Discovered references are not counted as current-scope progress once
        // they are already known. Repeating broad searches should pressure the
        // model to pivot into direct reads of those open follow-up slots.
        let generic_scope_saturated = matched_slot_id.is_none()
            && !self.turn_progress_ledger.has_explicit_evidence_slots()
            && !discovered_new_evidence
            && semantic_scope.as_ref().is_some_and(|scope| {
                self.turn_progress_ledger
                    .generic_observation_scope_attempts
                    .get(scope)
                    .copied()
                    .unwrap_or_default()
                    >= GENERIC_OBSERVATION_SCOPE_PRODUCTIVE_ALLOWANCE
            });
        let unmatched_after_required_evidence = matched_slot_id.is_none()
            && self.turn_progress_ledger.has_explicit_evidence_slots()
            && self
                .turn_progress_ledger
                .open_explicit_evidence_slots()
                .is_empty();
        let outcome = match result {
            Some(result) if result.success => {
                if matched_slot_id.is_some()
                    || matches!(existing_status, Some(ProgressSlotStatus::RetryableFailure))
                {
                    ToolProgressOutcome::Advanced
                } else if unmatched_after_required_evidence
                    || generic_scope_saturated
                    || seen_before
                {
                    ToolProgressOutcome::Duplicate
                } else {
                    ToolProgressOutcome::Advanced
                }
            }
            Some(result) => {
                let failure_key = tool_progress_failure_key(&fingerprint, result);
                if self
                    .turn_progress_ledger
                    .seen_retryable_failures
                    .insert(failure_key)
                {
                    ToolProgressOutcome::RetryableFailure
                } else {
                    ToolProgressOutcome::Duplicate
                }
            }
            None => {
                if seen_before {
                    ToolProgressOutcome::Duplicate
                } else {
                    ToolProgressOutcome::Advanced
                }
            }
        };

        if result.is_none_or(|result| result.success) {
            if let Some(scope) = semantic_scope.as_ref() {
                let attempts = self
                    .turn_progress_ledger
                    .generic_observation_scope_attempts
                    .entry(scope.clone())
                    .or_default();
                *attempts = attempts.saturating_add(1);
            }
        }

        if !discovered_references.is_empty() {
            self.seed_discovered_evidence_references(discovered_references);
        }

        if let Some(slot_id) = progress_slot_id.as_ref().filter(|_| {
            matches!(
                outcome,
                ToolProgressOutcome::Advanced | ToolProgressOutcome::RetryableFailure
            )
        }) {
            let label = matched_slot_id
                .as_deref()
                .and_then(|id| {
                    self.turn_progress_ledger
                        .evidence_slots
                        .get(id)
                        .map(|slot| slot.label.clone())
                })
                .unwrap_or_else(|| {
                    semantic_scope
                        .clone()
                        .unwrap_or_else(|| fingerprint.clone())
                });
            let normalized_target = normalize_contract_label(&label);
            let explicit = self.turn_progress_ledger.slot_is_explicit(slot_id);
            let result_success = result.is_some_and(|result| result.success);
            let slot = self
                .turn_progress_ledger
                .evidence_slots
                .entry(slot_id.clone())
                .or_insert_with(|| ProgressSlot {
                    id: slot_id.clone(),
                    kind: ProgressSlotKind::Evidence,
                    label,
                    normalized_target: (!normalized_target.is_empty()).then_some(normalized_target),
                    explicit,
                    status: ProgressSlotStatus::Open,
                    attempts: 0,
                    satisfied_by: Vec::new(),
                });
            slot.attempts = slot.attempts.saturating_add(1);
            match outcome {
                ToolProgressOutcome::Advanced if result_success => {
                    slot.status = ProgressSlotStatus::Satisfied;
                    if !slot.satisfied_by.iter().any(|id| id == &call.id) {
                        slot.satisfied_by.push(call.id.clone());
                    }
                }
                ToolProgressOutcome::RetryableFailure => {
                    slot.status = ProgressSlotStatus::RetryableFailure;
                }
                _ => {}
            }
        }

        let advances_slot = matches!(
            outcome,
            ToolProgressOutcome::Advanced | ToolProgressOutcome::RetryableFailure
        )
        .then_some(progress_slot_id)
        .flatten();

        self.turn_progress_ledger
            .tool_entries
            .push(ToolProgressEntry {
                call_id: call.id.clone(),
                tool_name: call.name.clone(),
                class: ToolProgressClass::Observation,
                target: tool_progress_display_target(call).or(Some(fingerprint)),
                advances_slot,
                outcome,
            });
        outcome
    }

    fn turn_progress_ledger_has_evidence_label(&self, label: &str) -> bool {
        let normalized = normalize_contract_label(label);
        if normalized.is_empty() {
            return false;
        }
        self.turn_progress_ledger
            .evidence_slots
            .contains_key(&format!("evidence:required:{normalized}"))
            || self
                .turn_progress_ledger
                .evidence_slots
                .contains_key(&format!("evidence:discovered:{normalized}"))
    }

    fn seed_discovered_evidence_references(&mut self, references: Vec<String>) {
        for reference in references {
            self.turn_progress_ledger
                .seed_discovered_evidence_slot(reference);
        }
    }

    fn record_mutation_tool_progress(&mut self, call: &ToolCall, result: Option<&ToolResult>) {
        let signature = tool_execution_signature(call);
        let target = tool_progress_display_target(call).unwrap_or_else(|| signature.clone());
        let slot_id = format!("mutation:{signature}");
        let outcome = match result {
            Some(result) if !result.success => ToolProgressOutcome::RetryableFailure,
            _ => ToolProgressOutcome::Advanced,
        };
        let slot = self
            .turn_progress_ledger
            .mutation_slots
            .entry(slot_id.clone())
            .or_insert_with(|| ProgressSlot {
                id: slot_id.clone(),
                kind: ProgressSlotKind::Mutation,
                label: target.clone(),
                normalized_target: Some(normalize_contract_label(&signature)),
                explicit: false,
                status: ProgressSlotStatus::Open,
                attempts: 0,
                satisfied_by: Vec::new(),
            });
        slot.attempts = slot.attempts.saturating_add(1);
        match outcome {
            ToolProgressOutcome::Advanced => {
                slot.status = ProgressSlotStatus::Satisfied;
                if !slot.satisfied_by.iter().any(|id| id == &call.id) {
                    slot.satisfied_by.push(call.id.clone());
                }
            }
            ToolProgressOutcome::RetryableFailure => {
                slot.status = ProgressSlotStatus::RetryableFailure;
            }
            _ => {}
        }
        self.turn_progress_ledger
            .tool_entries
            .push(ToolProgressEntry {
                call_id: call.id.clone(),
                tool_name: call.name.clone(),
                class: ToolProgressClass::Mutation,
                target: Some(target),
                advances_slot: Some(slot_id),
                outcome,
            });
    }

    pub(super) async fn request_tool_continuation(
        &mut self,
        llm: &dyn LlmProvider,
        context_messages: &[Message],
        continuation_tools: Vec<ToolDefinition>,
        tokens_used: &mut crate::act::TokenUsage,
        stream: CycleStream<'_>,
    ) -> Result<CompletionResponse, LoopError> {
        let continuation_tools = self.apply_turn_execution_profile_tool_surface(continuation_tools);
        let continuation_tools = self.apply_preflight_route_tool_surface(continuation_tools);
        let mut request = build_continuation_request(ContinuationRequestParams::new(
            context_messages,
            llm.model_name(),
            ToolRequestConfig::new(continuation_tools, self.effective_decompose_enabled()),
            self.request_build_context(),
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

        let response = self
            .request_completion(
                llm,
                request,
                StreamingRequestContext::new(
                    LoopStep::Act,
                    StreamPhase::Synthesize,
                    TextStreamVisibility::Preview,
                ),
                stream,
            )
            .await?;
        tokens_used.accumulate(super::response_usage_or_estimate(
            &response,
            context_messages,
        ));
        Ok(response)
    }
}

fn tool_round_activity_id(calls: &[ToolCall]) -> Option<String> {
    let first = calls.first()?;
    Some(format!("tool-round-{}", first.id))
}

fn tool_round_activity_title(calls: &[ToolCall]) -> Option<String> {
    if calls.is_empty() {
        return None;
    }

    Some(format!(
        "Ran {} {}",
        calls.len(),
        if calls.len() == 1 { "tool" } else { "tools" }
    ))
}

fn has_policy_deferred_tool_results(results: &[ToolResult]) -> bool {
    results
        .iter()
        .any(|result| matches!(result.failure_class, Some(FailureClass::PolicyDeferred)))
}

fn pending_subagent_ids(results: &[ToolResult]) -> Vec<String> {
    let mut known_ids = HashSet::new();
    let mut pending_ids = HashSet::new();
    let mut order = Vec::new();

    for result in results.iter().filter(|result| result.success) {
        match result.tool_name.as_str() {
            SPAWN_AGENT_TOOL_NAME => {
                let Some(handle) = parse_subagent_handle(&result.output) else {
                    continue;
                };
                if known_ids.insert(handle.id.clone()) {
                    order.push(handle.id.clone());
                }
                if handle.running {
                    pending_ids.insert(handle.id);
                }
            }
            SUBAGENT_STATUS_TOOL_NAME => {
                for handle in parse_subagent_handles(&result.output) {
                    if !known_ids.contains(&handle.id) {
                        continue;
                    }
                    if handle.running {
                        pending_ids.insert(handle.id);
                    } else {
                        pending_ids.remove(&handle.id);
                    }
                }
            }
            _ => {}
        }
    }

    order
        .into_iter()
        .filter(|id| pending_ids.contains(id))
        .collect()
}

fn subagent_wait_tool_call(index: usize, id: &str) -> ToolCall {
    ToolCall {
        id: format!("kernel-subagent-wait-{index}-{id}"),
        name: SUBAGENT_STATUS_TOOL_NAME.to_string(),
        arguments: serde_json::json!({
            "action": "wait",
            "id": id,
            "timeout_seconds": SUBAGENT_HARVEST_WAIT_TIMEOUT_SECONDS,
        }),
    }
}

struct SubagentHandleStatus {
    id: String,
    running: bool,
}

fn parse_subagent_handle(output: &str) -> Option<SubagentHandleStatus> {
    let value: serde_json::Value = serde_json::from_str(output).ok()?;
    subagent_handle_status(&value)
}

fn parse_subagent_handles(output: &str) -> Vec<SubagentHandleStatus> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(output) else {
        return Vec::new();
    };
    if let Some(handle) = subagent_handle_status(&value) {
        return vec![handle];
    }
    value
        .get("subagents")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(subagent_handle_status)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn subagent_handle_status(value: &serde_json::Value) -> Option<SubagentHandleStatus> {
    let id = value.get("id")?.as_str()?.trim();
    if id.is_empty() {
        return None;
    }
    let state = value.get("status")?.get("state")?.as_str()?;
    Some(SubagentHandleStatus {
        id: id.to_string(),
        running: state == "running",
    })
}

fn tools_named(tools: Vec<ToolDefinition>, names: &[&str]) -> Vec<ToolDefinition> {
    let allowed: HashSet<&str> = names.iter().copied().collect();
    tools
        .into_iter()
        .filter(|tool| allowed.contains(tool.name.as_str()))
        .collect()
}

fn resolve_fulfilled_policy_deferred_calls(
    state: &mut ToolRoundState,
    calls: &[ToolCall],
    results: &[ToolResult],
) {
    for result in results.iter().filter(|result| result.success) {
        let Some(call) = calls
            .iter()
            .find(|call| call.id == result.tool_call_id && call.name == result.tool_name)
        else {
            continue;
        };
        let Some(deferred_index) = state
            .pending_policy_deferred_calls
            .iter()
            .position(|deferred| tool_calls_match_for_deferred_resolution(deferred, call))
        else {
            continue;
        };
        let deferred = state.pending_policy_deferred_calls.remove(deferred_index);
        if let Some(result_index) = state.all_tool_results.iter().position(|tool_result| {
            tool_result.tool_call_id == deferred.id
                && matches!(
                    tool_result.failure_class,
                    Some(FailureClass::PolicyDeferred)
                )
        }) {
            state.all_tool_results.remove(result_index);
        }
    }
}

fn tool_calls_match_for_deferred_resolution(deferred: &ToolCall, executed: &ToolCall) -> bool {
    deferred.name == executed.name && deferred.arguments == executed.arguments
}

fn preflight_route_failure_class(results: &[&ToolResult]) -> Option<crate::act::FailureClass> {
    results
        .iter()
        .filter_map(|result| result.failure_classification())
        .filter(|class| class.is_reroute_relevant())
        .min_by_key(|class| preflight_route_failure_rank(*class))
}

fn preflight_route_failure_rank(class: crate::act::FailureClass) -> u8 {
    match class {
        crate::act::FailureClass::VisibilityMismatch => 0,
        crate::act::FailureClass::AuthRequired => 1,
        crate::act::FailureClass::UnsupportedResource => 2,
        crate::act::FailureClass::NotFound => 3,
        crate::act::FailureClass::InvalidRequest => 4,
        crate::act::FailureClass::RateLimited => 5,
        crate::act::FailureClass::Timeout => 6,
        crate::act::FailureClass::TransientTransport => 7,
        crate::act::FailureClass::Transient => 8,
        crate::act::FailureClass::Permanent | crate::act::FailureClass::Unknown => 9,
        crate::act::FailureClass::PolicyDeferred => 10,
    }
}

fn preflight_route_reroute_directive(
    failed_route: &super::preflight_route::PlannedRoute,
    next_route: &super::preflight_route::PlannedRoute,
    failure_class: crate::act::FailureClass,
) -> String {
    format!(
        concat!(
            "Kernel reroute: the current external-resource route failed with `{}` while using route family ",
            "`{:?}`. Continue the same task using only the next planned route tools now: {}."
        ),
        failure_class.as_str(),
        failed_route.family,
        next_route.tool_names.join(", ")
    )
}

fn classification_for_tool_name(
    executor: &dyn ToolExecutor,
    result: &ToolResult,
) -> ToolCallClassification {
    match executor.cacheability(&result.tool_name) {
        ToolCacheability::SideEffect => ToolCallClassification::Mutation,
        ToolCacheability::Cacheable | ToolCacheability::NeverCache => {
            ToolCallClassification::Observation
        }
    }
}

fn duplicate_success_terminal_round(executed: &ExecutedToolRound) -> bool {
    !executed.calls.is_empty()
        && executed.blocked.len() == executed.calls.len()
        && executed.blocked.iter().all(|blocked| {
            matches!(
                blocked.source,
                BlockedToolSource::Retry(RetryBlockKind::DuplicateSuccess)
            )
        })
}

fn render_block_directive(blocked_call: &BlockedToolCall) -> Option<String> {
    match blocked_call.source {
        BlockedToolSource::Retry(kind) => {
            let tool = blocked_call.call.name.as_str();
            let args = truncated_retry_arguments(&blocked_call.call.arguments);
            Some(match kind {
                RetryBlockKind::PermanentFailure => permanent_failure_retry_directive(tool, &args),
                RetryBlockKind::CycleFailureLimit => cycle_failure_limit_retry_directive(),
                RetryBlockKind::SameCallFailureLimit => same_call_retry_directive(tool, &args),
                RetryBlockKind::NoProgress => no_progress_retry_directive(tool, &args),
                RetryBlockKind::DuplicateSuccess => duplicate_success_retry_directive(tool, &args),
            })
        }
        BlockedToolSource::MutationGuard(kind) => Some(match kind {
            MutationGuardBlockKind::DuplicateVariant => {
                mutation_duplicate_variant_directive(&blocked_call.call.name)
            }
            MutationGuardBlockKind::CompletedIntent => {
                mutation_completed_intent_directive(&blocked_call.call.name)
            }
            MutationGuardBlockKind::FamilyFailure | MutationGuardBlockKind::RetryLimit => {
                mutation_failed_intent_directive(
                    &blocked_call.call.name,
                    blocked_call.failure_class,
                )
            }
        }),
        BlockedToolSource::Policy => None,
    }
}

fn truncated_retry_arguments(arguments: &serde_json::Value) -> String {
    const MAX_ARGUMENT_BYTES: usize = 160;
    const ELLIPSIS: &str = "...";

    let canonical = canonicalized_tool_arguments(arguments);
    if canonical.len() <= MAX_ARGUMENT_BYTES {
        canonical
    } else {
        let truncated = truncate_utf8_bytes(&canonical, MAX_ARGUMENT_BYTES - ELLIPSIS.len());
        format!("{truncated}{ELLIPSIS}")
    }
}

fn permanent_failure_retry_directive(tool: &str, args: &str) -> String {
    format!(
        concat!(
            "`{}` is blocked for the rest of this run with arguments {} ",
            "because the same call already failed permanently. Do not retry it unchanged; ",
            "use a different tool, change the arguments, or answer with the blocker."
        ),
        tool, args
    )
}

fn cycle_failure_limit_retry_directive() -> String {
    concat!(
        "This cycle hit the tool failure budget. Stop broad retry loops; only call another tool ",
        "if it is materially different from the blocked attempts, otherwise answer from the ",
        "current evidence or report the blocker."
    )
    .to_string()
}

fn same_call_retry_directive(tool: &str, args: &str) -> String {
    format!(
        concat!(
            "Do not call `{}` again with the same arguments {}; that exact call has ",
            "already failed repeatedly in this run. Change the arguments, use a different tool, ",
            "or answer with the blocker."
        ),
        tool, args
    )
}

fn no_progress_retry_directive(tool: &str, args: &str) -> String {
    format!(
        concat!(
            "Do not call `{}` again with the same arguments {}; that exact call already ",
            "returned the same result repeatedly in this run. Reuse the current evidence, change ",
            "the arguments, or use a different tool."
        ),
        tool, args
    )
}

fn duplicate_success_retry_directive(tool: &str, args: &str) -> String {
    format!(
        concat!(
            "Do not call `{}` again with the same arguments {}; that exact call already ",
            "succeeded in this turn. Use the existing result, make a materially different call, ",
            "or continue from the evidence already gathered."
        ),
        tool, args
    )
}

fn mutation_duplicate_variant_directive(tool: &str) -> String {
    format!(
        concat!(
            "Do not queue another near-equivalent deterministic mutation with `{}` in this turn. ",
            "Execute one variant, review the outcome, then either stop or explain the blocker."
        ),
        tool
    )
}

fn mutation_completed_intent_directive(tool: &str) -> String {
    format!(
        concat!(
            "Do not call `{}` again for the same deterministic intent in this turn; ",
            "that intent already completed. Reuse the current evidence and finish the response."
        ),
        tool
    )
}

fn mutation_failed_intent_directive(tool: &str, failure_class: Option<FailureClass>) -> String {
    let failure_label = match failure_class {
        Some(FailureClass::Timeout) => "timed out",
        Some(
            FailureClass::Transient | FailureClass::TransientTransport | FailureClass::RateLimited,
        ) => "already retried after a transient failure",
        Some(FailureClass::Unknown) => "failed with an unknown error",
        _ => "failed permanently",
    };
    format!(
        concat!(
            "Do not keep exploring more deterministic variants with `{}` in this turn; ",
            "that intent {}. Surface the blocker or wait for materially new information."
        ),
        tool, failure_label
    )
}

fn truncate_utf8_bytes(input: &str, max_bytes: usize) -> &str {
    let mut end = 0;
    for (start, ch) in input.char_indices() {
        let next = start + ch.len_utf8();
        if next > max_bytes {
            break;
        }
        end = next;
    }
    &input[..end]
}

fn collect_valid_tool_calls(
    allowed: &[ToolCall],
    malformed_results: &mut Vec<ToolResult>,
) -> Vec<ToolCall> {
    allowed
        .iter()
        .filter_map(|call| {
            if let Some(details) = fx_llm::malformed_tool_arguments(&call.arguments) {
                tracing::warn!(
                    tool = %call.name,
                    error = %details.error,
                    "rejecting tool call with malformed arguments"
                );
                malformed_results.push(ToolResult {
                    tool_call_id: call.id.clone(),
                    tool_name: call.name.clone(),
                    success: false,
                    output: format!(
                        "Tool call failed: arguments could not be parsed as valid JSON ({error}). Retry the same tool call with valid JSON escaping inside string values: use \\\\ for backslashes, \\\" for inner quotes, and \\n for newlines.",
                        error = details.error
                    ),
                    failure_class: None,
                });
                None
            } else {
                Some(call.clone())
            }
        })
        .collect()
}

pub(super) fn partition_by_call_classification(
    calls: &[ToolCall],
    executor: &dyn ToolExecutor,
    required: ToolCallClassification,
    reason: &str,
) -> (Vec<ToolCall>, Vec<BlockedToolCall>) {
    let mut allowed = Vec::new();
    let mut blocked = Vec::new();
    for call in calls {
        if executor.classify_call(call) == required {
            allowed.push(call.clone());
        } else {
            blocked.push(BlockedToolCall::policy(call.clone(), reason));
        }
    }
    (allowed, blocked)
}

pub(super) fn partition_by_allowed_tool_names(
    calls: &[ToolCall],
    allowed_names: &[String],
    reason: &str,
) -> (Vec<ToolCall>, Vec<BlockedToolCall>) {
    let allowed_names: HashSet<&str> = allowed_names.iter().map(String::as_str).collect();
    let mut allowed = Vec::new();
    let mut blocked = Vec::new();
    for call in calls {
        if allowed_names.contains(call.name.as_str()) {
            allowed.push(call.clone());
        } else {
            blocked.push(BlockedToolCall::policy(call.clone(), reason));
        }
    }
    (allowed, blocked)
}

pub(super) fn build_uniform_blocked_calls(
    calls: &[ToolCall],
    reason: &str,
) -> Vec<BlockedToolCall> {
    calls
        .iter()
        .cloned()
        .map(|call| BlockedToolCall::policy(call, reason))
        .collect()
}

fn observation_tool_fingerprint(call: &ToolCall) -> String {
    // The call ID is intentionally excluded because it is request-local and would
    // make identical observation rounds look new.
    format!(
        "{}:{}",
        call.name,
        canonicalized_tool_arguments(&call.arguments)
    )
}

// A model can evade exact fingerprint duplicate detection by varying grep
// patterns, sed ranges, or equivalent shell spelling while staring at the same
// source file. Treat the first couple of same-scope observations as useful
// inspection, then let further same-scope reads accumulate observation pressure.
const GENERIC_OBSERVATION_SCOPE_PRODUCTIVE_ALLOWANCE: u16 = 2;

fn observation_tool_semantic_scope(call: &ToolCall) -> Option<String> {
    if let Some(path) = call
        .arguments
        .get("path")
        .and_then(serde_json::Value::as_str)
    {
        return normalized_observation_path(path, None).map(|path| format!("path:{path}"));
    }

    if call.name != "run_command" {
        return None;
    }

    let command = call
        .arguments
        .get("command")
        .and_then(serde_json::Value::as_str)?;
    let cwd = call
        .arguments
        .get("cwd")
        .and_then(serde_json::Value::as_str);
    command
        .split_whitespace()
        .filter_map(|token| normalized_observation_path(token, cwd))
        .max_by_key(|path| (path.matches('/').count(), path.len()))
        .map(|path| format!("path:{path}"))
}

fn should_extract_progress_evidence_references(call: &ToolCall, result: &ToolResult) -> bool {
    match call.name.as_str() {
        "search_text" => true,
        "run_command" => run_command_output_can_discover_references(call, result),
        _ => false,
    }
}

fn run_command_output_can_discover_references(call: &ToolCall, result: &ToolResult) -> bool {
    if result.output.contains("diff --git ") {
        return true;
    }

    let Some(command) = call
        .arguments
        .get("command")
        .and_then(serde_json::Value::as_str)
    else {
        return false;
    };
    let normalized = command.trim().to_ascii_lowercase();
    normalized
        .split(['|', '&', ';'])
        .map(str::trim_start)
        .any(|segment| {
            segment.starts_with("rg ")
                || segment.starts_with("rg\t")
                || segment.starts_with("grep ")
                || segment.starts_with("grep\t")
                || segment.starts_with("find ")
                || segment.starts_with("find\t")
                || segment.starts_with("fd ")
                || segment.starts_with("fd\t")
                || segment.starts_with("git diff")
                || segment.starts_with("git --no-pager diff")
                || segment.starts_with("gh pr diff")
        })
}

fn normalized_observation_path(token: &str, cwd: Option<&str>) -> Option<String> {
    let mut cleaned = token
        .trim_matches(|ch: char| {
            matches!(
                ch,
                '`' | '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | ',' | ';'
            )
        })
        .trim()
        .trim_end_matches(':')
        .trim_end_matches('.');
    cleaned = cleaned
        .strip_prefix("a/")
        .or_else(|| cleaned.strip_prefix("b/"))
        .unwrap_or(cleaned);
    cleaned = cleaned.strip_prefix("./").unwrap_or(cleaned);

    if cleaned.is_empty()
        || cleaned.starts_with('-')
        || cleaned.contains("://")
        || cleaned.contains('*')
        || !(cleaned.contains('/') || path_token_has_file_extension(cleaned))
    {
        return None;
    }

    let path = if cleaned.starts_with('/') || cleaned.starts_with('~') {
        cleaned.to_string()
    } else if let Some(cwd) = cwd.filter(|cwd| !cwd.trim().is_empty()) {
        format!("{}/{}", cwd.trim_end_matches('/'), cleaned)
    } else {
        cleaned.to_string()
    };
    Some(collapse_observation_path_separators(&path))
}

fn path_token_has_file_extension(token: &str) -> bool {
    let Some((stem, extension)) = token.rsplit_once('.') else {
        return false;
    };
    !stem.is_empty()
        && extension.len() >= 2
        && extension
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn collapse_observation_path_separators(path: &str) -> String {
    let mut collapsed = String::with_capacity(path.len());
    let mut last_was_slash = false;
    for ch in path.chars() {
        if ch == '/' {
            if !last_was_slash {
                collapsed.push(ch);
            }
            last_was_slash = true;
        } else {
            collapsed.push(ch);
            last_was_slash = false;
        }
    }
    collapsed.trim_end_matches('/').to_string()
}

fn discovered_evidence_slot_id(label: &str) -> Option<String> {
    let normalized = normalize_contract_label(label);
    (!normalized.is_empty()).then(|| format!("evidence:discovered:{normalized}"))
}

fn tool_progress_observation_text(call: &ToolCall, result: Option<&ToolResult>) -> String {
    let output = result
        .map(|result| result.output.as_str())
        .unwrap_or_default();
    normalize_contract_label(&format!("{} {} {}", call.name, call.arguments, output))
}

fn tool_progress_failure_key(fingerprint: &str, result: &ToolResult) -> String {
    let mut hasher = DefaultHasher::new();
    result.output.hash(&mut hasher);
    result
        .failure_class
        .map(FailureClass::as_str)
        .hash(&mut hasher);
    format!("{fingerprint}:failure:{:016x}", hasher.finish())
}

pub(super) fn canonicalized_tool_arguments(arguments: &serde_json::Value) -> String {
    serde_json::to_string(&canonicalize_json_value(arguments))
        .unwrap_or_else(|_| arguments.to_string())
}

pub(super) fn tool_execution_signature(call: &ToolCall) -> String {
    format!(
        "{}:{}",
        call.name,
        canonicalized_tool_arguments(&call.arguments)
    )
}

fn tool_progress_display_target(call: &ToolCall) -> Option<String> {
    match call.name.as_str() {
        "run_command" => run_command_progress_display_target(&call.arguments),
        _ => None,
    }
}

fn run_command_progress_display_target(arguments: &serde_json::Value) -> Option<String> {
    if let Some(command) = arguments
        .get("command")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|command| !command.is_empty())
    {
        return Some(command.to_string());
    }

    arguments
        .get("argv")
        .and_then(serde_json::Value::as_array)
        .map(|argv| {
            // Display only: do not treat this as a shell-safe command line.
            // The executed form remains the structured argv vector.
            argv.iter()
                .filter_map(serde_json::Value::as_str)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|command| !command.trim().is_empty())
}

fn tool_result_cost_cents(
    prior_failures: u16,
    call: &ToolCall,
    result: &ToolResult,
    signature_failures: &mut HashMap<String, u16>,
) -> u64 {
    if call.name != "run_command" {
        return 1;
    }

    let signature = tool_execution_signature(call);
    let consecutive_failures = signature_failures
        .entry(signature)
        .or_insert(prior_failures);
    if result.success {
        *consecutive_failures = 0;
        return 1;
    }

    let multiplier = run_command_failure_cost_multiplier(*consecutive_failures);
    *consecutive_failures = consecutive_failures.saturating_add(1);
    multiplier
}

fn run_command_failure_cost_multiplier(consecutive_failures: u16) -> u64 {
    match consecutive_failures {
        0 => 1,
        1 => 2,
        2 => 4,
        _ => 8,
    }
}

fn canonicalize_json_value(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by_key(|(key, _)| *key);
            let mut canonical = serde_json::Map::with_capacity(entries.len());
            for (key, value) in entries {
                canonical.insert(key.clone(), canonicalize_json_value(value));
            }
            serde_json::Value::Object(canonical)
        }
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.iter().map(canonicalize_json_value).collect())
        }
        other => other.clone(),
    }
}

pub(super) fn blocked_tool_message(
    tool_name: &str,
    reason: &str,
    guidance: Option<&str>,
) -> String {
    match guidance {
        Some(guidance) => format!("Tool '{}' blocked: {}. {}", tool_name, reason, guidance),
        None => format!(
            "Tool '{}' blocked: {}. Try a different approach.",
            tool_name, reason
        ),
    }
}

fn tool_execution_failure_message(calls: &[ToolCall], error_message: &str) -> String {
    match calls {
        [call] => format!("Tool '{}' failed: {error_message}", call.name),
        _ => {
            let names = calls
                .iter()
                .map(|call| call.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            format!("Tool batch failed for [{names}]: {error_message}")
        }
    }
}

pub(super) fn build_blocked_tool_results(blocked: &[BlockedToolCall]) -> Vec<ToolResult> {
    blocked
        .iter()
        .map(|blocked_call| {
            ToolResult::failure(
                blocked_call.call.id.clone(),
                blocked_call.call.name.clone(),
                blocked_tool_message(
                    &blocked_call.call.name,
                    &blocked_call.reason,
                    blocked_call.guidance.as_deref(),
                ),
                blocked_call
                    .failure_class
                    .unwrap_or(crate::act::FailureClass::Unknown),
            )
        })
        .collect()
}

pub(super) fn reorder_results_by_calls(
    calls: &[ToolCall],
    results: Vec<ToolResult>,
) -> Vec<ToolResult> {
    if results.len() <= 1 {
        return results;
    }
    let mut by_id: HashMap<String, ToolResult> = HashMap::with_capacity(results.len());
    for result in results {
        by_id.insert(result.tool_call_id.clone(), result);
    }
    let mut ordered = Vec::with_capacity(calls.len());
    for call in calls {
        if let Some(result) = by_id.remove(&call.id) {
            ordered.push(result);
        }
    }
    ordered.extend(by_id.into_values());
    ordered
}

pub(super) fn truncate_tool_results(results: Vec<ToolResult>, max_bytes: usize) -> Vec<ToolResult> {
    results
        .into_iter()
        .map(|mut result| {
            if result.output.len() > max_bytes {
                result.output = truncate_tool_result(&result.output, max_bytes).into_owned();
            }
            result
        })
        .collect()
}

pub(super) fn evict_oldest_results(
    mut results: Vec<ToolResult>,
    max_tokens: usize,
) -> Vec<ToolResult> {
    if results.is_empty() {
        return results;
    }
    const MIN_SYNTHESIS_TOKENS: usize = 1_000;
    let max_tokens = max_tokens.max(MIN_SYNTHESIS_TOKENS);
    let total_tokens = estimate_results_tokens(&results);
    if total_tokens <= max_tokens {
        let total_bytes: usize = results.iter().map(|result| result.output.len()).sum();
        tracing::debug!(
            total_bytes,
            total_tokens,
            max_tokens,
            result_count = results.len(),
            "synthesis context guard: under token limit, no eviction needed"
        );
        return results;
    }
    let (evicted_count, bytes_saved) = evict_results_until_under_limit(&mut results, max_tokens);
    if evicted_count > 0 {
        tracing::info!(
            evicted_count,
            bytes_saved,
            remaining = results.len() - evicted_count.min(results.len()),
            "synthesis context guard: evicted oldest tool results"
        );
    }
    truncate_single_oversized_result(&mut results, max_tokens);
    results
}

fn estimate_results_tokens(results: &[ToolResult]) -> usize {
    results
        .iter()
        .map(|result| estimate_text_tokens(&result.output))
        .sum()
}

fn evict_results_until_under_limit(
    results: &mut [ToolResult],
    max_tokens: usize,
) -> (usize, usize) {
    let mut current_tokens = estimate_results_tokens(results);
    let mut evicted_count = 0usize;
    let mut bytes_saved = 0usize;
    for result in results.iter_mut() {
        if current_tokens <= max_tokens {
            break;
        }
        let old_tokens = estimate_text_tokens(&result.output);
        let stub = format!(
            "[evicted: {} result too large for synthesis]",
            result.tool_name
        );
        let stub_tokens = estimate_text_tokens(&stub);
        bytes_saved = bytes_saved.saturating_add(result.output.len());
        result.output = stub;
        current_tokens = current_tokens
            .saturating_sub(old_tokens)
            .saturating_add(stub_tokens);
        evicted_count = evicted_count.saturating_add(1);
    }
    (evicted_count, bytes_saved)
}

fn truncate_single_oversized_result(results: &mut [ToolResult], max_tokens: usize) {
    let current_tokens = estimate_results_tokens(results);
    if current_tokens <= max_tokens {
        return;
    }
    if let Some(largest) = results.iter_mut().max_by_key(|result| result.output.len()) {
        let excess_tokens = current_tokens.saturating_sub(max_tokens);
        let excess_bytes = excess_tokens.saturating_mul(4);
        let target_bytes = largest.output.len().saturating_sub(excess_bytes);
        largest.output = truncate_tool_result(&largest.output, target_bytes).into_owned();
    }
}

pub(super) fn tool_synthesis_prompt(tool_results: &[ToolResult], instruction: &str) -> String {
    let has_tool_error = tool_results.iter().any(|result| !result.success);
    let error_relay_instruction = if has_tool_error {
        "\nIf any tool returned an error, tell the user exactly what went wrong: include the actual error message. Do not soften, hedge, or paraphrase errors."
    } else {
        ""
    };
    let tool_summary = tool_results
        .iter()
        .map(|result| format!("- {}: {}", result.tool_name, result.output))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "You are Fawx. Never introduce yourself, greet the user, or add preamble. Answer the user's question using these tool results. \
Do NOT describe what tools were called, narrate the process, or comment on how you got the information. \
Just provide the answer directly. \
If the user asked for a specific format or value type, preserve that exact format. \
Do not convert timestamps to human-readable, counts to lists, or raw values to prose \
unless the user explicitly asked for that.{error_relay_instruction}\n\n\
{instruction}\n\n\
Tool results:\n{tool_summary}"
    )
}

fn synthesis_usage(prompt: &str, response: &str) -> TokenUsage {
    TokenUsage {
        input_tokens: estimate_tokens(prompt),
        output_tokens: estimate_tokens(response),
        ..Default::default()
    }
}

#[cfg(test)]
pub(super) fn append_tool_round_messages(
    context_messages: &mut Vec<Message>,
    calls: &[ToolCall],
    provider_item_ids: &HashMap<String, String>,
    results: &[ToolResult],
) -> Result<(), LoopError> {
    let (assistant_message, result_message) =
        build_tool_round_messages(calls, provider_item_ids, results)?;
    context_messages.push(assistant_message);
    context_messages.push(result_message);
    Ok(())
}

fn build_tool_round_messages(
    calls: &[ToolCall],
    provider_item_ids: &HashMap<String, String>,
    results: &[ToolResult],
) -> Result<(Message, Message), LoopError> {
    let assistant_message = build_tool_use_assistant_message(calls, provider_item_ids);
    let result_message = build_tool_result_message(calls, results)?;
    Ok((assistant_message, result_message))
}

pub(super) fn record_tool_round_messages(
    continuation_messages: &mut Vec<Message>,
    evidence_messages: &mut Vec<Message>,
    calls: &[ToolCall],
    provider_item_ids: &HashMap<String, String>,
    results: &[ToolResult],
) -> Result<(), LoopError> {
    let (assistant_message, result_message) =
        build_tool_round_messages(calls, provider_item_ids, results)?;
    continuation_messages.push(assistant_message.clone());
    continuation_messages.push(result_message.clone());
    evidence_messages.push(assistant_message);
    evidence_messages.push(result_message);
    Ok(())
}

pub(super) fn build_tool_use_assistant_message(
    calls: &[ToolCall],
    provider_item_ids: &HashMap<String, String>,
) -> Message {
    let content = calls
        .iter()
        .map(|call| ContentBlock::ToolUse {
            id: call.id.clone(),
            provider_id: provider_item_ids.get(&call.id).cloned(),
            name: call.name.clone(),
            input: call.arguments.clone(),
        })
        .collect();
    Message {
        role: MessageRole::Assistant,
        content,
    }
}

pub(super) fn extract_tool_use_provider_ids(content: &[ContentBlock]) -> HashMap<String, String> {
    content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::ToolUse {
                id,
                provider_id: Some(provider_id),
                ..
            } if !id.trim().is_empty() && !provider_id.trim().is_empty() => {
                Some((id.clone(), provider_id.clone()))
            }
            _ => None,
        })
        .collect()
}

pub(super) fn build_tool_result_message(
    calls: &[ToolCall],
    results: &[ToolResult],
) -> Result<Message, LoopError> {
    let call_order = calls
        .iter()
        .enumerate()
        .map(|(index, call)| (call.id.clone(), index))
        .collect::<HashMap<_, _>>();
    let mut ordered_results = indexed_tool_results(&call_order, results)?;
    ordered_results.sort_by_key(|(index, _)| *index);
    let content = ordered_results
        .into_iter()
        .map(|(_, result)| ContentBlock::ToolResult {
            tool_use_id: result.tool_call_id.clone(),
            content: result_block_content(result),
        })
        .collect();
    Ok(Message {
        role: MessageRole::Tool,
        content,
    })
}

fn indexed_tool_results<'a>(
    call_order: &HashMap<String, usize>,
    results: &'a [ToolResult],
) -> Result<Vec<(usize, &'a ToolResult)>, LoopError> {
    results
        .iter()
        .map(|result| {
            call_order
                .get(&result.tool_call_id)
                .copied()
                .map(|index| (index, result))
                .ok_or_else(|| unmatched_tool_call_id_error(result))
        })
        .collect()
}

fn result_block_content(result: &ToolResult) -> serde_json::Value {
    if result.success {
        serde_json::Value::String(result.output.clone())
    } else {
        serde_json::Value::String(format!("[ERROR] {}", result.output))
    }
}

fn unmatched_tool_call_id_error(result: &ToolResult) -> LoopError {
    loop_error(
        "act",
        &format!(
            "tool result has unmatched tool_call_id '{}' for tool '{}'",
            result.tool_call_id, result.tool_name
        ),
        false,
    )
}

fn calls_are_all_classification(
    calls: &[ToolCall],
    executor: &dyn ToolExecutor,
    required: ToolCallClassification,
) -> bool {
    !calls.is_empty()
        && calls
            .iter()
            .all(|call| executor.classify_call(call) == required)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::act::FailureClass;
    use crate::budget::{BudgetConfig, BudgetTracker};
    use crate::cancellation::CancellationToken;
    use crate::context_manager::ContextCompactor;
    use async_trait::async_trait;
    use fx_llm::{ProviderError, ToolDefinition};
    use std::sync::Arc;
    use std::time::Duration;

    #[derive(Debug)]
    struct DualToolExecutor;

    #[async_trait]
    impl ToolExecutor for DualToolExecutor {
        async fn execute_tools(
            &self,
            calls: &[ToolCall],
            _cancel: Option<&CancellationToken>,
        ) -> Result<Vec<ToolResult>, crate::act::ToolExecutorError> {
            Ok(calls
                .iter()
                .map(|call| ToolResult {
                    tool_call_id: call.id.clone(),
                    tool_name: call.name.clone(),
                    success: true,
                    output: format!("ok: {}", call.name),
                    failure_class: None,
                })
                .collect())
        }

        fn tool_definitions(&self) -> Vec<ToolDefinition> {
            vec![tool_definition("read_file"), tool_definition("write_file")]
        }

        fn cacheability(&self, tool_name: &str) -> ToolCacheability {
            match tool_name {
                "write_file" => ToolCacheability::SideEffect,
                _ => ToolCacheability::Cacheable,
            }
        }
    }

    #[derive(Debug)]
    struct FailureCostExecutor;

    #[async_trait]
    impl ToolExecutor for FailureCostExecutor {
        async fn execute_tools(
            &self,
            calls: &[ToolCall],
            _cancel: Option<&CancellationToken>,
        ) -> Result<Vec<ToolResult>, crate::act::ToolExecutorError> {
            Ok(calls
                .iter()
                .map(|call| {
                    ToolResult::failure(
                        call.id.clone(),
                        call.name.clone(),
                        format!("failed: {}", call.name),
                        FailureClass::Unknown,
                    )
                })
                .collect())
        }

        fn tool_definitions(&self) -> Vec<ToolDefinition> {
            vec![tool_definition("run_command"), tool_definition("read_file")]
        }

        fn cacheability(&self, tool_name: &str) -> ToolCacheability {
            match tool_name {
                "run_command" => ToolCacheability::SideEffect,
                _ => ToolCacheability::Cacheable,
            }
        }
    }

    #[derive(Debug)]
    struct TimeoutExecutor;

    #[derive(Debug)]
    struct NoopLlm;

    #[async_trait]
    impl LlmProvider for NoopLlm {
        async fn generate(&self, _: &str, _: u32) -> Result<String, fx_core::error::LlmError> {
            Ok(String::new())
        }

        async fn generate_streaming(
            &self,
            _: &str,
            _: u32,
            _callback: Box<dyn Fn(String) + Send + 'static>,
        ) -> Result<String, fx_core::error::LlmError> {
            Ok(String::new())
        }

        fn model_name(&self) -> &str {
            "noop"
        }

        async fn complete(
            &self,
            _: CompletionRequest,
        ) -> Result<CompletionResponse, ProviderError> {
            Ok(CompletionResponse {
                content: vec![ContentBlock::Text {
                    text: "final child summary".to_string(),
                }],
                tool_calls: Vec::new(),
                usage: None,
                stop_reason: None,
            })
        }
    }

    #[derive(Debug)]
    struct SubagentHarvestExecutor;

    #[async_trait]
    impl ToolExecutor for SubagentHarvestExecutor {
        async fn execute_tools(
            &self,
            calls: &[ToolCall],
            _cancel: Option<&CancellationToken>,
        ) -> Result<Vec<ToolResult>, crate::act::ToolExecutorError> {
            Ok(calls
                .iter()
                .map(|call| {
                    let id = call
                        .arguments
                        .get("id")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("agent-a");
                    subagent_result(&call.id, &call.name, completed_subagent(id))
                })
                .collect())
        }

        fn tool_definitions(&self) -> Vec<ToolDefinition> {
            vec![tool_definition(SUBAGENT_STATUS_TOOL_NAME)]
        }

        fn cacheability(&self, tool_name: &str) -> ToolCacheability {
            match tool_name {
                SPAWN_AGENT_TOOL_NAME => ToolCacheability::SideEffect,
                SUBAGENT_STATUS_TOOL_NAME => ToolCacheability::NeverCache,
                _ => ToolCacheability::NeverCache,
            }
        }
    }

    #[derive(Debug)]
    struct OrchestrationExecutor;

    #[async_trait]
    impl ToolExecutor for OrchestrationExecutor {
        async fn execute_tools(
            &self,
            calls: &[ToolCall],
            _cancel: Option<&CancellationToken>,
        ) -> Result<Vec<ToolResult>, crate::act::ToolExecutorError> {
            Ok(calls
                .iter()
                .map(|call| ToolResult {
                    tool_call_id: call.id.clone(),
                    tool_name: call.name.clone(),
                    success: true,
                    output: "ok".to_string(),
                    failure_class: None,
                })
                .collect())
        }

        fn tool_definitions(&self) -> Vec<ToolDefinition> {
            vec![
                tool_definition(SPAWN_AGENT_TOOL_NAME),
                tool_definition("write_file"),
            ]
        }

        fn cacheability(&self, tool_name: &str) -> ToolCacheability {
            match tool_name {
                SPAWN_AGENT_TOOL_NAME | "write_file" => ToolCacheability::SideEffect,
                _ => ToolCacheability::NeverCache,
            }
        }

        fn classify_call(&self, call: &ToolCall) -> ToolCallClassification {
            match call.name.as_str() {
                SPAWN_AGENT_TOOL_NAME => ToolCallClassification::Orchestration,
                "write_file" => ToolCallClassification::Mutation,
                _ => ToolCallClassification::Observation,
            }
        }
    }

    #[async_trait]
    impl ToolExecutor for TimeoutExecutor {
        async fn execute_tools(
            &self,
            calls: &[ToolCall],
            _cancel: Option<&CancellationToken>,
        ) -> Result<Vec<ToolResult>, crate::act::ToolExecutorError> {
            Ok(calls
                .iter()
                .map(|call| crate::act::timed_out_result(&call.id, &call.name))
                .collect())
        }

        fn tool_definitions(&self) -> Vec<ToolDefinition> {
            vec![tool_definition("run_command")]
        }

        fn concurrency_policy(&self) -> crate::act::ConcurrencyPolicy {
            crate::act::ConcurrencyPolicy {
                max_parallel: None,
                timeout_per_call: Some(Duration::from_millis(5)),
            }
        }

        fn cacheability(&self, _tool_name: &str) -> ToolCacheability {
            ToolCacheability::SideEffect
        }
    }

    fn tool_definition(name: &str) -> ToolDefinition {
        ToolDefinition {
            name: name.to_string(),
            description: format!("{name} tool"),
            parameters: serde_json::json!({"type":"object"}),
        }
    }

    fn tool_execution_engine(executor: Arc<dyn ToolExecutor>) -> LoopEngine {
        LoopEngine::builder()
            .budget(BudgetTracker::new(BudgetConfig::default(), 0, 0))
            .context(ContextCompactor::new(2048, 256))
            .max_iterations(3)
            .tool_executor(executor)
            .synthesis_instruction("Summarize".to_string())
            .build()
            .expect("build engine")
    }

    fn subagent_result(call_id: &str, tool_name: &str, output: serde_json::Value) -> ToolResult {
        ToolResult {
            tool_call_id: call_id.to_string(),
            tool_name: tool_name.to_string(),
            success: true,
            output: output.to_string(),
            failure_class: None,
        }
    }

    fn running_subagent(id: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "label": null,
            "mode": "run",
            "status": { "state": "running" },
            "initial_response": null
        })
    }

    fn completed_subagent(id: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "label": null,
            "mode": "run",
            "status": {
                "state": "completed",
                "result": "review complete",
                "tokens_used": 42
            },
            "initial_response": null
        })
    }

    #[test]
    fn pending_subagent_ids_tracks_only_spawned_running_children() {
        let results = vec![
            subagent_result(
                "spawn-a",
                SPAWN_AGENT_TOOL_NAME,
                running_subagent("agent-a"),
            ),
            subagent_result(
                "spawn-b",
                SPAWN_AGENT_TOOL_NAME,
                running_subagent("agent-b"),
            ),
            subagent_result(
                "status-a",
                SUBAGENT_STATUS_TOOL_NAME,
                completed_subagent("agent-a"),
            ),
            subagent_result(
                "status-unrelated",
                SUBAGENT_STATUS_TOOL_NAME,
                running_subagent("unrelated-agent"),
            ),
        ];

        assert_eq!(pending_subagent_ids(&results), vec!["agent-b".to_string()]);
    }

    #[tokio::test]
    async fn finalization_harvests_pending_subagent_status_before_final_answer() {
        let mut engine = tool_execution_engine(Arc::new(SubagentHarvestExecutor));
        let mut state = ToolRoundState::new(&[], &[], None);
        state.all_tool_results = vec![subagent_result(
            "spawn-a",
            SPAWN_AGENT_TOOL_NAME,
            running_subagent("agent-a"),
        )];
        let decision = Decision::UseTools(vec![ToolCall {
            id: "spawn-a".to_string(),
            name: SPAWN_AGENT_TOOL_NAME.to_string(),
            arguments: serde_json::json!({"task": "review kernel loop"}),
        }]);
        let response = CompletionResponse {
            content: vec![ContentBlock::Text {
                text: "I can summarize now.".to_string(),
            }],
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: None,
        };

        let action = engine
            .finalize_tool_response(
                &decision,
                state,
                &response,
                &NoopLlm,
                CycleStream::disabled(),
                None,
            )
            .await
            .expect("finalize pending subagent");

        match action.next_step {
            ActionNextStep::Finish(ActionTerminal::Complete { response }) => {
                assert_eq!(response, "final child summary");
                assert!(action.tool_results.iter().any(|result| {
                    result.tool_name == SUBAGENT_STATUS_TOOL_NAME
                        && result.output.contains("\"state\":\"completed\"")
                        && result.output.contains("agent-a")
                }));
            }
            other => panic!("expected final answer after subagent harvest, got {other:?}"),
        }
    }

    #[test]
    fn orchestration_calls_are_not_mutation_serialized() {
        let mut engine = tool_execution_engine(Arc::new(OrchestrationExecutor));
        let mut state = ToolRoundState::new(&[], &[], None);
        let calls = vec![
            ToolCall {
                id: "spawn-a".to_string(),
                name: SPAWN_AGENT_TOOL_NAME.to_string(),
                arguments: serde_json::json!({"task":"review kernel"}),
            },
            ToolCall {
                id: "spawn-b".to_string(),
                name: SPAWN_AGENT_TOOL_NAME.to_string(),
                arguments: serde_json::json!({"task":"review swift"}),
            },
            ToolCall {
                id: "spawn-c".to_string(),
                name: SPAWN_AGENT_TOOL_NAME.to_string(),
                arguments: serde_json::json!({"task":"review api"}),
            },
        ];

        engine.stage_tool_calls_for_round(&mut state, calls);

        assert_eq!(state.current_calls.len(), 3);
        assert!(state.all_tool_results.is_empty());
    }

    #[tokio::test]
    async fn execute_tool_calls_preserves_original_order_with_blocked_results() {
        let mut engine = tool_execution_engine(Arc::new(DualToolExecutor));
        engine.budget = BudgetTracker::new(
            BudgetConfig {
                max_consecutive_failures: 1,
                max_tool_retries: 0,
                ..BudgetConfig::default()
            },
            0,
            0,
        );
        engine.tool_retry_tracker.record_result(
            &ToolCall {
                id: "seed".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path":"README.md"}),
            },
            false,
        );
        let calls = vec![
            ToolCall {
                id: "call-1".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path":"README.md"}),
            },
            ToolCall {
                id: "call-2".to_string(),
                name: "write_file".to_string(),
                arguments: serde_json::json!({"path":"README.md","content":"hi"}),
            },
        ];

        let results = engine
            .execute_tool_calls_batch_with_stream(&calls, CycleStream::disabled())
            .await
            .expect("execute tool calls")
            .results;

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].tool_call_id, "call-1");
        assert_eq!(results[1].tool_call_id, "call-2");
        assert!(!results[0].success);
        assert!(results[1].success);
    }

    #[tokio::test]
    async fn repeated_failed_identical_run_command_costs_more_than_flat_failures() {
        let mut run_command_engine = tool_execution_engine(Arc::new(FailureCostExecutor));
        let first_run = ToolCall {
            id: "run-1".to_string(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({"command":"false","shell":false}),
        };
        let second_run = ToolCall {
            id: "run-2".to_string(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({"shell":false,"command":"false"}),
        };
        let third_run = ToolCall {
            id: "run-3".to_string(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({"command":"false","shell":false}),
        };

        run_command_engine
            .execute_tool_calls_batch_with_stream(&[first_run], CycleStream::disabled())
            .await
            .expect("execute first run_command failure");
        let first_run_cost = run_command_engine.budget.cost_cents_used();

        run_command_engine
            .execute_tool_calls_batch_with_stream(&[second_run], CycleStream::disabled())
            .await
            .expect("execute second run_command failure");
        let second_run_cost = run_command_engine.budget.cost_cents_used() - first_run_cost;

        run_command_engine
            .execute_tool_calls_batch_with_stream(&[third_run], CycleStream::disabled())
            .await
            .expect("execute third run_command failure");
        let third_run_cost =
            run_command_engine.budget.cost_cents_used() - first_run_cost - second_run_cost;

        let mut read_file_engine = tool_execution_engine(Arc::new(FailureCostExecutor));
        let first_read = ToolCall {
            id: "read-1".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path":"README.md"}),
        };
        let second_read = ToolCall {
            id: "read-2".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path":"README.md"}),
        };
        let third_read = ToolCall {
            id: "read-3".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path":"README.md"}),
        };

        read_file_engine
            .execute_tool_calls_batch_with_stream(&[first_read], CycleStream::disabled())
            .await
            .expect("execute first read_file failure");
        let first_read_cost = read_file_engine.budget.cost_cents_used();

        read_file_engine
            .execute_tool_calls_batch_with_stream(&[second_read], CycleStream::disabled())
            .await
            .expect("execute second read_file failure");
        let second_read_cost = read_file_engine.budget.cost_cents_used() - first_read_cost;

        read_file_engine
            .execute_tool_calls_batch_with_stream(&[third_read], CycleStream::disabled())
            .await
            .expect("execute third read_file failure");
        let third_read_cost =
            read_file_engine.budget.cost_cents_used() - first_read_cost - second_read_cost;

        assert_eq!(first_run_cost, 1);
        assert_eq!(second_run_cost, 2);
        assert_eq!(third_run_cost, 4);
        assert_eq!(first_read_cost, 1);
        assert_eq!(second_read_cost, 1);
        assert_eq!(third_read_cost, 1);
        assert_eq!(run_command_engine.budget.cost_cents_used(), 7);
        assert_eq!(read_file_engine.budget.cost_cents_used(), 3);
    }

    #[tokio::test]
    async fn timed_out_tool_execution_emits_timeout_signal() {
        let mut engine = tool_execution_engine(Arc::new(TimeoutExecutor));
        let call = ToolCall {
            id: "call-1".to_string(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({"command":"sleep 1","shell":false}),
        };

        let batch = engine
            .execute_tool_calls_batch_with_stream(
                std::slice::from_ref(&call),
                CycleStream::disabled(),
            )
            .await
            .expect("execute timed out tool");
        assert!(!batch.results[0].success);
        assert!(batch.results[0].is_timeout());
        assert_eq!(
            batch.results[0].failure_classification(),
            Some(FailureClass::Timeout)
        );

        engine.emit_action_signals(std::slice::from_ref(&call), &batch.results);

        let signals = engine.signals.drain_all();
        let friction = signals
            .iter()
            .find(|signal| signal.kind == SignalKind::Friction)
            .expect("friction signal");
        let timeout = signals
            .iter()
            .find(|signal| signal.kind == SignalKind::Timeout)
            .expect("timeout signal");

        assert_eq!(timeout.cause_id, Some(friction.id));
        assert_eq!(timeout.metadata["tool"], "run_command");
        assert_eq!(timeout.metadata["tool_call_id"], "call-1");
        assert_eq!(timeout.metadata["timeout_ms"], serde_json::json!(5));
        assert_eq!(timeout.metadata["failure_class"], "timeout");
    }

    #[test]
    fn allowed_tool_name_blocks_are_classified_permanent() {
        let calls = vec![ToolCall {
            id: "call-1".to_string(),
            name: "write_file".to_string(),
            arguments: serde_json::json!({"path":"README.md","content":"hi"}),
        }];

        let (_allowed, blocked) =
            partition_by_allowed_tool_names(&calls, &["read_file".to_string()], "disallowed");

        assert_eq!(blocked.len(), 1);
        assert_eq!(blocked[0].source, BlockedToolSource::Policy);
        assert_eq!(blocked[0].failure_class, Some(FailureClass::Permanent));
        let results = build_blocked_tool_results(&blocked);
        assert_eq!(
            results[0].failure_classification(),
            Some(FailureClass::Permanent)
        );
    }

    #[test]
    fn classification_blocks_are_classified_permanent() {
        let calls = vec![ToolCall {
            id: "call-1".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path":"README.md"}),
        }];

        let (_allowed, blocked) = partition_by_call_classification(
            &calls,
            &DualToolExecutor,
            ToolCallClassification::Mutation,
            "mutation_only",
        );

        assert_eq!(blocked.len(), 1);
        assert_eq!(blocked[0].source, BlockedToolSource::Policy);
        assert_eq!(blocked[0].failure_class, Some(FailureClass::Permanent));
    }

    #[test]
    fn uniform_policy_blocks_are_classified_permanent() {
        let calls = vec![ToolCall {
            id: "call-1".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path":"README.md"}),
        }];

        let blocked = build_uniform_blocked_calls(&calls, "policy");
        assert_eq!(blocked.len(), 1);
        assert_eq!(blocked[0].source, BlockedToolSource::Policy);
        assert_eq!(blocked[0].failure_class, Some(FailureClass::Permanent));
    }

    #[test]
    fn truncated_retry_arguments_preserves_utf8_boundaries() {
        let truncated = truncated_retry_arguments(&serde_json::json!({
            "query": "🙂".repeat(80),
        }));

        assert!(truncated.len() <= 160);
        assert!(truncated.ends_with("..."));
    }

    #[test]
    fn build_tool_round_messages_preserves_provider_ids() {
        let calls = vec![ToolCall {
            id: "call-1".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path":"README.md"}),
        }];
        let results = vec![ToolResult {
            tool_call_id: "call-1".to_string(),
            tool_name: "read_file".to_string(),
            success: true,
            output: "ok".to_string(),
            failure_class: None,
        }];
        let provider_item_ids =
            HashMap::from([(String::from("call-1"), String::from("provider-1"))]);

        let (assistant_message, result_message) =
            build_tool_round_messages(&calls, &provider_item_ids, &results)
                .expect("build tool round messages");

        assert_eq!(result_message.role, MessageRole::Tool);
        match &assistant_message.content[0] {
            ContentBlock::ToolUse { provider_id, .. } => {
                assert_eq!(provider_id.as_deref(), Some("provider-1"));
            }
            other => panic!("expected tool use block, got {other:?}"),
        }
    }
}
