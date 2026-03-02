//! Agentic loop orchestrator.

use crate::act::{ActionResult, TokenUsage, ToolExecutor, ToolResult};
use crate::budget::{ActionCost, BudgetRemaining, BudgetResource, BudgetTracker};
use crate::cancellation::CancellationToken;
use crate::context_manager::ContextCompactor;
use crate::continuation::Continuation;
use crate::decide::{Decision, CONFIDENCE_CLARIFY_THRESHOLD};
use crate::input::{LoopCommand, LoopInputChannel};
use crate::learn::Learning;
use crate::perceive::{ProcessedPerception, TrimmingPolicy};
use crate::signals::{LoopStep, Signal, SignalCollector, SignalKind};
use crate::types::{
    Goal, IdentityContext, LoopError, PerceptionSnapshot, ReasoningContext, WorkingMemoryEntry,
};
use crate::verify::Verification;
use async_trait::async_trait;
use fx_core::types::{InputSource, ScreenState, UserInput};
use fx_decompose::{
    AggregationStrategy, DecompositionPlan, SubGoal, SubGoalOutcome, SubGoalResult,
};
use fx_llm::{
    CompletionRequest, CompletionResponse, ContentBlock, Message, MessageRole, ProviderError,
    ToolCall, ToolDefinition, Usage,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

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
        /// Learning artifacts produced across iterations.
        learnings: Vec<Learning>,
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
    /// Loop requires additional user input.
    NeedsInput {
        /// Prompt to present to user.
        prompt: String,
        /// Iterations completed before requesting input.
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

/// Core orchestrator for the 7-step agentic loop.
///
/// Note: `LoopEngine` previously derived `Clone`, but Phase 4 added
/// `LoopInputChannel` which contains `mpsc::Receiver` (not `Clone`).
/// No existing code clones `LoopEngine`, so this is a safe change.
#[derive(Debug)]
pub struct LoopEngine {
    budget: BudgetTracker,
    context: ContextCompactor,
    tool_executor: Arc<dyn ToolExecutor>,
    max_iterations: u32,
    iteration_count: u32,
    synthesis_instruction: String,
    memory_context: Option<String>,
    signals: SignalCollector,
    cancel_token: Option<CancellationToken>,
    input_channel: Option<LoopInputChannel>,
    user_stop_requested: bool,
    event_bus: Option<fx_core::EventBus>,
}

#[derive(Debug, Default, Clone)]
struct CycleState {
    learnings: Vec<Learning>,
    tokens: TokenUsage,
    partial_response: Option<String>,
}

#[derive(Debug, Clone)]
struct IterationOutcome {
    response_text: String,
    continuation: Continuation,
    learning: Learning,
}

#[derive(Debug, Clone)]
enum IterationStep {
    Progress(IterationOutcome),
    Terminal(LoopResult),
}

#[derive(Debug, Clone)]
struct ToolRoundState {
    all_tool_results: Vec<ToolResult>,
    current_calls: Vec<ToolCall>,
    continuation_messages: Vec<Message>,
    tokens_used: TokenUsage,
}

impl ToolRoundState {
    fn new(calls: &[ToolCall], context_messages: &[Message]) -> Self {
        Self {
            all_tool_results: Vec::new(),
            current_calls: calls.to_vec(),
            continuation_messages: context_messages.to_vec(),
            tokens_used: TokenUsage::default(),
        }
    }
}

#[derive(Debug)]
enum ToolRoundOutcome {
    Cancelled,
    Response(CompletionResponse),
}

#[derive(Debug)]
struct SubGoalExecution {
    result: SubGoalResult,
    budget: BudgetTracker,
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
}

impl From<DecomposeSubGoalArguments> for SubGoal {
    fn from(value: DecomposeSubGoalArguments) -> Self {
        Self {
            description: value.description,
            required_tools: value.required_tools,
            expected_output: value.expected_output,
        }
    }
}

const REASONING_OUTPUT_TOKEN_HEURISTIC: u64 = 192;
const TOOL_SYNTHESIS_TOKEN_HEURISTIC: u64 = 320;
const REASONING_MAX_OUTPUT_TOKENS: u32 = 4096;
const REASONING_TEMPERATURE: f32 = 0.2;
const TOOL_SYNTHESIS_MAX_OUTPUT_TOKENS: u32 = 1024;
const MAX_CONTINUATION_ATTEMPTS: u32 = 3;
const DEFAULT_LLM_ACTION_COST_CENTS: u64 = 2;
const SAFE_FALLBACK_RESPONSE: &str = "I wasn't able to process that. Could you try rephrasing?";
const DECOMPOSE_TOOL_NAME: &str = "decompose";
const DECOMPOSE_TOOL_DESCRIPTION: &str =
    "Break a complex task into 2-4 high-level sub-goals. Each sub-goal should be substantial enough to justify its own execution context. Do NOT create more than 5 sub-goals. Prefer fewer, broader goals over many narrow ones. Only use this for tasks that genuinely cannot be handled with direct tool calls.";
const MAX_SUB_GOALS: usize = 5;
const DECOMPOSITION_DEPTH_LIMIT_RESPONSE: &str =
    "I can't decompose this request further because the recursion depth limit was reached.";
const REASONING_SYSTEM_PROMPT: &str = "You are Fawx, a capable personal assistant. \
Answer the user directly and concisely. \
Never introduce yourself, greet the user, or add preamble — just answer. \
Use tools when you need information not already in the conversation \
(current time, file contents, directory listings, search results, memory, etc.). \
When the user's request relates to an available tool's purpose, prefer calling the tool \
over answering from general knowledge. \
After using tools, respond with the answer — never narrate what tools you used, \
describe the process, or comment on tool output metadata. \
Never narrate your process, hedge with qualifiers, or reference tool mechanics. \
Avoid filler openers like \"I notice\", \"I can see that\", \"Based on the results\", \
\"It appears that\", \"Let me\", or \"I aim to\". Just answer the question. \
If the user makes a statement (not a question), acknowledge it naturally and briefly. \
If a tool call stores data (like memory_write), confirm the action in one short sentence. You were created by Joe as a TUI-first agentic engine. You are open-source, written in Rust, and designed to be self-extending through a plugin system.";

const MEMORY_INSTRUCTION: &str = "\n\nYou have persistent memory across sessions. \
Use memory_write to save important facts about the user, their preferences, \
and project context. Use memory_read to recall specific details. \
Memories survive restart \u{2014} write anything worth remembering.";

const VERIFICATION_CONFIDENCE_CLEAN: f64 = 0.9;
const VERIFICATION_CONFIDENCE_SINGLE_DISCREPANCY: f64 = 0.45;
const VERIFICATION_CONFIDENCE_MULTIPLE_DISCREPANCIES: f64 = 0.25;

impl LoopEngine {
    /// Create a loop engine with an injected tool executor.
    pub fn new(
        budget: BudgetTracker,
        context: ContextCompactor,
        max_iterations: u32,
        tool_executor: Arc<dyn ToolExecutor>,
        synthesis_instruction: String,
    ) -> Self {
        Self {
            budget,
            context,
            tool_executor,
            max_iterations: max_iterations.max(1),
            iteration_count: 0,
            synthesis_instruction,
            memory_context: None,
            signals: SignalCollector::default(),
            cancel_token: None,
            input_channel: None,
            user_stop_requested: false,
            event_bus: None,
        }
    }

    /// Attach an fx-core event bus for inter-component progress events.
    pub fn set_event_bus(&mut self, bus: fx_core::EventBus) {
        self.event_bus = Some(bus);
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
        if context.trim().is_empty() {
            self.memory_context = None;
        } else {
            self.memory_context = Some(context);
        }
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
        self.signals.emit(Signal {
            step,
            kind,
            message: message.into(),
            metadata,
            timestamp_ms: current_time_ms(),
        });
    }

    fn finalize_result(&mut self, result: LoopResult) -> LoopResult {
        self.emit_cache_stats_signal();
        let signals = self.signals.drain_all();
        attach_signals(result, signals)
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
        mut perception: PerceptionSnapshot,
        llm: &dyn LlmProvider,
    ) -> Result<LoopResult, LoopError> {
        self.prepare_cycle();
        let mut state = CycleState::default();

        while self.iteration_count < self.max_iterations {
            self.iteration_count = self.iteration_count.saturating_add(1);
            match self.execute_iteration(&perception, llm, &mut state).await? {
                IterationStep::Terminal(result) => return Ok(self.finalize_result(result)),
                IterationStep::Progress(outcome) => {
                    let IterationOutcome {
                        response_text,
                        continuation,
                        learning,
                    } = outcome;
                    if let Some(result) = self.handle_continuation(
                        continuation,
                        response_text,
                        learning,
                        &mut perception,
                        &mut state,
                    ) {
                        return Ok(self.finalize_result(result));
                    }
                }
            }
        }

        Ok(self.finalize_result(LoopResult::Error {
            message: format!(
                "Loop reached safety limit of {} iterations without completion.",
                self.max_iterations
            ),
            recoverable: true,
            signals: Vec::new(),
        }))
    }

    /// Drain the input channel and return the highest-priority command.
    ///
    /// Priority ordering: `Abort` always wins. For other commands (`Stop`,
    /// `Wait`, `Resume`), the last received command takes precedence.
    /// In practice this is fine — users cannot queue multiple commands in
    /// the microsecond window between drain calls.
    fn check_user_input(&mut self) -> Option<LoopCommand> {
        let channel = self.input_channel.as_mut()?;
        let mut highest: Option<LoopCommand> = None;
        while let Some(cmd) = channel.try_recv() {
            highest = Some(match (&highest, cmd) {
                (Some(LoopCommand::Abort), _) => LoopCommand::Abort,
                (_, LoopCommand::Abort) => LoopCommand::Abort,
                _ => cmd,
            });
        }
        highest
    }

    /// Check both the cancellation token and input channel.
    fn check_cancellation(&mut self, partial: Option<String>) -> Option<IterationStep> {
        if self.user_stop_requested {
            self.user_stop_requested = false;
            return Some(self.user_stopped_step(partial, "user stopped", "input_channel"));
        }

        if self.cancellation_token_triggered() {
            return Some(self.user_stopped_step(partial, "user cancelled", "cancellation_token"));
        }

        if self.consume_stop_or_abort_command() {
            return Some(self.user_stopped_step(partial, "user stopped", "input_channel"));
        }

        None
    }

    fn user_stopped_step(
        &mut self,
        partial: Option<String>,
        message: &str,
        source: &str,
    ) -> IterationStep {
        self.emit_signal(
            LoopStep::Act,
            SignalKind::Blocked,
            message,
            serde_json::json!({ "source": source }),
        );
        IterationStep::Terminal(LoopResult::UserStopped {
            partial_response: partial,
            iterations: self.iteration_count,
            signals: Vec::new(),
        })
    }

    fn consume_stop_or_abort_command(&mut self) -> bool {
        matches!(
            self.check_user_input(),
            Some(LoopCommand::Stop | LoopCommand::Abort)
        )
    }

    fn prepare_cycle(&mut self) {
        self.iteration_count = 0;
        self.budget.reset(current_time_ms());
        self.signals.clear();
        self.user_stop_requested = false;
        if let Some(token) = &self.cancel_token {
            token.reset();
        }
        self.tool_executor.clear_cache();
    }

    async fn execute_iteration(
        &mut self,
        perception: &PerceptionSnapshot,
        llm: &dyn LlmProvider,
        state: &mut CycleState,
    ) -> Result<IterationStep, LoopError> {
        if let Some(step) =
            self.budget_terminal(ActionCost::default(), state.partial_response.clone())
        {
            return Ok(step);
        }

        if let Some(step) = self.check_cancellation(state.partial_response.clone()) {
            return Ok(step);
        }

        let processed = self.perceive(perception).await?;
        let reason_cost = self.estimate_reasoning_cost(&processed);
        if let Some(step) = self.budget_terminal(reason_cost, state.partial_response.clone()) {
            return Ok(step);
        }

        let response = self.reason(&processed, llm).await?;
        self.record_reasoning_cost(reason_cost, state);

        let decision = self.decide(&response).await?;
        if let Some(step) = self.budget_terminal(
            self.estimate_action_cost(&decision),
            state.partial_response.clone(),
        ) {
            return Ok(step);
        }

        self.execute_action_and_finalize(&decision, llm, state, &processed.context_window)
            .await
    }

    async fn execute_action_and_finalize(
        &mut self,
        decision: &Decision,
        llm: &dyn LlmProvider,
        state: &mut CycleState,
        context_messages: &[Message],
    ) -> Result<IterationStep, LoopError> {
        let action = self.act(decision, llm, context_messages).await?;
        let action_cost = self.action_cost_from_result(&action);
        if let Some(step) = self.budget_terminal(action_cost, Some(action.response_text.clone())) {
            return Ok(step);
        }

        self.budget.record(&action_cost);
        state.tokens.accumulate(action.tokens_used);
        state.partial_response = Some(action.response_text.clone());

        if let Some(step) = self.check_cancellation(state.partial_response.clone()) {
            return Ok(step);
        }

        let verification = self.verify(&action).await?;
        let learning = self.learn(&verification).await?;
        let continuation = self
            .should_continue(&action.decision, &verification, &learning)
            .await?;

        Ok(IterationStep::Progress(IterationOutcome {
            response_text: action.response_text,
            continuation,
            learning,
        }))
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
    ) -> Option<IterationStep> {
        self.handle_budget_check(cost, partial_response)
            .map(IterationStep::Terminal)
    }

    fn handle_budget_check(
        &mut self,
        cost: ActionCost,
        partial_response: Option<String>,
    ) -> Option<LoopResult> {
        if self.budget.check_at(current_time_ms(), &cost).is_ok() {
            return None;
        }

        self.emit_signal(
            LoopStep::Continue,
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

    fn handle_continuation(
        &mut self,
        continuation: Continuation,
        response_text: String,
        learning: Learning,
        perception: &mut PerceptionSnapshot,
        state: &mut CycleState,
    ) -> Option<LoopResult> {
        state.learnings.push(learning);
        match continuation {
            Continuation::Complete => Some(LoopResult::Complete {
                response: response_text,
                iterations: self.iteration_count,
                tokens_used: state.tokens,
                learnings: state.learnings.clone(),
                signals: Vec::new(),
            }),
            Continuation::NeedsInput(prompt) => Some(LoopResult::NeedsInput {
                prompt,
                iterations: self.iteration_count,
                signals: Vec::new(),
            }),
            Continuation::Continue(sub_goal) => {
                *perception = next_perception_from_sub_goal(perception, &sub_goal);
                None
            }
        }
    }

    /// Perceive step.
    async fn perceive(
        &mut self,
        snapshot: &PerceptionSnapshot,
    ) -> Result<ProcessedPerception, LoopError> {
        let user_message = extract_user_message(snapshot)?;
        self.emit_signal(
            LoopStep::Perceive,
            SignalKind::Trace,
            "processing user input",
            serde_json::json!({"input_length": user_message.len()}),
        );
        let mut context_window = snapshot.conversation_history.clone();
        context_window.push(Message::user(user_message.clone()));

        self.append_compacted_summary(snapshot, &user_message, &mut context_window);

        Ok(ProcessedPerception {
            user_message: user_message.clone(),
            context_window,
            active_goals: vec![format!("Help the user with: {user_message}")],
            budget_remaining: self.budget.remaining(snapshot.timestamp_ms),
        })
    }

    /// Reason step.
    async fn reason(
        &mut self,
        perception: &ProcessedPerception,
        llm: &dyn LlmProvider,
    ) -> Result<CompletionResponse, LoopError> {
        let request = build_reasoning_request(
            perception,
            llm.model_name(),
            self.tool_executor.tool_definitions(),
            self.memory_context.as_deref(),
        );
        let reasoning_messages = request.messages.clone();
        let started = current_time_ms();
        let response = llm
            .complete(request)
            .await
            .map_err(|error| loop_error("reason", &format!("completion failed: {error}"), true))?;
        let response = self
            .continue_truncated_response(response, &reasoning_messages, llm, LoopStep::Reason)
            .await?;
        let latency_ms = current_time_ms().saturating_sub(started);
        let usage = response.usage;
        self.emit_reason_trace_and_perf(latency_ms, usage.as_ref());
        Ok(response)
    }

    fn emit_continuation_trace(&mut self, step: LoopStep, attempt: u32) {
        self.emit_signal(
            step,
            SignalKind::Trace,
            format!("response truncated, continuing ({attempt}/{MAX_CONTINUATION_ATTEMPTS})"),
            serde_json::json!({"attempt": attempt}),
        );
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
    ) -> Result<CompletionResponse, LoopError> {
        self.ensure_continuation_budget(continuation_messages, step)?;
        let request = build_truncation_continuation_request(
            llm.model_name(),
            continuation_messages,
            self.tool_executor.tool_definitions(),
            self.memory_context.as_deref(),
            step,
        );
        let request_messages = request.messages.clone();
        let stage = step_stage(step);
        let response = llm.complete(request).await.map_err(|error| {
            loop_error(
                stage,
                &format!("continuation completion failed: {error}"),
                true,
            )
        })?;
        self.record_continuation_budget(&response, &request_messages);
        Ok(response)
    }

    async fn continue_truncated_response(
        &mut self,
        initial_response: CompletionResponse,
        base_messages: &[Message],
        llm: &dyn LlmProvider,
        step: LoopStep,
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
                .request_truncated_continuation(llm, &continuation_messages, step)
                .await?;
            combined = merge_continuation_response(combined, continued, &mut full_text);
        }

        Ok(combined)
    }

    /// Decide step.
    async fn decide(&mut self, response: &CompletionResponse) -> Result<Decision, LoopError> {
        // Decompose takes priority over all other tool calls in the same response.
        // Other tool calls are intentionally discarded — the sub-goals will re-invoke tools as needed.
        if let Some(decompose_call) = find_decompose_tool_call(&response.tool_calls) {
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
            let decision = Decision::UseTools(response.tool_calls.clone());
            self.emit_decision_signals(&decision);
            return Ok(decision);
        }

        let raw = extract_response_text(response);
        let text = extract_readable_text(&raw);
        let decision = Decision::Respond(ensure_non_empty_response(&text));
        self.emit_decision_signals(&decision);
        Ok(decision)
    }

    /// Act step.
    async fn act(
        &mut self,
        decision: &Decision,
        llm: &dyn LlmProvider,
        context_messages: &[Message],
    ) -> Result<ActionResult, LoopError> {
        match decision {
            // Note: Clarify and Defer are not produced by decide() in the current
            // loop engine flow, but are kept for external callers (Decision is pub).
            Decision::Respond(text) | Decision::Clarify(text) | Decision::Defer(text) => {
                Ok(self.text_action_result(decision, text))
            }
            Decision::UseTools(calls) => {
                let action = self
                    .act_with_tools(decision, calls, llm, context_messages)
                    .await?;
                self.emit_action_signals(&action.tool_results);
                Ok(action)
            }
            Decision::Decompose(plan) => {
                self.execute_decomposition(decision, plan, llm, context_messages)
                    .await
            }
        }
    }

    async fn execute_decomposition(
        &mut self,
        decision: &Decision,
        plan: &DecompositionPlan,
        llm: &dyn LlmProvider,
        context_messages: &[Message],
    ) -> Result<ActionResult, LoopError> {
        if self.decomposition_depth_limited() {
            return Ok(self.depth_limited_decomposition_result(decision));
        }

        if let Some(original_sub_goals) = plan.truncated_from {
            self.emit_decomposition_truncation_signal(original_sub_goals, plan.sub_goals.len());
        }

        let results = match &plan.strategy {
            AggregationStrategy::Parallel => {
                self.execute_sub_goals_concurrent(plan, llm, context_messages)
                    .await
            }
            AggregationStrategy::Sequential => {
                self.execute_sub_goals_sequential(plan, llm, context_messages)
                    .await
            }
            AggregationStrategy::Custom(s) => {
                unreachable!("custom strategy '{s}' should be rejected during parsing")
            }
        };

        Ok(ActionResult {
            decision: decision.clone(),
            tool_results: Vec::new(),
            response_text: aggregate_sub_goal_results(&results),
            tokens_used: TokenUsage::default(),
        })
    }

    async fn execute_sub_goals_sequential(
        &mut self,
        plan: &DecompositionPlan,
        llm: &dyn LlmProvider,
        context_messages: &[Message],
    ) -> Vec<SubGoalResult> {
        // Each sequential sub-goal gets 50% of the parent's *remaining* budget
        // after prior sub-goals' consumption. This creates a diminishing series
        // (50%, 25%, 12.5%, ...) that naturally throttles later sub-goals.
        let per_goal_fraction = 0.5;
        let total = plan.sub_goals.len();
        let mut results = Vec::with_capacity(total);
        for (index, sub_goal) in plan.sub_goals.iter().enumerate() {
            self.emit_sub_goal_progress(index, total, &sub_goal.description);
            let execution = self
                .run_sub_goal(sub_goal, per_goal_fraction, llm, context_messages)
                .await;
            self.budget.absorb_child_usage(&execution.budget);
            self.roll_up_sub_goal_signals(&execution.result.signals);
            self.emit_sub_goal_completed(index, total, &execution.result);
            results.push(execution.result);
        }
        results
    }

    async fn execute_sub_goals_concurrent(
        &mut self,
        plan: &DecompositionPlan,
        llm: &dyn LlmProvider,
        context_messages: &[Message],
    ) -> Vec<SubGoalResult> {
        let count = plan.sub_goals.len().max(1);
        // Concurrent sub-goals split the budget equally (1/N each) since all
        // start from the same remaining budget snapshot and can't see each
        // other's consumption until aggregation.
        let per_goal_fraction = 1.0 / count as f32;
        let total = plan.sub_goals.len();
        for (index, sub_goal) in plan.sub_goals.iter().enumerate() {
            self.emit_sub_goal_progress(index, total, &sub_goal.description);
        }

        let child_futures =
            self.build_concurrent_futures(plan, per_goal_fraction, llm, context_messages);
        let executions = futures_util::future::join_all(child_futures).await;
        self.collect_concurrent_results(executions, total)
    }

    /// Build async futures for each sub-goal in the plan.
    ///
    /// Uses `futures_util::join_all` to multiplex all futures on the current
    /// tokio task (cooperative concurrency). This is ideal for I/O-bound LLM
    /// calls but does not achieve true thread-level parallelism. We cannot use
    /// `tokio::JoinSet` because `llm: &dyn LlmProvider` is borrowed (not `'static`).
    fn build_concurrent_futures<'a>(
        &self,
        plan: &'a DecompositionPlan,
        fraction: f32,
        llm: &'a dyn LlmProvider,
        context_messages: &'a [Message],
    ) -> Vec<impl std::future::Future<Output = SubGoalExecution> + 'a> {
        plan.sub_goals
            .iter()
            .map(|sub_goal| {
                let ts = current_time_ms();
                let budget = self.budget.child_tracker(fraction, ts);
                let mut child = self.build_child_engine(budget);
                let snap = build_sub_goal_snapshot(sub_goal, context_messages, ts);
                let goal = sub_goal.clone();
                async move {
                    let result = match Box::pin(child.run_cycle(snap, llm)).await {
                        Ok(r) => sub_goal_result_from_loop(goal, r),
                        Err(e) => failed_sub_goal_result(goal, e.reason),
                    };
                    SubGoalExecution {
                        result,
                        budget: child.budget,
                    }
                }
            })
            .collect()
    }

    fn collect_concurrent_results(
        &mut self,
        executions: Vec<SubGoalExecution>,
        total: usize,
    ) -> Vec<SubGoalResult> {
        let mut results = Vec::with_capacity(total);
        for (index, execution) in executions.into_iter().enumerate() {
            self.budget.absorb_child_usage(&execution.budget);
            self.roll_up_sub_goal_signals(&execution.result.signals);
            self.emit_sub_goal_completed(index, total, &execution.result);
            results.push(execution.result);
        }
        results
    }

    fn emit_sub_goal_completed(&self, index: usize, total: usize, result: &SubGoalResult) {
        let success = matches!(result.outcome, SubGoalOutcome::Completed(_));
        if let Some(bus) = &self.event_bus {
            let _ = bus.publish(fx_core::message::InternalMessage::SubGoalCompleted {
                index,
                total,
                success,
            });
        }
    }

    async fn run_sub_goal(
        &self,
        sub_goal: &SubGoal,
        fraction: f32,
        llm: &dyn LlmProvider,
        context_messages: &[Message],
    ) -> SubGoalExecution {
        let timestamp_ms = current_time_ms();
        let child_budget = self.budget.child_tracker(fraction, timestamp_ms);
        let mut child = self.build_child_engine(child_budget);
        let snapshot = build_sub_goal_snapshot(sub_goal, context_messages, timestamp_ms);

        let result = match Box::pin(child.run_cycle(snapshot, llm)).await {
            Ok(result) => sub_goal_result_from_loop(sub_goal.clone(), result),
            Err(error) => failed_sub_goal_result(sub_goal.clone(), error.reason),
        };

        SubGoalExecution {
            result,
            budget: child.budget,
        }
    }

    fn build_child_engine(&self, budget: BudgetTracker) -> LoopEngine {
        let mut child = LoopEngine::new(
            budget,
            self.context.clone(),
            child_max_iterations(self.max_iterations),
            Arc::clone(&self.tool_executor),
            self.synthesis_instruction.clone(),
        );

        if let Some(memory_context) = &self.memory_context {
            child.set_memory_context(memory_context.clone());
        }
        if let Some(cancel_token) = &self.cancel_token {
            child.set_cancel_token(cancel_token.clone());
        }
        if let Some(bus) = &self.event_bus {
            child.set_event_bus(bus.clone());
        }

        child
    }

    fn decomposition_depth_limited(&self) -> bool {
        matches!(
            self.budget.check(&ActionCost::default()),
            Err(crate::budget::BudgetExceeded {
                resource: BudgetResource::RecursionDepth,
                ..
            })
        )
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
        if let Some(bus) = &self.event_bus {
            let _ = bus.publish(fx_core::message::InternalMessage::SubGoalStarted {
                index,
                total,
                description: description.to_string(),
            });
        }
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
        for signal in signals {
            self.signals.emit(signal.clone());
        }
    }

    /// Verify step.
    async fn verify(&mut self, action: &ActionResult) -> Result<Verification, LoopError> {
        let mut discrepancies = Vec::new();
        let has_tool_failure = action.tool_results.iter().any(|result| !result.success);
        let has_response = !action.response_text.trim().is_empty();

        // Tool errors are informational when synthesis already produced a response.
        // The synthesis prompt includes the error and the error relay instruction
        // guides the LLM to surface it directly. Retrying blindly produces worse
        // results because the continuation message confuses the model.
        if has_tool_failure && !has_response {
            discrepancies.push("tool calls failed and produced no response".to_string());
        }

        if !has_response && !has_tool_failure {
            discrepancies.push("action produced an empty response".to_string());
        }

        // Detect safe fallback responses — the model returned no tool calls
        // and produced empty/unparseable text that was replaced by the fallback.
        // This triggers a retry with a tool-directive continuation message.
        if action.response_text == SAFE_FALLBACK_RESPONSE && action.tool_results.is_empty() {
            discrepancies.push(
                "model produced fallback instead of using tools or giving a substantive answer"
                    .to_string(),
            );
        }

        let verification = build_verification(discrepancies);
        self.emit_verification_signals(&verification);
        Ok(verification)
    }

    /// Learn step.
    async fn learn(&mut self, verification: &Verification) -> Result<Learning, LoopError> {
        let episode = if verification.outcome_satisfactory {
            "Action completed satisfactorily.".to_string()
        } else {
            format!(
                "Verification found discrepancies: {}",
                verification.discrepancies.join("; ")
            )
        };

        let pattern = if verification.outcome_satisfactory {
            None
        } else {
            Some("mismatch_between_intended_and_observed_outcome".to_string())
        };

        let adjustment = if verification.outcome_satisfactory {
            None
        } else {
            Some("ask_for_clarification_or_refine_reasoning_prompt".to_string())
        };

        Ok(Learning {
            episode,
            pattern,
            adjustment,
        })
    }

    /// Continue step.
    async fn should_continue(
        &mut self,
        decision: &Decision,
        verification: &Verification,
        _learning: &Learning,
    ) -> Result<Continuation, LoopError> {
        // Note: Clarify and Defer are not produced by decide() in the current
        // loop engine flow, but are kept for external callers (Decision is pub).
        if let Decision::Clarify(prompt) | Decision::Defer(prompt) = decision {
            let continuation = Continuation::NeedsInput(prompt.clone());
            self.emit_continue_signal(&continuation);
            return Ok(continuation);
        }

        if verification.outcome_satisfactory {
            let continuation = Continuation::Complete;
            self.emit_continue_signal(&continuation);
            return Ok(continuation);
        }

        // Post-Phase-2: CONFIDENCE_CLARIFY_THRESHOLD gates whether a
        // low-confidence verification triggers a user-facing clarification
        // request. This keeps the verify→continue safety net independent
        // of the removed ReasonedIntent confidence gates.
        if verification.confidence < CONFIDENCE_CLARIFY_THRESHOLD {
            let continuation = Continuation::NeedsInput(
                "I need a bit more detail to continue safely. Could you clarify your goal?"
                    .to_string(),
            );
            self.emit_continue_signal(&continuation);
            return Ok(continuation);
        }

        // When the model produced a safe fallback (no tools, no real response),
        // retry with a tool-directive message to nudge toward tool use.
        let is_fallback =
            matches!(decision, Decision::Respond(text) if text == SAFE_FALLBACK_RESPONSE);
        let message = if is_fallback {
            "The previous attempt did not use tools. The user's question likely requires gathering information. Use the available tools (read_file, list_directory, search_text, etc.) to find the answer instead of responding with text alone."
        } else {
            "The previous attempt produced no response. Try a different approach to answer the user's question."
        };
        let continuation = Continuation::Continue(message.to_string());
        self.emit_continue_signal(&continuation);
        Ok(continuation)
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

    fn emit_action_signals(&mut self, results: &[ToolResult]) {
        for result in results {
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
            self.emit_signal(
                LoopStep::Act,
                kind,
                format!("tool {}", result.tool_name),
                serde_json::json!({"success": result.success, "output": truncated_output}),
            );
        }
    }

    fn emit_verification_signals(&mut self, verification: &Verification) {
        self.emit_signal(
            LoopStep::Verify,
            SignalKind::Decision,
            "verification evaluated",
            serde_json::json!({
                "outcome_satisfactory": verification.outcome_satisfactory,
                "confidence": verification.confidence,
            }),
        );
        if !verification.discrepancies.is_empty() {
            self.emit_signal(
                LoopStep::Verify,
                SignalKind::Friction,
                "verification discrepancy found",
                serde_json::json!({"discrepancies": verification.discrepancies}),
            );
        }
    }

    fn emit_continue_signal(&mut self, continuation: &Continuation) {
        self.emit_signal(
            LoopStep::Continue,
            SignalKind::Decision,
            "continuation decided",
            serde_json::json!({"continuation": continuation_label(continuation)}),
        );
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
        ActionResult {
            decision: decision.clone(),
            tool_results: Vec::new(),
            response_text: ensure_non_empty_response(text),
            tokens_used: TokenUsage::default(),
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
        tokens_used: TokenUsage,
    ) -> ActionResult {
        ActionResult {
            decision: decision.clone(),
            tool_results,
            response_text: SAFE_FALLBACK_RESPONSE.to_string(),
            tokens_used,
        }
    }

    fn cancelled_tool_action_from_state(
        &self,
        decision: &Decision,
        state: ToolRoundState,
    ) -> ActionResult {
        self.cancelled_tool_action(decision, state.all_tool_results, state.tokens_used)
    }

    // Evaluated introducing a ToolActionContext wrapper here, but kept explicit
    // arguments because there are only four call-site inputs and bundling them
    // made the call site less readable.
    async fn act_with_tools(
        &mut self,
        decision: &Decision,
        calls: &[ToolCall],
        llm: &dyn LlmProvider,
        context_messages: &[Message],
    ) -> Result<ActionResult, LoopError> {
        let mut state = ToolRoundState::new(calls, context_messages);

        for round in 0..self.max_iterations {
            if self.tool_round_interrupted() {
                return Ok(self.cancelled_tool_action_from_state(decision, state));
            }

            match self.execute_tool_round(round + 1, llm, &mut state).await? {
                ToolRoundOutcome::Cancelled => {
                    return Ok(self.cancelled_tool_action_from_state(decision, state));
                }
                ToolRoundOutcome::Response(response) => {
                    if !response.tool_calls.is_empty() {
                        state.current_calls = response.tool_calls;
                        continue;
                    }

                    let response = self
                        .continue_truncated_response(
                            response,
                            &state.continuation_messages,
                            llm,
                            LoopStep::Act,
                        )
                        .await?;

                    return Ok(self.finalize_tool_response(
                        decision,
                        state.all_tool_results,
                        &response,
                        state.tokens_used,
                    ));
                }
            }
        }

        self.synthesize_tool_fallback(decision, state.all_tool_results, state.tokens_used, llm)
            .await
    }

    async fn execute_tool_round(
        &mut self,
        round: u32,
        llm: &dyn LlmProvider,
        state: &mut ToolRoundState,
    ) -> Result<ToolRoundOutcome, LoopError> {
        let round_started = current_time_ms();
        let results = self.execute_tool_calls(&state.current_calls).await?;
        append_tool_round_messages(
            &mut state.continuation_messages,
            &state.current_calls,
            &results,
        )?;
        state.all_tool_results.extend(results);

        if self.cancellation_token_triggered() {
            return Ok(ToolRoundOutcome::Cancelled);
        }

        let response = self
            .request_tool_continuation(llm, &state.continuation_messages, &mut state.tokens_used)
            .await?;
        self.emit_tool_round_trace_and_perf(
            round,
            state.current_calls.len(),
            &response,
            current_time_ms().saturating_sub(round_started),
        );

        if self.cancellation_token_triggered() {
            return Ok(ToolRoundOutcome::Cancelled);
        }

        Ok(ToolRoundOutcome::Response(response))
    }

    async fn execute_tool_calls(&self, calls: &[ToolCall]) -> Result<Vec<ToolResult>, LoopError> {
        self.tool_executor
            .execute_tools(calls, self.cancel_token.as_ref())
            .await
            .map_err(|error| {
                loop_error(
                    "act",
                    &format!("tool execution failed: {}", error.message),
                    error.recoverable,
                )
            })
    }

    async fn request_tool_continuation(
        &self,
        llm: &dyn LlmProvider,
        context_messages: &[Message],
        tokens_used: &mut TokenUsage,
    ) -> Result<CompletionResponse, LoopError> {
        let request = build_continuation_request(
            context_messages,
            llm.model_name(),
            self.tool_executor.tool_definitions(),
            self.memory_context.as_deref(),
        );
        let response = llm.complete(request).await.map_err(|error| {
            loop_error(
                "act",
                &format!("tool continuation completion failed: {error}"),
                true,
            )
        })?;
        tokens_used.accumulate(response_usage_or_estimate(&response, context_messages));
        Ok(response)
    }

    fn finalize_tool_response(
        &mut self,
        decision: &Decision,
        tool_results: Vec<ToolResult>,
        response: &CompletionResponse,
        tokens_used: TokenUsage,
    ) -> ActionResult {
        let text = extract_response_text(response);
        let readable = extract_readable_text(&text);
        let (response_text, used_fallback) = ensure_non_empty_response_with_flag(&readable);
        if used_fallback {
            self.emit_signal(
                LoopStep::Act,
                SignalKind::Trace,
                "tool continuation returned empty text; using safe fallback",
                serde_json::json!({
                    "tool_count": tool_results.len(),
                }),
            );
        }
        ActionResult {
            decision: decision.clone(),
            tool_results,
            response_text,
            tokens_used,
        }
    }

    async fn synthesize_tool_fallback(
        &self,
        decision: &Decision,
        tool_results: Vec<ToolResult>,
        mut tokens_used: TokenUsage,
        llm: &dyn LlmProvider,
    ) -> Result<ActionResult, LoopError> {
        let synthesis_prompt = tool_synthesis_prompt(&tool_results, &self.synthesis_instruction);
        let llm_text = self.generate_tool_summary(&synthesis_prompt, llm).await?;
        tokens_used.accumulate(synthesis_usage(&synthesis_prompt, &llm_text));
        Ok(ActionResult {
            decision: decision.clone(),
            tool_results,
            response_text: ensure_non_empty_response(&llm_text),
            tokens_used,
        })
    }

    async fn generate_tool_summary(
        &self,
        synthesis_prompt: &str,
        llm: &dyn LlmProvider,
    ) -> Result<String, LoopError> {
        let chunks = Arc::new(Mutex::new(Vec::new()));
        let callback_chunks = Arc::clone(&chunks);
        let callback = Box::new(move |chunk: String| {
            if let Ok(mut guard) = callback_chunks.lock() {
                guard.push(chunk);
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
            } else if action.tool_results.is_empty() {
                0
            } else {
                1
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

/// Cap child iterations at 3, with a floor of 1.
/// Note: for parent max_iterations <= 3, children get the same count
/// as the parent. This is intentional — sub-goals should be focused
/// and complete within their allocation.
fn child_max_iterations(max_iterations: u32) -> u32 {
    max_iterations.clamp(1, 3)
}

fn build_sub_goal_snapshot(
    sub_goal: &SubGoal,
    context_messages: &[Message],
    timestamp_ms: u64,
) -> PerceptionSnapshot {
    let description = sub_goal.description.clone();
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
        }),
        sensor_data: None,
        conversation_history: context_messages.to_vec(),
    }
}

fn sub_goal_result_from_loop(goal: SubGoal, result: LoopResult) -> SubGoalResult {
    match result {
        LoopResult::Complete {
            response, signals, ..
        } => SubGoalResult {
            goal,
            outcome: SubGoalOutcome::Completed(response),
            signals,
        },
        LoopResult::BudgetExhausted { signals, .. } => SubGoalResult {
            goal,
            outcome: SubGoalOutcome::BudgetExhausted,
            signals,
        },
        LoopResult::Error {
            message, signals, ..
        } => failed_sub_goal_result_with_signals(goal, message, signals),
        LoopResult::NeedsInput {
            prompt, signals, ..
        } => {
            let message = format!("sub-goal needs user input: {prompt}");
            failed_sub_goal_result_with_signals(goal, message, signals)
        }
        LoopResult::UserStopped { signals, .. } => {
            let message = "sub-goal stopped before completion".to_string();
            failed_sub_goal_result_with_signals(goal, message, signals)
        }
    }
}

fn failed_sub_goal_result(goal: SubGoal, message: String) -> SubGoalResult {
    failed_sub_goal_result_with_signals(goal, message, Vec::new())
}

fn failed_sub_goal_result_with_signals(
    goal: SubGoal,
    message: String,
    signals: Vec<Signal>,
) -> SubGoalResult {
    SubGoalResult {
        goal,
        outcome: SubGoalOutcome::Failed(message),
        signals,
    }
}

fn aggregate_sub_goal_results(results: &[SubGoalResult]) -> String {
    if results.is_empty() {
        return "Task decomposition contained no sub-goals.".to_string();
    }

    let mut lines = Vec::with_capacity(results.len() + 1);
    lines.push("Task decomposition results:".to_string());
    for (index, result) in results.iter().enumerate() {
        lines.push(format_sub_goal_line(index + 1, result));
    }
    lines.join("\n")
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
        SubGoalOutcome::Failed(message) => format!("failed: {message}"),
        SubGoalOutcome::BudgetExhausted => "budget exhausted".to_string(),
    }
}

fn find_decompose_tool_call(tool_calls: &[ToolCall]) -> Option<&ToolCall> {
    tool_calls
        .iter()
        .find(|call| call.name == DECOMPOSE_TOOL_NAME)
}

fn parse_decomposition_plan(arguments: &serde_json::Value) -> Result<DecompositionPlan, LoopError> {
    let parsed = parse_decompose_arguments(arguments)?;
    if let Some(strategy) = &parsed.strategy {
        if matches!(strategy, AggregationStrategy::Custom(_)) {
            return Err(loop_error(
                "decide",
                &format!("unsupported decomposition strategy: {strategy:?}"),
                false,
            ));
        }
    }

    if parsed.sub_goals.is_empty() {
        return Err(loop_error(
            "decide",
            "decompose tool requires at least one sub_goal",
            false,
        ));
    }

    let mut sub_goals: Vec<SubGoal> = parsed.sub_goals.into_iter().map(SubGoal::from).collect();
    let truncated_from = if sub_goals.len() > MAX_SUB_GOALS {
        let original_sub_goals = sub_goals.len();
        sub_goals.truncate(MAX_SUB_GOALS);
        Some(original_sub_goals)
    } else {
        None
    };

    Ok(DecompositionPlan {
        sub_goals,
        strategy: parsed.strategy.unwrap_or(AggregationStrategy::Sequential),
        truncated_from,
    })
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

fn decision_variant(decision: &Decision) -> &'static str {
    match decision {
        Decision::Respond(_) => "Respond",
        Decision::UseTools(_) => "UseTools",
        Decision::Clarify(_) => "Clarify",
        Decision::Defer(_) => "Defer",
        Decision::Decompose(_) => "Decompose",
    }
}

fn continuation_label(continuation: &Continuation) -> &'static str {
    match continuation {
        Continuation::Complete => "complete",
        Continuation::NeedsInput(_) => "needs_input",
        Continuation::Continue(_) => "continue",
    }
}

fn attach_signals(result: LoopResult, signals: Vec<Signal>) -> LoopResult {
    match result {
        LoopResult::Complete {
            response,
            iterations,
            tokens_used,
            learnings,
            ..
        } => LoopResult::Complete {
            response,
            iterations,
            tokens_used,
            learnings,
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
        LoopResult::NeedsInput {
            prompt, iterations, ..
        } => LoopResult::NeedsInput {
            prompt,
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

fn compacted_context_summary(context: &ReasoningContext) -> Option<&str> {
    context
        .working_memory
        .iter()
        .find(|entry| entry.key == "compacted_context_summary")
        .map(|entry| entry.value.as_str())
}

fn tool_synthesis_prompt(tool_results: &[ToolResult], instruction: &str) -> String {
    let has_tool_error = tool_results.iter().any(|result| !result.success);
    let error_relay_instruction = if has_tool_error {
        "\nIf any tool returned an error, tell the user exactly what went wrong — include the actual error message. Do not soften, hedge, or paraphrase errors."
    } else {
        ""
    };
    let tool_summary = tool_results
        .iter()
        .map(|result| format!("- {}: {}", result.tool_name, result.output))
        .collect::<Vec<_>>()
        .join("\n");

    format!("You are Fawx. Never introduce yourself, greet the user, or add preamble. Answer the user's question using these tool results. \
Do NOT describe what tools were called, narrate the process, or comment on how you got the information. \
Just provide the answer directly. \
If the user asked for a specific format or value type, preserve that exact format. \
Do not convert timestamps to human-readable, counts to lists, or raw values to prose \
unless the user explicitly asked for that.{error_relay_instruction}\n\n\
{instruction}\n\n\
Tool results:\n{tool_summary}")
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

fn append_tool_round_messages(
    context_messages: &mut Vec<Message>,
    calls: &[ToolCall],
    results: &[ToolResult],
) -> Result<(), LoopError> {
    let assistant_message = build_tool_use_assistant_message(calls);
    let result_message = build_tool_result_message(calls, results)?;
    context_messages.push(assistant_message);
    context_messages.push(result_message);
    Ok(())
}

/// Build an assistant message containing ToolUse content blocks.
fn build_tool_use_assistant_message(calls: &[ToolCall]) -> Message {
    let content = calls
        .iter()
        .map(|call| ContentBlock::ToolUse {
            id: call.id.clone(),
            name: call.name.clone(),
            input: call.arguments.clone(),
        })
        .collect();
    Message {
        role: MessageRole::Assistant,
        content,
    }
}

/// Build a tool message containing ToolResult content blocks.
///
/// Returns an error if any result has a `tool_call_id` not found in `calls`.
fn build_tool_result_message(
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
            content: if result.success {
                serde_json::Value::String(result.output.clone())
            } else {
                serde_json::Value::String(format!("[ERROR] {}", result.output))
            },
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

fn tool_definitions_with_decompose(
    mut tool_definitions: Vec<ToolDefinition>,
) -> Vec<ToolDefinition> {
    let has_decompose = tool_definitions
        .iter()
        .any(|tool| tool.name == DECOMPOSE_TOOL_NAME);
    if !has_decompose {
        tool_definitions.push(decompose_tool_definition());
    }
    tool_definitions
}

fn decompose_tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: DECOMPOSE_TOOL_NAME.to_string(),
        description: DECOMPOSE_TOOL_DESCRIPTION.to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "sub_goals": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "description": {"type": "string", "description": "What this sub-goal should accomplish"},
                            "required_tools": {"type": "array", "items": {"type": "string"}, "description": "Tools needed for this sub-goal"},
                            "expected_output": {"type": "string", "description": "What the result should look like"}
                        },
                        "required": ["description"]
                    },
                    "description": "List of sub-goals to execute"
                },
                "strategy": {"type": "string", "enum": ["Sequential"], "description": "Execution strategy (only Sequential supported currently)"}
            },
            "required": ["sub_goals"]
        }),
    }
}

/// Build a CompletionRequest for tool result re-prompting.
fn build_continuation_request(
    context_messages: &[Message],
    model: &str,
    tool_definitions: Vec<ToolDefinition>,
    memory_context: Option<&str>,
) -> CompletionRequest {
    let tools = tool_definitions_with_decompose(tool_definitions);
    let system_prompt = build_reasoning_system_prompt(&tools, memory_context);
    CompletionRequest {
        model: model.to_string(),
        messages: context_messages.to_vec(),
        tools,
        temperature: Some(REASONING_TEMPERATURE),
        max_tokens: Some(REASONING_MAX_OUTPUT_TOKENS),
        system_prompt: Some(system_prompt),
    }
}

fn build_truncation_continuation_request(
    model: &str,
    continuation_messages: &[Message],
    tool_definitions: Vec<ToolDefinition>,
    memory_context: Option<&str>,
    step: LoopStep,
) -> CompletionRequest {
    let tools = tool_definitions_with_decompose(tool_definitions);
    let system_prompt = build_reasoning_system_prompt(&tools, memory_context);
    CompletionRequest {
        model: model.to_string(),
        messages: continuation_messages.to_vec(),
        tools: continuation_tools_for_step(step, tools),
        temperature: Some(REASONING_TEMPERATURE),
        max_tokens: Some(REASONING_MAX_OUTPUT_TOKENS),
        system_prompt: Some(system_prompt),
    }
}

fn continuation_tools_for_step(step: LoopStep, tools: Vec<ToolDefinition>) -> Vec<ToolDefinition> {
    match step {
        LoopStep::Reason => tools,
        _ => Vec::new(),
    }
}

fn build_continuation_messages(base_messages: &[Message], full_text: &str) -> Vec<Message> {
    let mut continuation_messages = base_messages.to_vec();
    continuation_messages.push(Message::assistant(full_text.to_string()));
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
        })
        .sum();
    let text = extract_response_text(response);
    TokenUsage {
        input_tokens: prompt_estimate,
        output_tokens: estimate_tokens(&text),
    }
}

fn build_verification(discrepancies: Vec<String>) -> Verification {
    let confidence = if discrepancies.is_empty() {
        VERIFICATION_CONFIDENCE_CLEAN
    } else if discrepancies.len() == 1 {
        VERIFICATION_CONFIDENCE_SINGLE_DISCREPANCY
    } else {
        VERIFICATION_CONFIDENCE_MULTIPLE_DISCREPANCIES
    };

    Verification {
        outcome_satisfactory: discrepancies.is_empty(),
        confidence,
        discrepancies,
    }
}

fn reasoning_token_usage(total_tokens: u64) -> TokenUsage {
    TokenUsage {
        input_tokens: total_tokens.saturating_mul(3) / 5,
        output_tokens: total_tokens.saturating_mul(2) / 5,
    }
}

fn next_perception_from_sub_goal(
    previous: &PerceptionSnapshot,
    sub_goal: &str,
) -> PerceptionSnapshot {
    let timestamp_ms = previous.timestamp_ms.saturating_add(1);
    let mut next = previous.clone();
    next.timestamp_ms = timestamp_ms;
    next.user_input = Some(UserInput {
        text: sub_goal.to_string(),
        source: InputSource::Text,
        timestamp: timestamp_ms,
        context_id: Some("loop-continuation".to_string()),
    });
    next
}

fn estimate_tokens(text: &str) -> u64 {
    if text.trim().is_empty() {
        return 0;
    }

    let char_count = text.chars().count();
    let char_estimate = char_count.div_ceil(4) as u64;
    let word_estimate = text.split_whitespace().count() as u64;
    char_estimate.max(word_estimate).max(1)
}

fn message_to_text(message: &Message) -> String {
    let role = format!("{:?}", message.role);
    let content = message
        .content
        .iter()
        .map(|block| match block {
            fx_llm::ContentBlock::Text { text } => text.clone(),
            fx_llm::ContentBlock::ToolUse { name, .. } => format!("[tool_use:{name}]"),
            fx_llm::ContentBlock::ToolResult { tool_use_id, .. } => {
                format!("[tool_result:{tool_use_id}]")
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    format!("{role}: {content}")
}

fn completion_request_to_prompt(request: &CompletionRequest) -> String {
    let system = request
        .system_prompt
        .as_deref()
        .map(|prompt| {
            format!(
                "System:
{prompt}

"
            )
        })
        .unwrap_or_default();
    let messages = request
        .messages
        .iter()
        .map(message_to_text)
        .collect::<Vec<_>>()
        .join(
            "
",
        );

    format!("{system}{messages}")
}

fn build_reasoning_request(
    perception: &ProcessedPerception,
    model: &str,
    tool_definitions: Vec<ToolDefinition>,
    memory_context: Option<&str>,
) -> CompletionRequest {
    let context = perception.context_window.clone();
    let user_prompt = reasoning_user_prompt(perception);
    let tools = tool_definitions_with_decompose(tool_definitions);
    let system_prompt = build_reasoning_system_prompt(&tools, memory_context);

    CompletionRequest {
        model: model.to_string(),
        messages: [context, vec![Message::user(user_prompt)]].concat(),
        tools,
        temperature: Some(REASONING_TEMPERATURE),
        max_tokens: Some(REASONING_MAX_OUTPUT_TOKENS),
        system_prompt: Some(system_prompt),
    }
}

fn reasoning_user_prompt(perception: &ProcessedPerception) -> String {
    format!(
        "Active goals:
- {}

Budget remaining: {} tokens, {} llm calls

User message:
{}",
        perception.active_goals.join(
            "
- "
        ),
        perception.budget_remaining.tokens,
        perception.budget_remaining.llm_calls,
        perception.user_message,
    )
}

fn build_reasoning_system_prompt(
    tool_definitions: &[ToolDefinition],
    memory_context: Option<&str>,
) -> String {
    let mut prompt = format!(
        "{REASONING_SYSTEM_PROMPT}

{}",
        available_tools_instructions(tool_definitions)
    );
    if let Some(mem) = memory_context {
        prompt.push_str("\n\n");
        prompt.push_str(mem);
        prompt.push_str(MEMORY_INSTRUCTION);
    }
    prompt
}

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
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn ensure_non_empty_response(text: &str) -> String {
    ensure_non_empty_response_with_flag(text).0
}

fn ensure_non_empty_response_with_flag(text: &str) -> (String, bool) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return (SAFE_FALLBACK_RESPONSE.to_string(), true);
    }
    (trimmed.to_string(), false)
}

fn loop_error(stage: &str, reason: &str, recoverable: bool) -> LoopError {
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
mod tests {
    use super::*;
    use async_trait::async_trait;
    use fx_core::error::LlmError as CoreLlmError;
    use fx_core::types::{InputSource, ScreenState, UserInput};
    use fx_llm::{
        CompletionResponse, ContentBlock, Message, ProviderError, ToolCall, ToolDefinition,
    };
    use std::collections::VecDeque;
    use std::sync::Mutex;

    #[derive(Debug, Default)]
    struct TestStubToolExecutor;

    #[async_trait]
    impl ToolExecutor for TestStubToolExecutor {
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
                })
                .collect())
        }

        fn tool_definitions(&self) -> Vec<ToolDefinition> {
            vec![ToolDefinition {
                name: "read_file".to_string(),
                description: "Read a file".to_string(),
                parameters: serde_json::json!({"type":"object"}),
            }]
        }
    }

    #[derive(Debug)]
    struct MockLlm {
        responses: Mutex<VecDeque<CompletionResponse>>,
    }

    impl MockLlm {
        fn new(responses: Vec<CompletionResponse>) -> Self {
            Self {
                responses: Mutex::new(VecDeque::from(responses)),
            }
        }
    }

    #[async_trait]
    impl LlmProvider for MockLlm {
        async fn generate(&self, _: &str, _: u32) -> Result<String, CoreLlmError> {
            Ok("summary".to_string())
        }

        async fn generate_streaming(
            &self,
            _: &str,
            _: u32,
            callback: Box<dyn Fn(String) + Send + 'static>,
        ) -> Result<String, CoreLlmError> {
            callback("summary".to_string());
            Ok("summary".to_string())
        }

        fn model_name(&self) -> &str {
            "mock"
        }

        async fn complete(
            &self,
            _: CompletionRequest,
        ) -> Result<CompletionResponse, ProviderError> {
            self.responses
                .lock()
                .expect("lock")
                .pop_front()
                .ok_or_else(|| ProviderError::Provider("no response".to_string()))
        }
    }

    fn default_engine() -> LoopEngine {
        LoopEngine::new(
            BudgetTracker::new(crate::budget::BudgetConfig::default(), 0, 0),
            ContextCompactor::new(2048, 256),
            3,
            Arc::new(TestStubToolExecutor),
            "Summarize tool output".to_string(),
        )
    }

    fn base_snapshot(text: &str) -> PerceptionSnapshot {
        PerceptionSnapshot {
            timestamp_ms: 1,
            screen: ScreenState {
                current_app: "terminal".to_string(),
                elements: Vec::new(),
                text_content: text.to_string(),
            },
            notifications: Vec::new(),
            active_app: "terminal".to_string(),
            user_input: Some(UserInput {
                text: text.to_string(),
                source: InputSource::Text,
                timestamp: 1,
                context_id: None,
            }),
            sensor_data: None,
            conversation_history: vec![Message::user(text)],
        }
    }

    #[test]
    fn system_prompt_includes_tool_use_guidance() {
        let defs = vec![ToolDefinition {
            name: "current_time".to_string(),
            description: "Get the current time".to_string(),
            parameters: serde_json::json!({"type": "object", "properties": {}, "required": []}),
        }];
        let prompt = build_reasoning_system_prompt(&defs, None);
        assert!(
            prompt.contains("Use tools when you need information not already in the conversation")
        );
        assert!(
            prompt.contains(
                "When the user's request relates to an available tool's purpose, prefer calling the tool"
            ),
            "system prompt should encourage proactive tool usage for matching requests"
        );
        assert!(prompt.contains("current time"));
    }

    #[test]
    fn system_prompt_prohibits_greeting_and_preamble() {
        let defs = vec![ToolDefinition {
            name: "current_time".to_string(),
            description: "Get the current time".to_string(),
            parameters: serde_json::json!({"type": "object", "properties": {}, "required": []}),
        }];
        let prompt = build_reasoning_system_prompt(&defs, None);
        assert!(
            prompt.contains("Never introduce yourself"),
            "system prompt must prohibit self-introduction (issue #959)"
        );
        assert!(
            prompt.contains("greet the user"),
            "system prompt must prohibit greeting (issue #959)"
        );
    }

    #[test]
    fn system_prompt_without_memory_omits_persistent_memory_block() {
        let defs = vec![ToolDefinition {
            name: "current_time".to_string(),
            description: "Get the current time".to_string(),
            parameters: serde_json::json!({"type": "object", "properties": {}, "required": []}),
        }];
        let prompt = build_reasoning_system_prompt(&defs, None);
        assert!(
            !prompt.contains("You have persistent memory across sessions"),
            "system prompt without memory context should NOT include the persistent memory block"
        );
    }

    #[test]
    fn system_prompt_with_memory_includes_memory_instruction() {
        let defs = vec![ToolDefinition {
            name: "memory_write".to_string(),
            description: "Store a fact".to_string(),
            parameters: serde_json::json!({"type": "object"}),
        }];
        let prompt = build_reasoning_system_prompt(&defs, Some("user prefers dark mode"));
        assert!(
            prompt.contains("memory_write"),
            "system prompt with memory context should mention memory_write"
        );
        assert!(
            prompt.contains("user prefers dark mode"),
            "system prompt should include the memory context"
        );
    }

    #[test]
    fn tool_synthesis_prompt_content_is_complete() {
        let results = vec![ToolResult {
            tool_call_id: "call-1".to_string(),
            tool_name: "current_time".to_string(),
            output: "2026-02-28T14:00:00Z".to_string(),
            success: true,
        }];
        let prompt = tool_synthesis_prompt(&results, "Tell the user the time.");
        assert!(
            prompt.contains("You are Fawx"),
            "synthesis prompt must include assistant identity"
        );
        assert!(
            prompt.contains("Answer the user's question using these tool results"),
            "synthesis prompt must instruct direct answering"
        );
        assert!(
            prompt.contains("Do NOT describe what tools were called"),
            "synthesis prompt must block meta-narration"
        );
        assert!(
            prompt.contains(
                "If the user asked for a specific format or value type, preserve that exact format."
            ),
            "synthesis prompt must preserve requested output formats"
        );
        assert!(
            prompt.contains(
                "Do not convert timestamps to human-readable, counts to lists, or raw values to prose unless the user explicitly asked for that."
            ),
            "synthesis prompt must forbid format rewriting"
        );
        assert!(
            prompt.contains("Tell the user the time."),
            "synthesis prompt must include the instruction"
        );
        assert!(
            prompt.contains("current_time: 2026-02-28T14:00:00Z"),
            "synthesis prompt must include tool results"
        );
    }

    #[test]
    fn tool_synthesis_prompt_explicitly_prohibits_intro_and_greeting() {
        let prompt = tool_synthesis_prompt(&[], "Combine outputs");
        assert!(
            prompt.contains("Never introduce yourself, greet the user, or add preamble"),
            "synthesis prompt should mirror no-intro guidance from reasoning prompt"
        );
    }

    #[test]
    fn synthesis_includes_all_results() {
        let results = vec![
            ToolResult {
                tool_call_id: "call-1".to_string(),
                tool_name: "read_file".to_string(),
                output: "alpha".to_string(),
                success: true,
            },
            ToolResult {
                tool_call_id: "call-2".to_string(),
                tool_name: "search".to_string(),
                output: "beta".to_string(),
                success: true,
            },
        ];

        let prompt = tool_synthesis_prompt(&results, "Combine outputs");

        assert!(prompt.contains("read_file: alpha"));
        assert!(prompt.contains("search: beta"));

        let tool_results_section = prompt
            .split("Tool results:\n")
            .nth(1)
            .expect("prompt should include tool results section");
        let result_count = tool_results_section
            .lines()
            .take_while(|line| !line.trim().is_empty())
            .filter(|line| line.starts_with("- "))
            .count();
        assert_eq!(
            result_count, 2,
            "prompt should include exactly 2 tool results"
        );
    }

    #[test]
    fn synthesis_includes_failed_tool_results() {
        let results = vec![
            ToolResult {
                tool_call_id: "call-1".to_string(),
                tool_name: "read_file".to_string(),
                output: "alpha".to_string(),
                success: true,
            },
            ToolResult {
                tool_call_id: "call-2".to_string(),
                tool_name: "run_command".to_string(),
                output: "permission denied".to_string(),
                success: false,
            },
        ];

        let prompt = tool_synthesis_prompt(&results, "Combine outputs");

        assert!(prompt.contains("read_file: alpha"));
        assert!(prompt.contains("run_command: permission denied"));
    }

    #[test]
    fn synthesis_prompt_includes_error_relay_instruction_when_tool_failed() {
        let results = vec![ToolResult {
            tool_call_id: "call-1".to_string(),
            tool_name: "read_file".to_string(),
            output: "file not found: /foo/bar".to_string(),
            success: false,
        }];

        let prompt = tool_synthesis_prompt(&results, "Combine outputs");

        assert!(prompt.contains("If any tool returned an error, tell the user exactly what went wrong — include the actual error message. Do not soften, hedge, or paraphrase errors."));
    }

    #[test]
    fn synthesis_prompt_omits_error_relay_when_all_tools_succeed() {
        let results = vec![ToolResult {
            tool_call_id: "call-1".to_string(),
            tool_name: "read_file".to_string(),
            output: "alpha".to_string(),
            success: true,
        }];

        let prompt = tool_synthesis_prompt(&results, "Combine outputs");

        assert!(!prompt.contains("If any tool returned an error, tell the user exactly what went wrong — include the actual error message. Do not soften, hedge, or paraphrase errors."));
    }

    #[test]
    fn synthesis_prompt_error_relay_with_mixed_results() {
        let results = vec![
            ToolResult {
                tool_call_id: "call-1".to_string(),
                tool_name: "read_file".to_string(),
                output: "alpha".to_string(),
                success: true,
            },
            ToolResult {
                tool_call_id: "call-2".to_string(),
                tool_name: "run_command".to_string(),
                output: "permission denied".to_string(),
                success: false,
            },
        ];

        let prompt = tool_synthesis_prompt(&results, "Combine outputs");

        assert!(prompt.contains("If any tool returned an error, tell the user exactly what went wrong — include the actual error message. Do not soften, hedge, or paraphrase errors."));
    }

    #[test]
    fn synthesis_prompt_handles_empty_tool_results() {
        let prompt = tool_synthesis_prompt(&[], "Combine outputs");

        assert!(!prompt.contains("If any tool returned an error, tell the user exactly what went wrong — include the actual error message. Do not soften, hedge, or paraphrase errors."));
        assert!(prompt.contains("Tool results:\n"));
    }

    #[tokio::test]
    async fn reason_returns_completion_response_with_tool_calls() {
        let mut engine = default_engine();
        let llm = MockLlm::new(vec![CompletionResponse {
            content: Vec::new(),
            tool_calls: vec![ToolCall {
                id: "1".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path":"Cargo.toml"}),
            }],
            usage: None,
            stop_reason: None,
        }]);

        let perception = engine
            .perceive(&base_snapshot("read"))
            .await
            .expect("perceive");
        let response = engine.reason(&perception, &llm).await.expect("reason");
        assert_eq!(response.tool_calls.len(), 1);
    }

    #[tokio::test]
    async fn decide_maps_text_response_to_respond_decision() {
        let mut engine = default_engine();
        let response = CompletionResponse {
            content: vec![ContentBlock::Text {
                text: "hello".to_string(),
            }],
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: None,
        };
        let decision = engine.decide(&response).await.expect("decision");
        assert!(matches!(decision, Decision::Respond(text) if text == "hello"));
    }

    #[tokio::test]
    async fn decide_extracts_single_tool_call() {
        let mut engine = default_engine();
        let response = CompletionResponse {
            content: vec![ContentBlock::Text {
                text: "ignore me".to_string(),
            }],
            tool_calls: vec![ToolCall {
                id: "1".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path":"Cargo.toml"}),
            }],
            usage: None,
            stop_reason: None,
        };
        let decision = engine.decide(&response).await.expect("decision");
        assert!(matches!(decision, Decision::UseTools(calls) if calls.len() == 1));
    }

    #[tokio::test]
    async fn decide_no_tool_calls_returns_safe_fallback() {
        let mut engine = default_engine();
        let response = CompletionResponse {
            content: Vec::new(),
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: None,
        };
        let decision = engine.decide(&response).await.expect("decision");
        assert!(matches!(decision, Decision::Respond(text) if text == SAFE_FALLBACK_RESPONSE));
    }

    #[tokio::test]
    async fn verify_passes_when_tools_succeed_without_intent() {
        let mut engine = default_engine();
        let action = ActionResult {
            decision: Decision::UseTools(vec![ToolCall {
                id: "1".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path":"Cargo.toml"}),
            }]),
            tool_results: vec![ToolResult {
                tool_call_id: "call-1".to_string(),
                tool_name: "read_file".to_string(),
                success: true,
                output: "ok".to_string(),
            }],
            response_text: "done".to_string(),
            tokens_used: TokenUsage::default(),
        };
        let verification = engine.verify(&action).await.expect("verification");
        assert!(verification.outcome_satisfactory);
        assert!(verification.discrepancies.is_empty());
    }
}

#[cfg(test)]
mod phase2_tests {
    use super::*;
    use async_trait::async_trait;
    use fx_core::error::LlmError as CoreLlmError;
    use fx_core::types::{InputSource, ScreenState, UserInput};
    use fx_llm::{
        CompletionResponse, ContentBlock, Message, ProviderError, ToolCall, ToolDefinition,
    };
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    #[derive(Debug, Default)]
    struct StubToolExecutor;

    #[async_trait]
    impl ToolExecutor for StubToolExecutor {
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
                })
                .collect())
        }

        fn tool_definitions(&self) -> Vec<ToolDefinition> {
            vec![ToolDefinition {
                name: "read_file".to_string(),
                description: "Read a file".to_string(),
                parameters: serde_json::json!({"type":"object"}),
            }]
        }
    }

    #[derive(Debug, Default)]
    struct FailingToolExecutor;

    #[async_trait]
    impl ToolExecutor for FailingToolExecutor {
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
                    success: false,
                    output: "path escapes working directory".to_string(),
                })
                .collect())
        }

        fn tool_definitions(&self) -> Vec<ToolDefinition> {
            vec![ToolDefinition {
                name: "read_file".to_string(),
                description: "Read a file".to_string(),
                parameters: serde_json::json!({"type":"object"}),
            }]
        }
    }

    #[derive(Debug)]
    struct CacheAwareToolExecutor {
        clear_calls: Arc<AtomicUsize>,
        stats: crate::act::ToolCacheStats,
    }

    impl CacheAwareToolExecutor {
        fn new(clear_calls: Arc<AtomicUsize>, stats: crate::act::ToolCacheStats) -> Self {
            Self { clear_calls, stats }
        }
    }

    #[async_trait]
    impl ToolExecutor for CacheAwareToolExecutor {
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
                })
                .collect())
        }

        fn tool_definitions(&self) -> Vec<ToolDefinition> {
            vec![ToolDefinition {
                name: "read_file".to_string(),
                description: "Read a file".to_string(),
                parameters: serde_json::json!({"type":"object"}),
            }]
        }

        fn clear_cache(&self) {
            self.clear_calls.fetch_add(1, Ordering::Relaxed);
        }

        fn cache_stats(&self) -> Option<crate::act::ToolCacheStats> {
            Some(self.stats)
        }
    }

    #[derive(Debug)]
    struct SequentialMockLlm {
        responses: Mutex<VecDeque<CompletionResponse>>,
    }

    impl SequentialMockLlm {
        fn new(responses: Vec<CompletionResponse>) -> Self {
            Self {
                responses: Mutex::new(VecDeque::from(responses)),
            }
        }
    }

    #[async_trait]
    impl LlmProvider for SequentialMockLlm {
        async fn generate(&self, _: &str, _: u32) -> Result<String, CoreLlmError> {
            Ok("summary".to_string())
        }

        async fn generate_streaming(
            &self,
            _: &str,
            _: u32,
            callback: Box<dyn Fn(String) + Send + 'static>,
        ) -> Result<String, CoreLlmError> {
            callback("summary".to_string());
            Ok("summary".to_string())
        }

        fn model_name(&self) -> &str {
            "mock"
        }

        async fn complete(
            &self,
            _: CompletionRequest,
        ) -> Result<CompletionResponse, ProviderError> {
            self.responses
                .lock()
                .expect("lock")
                .pop_front()
                .ok_or_else(|| ProviderError::Provider("no response".to_string()))
        }
    }

    fn test_engine() -> LoopEngine {
        LoopEngine::new(
            BudgetTracker::new(crate::budget::BudgetConfig::default(), current_time_ms(), 0),
            ContextCompactor::new(2048, 256),
            3,
            Arc::new(StubToolExecutor),
            "Summarize tool output".to_string(),
        )
    }

    fn failing_tool_engine() -> LoopEngine {
        LoopEngine::new(
            BudgetTracker::new(crate::budget::BudgetConfig::default(), current_time_ms(), 0),
            ContextCompactor::new(2048, 256),
            3,
            Arc::new(FailingToolExecutor),
            "Summarize tool output".to_string(),
        )
    }

    fn test_snapshot(text: &str) -> PerceptionSnapshot {
        PerceptionSnapshot {
            timestamp_ms: 1,
            screen: ScreenState {
                current_app: "terminal".to_string(),
                elements: Vec::new(),
                text_content: text.to_string(),
            },
            notifications: Vec::new(),
            active_app: "terminal".to_string(),
            user_input: Some(UserInput {
                text: text.to_string(),
                source: InputSource::Text,
                timestamp: 1,
                context_id: None,
            }),
            sensor_data: None,
            conversation_history: vec![Message::user(text)],
        }
    }

    fn text_response(
        text: &str,
        stop_reason: Option<&str>,
        usage: Option<fx_llm::Usage>,
    ) -> CompletionResponse {
        CompletionResponse {
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            tool_calls: Vec::new(),
            usage,
            stop_reason: stop_reason.map(|value| value.to_string()),
        }
    }

    fn tool_call_response(
        id: &str,
        name: &str,
        arguments: serde_json::Value,
    ) -> CompletionResponse {
        CompletionResponse {
            content: Vec::new(),
            tool_calls: vec![ToolCall {
                id: id.to_string(),
                name: name.to_string(),
                arguments,
            }],
            usage: None,
            stop_reason: Some("tool_use".to_string()),
        }
    }

    fn expect_complete(result: LoopResult) -> (String, u32, Vec<Signal>) {
        match result {
            LoopResult::Complete {
                response,
                iterations,
                signals,
                ..
            } => (response, iterations, signals),
            other => panic!("expected LoopResult::Complete, got: {other:?}"),
        }
    }

    fn has_truncation_trace(signals: &[Signal], step: LoopStep) -> bool {
        signals.iter().any(|signal| {
            signal.step == step
                && signal.kind == SignalKind::Trace
                && signal.message.starts_with("response truncated, continuing")
        })
    }

    #[derive(Debug)]
    struct StreamingCaptureLlm {
        streamed_max_tokens: Mutex<Vec<u32>>,
        complete_calls: Mutex<u32>,
        output: String,
    }

    impl StreamingCaptureLlm {
        fn new(output: &str) -> Self {
            Self {
                streamed_max_tokens: Mutex::new(Vec::new()),
                complete_calls: Mutex::new(0),
                output: output.to_string(),
            }
        }

        fn streamed_max_tokens(&self) -> Vec<u32> {
            self.streamed_max_tokens.lock().expect("lock").clone()
        }

        fn complete_calls(&self) -> u32 {
            *self.complete_calls.lock().expect("lock")
        }
    }

    #[async_trait]
    impl LlmProvider for StreamingCaptureLlm {
        async fn generate(&self, _: &str, _: u32) -> Result<String, CoreLlmError> {
            Ok(self.output.clone())
        }

        async fn generate_streaming(
            &self,
            _: &str,
            max_tokens: u32,
            callback: Box<dyn Fn(String) + Send + 'static>,
        ) -> Result<String, CoreLlmError> {
            self.streamed_max_tokens
                .lock()
                .expect("lock")
                .push(max_tokens);
            callback(self.output.clone());
            Ok(self.output.clone())
        }

        fn model_name(&self) -> &str {
            "stream-capture"
        }

        async fn complete(
            &self,
            _: CompletionRequest,
        ) -> Result<CompletionResponse, ProviderError> {
            let mut calls = self.complete_calls.lock().expect("lock");
            *calls = calls.saturating_add(1);
            Err(ProviderError::Provider(
                "complete should not be called".to_string(),
            ))
        }
    }

    #[tokio::test]
    async fn verify_passes_when_tool_fails_but_synthesis_produced_response() {
        let mut engine = test_engine();
        let action = ActionResult {
            decision: Decision::UseTools(vec![]),
            tool_results: vec![ToolResult {
                tool_call_id: "call-1".to_string(),
                tool_name: "read_file".to_string(),
                output: "path escapes working directory".to_string(),
                success: false,
            }],
            response_text: "The file could not be read: path escapes working directory."
                .to_string(),
            tokens_used: TokenUsage {
                input_tokens: 100,
                output_tokens: 50,
            },
        };

        let verification = engine.verify(&action).await.expect("verify");
        assert!(
            verification.outcome_satisfactory,
            "tool error with synthesis should pass verification"
        );
        assert!(verification.discrepancies.is_empty());
    }

    #[tokio::test]
    async fn verify_fails_when_tool_fails_and_response_is_empty() {
        let mut engine = test_engine();
        let action = ActionResult {
            decision: Decision::UseTools(vec![]),
            tool_results: vec![ToolResult {
                tool_call_id: "call-1".to_string(),
                tool_name: "read_file".to_string(),
                output: "path escapes working directory".to_string(),
                success: false,
            }],
            response_text: "".to_string(),
            tokens_used: TokenUsage {
                input_tokens: 100,
                output_tokens: 0,
            },
        };

        let verification = engine.verify(&action).await.expect("verify");
        assert!(
            !verification.outcome_satisfactory,
            "tool error with empty response should fail"
        );
        assert!(!verification.discrepancies.is_empty());
        assert!(verification
            .discrepancies
            .iter()
            .any(|d| d.contains("tool calls failed and produced no response")));
    }

    #[tokio::test]
    async fn verify_fails_when_response_is_empty_without_tool_failure() {
        let mut engine = test_engine();
        let action = ActionResult {
            decision: Decision::UseTools(vec![]),
            tool_results: vec![ToolResult {
                tool_call_id: "call-1".to_string(),
                tool_name: "read_file".to_string(),
                output: "file contents here".to_string(),
                success: true,
            }],
            response_text: "   ".to_string(),
            tokens_used: TokenUsage {
                input_tokens: 100,
                output_tokens: 5,
            },
        };

        let verification = engine.verify(&action).await.expect("verify");
        assert!(
            !verification.outcome_satisfactory,
            "empty response should still fail"
        );
    }

    // NB2-3: decide extracts multiple tool calls
    #[tokio::test]
    async fn decide_extracts_multiple_tool_calls() {
        let mut engine = test_engine();
        let response = CompletionResponse {
            content: Vec::new(),
            tool_calls: vec![
                ToolCall {
                    id: "1".to_string(),
                    name: "read_file".to_string(),
                    arguments: serde_json::json!({"path":"a.txt"}),
                },
                ToolCall {
                    id: "2".to_string(),
                    name: "write_file".to_string(),
                    arguments: serde_json::json!({"path":"b.txt","content":"hi"}),
                },
                ToolCall {
                    id: "3".to_string(),
                    name: "run_command".to_string(),
                    arguments: serde_json::json!({"cmd":"ls"}),
                },
            ],
            usage: None,
            stop_reason: None,
        };

        let decision = engine.decide(&response).await.expect("decision");

        match decision {
            Decision::UseTools(calls) => {
                assert_eq!(calls.len(), 3, "all 3 tool calls should be preserved");
                assert_eq!(calls[0].name, "read_file");
                assert_eq!(calls[1].name, "write_file");
                assert_eq!(calls[2].name, "run_command");
            }
            other => panic!("expected Decision::UseTools, got: {other:?}"),
        }
    }

    // NB2-4: run_cycle completes with a direct tool call
    #[tokio::test]
    async fn run_cycle_completes_with_direct_tool_call() {
        let mut engine = test_engine();

        // First response: LLM returns a tool call
        // Second response: LLM synthesizes the tool results into a final answer
        let llm = SequentialMockLlm::new(vec![
            CompletionResponse {
                content: Vec::new(),
                tool_calls: vec![ToolCall {
                    id: "call-1".to_string(),
                    name: "read_file".to_string(),
                    arguments: serde_json::json!({"path":"README.md"}),
                }],
                usage: None,
                stop_reason: Some("tool_use".to_string()),
            },
            CompletionResponse {
                content: vec![ContentBlock::Text {
                    text: "README loaded".to_string(),
                }],
                tool_calls: Vec::new(),
                usage: None,
                stop_reason: None,
            },
        ]);

        let result = engine
            .run_cycle(test_snapshot("read the readme"), &llm)
            .await
            .expect("run_cycle");

        assert!(
            matches!(result, LoopResult::Complete { .. }),
            "expected LoopResult::Complete, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn run_cycle_completes_in_one_iteration_when_tool_fails_but_synthesis_exists() {
        let mut engine = failing_tool_engine();

        let llm = SequentialMockLlm::new(vec![
            CompletionResponse {
                content: Vec::new(),
                tool_calls: vec![ToolCall {
                    id: "call-1".to_string(),
                    name: "read_file".to_string(),
                    arguments: serde_json::json!({"path":"README.md"}),
                }],
                usage: None,
                stop_reason: Some("tool_use".to_string()),
            },
            CompletionResponse {
                content: vec![ContentBlock::Text {
                    text: "The file could not be read: path escapes working directory.".to_string(),
                }],
                tool_calls: Vec::new(),
                usage: None,
                stop_reason: None,
            },
        ]);

        let result = engine
            .run_cycle(test_snapshot("read the readme"), &llm)
            .await
            .expect("run_cycle");

        match result {
            LoopResult::Complete {
                response,
                iterations,
                ..
            } => {
                assert_eq!(iterations, 1, "expected exactly one iteration");
                assert_eq!(
                    response,
                    "The file could not be read: path escapes working directory."
                );
            }
            other => panic!("expected LoopResult::Complete, got: {other:?}"),
        }
    }

    // NB2-5: run_cycle returns budget exhausted when budget is 0
    #[tokio::test]
    async fn run_cycle_returns_budget_exhausted() {
        let zero_budget = crate::budget::BudgetConfig {
            max_llm_calls: 0,
            max_tool_invocations: 0,
            max_tokens: 0,
            max_cost_cents: 0,
            max_wall_time_ms: 0,
            max_recursion_depth: 0,
        };
        let mut engine = LoopEngine::new(
            BudgetTracker::new(zero_budget, 0, 0),
            ContextCompactor::new(2048, 256),
            3,
            Arc::new(StubToolExecutor),
            "Summarize tool output".to_string(),
        );

        let llm = SequentialMockLlm::new(vec![CompletionResponse {
            content: vec![ContentBlock::Text {
                text: "hello".to_string(),
            }],
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: None,
        }]);

        let result = engine
            .run_cycle(test_snapshot("hello"), &llm)
            .await
            .expect("run_cycle");

        assert!(
            matches!(result, LoopResult::BudgetExhausted { .. }),
            "expected LoopResult::BudgetExhausted, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn budget_exhaustion_emits_blocked_signal() {
        let zero_budget = crate::budget::BudgetConfig {
            max_llm_calls: 0,
            max_tool_invocations: 0,
            max_tokens: 0,
            max_cost_cents: 0,
            max_wall_time_ms: 0,
            max_recursion_depth: 0,
        };
        let mut engine = LoopEngine::new(
            BudgetTracker::new(zero_budget, 0, 0),
            ContextCompactor::new(2048, 256),
            3,
            Arc::new(StubToolExecutor),
            "Summarize tool output".to_string(),
        );

        let llm = SequentialMockLlm::new(vec![CompletionResponse {
            content: vec![ContentBlock::Text {
                text: "hello".to_string(),
            }],
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: None,
        }]);

        let result = engine
            .run_cycle(test_snapshot("hello"), &llm)
            .await
            .expect("run_cycle");

        let signals = match result {
            LoopResult::Complete { signals, .. }
            | LoopResult::BudgetExhausted { signals, .. }
            | LoopResult::NeedsInput { signals, .. }
            | LoopResult::UserStopped { signals, .. }
            | LoopResult::Error { signals, .. } => signals,
        };

        assert!(signals
            .iter()
            .any(|s| s.step == LoopStep::Continue && s.kind == SignalKind::Blocked));
    }

    #[tokio::test]
    async fn run_cycle_emits_signals() {
        let mut engine = test_engine();
        let llm = SequentialMockLlm::new(vec![CompletionResponse {
            content: vec![ContentBlock::Text {
                text: "hello".to_string(),
            }],
            tool_calls: Vec::new(),
            usage: Some(fx_llm::Usage {
                input_tokens: 8,
                output_tokens: 4,
            }),
            stop_reason: None,
        }]);

        let result = engine
            .run_cycle(test_snapshot("hello"), &llm)
            .await
            .expect("run_cycle");

        let signals = match result {
            LoopResult::Complete { signals, .. }
            | LoopResult::BudgetExhausted { signals, .. }
            | LoopResult::NeedsInput { signals, .. }
            | LoopResult::UserStopped { signals, .. }
            | LoopResult::Error { signals, .. } => signals,
        };

        // Verify expected signal types for a text-response cycle.
        assert!(signals
            .iter()
            .any(|s| s.step == LoopStep::Perceive && s.kind == SignalKind::Trace));
        assert!(signals
            .iter()
            .any(|s| s.step == LoopStep::Reason && s.kind == SignalKind::Trace));
        assert!(signals
            .iter()
            .any(|s| s.step == LoopStep::Reason && s.kind == SignalKind::Performance));
        assert!(signals
            .iter()
            .any(|s| s.step == LoopStep::Decide && s.kind == SignalKind::Decision));
        assert!(signals
            .iter()
            .any(|s| s.step == LoopStep::Verify && s.kind == SignalKind::Decision));
        assert!(signals
            .iter()
            .any(|s| s.step == LoopStep::Continue && s.kind == SignalKind::Decision));
    }

    #[tokio::test]
    async fn run_cycle_clears_tool_cache_at_cycle_boundary() {
        let clear_calls = Arc::new(AtomicUsize::new(0));
        let stats = crate::act::ToolCacheStats::default();
        let executor = CacheAwareToolExecutor::new(Arc::clone(&clear_calls), stats);
        let mut engine = LoopEngine::new(
            BudgetTracker::new(crate::budget::BudgetConfig::default(), 0, 0),
            ContextCompactor::new(2048, 256),
            3,
            Arc::new(executor),
            "Summarize tool output".to_string(),
        );

        let llm = SequentialMockLlm::new(vec![
            CompletionResponse {
                content: vec![ContentBlock::Text {
                    text: "one".to_string(),
                }],
                tool_calls: Vec::new(),
                usage: None,
                stop_reason: None,
            },
            CompletionResponse {
                content: vec![ContentBlock::Text {
                    text: "two".to_string(),
                }],
                tool_calls: Vec::new(),
                usage: None,
                stop_reason: None,
            },
        ]);

        engine
            .run_cycle(test_snapshot("hello"), &llm)
            .await
            .expect("first cycle");
        engine
            .run_cycle(test_snapshot("hello"), &llm)
            .await
            .expect("second cycle");

        assert_eq!(clear_calls.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn run_cycle_emits_tool_cache_stats_signal() {
        let clear_calls = Arc::new(AtomicUsize::new(0));
        let stats = crate::act::ToolCacheStats {
            hits: 2,
            misses: 1,
            entries: 4,
            evictions: 1,
        };
        let executor = CacheAwareToolExecutor::new(Arc::clone(&clear_calls), stats);
        let mut engine = LoopEngine::new(
            BudgetTracker::new(crate::budget::BudgetConfig::default(), 0, 0),
            ContextCompactor::new(2048, 256),
            3,
            Arc::new(executor),
            "Summarize tool output".to_string(),
        );

        let llm = SequentialMockLlm::new(vec![CompletionResponse {
            content: vec![ContentBlock::Text {
                text: "done".to_string(),
            }],
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: None,
        }]);

        let result = engine
            .run_cycle(test_snapshot("hello"), &llm)
            .await
            .expect("run cycle");
        let signals = match result {
            LoopResult::Complete { signals, .. }
            | LoopResult::BudgetExhausted { signals, .. }
            | LoopResult::NeedsInput { signals, .. }
            | LoopResult::UserStopped { signals, .. }
            | LoopResult::Error { signals, .. } => signals,
        };

        let cache_signal = signals
            .iter()
            .find(|signal| {
                signal.step == LoopStep::Act
                    && signal.kind == SignalKind::Performance
                    && signal.message == "tool cache stats"
            })
            .expect("cache stats signal");

        assert_eq!(cache_signal.metadata["hits"], serde_json::json!(2));
        assert_eq!(cache_signal.metadata["misses"], serde_json::json!(1));
        assert_eq!(cache_signal.metadata["entries"], serde_json::json!(4));
        assert_eq!(cache_signal.metadata["evictions"], serde_json::json!(1));
        assert_eq!(
            cache_signal.metadata["hit_rate"],
            serde_json::json!(2.0 / 3.0)
        );
        assert_eq!(clear_calls.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn signals_include_decision_on_tool_call() {
        let mut engine = test_engine();
        let llm = SequentialMockLlm::new(vec![
            CompletionResponse {
                content: Vec::new(),
                tool_calls: vec![ToolCall {
                    id: "call-1".to_string(),
                    name: "read_file".to_string(),
                    arguments: serde_json::json!({"path":"README.md"}),
                }],
                usage: Some(fx_llm::Usage {
                    input_tokens: 10,
                    output_tokens: 2,
                }),
                stop_reason: Some("tool_use".to_string()),
            },
            CompletionResponse {
                content: vec![ContentBlock::Text {
                    text: "done".to_string(),
                }],
                tool_calls: Vec::new(),
                usage: None,
                stop_reason: None,
            },
        ]);

        let result = engine
            .run_cycle(test_snapshot("read the readme"), &llm)
            .await
            .expect("run_cycle");

        let signals = match result {
            LoopResult::Complete { signals, .. }
            | LoopResult::BudgetExhausted { signals, .. }
            | LoopResult::NeedsInput { signals, .. }
            | LoopResult::UserStopped { signals, .. }
            | LoopResult::Error { signals, .. } => signals,
        };

        assert!(signals.iter().any(|signal| {
            signal.step == LoopStep::Decide && signal.kind == SignalKind::Decision
        }));
    }

    #[tokio::test]
    async fn tool_continuation_rounds_emit_trace_and_performance_signals() {
        let mut engine = test_engine();
        let llm = SequentialMockLlm::new(vec![
            CompletionResponse {
                content: Vec::new(),
                tool_calls: vec![ToolCall {
                    id: "call-1".to_string(),
                    name: "read_file".to_string(),
                    arguments: serde_json::json!({"path":"README.md"}),
                }],
                usage: Some(fx_llm::Usage {
                    input_tokens: 10,
                    output_tokens: 2,
                }),
                stop_reason: Some("tool_use".to_string()),
            },
            CompletionResponse {
                content: Vec::new(),
                tool_calls: vec![ToolCall {
                    id: "call-2".to_string(),
                    name: "read_file".to_string(),
                    arguments: serde_json::json!({"path":"Cargo.toml"}),
                }],
                usage: Some(fx_llm::Usage {
                    input_tokens: 6,
                    output_tokens: 3,
                }),
                stop_reason: Some("tool_use".to_string()),
            },
            CompletionResponse {
                content: vec![ContentBlock::Text {
                    text: "done".to_string(),
                }],
                tool_calls: Vec::new(),
                usage: Some(fx_llm::Usage {
                    input_tokens: 5,
                    output_tokens: 4,
                }),
                stop_reason: None,
            },
        ]);

        let result = engine
            .run_cycle(test_snapshot("read files"), &llm)
            .await
            .expect("run_cycle");

        let signals = match result {
            LoopResult::Complete { signals, .. }
            | LoopResult::BudgetExhausted { signals, .. }
            | LoopResult::NeedsInput { signals, .. }
            | LoopResult::UserStopped { signals, .. }
            | LoopResult::Error { signals, .. } => signals,
        };

        let round_trace_count = signals
            .iter()
            .filter(|signal| {
                signal.step == LoopStep::Act
                    && signal.kind == SignalKind::Trace
                    && signal.message == "tool continuation round"
            })
            .count();
        let round_perf_count = signals
            .iter()
            .filter(|signal| {
                signal.step == LoopStep::Act
                    && signal.kind == SignalKind::Performance
                    && signal.message == "tool continuation latency"
            })
            .count();
        assert_eq!(round_trace_count, 2, "expected 2 round trace signals");
        assert_eq!(round_perf_count, 2, "expected 2 round performance signals");
    }

    #[tokio::test]
    async fn empty_tool_continuation_emits_safe_fallback_trace() {
        let mut engine = test_engine();
        let llm = SequentialMockLlm::new(vec![
            CompletionResponse {
                content: Vec::new(),
                tool_calls: vec![ToolCall {
                    id: "call-1".to_string(),
                    name: "read_file".to_string(),
                    arguments: serde_json::json!({"path":"README.md"}),
                }],
                usage: None,
                stop_reason: Some("tool_use".to_string()),
            },
            CompletionResponse {
                content: Vec::new(),
                tool_calls: Vec::new(),
                usage: None,
                stop_reason: None,
            },
        ]);

        let result = engine
            .run_cycle(test_snapshot("read the readme"), &llm)
            .await
            .expect("run_cycle");

        let (response, signals) = match result {
            LoopResult::Complete {
                response, signals, ..
            } => (response, signals),
            other => panic!("expected LoopResult::Complete, got: {other:?}"),
        };

        assert_eq!(response, SAFE_FALLBACK_RESPONSE);
        assert!(signals.iter().any(|signal| {
            signal.step == LoopStep::Act
                && signal.kind == SignalKind::Trace
                && signal.message == "tool continuation returned empty text; using safe fallback"
        }));
    }

    #[test]
    fn is_truncated_detects_anthropic_stop_reason() {
        assert!(is_truncated(Some("max_tokens")));
        assert!(is_truncated(Some("MAX_TOKENS")));
    }

    #[test]
    fn is_truncated_detects_openai_finish_reason() {
        assert!(is_truncated(Some("length")));
        assert!(is_truncated(Some("LENGTH")));
    }

    #[test]
    fn is_truncated_handles_none_and_unknown() {
        assert!(!is_truncated(None));
        assert!(!is_truncated(Some("stop")));
        assert!(!is_truncated(Some("tool_use")));
    }

    #[test]
    fn merge_usage_combines_token_counts() {
        let merged = merge_usage(
            Some(fx_llm::Usage {
                input_tokens: 100,
                output_tokens: 25,
            }),
            Some(fx_llm::Usage {
                input_tokens: 30,
                output_tokens: 10,
            }),
        )
        .expect("usage should merge");
        assert_eq!(merged.input_tokens, 130);
        assert_eq!(merged.output_tokens, 35);

        let right_only = merge_usage(
            None,
            Some(fx_llm::Usage {
                input_tokens: 7,
                output_tokens: 3,
            }),
        )
        .expect("right usage should be preserved");
        assert_eq!(right_only.input_tokens, 7);
        assert_eq!(right_only.output_tokens, 3);

        assert!(merge_usage(None, None).is_none());
    }

    #[test]
    fn merge_continuation_response_preserves_tool_calls_when_continuation_has_none() {
        let previous = CompletionResponse {
            content: vec![ContentBlock::Text {
                text: "preface".to_string(),
            }],
            tool_calls: vec![ToolCall {
                id: "call-1".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path":"README.md"}),
            }],
            usage: None,
            stop_reason: Some("max_tokens".to_string()),
        };
        let continued = text_response(" continuation", Some("stop"), None);
        let mut full_text = "preface".to_string();

        let merged = merge_continuation_response(previous, continued, &mut full_text);

        assert_eq!(merged.tool_calls.len(), 1);
        assert_eq!(merged.tool_calls[0].id, "call-1");
    }

    #[test]
    fn build_truncation_continuation_request_enables_tools_only_for_reason_step() {
        let tool_definitions = vec![ToolDefinition {
            name: "read_file".to_string(),
            description: "Read a file".to_string(),
            parameters: serde_json::json!({"type":"object"}),
        }];
        let messages = vec![Message::user("continue")];

        let reason_request = build_truncation_continuation_request(
            "mock",
            &messages,
            tool_definitions.clone(),
            None,
            LoopStep::Reason,
        );
        let act_request = build_truncation_continuation_request(
            "mock",
            &messages,
            tool_definitions,
            None,
            LoopStep::Act,
        );

        assert!(reason_request
            .tools
            .iter()
            .any(|tool| tool.name == "read_file"));
        assert!(act_request.tools.is_empty());
    }

    #[tokio::test]
    async fn continue_truncated_response_stitches_text() {
        let mut engine = test_engine();
        let initial = text_response(
            "Hello",
            Some("max_tokens"),
            Some(fx_llm::Usage {
                input_tokens: 10,
                output_tokens: 4,
            }),
        );
        let llm = SequentialMockLlm::new(vec![text_response(
            " world",
            Some("stop"),
            Some(fx_llm::Usage {
                input_tokens: 3,
                output_tokens: 2,
            }),
        )]);

        let stitched = engine
            .continue_truncated_response(initial, &[Message::user("hello")], &llm, LoopStep::Reason)
            .await
            .expect("continuation should succeed");

        assert_eq!(extract_response_text(&stitched), "Hello world");
        assert_eq!(stitched.stop_reason.as_deref(), Some("stop"));
        let usage = stitched.usage.expect("usage should be merged");
        assert_eq!(usage.input_tokens, 13);
        assert_eq!(usage.output_tokens, 6);
    }

    #[tokio::test]
    async fn continue_truncated_response_respects_max_attempts() {
        let mut engine = test_engine();
        let initial = text_response("A", Some("max_tokens"), None);
        let llm = SequentialMockLlm::new(vec![
            text_response("B", Some("max_tokens"), None),
            text_response("C", Some("max_tokens"), None),
            text_response("D", Some("max_tokens"), None),
        ]);

        let stitched = engine
            .continue_truncated_response(
                initial,
                &[Message::user("continue")],
                &llm,
                LoopStep::Reason,
            )
            .await
            .expect("continuation should stop at max attempts");

        assert_eq!(extract_response_text(&stitched), "ABCD");
        assert_eq!(stitched.stop_reason.as_deref(), Some("max_tokens"));
    }

    #[tokio::test]
    async fn continue_truncated_response_stops_on_natural_end() {
        let mut engine = test_engine();
        let initial = text_response("A", Some("max_tokens"), None);
        let llm = SequentialMockLlm::new(vec![
            text_response("B", Some("stop"), None),
            text_response("C", Some("max_tokens"), None),
        ]);

        let stitched = engine
            .continue_truncated_response(
                initial,
                &[Message::user("continue")],
                &llm,
                LoopStep::Reason,
            )
            .await
            .expect("continuation should stop when natural stop reason arrives");

        assert_eq!(extract_response_text(&stitched), "AB");
        assert_eq!(stitched.stop_reason.as_deref(), Some("stop"));
    }

    #[tokio::test]
    async fn run_cycle_auto_continues_truncated_response() {
        let mut engine = test_engine();
        let llm = SequentialMockLlm::new(vec![
            text_response("First half", Some("max_tokens"), None),
            text_response(" second half", Some("stop"), None),
        ]);

        let result = engine
            .run_cycle(test_snapshot("finish your sentence"), &llm)
            .await
            .expect("run_cycle should succeed");
        let (response, iterations, _) = expect_complete(result);

        assert_eq!(iterations, 1);
        assert_eq!(response, "First half second half");
    }

    #[tokio::test]
    async fn tool_continuation_auto_continues_truncated_response() {
        let mut engine = test_engine();
        let llm = SequentialMockLlm::new(vec![
            tool_call_response(
                "call-1",
                "read_file",
                serde_json::json!({"path":"README.md"}),
            ),
            text_response("Tool answer part", Some("length"), None),
            text_response(" two", Some("stop"), None),
        ]);

        let result = engine
            .run_cycle(test_snapshot("read the file"), &llm)
            .await
            .expect("run_cycle should succeed");
        let (response, iterations, _) = expect_complete(result);

        assert_eq!(iterations, 1);
        assert_eq!(response, "Tool answer part two");
    }

    #[tokio::test]
    async fn reason_truncation_continuation_preserves_initial_tool_calls() {
        let mut engine = test_engine();
        let llm = SequentialMockLlm::new(vec![
            CompletionResponse {
                content: vec![ContentBlock::Text {
                    text: "I will read the file".to_string(),
                }],
                tool_calls: vec![ToolCall {
                    id: "call-1".to_string(),
                    name: "read_file".to_string(),
                    arguments: serde_json::json!({"path":"README.md"}),
                }],
                usage: None,
                stop_reason: Some("max_tokens".to_string()),
            },
            text_response(" and summarize it", Some("stop"), None),
            text_response("tool executed", Some("stop"), None),
        ]);

        let result = engine
            .run_cycle(test_snapshot("read the file"), &llm)
            .await
            .expect("run_cycle should succeed");
        let (response, _, signals) = expect_complete(result);

        assert_eq!(response, "tool executed");
        assert!(has_truncation_trace(&signals, LoopStep::Reason));
        assert!(signals.iter().any(|signal| {
            signal.step == LoopStep::Act
                && signal.kind == SignalKind::Success
                && signal.message == "tool read_file"
        }));
    }

    #[tokio::test]
    async fn finalize_tool_response_receives_stitched_text_after_continuation() {
        let mut engine = test_engine();
        let overlap = "x".repeat(90);
        let first = format!("Start {overlap}");
        let second = format!("{overlap} End");
        let expected = format!("Start {overlap} End");
        let llm = SequentialMockLlm::new(vec![
            tool_call_response(
                "call-1",
                "read_file",
                serde_json::json!({"path":"README.md"}),
            ),
            text_response(&first, Some("max_tokens"), None),
            text_response(&second, Some("stop"), None),
        ]);

        let result = engine
            .run_cycle(test_snapshot("summarize tool output"), &llm)
            .await
            .expect("run_cycle should succeed");
        let (response, _, _) = expect_complete(result);

        assert_eq!(response, expected);
    }

    #[tokio::test]
    async fn truncation_continuation_emits_reason_and_act_trace_signals() {
        let mut reason_engine = test_engine();
        let reason_llm = SequentialMockLlm::new(vec![
            text_response("Reason part", Some("max_tokens"), None),
            text_response(" complete", Some("stop"), None),
        ]);

        let reason_result = reason_engine
            .run_cycle(test_snapshot("reason continuation"), &reason_llm)
            .await
            .expect("reason run should succeed");
        let (_, _, reason_signals) = expect_complete(reason_result);
        assert!(has_truncation_trace(&reason_signals, LoopStep::Reason));

        let mut act_engine = test_engine();
        let act_llm = SequentialMockLlm::new(vec![
            tool_call_response(
                "call-1",
                "read_file",
                serde_json::json!({"path":"README.md"}),
            ),
            text_response("Act part", Some("length"), None),
            text_response(" complete", Some("stop"), None),
        ]);

        let act_result = act_engine
            .run_cycle(test_snapshot("act continuation"), &act_llm)
            .await
            .expect("act run should succeed");
        let (_, _, act_signals) = expect_complete(act_result);
        assert!(has_truncation_trace(&act_signals, LoopStep::Act));
    }

    #[tokio::test]
    async fn continuation_calls_record_budget() {
        let mut baseline_engine = test_engine();
        let baseline_llm = SequentialMockLlm::new(vec![text_response("done", Some("stop"), None)]);
        baseline_engine
            .run_cycle(test_snapshot("baseline"), &baseline_llm)
            .await
            .expect("baseline run should succeed");
        let baseline_calls = baseline_engine.status(current_time_ms()).llm_calls_used;

        let mut continuation_engine = test_engine();
        let continuation_llm = SequentialMockLlm::new(vec![
            text_response("first", Some("max_tokens"), None),
            text_response(" second", Some("stop"), None),
        ]);
        continuation_engine
            .run_cycle(test_snapshot("needs continuation"), &continuation_llm)
            .await
            .expect("continuation run should succeed");
        let continuation_calls = continuation_engine.status(current_time_ms()).llm_calls_used;

        assert_eq!(continuation_calls, baseline_calls.saturating_add(1));
    }

    #[test]
    fn raised_max_tokens_constants_are_applied() {
        assert_eq!(REASONING_MAX_OUTPUT_TOKENS, 4096);
        assert_eq!(TOOL_SYNTHESIS_MAX_OUTPUT_TOKENS, 1024);

        let perception = ProcessedPerception {
            user_message: "hello".to_string(),
            context_window: vec![Message::user("hello")],
            active_goals: vec!["reply".to_string()],
            budget_remaining: BudgetRemaining {
                llm_calls: 8,
                tool_invocations: 16,
                tokens: 10_000,
                cost_cents: 100,
                wall_time_ms: 1_000,
            },
        };

        let reasoning_request = build_reasoning_request(&perception, "mock", vec![], None);
        let continuation_request =
            build_continuation_request(&perception.context_window, "mock", vec![], None);

        assert_eq!(reasoning_request.max_tokens, Some(4096));
        assert_eq!(continuation_request.max_tokens, Some(4096));
    }

    #[tokio::test]
    async fn tool_synthesis_uses_raised_token_cap_without_stop_reason_assumptions() {
        let engine = test_engine();
        let llm = StreamingCaptureLlm::new("summary from stream");

        let summary = engine
            .generate_tool_summary("summarize this", &llm)
            .await
            .expect("streaming synthesis should succeed");

        assert_eq!(summary, "summary from stream");
        assert_eq!(
            llm.streamed_max_tokens(),
            vec![TOOL_SYNTHESIS_MAX_OUTPUT_TOKENS]
        );
        assert_eq!(llm.complete_calls(), 0);
    }

    // B2: extract_readable_text unit tests
    #[test]
    fn extract_readable_text_passes_plain_text_through() {
        assert_eq!(extract_readable_text("Hello world"), "Hello world");
    }

    #[test]
    fn extract_readable_text_extracts_text_field() {
        let json = r##"{"text": "Hello from JSON"}"##;
        assert_eq!(extract_readable_text(json), "Hello from JSON");
    }

    #[test]
    fn extract_readable_text_extracts_response_field() {
        let json = r#"{"response": "Extracted response"}"#;
        assert_eq!(extract_readable_text(json), "Extracted response");
    }

    #[test]
    fn extract_readable_text_returns_raw_for_unrecognized_json() {
        let json = r#"{"weird_key": "some value"}"#;
        assert_eq!(extract_readable_text(json), json);
    }

    #[test]
    fn extract_readable_text_handles_invalid_json() {
        let broken = r#"{not valid json"#;
        assert_eq!(extract_readable_text(broken), broken);
    }
}

#[cfg(test)]
mod phase4_tests {
    use super::*;
    use crate::cancellation::CancellationToken;
    use crate::input::{loop_input_channel, LoopCommand};
    use async_trait::async_trait;
    use fx_core::error::LlmError as CoreLlmError;
    use fx_core::types::{InputSource, ScreenState, UserInput};
    use fx_llm::{
        CompletionResponse, ContentBlock, Message, ProviderError, ToolCall, ToolDefinition,
    };
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};

    /// Tool executor that tracks how many calls were actually executed
    /// and supports cooperative cancellation.
    #[derive(Debug)]
    struct CountingToolExecutor {
        executed_count: Arc<AtomicU32>,
    }

    #[async_trait]
    impl ToolExecutor for CountingToolExecutor {
        async fn execute_tools(
            &self,
            calls: &[ToolCall],
            cancel: Option<&CancellationToken>,
        ) -> Result<Vec<ToolResult>, crate::act::ToolExecutorError> {
            let mut results = Vec::new();
            for call in calls {
                if let Some(token) = cancel {
                    if token.is_cancelled() {
                        break;
                    }
                }
                self.executed_count.fetch_add(1, Ordering::SeqCst);
                results.push(ToolResult {
                    tool_call_id: call.id.clone(),
                    tool_name: call.name.clone(),
                    success: true,
                    output: "ok".to_string(),
                });
                // Cancel after first tool call to test partial execution
                if let Some(token) = cancel {
                    token.cancel();
                }
            }
            Ok(results)
        }

        fn tool_definitions(&self) -> Vec<ToolDefinition> {
            vec![ToolDefinition {
                name: "read_file".to_string(),
                description: "Read a file".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {"path": {"type": "string"}},
                    "required": ["path"]
                }),
            }]
        }
    }

    #[derive(Debug, Default)]
    struct Phase4StubToolExecutor;

    #[async_trait]
    impl ToolExecutor for Phase4StubToolExecutor {
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
                })
                .collect())
        }

        fn tool_definitions(&self) -> Vec<ToolDefinition> {
            vec![ToolDefinition {
                name: "read_file".to_string(),
                description: "Read a file".to_string(),
                parameters: serde_json::json!({"type":"object"}),
            }]
        }
    }

    #[derive(Debug)]
    struct Phase4MockLlm {
        responses: Mutex<VecDeque<CompletionResponse>>,
    }

    impl Phase4MockLlm {
        fn new(responses: Vec<CompletionResponse>) -> Self {
            Self {
                responses: Mutex::new(VecDeque::from(responses)),
            }
        }
    }

    /// Mock LLM that cancels a token during `complete()` to simulate
    /// mid-cycle cancellation (e.g. user pressing Ctrl+C while the LLM
    /// is generating a response).
    #[derive(Debug)]
    struct CancellingMockLlm {
        token: CancellationToken,
        responses: Mutex<VecDeque<CompletionResponse>>,
    }

    impl CancellingMockLlm {
        fn new(token: CancellationToken, responses: Vec<CompletionResponse>) -> Self {
            Self {
                token,
                responses: Mutex::new(VecDeque::from(responses)),
            }
        }
    }

    #[async_trait]
    impl LlmProvider for CancellingMockLlm {
        async fn generate(&self, _: &str, _: u32) -> Result<String, CoreLlmError> {
            Ok("summary".to_string())
        }

        async fn generate_streaming(
            &self,
            _: &str,
            _: u32,
            callback: Box<dyn Fn(String) + Send + 'static>,
        ) -> Result<String, CoreLlmError> {
            callback("summary".to_string());
            Ok("summary".to_string())
        }

        fn model_name(&self) -> &str {
            "mock-cancelling"
        }

        async fn complete(
            &self,
            _: CompletionRequest,
        ) -> Result<CompletionResponse, ProviderError> {
            // Cancel the token mid-cycle (simulates Ctrl+C during LLM call)
            self.token.cancel();
            self.responses
                .lock()
                .expect("lock")
                .pop_front()
                .ok_or_else(|| ProviderError::Provider("no response".to_string()))
        }
    }

    #[async_trait]
    impl LlmProvider for Phase4MockLlm {
        async fn generate(&self, _: &str, _: u32) -> Result<String, CoreLlmError> {
            Ok("summary".to_string())
        }

        async fn generate_streaming(
            &self,
            _: &str,
            _: u32,
            callback: Box<dyn Fn(String) + Send + 'static>,
        ) -> Result<String, CoreLlmError> {
            callback("summary".to_string());
            Ok("summary".to_string())
        }

        fn model_name(&self) -> &str {
            "mock"
        }

        async fn complete(
            &self,
            _: CompletionRequest,
        ) -> Result<CompletionResponse, ProviderError> {
            self.responses
                .lock()
                .expect("lock")
                .pop_front()
                .ok_or_else(|| ProviderError::Provider("no response".to_string()))
        }
    }

    fn p4_engine() -> LoopEngine {
        LoopEngine::new(
            BudgetTracker::new(crate::budget::BudgetConfig::default(), 0, 0),
            ContextCompactor::new(2048, 256),
            3,
            Arc::new(Phase4StubToolExecutor),
            "Summarize tool output".to_string(),
        )
    }

    fn p4_snapshot(text: &str) -> PerceptionSnapshot {
        PerceptionSnapshot {
            timestamp_ms: 1,
            screen: ScreenState {
                current_app: "terminal".to_string(),
                elements: Vec::new(),
                text_content: text.to_string(),
            },
            notifications: Vec::new(),
            active_app: "terminal".to_string(),
            user_input: Some(UserInput {
                text: text.to_string(),
                source: InputSource::Text,
                timestamp: 1,
                context_id: None,
            }),
            sensor_data: None,
            conversation_history: vec![Message::user(text)],
        }
    }

    fn read_file_call(id: &str, path: &str) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": path}),
        }
    }

    fn calls_from_decision(decision: &Decision) -> &[ToolCall] {
        match decision {
            Decision::UseTools(calls) => calls.as_slice(),
            _ => panic!("decision should contain tool calls"),
        }
    }

    fn tool_use_response(calls: Vec<ToolCall>) -> CompletionResponse {
        CompletionResponse {
            content: Vec::new(),
            tool_calls: calls,
            usage: None,
            stop_reason: Some("tool_use".to_string()),
        }
    }

    fn text_response(text: &str) -> CompletionResponse {
        CompletionResponse {
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: None,
        }
    }

    fn assert_tool_result_block(block: &ContentBlock, expected_id: &str, expected_content: &str) {
        match block {
            ContentBlock::ToolResult {
                tool_use_id,
                content,
            } => {
                assert_eq!(tool_use_id, expected_id);
                assert_eq!(content.as_str(), Some(expected_content));
            }
            other => panic!("expected ToolResult block, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn act_with_tools_executes_all_calls_and_returns_completion_text() {
        let mut engine = p4_engine();
        let decision = Decision::UseTools(vec![
            read_file_call("1", "a.txt"),
            read_file_call("2", "b.txt"),
        ]);
        let llm = Phase4MockLlm::new(vec![text_response("combined tool output")]);
        let context_messages = vec![Message::user("read two files")];

        let action = engine
            .act_with_tools(
                &decision,
                calls_from_decision(&decision),
                &llm,
                &context_messages,
            )
            .await
            .expect("act_with_tools");

        assert_eq!(action.tool_results.len(), 2);
        assert_eq!(action.tool_results[0].tool_name, "read_file");
        assert_eq!(action.tool_results[1].tool_name, "read_file");
        assert_eq!(action.response_text, "combined tool output");
    }

    #[tokio::test]
    async fn act_with_tools_reprompts_on_follow_up_tool_calls() {
        let mut engine = p4_engine();
        let decision = Decision::UseTools(vec![read_file_call("call-1", "a.txt")]);
        let llm = Phase4MockLlm::new(vec![
            tool_use_response(vec![read_file_call("call-2", "b.txt")]),
            text_response("done after two rounds"),
        ]);
        let context_messages = vec![Message::user("read files")];

        let action = engine
            .act_with_tools(
                &decision,
                calls_from_decision(&decision),
                &llm,
                &context_messages,
            )
            .await
            .expect("act_with_tools");

        assert_eq!(action.tool_results.len(), 2);
        assert_eq!(action.tool_results[0].tool_call_id, "call-1");
        assert_eq!(action.tool_results[1].tool_call_id, "call-2");
        assert_eq!(action.response_text, "done after two rounds");
    }

    #[tokio::test]
    async fn act_with_tools_chains_three_tool_rounds() {
        let mut engine = p4_engine();
        let decision = Decision::UseTools(vec![read_file_call("call-1", "a.txt")]);
        let llm = Phase4MockLlm::new(vec![
            tool_use_response(vec![read_file_call("call-2", "b.txt")]),
            tool_use_response(vec![read_file_call("call-3", "c.txt")]),
            text_response("done after three rounds"),
        ]);
        let context_messages = vec![Message::user("read files")];

        let action = engine
            .act_with_tools(
                &decision,
                calls_from_decision(&decision),
                &llm,
                &context_messages,
            )
            .await
            .expect("act_with_tools");

        assert_eq!(action.tool_results.len(), 3);
        assert_eq!(action.tool_results[0].tool_call_id, "call-1");
        assert_eq!(action.tool_results[1].tool_call_id, "call-2");
        assert_eq!(action.tool_results[2].tool_call_id, "call-3");
        assert_eq!(action.response_text, "done after three rounds");
    }

    #[tokio::test]
    async fn act_with_tools_falls_back_to_synthesis_on_max_iterations() {
        let mut engine = LoopEngine::new(
            BudgetTracker::new(crate::budget::BudgetConfig::default(), 0, 0),
            ContextCompactor::new(2048, 256),
            1,
            Arc::new(Phase4StubToolExecutor),
            "Summarize tool output".to_string(),
        );
        let decision = Decision::UseTools(vec![read_file_call("call-1", "a.txt")]);
        let llm = Phase4MockLlm::new(vec![tool_use_response(vec![read_file_call(
            "call-2", "b.txt",
        )])]);
        let context_messages = vec![Message::user("read files")];

        let action = engine
            .act_with_tools(
                &decision,
                calls_from_decision(&decision),
                &llm,
                &context_messages,
            )
            .await
            .expect("act_with_tools");

        assert_eq!(action.tool_results.len(), 1);
        assert_eq!(action.response_text, "summary");
    }

    #[tokio::test]
    async fn tool_result_has_tool_call_id() {
        let executor = Phase4StubToolExecutor;
        let calls = vec![ToolCall {
            id: "call-42".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": "README.md"}),
        }];

        let results = executor
            .execute_tools(&calls, None)
            .await
            .expect("execute_tools");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].tool_call_id, "call-42");
    }

    #[test]
    fn build_tool_use_assistant_message_creates_correct_blocks() {
        let calls = vec![
            ToolCall {
                id: "call-1".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path": "a.txt"}),
            },
            ToolCall {
                id: "call-2".to_string(),
                name: "run_command".to_string(),
                arguments: serde_json::json!({"command": "ls"}),
            },
        ];

        let message = build_tool_use_assistant_message(&calls);

        assert_eq!(message.role, fx_llm::MessageRole::Assistant);
        assert_eq!(message.content.len(), 2);
        match &message.content[0] {
            ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "call-1");
                assert_eq!(name, "read_file");
                assert_eq!(input["path"], "a.txt");
            }
            other => panic!("expected ToolUse block, got: {other:?}"),
        }
    }

    #[test]
    fn append_tool_round_messages_appends_assistant_then_tool_messages() {
        let calls = vec![read_file_call("call-1", "a.txt")];
        let results = vec![ToolResult {
            tool_call_id: "call-1".to_string(),
            tool_name: "read_file".to_string(),
            success: true,
            output: "ok".to_string(),
        }];
        let mut messages = vec![Message::user("prompt")];

        append_tool_round_messages(&mut messages, &calls, &results)
            .expect("append_tool_round_messages");

        assert_eq!(messages.len(), 3);
        assert_eq!(messages[1].role, fx_llm::MessageRole::Assistant);
        assert_eq!(messages[2].role, fx_llm::MessageRole::Tool);
    }

    #[test]
    fn build_tool_result_message_creates_correct_blocks() {
        let calls = vec![
            read_file_call("call-1", "a.txt"),
            ToolCall {
                id: "call-2".to_string(),
                name: "run_command".to_string(),
                arguments: serde_json::json!({"command": "ls"}),
            },
        ];
        let results = vec![
            ToolResult {
                tool_call_id: "call-2".to_string(),
                tool_name: "run_command".to_string(),
                success: false,
                output: "permission denied".to_string(),
            },
            ToolResult {
                tool_call_id: "call-1".to_string(),
                tool_name: "read_file".to_string(),
                success: true,
                output: "ok".to_string(),
            },
        ];

        let message =
            build_tool_result_message(&calls, &results).expect("build_tool_result_message");

        assert_eq!(message.role, fx_llm::MessageRole::Tool);
        assert_eq!(message.content.len(), 2);
        assert_tool_result_block(&message.content[0], "call-1", "ok");
        assert_tool_result_block(&message.content[1], "call-2", "[ERROR] permission denied");
    }

    #[test]
    fn build_tool_result_message_uses_tool_role() {
        let calls = vec![read_file_call("call-1", "a.txt")];
        let results = vec![ToolResult {
            tool_call_id: "call-1".to_string(),
            tool_name: "read_file".to_string(),
            success: true,
            output: "ok".to_string(),
        }];

        let message =
            build_tool_result_message(&calls, &results).expect("build_tool_result_message");

        assert_eq!(message.role, fx_llm::MessageRole::Tool);
    }

    #[test]
    fn build_tool_result_message_formats_error_with_prefix() {
        let calls = vec![read_file_call("call-1", "a.txt")];
        let results = vec![ToolResult {
            tool_call_id: "call-1".to_string(),
            tool_name: "read_file".to_string(),
            success: false,
            output: "permission denied".to_string(),
        }];

        let message =
            build_tool_result_message(&calls, &results).expect("build_tool_result_message");

        assert_eq!(message.content.len(), 1);
        assert_tool_result_block(&message.content[0], "call-1", "[ERROR] permission denied");
    }

    #[test]
    fn build_tool_result_message_rejects_unmatched_tool_call_id() {
        let calls = vec![read_file_call("call-1", "a.txt")];
        let results = vec![ToolResult {
            tool_call_id: "call-999".to_string(),
            tool_name: "read_file".to_string(),
            success: true,
            output: "ok".to_string(),
        }];

        let error = build_tool_result_message(&calls, &results)
            .expect_err("should reject unmatched tool_call_id");
        assert_eq!(error.stage, "act");
        assert!(
            error.reason.contains("call-999"),
            "error should mention the unmatched id: {}",
            error.reason
        );
    }

    // P4-1: execute_tools_cancellation_between_calls
    #[tokio::test]
    async fn execute_tools_cancellation_between_calls() {
        let count = Arc::new(AtomicU32::new(0));
        let executor = CountingToolExecutor {
            executed_count: Arc::clone(&count),
        };
        let token = CancellationToken::new();

        // 3 tool calls — executor cancels after the first
        let calls = vec![
            ToolCall {
                id: "1".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path": "a.txt"}),
            },
            ToolCall {
                id: "2".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path": "b.txt"}),
            },
            ToolCall {
                id: "3".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path": "c.txt"}),
            },
        ];

        let results = executor
            .execute_tools(&calls, Some(&token))
            .await
            .expect("execute_tools");

        assert_eq!(
            count.load(Ordering::SeqCst),
            1,
            "only the first call should execute before cancellation"
        );
        assert_eq!(results.len(), 1);
    }

    // P4-2: loop_command_stop_ends_cycle
    #[tokio::test]
    async fn loop_command_stop_ends_cycle() {
        let mut engine = p4_engine();
        let (sender, channel) = loop_input_channel();
        engine.set_input_channel(channel);

        // Pre-send Stop before the cycle runs
        sender.send(LoopCommand::Stop).expect("send Stop");

        let llm = Phase4MockLlm::new(vec![CompletionResponse {
            content: vec![ContentBlock::Text {
                text: "hello".to_string(),
            }],
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: None,
        }]);

        let result = engine
            .run_cycle(p4_snapshot("hello"), &llm)
            .await
            .expect("run_cycle");

        assert!(
            matches!(result, LoopResult::UserStopped { .. }),
            "expected LoopResult::UserStopped, got: {result:?}"
        );
    }

    // P4-3: loop_command_abort_ends_immediately
    #[tokio::test]
    async fn loop_command_abort_ends_immediately() {
        let mut engine = p4_engine();
        let (sender, channel) = loop_input_channel();
        engine.set_input_channel(channel);

        sender.send(LoopCommand::Abort).expect("send Abort");

        let llm = Phase4MockLlm::new(vec![CompletionResponse {
            content: vec![ContentBlock::Text {
                text: "hello".to_string(),
            }],
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: None,
        }]);

        let result = engine
            .run_cycle(p4_snapshot("hello"), &llm)
            .await
            .expect("run_cycle");

        assert!(
            matches!(result, LoopResult::UserStopped { .. }),
            "expected LoopResult::UserStopped, got: {result:?}"
        );
    }

    // P4-4: cancellation token stops the cycle (cancelled mid-cycle)
    #[tokio::test]
    async fn cancel_token_stops_cycle() {
        let mut engine = p4_engine();
        let token = CancellationToken::new();
        engine.set_cancel_token(token.clone());

        // LLM cancels the token during complete() to simulate mid-cycle Ctrl+C
        let llm = CancellingMockLlm::new(
            token,
            vec![CompletionResponse {
                content: vec![ContentBlock::Text {
                    text: "hello".to_string(),
                }],
                tool_calls: Vec::new(),
                usage: None,
                stop_reason: None,
            }],
        );

        let result = engine
            .run_cycle(p4_snapshot("hello"), &llm)
            .await
            .expect("run_cycle");

        assert!(
            matches!(result, LoopResult::UserStopped { .. }),
            "expected LoopResult::UserStopped, got: {result:?}"
        );
    }

    // P4-5: UserStopped signals are attached
    #[tokio::test]
    async fn user_stopped_includes_signals() {
        let mut engine = p4_engine();
        let token = CancellationToken::new();
        engine.set_cancel_token(token.clone());

        // LLM cancels mid-cycle to produce a UserStopped
        let llm = CancellingMockLlm::new(
            token,
            vec![CompletionResponse {
                content: vec![ContentBlock::Text {
                    text: "hello".to_string(),
                }],
                tool_calls: Vec::new(),
                usage: None,
                stop_reason: None,
            }],
        );

        let result = engine
            .run_cycle(p4_snapshot("hello"), &llm)
            .await
            .expect("run_cycle");

        match result {
            LoopResult::UserStopped { signals, .. } => {
                assert!(
                    signals.iter().any(|s| s.kind == SignalKind::Blocked),
                    "UserStopped should include a Blocked signal"
                );
            }
            other => panic!("expected UserStopped, got: {other:?}"),
        }
    }

    // B1: Integration test — verify cancellation resets between cycles
    #[tokio::test]
    async fn run_cycle_resets_cancellation_between_cycles() {
        let mut engine = p4_engine();
        let token = CancellationToken::new();
        engine.set_cancel_token(token.clone());

        // First cycle: LLM cancels mid-cycle -> UserStopped
        let llm = CancellingMockLlm::new(
            token.clone(),
            vec![
                // First cycle: LLM response (cancelled during complete())
                CompletionResponse {
                    content: vec![ContentBlock::Text {
                        text: "first response".to_string(),
                    }],
                    tool_calls: Vec::new(),
                    usage: None,
                    stop_reason: None,
                },
            ],
        );

        let result1 = engine
            .run_cycle(p4_snapshot("first"), &llm)
            .await
            .expect("first run_cycle");
        assert!(
            matches!(result1, LoopResult::UserStopped { .. }),
            "first cycle should be UserStopped, got: {result1:?}"
        );

        // Second cycle: prepare_cycle() should have reset the token.
        // Use a normal (non-cancelling) LLM to verify the cycle runs clean.
        let llm2 = Phase4MockLlm::new(vec![CompletionResponse {
            content: vec![ContentBlock::Text {
                text: "second cycle response".to_string(),
            }],
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: None,
        }]);

        let result2 = engine
            .run_cycle(p4_snapshot("second"), &llm2)
            .await
            .expect("second run_cycle");
        assert!(
            matches!(result2, LoopResult::Complete { .. }),
            "second cycle should Complete (token was reset), got: {result2:?}"
        );
    }
}

#[cfg(test)]
mod cancellation_tests {
    use super::*;
    use crate::cancellation::CancellationToken;
    use crate::input::{loop_input_channel, LoopCommand};
    use async_trait::async_trait;
    use fx_core::error::LlmError as CoreLlmError;
    use fx_core::types::{InputSource, ScreenState, UserInput};
    use fx_llm::{
        CompletionRequest, CompletionResponse, ContentBlock, Message, ProviderError, ToolCall,
        ToolDefinition,
    };
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use tokio::time::{Duration, Instant};

    #[derive(Debug, Default)]
    struct NoopToolExecutor;

    #[async_trait]
    impl ToolExecutor for NoopToolExecutor {
        async fn execute_tools(
            &self,
            calls: &[ToolCall],
            _cancel: Option<&CancellationToken>,
        ) -> Result<Vec<ToolResult>, crate::act::ToolExecutorError> {
            Ok(calls.iter().map(success_result).collect())
        }

        fn tool_definitions(&self) -> Vec<ToolDefinition> {
            vec![read_file_definition()]
        }
    }

    #[derive(Debug)]
    struct DelayedToolExecutor {
        delay: Duration,
    }

    impl DelayedToolExecutor {
        fn new(delay: Duration) -> Self {
            Self { delay }
        }
    }

    #[async_trait]
    impl ToolExecutor for DelayedToolExecutor {
        async fn execute_tools(
            &self,
            calls: &[ToolCall],
            cancel: Option<&CancellationToken>,
        ) -> Result<Vec<ToolResult>, crate::act::ToolExecutorError> {
            wait_for_delay_or_cancel(self.delay, cancel).await;
            if cancel.is_some_and(CancellationToken::is_cancelled) {
                return Ok(Vec::new());
            }
            Ok(calls.iter().map(success_result).collect())
        }

        fn tool_definitions(&self) -> Vec<ToolDefinition> {
            vec![read_file_definition()]
        }
    }

    #[derive(Debug)]
    struct RoundCancellingToolExecutor {
        delay: Duration,
        rounds: Arc<AtomicUsize>,
        cancel_after_round: usize,
    }

    impl RoundCancellingToolExecutor {
        fn new(delay: Duration, rounds: Arc<AtomicUsize>, cancel_after_round: usize) -> Self {
            Self {
                delay,
                rounds,
                cancel_after_round,
            }
        }
    }

    #[async_trait]
    impl ToolExecutor for RoundCancellingToolExecutor {
        async fn execute_tools(
            &self,
            calls: &[ToolCall],
            cancel: Option<&CancellationToken>,
        ) -> Result<Vec<ToolResult>, crate::act::ToolExecutorError> {
            tokio::time::sleep(self.delay).await;
            let current_round = self.rounds.fetch_add(1, Ordering::SeqCst) + 1;
            let results = calls.iter().map(success_result).collect();
            if current_round >= self.cancel_after_round {
                if let Some(token) = cancel {
                    token.cancel();
                }
            }
            Ok(results)
        }

        fn tool_definitions(&self) -> Vec<ToolDefinition> {
            vec![read_file_definition()]
        }
    }

    #[derive(Debug)]
    struct ScriptedLlm {
        responses: Mutex<VecDeque<CompletionResponse>>,
    }

    impl ScriptedLlm {
        fn new(responses: Vec<CompletionResponse>) -> Self {
            Self {
                responses: Mutex::new(VecDeque::from(responses)),
            }
        }
    }

    #[async_trait]
    impl LlmProvider for ScriptedLlm {
        async fn generate(&self, _: &str, _: u32) -> Result<String, CoreLlmError> {
            Ok("summary".to_string())
        }

        async fn generate_streaming(
            &self,
            _: &str,
            _: u32,
            callback: Box<dyn Fn(String) + Send + 'static>,
        ) -> Result<String, CoreLlmError> {
            callback("summary".to_string());
            Ok("summary".to_string())
        }

        fn model_name(&self) -> &str {
            "scripted"
        }

        async fn complete(
            &self,
            _: CompletionRequest,
        ) -> Result<CompletionResponse, ProviderError> {
            self.responses
                .lock()
                .expect("lock")
                .pop_front()
                .ok_or_else(|| ProviderError::Provider("no response".to_string()))
        }
    }

    fn engine_with_executor(executor: Arc<dyn ToolExecutor>, max_iterations: u32) -> LoopEngine {
        LoopEngine::new(
            BudgetTracker::new(crate::budget::BudgetConfig::default(), 0, 0),
            ContextCompactor::new(2048, 256),
            max_iterations,
            executor,
            "Summarize tool output".to_string(),
        )
    }

    fn test_snapshot(text: &str) -> PerceptionSnapshot {
        PerceptionSnapshot {
            timestamp_ms: 1,
            screen: ScreenState {
                current_app: "terminal".to_string(),
                elements: Vec::new(),
                text_content: text.to_string(),
            },
            notifications: Vec::new(),
            active_app: "terminal".to_string(),
            user_input: Some(UserInput {
                text: text.to_string(),
                source: InputSource::Text,
                timestamp: 1,
                context_id: None,
            }),
            sensor_data: None,
            conversation_history: vec![Message::user(text)],
        }
    }

    fn read_file_definition() -> ToolDefinition {
        ToolDefinition {
            name: "read_file".to_string(),
            description: "Read a file".to_string(),
            parameters: serde_json::json!({"type":"object"}),
        }
    }

    fn read_file_call(id: &str) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path":"README.md"}),
        }
    }

    fn success_result(call: &ToolCall) -> ToolResult {
        ToolResult {
            tool_call_id: call.id.clone(),
            tool_name: call.name.clone(),
            success: true,
            output: "ok".to_string(),
        }
    }

    fn tool_use_response(call_id: &str) -> CompletionResponse {
        CompletionResponse {
            content: Vec::new(),
            tool_calls: vec![read_file_call(call_id)],
            usage: None,
            stop_reason: Some("tool_use".to_string()),
        }
    }

    fn text_response(text: &str) -> CompletionResponse {
        CompletionResponse {
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: None,
        }
    }

    async fn wait_for_cancel(token: &CancellationToken) {
        while !token.is_cancelled() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    async fn wait_for_delay_or_cancel(delay: Duration, cancel: Option<&CancellationToken>) {
        if let Some(token) = cancel {
            tokio::select! {
                _ = tokio::time::sleep(delay) => {}
                _ = wait_for_cancel(token) => {}
            }
            return;
        }
        tokio::time::sleep(delay).await;
    }

    async fn run_cycle_with_inflight_command(command: LoopCommand) -> (LoopResult, usize) {
        let rounds = Arc::new(AtomicUsize::new(0));
        let executor = RoundCancellingToolExecutor::new(
            Duration::from_millis(120),
            Arc::clone(&rounds),
            usize::MAX,
        );
        let mut engine = engine_with_executor(Arc::new(executor), 4);
        let (sender, channel) = loop_input_channel();
        engine.set_input_channel(channel);
        let llm = ScriptedLlm::new(vec![
            tool_use_response("call-1"),
            tool_use_response("call-2"),
            text_response("done"),
        ]);

        let send_task = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            sender.send(command).expect("send command");
        });

        let result = engine
            .run_cycle(test_snapshot("read file"), &llm)
            .await
            .expect("run_cycle");
        send_task.await.expect("send task");
        (result, rounds.load(Ordering::SeqCst))
    }

    #[test]
    fn check_user_input_prioritizes_abort_over_queued_commands() {
        let mut engine = engine_with_executor(Arc::new(NoopToolExecutor), 3);
        let (sender, channel) = loop_input_channel();
        engine.set_input_channel(channel);

        sender.send(LoopCommand::Stop).expect("send Stop");
        sender.send(LoopCommand::Abort).expect("send Abort");
        sender.send(LoopCommand::Resume).expect("send Resume");

        assert_eq!(engine.check_user_input(), Some(LoopCommand::Abort));
    }

    #[test]
    fn check_cancellation_without_token_or_input_returns_none() {
        let mut engine = engine_with_executor(Arc::new(NoopToolExecutor), 3);
        assert!(engine.check_cancellation(None).is_none());
    }

    #[tokio::test]
    async fn cancellation_during_delayed_tool_execution_returns_user_stopped_quickly() {
        let token = CancellationToken::new();
        let mut engine = engine_with_executor(
            Arc::new(DelayedToolExecutor::new(Duration::from_secs(5))),
            4,
        );
        engine.set_cancel_token(token.clone());
        let llm = ScriptedLlm::new(vec![tool_use_response("call-1")]);

        let cancel_task = tokio::spawn({
            let token = token.clone();
            async move {
                tokio::time::sleep(Duration::from_millis(40)).await;
                token.cancel();
            }
        });

        let started = Instant::now();
        let result = engine
            .run_cycle(test_snapshot("read file"), &llm)
            .await
            .expect("run_cycle");
        cancel_task.await.expect("cancel task");

        assert!(
            matches!(result, LoopResult::UserStopped { .. }),
            "expected UserStopped, got: {result:?}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "cancellation should return quickly"
        );
    }

    #[tokio::test]
    async fn cancellation_between_tool_continuation_rounds_returns_user_stopped() {
        let token = CancellationToken::new();
        let rounds = Arc::new(AtomicUsize::new(0));
        let executor =
            RoundCancellingToolExecutor::new(Duration::from_millis(20), Arc::clone(&rounds), 1);
        let mut engine = engine_with_executor(Arc::new(executor), 4);
        engine.set_cancel_token(token);

        let llm = ScriptedLlm::new(vec![
            tool_use_response("call-1"),
            tool_use_response("call-2"),
        ]);

        let result = engine
            .run_cycle(test_snapshot("read files"), &llm)
            .await
            .expect("run_cycle");

        assert!(
            matches!(result, LoopResult::UserStopped { .. }),
            "expected UserStopped, got: {result:?}"
        );
        assert_eq!(
            rounds.load(Ordering::SeqCst),
            1,
            "cancellation should stop before the second tool round executes"
        );
    }

    #[tokio::test]
    async fn stop_command_sent_during_tool_round_is_caught_at_iteration_boundary() {
        let (result, rounds) = run_cycle_with_inflight_command(LoopCommand::Stop).await;
        assert!(
            matches!(result, LoopResult::UserStopped { .. }),
            "expected UserStopped for Stop, got: {result:?}"
        );
        assert_eq!(
            rounds, 1,
            "Stop should be caught before the second tool round executes"
        );
    }

    #[tokio::test]
    async fn abort_command_sent_during_tool_round_is_caught_at_iteration_boundary() {
        let (result, rounds) = run_cycle_with_inflight_command(LoopCommand::Abort).await;
        assert!(
            matches!(result, LoopResult::UserStopped { .. }),
            "expected UserStopped for Abort, got: {result:?}"
        );
        assert_eq!(
            rounds, 1,
            "Abort should be caught before the second tool round executes"
        );
    }
}

#[cfg(test)]
mod fallback_retry_tests {
    use super::*;
    use async_trait::async_trait;
    use fx_core::error::LlmError as CoreLlmError;
    use fx_core::types::{InputSource, ScreenState, UserInput};
    use fx_llm::{
        CompletionResponse, ContentBlock, Message, ProviderError, ToolCall, ToolDefinition,
    };
    use std::collections::VecDeque;
    use std::sync::Mutex;

    #[derive(Debug, Default)]
    struct StubToolExecutor;

    #[async_trait]
    impl ToolExecutor for StubToolExecutor {
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
                })
                .collect())
        }

        fn tool_definitions(&self) -> Vec<ToolDefinition> {
            vec![ToolDefinition {
                name: "read_file".to_string(),
                description: "Read a file".to_string(),
                parameters: serde_json::json!({"type":"object"}),
            }]
        }
    }

    #[derive(Debug)]
    struct SequentialMockLlm {
        responses: Mutex<VecDeque<CompletionResponse>>,
    }

    impl SequentialMockLlm {
        fn new(responses: Vec<CompletionResponse>) -> Self {
            Self {
                responses: Mutex::new(VecDeque::from(responses)),
            }
        }
    }

    #[async_trait]
    impl LlmProvider for SequentialMockLlm {
        async fn generate(&self, _: &str, _: u32) -> Result<String, CoreLlmError> {
            Ok("summary".to_string())
        }

        async fn generate_streaming(
            &self,
            _: &str,
            _: u32,
            callback: Box<dyn Fn(String) + Send + 'static>,
        ) -> Result<String, CoreLlmError> {
            callback("summary".to_string());
            Ok("summary".to_string())
        }

        fn model_name(&self) -> &str {
            "mock"
        }

        async fn complete(
            &self,
            _: CompletionRequest,
        ) -> Result<CompletionResponse, ProviderError> {
            self.responses
                .lock()
                .expect("lock")
                .pop_front()
                .ok_or_else(|| ProviderError::Provider("no response".to_string()))
        }
    }

    fn test_engine() -> LoopEngine {
        LoopEngine::new(
            BudgetTracker::new(crate::budget::BudgetConfig::default(), 0, 0),
            ContextCompactor::new(2048, 256),
            3,
            Arc::new(StubToolExecutor),
            "Summarize tool output".to_string(),
        )
    }

    fn test_snapshot(text: &str) -> PerceptionSnapshot {
        PerceptionSnapshot {
            timestamp_ms: 1,
            screen: ScreenState {
                current_app: "terminal".to_string(),
                elements: Vec::new(),
                text_content: text.to_string(),
            },
            notifications: Vec::new(),
            active_app: "terminal".to_string(),
            user_input: Some(UserInput {
                text: text.to_string(),
                source: InputSource::Text,
                timestamp: 1,
                context_id: None,
            }),
            sensor_data: None,
            conversation_history: vec![Message::user(text)],
        }
    }

    #[tokio::test]
    async fn verify_detects_safe_fallback_as_discrepancy() {
        let mut engine = test_engine();
        let action = ActionResult {
            decision: Decision::Respond(SAFE_FALLBACK_RESPONSE.to_string()),
            tool_results: Vec::new(),
            response_text: SAFE_FALLBACK_RESPONSE.to_string(),
            tokens_used: TokenUsage::default(),
        };

        let verification = engine.verify(&action).await.expect("verify");
        assert!(
            !verification.outcome_satisfactory,
            "safe fallback should not pass verification"
        );
        assert!(
            verification
                .discrepancies
                .iter()
                .any(|d| d.contains("fallback")),
            "discrepancy should mention fallback: {:?}",
            verification.discrepancies
        );
    }

    #[tokio::test]
    async fn verify_does_not_flag_fallback_when_tools_were_used() {
        let mut engine = test_engine();
        let action = ActionResult {
            decision: Decision::UseTools(vec![ToolCall {
                id: "1".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path": "a.txt"}),
            }]),
            tool_results: vec![ToolResult {
                tool_call_id: "1".to_string(),
                tool_name: "read_file".to_string(),
                success: true,
                output: "ok".to_string(),
            }],
            response_text: SAFE_FALLBACK_RESPONSE.to_string(),
            tokens_used: TokenUsage::default(),
        };

        let verification = engine.verify(&action).await.expect("verify");
        assert!(
            verification.outcome_satisfactory,
            "fallback with tools used should still pass (tools produced results)"
        );
        assert!(
            verification.discrepancies.is_empty(),
            "fallback with tools used should not report discrepancies: {:?}",
            verification.discrepancies
        );
    }

    #[tokio::test]
    async fn should_continue_returns_tool_directive_for_fallback() {
        let mut engine = test_engine();
        let decision = Decision::Respond(SAFE_FALLBACK_RESPONSE.to_string());
        let verification = Verification {
            outcome_satisfactory: false,
            confidence: 0.45,
            discrepancies: vec!["model produced fallback".to_string()],
        };
        let learning = Learning {
            episode: "test".to_string(),
            pattern: None,
            adjustment: None,
        };

        let continuation = engine
            .should_continue(&decision, &verification, &learning)
            .await
            .expect("should_continue");

        match continuation {
            Continuation::Continue(msg) => {
                assert!(
                    msg.contains("did not use tools"),
                    "continuation for fallback should mention tools: {msg}"
                );
            }
            other => panic!("expected Continue, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn should_continue_returns_generic_message_for_non_fallback_failure() {
        let mut engine = test_engine();
        let decision = Decision::Respond("some other response".to_string());
        let verification = Verification {
            outcome_satisfactory: false,
            confidence: 0.45,
            discrepancies: vec!["action produced an empty response".to_string()],
        };
        let learning = Learning {
            episode: "test".to_string(),
            pattern: None,
            adjustment: None,
        };

        let continuation = engine
            .should_continue(&decision, &verification, &learning)
            .await
            .expect("should_continue");

        match continuation {
            Continuation::Continue(msg) => {
                assert!(
                    msg.contains("produced no response"),
                    "non-fallback continuation should use generic message: {msg}"
                );
            }
            other => panic!("expected Continue, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn run_cycle_retries_on_fallback_then_succeeds_with_tools() {
        let mut engine = test_engine();

        // First response: model returns empty text (no tools) -> fallback
        // Second response (retry): model uses tools
        // Third response: tool continuation with final answer
        let llm = SequentialMockLlm::new(vec![
            // Iteration 1: empty response -> SAFE_FALLBACK
            CompletionResponse {
                content: Vec::new(),
                tool_calls: Vec::new(),
                usage: None,
                stop_reason: None,
            },
            // Iteration 2 (retry): model uses tools
            CompletionResponse {
                content: Vec::new(),
                tool_calls: vec![ToolCall {
                    id: "call-1".to_string(),
                    name: "read_file".to_string(),
                    arguments: serde_json::json!({"path": "README.md"}),
                }],
                usage: None,
                stop_reason: Some("tool_use".to_string()),
            },
            // Tool continuation: final answer
            CompletionResponse {
                content: vec![ContentBlock::Text {
                    text: "Here are the file contents.".to_string(),
                }],
                tool_calls: Vec::new(),
                usage: None,
                stop_reason: None,
            },
        ]);

        let result = engine
            .run_cycle(test_snapshot("read the readme"), &llm)
            .await
            .expect("run_cycle");

        match result {
            LoopResult::Complete {
                response,
                iterations,
                ..
            } => {
                assert_eq!(iterations, 2, "should take 2 iterations (fallback + retry)");
                assert_eq!(response, "Here are the file contents.");
            }
            other => panic!("expected Complete, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn run_cycle_exhausts_iterations_on_repeated_fallback() {
        let mut engine = LoopEngine::new(
            BudgetTracker::new(crate::budget::BudgetConfig::default(), 0, 0),
            ContextCompactor::new(2048, 256),
            2,
            Arc::new(StubToolExecutor),
            "Summarize".to_string(),
        );

        // Both iterations return empty -> fallback each time
        let llm = SequentialMockLlm::new(vec![
            CompletionResponse {
                content: Vec::new(),
                tool_calls: Vec::new(),
                usage: None,
                stop_reason: None,
            },
            CompletionResponse {
                content: Vec::new(),
                tool_calls: Vec::new(),
                usage: None,
                stop_reason: None,
            },
        ]);

        let result = engine
            .run_cycle(test_snapshot("do something broad"), &llm)
            .await
            .expect("run_cycle");

        assert!(
            matches!(result, LoopResult::Error { .. }),
            "repeated fallbacks should exhaust iterations: {result:?}"
        );
    }
}

#[cfg(test)]
mod decomposition_tests {
    use super::*;
    use crate::budget::BudgetConfig;
    use async_trait::async_trait;
    use fx_core::message::InternalMessage;
    use fx_decompose::{AggregationStrategy, DecompositionPlan, SubGoal};
    use fx_llm::{
        CompletionRequest, CompletionResponse, ContentBlock, Message, ProviderError, ToolCall,
        ToolDefinition,
    };
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    #[derive(Debug, Default)]
    struct PassiveToolExecutor;

    #[async_trait]
    impl ToolExecutor for PassiveToolExecutor {
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
                })
                .collect())
        }
    }

    #[derive(Debug)]
    struct ScriptedLlm {
        responses: Mutex<VecDeque<Result<CompletionResponse, ProviderError>>>,
        complete_calls: AtomicUsize,
    }

    impl ScriptedLlm {
        fn new(responses: Vec<Result<CompletionResponse, ProviderError>>) -> Self {
            Self {
                responses: Mutex::new(VecDeque::from(responses)),
                complete_calls: AtomicUsize::new(0),
            }
        }

        fn complete_calls(&self) -> usize {
            self.complete_calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl LlmProvider for ScriptedLlm {
        async fn generate(&self, _: &str, _: u32) -> Result<String, fx_core::error::LlmError> {
            Ok("summary".to_string())
        }

        async fn generate_streaming(
            &self,
            _: &str,
            _: u32,
            callback: Box<dyn Fn(String) + Send + 'static>,
        ) -> Result<String, fx_core::error::LlmError> {
            callback("summary".to_string());
            Ok("summary".to_string())
        }

        fn model_name(&self) -> &str {
            "scripted-llm"
        }

        async fn complete(
            &self,
            _: CompletionRequest,
        ) -> Result<CompletionResponse, ProviderError> {
            self.complete_calls.fetch_add(1, Ordering::SeqCst);
            self.responses
                .lock()
                .expect("lock")
                .pop_front()
                .unwrap_or_else(|| Err(ProviderError::Provider("no scripted response".to_string())))
        }
    }

    fn budget_config(max_llm_calls: u32, max_recursion_depth: u32) -> BudgetConfig {
        BudgetConfig {
            max_llm_calls,
            max_tool_invocations: 20,
            max_tokens: 10_000,
            max_cost_cents: 100,
            max_wall_time_ms: 60_000,
            max_recursion_depth,
        }
    }

    fn decomposition_engine(config: BudgetConfig, depth: u32) -> LoopEngine {
        let started_at_ms = current_time_ms();
        LoopEngine::new(
            BudgetTracker::new(config, started_at_ms, depth),
            ContextCompactor::new(2048, 256),
            4,
            Arc::new(PassiveToolExecutor),
            "Summarize tool output".to_string(),
        )
    }

    fn decomposition_plan(descriptions: &[&str]) -> DecompositionPlan {
        DecompositionPlan {
            sub_goals: descriptions
                .iter()
                .map(|description| SubGoal {
                    description: (*description).to_string(),
                    required_tools: Vec::new(),
                    expected_output: Some(format!("output for {description}")),
                })
                .collect(),
            strategy: AggregationStrategy::Sequential,
            truncated_from: None,
        }
    }

    async fn collect_internal_events(
        receiver: &mut tokio::sync::broadcast::Receiver<InternalMessage>,
        count: usize,
    ) -> Vec<InternalMessage> {
        let mut events = Vec::with_capacity(count);
        for _ in 0..count {
            events.push(receiver.recv().await.expect("event"));
        }
        events
    }

    fn text_response(text: &str) -> CompletionResponse {
        CompletionResponse {
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: None,
        }
    }

    fn decompose_tool_call(arguments: serde_json::Value) -> ToolCall {
        ToolCall {
            id: "decompose-call".to_string(),
            name: DECOMPOSE_TOOL_NAME.to_string(),
            arguments,
        }
    }

    fn sample_tool_definition() -> ToolDefinition {
        ToolDefinition {
            name: "read_file".to_string(),
            description: "Read files".to_string(),
            parameters: serde_json::json!({"type": "object"}),
        }
    }

    fn sample_budget_remaining() -> BudgetRemaining {
        BudgetRemaining {
            llm_calls: 8,
            tool_invocations: 10,
            tokens: 2_000,
            cost_cents: 50,
            wall_time_ms: 5_000,
        }
    }

    fn sample_perception() -> ProcessedPerception {
        ProcessedPerception {
            user_message: "Break this task into phases".to_string(),
            context_window: vec![Message::user("context")],
            active_goals: vec!["Help the user".to_string()],
            budget_remaining: sample_budget_remaining(),
        }
    }

    fn assert_decompose_tool_present(tools: &[ToolDefinition]) {
        let decompose_tools = tools
            .iter()
            .filter(|tool| tool.name == DECOMPOSE_TOOL_NAME)
            .collect::<Vec<_>>();
        assert_eq!(
            decompose_tools.len(),
            1,
            "decompose tool should be present once"
        );
        assert_eq!(decompose_tools[0].description, DECOMPOSE_TOOL_DESCRIPTION);
        assert_eq!(
            decompose_tools[0].parameters["required"],
            serde_json::json!(["sub_goals"])
        );
    }

    #[tokio::test]
    async fn decomposition_uses_half_remaining_budget_for_each_sub_goal() {
        let mut engine = decomposition_engine(budget_config(4, 6), 0);
        let plan = decomposition_plan(&["first", "second", "third"]);
        let decision = Decision::Decompose(plan.clone());
        let llm = ScriptedLlm::new(vec![
            Ok(text_response("first-ok")),
            Ok(text_response("second-ok")),
            Ok(text_response("third-ok")),
        ]);

        let action = engine
            .execute_decomposition(&decision, &plan, &llm, &[])
            .await
            .expect("decomposition");

        assert_eq!(llm.complete_calls(), 3);
        assert!(action
            .response_text
            .contains("first => completed: first-ok"));
        assert!(action
            .response_text
            .contains("second => completed: second-ok"));
        assert!(action
            .response_text
            .contains("third => completed: third-ok"));

        let status = engine.status(current_time_ms());
        assert_eq!(status.llm_calls_used, 3);
        assert_eq!(status.remaining.llm_calls, 1);
        assert_eq!(status.tool_invocations_used, 0);
        assert_eq!(status.cost_cents_used, 6);
        assert!(status.tokens_used > 0);
    }

    #[test]
    fn child_max_iterations_caps_at_three() {
        assert_eq!(child_max_iterations(10), 3);
        assert_eq!(child_max_iterations(3), 3);
        assert_eq!(child_max_iterations(2), 2);
        assert_eq!(child_max_iterations(1), 1);
    }

    #[tokio::test]
    async fn sub_goal_failure_does_not_stop_remaining_sub_goals() {
        let mut engine = decomposition_engine(budget_config(20, 6), 0);
        let plan = decomposition_plan(&["first", "second", "third"]);
        let decision = Decision::Decompose(plan.clone());
        let llm = ScriptedLlm::new(vec![
            Ok(text_response("first-ok")),
            Err(ProviderError::Provider("boom".to_string())),
            Ok(text_response("third-ok")),
        ]);

        let action = engine
            .execute_decomposition(&decision, &plan, &llm, &[])
            .await
            .expect("decomposition");

        assert_eq!(llm.complete_calls(), 3);
        assert!(action
            .response_text
            .contains("first => completed: first-ok"));
        assert!(action.response_text.contains("second => failed:"));
        assert!(action
            .response_text
            .contains("third => completed: third-ok"));
    }

    #[tokio::test]
    async fn budget_exhausted_sub_goal_maps_to_budget_exhausted_outcome() {
        let mut engine = decomposition_engine(budget_config(0, 6), 0);
        let plan = decomposition_plan(&["budget-limited"]);
        let decision = Decision::Decompose(plan.clone());
        let llm = ScriptedLlm::new(vec![Ok(text_response("unused"))]);

        let action = engine
            .execute_decomposition(&decision, &plan, &llm, &[])
            .await
            .expect("decomposition");

        assert_eq!(llm.complete_calls(), 0);
        assert!(action
            .response_text
            .contains("budget-limited => budget exhausted"));
    }

    #[tokio::test]
    async fn decomposition_rolls_up_child_signals_into_parent_collector() {
        let mut engine = decomposition_engine(budget_config(10, 6), 0);
        let plan = decomposition_plan(&["collect-signals"]);
        let decision = Decision::Decompose(plan.clone());
        let llm = ScriptedLlm::new(vec![Ok(text_response("done"))]);

        let _action = engine
            .execute_decomposition(&decision, &plan, &llm, &[])
            .await
            .expect("decomposition");

        assert!(engine
            .signals
            .signals()
            .iter()
            .any(|signal| signal.step == LoopStep::Perceive));
    }

    #[tokio::test]
    async fn decomposition_emits_progress_trace_for_each_sub_goal() {
        let mut engine = decomposition_engine(budget_config(10, 6), 0);
        let plan = decomposition_plan(&["first", "second"]);
        let decision = Decision::Decompose(plan.clone());
        let llm = ScriptedLlm::new(vec![Ok(text_response("one")), Ok(text_response("two"))]);

        let _action = engine
            .execute_decomposition(&decision, &plan, &llm, &[])
            .await
            .expect("decomposition");

        let progress_traces = engine
            .signals
            .signals()
            .iter()
            .filter(|signal| {
                signal.step == LoopStep::Act
                    && signal.kind == SignalKind::Trace
                    && signal.message.starts_with("Sub-goal ")
            })
            .collect::<Vec<_>>();

        assert_eq!(progress_traces.len(), 2);
        assert_eq!(progress_traces[0].message, "Sub-goal 1/2: first");
        assert_eq!(
            progress_traces[0].metadata["sub_goal_index"],
            serde_json::json!(0)
        );
        assert_eq!(progress_traces[0].metadata["total"], serde_json::json!(2));
        assert_eq!(progress_traces[1].message, "Sub-goal 2/2: second");
        assert_eq!(
            progress_traces[1].metadata["sub_goal_index"],
            serde_json::json!(1)
        );
        assert_eq!(progress_traces[1].metadata["total"], serde_json::json!(2));
    }

    #[tokio::test]
    async fn concurrent_execution_rolls_up_signals_from_all_children() {
        let mut engine = decomposition_engine(budget_config(10, 6), 0);
        let plan = concurrent_plan(&["signal-a", "signal-b"]);
        let decision = Decision::Decompose(plan.clone());
        let llm = ScriptedLlm::new(vec![Ok(text_response("one")), Ok(text_response("two"))]);

        let _action = engine
            .execute_decomposition(&decision, &plan, &llm, &[])
            .await
            .expect("decomposition");

        let perceive_count = engine
            .signals
            .signals()
            .iter()
            .filter(|signal| signal.step == LoopStep::Perceive)
            .count();
        assert!(perceive_count >= 2);
    }

    #[tokio::test]
    async fn concurrent_execution_emits_progress_events_via_event_bus() {
        let mut engine = decomposition_engine(budget_config(10, 6), 0);
        let bus = fx_core::EventBus::new(16);
        let mut receiver = bus.subscribe();
        engine.set_event_bus(bus);

        let plan = concurrent_plan(&["first", "second"]);
        let decision = Decision::Decompose(plan.clone());
        let llm = ScriptedLlm::new(vec![Ok(text_response("one")), Ok(text_response("two"))]);

        let _action = engine
            .execute_decomposition(&decision, &plan, &llm, &[])
            .await
            .expect("decomposition");

        let events = collect_internal_events(&mut receiver, 4).await;
        assert!(
            matches!(&events[0], InternalMessage::SubGoalStarted { index: 0, total: 2, description } if description == "first")
        );
        assert!(
            matches!(&events[1], InternalMessage::SubGoalStarted { index: 1, total: 2, description } if description == "second")
        );
        assert!(matches!(
            events[2],
            InternalMessage::SubGoalCompleted {
                index: 0,
                total: 2,
                success: true
            }
        ));
        assert!(matches!(
            events[3],
            InternalMessage::SubGoalCompleted {
                index: 1,
                total: 2,
                success: true
            }
        ));
    }

    #[tokio::test]
    async fn sequential_execution_emits_progress_events_via_event_bus() {
        let mut engine = decomposition_engine(budget_config(10, 6), 0);
        let bus = fx_core::EventBus::new(16);
        let mut receiver = bus.subscribe();
        engine.set_event_bus(bus);

        let plan = decomposition_plan(&["first", "second"]);
        let decision = Decision::Decompose(plan.clone());
        let llm = ScriptedLlm::new(vec![Ok(text_response("one")), Ok(text_response("two"))]);

        let _action = engine
            .execute_decomposition(&decision, &plan, &llm, &[])
            .await
            .expect("decomposition");

        let events = collect_internal_events(&mut receiver, 4).await;
        assert!(
            matches!(&events[0], InternalMessage::SubGoalStarted { index: 0, total: 2, description } if description == "first")
        );
        assert!(matches!(
            events[1],
            InternalMessage::SubGoalCompleted {
                index: 0,
                total: 2,
                success: true
            }
        ));
        assert!(
            matches!(&events[2], InternalMessage::SubGoalStarted { index: 1, total: 2, description } if description == "second")
        );
        assert!(matches!(
            events[3],
            InternalMessage::SubGoalCompleted {
                index: 1,
                total: 2,
                success: true
            }
        ));
    }

    #[tokio::test]
    async fn decomposition_emits_truncation_signal_when_plan_is_truncated() {
        let mut engine = decomposition_engine(budget_config(10, 6), 0);
        let mut plan = decomposition_plan(&["first"]);
        plan.truncated_from = Some(8);
        let decision = Decision::Decompose(plan.clone());
        let llm = ScriptedLlm::new(vec![Ok(text_response("done"))]);

        let _action = engine
            .execute_decomposition(&decision, &plan, &llm, &[])
            .await
            .expect("decomposition");

        let truncation_signal = engine
            .signals
            .signals()
            .iter()
            .find(|signal| {
                signal.step == LoopStep::Act
                    && signal.kind == SignalKind::Friction
                    && signal.message == "decomposition plan truncated to max sub-goals"
            })
            .expect("truncation signal");

        assert_eq!(
            truncation_signal.metadata["original_sub_goals"],
            serde_json::json!(8)
        );
        assert_eq!(
            truncation_signal.metadata["retained_sub_goals"],
            serde_json::json!(1)
        );
        assert_eq!(
            truncation_signal.metadata["max_sub_goals"],
            serde_json::json!(MAX_SUB_GOALS)
        );
    }

    #[tokio::test]
    async fn decomposition_at_depth_limit_returns_fallback_without_child_execution() {
        let mut engine = decomposition_engine(budget_config(10, 1), 1);
        let plan = decomposition_plan(&["depth-guarded"]);
        let decision = Decision::Decompose(plan.clone());
        let llm = ScriptedLlm::new(vec![Ok(text_response("unused"))]);

        let action = engine
            .execute_decomposition(&decision, &plan, &llm, &[])
            .await
            .expect("decomposition");

        assert_eq!(llm.complete_calls(), 0);
        assert!(action
            .response_text
            .contains("recursion depth limit was reached"));
    }

    #[tokio::test]
    async fn aggregated_response_includes_results_from_all_sub_goals() {
        let mut engine = decomposition_engine(budget_config(10, 6), 0);
        let plan = decomposition_plan(&["analyze", "summarize"]);
        let decision = Decision::Decompose(plan.clone());
        let llm = ScriptedLlm::new(vec![
            Ok(text_response("analysis")),
            Ok(text_response("summary")),
        ]);

        let action = engine
            .execute_decomposition(&decision, &plan, &llm, &[])
            .await
            .expect("decomposition");

        assert!(
            action
                .response_text
                .contains("analyze => completed: analysis"),
            "unexpected aggregate response: {}",
            action.response_text
        );
        assert!(
            action
                .response_text
                .contains("summarize => completed: summary"),
            "unexpected aggregate response: {}",
            action.response_text
        );
    }

    #[test]
    fn estimate_action_cost_for_decompose_scales_with_sub_goal_count() {
        let engine = decomposition_engine(budget_config(10, 6), 0);
        let plan = decomposition_plan(&["a", "b", "c"]);
        let cost = engine.estimate_action_cost(&Decision::Decompose(plan));

        assert_eq!(cost.llm_calls, 3);
        assert_eq!(cost.tool_invocations, 0);
        assert_eq!(cost.tokens, TOOL_SYNTHESIS_TOKEN_HEURISTIC * 3);
        assert_eq!(cost.cost_cents, DEFAULT_LLM_ACTION_COST_CENTS * 3);
    }

    #[test]
    fn decision_variant_labels_decompose_decisions() {
        let plan = decomposition_plan(&["single"]);
        assert_eq!(decision_variant(&Decision::Decompose(plan)), "Decompose");
    }

    #[test]
    fn emit_decision_signals_includes_decomposition_metadata() {
        let mut engine = decomposition_engine(budget_config(10, 6), 0);
        let decision = Decision::Decompose(DecompositionPlan {
            sub_goals: decomposition_plan(&["one", "two"]).sub_goals,
            strategy: AggregationStrategy::Parallel,
            truncated_from: None,
        });

        engine.emit_decision_signals(&decision);

        let decomposition_trace = engine
            .signals
            .signals()
            .iter()
            .find(|signal| signal.message == "task decomposition initiated")
            .expect("trace signal");

        assert_eq!(
            decomposition_trace.metadata["sub_goals"],
            serde_json::json!(2)
        );
        assert_eq!(
            decomposition_trace.metadata["strategy"],
            serde_json::json!("Parallel")
        );
    }

    #[tokio::test]
    async fn decide_decompose_drops_other_tools_with_signal() {
        let mut engine = decomposition_engine(budget_config(10, 6), 0);
        let response = CompletionResponse {
            content: Vec::new(),
            tool_calls: vec![
                ToolCall {
                    id: "regular-tool".to_string(),
                    name: "read_file".to_string(),
                    arguments: serde_json::json!({"path": "Cargo.toml"}),
                },
                decompose_tool_call(serde_json::json!({
                    "sub_goals": [{
                        "description": "Inspect crate configuration",
                        "required_tools": ["read_file"],
                        "expected_output": "Cargo metadata"
                    }],
                    "strategy": "Sequential"
                })),
            ],
            usage: None,
            stop_reason: None,
        };

        let decision = engine.decide(&response).await.expect("decision");
        match decision {
            Decision::Decompose(plan) => {
                assert_eq!(plan.sub_goals.len(), 1);
                assert_eq!(plan.sub_goals[0].description, "Inspect crate configuration");
                assert_eq!(plan.sub_goals[0].required_tools, vec!["read_file"]);
                assert_eq!(
                    plan.sub_goals[0].expected_output,
                    Some("Cargo metadata".to_string())
                );
                assert_eq!(plan.strategy, AggregationStrategy::Sequential);
                assert_eq!(plan.truncated_from, None);
            }
            other => panic!("expected decomposition decision, got: {other:?}"),
        }

        let drop_signal = engine
            .signals
            .signals()
            .iter()
            .find(|signal| {
                signal.step == LoopStep::Decide
                    && signal.kind == SignalKind::Trace
                    && signal.message == "decompose takes precedence; dropping other tool calls"
            })
            .expect("drop trace signal");

        assert_eq!(drop_signal.metadata["dropped_count"], serde_json::json!(1));
    }

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

    #[tokio::test]
    async fn decide_rejects_empty_sub_goals() {
        let mut engine = decomposition_engine(budget_config(10, 6), 0);
        let response = CompletionResponse {
            content: Vec::new(),
            tool_calls: vec![decompose_tool_call(serde_json::json!({"sub_goals": []}))],
            usage: None,
            stop_reason: None,
        };

        let error = engine.decide(&response).await.expect_err("empty sub goals");
        assert_eq!(error.stage, "decide");
        assert!(error.reason.contains("at least one sub_goal"));
    }

    #[tokio::test]
    async fn decide_rejects_malformed_decompose_arguments() {
        let mut engine = decomposition_engine(budget_config(10, 6), 0);
        let response = CompletionResponse {
            content: Vec::new(),
            tool_calls: vec![decompose_tool_call(serde_json::json!({
                "sub_goals": "not-an-array"
            }))],
            usage: None,
            stop_reason: None,
        };

        let error = engine
            .decide(&response)
            .await
            .expect_err("malformed arguments");
        assert_eq!(error.stage, "decide");
        assert!(error.reason.contains("invalid decompose tool arguments"));
    }

    #[tokio::test]
    async fn decide_rejects_unsupported_strategy() {
        let mut engine = decomposition_engine(budget_config(10, 6), 0);
        let response = CompletionResponse {
            content: Vec::new(),
            tool_calls: vec![decompose_tool_call(serde_json::json!({
                "sub_goals": [{"description": "Inspect crate configuration"}],
                "strategy": {"Custom": "fan-out"}
            }))],
            usage: None,
            stop_reason: None,
        };

        let error = engine
            .decide(&response)
            .await
            .expect_err("unsupported strategy");
        assert_eq!(error.stage, "decide");
        assert!(error.reason.contains("unsupported decomposition strategy"));
    }

    #[tokio::test]
    async fn decide_normal_tools_still_work_with_decompose_registered() {
        let mut engine = decomposition_engine(budget_config(10, 6), 0);
        let response = CompletionResponse {
            content: Vec::new(),
            tool_calls: vec![ToolCall {
                id: "regular-tool".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path": "Cargo.toml"}),
            }],
            usage: None,
            stop_reason: None,
        };

        let decision = engine.decide(&response).await.expect("decision");
        assert!(
            matches!(decision, Decision::UseTools(calls) if calls.len() == 1 && calls[0].name == "read_file")
        );
    }

    #[test]
    fn decompose_tool_definition_included_in_reasoning_request() {
        let request = build_reasoning_request(
            &sample_perception(),
            "mock-model",
            vec![sample_tool_definition()],
            None,
        );

        assert_decompose_tool_present(&request.tools);
    }

    #[test]
    fn decompose_tool_definition_included_in_continuation_request() {
        let request = build_continuation_request(
            &[Message::assistant("intermediate")],
            "mock-model",
            vec![sample_tool_definition()],
            None,
        );

        assert_decompose_tool_present(&request.tools);
    }

    #[test]
    fn tool_definitions_with_decompose_does_not_duplicate() {
        let tools = tool_definitions_with_decompose(vec![
            sample_tool_definition(),
            decompose_tool_definition(),
        ]);
        let decompose_tools = tools
            .iter()
            .filter(|tool| tool.name == DECOMPOSE_TOOL_NAME)
            .collect::<Vec<_>>();

        assert_eq!(tools.len(), 2);
        assert_eq!(decompose_tools.len(), 1);
        assert_eq!(decompose_tools[0].description, DECOMPOSE_TOOL_DESCRIPTION);
    }

    #[tokio::test]
    async fn decide_decompose_with_optional_fields() {
        let mut engine = decomposition_engine(budget_config(10, 6), 0);
        let response = CompletionResponse {
            content: Vec::new(),
            tool_calls: vec![decompose_tool_call(serde_json::json!({
                "sub_goals": [{"description": "Summarize findings"}]
            }))],
            usage: None,
            stop_reason: None,
        };

        let decision = engine.decide(&response).await.expect("decision");
        match decision {
            Decision::Decompose(plan) => {
                assert_eq!(plan.sub_goals.len(), 1);
                assert_eq!(plan.sub_goals[0].description, "Summarize findings");
                assert!(plan.sub_goals[0].required_tools.is_empty());
                assert_eq!(plan.sub_goals[0].expected_output, None);
                assert_eq!(plan.strategy, AggregationStrategy::Sequential);
            }
            other => panic!("expected decomposition decision, got: {other:?}"),
        }
    }

    fn concurrent_plan(descriptions: &[&str]) -> DecompositionPlan {
        DecompositionPlan {
            sub_goals: descriptions
                .iter()
                .map(|d| SubGoal {
                    description: (*d).to_string(),
                    required_tools: Vec::new(),
                    expected_output: Some(format!("output for {d}")),
                })
                .collect(),
            strategy: AggregationStrategy::Parallel,
            truncated_from: None,
        }
    }

    #[tokio::test]
    async fn parallel_strategy_accepted_by_decide() {
        let mut engine = decomposition_engine(budget_config(10, 6), 0);
        let response = CompletionResponse {
            content: Vec::new(),
            tool_calls: vec![decompose_tool_call(serde_json::json!({
                "sub_goals": [{"description": "Check config"}],
                "strategy": "Parallel"
            }))],
            usage: None,
            stop_reason: None,
        };
        let decision = engine.decide(&response).await.expect("decision");
        assert!(
            matches!(decision, Decision::Decompose(p) if p.strategy == AggregationStrategy::Parallel)
        );
    }

    #[tokio::test]
    async fn concurrent_execution_completes_all_sub_goals() {
        let mut engine = decomposition_engine(budget_config(20, 6), 0);
        let plan = concurrent_plan(&["first", "second", "third"]);
        let decision = Decision::Decompose(plan.clone());
        let llm = ScriptedLlm::new(vec![
            Ok(text_response("first-ok")),
            Ok(text_response("second-ok")),
            Ok(text_response("third-ok")),
        ]);
        let action = engine
            .execute_decomposition(&decision, &plan, &llm, &[])
            .await
            .expect("decomposition");
        assert!(action
            .response_text
            .contains("first => completed: first-ok"));
        assert!(action
            .response_text
            .contains("second => completed: second-ok"));
        assert!(action
            .response_text
            .contains("third => completed: third-ok"));
    }

    #[tokio::test]
    async fn concurrent_execution_absorbs_budget_from_all_children() {
        let mut engine = decomposition_engine(budget_config(20, 6), 0);
        let plan = concurrent_plan(&["a", "b"]);
        let decision = Decision::Decompose(plan.clone());
        let llm = ScriptedLlm::new(vec![
            Ok(text_response("a-done")),
            Ok(text_response("b-done")),
        ]);
        engine
            .execute_decomposition(&decision, &plan, &llm, &[])
            .await
            .expect("decomposition");
        let status = engine.status(current_time_ms());
        assert_eq!(status.llm_calls_used, 2);
    }

    #[tokio::test]
    async fn concurrent_execution_rolls_up_signals() {
        let mut engine = decomposition_engine(budget_config(20, 6), 0);
        let plan = concurrent_plan(&["sig-a", "sig-b"]);
        let decision = Decision::Decompose(plan.clone());
        let llm = ScriptedLlm::new(vec![
            Ok(text_response("a-done")),
            Ok(text_response("b-done")),
        ]);
        engine
            .execute_decomposition(&decision, &plan, &llm, &[])
            .await
            .expect("decomposition");
        assert!(engine
            .signals
            .signals()
            .iter()
            .any(|s| s.step == LoopStep::Perceive));
    }

    #[tokio::test]
    async fn concurrent_execution_handles_partial_failure() {
        let mut engine = decomposition_engine(budget_config(20, 6), 0);
        let plan = concurrent_plan(&["ok-1", "fail", "ok-2"]);
        let decision = Decision::Decompose(plan.clone());
        let llm = ScriptedLlm::new(vec![
            Ok(text_response("ok-1-done")),
            Err(ProviderError::Provider("boom".to_string())),
            Ok(text_response("ok-2-done")),
        ]);
        let action = engine
            .execute_decomposition(&decision, &plan, &llm, &[])
            .await
            .expect("decomposition");
        assert!(action
            .response_text
            .contains("ok-1 => completed: ok-1-done"));
        assert!(action.response_text.contains("fail => failed:"));
        assert!(action
            .response_text
            .contains("ok-2 => completed: ok-2-done"));
    }

    #[tokio::test]
    async fn concurrent_execution_emits_event_bus_progress() {
        let mut engine = decomposition_engine(budget_config(20, 6), 0);
        let bus = fx_core::EventBus::new(32);
        let mut rx = bus.subscribe();
        engine.set_event_bus(bus);
        let plan = concurrent_plan(&["ev-a", "ev-b"]);
        let decision = Decision::Decompose(plan.clone());
        let llm = ScriptedLlm::new(vec![Ok(text_response("a")), Ok(text_response("b"))]);
        engine
            .execute_decomposition(&decision, &plan, &llm, &[])
            .await
            .expect("decomposition");
        let mut started = 0usize;
        let mut completed = 0usize;
        while let Ok(msg) = rx.try_recv() {
            match msg {
                fx_core::message::InternalMessage::SubGoalStarted { .. } => started += 1,
                fx_core::message::InternalMessage::SubGoalCompleted { .. } => completed += 1,
                _ => {}
            }
        }
        assert_eq!(started, 2);
        assert_eq!(completed, 2);
    }

    #[tokio::test]
    async fn sequential_execution_emits_event_bus_progress() {
        let mut engine = decomposition_engine(budget_config(20, 6), 0);
        let bus = fx_core::EventBus::new(32);
        let mut rx = bus.subscribe();
        engine.set_event_bus(bus);
        let plan = decomposition_plan(&["seq-a", "seq-b"]);
        let decision = Decision::Decompose(plan.clone());
        let llm = ScriptedLlm::new(vec![Ok(text_response("a")), Ok(text_response("b"))]);
        engine
            .execute_decomposition(&decision, &plan, &llm, &[])
            .await
            .expect("decomposition");
        let mut started = 0usize;
        let mut completed = 0usize;
        while let Ok(msg) = rx.try_recv() {
            match msg {
                fx_core::message::InternalMessage::SubGoalStarted { .. } => started += 1,
                fx_core::message::InternalMessage::SubGoalCompleted { .. } => completed += 1,
                _ => {}
            }
        }
        assert_eq!(started, 2);
        assert_eq!(completed, 2);
    }

    #[tokio::test]
    async fn concurrent_execution_with_empty_plan_returns_empty_results() {
        let mut engine = decomposition_engine(budget_config(20, 6), 0);
        let plan = DecompositionPlan {
            sub_goals: Vec::new(),
            strategy: AggregationStrategy::Parallel,
            truncated_from: None,
        };
        let llm = ScriptedLlm::new(vec![]);

        let results = engine.execute_sub_goals_concurrent(&plan, &llm, &[]).await;

        assert!(results.is_empty());
    }
}
