use super::bounded_local::{
    bounded_local_terminal_partial_response, partition_by_bounded_local_phase_semantics,
    BoundedLocalTerminalReason, TurnExecutionProfile,
};
use super::compaction::CompactionScope;
use super::request::{
    build_continuation_request, ContinuationRequestParams, RequestBuildContext, ToolRequestConfig,
};
use super::retry::{partition_by_retry_policy, BlockedToolCall};
use super::streaming::{StreamingRequestContext, TextStreamVisibility};
use super::{
    continuation_budget_cost, current_time_ms, estimate_text_tokens, estimate_tokens,
    find_decompose_tool_call, loop_error, meaningful_response_text, response_text_segment,
    stitch_response_segments, stitched_response_text, summarize_tool_progress,
    tool_continuation_artifact_write_target, tool_continuation_turn_commitment,
    tool_error_relay_directive, CycleStream, FollowUpDecomposeContext, LlmProvider, LoopEngine,
    ToolRoundState, NOTIFY_TOOL_NAME, OBSERVATION_ONLY_CALL_BLOCK_REASON,
    OBSERVATION_ONLY_MUTATION_REPLAN_DIRECTIVE, OBSERVATION_ONLY_TOOL_ROUND_NUDGE,
    TOOL_ROUND_PROGRESS_NUDGE,
};
use crate::act::{
    ActionContinuation, ActionNextStep, ActionResult, ActionTerminal, ContinuationToolScope,
    TokenUsage, ToolCacheability, ToolCallClassification, ToolExecutor, ToolResult,
};
use crate::budget::{truncate_tool_result, ActionCost, BudgetState};
use crate::decide::Decision;
use crate::signals::{LoopStep, SignalKind};
use crate::streaming::{ErrorCategory, Phase};
use crate::types::LoopError;
use fx_core::message::{InternalMessage, StreamPhase, ToolRoundCall, ToolRoundResult};
use fx_llm::{CompletionResponse, ContentBlock, Message, MessageRole, ToolCall, ToolDefinition};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

pub(super) const TOOL_SYNTHESIS_MAX_OUTPUT_TOKENS: u32 = 1024;
const DIRECT_INSPECTION_EMPTY_SUMMARY_RESPONSE: &str =
    "Inspection completed but produced no summary.";

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
pub(super) enum ToolRoundOutcome {
    Cancelled,
    /// Budget soft-ceiling crossed after tool execution; skip LLM continuation.
    BudgetLow,
    /// Direct utility profile can answer immediately from tool output.
    DirectUtilityAnswered(String),
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
    Break(ToolRoundState),
    Return(Box<ActionResult>),
}

enum ToolLoopExit {
    Exhausted(ToolRoundState),
    Return(Box<ActionResult>),
}

struct ExecutedToolRound {
    calls: Vec<ToolCall>,
    results: Vec<ToolResult>,
    has_tool_errors: bool,
    started_at_ms: u64,
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
}

impl LoopEngine {
    pub(super) fn publish_tool_calls(&self, calls: &[ToolCall], stream: CycleStream<'_>) {
        for call in calls {
            stream.tool_call_start(call);
            stream.tool_call_complete(call);
            self.publish_tool_use(call);
        }
    }

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

    pub(super) fn publish_tool_results(&mut self, results: &[ToolResult], stream: CycleStream<'_>) {
        for result in results {
            stream.tool_result(result);
            self.publish_tool_result(result);
        }
    }

    pub(super) fn publish_tool_round(
        &mut self,
        calls: &[ToolCall],
        results: &[ToolResult],
        stream: CycleStream<'_>,
    ) {
        self.publish_tool_calls(calls, stream);
        self.publish_tool_results(results, stream);

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

    pub(super) fn record_tool_execution_cost(&mut self, tool_count: usize) {
        self.budget.record(&ActionCost {
            llm_calls: 0,
            tool_invocations: tool_count as u32,
            tokens: 0,
            cost_cents: tool_count as u64,
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
        self.execute_tool_calls_with_stream(calls, CycleStream::disabled())
            .await
    }

    pub(super) async fn execute_tool_calls_with_stream(
        &mut self,
        calls: &[ToolCall],
        stream: CycleStream<'_>,
    ) -> Result<Vec<ToolResult>, LoopError> {
        let prepared = self.prepare_tool_calls_for_execution(calls);
        self.emit_blocked_tool_errors(&prepared.blocked, stream);
        let mut results = self
            .execute_allowed_tool_calls(&prepared.allowed, stream)
            .await?;
        self.tool_retry_tracker
            .record_results(&prepared.allowed, &results);
        results.extend(build_blocked_tool_results(&prepared.blocked));
        Ok(reorder_results_by_calls(calls, results))
    }

    fn prepare_tool_calls_for_execution(&self, calls: &[ToolCall]) -> PreparedToolCalls {
        let retry_policy = self.budget.config().retry_policy();
        let (allowed, blocked) =
            partition_by_retry_policy(calls, &self.tool_retry_tracker, &retry_policy);
        let prepared = PreparedToolCalls::new(allowed, blocked);
        let prepared = self.filter_calls_by_profile_tool_names(prepared);
        let prepared = self.filter_calls_by_bounded_local_semantics(prepared);
        self.filter_calls_by_observation_controls(prepared)
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

    pub(super) fn emit_blocked_tool_errors(
        &mut self,
        blocked: &[BlockedToolCall],
        stream: CycleStream<'_>,
    ) {
        for blocked_call in blocked {
            let call = &blocked_call.call;
            let signature_failures = self.tool_retry_tracker.consecutive_failures_for(call);
            self.emit_signal(
                LoopStep::Act,
                SignalKind::Blocked,
                format!("tool '{}' blocked: {}", call.name, blocked_call.reason),
                serde_json::json!({
                    "tool": call.name,
                    "reason": blocked_call.reason,
                    "signature_failures": signature_failures,
                    "cycle_total_failures": self.tool_retry_tracker.cycle_total_failures(),
                }),
            );
            stream.emit_error(
                ErrorCategory::ToolExecution,
                blocked_tool_message(&call.name, &blocked_call.reason),
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
            ToolLoopExit::Exhausted(state) => {
                self.finish_tool_loop_on_exhaustion(decision, state, llm, stream)
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
        let initial_text = self.pending_tool_response_text.take();
        let mut state = ToolRoundState::new(calls, context_messages, initial_text);
        let (execute_calls, deferred) = self.apply_fan_out_cap(calls);
        state.current_calls = execute_calls;
        if !deferred.is_empty() {
            self.append_deferred_tool_results(&mut state, &deferred, calls.len());
        }
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
        for round in 0..self.max_iterations {
            if self.tool_round_interrupted() {
                return Ok(ToolLoopExit::Return(Box::new(
                    self.cancelled_tool_action_from_state(decision, state),
                )));
            }
            if self.tool_loop_budget_low(round) {
                return Ok(ToolLoopExit::Exhausted(state));
            }

            match self
                .run_tool_loop_round(decision, llm, state, round, stream)
                .await?
            {
                ToolLoopStep::Continue(next_state) => state = next_state,
                ToolLoopStep::Break(next_state) => {
                    return Ok(ToolLoopExit::Exhausted(next_state));
                }
                ToolLoopStep::Return(action) => return Ok(ToolLoopExit::Return(action)),
            }
        }
        Ok(ToolLoopExit::Exhausted(state))
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
            ToolRoundOutcome::BudgetLow => Ok(ToolLoopStep::Break(state)),
            ToolRoundOutcome::DirectUtilityAnswered(response) => Ok(ToolLoopStep::Return(
                Box::new(self.direct_utility_action_result(decision, state, response)),
            )),
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
        let partial = stitched_response_text(
            &state.accumulated_text,
            tail.or_else(|| summarize_tool_progress(&state.all_tool_results)),
        );
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
        stream.phase(Phase::Synthesize);
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
        let (capped, round_deferred) = self.apply_fan_out_cap(&response.tool_calls);
        if !round_deferred.is_empty() {
            self.append_deferred_tool_results(
                &mut state,
                &round_deferred,
                response.tool_calls.len(),
            );
        }
        state.current_calls = capped;
        Ok(ToolLoopStep::Continue(state))
    }

    async fn finish_tool_loop_on_exhaustion(
        &self,
        decision: &Decision,
        state: ToolRoundState,
        llm: &dyn LlmProvider,
        stream: CycleStream<'_>,
    ) -> Result<ActionResult, LoopError> {
        let next_tool_scope = self.continuation_tool_scope_for_round(&state);
        self.synthesize_tool_fallback(decision, state, llm, stream, next_tool_scope)
            .await
    }

    async fn finalize_tool_response(
        &mut self,
        decision: &Decision,
        state: ToolRoundState,
        response: &CompletionResponse,
        llm: &dyn LlmProvider,
        stream: CycleStream<'_>,
        next_tool_scope: Option<ContinuationToolScope>,
    ) -> Result<ActionResult, LoopError> {
        let current_round_text = response_text_segment(response);
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
            },
        )
    }

    fn incomplete_tool_continuation_action(
        &self,
        decision: &Decision,
        state: ToolRoundState,
    ) -> ActionResult {
        let tool_summary = stitched_response_text(
            &state.accumulated_text,
            summarize_tool_progress(&state.all_tool_results),
        );
        self.incomplete_action_result(
            decision,
            state.all_tool_results,
            tool_summary,
            "tool continuation did not produce a usable final response",
            state.tokens_used,
        )
    }

    async fn synthesize_tool_fallback(
        &self,
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
            ..
        } = state;
        let max_tokens = self.budget.config().max_synthesis_tokens;
        let evicted = evict_oldest_results(all_tool_results, max_tokens);
        let synthesis_prompt = tool_synthesis_prompt(&evicted, &self.synthesis_instruction);
        stream.phase(Phase::Synthesize);
        let llm_text = self
            .generate_tool_summary(&synthesis_prompt, llm, stream, TextStreamVisibility::Hidden)
            .await?;
        tokens_used.accumulate(synthesis_usage(&synthesis_prompt, &llm_text));
        let synthesized_text = meaningful_response_text(&llm_text);
        let response_text = stitch_response_segments(&accumulated_text, synthesized_text.clone());
        let final_response = meaningful_response_text(&response_text);
        let tool_summary =
            stitched_response_text(&accumulated_text, summarize_tool_progress(&evicted));
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
                },
            ),
            None if self
                .turn_execution_profile
                .direct_inspection_profile()
                .is_some() =>
            {
                self.direct_inspection_empty_summary_action_result(decision, evicted, tokens_used)
            }
            None => self.incomplete_action_result(
                decision,
                evicted,
                tool_summary,
                "tool synthesis did not produce a usable final response",
                tokens_used,
            ),
        })
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
        } = payload;
        let turn_commitment = tool_continuation_turn_commitment(decision, next_tool_scope.as_ref());
        let artifact_write_target = tool_continuation_artifact_write_target(
            self.requested_artifact_target.as_deref(),
            next_tool_scope.as_ref(),
        );
        let continuation = if context_messages.is_empty() {
            ActionContinuation::new(Some(response.clone()), Some(response.clone()))
        } else {
            ActionContinuation::new(Some(response.clone()), None)
                .with_context_messages(context_messages)
        };
        let continuation = match next_tool_scope {
            Some(scope) => continuation.with_tool_scope(scope),
            None => continuation,
        };
        let continuation = match artifact_write_target {
            Some(path) => continuation.with_artifact_write_target(path),
            None => continuation,
        };
        let continuation = match turn_commitment {
            Some(commitment) => continuation.with_turn_commitment(commitment),
            None => continuation,
        };
        if self.turn_execution_profile.completes_terminally() {
            return ActionResult {
                decision: decision.clone(),
                tool_results,
                response_text,
                tokens_used,
                next_step: ActionNextStep::Finish(ActionTerminal::Complete { response }),
            };
        }
        ActionResult {
            decision: decision.clone(),
            tool_results,
            response_text,
            tokens_used,
            next_step: ActionNextStep::Continue(continuation),
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

    pub(super) async fn generate_tool_summary(
        &self,
        synthesis_prompt: &str,
        llm: &dyn LlmProvider,
        stream: CycleStream<'_>,
        text_visibility: TextStreamVisibility,
    ) -> Result<String, LoopError> {
        let chunks = Arc::new(Mutex::new(Vec::new()));
        let callback_chunks = Arc::clone(&chunks);
        let stream_callback = stream.callback.cloned();
        let callback = Box::new(move |chunk: String| {
            if let Ok(mut guard) = callback_chunks.lock() {
                guard.push(chunk.clone());
            }
            if matches!(text_visibility, TextStreamVisibility::Public) {
                if let Some(callback) = &stream_callback {
                    callback(super::StreamEvent::TextDelta { text: chunk });
                }
            }
        });
        let fallback = llm
            .generate_streaming(synthesis_prompt, TOOL_SYNTHESIS_MAX_OUTPUT_TOKENS, callback)
            .await
            .map_err(|error| {
                loop_error(
                    "act",
                    &format!("tool synthesis generation failed: {error}"),
                    true,
                )
            })?;
        let assembled = join_streamed_chunks(&chunks)?;
        if assembled.trim().is_empty() {
            Ok(fallback)
        } else {
            Ok(assembled)
        }
    }

    pub(super) fn apply_fan_out_cap(
        &mut self,
        calls: &[ToolCall],
    ) -> (Vec<ToolCall>, Vec<ToolCall>) {
        let max_fan_out = self.budget.config().max_fan_out;
        if calls.len() <= max_fan_out {
            return (calls.to_vec(), Vec::new());
        }

        let execute = calls[..max_fan_out].to_vec();
        let deferred = calls[max_fan_out..].to_vec();
        let deferred_names: Vec<&str> = deferred.iter().map(|call| call.name.as_str()).collect();
        self.emit_signal(
            LoopStep::Act,
            SignalKind::Friction,
            format!(
                "fan-out cap: executing {}/{}, deferring: {}",
                max_fan_out,
                calls.len(),
                deferred_names.join(", ")
            ),
            serde_json::json!({
                "executed": max_fan_out,
                "total": calls.len(),
                "deferred_tools": deferred_names,
            }),
        );
        (execute, deferred)
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
        for call in deferred {
            state.all_tool_results.push(ToolResult {
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                success: false,
                output: message.clone(),
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
            serde_json::json!({"reason": "budget_soft_ceiling"}),
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
        let compacted = {
            let compaction = self.compaction();
            compaction
                .compact_if_needed(messages, CompactionScope::ToolContinuation, round)
                .await?
        };
        if let Cow::Owned(compacted_messages) = compacted {
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
            serde_json::json!({"reason": "budget_soft_ceiling", "round": round}),
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
        let request = ToolRoundContinuationRequest {
            round,
            llm,
            continuation_tools,
            calls_count: executed.calls.len(),
            started_at_ms: executed.started_at_ms,
            stream,
        };
        self.record_executed_tool_round(state, executed)?;
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
        let results = self.execute_tool_calls_with_stream(&calls, stream).await?;
        self.publish_tool_round(&calls, &results, stream);
        let has_tool_errors = self.emit_tool_errors(&results, stream);
        self.record_tool_execution_cost(results.len());
        Ok(ExecutedToolRound {
            calls,
            results,
            has_tool_errors,
            started_at_ms,
        })
    }

    fn record_executed_tool_round(
        &mut self,
        state: &mut ToolRoundState,
        executed: ExecutedToolRound,
    ) -> Result<(), LoopError> {
        let ExecutedToolRound {
            calls,
            results,
            has_tool_errors,
            ..
        } = executed;
        self.record_successful_tool_classifications(state, &calls, &results);
        self.record_tool_round_result_bytes(&results);
        self.record_round_messages(state, &calls, &results, has_tool_errors)?;
        self.record_tool_round_kind(&calls);
        self.advance_bounded_local_phase_after_tool_round(&calls, &results);
        state.all_tool_results.extend(results);
        Ok(())
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
        has_tool_errors: bool,
    ) -> Result<(), LoopError> {
        record_tool_round_messages(
            &mut state.continuation_messages,
            &mut state.evidence_messages,
            calls,
            &self.tool_call_provider_ids,
            results,
        )?;
        if has_tool_errors {
            self.append_tool_error_relay(state, results);
        }
        Ok(())
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
            return Some(ToolRoundOutcome::DirectUtilityAnswered(response));
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
            return Ok(Some(ToolRoundOutcome::BudgetLow));
        }
        Ok(None)
    }

    async fn request_tool_round_response(
        &mut self,
        state: &mut ToolRoundState,
        request: ToolRoundContinuationRequest<'_>,
    ) -> Result<ToolRoundOutcome, LoopError> {
        request.stream.phase(Phase::Synthesize);
        let response = self
            .request_tool_continuation(
                request.llm,
                &state.continuation_messages,
                request.continuation_tools,
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
        let termination = self.current_termination_config();
        let config = termination.as_ref();
        let tool_nudge = u32::from(config.tool_round_nudge_after);
        let tool_strip = tool_nudge.saturating_add(u32::from(config.tool_round_strip_after_nudge));
        let observation_nudge = u32::from(config.observation_only_round_nudge_after);
        let observation_strip = observation_nudge
            .saturating_add(u32::from(config.observation_only_round_strip_after_nudge));
        let observation_rounds = u32::from(self.consecutive_observation_only_rounds);
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
        if !profile_owns_surface && observation_nudge > 0 && observation_rounds >= observation_strip
        {
            return self.side_effect_tool_definitions();
        }
        if tool_nudge > 0 && round >= tool_strip {
            self.progress_limited_tool_definitions()
        } else {
            all_tools
        }
    }

    pub(super) fn continuation_tool_scope_for_round(
        &self,
        state: &ToolRoundState,
    ) -> Option<ContinuationToolScope> {
        if self.turn_execution_profile.owns_tool_surface() {
            return None;
        }
        if state.used_observation_tools && !state.used_mutation_tools {
            let mutation_tools = self.side_effect_tool_definitions();
            if !mutation_tools.is_empty() {
                return Some(ContinuationToolScope::MutationOnly);
            }
        }
        None
    }

    fn observation_only_call_restriction_active(&self) -> bool {
        let termination = self.current_termination_config();
        let config = termination.as_ref();
        let nudge_threshold = u32::from(config.observation_only_round_nudge_after);
        let strip_threshold = nudge_threshold
            .saturating_add(u32::from(config.observation_only_round_strip_after_nudge));
        nudge_threshold > 0
            && u32::from(self.consecutive_observation_only_rounds) >= strip_threshold
    }

    pub(super) fn record_tool_round_kind(&mut self, calls: &[ToolCall]) {
        let observation_only = !calls.is_empty()
            && calls.iter().all(|call| {
                self.tool_executor.classify_call(call) == ToolCallClassification::Observation
            });
        if observation_only {
            self.consecutive_observation_only_rounds =
                self.consecutive_observation_only_rounds.saturating_add(1);
        } else {
            self.consecutive_observation_only_rounds = 0;
        }
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
        let mut request = build_continuation_request(ContinuationRequestParams::new(
            context_messages,
            llm.model_name(),
            ToolRequestConfig::new(continuation_tools, self.effective_decompose_enabled()),
            RequestBuildContext::new(
                self.memory_context.as_deref(),
                self.scratchpad_context.as_deref(),
                self.thinking_config.clone(),
                self.notify_tool_guidance_enabled,
            ),
        ));
        if let Some(directive) = self.turn_execution_profile_directive() {
            if let Some(system_prompt) = request.system_prompt.as_mut() {
                system_prompt.push_str(&directive);
            }
        }

        let response = self
            .request_completion(
                llm,
                request,
                StreamingRequestContext::new(
                    "act",
                    StreamPhase::Synthesize,
                    TextStreamVisibility::Hidden,
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

fn collect_valid_tool_calls(
    allowed: &[ToolCall],
    malformed_results: &mut Vec<ToolResult>,
) -> Vec<ToolCall> {
    allowed
        .iter()
        .filter_map(|call| {
            if call.arguments.get("__fawx_raw_args").is_some() {
                tracing::warn!(
                    tool = %call.name,
                    "skipping tool call with malformed arguments"
                );
                malformed_results.push(ToolResult {
                    tool_call_id: call.id.clone(),
                    tool_name: call.name.clone(),
                    success: false,
                    output: "Tool call failed: arguments could not be parsed as valid JSON".into(),
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
            blocked.push(BlockedToolCall {
                call: call.clone(),
                reason: reason.to_string(),
            });
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
            blocked.push(BlockedToolCall {
                call: call.clone(),
                reason: reason.to_string(),
            });
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
        .map(|call| BlockedToolCall {
            call,
            reason: reason.to_string(),
        })
        .collect()
}

pub(super) fn blocked_tool_message(tool_name: &str, reason: &str) -> String {
    format!(
        "Tool '{}' blocked: {}. Try a different approach.",
        tool_name, reason
    )
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
        .map(|blocked_call| ToolResult {
            tool_call_id: blocked_call.call.id.clone(),
            tool_name: blocked_call.call.name.clone(),
            success: false,
            output: blocked_tool_message(&blocked_call.call.name, &blocked_call.reason),
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

fn join_streamed_chunks(chunks: &Arc<Mutex<Vec<String>>>) -> Result<String, LoopError> {
    let parts = chunks
        .lock()
        .map_err(|_| loop_error("act", "tool synthesis stream collection failed", true))?;
    Ok(parts.join(""))
}

fn synthesis_usage(prompt: &str, response: &str) -> TokenUsage {
    TokenUsage {
        input_tokens: estimate_tokens(prompt),
        output_tokens: estimate_tokens(response),
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
    use crate::budget::{BudgetConfig, BudgetTracker};
    use crate::cancellation::CancellationToken;
    use crate::context_manager::ContextCompactor;
    use async_trait::async_trait;
    use fx_llm::ToolDefinition;
    use std::sync::Arc;

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
            .execute_tool_calls_with_stream(&calls, CycleStream::disabled())
            .await
            .expect("execute tool calls");

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].tool_call_id, "call-1");
        assert_eq!(results[1].tool_call_id, "call-2");
        assert!(!results[0].success);
        assert!(results[1].success);
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
