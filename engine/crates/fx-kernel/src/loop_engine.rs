//! Agentic loop orchestrator.

use crate::act::{ActionResult, TokenUsage, ToolExecutor, ToolResult};
use crate::budget::{ActionCost, BudgetRemaining, BudgetTracker};
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
use fx_core::types::{InputSource, UserInput};
use fx_llm::{
    CompletionRequest, CompletionResponse, ContentBlock, Message, MessageRole, ProviderError,
    ToolCall, ToolDefinition,
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

const REASONING_OUTPUT_TOKEN_HEURISTIC: u64 = 192;
const TOOL_SYNTHESIS_TOKEN_HEURISTIC: u64 = 320;
const REASONING_MAX_OUTPUT_TOKENS: u32 = 768;
const REASONING_TEMPERATURE: f32 = 0.2;
const TOOL_SYNTHESIS_MAX_OUTPUT_TOKENS: u32 = 384;
const DEFAULT_LLM_ACTION_COST_CENTS: u64 = 2;
const SAFE_FALLBACK_RESPONSE: &str = "I wasn't able to process that. Could you try rephrasing?";
const REASONING_SYSTEM_PROMPT: &str = "You are Fawx, a capable personal assistant. \
Answer the user directly and concisely. \
Never introduce yourself, greet the user, or add preamble — just answer. \
Use tools when you need information not already in the conversation \
(current time, file contents, directory listings, search results, memory, etc.). \
After using tools, respond with the answer — never narrate what tools you used, \
describe the process, or comment on tool output metadata. \
Never narrate your process, hedge with qualifiers, or reference tool mechanics. \
Avoid filler openers like \"I notice\", \"I can see that\", \"Based on the results\", \
\"It appears that\", \"Let me\", or \"I aim to\". Just answer the question. \
If the user makes a statement (not a question), acknowledge it naturally and briefly. \
If a tool call stores data (like memory_write), confirm the action in one short sentence.";

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
        let signals = self.signals.drain_all();
        attach_signals(result, signals)
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
        let started = current_time_ms();
        let response = llm
            .complete(request)
            .await
            .map_err(|error| loop_error("reason", &format!("completion failed: {error}"), true))?;
        let latency_ms = current_time_ms().saturating_sub(started);
        let usage = response.usage;
        self.emit_reason_trace_and_perf(latency_ms, usage.as_ref());
        Ok(response)
    }

    /// Decide step.
    async fn decide(&mut self, response: &CompletionResponse) -> Result<Decision, LoopError> {
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

        let continuation = Continuation::Continue(
            "The previous attempt produced no response. Try a different approach to answer the user's question."
                .to_string(),
        );
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

fn decision_variant(decision: &Decision) -> &'static str {
    match decision {
        Decision::Respond(_) => "Respond",
        Decision::UseTools(_) => "UseTools",
        Decision::Clarify(_) => "Clarify",
        Decision::Defer(_) => "Defer",
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

    format!("You are Fawx. Answer the user's question using these tool results. \
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

/// Build a user message containing ToolResult content blocks.
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
        role: MessageRole::User,
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

/// Build a CompletionRequest for tool result re-prompting.
fn build_continuation_request(
    context_messages: &[Message],
    model: &str,
    tool_definitions: Vec<ToolDefinition>,
    memory_context: Option<&str>,
) -> CompletionRequest {
    let system_prompt = build_reasoning_system_prompt(&tool_definitions, memory_context);
    CompletionRequest {
        model: model.to_string(),
        messages: context_messages.to_vec(),
        tools: tool_definitions,
        temperature: Some(REASONING_TEMPERATURE),
        max_tokens: Some(REASONING_MAX_OUTPUT_TOKENS),
        system_prompt: Some(system_prompt),
    }
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
    let system_prompt = build_reasoning_system_prompt(&tool_definitions, memory_context);

    CompletionRequest {
        model: model.to_string(),
        messages: [context, vec![Message::user(user_prompt)]].concat(),
        tools: tool_definitions,
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

    fn failing_tool_engine() -> LoopEngine {
        LoopEngine::new(
            BudgetTracker::new(crate::budget::BudgetConfig::default(), 0, 0),
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

        assert_eq!(message.role, fx_llm::MessageRole::User);
        assert_eq!(message.content.len(), 2);
        assert_tool_result_block(&message.content[0], "call-1", "ok");
        assert_tool_result_block(&message.content[1], "call-2", "[ERROR] permission denied");
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
