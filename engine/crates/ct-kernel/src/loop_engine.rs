//! Agentic loop orchestrator.

use crate::act::{ActionResult, TokenUsage, ToolResult};
use crate::budget::{ActionCost, BudgetRemaining, BudgetTracker};
use crate::context_manager::ContextCompactor;
use crate::continuation::Continuation;
use crate::decide::{decide_from_intent, Decision};
use crate::learn::Learning;
use crate::perceive::{ProcessedPerception, TrimmingPolicy};
use crate::types::{
    Goal, IdentityContext, IntendedAction, LoopError, PerceptionSnapshot, ReasonedIntent,
    ReasoningContext, WorkingMemoryEntry,
};
use crate::verify::Verification;
use ct_core::types::{InputSource, UserInput};
use ct_llm::Message;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Re-exported LLM provider trait used by the loop.
pub use ct_llm::LlmProvider;

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
    max_iterations: u32,
    iteration_count: u32,
}

impl LoopEngine {
    /// Create a new loop engine with budget + context managers.
    pub fn new(budget: BudgetTracker, context: ContextCompactor, max_iterations: u32) -> Self {
        Self {
            budget,
            context,
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
        self.iteration_count = 0;
        let mut learnings = Vec::new();
        let mut cycle_tokens = TokenUsage::default();
        let mut partial_response: Option<String> = None;

        while self.iteration_count < self.max_iterations {
            self.iteration_count = self.iteration_count.saturating_add(1);

            let processed = self.perceive(&perception).await?;

            let reason_cost = self.estimate_reasoning_cost(&processed);
            if self.budget.check(&reason_cost).is_err() {
                return Ok(LoopResult::BudgetExhausted {
                    partial_response,
                    iterations: self.iteration_count,
                });
            }

            let intent = self.reason(&processed, llm).await?;
            self.budget.record(&reason_cost);
            cycle_tokens.accumulate(TokenUsage {
                input_tokens: reason_cost.tokens.saturating_mul(3) / 5,
                output_tokens: reason_cost.tokens.saturating_mul(2) / 5,
            });

            let decision = self.decide(&intent).await?;
            let estimated_action_cost = self.estimate_action_cost(&decision);
            if self.budget.check(&estimated_action_cost).is_err() {
                return Ok(LoopResult::BudgetExhausted {
                    partial_response,
                    iterations: self.iteration_count,
                });
            }

            let action = self.act(&decision, llm).await?;

            let action_cost = self.action_cost_from_result(&action);
            if self.budget.check(&action_cost).is_err() {
                return Ok(LoopResult::BudgetExhausted {
                    partial_response: Some(action.response_text),
                    iterations: self.iteration_count,
                });
            }

            self.budget.record(&action_cost);
            cycle_tokens.accumulate(action.tokens_used);
            partial_response = Some(action.response_text.clone());

            let verification = self.verify(&action, &intent).await?;
            let learning = self.learn(&verification).await?;
            let continuation = self.should_continue(&verification, &learning).await?;

            learnings.push(learning);

            match continuation {
                Continuation::Complete => {
                    return Ok(LoopResult::Complete {
                        response: action.response_text,
                        iterations: self.iteration_count,
                        tokens_used: cycle_tokens,
                        learnings,
                    });
                }
                Continuation::NeedsInput(prompt) => {
                    return Ok(LoopResult::NeedsInput {
                        prompt,
                        iterations: self.iteration_count,
                    });
                }
                Continuation::Continue(sub_goal) => {
                    perception = next_perception_from_sub_goal(&perception, &sub_goal);
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

    /// Perceive step.
    async fn perceive(&self, snapshot: &PerceptionSnapshot) -> Result<ProcessedPerception, LoopError> {
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

        let mut context_window = vec![Message::user(user_message.clone())];

        let synthetic_context = self.synthetic_context(snapshot, &user_message);
        if self.context.needs_compaction(&synthetic_context) {
            let compacted = self
                .context
                .compact(synthetic_context, TrimmingPolicy::ByRelevance);
            if let Some(summary) = compacted
                .working_memory
                .iter()
                .find(|entry| entry.key == "compacted_context_summary")
            {
                context_window.push(Message::assistant(summary.value.clone()));
            }
        }

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
        let system_prompt = "You are Citros, an autonomous assistant kernel. Reason step output MUST be JSON. Return one ReasonedIntent with action, rationale, confidence, expected_outcome (optional), and sub_goals.";
        let context_messages = perception
            .context_window
            .iter()
            .map(message_to_text)
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = format!(
            "{system_prompt}\n\nActive goals:\n- {}\n\nBudget remaining: {} tokens, {} llm calls\n\nContext:\n{context_messages}\n\nUser message:\n{}\n\nRespond with JSON only.",
            perception.active_goals.join("\n- "),
            perception.budget_remaining.tokens,
            perception.budget_remaining.llm_calls,
            perception.user_message,
        );

        let raw = llm
            .generate(&prompt, 768)
            .await
            .map_err(|error| loop_error("reason", &format!("llm generation failed: {error}"), true))?;

        Ok(parse_reasoned_intent(&raw, &perception.user_message))
    }

    /// Decide step.
    async fn decide(&self, intent: &ReasonedIntent) -> Result<Decision, LoopError> {
        Ok(decide_from_intent(intent))
    }

    /// Act step.
    async fn act(&self, decision: &Decision, llm: &dyn LlmProvider) -> Result<ActionResult, LoopError> {
        match decision {
            Decision::Respond(text) => Ok(ActionResult {
                decision: decision.clone(),
                tool_results: Vec::new(),
                response_text: text.clone(),
                tokens_used: TokenUsage::default(),
            }),
            Decision::Clarify(text) => Ok(ActionResult {
                decision: decision.clone(),
                tool_results: Vec::new(),
                response_text: text.clone(),
                tokens_used: TokenUsage::default(),
            }),
            Decision::Defer(text) => Ok(ActionResult {
                decision: decision.clone(),
                tool_results: Vec::new(),
                response_text: text.clone(),
                tokens_used: TokenUsage::default(),
            }),
            Decision::UseTools(calls) => {
                let tool_results = calls
                    .iter()
                    .map(|call| ToolResult {
                        tool_name: call.name.clone(),
                        success: true,
                        output: format!(
                            "Stub tool execution for '{}' with args {}",
                            call.name, call.arguments
                        ),
                    })
                    .collect::<Vec<_>>();

                let tool_summary = tool_results
                    .iter()
                    .map(|result| format!("- {}: {}", result.tool_name, result.output))
                    .collect::<Vec<_>>()
                    .join("\n");

                let synthesis_prompt = format!(
                    "You are Citros. Summarize the following tool activity for the user in concise text:\n{tool_summary}\n\nNote: tool execution is stubbed in this build."
                );

                let llm_text = llm.generate(&synthesis_prompt, 384).await.unwrap_or_else(|_| {
                    "I prepared tool actions, but tool execution is still stubbed in this phase.".to_string()
                });

                let usage = TokenUsage {
                    input_tokens: estimate_tokens(&synthesis_prompt),
                    output_tokens: estimate_tokens(&llm_text),
                };

                Ok(ActionResult {
                    decision: decision.clone(),
                    tool_results,
                    response_text: llm_text,
                    tokens_used: usage,
                })
            }
        }
    }

    /// Verify step.
    async fn verify(
        &self,
        action: &ActionResult,
        intent: &ReasonedIntent,
    ) -> Result<Verification, LoopError> {
        let mut discrepancies = Vec::new();

        let action_matches_intent = match &intent.action {
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
        };

        if !action_matches_intent {
            discrepancies.push("action response does not align with intent action".to_string());
        }

        if let Some(expected) = &intent.expected_outcome {
            let expected_text = expected.description.to_ascii_lowercase();
            let response_text = action.response_text.to_ascii_lowercase();

            if !response_text.contains(&expected_text) {
                discrepancies.push(format!(
                    "expected outcome not reflected in action response: {}",
                    expected.description
                ));
            }
        }

        let outcome_matches_intent = discrepancies.is_empty();
        let confidence = if outcome_matches_intent {
            0.9
        } else if discrepancies.len() == 1 {
            0.45
        } else {
            0.25
        };

        Ok(Verification {
            outcome_matches_intent,
            confidence,
            discrepancies,
        })
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
        verification: &Verification,
        _learning: &Learning,
    ) -> Result<Continuation, LoopError> {
        if verification.outcome_matches_intent {
            return Ok(Continuation::Complete);
        }

        if verification.confidence < 0.35 {
            return Ok(Continuation::NeedsInput(
                "I need a bit more detail to continue safely. Could you clarify your goal?"
                    .to_string(),
            ));
        }

        Ok(Continuation::Continue(
            "Refine the response using tighter intent alignment.".to_string(),
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

        let output_tokens = 192;

        ActionCost {
            llm_calls: 1,
            tool_invocations: 0,
            tokens: input_tokens.saturating_add(output_tokens),
            cost_cents: 2,
        }
    }

    fn estimate_action_cost(&self, decision: &Decision) -> ActionCost {
        match decision {
            Decision::UseTools(calls) => ActionCost {
                llm_calls: 1,
                tool_invocations: calls.len() as u32,
                tokens: 320,
                cost_cents: 2,
            },
            Decision::Respond(_) | Decision::Clarify(_) | Decision::Defer(_) => ActionCost::default(),
        }
    }

    fn action_cost_from_result(&self, action: &ActionResult) -> ActionCost {
        ActionCost {
            llm_calls: if action.tokens_used.total_tokens() > 0 { 1 } else { 0 },
            tool_invocations: action.tool_results.len() as u32,
            tokens: action.tokens_used.total_tokens(),
            cost_cents: if action.tokens_used.total_tokens() > 0 {
                2
            } else if action.tool_results.is_empty() {
                0
            } else {
                1
            },
        }
    }

    fn synthetic_context(&self, snapshot: &PerceptionSnapshot, user_message: &str) -> ReasoningContext {
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
    let char_estimate = ((char_count + 3) / 4) as u64;
    let word_estimate = text.split_whitespace().count() as u64;
    char_estimate.max(word_estimate).max(1)
}

fn message_to_text(message: &Message) -> String {
    let role = format!("{:?}", message.role);
    let content = message
        .content
        .iter()
        .map(|block| match block {
            ct_llm::ContentBlock::Text { text } => text.clone(),
            ct_llm::ContentBlock::ToolUse { name, .. } => format!("[tool_use:{name}]"),
            ct_llm::ContentBlock::ToolResult { tool_use_id, .. } => {
                format!("[tool_result:{tool_use_id}]")
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    format!("{role}: {content}")
}

fn parse_reasoned_intent(raw: &str, fallback_user_message: &str) -> ReasonedIntent {
    parse_reasoned_intent_inner(raw).unwrap_or_else(|| ReasonedIntent {
        action: IntendedAction::Respond {
            text: raw
                .trim()
                .strip_prefix("```json")
                .unwrap_or(raw.trim())
                .trim_matches('`')
                .trim()
                .to_string(),
        },
        rationale: "Fallback intent generated from unstructured reasoning output".to_string(),
        confidence: 0.4,
        expected_outcome: None,
        sub_goals: vec![Goal::new(
            format!("Respond clearly to: {fallback_user_message}"),
            vec!["User receives a relevant response".to_string()],
            Some(1),
        )],
    })
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
    intent
}

fn loop_error(stage: &str, reason: &str, recoverable: bool) -> LoopError {
    LoopError {
        stage: stage.to_string(),
        reason: reason.to_string(),
        recoverable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use ct_core::error::LlmError as CoreLlmError;
    use ct_core::types::ScreenState;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    #[derive(Debug)]
    struct MockLlm {
        name: String,
        responses: Mutex<VecDeque<String>>,
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
            }
        }
    }

    #[async_trait]
    impl LlmProvider for MockLlm {
        async fn generate(&self, _prompt: &str, _max_tokens: u32) -> Result<String, CoreLlmError> {
            self.responses
                .lock()
                .expect("responses lock")
                .pop_front()
                .ok_or_else(|| CoreLlmError::Inference("no mock response available".to_string()))
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

        fn model_name(&self) -> &str {
            &self.name
        }
    }

    fn base_snapshot(text: &str) -> PerceptionSnapshot {
        PerceptionSnapshot {
            screen: ScreenState {
                current_app: "citros.tui".to_string(),
                elements: Vec::new(),
                text_content: text.to_string(),
            },
            notifications: Vec::new(),
            active_app: "citros.tui".to_string(),
            timestamp_ms: 1_700_000_000_000,
            sensor_data: None,
            user_input: Some(UserInput {
                text: text.to_string(),
                source: InputSource::Text,
                timestamp: 1_700_000_000_000,
                context_id: None,
            }),
        }
    }

    fn default_engine(max_iterations: u32) -> LoopEngine {
        let budget = BudgetTracker::new(crate::budget::BudgetConfig::default(), 0, 0);
        let context = ContextCompactor::new(4_000, 3_000);
        LoopEngine::new(budget, context, max_iterations)
    }

    #[tokio::test]
    async fn run_cycle_returns_complete_with_mock_llm() {
        let mut engine = default_engine(10);
        let llm = MockLlm::with_responses(vec![
            r#"{"action":{"Respond":{"text":"Hello from Citros"}},"rationale":"reply","confidence":0.92,"expected_outcome":null,"sub_goals":[]}"#,
        ]);

        let result = engine
            .run_cycle(base_snapshot("Hi there"), &llm)
            .await
            .expect("loop result");

        assert!(matches!(
            result,
            LoopResult::Complete {
                response,
                iterations: 1,
                ..
            } if response == "Hello from Citros"
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
        let budget = BudgetTracker::new(config, 0, 0);
        let context = ContextCompactor::new(4_000, 3_000);
        let mut engine = LoopEngine::new(budget, context, 10);

        let llm = MockLlm::with_responses(vec![r#"{"action":{"Respond":{"text":"never used"}},"rationale":"n/a","confidence":0.9,"expected_outcome":null,"sub_goals":[]}"#]);

        let result = engine
            .run_cycle(base_snapshot("hi"), &llm)
            .await
            .expect("loop result");

        assert!(matches!(
            result,
            LoopResult::BudgetExhausted {
                partial_response: None,
                iterations: 1
            }
        ));
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
        let decision = Decision::UseTools(vec![ct_llm::ToolCall {
            id: "tool-1".to_string(),
            name: "search".to_string(),
            arguments: serde_json::json!({"q":"citros"}),
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
            .should_continue(&failed_verification, &learning)
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
            .should_continue(&success_verification, &success_learning)
            .await
            .expect("done continuation");
        assert!(matches!(done, Continuation::Complete));
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
}
