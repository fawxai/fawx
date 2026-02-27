//! Agentic loop orchestrator.

use crate::act::{ActionResult, StubToolExecutor, TokenUsage, ToolExecutor, ToolResult};
use crate::budget::{ActionCost, BudgetRemaining, BudgetTracker};
use crate::context_manager::ContextCompactor;
use crate::continuation::Continuation;
use crate::decide::{decide_from_intent, Decision, CONFIDENCE_CLARIFY_THRESHOLD};
use crate::learn::Learning;
use crate::perceive::{ProcessedPerception, TrimmingPolicy};
use crate::types::{
    Goal, IdentityContext, IntendedAction, LoopError, PerceptionSnapshot, ReasonedIntent,
    ReasoningContext, WorkingMemoryEntry,
};
use crate::verify::Verification;
use async_trait::async_trait;
use fx_core::types::{InputSource, UserInput};
use fx_llm::{
    CompletionRequest, CompletionResponse, Message, ProviderError, ToolCall, ToolDefinition,
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
    },
    /// Loop exited because budget limits were reached.
    BudgetExhausted {
        /// Optional best-effort partial response text.
        partial_response: Option<String>,
        /// Iterations completed before exhaustion.
        iterations: u32,
    },
    /// Loop requires additional user input.
    NeedsInput {
        /// Prompt to present to user.
        prompt: String,
        /// Iterations completed before requesting input.
        iterations: u32,
    },
    /// Loop ended with a recoverable or non-recoverable runtime error.
    Error {
        /// Error message to surface to the caller.
        message: String,
        /// Whether retrying may succeed.
        recoverable: bool,
    },
}

/// Core orchestrator for the 7-step agentic loop.
#[derive(Debug, Clone)]
pub struct LoopEngine {
    budget: BudgetTracker,
    context: ContextCompactor,
    tool_executor: Arc<dyn ToolExecutor>,
    max_iterations: u32,
    iteration_count: u32,
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

const REASONING_OUTPUT_TOKEN_HEURISTIC: u64 = 192;
const TOOL_SYNTHESIS_TOKEN_HEURISTIC: u64 = 320;
const REASONING_MAX_OUTPUT_TOKENS: u32 = 768;
const TOOL_SYNTHESIS_MAX_OUTPUT_TOKENS: u32 = 384;
const DEFAULT_LLM_ACTION_COST_CENTS: u64 = 2;
const SAFE_FALLBACK_RESPONSE: &str = "I wasn't able to process that. Could you try rephrasing?";
const REASONING_SYSTEM_PROMPT: &str = "You are Fawx, an autonomous assistant. \
Always use the emit_intent tool to respond. \
The action field must deserialize to IntendedAction and use exactly one of these variants: \
Respond, Tap, Type, Swipe, LaunchApp, Navigate, Wait, Delegate, or Composite. \
For TUI-first conversations, prefer Respond for direct user answers and Delegate for tool use.";

const VERIFICATION_CONFIDENCE_CLEAN: f64 = 0.9;
const VERIFICATION_CONFIDENCE_SINGLE_DISCREPANCY: f64 = 0.45;
const VERIFICATION_CONFIDENCE_MULTIPLE_DISCREPANCIES: f64 = 0.25;

impl LoopEngine {
    /// Create a new loop engine with budget + context managers.
    pub fn new(budget: BudgetTracker, context: ContextCompactor, max_iterations: u32) -> Self {
        Self::new_with_executor(budget, context, max_iterations, Arc::new(StubToolExecutor))
    }

    /// Create a loop engine with an injected tool executor.
    pub fn new_with_executor(
        budget: BudgetTracker,
        context: ContextCompactor,
        max_iterations: u32,
        tool_executor: Arc<dyn ToolExecutor>,
    ) -> Self {
        Self {
            budget,
            context,
            tool_executor,
            max_iterations: max_iterations.max(1),
            iteration_count: 0,
        }
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
                IterationStep::Terminal(result) => return Ok(result),
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
                        return Ok(result);
                    }
                }
            }
        }

        Ok(LoopResult::Error {
            message: format!(
                "Loop reached safety limit of {} iterations without completion.",
                self.max_iterations
            ),
            recoverable: true,
        })
    }

    fn prepare_cycle(&mut self) {
        self.iteration_count = 0;
        self.budget.reset(current_time_ms());
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

        let processed = self.perceive(perception).await?;
        let reason_cost = self.estimate_reasoning_cost(&processed);
        if let Some(step) = self.budget_terminal(reason_cost, state.partial_response.clone()) {
            return Ok(step);
        }

        let intent = self.reason(&processed, llm).await?;
        self.record_reasoning_cost(reason_cost, state);

        let decision = self.decide(&intent).await?;
        if let Some(step) = self.budget_terminal(
            self.estimate_action_cost(&decision),
            state.partial_response.clone(),
        ) {
            return Ok(step);
        }

        self.execute_action_and_finalize(&intent, &decision, llm, state)
            .await
    }

    async fn execute_action_and_finalize(
        &mut self,
        intent: &ReasonedIntent,
        decision: &Decision,
        llm: &dyn LlmProvider,
        state: &mut CycleState,
    ) -> Result<IterationStep, LoopError> {
        let action = self.act(decision, llm).await?;
        let action_cost = self.action_cost_from_result(&action);
        if let Some(step) = self.budget_terminal(action_cost, Some(action.response_text.clone())) {
            return Ok(step);
        }

        self.budget.record(&action_cost);
        state.tokens.accumulate(action.tokens_used);
        state.partial_response = Some(action.response_text.clone());

        let verification = self.verify(&action, intent).await?;
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
        &self,
        cost: ActionCost,
        partial_response: Option<String>,
    ) -> Option<IterationStep> {
        self.handle_budget_check(cost, partial_response)
            .map(IterationStep::Terminal)
    }

    fn handle_budget_check(
        &self,
        cost: ActionCost,
        partial_response: Option<String>,
    ) -> Option<LoopResult> {
        if self.budget.check_at(current_time_ms(), &cost).is_ok() {
            return None;
        }

        Some(LoopResult::BudgetExhausted {
            partial_response,
            iterations: self.iteration_count,
        })
    }

    fn handle_continuation(
        &self,
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
            }),
            Continuation::NeedsInput(prompt) => Some(LoopResult::NeedsInput {
                prompt,
                iterations: self.iteration_count,
            }),
            Continuation::Continue(sub_goal) => {
                *perception = next_perception_from_sub_goal(perception, &sub_goal);
                None
            }
        }
    }

    /// Perceive step.
    async fn perceive(
        &self,
        snapshot: &PerceptionSnapshot,
    ) -> Result<ProcessedPerception, LoopError> {
        let user_message = extract_user_message(snapshot)?;
        let mut context_window = vec![Message::user(user_message.clone())];

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
        &self,
        perception: &ProcessedPerception,
        llm: &dyn LlmProvider,
    ) -> Result<ReasonedIntent, LoopError> {
        let request = build_reasoning_request(perception, llm.model_name());
        let response = llm
            .complete(request)
            .await
            .map_err(|error| loop_error("reason", &format!("completion failed: {error}"), true))?;

        if let Some(intent) = parse_tool_call_intent(&response) {
            return Ok(intent);
        }

        eprintln!("reason: falling back to text parsing (emit_intent tool call missing/invalid)");
        let raw = extract_response_text(&response);
        Ok(parse_reasoned_intent(&raw, &perception.user_message))
    }

    /// Decide step.
    async fn decide(&self, intent: &ReasonedIntent) -> Result<Decision, LoopError> {
        decide_from_intent(intent).map_err(|error| {
            loop_error(
                "decide",
                &format!("intent-to-decision mapping failed: {error}"),
                true,
            )
        })
    }

    /// Act step.
    async fn act(
        &self,
        decision: &Decision,
        llm: &dyn LlmProvider,
    ) -> Result<ActionResult, LoopError> {
        match decision {
            Decision::Respond(text) | Decision::Clarify(text) | Decision::Defer(text) => {
                Ok(self.text_action_result(decision, text))
            }
            Decision::UseTools(calls) => self.act_with_tools(decision, calls, llm).await,
        }
    }

    /// Verify step.
    async fn verify(
        &self,
        action: &ActionResult,
        intent: &ReasonedIntent,
    ) -> Result<Verification, LoopError> {
        let mut discrepancies = Vec::new();

        if !action_matches_intent(action, intent) {
            discrepancies.push("action response does not align with intent action".to_string());
        }

        if let Some(mismatch) = expected_outcome_mismatch(action, intent) {
            discrepancies.push(mismatch);
        }

        Ok(build_verification(discrepancies))
    }

    /// Learn step.
    async fn learn(&mut self, verification: &Verification) -> Result<Learning, LoopError> {
        let episode = if verification.outcome_matches_intent {
            "Action matched intent and verification passed.".to_string()
        } else {
            format!(
                "Verification found discrepancies: {}",
                verification.discrepancies.join("; ")
            )
        };

        let pattern = if verification.outcome_matches_intent {
            None
        } else {
            Some("mismatch_between_intended_and_observed_outcome".to_string())
        };

        let adjustment = if verification.outcome_matches_intent {
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
        &self,
        decision: &Decision,
        verification: &Verification,
        _learning: &Learning,
    ) -> Result<Continuation, LoopError> {
        if let Decision::Clarify(prompt) | Decision::Defer(prompt) = decision {
            return Ok(Continuation::NeedsInput(prompt.clone()));
        }

        if verification.outcome_matches_intent {
            return Ok(Continuation::Complete);
        }

        if verification.confidence < CONFIDENCE_CLARIFY_THRESHOLD {
            return Ok(Continuation::NeedsInput(
                "I need a bit more detail to continue safely. Could you clarify your goal?"
                    .to_string(),
            ));
        }

        Ok(Continuation::Continue(
            "Refine the response using tighter intent alignment.".to_string(),
        ))
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

    async fn act_with_tools(
        &self,
        decision: &Decision,
        calls: &[fx_llm::ToolCall],
        llm: &dyn LlmProvider,
    ) -> Result<ActionResult, LoopError> {
        let tool_results = self
            .tool_executor
            .execute_tools(calls)
            .await
            .map_err(|error| {
                loop_error(
                    "act",
                    &format!("tool execution failed: {}", error.message),
                    error.recoverable,
                )
            })?;
        let synthesis_prompt = tool_synthesis_prompt(&tool_results);
        let llm_text = self.generate_tool_summary(&synthesis_prompt, llm).await?;
        let usage = synthesis_usage(&synthesis_prompt, &llm_text);

        Ok(ActionResult {
            decision: decision.clone(),
            tool_results,
            response_text: ensure_non_empty_response(&llm_text),
            tokens_used: usage,
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

fn tool_synthesis_prompt(tool_results: &[ToolResult]) -> String {
    let tool_summary = tool_results
        .iter()
        .map(|result| format!("- {}: {}", result.tool_name, result.output))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "You are Fawx. Summarize the following tool activity for the user in concise text:\n{tool_summary}\n\nTool outputs are authoritative; summarize clearly and concisely."
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

fn action_matches_intent(action: &ActionResult, intent: &ReasonedIntent) -> bool {
    match &intent.action {
        IntendedAction::Respond { text } => {
            let expected = text.trim().to_ascii_lowercase();
            let actual = action.response_text.trim().to_ascii_lowercase();
            actual == expected || actual.contains(&expected)
        }
        IntendedAction::Delegate { .. }
        | IntendedAction::Tap { .. }
        | IntendedAction::Type { .. }
        | IntendedAction::Swipe { .. }
        | IntendedAction::LaunchApp { .. }
        | IntendedAction::Navigate { .. }
        | IntendedAction::Wait { .. }
        | IntendedAction::Composite(_) => !action.response_text.trim().is_empty(),
    }
}

fn expected_outcome_mismatch(action: &ActionResult, intent: &ReasonedIntent) -> Option<String> {
    let expected = intent.expected_outcome.as_ref()?;
    let expected_text = expected.description.to_ascii_lowercase();
    let response_text = action.response_text.to_ascii_lowercase();

    if response_text.contains(&expected_text) {
        return None;
    }

    Some(format!(
        "expected outcome not reflected in action response: {}",
        expected.description
    ))
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
        outcome_matches_intent: discrepancies.is_empty(),
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
        .map(|prompt| format!("System:\n{prompt}\n\n"))
        .unwrap_or_default();
    let messages = request
        .messages
        .iter()
        .map(message_to_text)
        .collect::<Vec<_>>()
        .join("\n");

    format!("{system}{messages}")
}

fn build_reasoning_request(perception: &ProcessedPerception, model: &str) -> CompletionRequest {
    let context = perception.context_window.clone();
    let user_prompt = reasoning_user_prompt(perception);

    CompletionRequest {
        model: model.to_string(),
        messages: [context, vec![Message::user(user_prompt)]].concat(),
        tools: vec![emit_intent_tool_definition()],
        temperature: Some(0.2),
        max_tokens: Some(REASONING_MAX_OUTPUT_TOKENS),
        system_prompt: Some(REASONING_SYSTEM_PROMPT.to_string()),
    }
}

fn reasoning_user_prompt(perception: &ProcessedPerception) -> String {
    format!(
        "Active goals:\n- {}\n\nBudget remaining: {} tokens, {} llm calls\n\nUser message:\n{}",
        perception.active_goals.join("\n- "),
        perception.budget_remaining.tokens,
        perception.budget_remaining.llm_calls,
        perception.user_message,
    )
}

fn emit_intent_tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: "emit_intent".to_string(),
        description: "Emit a structured intent. ALWAYS call this tool with your response."
            .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "object",
                    "description": "IntendedAction variant object. Allowed variants: Respond, Tap, Type, Swipe, LaunchApp, Navigate, Wait, Delegate, Composite. Examples: {\"Respond\": {\"text\": \"your answer\"}} or {\"Delegate\": {\"skill_id\": \"search\", \"params\": {\"q\": \"rust async\"}}}."
                },
                "rationale": {"type": "string"},
                "confidence": {"type": "number"},
                "expected_outcome": {"type": "string"},
                "sub_goals": {"type": "array", "items": {"type": "string"}}
            },
            "required": ["action", "rationale", "confidence"]
        }),
    }
}

fn parse_tool_call_intent(response: &CompletionResponse) -> Option<ReasonedIntent> {
    response
        .tool_calls
        .iter()
        .find(|call| call.name == "emit_intent")
        .and_then(parse_emit_intent_call)
}

fn parse_emit_intent_call(call: &ToolCall) -> Option<ReasonedIntent> {
    let args = call.arguments.as_object()?;
    let action = parse_tool_action(args.get("action")?)?;
    let rationale = args.get("rationale")?.as_str()?.to_string();
    let confidence = args.get("confidence")?.as_f64()? as f32;
    let expected_outcome = parse_expected_outcome(args.get("expected_outcome"));
    let sub_goals = parse_sub_goals(args.get("sub_goals"));

    Some(sanitize_intent(ReasonedIntent {
        action,
        rationale,
        confidence,
        expected_outcome,
        sub_goals,
    }))
}

fn parse_tool_action(value: &serde_json::Value) -> Option<IntendedAction> {
    if let Ok(action) = serde_json::from_value::<IntendedAction>(value.clone()) {
        return Some(action);
    }

    parse_tool_use_action(value)
}

fn parse_tool_use_action(value: &serde_json::Value) -> Option<IntendedAction> {
    let calls = value.get("UseTools")?.get("tool_calls")?.as_array()?;
    let delegates = calls
        .iter()
        .map(delegate_from_tool_call)
        .collect::<Option<Vec<_>>>()?;
    Some(IntendedAction::Composite(delegates))
}

fn delegate_from_tool_call(call: &serde_json::Value) -> Option<IntendedAction> {
    let tool = call.get("tool")?.as_str()?.to_string();
    let args = call
        .get("args")
        .and_then(serde_json::Value::as_object)
        .cloned()
        .unwrap_or_default();
    let params = args
        .into_iter()
        .map(|(key, value)| (key, value_to_param_string(value)))
        .collect::<HashMap<_, _>>();

    Some(IntendedAction::Delegate {
        skill_id: tool,
        params,
    })
}

fn value_to_param_string(value: serde_json::Value) -> String {
    value
        .as_str()
        .map(ToString::to_string)
        .unwrap_or_else(|| value.to_string())
}

fn parse_expected_outcome(
    value: Option<&serde_json::Value>,
) -> Option<crate::types::ExpectedOutcome> {
    value
        .and_then(serde_json::Value::as_str)
        .map(|description| crate::types::ExpectedOutcome {
            description: description.to_string(),
            artifact_checks: Vec::new(),
        })
}

fn parse_sub_goals(value: Option<&serde_json::Value>) -> Vec<Goal> {
    value
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(|text| Goal::new(text, vec!["Complete the sub-goal".to_string()], Some(1)))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
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

fn parse_reasoned_intent(raw: &str, fallback_user_message: &str) -> ReasonedIntent {
    parse_reasoned_intent_inner(raw)
        .unwrap_or_else(|| build_fallback_intent(raw, fallback_user_message))
}

fn build_fallback_intent(raw: &str, fallback_user_message: &str) -> ReasonedIntent {
    let text = ensure_non_empty_response(&extract_fallback_text(raw));
    ReasonedIntent {
        action: IntendedAction::Respond { text },
        rationale: "Fallback: model output did not match schema".to_string(),
        confidence: 0.4,
        expected_outcome: None,
        sub_goals: vec![Goal::new(
            format!("Respond clearly to: {fallback_user_message}"),
            vec!["User receives a relevant response".to_string()],
            Some(1),
        )],
    }
}

fn extract_fallback_text(raw: &str) -> String {
    // Try to pull a readable response from malformed JSON
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) {
        // Look for text in common response-like fields
        let candidates = ["text", "response", "answer", "message", "rationale"];
        if let Some(text) = find_text_in_json(&value, &candidates) {
            return text;
        }
    }
    // Strip markdown fences and return as-is
    raw.trim()
        .strip_prefix("```json")
        .unwrap_or(raw.trim())
        .trim_matches('`')
        .trim()
        .to_string()
}

fn find_text_in_json(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    match value {
        serde_json::Value::Object(map) => {
            for key in keys {
                if let Some(serde_json::Value::String(s)) = map.get(*key) {
                    if !s.is_empty() {
                        return Some(s.clone());
                    }
                }
            }
            // Recurse into nested values.
            for v in map.values() {
                if let Some(found) = find_text_in_json(v, keys) {
                    return Some(found);
                }
            }
            None
        }
        serde_json::Value::Array(values) => {
            for value in values {
                if let Some(found) = find_text_in_json(value, keys) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

fn parse_reasoned_intent_inner(raw: &str) -> Option<ReasonedIntent> {
    #[derive(Debug, Deserialize)]
    struct Envelope {
        intent: Option<ReasonedIntent>,
        intents: Option<Vec<ReasonedIntent>>,
    }

    if let Ok(intent) = serde_json::from_str::<ReasonedIntent>(raw) {
        return Some(sanitize_intent(intent));
    }

    if let Ok(envelope) = serde_json::from_str::<Envelope>(raw) {
        if let Some(intent) = envelope.intent {
            return Some(sanitize_intent(intent));
        }

        if let Some(mut intents) = envelope.intents {
            if let Some(intent) = intents.pop() {
                return Some(sanitize_intent(intent));
            }
        }
    }

    let json = extract_json_candidate(raw)?;
    if json.trim() == raw.trim() {
        return None;
    }

    parse_reasoned_intent_inner(&json)
}

fn extract_json_candidate(raw: &str) -> Option<String> {
    let fenced = raw
        .rfind("```")
        .and_then(|end| raw[..end].rfind("```").map(|start| (start, end)))
        .map(|(start, end)| {
            raw[start + 3..end]
                .trim_start_matches("json")
                .trim()
                .to_string()
        })
        .filter(|payload| !payload.is_empty());

    if fenced.is_some() {
        return fenced;
    }

    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    if end <= start {
        return None;
    }

    Some(raw[start..=end].to_string())
}

fn sanitize_intent(mut intent: ReasonedIntent) -> ReasonedIntent {
    if !intent.confidence.is_finite() {
        intent.confidence = 0.5;
    }

    intent.confidence = intent.confidence.clamp(0.0, 1.0);
    if let IntendedAction::Respond { text } = &mut intent.action {
        *text = ensure_non_empty_response(text);
    }
    intent
}

fn ensure_non_empty_response(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return SAFE_FALLBACK_RESPONSE.to_string();
    }
    trimmed.to_string()
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
    use fx_core::types::ScreenState;
    use fx_llm::{CompletionResponse, ContentBlock, ProviderError};
    use std::collections::VecDeque;
    use std::sync::Mutex;

    #[derive(Debug)]
    struct MockLlm {
        name: String,
        responses: Mutex<VecDeque<String>>,
        completion_responses: Mutex<VecDeque<CompletionResponse>>,
        captured_requests: Mutex<Vec<CompletionRequest>>,
    }

    impl MockLlm {
        fn with_responses(responses: Vec<&str>) -> Self {
            Self {
                name: "mock-llm".to_string(),
                responses: Mutex::new(
                    responses
                        .into_iter()
                        .map(ToString::to_string)
                        .collect::<VecDeque<_>>(),
                ),
                completion_responses: Mutex::new(VecDeque::new()),
                captured_requests: Mutex::new(Vec::new()),
            }
        }

        fn with_completion_responses(responses: Vec<CompletionResponse>) -> Self {
            Self {
                name: "mock-llm".to_string(),
                responses: Mutex::new(VecDeque::new()),
                completion_responses: Mutex::new(responses.into_iter().collect()),
                captured_requests: Mutex::new(Vec::new()),
            }
        }

        fn capture_count(&self) -> usize {
            self.captured_requests
                .lock()
                .expect("request capture lock")
                .len()
        }

        fn captured_request(&self, index: usize) -> CompletionRequest {
            self.captured_requests.lock().expect("request capture lock")[index].clone()
        }

        fn pop_text_response(&self) -> Result<String, CoreLlmError> {
            self.responses
                .lock()
                .expect("responses lock")
                .pop_front()
                .ok_or_else(|| CoreLlmError::Inference("no mock response available".to_string()))
        }
    }

    /// Mock LLM that introduces latency to test wall-time budgets.
    #[derive(Debug)]
    struct SlowMockLlm {
        inner: MockLlm,
        delay_ms: u64,
    }

    #[derive(Debug)]
    struct ChunkedMockLlm {
        chunks: Vec<String>,
        fallback: String,
    }

    #[async_trait]
    impl LlmProvider for SlowMockLlm {
        async fn generate(&self, prompt: &str, max_tokens: u32) -> Result<String, CoreLlmError> {
            tokio::time::sleep(std::time::Duration::from_millis(self.delay_ms)).await;
            self.inner.generate(prompt, max_tokens).await
        }

        async fn generate_streaming(
            &self,
            prompt: &str,
            max_tokens: u32,
            callback: Box<dyn Fn(String) + Send + 'static>,
        ) -> Result<String, CoreLlmError> {
            tokio::time::sleep(std::time::Duration::from_millis(self.delay_ms)).await;
            self.inner
                .generate_streaming(prompt, max_tokens, callback)
                .await
        }

        async fn complete(
            &self,
            request: CompletionRequest,
        ) -> Result<CompletionResponse, ProviderError> {
            tokio::time::sleep(std::time::Duration::from_millis(self.delay_ms)).await;
            self.inner.complete(request).await
        }

        fn model_name(&self) -> &str {
            self.inner.model_name()
        }
    }

    #[async_trait]
    impl LlmProvider for MockLlm {
        async fn generate(&self, _prompt: &str, _max_tokens: u32) -> Result<String, CoreLlmError> {
            self.pop_text_response()
        }

        async fn generate_streaming(
            &self,
            prompt: &str,
            max_tokens: u32,
            callback: Box<dyn Fn(String) + Send + 'static>,
        ) -> Result<String, CoreLlmError> {
            let text = self.generate(prompt, max_tokens).await?;
            callback(text.clone());
            Ok(text)
        }

        async fn complete(
            &self,
            request: CompletionRequest,
        ) -> Result<CompletionResponse, ProviderError> {
            self.captured_requests
                .lock()
                .expect("request capture lock")
                .push(request);

            if let Some(response) = self
                .completion_responses
                .lock()
                .expect("completion responses lock")
                .pop_front()
            {
                return Ok(response);
            }

            let text = self.pop_text_response().map_err(|error| {
                ProviderError::Provider(format!("mock completion fallback failed: {error}"))
            })?;

            Ok(CompletionResponse {
                content: vec![ContentBlock::Text { text }],
                tool_calls: Vec::new(),
                usage: None,
                stop_reason: None,
            })
        }

        fn model_name(&self) -> &str {
            &self.name
        }
    }

    #[async_trait]
    impl LlmProvider for ChunkedMockLlm {
        async fn generate(&self, _prompt: &str, _max_tokens: u32) -> Result<String, CoreLlmError> {
            Ok(self.fallback.clone())
        }

        async fn generate_streaming(
            &self,
            _prompt: &str,
            _max_tokens: u32,
            callback: Box<dyn Fn(String) + Send + 'static>,
        ) -> Result<String, CoreLlmError> {
            for chunk in &self.chunks {
                callback(chunk.clone());
            }
            Ok(self.fallback.clone())
        }

        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionResponse, ProviderError> {
            Ok(CompletionResponse {
                content: vec![ContentBlock::Text {
                    text: self.fallback.clone(),
                }],
                tool_calls: Vec::new(),
                usage: None,
                stop_reason: None,
            })
        }

        fn model_name(&self) -> &str {
            "chunked-mock"
        }
    }

    fn base_snapshot(text: &str) -> PerceptionSnapshot {
        let timestamp_ms = current_time_ms();
        PerceptionSnapshot {
            screen: ScreenState {
                current_app: "fawx.tui".to_string(),
                elements: Vec::new(),
                text_content: text.to_string(),
            },
            notifications: Vec::new(),
            active_app: "fawx.tui".to_string(),
            timestamp_ms,
            sensor_data: None,
            user_input: Some(UserInput {
                text: text.to_string(),
                source: InputSource::Text,
                timestamp: timestamp_ms,
                context_id: None,
            }),
        }
    }

    fn default_engine(max_iterations: u32) -> LoopEngine {
        let budget =
            BudgetTracker::new(crate::budget::BudgetConfig::default(), current_time_ms(), 0);
        let context = ContextCompactor::new(4_000, 3_000);
        LoopEngine::new(budget, context, max_iterations)
    }

    #[test]
    fn status_returns_initial_metrics_before_any_cycle() {
        let max_iterations = 10;
        let engine = default_engine(max_iterations);

        let status = engine.status(current_time_ms());

        assert_eq!(status.iteration_count, 0);
        assert_eq!(status.max_iterations, max_iterations);
        assert_eq!(status.llm_calls_used, 0);
        assert_eq!(status.tool_invocations_used, 0);
        assert_eq!(status.tokens_used, 0);
        assert_eq!(status.cost_cents_used, 0);
    }

    #[tokio::test]
    async fn run_cycle_returns_complete_with_mock_llm() {
        let mut engine = default_engine(10);
        let llm = MockLlm::with_responses(vec![
            r#"{"action":{"Respond":{"text":"Hello from Fawx"}},"rationale":"reply","confidence":0.92,"expected_outcome":null,"sub_goals":[]}"#,
        ]);

        let result = engine
            .run_cycle(base_snapshot("Hi there"), &llm)
            .await
            .expect("loop result");

        assert!(matches!(
            result,
            LoopResult::Complete {
                response,
                iterations: _,
                ..
            } if response == "Hello from Fawx"
        ));
    }

    #[tokio::test]
    async fn run_cycle_respects_max_iterations_limit() {
        let mut engine = default_engine(2);
        let llm = MockLlm::with_responses(vec![
            r#"{"action":{"Delegate":{"skill_id":"search","params":{"q":"docs"}}},"rationale":"need tools","confidence":0.8,"expected_outcome":{"description":"MAGIC_TOKEN","artifact_checks":[]},"sub_goals":[]}"#,
            "tool pass one",
            r#"{"action":{"Delegate":{"skill_id":"search","params":{"q":"docs"}}},"rationale":"need tools","confidence":0.8,"expected_outcome":{"description":"MAGIC_TOKEN","artifact_checks":[]},"sub_goals":[]}"#,
            "tool pass two",
        ]);

        let result = engine
            .run_cycle(base_snapshot("loop forever"), &llm)
            .await
            .expect("loop result");

        assert!(matches!(
            result,
            LoopResult::Error {
                message,
                recoverable: true
            } if message.contains("safety limit")
        ));
    }

    #[tokio::test]
    async fn run_cycle_returns_budget_exhausted_when_tracker_blocks_reasoning() {
        let config = crate::budget::BudgetConfig {
            max_llm_calls: 1,
            max_tool_invocations: 1,
            max_tokens: 1,
            max_cost_cents: 1,
            max_wall_time_ms: 10_000,
            max_recursion_depth: 2,
        };
        let budget = BudgetTracker::new(config, current_time_ms(), 0);
        let context = ContextCompactor::new(4_000, 3_000);
        let mut engine = LoopEngine::new(budget, context, 10);

        let llm = MockLlm::with_responses(vec![
            r#"{"action":{"Respond":{"text":"never used"}},"rationale":"n/a","confidence":0.9,"expected_outcome":null,"sub_goals":[]}"#,
        ]);

        // Ensure wall time elapses during the cycle
        let result = engine
            .run_cycle(base_snapshot("hi"), &llm)
            .await
            .expect("loop result");

        assert!(
            matches!(result, LoopResult::BudgetExhausted { .. }),
            "expected BudgetExhausted but got: {result:?}"
        );
    }

    #[tokio::test]
    async fn run_cycle_enforces_wall_time_budget() {
        let mut config = crate::budget::BudgetConfig::unlimited();
        config.max_wall_time_ms = 1;

        let budget = BudgetTracker::new(config, current_time_ms(), 0);
        let context = ContextCompactor::new(4_000, 3_000);
        let mut engine = LoopEngine::new(budget, context, 10);

        let llm = SlowMockLlm {
            inner: MockLlm::with_responses(vec![
                r#"{"action":{"Respond":{"text":"never used"}},"rationale":"n/a","confidence":0.9,"expected_outcome":null,"sub_goals":[]}"#,
            ]),
            delay_ms: 10,
        };

        // Ensure wall time elapses during the cycle
        let result = engine
            .run_cycle(base_snapshot("hi"), &llm)
            .await
            .expect("loop result");

        assert!(
            matches!(result, LoopResult::BudgetExhausted { .. }),
            "expected BudgetExhausted but got: {result:?}"
        );
    }

    #[tokio::test]
    async fn run_cycle_resets_budget_tracker_between_messages() {
        let mut config = crate::budget::BudgetConfig::unlimited();
        config.max_llm_calls = 1;

        let budget = BudgetTracker::new(config, current_time_ms(), 0);
        let context = ContextCompactor::new(4_000, 3_000);
        let mut engine = LoopEngine::new(budget, context, 5);

        let llm = MockLlm::with_responses(vec![
            r#"{"action":{"Respond":{"text":"first"}},"rationale":"r1","confidence":0.9,"expected_outcome":null,"sub_goals":[]}"#,
            r#"{"action":{"Respond":{"text":"second"}},"rationale":"r2","confidence":0.9,"expected_outcome":null,"sub_goals":[]}"#,
        ]);

        let first = engine
            .run_cycle(base_snapshot("one"), &llm)
            .await
            .expect("first run result");
        let second = engine
            .run_cycle(base_snapshot("two"), &llm)
            .await
            .expect("second run result");

        assert!(matches!(first, LoopResult::Complete { response, .. } if response == "first"));
        assert!(matches!(second, LoopResult::Complete { response, .. } if response == "second"));
    }

    #[tokio::test]
    async fn run_cycle_propagates_act_synthesis_generation_error() {
        let mut engine = default_engine(5);
        let llm = MockLlm::with_responses(vec![
            r#"{"action":{"Delegate":{"skill_id":"search","params":{"q":"docs"}}},"rationale":"need tools","confidence":0.8,"expected_outcome":null,"sub_goals":[]}"#,
        ]);

        let result = engine.run_cycle(base_snapshot("trigger tools"), &llm).await;

        assert!(
            matches!(result, Err(error) if error.stage == "act" && error.reason.contains("tool synthesis generation failed"))
        );
    }

    #[tokio::test]
    async fn step_perceive_produces_processed_perception() {
        let engine = default_engine(3);
        let processed = engine
            .perceive(&base_snapshot("What time is it?"))
            .await
            .expect("processed perception");

        assert_eq!(processed.user_message, "What time is it?");
        assert!(!processed.context_window.is_empty());
        assert!(!processed.active_goals.is_empty());
    }

    #[tokio::test]
    async fn step_reason_parses_reasoned_intent() {
        let engine = default_engine(3);
        let llm = MockLlm::with_responses(vec![
            r#"{"action":{"Respond":{"text":"Sure"}},"rationale":"direct reply","confidence":0.7,"expected_outcome":null,"sub_goals":[]}"#,
        ]);

        let perception = engine
            .perceive(&base_snapshot("say sure"))
            .await
            .expect("perception");

        let intent = engine.reason(&perception, &llm).await.expect("intent");
        assert!(matches!(
            intent.action,
            IntendedAction::Respond { ref text } if text == "Sure"
        ));
    }

    #[tokio::test]
    async fn reason_prefers_emit_intent_tool_call_for_structured_intent() {
        let mut engine = default_engine(3);
        let llm = MockLlm::with_completion_responses(vec![CompletionResponse {
            content: Vec::new(),
            tool_calls: vec![ToolCall {
                id: "call-1".to_string(),
                name: "emit_intent".to_string(),
                arguments: serde_json::json!({
                    "action": {"Respond": {"text": "Paris is the capital of France"}},
                    "rationale": "direct known fact",
                    "confidence": 0.95
                }),
            }],
            usage: None,
            stop_reason: None,
        }]);

        let result = engine
            .run_cycle(base_snapshot("What is the capital of France?"), &llm)
            .await
            .expect("loop result");

        assert!(matches!(
            result,
            LoopResult::Complete { response, .. } if response == "Paris is the capital of France"
        ));
    }

    #[tokio::test]
    async fn reason_parses_emit_intent_respond_and_delegate_variants() {
        let engine = default_engine(3);
        let respond_call = ToolCall {
            id: "call-respond".to_string(),
            name: "emit_intent".to_string(),
            arguments: serde_json::json!({
                "action": {"Respond": {"text": "Hi"}},
                "rationale": "direct",
                "confidence": 0.9
            }),
        };
        let delegate_call = ToolCall {
            id: "call-delegate".to_string(),
            name: "emit_intent".to_string(),
            arguments: serde_json::json!({
                "action": {"Delegate": {"skill_id": "search", "params": {"q": "fawx"}}},
                "rationale": "lookup",
                "confidence": 0.9
            }),
        };

        let respond_intent = parse_emit_intent_call(&respond_call).expect("respond intent");
        let delegate_intent = parse_emit_intent_call(&delegate_call).expect("delegate intent");
        let delegate_decision = engine
            .decide(&delegate_intent)
            .await
            .expect("delegate decision");

        assert!(matches!(
            respond_intent.action,
            IntendedAction::Respond { ref text } if text == "Hi"
        ));
        assert!(matches!(
            delegate_decision,
            Decision::UseTools(ref calls) if calls.len() == 1 && calls[0].name == "search"
        ));
    }

    #[tokio::test]
    async fn reason_falls_back_to_plain_text_json_when_tool_call_missing() {
        let mut engine = default_engine(3);
        let llm = MockLlm::with_responses(vec![
            r#"{"action":{"Respond":{"text":"Fallback works"}},"rationale":"json text","confidence":0.9,"expected_outcome":null,"sub_goals":[]}"#,
        ]);

        let result = engine
            .run_cycle(base_snapshot("fallback please"), &llm)
            .await
            .expect("loop result");

        assert!(matches!(
            result,
            LoopResult::Complete { response, .. } if response == "Fallback works"
        ));
    }

    #[tokio::test]
    async fn reason_falls_back_to_text_extraction_when_tool_call_is_malformed() {
        let mut engine = default_engine(3);
        let llm = MockLlm::with_completion_responses(vec![CompletionResponse {
            content: vec![ContentBlock::Text {
                text: "Plain fallback answer".to_string(),
            }],
            tool_calls: vec![ToolCall {
                id: "call-1".to_string(),
                name: "emit_intent".to_string(),
                arguments: serde_json::json!({"action": "not-an-object"}),
            }],
            usage: None,
            stop_reason: None,
        }]);

        let result = engine
            .run_cycle(base_snapshot("malformed tool call"), &llm)
            .await
            .expect("loop result");

        assert!(matches!(
            result,
            LoopResult::Complete { response, .. } if response == "Plain fallback answer"
        ));
    }

    #[tokio::test]
    async fn reason_returns_safe_fallback_for_malformed_tool_call_without_text() {
        let mut engine = default_engine(3);
        let llm = MockLlm::with_completion_responses(vec![CompletionResponse {
            content: Vec::new(),
            tool_calls: vec![ToolCall {
                id: "call-1".to_string(),
                name: "emit_intent".to_string(),
                arguments: serde_json::json!({"action": "invalid"}),
            }],
            usage: None,
            stop_reason: None,
        }]);

        let result = engine
            .run_cycle(base_snapshot("bad args"), &llm)
            .await
            .expect("loop result");

        assert!(matches!(
            result,
            LoopResult::Complete { response, .. } if response == SAFE_FALLBACK_RESPONSE
        ));
    }

    #[tokio::test]
    async fn reason_missing_required_action_field_falls_back_gracefully() {
        let mut engine = default_engine(3);
        let llm = MockLlm::with_completion_responses(vec![CompletionResponse {
            content: vec![ContentBlock::Text {
                text: "".to_string(),
            }],
            tool_calls: vec![ToolCall {
                id: "call-1".to_string(),
                name: "emit_intent".to_string(),
                arguments: serde_json::json!({
                    "rationale": "missing action",
                    "confidence": 0.8
                }),
            }],
            usage: None,
            stop_reason: None,
        }]);

        let result = engine
            .run_cycle(base_snapshot("missing action"), &llm)
            .await
            .expect("loop result");

        assert!(matches!(
            result,
            LoopResult::Complete { response, .. } if response == SAFE_FALLBACK_RESPONSE
        ));
    }

    #[tokio::test]
    async fn reason_empty_tool_calls_and_empty_content_return_safe_fallback() {
        let mut engine = default_engine(3);
        let llm = MockLlm::with_completion_responses(vec![CompletionResponse {
            content: Vec::new(),
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: None,
        }]);

        let result = engine
            .run_cycle(base_snapshot("empty response"), &llm)
            .await
            .expect("loop result");

        assert!(matches!(
            result,
            LoopResult::Complete { response, .. } if response == SAFE_FALLBACK_RESPONSE
        ));
    }

    #[tokio::test]
    async fn reason_extracts_expected_outcome_and_sub_goals_from_tool_call() {
        let engine = default_engine(3);
        let llm = MockLlm::with_completion_responses(vec![CompletionResponse {
            content: Vec::new(),
            tool_calls: vec![ToolCall {
                id: "call-1".to_string(),
                name: "emit_intent".to_string(),
                arguments: serde_json::json!({
                    "action": {"Respond": {"text": "Done"}},
                    "rationale": "all optional fields",
                    "confidence": 0.95,
                    "expected_outcome": "User sees completion confirmation",
                    "sub_goals": ["Summarize result", "Offer next step"]
                }),
            }],
            usage: None,
            stop_reason: None,
        }]);

        let perception = engine
            .perceive(&base_snapshot("optional fields"))
            .await
            .expect("perception");
        let intent = engine.reason(&perception, &llm).await.expect("intent");

        assert_eq!(
            intent
                .expected_outcome
                .as_ref()
                .map(|outcome| outcome.description.as_str()),
            Some("User sees completion confirmation")
        );
        assert_eq!(intent.sub_goals.len(), 2);
        assert_eq!(intent.sub_goals[0].description, "Summarize result");
        assert_eq!(intent.sub_goals[1].description, "Offer next step");
    }

    #[tokio::test]
    async fn reason_includes_emit_intent_tool_definition_in_request() {
        let engine = default_engine(3);
        let llm = MockLlm::with_completion_responses(vec![CompletionResponse {
            content: vec![ContentBlock::Text {
                text: r#"{"action":{"Respond":{"text":"ok"}},"rationale":"r","confidence":0.9}"#
                    .to_string(),
            }],
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: None,
        }]);

        let perception = engine
            .perceive(&base_snapshot("check request tools"))
            .await
            .expect("perception");
        let _ = engine.reason(&perception, &llm).await.expect("intent");

        assert_eq!(llm.capture_count(), 1);
        let request = llm.captured_request(0);
        assert!(request.tools.iter().any(|tool| tool.name == "emit_intent"));
    }

    #[tokio::test]
    async fn reason_maps_use_tools_action_from_emit_intent_into_decision_use_tools() {
        let engine = default_engine(3);
        let llm = MockLlm::with_completion_responses(vec![CompletionResponse {
            content: Vec::new(),
            tool_calls: vec![ToolCall {
                id: "call-1".to_string(),
                name: "emit_intent".to_string(),
                arguments: serde_json::json!({
                    "action": {
                        "UseTools": {
                            "tool_calls": [
                                {"tool": "read_file", "args": {"path": "src/main.rs"}}
                            ]
                        }
                    },
                    "rationale": "need file context",
                    "confidence": 0.95
                }),
            }],
            usage: None,
            stop_reason: None,
        }]);

        let perception = engine
            .perceive(&base_snapshot("read the file"))
            .await
            .expect("perception");
        let intent = engine.reason(&perception, &llm).await.expect("intent");
        let decision = engine.decide(&intent).await.expect("decision");

        assert!(matches!(
            decision,
            Decision::UseTools(ref calls)
                if calls.len() == 1
                && calls[0].name == "read_file"
                && calls[0].arguments == serde_json::json!({"path":"src/main.rs"})
        ));
    }

    #[tokio::test]
    async fn step_decide_maps_intent_to_decision() {
        let engine = default_engine(3);
        let intent = ReasonedIntent {
            action: IntendedAction::Respond {
                text: "Hi".to_string(),
            },
            rationale: "reply".to_string(),
            confidence: 0.8,
            expected_outcome: None,
            sub_goals: Vec::new(),
        };

        let decision = engine.decide(&intent).await.expect("decision");
        assert!(matches!(decision, Decision::Respond(text) if text == "Hi"));
    }

    #[tokio::test]
    async fn step_act_supports_stubbed_tool_execution() {
        let engine = default_engine(3);
        let llm = MockLlm::with_responses(vec!["Tool stubs executed."]);
        let decision = Decision::UseTools(vec![fx_llm::ToolCall {
            id: "tool-1".to_string(),
            name: "search".to_string(),
            arguments: serde_json::json!({"q":"fawx"}),
        }]);

        let result = engine.act(&decision, &llm).await.expect("action result");
        assert_eq!(result.tool_results.len(), 1);
        assert!(result.response_text.contains("Tool"));
    }

    #[tokio::test]
    async fn step_verify_detects_discrepancies() {
        let engine = default_engine(3);
        let action = ActionResult {
            decision: Decision::Respond("A".to_string()),
            tool_results: Vec::new(),
            response_text: "B".to_string(),
            tokens_used: TokenUsage::default(),
        };
        let intent = ReasonedIntent {
            action: IntendedAction::Respond {
                text: "A".to_string(),
            },
            rationale: "test".to_string(),
            confidence: 0.9,
            expected_outcome: None,
            sub_goals: Vec::new(),
        };

        let verification = engine.verify(&action, &intent).await.expect("verification");
        assert!(!verification.outcome_matches_intent);
        assert!(!verification.discrepancies.is_empty());
    }

    #[test]
    fn extract_user_message_returns_error_when_all_input_is_empty() {
        let mut snapshot = base_snapshot("   ");
        snapshot.user_input = None;
        snapshot.screen.text_content = "   ".to_string();

        let result = extract_user_message(&snapshot);

        assert!(matches!(result, Err(error) if error.stage == "perceive"));
    }

    #[tokio::test]
    async fn stub_tool_executor_preserves_requested_tool_names() {
        let calls = vec![fx_llm::ToolCall {
            id: "tool-1".to_string(),
            name: "search".to_string(),
            arguments: serde_json::json!({"q":"fawx"}),
        }];

        let executor = StubToolExecutor;
        let results = executor.execute_tools(&calls).await.expect("tool results");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].tool_name, "search");
        assert!(results[0].success);
    }

    #[tokio::test]
    async fn act_with_tools_assembles_streaming_chunks() {
        let engine = default_engine(3);
        let llm = ChunkedMockLlm {
            chunks: vec!["Tool ".to_string(), "summary".to_string()],
            fallback: "fallback".to_string(),
        };
        let decision = Decision::UseTools(vec![fx_llm::ToolCall {
            id: "tool-1".to_string(),
            name: "search".to_string(),
            arguments: serde_json::json!({"q":"fawx"}),
        }]);

        let result = engine.act(&decision, &llm).await.expect("action result");
        assert_eq!(result.response_text, "Tool summary");
    }

    #[test]
    fn build_verification_confidence_scales_with_discrepancy_count() {
        let clean = build_verification(Vec::new());
        let one = build_verification(vec!["mismatch".to_string()]);
        let two = build_verification(vec!["a".to_string(), "b".to_string()]);

        assert!(clean.outcome_matches_intent);
        assert_eq!(clean.confidence, VERIFICATION_CONFIDENCE_CLEAN);
        assert_eq!(one.confidence, VERIFICATION_CONFIDENCE_SINGLE_DISCREPANCY);
        assert_eq!(
            two.confidence,
            VERIFICATION_CONFIDENCE_MULTIPLE_DISCREPANCIES
        );
    }

    #[tokio::test]
    async fn step_learn_and_continue_cover_complete_and_continue_paths() {
        let mut engine = default_engine(3);
        let failed_verification = Verification {
            outcome_matches_intent: false,
            confidence: 0.45,
            discrepancies: vec!["mismatch".to_string()],
        };

        let learning = engine
            .learn(&failed_verification)
            .await
            .expect("learning outcome");
        assert!(learning.adjustment.is_some());

        let continuation = engine
            .should_continue(
                &Decision::Respond("fallback".to_string()),
                &failed_verification,
                &learning,
            )
            .await
            .expect("continuation");
        assert!(matches!(continuation, Continuation::Continue(_)));

        let success_verification = Verification {
            outcome_matches_intent: true,
            confidence: 0.95,
            discrepancies: Vec::new(),
        };
        let success_learning = engine
            .learn(&success_verification)
            .await
            .expect("success learning");
        let done = engine
            .should_continue(
                &Decision::Respond("done".to_string()),
                &success_verification,
                &success_learning,
            )
            .await
            .expect("done continuation");
        assert!(matches!(done, Continuation::Complete));
    }

    #[tokio::test]
    async fn step_continue_clarify_and_defer_always_need_input() {
        let engine = default_engine(3);
        let verification = Verification {
            outcome_matches_intent: true,
            confidence: 0.95,
            discrepancies: Vec::new(),
        };
        let learning = Learning {
            episode: "test".to_string(),
            pattern: None,
            adjustment: None,
        };

        let clarify = engine
            .should_continue(
                &Decision::Clarify("Need more detail".to_string()),
                &verification,
                &learning,
            )
            .await
            .expect("clarify continuation");
        assert!(matches!(
            clarify,
            Continuation::NeedsInput(prompt) if prompt == "Need more detail"
        ));

        let defer = engine
            .should_continue(
                &Decision::Defer("Please provide more context".to_string()),
                &verification,
                &learning,
            )
            .await
            .expect("defer continuation");
        assert!(matches!(
            defer,
            Continuation::NeedsInput(prompt) if prompt == "Please provide more context"
        ));
    }

    #[tokio::test]
    async fn integration_user_message_to_loop_result_response_text() {
        let mut engine = default_engine(10);
        let llm = MockLlm::with_responses(vec![
            r#"{"intent":{"action":{"Respond":{"text":"Integrated response"}},"rationale":"integration","confidence":0.88,"expected_outcome":null,"sub_goals":[]}}"#,
        ]);

        let result = engine
            .run_cycle(base_snapshot("integration test"), &llm)
            .await
            .expect("loop result");

        match result {
            LoopResult::Complete { response, .. } => {
                assert_eq!(response, "Integrated response");
            }
            other => panic!("expected complete result, got {other:?}"),
        }
    }

    #[test]
    fn fallback_extracts_text_from_malformed_intent_json() {
        let raw =
            r#"{"ReasonedIntent":{"action":"greet","rationale":"Hello there!","confidence":0.9}}"#;
        let intent = parse_reasoned_intent(raw, "hey");
        match &intent.action {
            IntendedAction::Respond { text } => {
                assert!(!text.contains("ReasonedIntent"));
                assert!(text.contains("Hello there!"));
            }
            other => panic!("expected Respond, got {other:?}"),
        }
    }

    #[test]
    fn fallback_extracts_nested_text_field() {
        let raw = r#"{"response":{"text":"Paris is the capital.","meta":"ignored"}}"#;
        let intent = parse_reasoned_intent(raw, "capital?");
        match &intent.action {
            IntendedAction::Respond { text } => {
                assert_eq!(text, "Paris is the capital.");
            }
            other => panic!("expected Respond, got {other:?}"),
        }
    }

    #[test]
    fn fallback_extracts_text_from_array_wrapped_response() {
        let raw = r#"[{"text":"Hello from array"}]"#;
        let intent = parse_reasoned_intent(raw, "hey");

        match &intent.action {
            IntendedAction::Respond { text } => {
                assert_eq!(text, "Hello from array");
            }
            other => panic!("expected Respond, got {other:?}"),
        }
    }

    #[test]
    fn fallback_extracts_text_from_nested_array_in_object() {
        let raw = r#"{"responses":[{"text":"Nested array text"}]}"#;
        let intent = parse_reasoned_intent(raw, "hey");

        match &intent.action {
            IntendedAction::Respond { text } => {
                assert_eq!(text, "Nested array text");
            }
            other => panic!("expected Respond, got {other:?}"),
        }
    }
}
