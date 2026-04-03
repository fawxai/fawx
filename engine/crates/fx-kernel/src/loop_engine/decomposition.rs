use super::{
    action_partial_response, append_continuation_context, build_user_message, current_time_ms,
    extract_user_message, loop_error, meaningful_response_text, truncate_prompt_text,
    CompactionScope, CycleStream, DecomposeToolArguments, DirectInspectionOwnership,
    ExecutionVisibility, LlmProvider, LoopEngine, LoopEngineBuilder, LoopResult,
    DECOMPOSITION_DEPTH_LIMIT_RESPONSE, DECOMPOSITION_RESULTS_PREFIX, MAX_SUB_GOALS,
};
use crate::act::{
    ActionContinuation, ActionNextStep, ActionResult, TokenUsage, ToolCacheability, ToolExecutor,
};
use crate::budget::{
    build_skip_mask, effective_max_depth, estimate_complexity, ActionCost, AllocationMode,
    AllocationPlan, BudgetAllocator, BudgetConfig, BudgetRemaining, BudgetState, BudgetTracker,
    DepthMode, DEFAULT_LLM_CALL_COST_CENTS, DEFAULT_TOOL_INVOCATION_COST_CENTS,
};
use crate::decide::Decision;
use crate::scoped_tool_executor::scope_tool_executor;
use crate::signals::{LoopStep, SignalKind};
use crate::types::{LoopError, PerceptionSnapshot};
use fx_core::types::{InputSource, ScreenState, UserInput};
use fx_decompose::{
    AggregationStrategy, ComplexityHint, DecompositionPlan, ExecutionContract, SubGoal,
    SubGoalOutcome, SubGoalResult,
};
use fx_llm::{CompletionResponse, Message, ToolDefinition};
use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Debug)]
pub(super) struct SubGoalExecution {
    pub(super) result: SubGoalResult,
    pub(super) budget: BudgetTracker,
}

#[derive(Clone, Copy)]
struct SubGoalRunContext<'a> {
    llm: &'a dyn LlmProvider,
    context_messages: &'a [Message],
}

struct SubGoalRunRequest<'a> {
    sub_goal: &'a SubGoal,
    child_config: BudgetConfig,
    prior_results: &'a [SubGoalResult],
}

struct SequentialSubGoalContext<'a> {
    allocation: &'a AllocationPlan,
    skipped: &'a [bool],
    run: SubGoalRunContext<'a>,
}

struct SequentialSubGoalRequest<'a> {
    index: usize,
    total: usize,
    sub_goal: &'a SubGoal,
    prior_results: &'a [SubGoalResult],
}

struct ConcurrentSubGoalContext<'a> {
    sub_goal_budgets: &'a [BudgetConfig],
    skipped: &'a [bool],
    run: SubGoalRunContext<'a>,
}

struct SubGoalRetryContext {
    initial_response: String,
    initial_signals: Vec<super::Signal>,
    required_tool_names: Vec<String>,
}

pub(super) type IndexedSubGoalExecution = (usize, SubGoalExecution);

#[derive(Debug, Clone, PartialEq, Eq)]
enum SubGoalCompletionCheck {
    Valid,
    MissingRequiredSideEffectTools {
        message: String,
        tool_names: Vec<String>,
    },
    Incomplete(String),
}

#[derive(Debug)]
enum FollowUpRoundResult {
    Terminal(LoopResult),
    Continue(ActionContinuation),
}

enum FollowUpOutcome {
    Loop(LoopResult),
    Result(SubGoalResult),
}

const SUB_GOAL_MUTATION_RETRY_INCOMPLETE_REASON: &str =
    "sub-goal required a bounded mutation retry but still did not execute the required work";
const SUB_GOAL_MUTATION_RETRY_FOLLOW_UP_REASON: &str =
    "sub-goal follow-up still required another reasoning pass after the bounded mutation retry";

impl LoopEngine {
    pub(super) async fn execute_decomposition(
        &mut self,
        decision: &Decision,
        plan: &DecompositionPlan,
        llm: &dyn LlmProvider,
        context_messages: &[Message],
    ) -> Result<ActionResult, LoopError> {
        if self.budget.state() == BudgetState::Low {
            return Ok(self.budget_low_blocked_result(decision, "decomposition", &[]));
        }
        let timestamp_ms = current_time_ms();
        let effective_cap =
            self.effective_decomposition_depth_cap(&self.budget.remaining(timestamp_ms));
        if self.decomposition_depth_limited(effective_cap) {
            return Ok(self.depth_limited_decomposition_result(decision));
        }
        self.emit_decomposition_truncation(plan);
        let allocation = self.prepare_allocation_plan(plan, timestamp_ms, effective_cap);
        let results = self
            .execute_allocated_sub_goals(plan, &allocation, llm, context_messages)
            .await;
        Ok(build_decomposition_action(
            decision,
            aggregate_sub_goal_results(&results),
        ))
    }

    fn emit_decomposition_truncation(&mut self, plan: &DecompositionPlan) {
        if let Some(original_sub_goals) = plan.truncated_from {
            self.emit_decomposition_truncation_signal(original_sub_goals, plan.sub_goals.len());
        }
    }

    fn prepare_allocation_plan(
        &self,
        plan: &DecompositionPlan,
        timestamp_ms: u64,
        effective_cap: u32,
    ) -> AllocationPlan {
        let mode = allocation_mode_for_strategy(&plan.strategy);
        let mut allocation =
            BudgetAllocator::new().allocate(&self.budget, &plan.sub_goals, mode, timestamp_ms);
        self.apply_effective_depth_cap(&mut allocation.sub_goal_budgets, effective_cap);
        allocation
    }

    pub(super) fn decomposition_depth_limited(&self, effective_cap: u32) -> bool {
        self.budget.depth() >= effective_cap
    }

    pub(super) fn effective_decomposition_depth_cap(&self, remaining: &BudgetRemaining) -> u32 {
        let config = self.budget.config();
        match config.decompose_depth_mode {
            DepthMode::Static => config.max_recursion_depth,
            DepthMode::Adaptive => config
                .max_recursion_depth
                .min(effective_max_depth(remaining)),
        }
    }

    pub(super) fn apply_effective_depth_cap(
        &self,
        sub_goal_budgets: &mut [BudgetConfig],
        effective_cap: u32,
    ) {
        for budget in sub_goal_budgets {
            budget.max_recursion_depth = budget.max_recursion_depth.min(effective_cap);
        }
    }

    pub(super) fn zero_sub_goal_budget(&self) -> BudgetConfig {
        let template = self.budget.config();
        BudgetConfig {
            max_llm_calls: 0,
            max_tool_invocations: 0,
            max_tokens: 0,
            max_cost_cents: 0,
            max_wall_time_ms: 0,
            max_recursion_depth: template.max_recursion_depth,
            decompose_depth_mode: template.decompose_depth_mode,
            soft_ceiling_percent: template.soft_ceiling_percent,
            max_fan_out: template.max_fan_out,
            max_tool_result_bytes: template.max_tool_result_bytes,
            max_aggregate_result_bytes: template.max_aggregate_result_bytes,
            max_synthesis_tokens: template.max_synthesis_tokens,
            max_consecutive_failures: template.max_consecutive_failures,
            max_cycle_failures: template.max_cycle_failures,
            max_no_progress: template.max_no_progress,
            max_tool_retries: template.max_tool_retries,
            termination: template.termination.clone(),
        }
    }

    fn depth_limited_decomposition_result(&mut self, decision: &Decision) -> ActionResult {
        self.emit_signal(
            LoopStep::Act,
            SignalKind::Blocked,
            "task decomposition blocked by recursion depth",
            serde_json::json!({"reason": "max recursion depth reached"}),
        );
        self.text_action_result(decision, DECOMPOSITION_DEPTH_LIMIT_RESPONSE)
    }

    async fn execute_allocated_sub_goals(
        &mut self,
        plan: &DecompositionPlan,
        allocation: &AllocationPlan,
        llm: &dyn LlmProvider,
        context_messages: &[Message],
    ) -> Vec<SubGoalResult> {
        match &plan.strategy {
            AggregationStrategy::Parallel => {
                self.execute_sub_goals_concurrent(plan, allocation, llm, context_messages)
                    .await
            }
            AggregationStrategy::Sequential => {
                self.execute_sub_goals_sequential(plan, allocation, llm, context_messages)
                    .await
            }
            AggregationStrategy::Custom(strategy) => {
                unreachable!("custom strategy '{strategy}' should be rejected during parsing")
            }
        }
    }

    async fn execute_sub_goals_sequential(
        &mut self,
        plan: &DecompositionPlan,
        allocation: &AllocationPlan,
        llm: &dyn LlmProvider,
        context_messages: &[Message],
    ) -> Vec<SubGoalResult> {
        let total = plan.sub_goals.len();
        let skipped = build_skip_mask(total, &allocation.skipped_indices);
        let context = SequentialSubGoalContext {
            allocation,
            skipped: &skipped,
            run: SubGoalRunContext {
                llm,
                context_messages,
            },
        };
        let mut results = Vec::with_capacity(total);

        for (index, sub_goal) in plan.sub_goals.iter().enumerate() {
            let request = SequentialSubGoalRequest {
                index,
                total,
                sub_goal,
                prior_results: &results,
            };
            let result = self.execute_sequential_sub_goal(request, &context).await;
            if self.record_sequential_sub_goal(index, total, result, &mut results) {
                break;
            }
        }

        results
    }

    async fn execute_sequential_sub_goal(
        &mut self,
        request: SequentialSubGoalRequest<'_>,
        context: &SequentialSubGoalContext<'_>,
    ) -> SubGoalResult {
        let description = &request.sub_goal.description;
        self.emit_sub_goal_progress(request.index, request.total, description);
        if context.skipped.get(request.index).copied().unwrap_or(false) {
            self.emit_sub_goal_skipped(request.index, request.total, description);
            return skipped_sub_goal_result(request.sub_goal.clone());
        }

        let execution = self
            .run_sub_goal_request(
                SubGoalRunRequest {
                    sub_goal: request.sub_goal,
                    child_config: child_budget_config(
                        &context.allocation.sub_goal_budgets,
                        request.index,
                        self.zero_sub_goal_budget(),
                    ),
                    prior_results: request.prior_results,
                },
                context.run,
            )
            .await;
        self.record_sub_goal_execution(&execution);
        execution.result
    }

    fn record_sub_goal_execution(&mut self, execution: &SubGoalExecution) {
        self.budget.absorb_child_usage(&execution.budget);
        self.roll_up_sub_goal_signals(&execution.result.signals);
    }

    fn record_sequential_sub_goal(
        &mut self,
        index: usize,
        total: usize,
        result: SubGoalResult,
        results: &mut Vec<SubGoalResult>,
    ) -> bool {
        let should_halt = should_halt_sub_goal_sequence(&result);
        let exhausted_with_partial =
            matches!(result.outcome, SubGoalOutcome::BudgetExhausted { .. }) && !should_halt;
        self.emit_sub_goal_completed(index, total, &result);
        results.push(result);
        self.emit_sequence_budget_trace(index, total, should_halt, exhausted_with_partial);
        should_halt
    }

    fn emit_sequence_budget_trace(
        &mut self,
        index: usize,
        total: usize,
        should_halt: bool,
        exhausted_with_partial: bool,
    ) {
        if should_halt {
            self.emit_signal(
                LoopStep::Act,
                SignalKind::Trace,
                "stopping remaining sub-goals after budget exhaustion",
                serde_json::json!({"completed_sub_goals": index + 1, "total_sub_goals": total}),
            );
            return;
        }
        if exhausted_with_partial {
            self.emit_signal(
                LoopStep::Act,
                SignalKind::Trace,
                "continuing remaining sub-goals after partial budget exhaustion",
                serde_json::json!({"completed_sub_goals": index + 1, "total_sub_goals": total}),
            );
        }
    }

    pub(super) async fn execute_sub_goals_concurrent(
        &mut self,
        plan: &DecompositionPlan,
        allocation: &AllocationPlan,
        llm: &dyn LlmProvider,
        context_messages: &[Message],
    ) -> Vec<SubGoalResult> {
        let total = plan.sub_goals.len();
        let skipped = build_skip_mask(total, &allocation.skipped_indices);

        for (index, sub_goal) in plan.sub_goals.iter().enumerate() {
            self.emit_sub_goal_progress(index, total, &sub_goal.description);
        }

        let executions = futures_util::future::join_all(self.build_concurrent_futures(
            plan,
            ConcurrentSubGoalContext {
                sub_goal_budgets: &allocation.sub_goal_budgets,
                skipped: &skipped,
                run: SubGoalRunContext {
                    llm,
                    context_messages,
                },
            },
        ))
        .await;
        self.collect_concurrent_results(plan, executions, &skipped)
    }

    fn build_concurrent_futures<'a>(
        &'a self,
        plan: &'a DecompositionPlan,
        context: ConcurrentSubGoalContext<'a>,
    ) -> Vec<impl std::future::Future<Output = IndexedSubGoalExecution> + 'a> {
        plan.sub_goals
            .iter()
            .enumerate()
            .filter_map(|(index, sub_goal)| {
                if context.skipped.get(index).copied().unwrap_or(false) {
                    return None;
                }

                let goal = sub_goal.clone();
                let child_config = child_budget_config(
                    context.sub_goal_budgets,
                    index,
                    self.zero_sub_goal_budget(),
                );
                Some(async move {
                    let execution = self
                        .run_sub_goal_request(
                            SubGoalRunRequest {
                                sub_goal: &goal,
                                child_config,
                                prior_results: &[],
                            },
                            context.run,
                        )
                        .await;
                    (index, execution)
                })
            })
            .collect()
    }

    pub(super) fn collect_concurrent_results(
        &mut self,
        plan: &DecompositionPlan,
        executions: Vec<IndexedSubGoalExecution>,
        skipped: &[bool],
    ) -> Vec<SubGoalResult> {
        let total = plan.sub_goals.len();
        let mut ordered = vec![None; total];
        self.fill_skipped_concurrent_results(plan, total, skipped, &mut ordered);

        for (index, execution) in executions {
            self.record_sub_goal_execution(&execution);
            self.emit_sub_goal_completed(index, total, &execution.result);
            if let Some(slot) = ordered.get_mut(index) {
                *slot = Some(execution.result);
            }
        }

        ordered
            .into_iter()
            .enumerate()
            .filter_map(|(index, maybe_result)| {
                debug_assert!(
                    maybe_result.is_some() || skipped.get(index).copied().unwrap_or(false),
                    "unexpected missing result at index {index}"
                );
                maybe_result.or_else(|| {
                    plan.sub_goals
                        .get(index)
                        .cloned()
                        .map(skipped_sub_goal_result)
                })
            })
            .collect()
    }

    fn fill_skipped_concurrent_results(
        &mut self,
        plan: &DecompositionPlan,
        total: usize,
        skipped: &[bool],
        ordered: &mut [Option<SubGoalResult>],
    ) {
        for (index, slot) in ordered.iter_mut().enumerate().take(total) {
            if !skipped.get(index).copied().unwrap_or(false) {
                continue;
            }
            if let Some(goal) = plan.sub_goals.get(index) {
                self.emit_sub_goal_skipped(index, total, &goal.description);
                let result = skipped_sub_goal_result(goal.clone());
                self.emit_sub_goal_completed(index, total, &result);
                *slot = Some(result);
            }
        }
    }

    fn emit_sub_goal_completed(&self, index: usize, total: usize, result: &SubGoalResult) {
        let success = matches!(result.outcome, SubGoalOutcome::Completed(_));
        if let Some(bus) = self.public_event_bus() {
            let _ = bus.publish(fx_core::message::InternalMessage::SubGoalCompleted {
                index,
                total,
                success,
            });
        }
    }

    #[cfg(test)]
    pub(super) async fn run_sub_goal(
        &self,
        sub_goal: &SubGoal,
        child_config: BudgetConfig,
        llm: &dyn LlmProvider,
        context_messages: &[Message],
        prior_results: &[SubGoalResult],
    ) -> SubGoalExecution {
        self.run_sub_goal_request(
            SubGoalRunRequest {
                sub_goal,
                child_config,
                prior_results,
            },
            SubGoalRunContext {
                llm,
                context_messages,
            },
        )
        .await
    }

    async fn run_sub_goal_request(
        &self,
        request: SubGoalRunRequest<'_>,
        run_context: SubGoalRunContext<'_>,
    ) -> SubGoalExecution {
        let timestamp_ms = current_time_ms();
        let (mut child, snapshot) = match self
            .prepare_sub_goal_run(&request, run_context.context_messages, timestamp_ms)
            .await
        {
            Ok(values) => values,
            Err(execution) => return execution,
        };
        let result = self
            .execute_prepared_sub_goal(&mut child, request.sub_goal, &snapshot, run_context.llm)
            .await;
        SubGoalExecution {
            result,
            budget: child.budget,
        }
    }

    async fn prepare_sub_goal_run(
        &self,
        request: &SubGoalRunRequest<'_>,
        context_messages: &[Message],
        timestamp_ms: u64,
    ) -> Result<(LoopEngine, PerceptionSnapshot), SubGoalExecution> {
        let child_budget = self.sub_goal_budget_tracker(request.child_config.clone(), timestamp_ms);
        let compacted_context = self
            .compact_sub_goal_context(request.sub_goal, child_budget.clone(), context_messages)
            .await?;
        let snapshot = build_sub_goal_snapshot(
            request.sub_goal,
            request.prior_results,
            compacted_context.as_ref(),
            timestamp_ms,
        );
        let child = self
            .build_child_engine(request.sub_goal, child_budget.clone())
            .map_err(|error| {
                failed_sub_goal_execution(request.sub_goal, error.reason, child_budget)
            })?;
        Ok((child, snapshot))
    }

    fn sub_goal_budget_tracker(
        &self,
        child_config: BudgetConfig,
        timestamp_ms: u64,
    ) -> BudgetTracker {
        BudgetTracker::new(child_config, timestamp_ms, self.budget.child_depth())
    }

    async fn execute_prepared_sub_goal(
        &self,
        child: &mut LoopEngine,
        sub_goal: &SubGoal,
        snapshot: &PerceptionSnapshot,
        llm: &dyn LlmProvider,
    ) -> SubGoalResult {
        let retry_snapshot = snapshot.clone();
        match Box::pin(child.run_cycle(snapshot.clone(), llm)).await {
            Ok(LoopResult::Complete {
                response, signals, ..
            }) => {
                self.completed_sub_goal_result(
                    child,
                    sub_goal,
                    &retry_snapshot,
                    llm,
                    response,
                    signals,
                )
                .await
            }
            Ok(result) => sub_goal_result_from_loop(sub_goal.clone(), result),
            Err(error) => failed_sub_goal_result(sub_goal.clone(), error.reason),
        }
    }

    async fn completed_sub_goal_result(
        &self,
        child: &mut LoopEngine,
        sub_goal: &SubGoal,
        snapshot: &PerceptionSnapshot,
        llm: &dyn LlmProvider,
        response: String,
        signals: Vec<super::Signal>,
    ) -> SubGoalResult {
        match self.check_sub_goal_completion(sub_goal, &signals, &response) {
            SubGoalCompletionCheck::Valid => {
                completed_sub_goal_result(sub_goal.clone(), response, signals)
            }
            SubGoalCompletionCheck::MissingRequiredSideEffectTools { tool_names, .. } => {
                self.retry_sub_goal_required_side_effect_completion(
                    child,
                    sub_goal,
                    snapshot,
                    llm,
                    SubGoalRetryContext {
                        initial_response: response,
                        initial_signals: signals,
                        required_tool_names: tool_names,
                    },
                )
                .await
            }
            SubGoalCompletionCheck::Incomplete(message) => {
                incomplete_sub_goal_result_with_signals(sub_goal.clone(), message, signals)
            }
        }
    }

    async fn retry_sub_goal_required_side_effect_completion(
        &self,
        child: &mut LoopEngine,
        sub_goal: &SubGoal,
        snapshot: &PerceptionSnapshot,
        llm: &dyn LlmProvider,
        retry: SubGoalRetryContext,
    ) -> SubGoalResult {
        let continuation_tools =
            self.required_side_effect_sub_goal_tools(&retry.required_tool_names);
        if continuation_tools.is_empty() {
            return missing_side_effect_retry_tools_result(sub_goal, retry);
        }

        let continuation_messages = self.sub_goal_retry_messages(
            child,
            snapshot,
            &retry.initial_response,
            &retry.required_tool_names,
        );
        child.last_reasoning_messages = continuation_messages.clone();
        let follow_up = self
            .follow_up_retry_result(
                child,
                sub_goal,
                llm,
                &continuation_messages,
                continuation_tools,
                &retry.initial_signals,
            )
            .await;
        merge_sub_goal_signals(follow_up, retry.initial_signals)
    }

    async fn follow_up_retry_result(
        &self,
        child: &mut LoopEngine,
        sub_goal: &SubGoal,
        llm: &dyn LlmProvider,
        continuation_messages: &[Message],
        continuation_tools: Vec<ToolDefinition>,
        initial_signals: &[super::Signal],
    ) -> SubGoalResult {
        match self
            .run_bounded_sub_goal_follow_up(
                child,
                sub_goal,
                llm,
                continuation_messages,
                continuation_tools,
            )
            .await
        {
            Ok(result) => result,
            Err(error) => failed_sub_goal_result_with_signals(
                sub_goal.clone(),
                error.reason,
                initial_signals.to_vec(),
            ),
        }
    }

    async fn run_bounded_sub_goal_follow_up(
        &self,
        child: &mut LoopEngine,
        sub_goal: &SubGoal,
        llm: &dyn LlmProvider,
        continuation_messages: &[Message],
        continuation_tools: Vec<ToolDefinition>,
    ) -> Result<SubGoalResult, LoopError> {
        let outcome = self
            .follow_up_outcome(
                child,
                sub_goal,
                llm,
                continuation_messages,
                continuation_tools,
            )
            .await?;
        Ok(match outcome {
            FollowUpOutcome::Result(result) => result,
            FollowUpOutcome::Loop(loop_result) => {
                let loop_result = child.finalize_result(loop_result);
                self.sub_goal_result_from_follow_up(sub_goal, loop_result)
            }
        })
    }

    async fn follow_up_outcome(
        &self,
        child: &mut LoopEngine,
        sub_goal: &SubGoal,
        llm: &dyn LlmProvider,
        continuation_messages: &[Message],
        continuation_tools: Vec<ToolDefinition>,
    ) -> Result<FollowUpOutcome, LoopError> {
        let first_round = self
            .execute_bounded_sub_goal_follow_up_round(
                child,
                llm,
                continuation_messages,
                &continuation_tools,
            )
            .await?;
        match first_round {
            FollowUpRoundResult::Terminal(loop_result) => Ok(FollowUpOutcome::Loop(loop_result)),
            FollowUpRoundResult::Continue(continuation) => {
                self.follow_up_outcome_from_continuation(
                    child,
                    sub_goal,
                    llm,
                    continuation_messages,
                    &continuation_tools,
                    continuation,
                )
                .await
            }
        }
    }

    async fn follow_up_outcome_from_continuation(
        &self,
        child: &mut LoopEngine,
        sub_goal: &SubGoal,
        llm: &dyn LlmProvider,
        continuation_messages: &[Message],
        continuation_tools: &[ToolDefinition],
        continuation: ActionContinuation,
    ) -> Result<FollowUpOutcome, LoopError> {
        if let Some(result) = self.partial_follow_up_result(child, sub_goal, &continuation) {
            return Ok(FollowUpOutcome::Result(result));
        }

        let follow_up_messages = build_follow_up_messages(continuation_messages, &continuation);
        let loop_result = match self
            .execute_bounded_sub_goal_follow_up_round(
                child,
                llm,
                &follow_up_messages,
                continuation_tools,
            )
            .await?
        {
            FollowUpRoundResult::Terminal(loop_result) => loop_result,
            FollowUpRoundResult::Continue(continuation) => LoopResult::Incomplete {
                partial_response: continuation.partial_response,
                reason: SUB_GOAL_MUTATION_RETRY_FOLLOW_UP_REASON.to_string(),
                iterations: child.iteration_count,
                signals: Vec::new(),
            },
        };
        Ok(FollowUpOutcome::Loop(loop_result))
    }

    fn partial_follow_up_result(
        &self,
        child: &LoopEngine,
        sub_goal: &SubGoal,
        continuation: &ActionContinuation,
    ) -> Option<SubGoalResult> {
        let response = continuation
            .partial_response
            .as_deref()
            .and_then(meaningful_response_text)?;
        let signals = child.signals.signals().to_vec();
        Some(
            match self.check_sub_goal_completion(sub_goal, &signals, &response) {
                SubGoalCompletionCheck::Valid => {
                    completed_sub_goal_result(sub_goal.clone(), response, signals)
                }
                SubGoalCompletionCheck::MissingRequiredSideEffectTools { message, .. }
                | SubGoalCompletionCheck::Incomplete(message) => {
                    incomplete_sub_goal_result_with_signals(
                        sub_goal.clone(),
                        meaningful_response_text(&response).unwrap_or(message),
                        signals,
                    )
                }
            },
        )
    }

    async fn execute_bounded_sub_goal_follow_up_round(
        &self,
        child: &mut LoopEngine,
        llm: &dyn LlmProvider,
        continuation_messages: &[Message],
        continuation_tools: &[ToolDefinition],
    ) -> Result<FollowUpRoundResult, LoopError> {
        let action = self
            .follow_up_round_action(child, llm, continuation_messages, continuation_tools)
            .await?;
        let action_partial = action_partial_response(&action);
        Ok(match action.next_step {
            ActionNextStep::Finish(terminal) => FollowUpRoundResult::Terminal(
                child.loop_result_from_action_terminal(terminal, action.tokens_used),
            ),
            ActionNextStep::Continue(continuation) => FollowUpRoundResult::Continue(
                continuation_with_action_partial(continuation, action_partial),
            ),
        })
    }

    async fn follow_up_round_action(
        &self,
        child: &mut LoopEngine,
        llm: &dyn LlmProvider,
        continuation_messages: &[Message],
        continuation_tools: &[ToolDefinition],
    ) -> Result<ActionResult, LoopError> {
        let response = self
            .follow_up_round_response(child, llm, continuation_messages, continuation_tools)
            .await?;
        let decision = child.decide(&response).await?;
        self.execute_follow_up_action(child, &decision, llm, continuation_messages)
            .await
    }

    async fn follow_up_round_response(
        &self,
        child: &mut LoopEngine,
        llm: &dyn LlmProvider,
        continuation_messages: &[Message],
        continuation_tools: &[ToolDefinition],
    ) -> Result<CompletionResponse, LoopError> {
        let mut tokens_used = TokenUsage::default();
        let response = child
            .request_tool_continuation(
                llm,
                continuation_messages,
                continuation_tools.to_vec(),
                &mut tokens_used,
                CycleStream::disabled(),
            )
            .await?;
        child.record_continuation_cost(&response, continuation_messages);
        child
            .continue_truncated_response(
                response,
                continuation_messages,
                llm,
                LoopStep::Act,
                CycleStream::disabled(),
            )
            .await
    }

    async fn execute_follow_up_action(
        &self,
        child: &mut LoopEngine,
        decision: &Decision,
        llm: &dyn LlmProvider,
        continuation_messages: &[Message],
    ) -> Result<ActionResult, LoopError> {
        let action = Box::pin(child.act(
            decision,
            llm,
            continuation_messages,
            CycleStream::disabled(),
        ))
        .await?;
        child.emit_action_observations(&action);
        child.record_action_cost_if_present(&action);
        Ok(action)
    }

    fn record_action_cost_if_present(&mut self, action: &ActionResult) {
        if let Some(action_cost) = self.recorded_action_cost(action) {
            self.budget.record(&action_cost);
        }
    }

    fn sub_goal_result_from_follow_up(
        &self,
        sub_goal: &SubGoal,
        result: LoopResult,
    ) -> SubGoalResult {
        match result {
            LoopResult::Complete {
                response, signals, ..
            } => self.completed_follow_up_sub_goal_result(sub_goal, response, signals),
            LoopResult::Incomplete {
                partial_response,
                reason,
                signals,
                ..
            } => incomplete_sub_goal_result_with_signals(
                sub_goal.clone(),
                partial_response.unwrap_or(reason),
                signals,
            ),
            LoopResult::BudgetExhausted {
                partial_response,
                signals,
                ..
            } => SubGoalResult {
                goal: sub_goal.clone(),
                outcome: SubGoalOutcome::BudgetExhausted { partial_response },
                signals,
            },
            LoopResult::Error {
                message, signals, ..
            } => failed_sub_goal_result_with_signals(sub_goal.clone(), message, signals),
            LoopResult::UserStopped { signals, .. } => incomplete_sub_goal_result_with_signals(
                sub_goal.clone(),
                "sub-goal stopped before completion".to_string(),
                signals,
            ),
        }
    }

    fn completed_follow_up_sub_goal_result(
        &self,
        sub_goal: &SubGoal,
        response: String,
        signals: Vec<super::Signal>,
    ) -> SubGoalResult {
        match self.check_sub_goal_completion(sub_goal, &signals, &response) {
            SubGoalCompletionCheck::Valid => {
                completed_sub_goal_result(sub_goal.clone(), response, signals)
            }
            SubGoalCompletionCheck::MissingRequiredSideEffectTools { message, .. }
            | SubGoalCompletionCheck::Incomplete(message) => {
                incomplete_follow_up_result(sub_goal, response, message, signals)
            }
        }
    }

    async fn compact_sub_goal_context<'a>(
        &self,
        sub_goal: &SubGoal,
        child_budget: BudgetTracker,
        context_messages: &'a [Message],
    ) -> Result<Cow<'a, [Message]>, SubGoalExecution> {
        let compacted_context = self
            .compaction()
            .compact_if_needed(
                context_messages,
                CompactionScope::DecomposeChild,
                self.iteration_count,
            )
            .await
            .map_err(|error| {
                failed_sub_goal_execution(sub_goal, error.reason, child_budget.clone())
            })?;

        self.compaction()
            .ensure_within_hard_limit(CompactionScope::DecomposeChild, compacted_context.as_ref())
            .map_err(|error| {
                failed_sub_goal_execution(sub_goal, error.reason, child_budget.clone())
            })?;
        Ok(compacted_context)
    }

    pub(super) fn build_child_engine(
        &self,
        sub_goal: &SubGoal,
        budget: BudgetTracker,
    ) -> Result<LoopEngine, LoopError> {
        let child_executor = self.child_tool_executor(sub_goal);
        let builder = self.child_engine_builder(sub_goal, budget, child_executor);
        let mut child = builder.build()?;
        self.configure_child_engine(&mut child);
        Ok(child)
    }

    fn child_tool_executor(&self, sub_goal: &SubGoal) -> Arc<dyn ToolExecutor> {
        if sub_goal.required_tools.is_empty() {
            Arc::clone(&self.tool_executor)
        } else {
            scope_tool_executor(Arc::clone(&self.tool_executor), &sub_goal.required_tools)
        }
    }

    fn child_engine_builder(
        &self,
        sub_goal: &SubGoal,
        budget: BudgetTracker,
        child_executor: Arc<dyn ToolExecutor>,
    ) -> LoopEngineBuilder {
        let builder = LoopEngine::builder()
            .budget(budget)
            .context(self.context.clone())
            .max_iterations(child_max_iterations(self.max_iterations))
            .tool_executor(child_executor)
            .synthesis_instruction(self.synthesis_instruction.clone())
            .compaction_config(self.compaction_config.clone())
            .allow_decompose(sub_goal.required_tools.is_empty())
            .execution_visibility(ExecutionVisibility::Internal)
            .session_memory(Arc::clone(&self.session_memory));
        self.with_child_optional_contexts(builder)
    }

    fn with_child_optional_contexts(&self, mut builder: LoopEngineBuilder) -> LoopEngineBuilder {
        if let Some(memory_context) = &self.memory_context {
            builder = builder.memory_context(memory_context.clone());
        }
        if let Some(scratchpad_context) = &self.scratchpad_context {
            builder = builder.scratchpad_context(scratchpad_context.clone());
        }
        if let Some(provider) = &self.scratchpad_provider {
            builder = builder.scratchpad_provider(Arc::clone(provider));
        }
        if let Some(counter) = &self.iteration_counter {
            builder = builder.iteration_counter(Arc::clone(counter));
        }
        if let Some(cancel_token) = &self.cancel_token {
            builder = builder.cancel_token(cancel_token.clone());
        }
        if let Some(bus) = &self.event_bus {
            builder = builder.event_bus(bus.clone());
        }
        builder
    }

    fn configure_child_engine(&self, child: &mut LoopEngine) {
        child.notify_tool_guidance_enabled = self.notify_tool_guidance_enabled;
        child.direct_inspection_ownership = DirectInspectionOwnership::PreserveParent(
            self.turn_execution_profile.direct_inspection_profile(),
        );
    }

    fn required_side_effect_sub_goal_tools(
        &self,
        required_tool_names: &[String],
    ) -> Vec<ToolDefinition> {
        let required_names: HashSet<&str> =
            required_tool_names.iter().map(String::as_str).collect();
        self.tool_executor
            .tool_definitions()
            .into_iter()
            .filter(|tool| {
                required_names.contains(tool.name.as_str())
                    && self.tool_executor.cacheability(&tool.name) == ToolCacheability::SideEffect
            })
            .collect()
    }

    fn sub_goal_retry_messages(
        &self,
        child: &LoopEngine,
        snapshot: &PerceptionSnapshot,
        initial_response: &str,
        required_tool_names: &[String],
    ) -> Vec<Message> {
        let mut messages = child_retry_messages(child, snapshot);
        if let Some(response) = meaningful_response_text(initial_response) {
            messages.push(Message::assistant(response));
        }
        messages.push(Message::system(sub_goal_mutation_retry_directive(
            required_tool_names,
        )));
        messages
    }

    fn check_sub_goal_completion(
        &self,
        sub_goal: &SubGoal,
        signals: &[super::Signal],
        response: &str,
    ) -> SubGoalCompletionCheck {
        if let Some(check) = contract_completion_check(sub_goal, response) {
            return check;
        }
        self.required_tool_completion_check(sub_goal, signals, response)
    }

    fn required_tool_completion_check(
        &self,
        sub_goal: &SubGoal,
        signals: &[super::Signal],
        response: &str,
    ) -> SubGoalCompletionCheck {
        let used_tools = successful_tool_names(signals);
        let used_mutation_tools = successful_mutation_tool_names(signals);
        let required_side_effect_tools = self.required_side_effect_tool_names(sub_goal);
        if let Some(check) = side_effect_completion_check(
            &required_side_effect_tools,
            &used_mutation_tools,
            response,
        ) {
            return check;
        }
        missing_required_tool_check(sub_goal, &used_tools, response)
    }

    fn required_side_effect_tool_names(&self, sub_goal: &SubGoal) -> Vec<String> {
        sub_goal
            .required_tools
            .iter()
            .filter(|tool_name| {
                self.tool_executor.cacheability(tool_name.as_str()) == ToolCacheability::SideEffect
            })
            .cloned()
            .collect()
    }
}

fn build_decomposition_action(decision: &Decision, aggregate: String) -> ActionResult {
    ActionResult {
        decision: decision.clone(),
        tool_results: Vec::new(),
        response_text: aggregate.clone(),
        tokens_used: TokenUsage::default(),
        next_step: ActionNextStep::Continue(ActionContinuation::new(None, Some(aggregate))),
    }
}

pub(super) fn aggregate_sub_goal_results(results: &[SubGoalResult]) -> String {
    if results.is_empty() {
        return "Task decomposition contained no sub-goals.".to_string();
    }
    let mut lines = Vec::with_capacity(results.len() + 1);
    lines.push(DECOMPOSITION_RESULTS_PREFIX.to_string());
    for (index, result) in results.iter().enumerate() {
        lines.push(format_sub_goal_line(index + 1, result));
    }
    lines.join("\n")
}

pub(super) fn is_decomposition_results_message(text: &str) -> bool {
    text.trim_start().starts_with(DECOMPOSITION_RESULTS_PREFIX)
}

pub(super) fn decomposition_results_all_skipped(text: &str) -> bool {
    is_decomposition_results_message(text)
        && text
            .lines()
            .skip(1)
            .all(|line| line.contains("=> skipped (below floor)"))
}

fn format_sub_goal_line(index: usize, result: &SubGoalResult) -> String {
    format!(
        "{index}. {} => {}",
        result.goal.description,
        format_sub_goal_outcome(&result.outcome)
    )
}

fn format_sub_goal_outcome(outcome: &SubGoalOutcome) -> String {
    match outcome {
        SubGoalOutcome::Completed(response) => format!("completed: {response}"),
        SubGoalOutcome::Incomplete(message) => format!("incomplete: {message}"),
        SubGoalOutcome::Failed(message) => format!("failed: {message}"),
        SubGoalOutcome::BudgetExhausted { partial_response } => partial_response
            .as_deref()
            .filter(|text| !text.trim().is_empty())
            .map(|text| {
                format!(
                    "budget exhausted after partial: {}",
                    truncate_prompt_text(text, 240)
                )
            })
            .unwrap_or_else(|| "budget exhausted".to_string()),
        SubGoalOutcome::Skipped => "skipped (below floor)".to_string(),
    }
}

fn allocation_mode_for_strategy(strategy: &AggregationStrategy) -> AllocationMode {
    match strategy {
        AggregationStrategy::Sequential => AllocationMode::Sequential,
        AggregationStrategy::Parallel => AllocationMode::Concurrent,
        AggregationStrategy::Custom(strategy) => {
            unreachable!("custom strategy '{strategy}' should be rejected during parsing")
        }
    }
}

pub(super) fn parse_decomposition_plan(
    arguments: &serde_json::Value,
) -> Result<DecompositionPlan, LoopError> {
    let parsed = parse_decompose_arguments(arguments)?;
    reject_custom_strategy(parsed.strategy.as_ref())?;
    ensure_sub_goals_present(&parsed)?;
    let (sub_goals, truncated_from) = parsed_sub_goals(parsed.sub_goals);
    Ok(DecompositionPlan {
        sub_goals,
        strategy: parsed.strategy.unwrap_or(AggregationStrategy::Sequential),
        truncated_from,
    })
}

fn reject_custom_strategy(strategy: Option<&AggregationStrategy>) -> Result<(), LoopError> {
    if let Some(strategy) = strategy {
        if matches!(strategy, AggregationStrategy::Custom(_)) {
            return Err(loop_error(
                "decide",
                &format!("unsupported decomposition strategy: {strategy:?}"),
                false,
            ));
        }
    }
    Ok(())
}

fn ensure_sub_goals_present(parsed: &DecomposeToolArguments) -> Result<(), LoopError> {
    if parsed.sub_goals.is_empty() {
        return Err(loop_error(
            "decide",
            "decompose tool requires at least one sub_goal",
            false,
        ));
    }
    Ok(())
}

fn parsed_sub_goals(
    sub_goals: Vec<super::DecomposeSubGoalArguments>,
) -> (Vec<SubGoal>, Option<usize>) {
    let mut sub_goals: Vec<SubGoal> = sub_goals.into_iter().map(SubGoal::from).collect();
    if sub_goals.len() > MAX_SUB_GOALS {
        let original_sub_goals = sub_goals.len();
        sub_goals.truncate(MAX_SUB_GOALS);
        return (sub_goals, Some(original_sub_goals));
    }
    (sub_goals, None)
}

fn parse_decompose_arguments(
    arguments: &serde_json::Value,
) -> Result<DecomposeToolArguments, LoopError> {
    serde_json::from_value(arguments.clone()).map_err(|error| {
        loop_error(
            "decide",
            &format!("invalid decompose tool arguments: {error}"),
            false,
        )
    })
}

pub(super) fn estimate_plan_cost(plan: &DecompositionPlan) -> ActionCost {
    plan.sub_goals
        .iter()
        .fold(ActionCost::default(), |mut acc, sub_goal| {
            let llm_calls = estimated_llm_calls(sub_goal);
            let tool_invocations = sub_goal.required_tools.len() as u32;
            acc.llm_calls = acc.llm_calls.saturating_add(llm_calls);
            acc.tool_invocations = acc.tool_invocations.saturating_add(tool_invocations);
            acc.cost_cents = acc.cost_cents.saturating_add(
                u64::from(llm_calls) * DEFAULT_LLM_CALL_COST_CENTS
                    + u64::from(tool_invocations) * DEFAULT_TOOL_INVOCATION_COST_CENTS,
            );
            acc
        })
}

fn estimated_llm_calls(sub_goal: &SubGoal) -> u32 {
    match sub_goal
        .complexity_hint
        .unwrap_or_else(|| estimate_complexity(sub_goal))
    {
        ComplexityHint::Trivial => 1,
        ComplexityHint::Moderate => 2,
        ComplexityHint::Complex => 4,
    }
}

fn child_budget_config(
    sub_goal_budgets: &[BudgetConfig],
    index: usize,
    fallback: BudgetConfig,
) -> BudgetConfig {
    sub_goal_budgets.get(index).cloned().unwrap_or(fallback)
}

fn completed_sub_goal_result(
    goal: SubGoal,
    response: String,
    signals: Vec<super::Signal>,
) -> SubGoalResult {
    SubGoalResult {
        goal,
        outcome: SubGoalOutcome::Completed(response),
        signals,
    }
}

fn missing_side_effect_retry_tools_result(
    sub_goal: &SubGoal,
    retry: SubGoalRetryContext,
) -> SubGoalResult {
    let message = format!(
        "sub-goal required side-effect tools ({}) are not available for bounded retry",
        retry.required_tool_names.join(", ")
    );
    incomplete_sub_goal_result_with_signals(sub_goal.clone(), message, retry.initial_signals)
}

fn build_follow_up_messages(
    continuation_messages: &[Message],
    continuation: &ActionContinuation,
) -> Vec<Message> {
    let mut messages = continuation_messages.to_vec();
    append_continuation_context(&mut messages, continuation);
    messages
}

fn continuation_with_action_partial(
    continuation: ActionContinuation,
    action_partial: Option<String>,
) -> ActionContinuation {
    ActionContinuation {
        partial_response: continuation.partial_response.or(action_partial),
        context_message: continuation.context_message,
        context_messages: continuation.context_messages,
        next_tool_scope: continuation.next_tool_scope,
        turn_commitment: continuation.turn_commitment,
        artifact_write_target: continuation.artifact_write_target,
    }
}

fn incomplete_follow_up_result(
    sub_goal: &SubGoal,
    response: String,
    message: String,
    signals: Vec<super::Signal>,
) -> SubGoalResult {
    let partial_response = meaningful_response_text(&response).or(Some(message));
    incomplete_sub_goal_result_with_signals(
        sub_goal.clone(),
        partial_response.unwrap_or_else(|| SUB_GOAL_MUTATION_RETRY_INCOMPLETE_REASON.to_string()),
        signals,
    )
}

fn child_retry_messages(child: &LoopEngine, snapshot: &PerceptionSnapshot) -> Vec<Message> {
    if child.last_reasoning_messages.is_empty() {
        return rebuilt_child_retry_messages(snapshot);
    }
    child.last_reasoning_messages.clone()
}

fn rebuilt_child_retry_messages(snapshot: &PerceptionSnapshot) -> Vec<Message> {
    let mut messages = snapshot.conversation_history.clone();
    let user_message = extract_user_message(snapshot).unwrap_or_else(|_| {
        snapshot
            .user_input
            .as_ref()
            .map(|input| input.text.clone())
            .unwrap_or_else(|| sub_goal_fallback_user_message(snapshot))
    });
    messages.push(build_user_message(snapshot, &user_message));
    messages
}

fn contract_completion_check(sub_goal: &SubGoal, response: &str) -> Option<SubGoalCompletionCheck> {
    match sub_goal.classify(response) {
        fx_decompose::SubGoalCompletionClassification::Completed => None,
        fx_decompose::SubGoalCompletionClassification::Incomplete(message) => {
            Some(SubGoalCompletionCheck::Incomplete(message))
        }
    }
}

fn side_effect_completion_check(
    required_side_effect_tools: &[String],
    used_mutation_tools: &HashSet<&str>,
    response: &str,
) -> Option<SubGoalCompletionCheck> {
    if required_side_effect_tools.is_empty() {
        return None;
    }
    if required_side_effect_tools
        .iter()
        .all(|tool_name| !used_mutation_tools.contains(tool_name.as_str()))
    {
        return Some(SubGoalCompletionCheck::MissingRequiredSideEffectTools {
            message: format!(
                "sub-goal ended without using any required side-effect tools ({}) despite returning a response: {}",
                required_side_effect_tools.join(", "),
                truncate_prompt_text(response, 180)
            ),
            tool_names: required_side_effect_tools.to_vec(),
        });
    }
    None
}

fn missing_required_tool_check(
    sub_goal: &SubGoal,
    used_tools: &HashSet<&str>,
    response: &str,
) -> SubGoalCompletionCheck {
    if !sub_goal.required_tools.is_empty()
        && sub_goal
            .required_tools
            .iter()
            .all(|tool_name| !used_tools.contains(tool_name.as_str()))
    {
        return SubGoalCompletionCheck::Incomplete(format!(
            "sub-goal ended without using any required tools ({}) despite returning a response: {}",
            sub_goal.required_tools.join(", "),
            truncate_prompt_text(response, 180)
        ));
    }
    SubGoalCompletionCheck::Valid
}

pub(super) fn child_max_iterations(max_iterations: u32) -> u32 {
    max_iterations.clamp(1, 3)
}

pub(super) fn build_sub_goal_snapshot(
    sub_goal: &SubGoal,
    prior_results: &[SubGoalResult],
    context_messages: &[Message],
    timestamp_ms: u64,
) -> PerceptionSnapshot {
    let description = sub_goal.description.clone();
    let mut conversation_history = context_messages.to_vec();
    if !prior_results.is_empty() {
        conversation_history.push(Message::assistant(format!(
            "Prior decomposition results for context only:\n{}",
            aggregate_sub_goal_results(prior_results)
        )));
    }
    PerceptionSnapshot {
        timestamp_ms,
        screen: ScreenState {
            current_app: "decomposition".to_string(),
            elements: Vec::new(),
            text_content: description.clone(),
        },
        notifications: Vec::new(),
        active_app: "decomposition".to_string(),
        user_input: Some(UserInput {
            text: description,
            source: InputSource::Text,
            timestamp: timestamp_ms,
            context_id: None,
            images: Vec::new(),
            documents: Vec::new(),
        }),
        sensor_data: None,
        conversation_history,
        steer_context: None,
    }
}

fn sub_goal_fallback_user_message(snapshot: &PerceptionSnapshot) -> String {
    snapshot.screen.text_content.trim().to_string()
}

pub(super) fn sub_goal_result_from_loop(goal: SubGoal, result: LoopResult) -> SubGoalResult {
    match result {
        LoopResult::Complete {
            response, signals, ..
        } => completed_sub_goal_result(goal, response, signals),
        LoopResult::BudgetExhausted {
            partial_response,
            signals,
            ..
        } => SubGoalResult {
            goal,
            outcome: SubGoalOutcome::BudgetExhausted { partial_response },
            signals,
        },
        LoopResult::Incomplete {
            partial_response,
            reason,
            signals,
            ..
        } => SubGoalResult {
            goal,
            outcome: SubGoalOutcome::BudgetExhausted {
                partial_response: partial_response.or(Some(reason)),
            },
            signals,
        },
        LoopResult::Error {
            message, signals, ..
        } => failed_sub_goal_result_with_signals(goal, message, signals),
        LoopResult::UserStopped { signals, .. } => failed_sub_goal_result_with_signals(
            goal,
            "sub-goal stopped before completion".to_string(),
            signals,
        ),
    }
}

pub(super) fn successful_tool_names(signals: &[super::Signal]) -> HashSet<&str> {
    signals
        .iter()
        .filter(|signal| signal.step == LoopStep::Act && signal.kind == SignalKind::Success)
        .filter_map(|signal| signal.message.strip_prefix("tool "))
        .collect()
}

pub(super) fn successful_mutation_tool_names(signals: &[super::Signal]) -> HashSet<&str> {
    signals
        .iter()
        .filter(|signal| signal.step == LoopStep::Act && signal.kind == SignalKind::Success)
        .filter(|signal| {
            signal
                .metadata
                .get("classification")
                .and_then(serde_json::Value::as_str)
                == Some("mutation")
        })
        .filter_map(|signal| signal.message.strip_prefix("tool "))
        .collect()
}

fn failed_sub_goal_execution(
    goal: &SubGoal,
    message: String,
    budget: BudgetTracker,
) -> SubGoalExecution {
    SubGoalExecution {
        result: failed_sub_goal_result(goal.clone(), message),
        budget,
    }
}

fn failed_sub_goal_result(goal: SubGoal, message: String) -> SubGoalResult {
    failed_sub_goal_result_with_signals(goal, message, Vec::new())
}

fn incomplete_sub_goal_result_with_signals(
    goal: SubGoal,
    message: String,
    signals: Vec<super::Signal>,
) -> SubGoalResult {
    SubGoalResult {
        goal,
        outcome: SubGoalOutcome::Incomplete(message),
        signals,
    }
}

fn failed_sub_goal_result_with_signals(
    goal: SubGoal,
    message: String,
    signals: Vec<super::Signal>,
) -> SubGoalResult {
    SubGoalResult {
        goal,
        outcome: SubGoalOutcome::Failed(message),
        signals,
    }
}

fn skipped_sub_goal_result(goal: SubGoal) -> SubGoalResult {
    SubGoalResult {
        goal,
        outcome: SubGoalOutcome::Skipped,
        signals: Vec::new(),
    }
}

fn merge_sub_goal_signals(
    mut result: SubGoalResult,
    mut prior_signals: Vec<super::Signal>,
) -> SubGoalResult {
    prior_signals.extend(result.signals);
    result.signals = prior_signals;
    result
}

pub(super) fn should_halt_sub_goal_sequence(result: &SubGoalResult) -> bool {
    match &result.outcome {
        SubGoalOutcome::BudgetExhausted { partial_response } => partial_response
            .as_deref()
            .map(str::trim)
            .is_none_or(str::is_empty),
        _ => false,
    }
}

fn sub_goal_mutation_retry_directive(tool_names: &[String]) -> String {
    format!(
        "You already have enough context for this sub-goal. Do not describe next steps or restate the plan. Use one of these required side-effect tools now: {}. If you truly cannot execute, answer briefly with the concrete blocker.",
        tool_names.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::act::{ToolExecutor, ToolExecutorError, ToolResult};
    use crate::budget::{BudgetConfig, BudgetTracker};
    use crate::cancellation::CancellationToken;
    use crate::context_manager::ContextCompactor;
    use async_trait::async_trait;
    use fx_decompose::{SubGoalContract, SubGoalOutcome};
    use std::sync::Arc;

    #[test]
    fn parse_decomposition_plan_truncates_sub_goals_to_maximum() {
        let sub_goals = (0..8)
            .map(|index| serde_json::json!({"description": format!("goal-{index}")}))
            .collect::<Vec<_>>();
        let arguments = serde_json::json!({"sub_goals": sub_goals});

        let plan = parse_decomposition_plan(&arguments).expect("plan should parse");

        assert_eq!(plan.sub_goals.len(), MAX_SUB_GOALS);
        assert_eq!(plan.sub_goals[0].description, "goal-0");
        assert_eq!(plan.sub_goals[MAX_SUB_GOALS - 1].description, "goal-4");
        assert_eq!(plan.truncated_from, Some(8));
    }

    #[test]
    fn aggregate_sub_goal_results_marks_all_skipped() {
        let aggregate =
            aggregate_sub_goal_results(&[skipped_result("first"), skipped_result("second")]);

        assert!(is_decomposition_results_message(&aggregate));
        assert!(decomposition_results_all_skipped(&aggregate));
    }

    #[test]
    fn format_sub_goal_outcome_includes_skipped_variant() {
        assert_eq!(
            format_sub_goal_outcome(&SubGoalOutcome::Skipped),
            "skipped (below floor)"
        );
    }

    #[test]
    fn format_sub_goal_outcome_includes_budget_exhausted_partial_response() {
        let outcome = SubGoalOutcome::BudgetExhausted {
            partial_response: Some(
                "I have enough from the search results to write a comprehensive spec.".to_string(),
            ),
        };

        assert_eq!(
            format_sub_goal_outcome(&outcome),
            "budget exhausted after partial: I have enough from the search results to write a comprehensive spec."
        );
    }

    #[test]
    fn estimate_plan_cost_trivial_no_tools() {
        let cost = estimate_plan_cost(&plan(vec![sub_goal(
            "a",
            &[],
            Some(ComplexityHint::Trivial),
        )]));

        assert_eq!(cost.llm_calls, 1);
        assert_eq!(cost.tool_invocations, 0);
        assert_eq!(cost.cost_cents, 2);
    }

    #[test]
    fn estimate_plan_cost_complex_with_tools() {
        let cost = estimate_plan_cost(&plan(vec![sub_goal(
            "task",
            &["t1", "t2"],
            Some(ComplexityHint::Complex),
        )]));

        assert_eq!(cost.llm_calls, 4);
        assert_eq!(cost.tool_invocations, 2);
        assert_eq!(cost.cost_cents, 10);
    }

    #[test]
    fn estimate_plan_cost_accumulates_across_sub_goals() {
        let cost = estimate_plan_cost(&plan(vec![
            sub_goal("a", &["t1"], Some(ComplexityHint::Trivial)),
            sub_goal("b", &["t1", "t2"], Some(ComplexityHint::Moderate)),
        ]));

        assert_eq!(cost.llm_calls, 3);
        assert_eq!(cost.tool_invocations, 3);
        assert_eq!(cost.cost_cents, 9);
    }

    #[test]
    fn depth_limited_result_emits_blocked_signal() {
        let config = BudgetConfig {
            max_recursion_depth: 1,
            ..BudgetConfig::default()
        };
        let mut engine = build_engine_with_budget(config, 1);
        let decision = Decision::Decompose(plan(vec![sub_goal("blocked", &[], None)]));

        let result = engine.depth_limited_decomposition_result(&decision);

        assert!(result.tool_results.is_empty());
        let blocked = engine
            .signals
            .signals()
            .iter()
            .filter(|signal| signal.kind == SignalKind::Blocked)
            .collect::<Vec<_>>();
        assert_eq!(blocked.len(), 1);
        assert!(blocked[0].message.contains("recursion depth"));
    }

    #[tokio::test]
    async fn prepare_sub_goal_run_shares_timestamp_between_budget_and_snapshot() {
        let child_config = BudgetConfig {
            max_wall_time_ms: 250,
            ..BudgetConfig::default()
        };
        let engine = build_engine_with_budget(BudgetConfig::default(), 0);
        let goal = sub_goal("child", &[], None);
        let prior_results = Vec::new();
        let request = SubGoalRunRequest {
            sub_goal: &goal,
            child_config: child_config.clone(),
            prior_results: &prior_results,
        };

        let (child, snapshot) = engine
            .prepare_sub_goal_run(&request, &[], 77)
            .await
            .expect("sub-goal preparation should succeed");

        assert_eq!(snapshot.timestamp_ms, 77);
        assert_eq!(
            child.budget.remaining(77).wall_time_ms,
            child_config.max_wall_time_ms
        );
    }

    fn build_engine_with_budget(config: BudgetConfig, depth: u32) -> LoopEngine {
        LoopEngine::builder()
            .budget(BudgetTracker::new(config, 0, depth))
            .context(ContextCompactor::new(2048, 256))
            .max_iterations(1)
            .tool_executor(Arc::new(PassiveToolExecutor))
            .synthesis_instruction("Summarize tool output".to_string())
            .build()
            .expect("test engine build")
    }

    fn plan(sub_goals: Vec<SubGoal>) -> DecompositionPlan {
        DecompositionPlan {
            sub_goals,
            strategy: AggregationStrategy::Parallel,
            truncated_from: None,
        }
    }

    fn skipped_result(description: &str) -> SubGoalResult {
        SubGoalResult {
            goal: sub_goal(description, &[], None),
            outcome: SubGoalOutcome::Skipped,
            signals: Vec::new(),
        }
    }

    fn sub_goal(
        description: &str,
        required_tools: &[&str],
        complexity_hint: Option<ComplexityHint>,
    ) -> SubGoal {
        SubGoal {
            description: description.to_string(),
            required_tools: required_tools
                .iter()
                .map(|tool| (*tool).to_string())
                .collect(),
            completion_contract: SubGoalContract::from_definition_of_done(None),
            complexity_hint,
        }
    }

    #[derive(Debug, Default)]
    struct PassiveToolExecutor;

    #[async_trait]
    impl ToolExecutor for PassiveToolExecutor {
        async fn execute_tools(
            &self,
            _calls: &[fx_llm::ToolCall],
            _cancel: Option<&CancellationToken>,
        ) -> Result<Vec<ToolResult>, ToolExecutorError> {
            Ok(Vec::new())
        }
    }
}
