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

    fn route_sub_goal_call(
        &self,
        request: &crate::act::SubGoalToolRoutingRequest,
        call_id: &str,
    ) -> Option<ToolCall> {
        Some(ToolCall {
            id: call_id.to_string(),
            name: request.required_tools.first()?.clone(),
            arguments: serde_json::json!({
                "description": request.description,
            }),
        })
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

    async fn complete(&self, _: CompletionRequest) -> Result<CompletionResponse, ProviderError> {
        self.complete_calls.fetch_add(1, Ordering::SeqCst);
        self.responses
            .lock()
            .expect("lock")
            .pop_front()
            .unwrap_or_else(|| Err(ProviderError::Provider("no scripted response".to_string())))
    }
}

fn budget_config_with_mode(
    max_llm_calls: u32,
    max_recursion_depth: u32,
    mode: DepthMode,
) -> BudgetConfig {
    BudgetConfig {
        max_llm_calls,
        max_tool_invocations: 20,
        max_tokens: 10_000,
        max_cost_cents: 100,
        max_wall_time_ms: 60_000,
        max_recursion_depth,
        decompose_depth_mode: mode,
        ..BudgetConfig::default()
    }
}

fn budget_config(max_llm_calls: u32, max_recursion_depth: u32) -> BudgetConfig {
    budget_config_with_mode(max_llm_calls, max_recursion_depth, DepthMode::Static)
}

fn decomposition_engine(config: BudgetConfig, depth: u32) -> LoopEngine {
    let started_at_ms = current_time_ms();
    LoopEngine::builder()
        .budget(BudgetTracker::new(config, started_at_ms, depth))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(4)
        .tool_executor(Arc::new(PassiveToolExecutor))
        .synthesis_instruction("Summarize tool output".to_string())
        .build()
        .expect("test engine build")
}

fn decomposition_plan(descriptions: &[&str]) -> DecompositionPlan {
    DecompositionPlan {
        sub_goals: descriptions
            .iter()
            .map(|description| {
                SubGoal::with_definition_of_done(
                    (*description).to_string(),
                    Vec::new(),
                    Some(&format!("output for {description}")),
                    None,
                )
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
    while events.len() < count {
        let event = receiver.recv().await.expect("event");
        if matches!(
            event,
            InternalMessage::SubGoalStarted { .. } | InternalMessage::SubGoalCompleted { .. }
        ) {
            events.push(event);
        }
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

fn decomposition_run_snapshot(text: &str) -> PerceptionSnapshot {
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
            images: Vec::new(),
            documents: Vec::new(),
        }),
        sensor_data: None,
        conversation_history: vec![Message::user(text)],
        steer_context: None,
    }
}

fn decompose_plan_response(descriptions: &[&str]) -> CompletionResponse {
    let sub_goals = descriptions
        .iter()
        .map(|description| serde_json::json!({"description": description}))
        .collect::<Vec<_>>();
    CompletionResponse {
        content: Vec::new(),
        tool_calls: vec![decompose_tool_call(serde_json::json!({
            "sub_goals": sub_goals,
            "strategy": "Sequential"
        }))],
        usage: None,
        stop_reason: Some("tool_use".to_string()),
    }
}

fn signals_from_result(result: &LoopResult) -> &[Signal] {
    result.signals()
}

fn sample_signal(message: &str) -> Signal {
    Signal {
        step: LoopStep::Act,
        kind: SignalKind::Success,
        message: message.to_string(),
        metadata: serde_json::json!({"source": "test"}),
        timestamp_ms: 1,
    }
}

fn assert_loop_result_signals(result: LoopResult, expected: Vec<Signal>) {
    assert_eq!(result.signals(), expected.as_slice());
}

#[test]
fn loop_result_signals_returns_variant_signals() {
    let complete = vec![sample_signal("complete")];
    assert_loop_result_signals(
        LoopResult::Complete {
            response: "done".to_string(),
            iterations: 1,
            tokens_used: TokenUsage::default(),
            signals: complete.clone(),
        },
        complete,
    );

    let budget_exhausted = vec![sample_signal("budget")];
    assert_loop_result_signals(
        LoopResult::BudgetExhausted {
            partial_response: Some("partial".to_string()),
            iterations: 2,
            signals: budget_exhausted.clone(),
        },
        budget_exhausted,
    );

    let stopped = vec![sample_signal("stopped")];
    assert_loop_result_signals(
        LoopResult::UserStopped {
            partial_response: Some("partial".to_string()),
            iterations: 4,
            signals: stopped.clone(),
        },
        stopped,
    );

    let error = vec![sample_signal("error")];
    assert_loop_result_signals(
        LoopResult::Error {
            message: "boom".to_string(),
            recoverable: true,
            signals: error.clone(),
        },
        error,
    );
}

async fn run_budget_exhausted_decomposition_cycle() -> (LoopResult, usize) {
    let mut engine = decomposition_engine(budget_config(4, 6), 0);
    let llm = ScriptedLlm::new(vec![
        Ok(decompose_plan_response(&["first", "second", "third"])),
        Ok(text_response("   ")),
        Ok(text_response("   ")),
        Ok(text_response("   ")),
    ]);
    let result = engine
        .run_cycle(
            decomposition_run_snapshot("break this into sub-goals"),
            &llm,
        )
        .await
        .expect("run_cycle");
    (result, llm.complete_calls())
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
        images: Vec::new(),
        documents: Vec::new(),
        context_window: vec![Message::user("context")],
        active_goals: vec!["Help the user".to_string()],
        budget_remaining: sample_budget_remaining(),
        steer_context: None,
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
async fn decomposition_uses_allocator_plan_for_each_sub_goal() {
    let mut engine = decomposition_engine(budget_config(20, 6), 0);
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
    assert_eq!(status.remaining.llm_calls, 17);
    assert_eq!(status.tool_invocations_used, 0);
    assert_eq!(status.cost_cents_used, 6);
    assert!(status.tokens_used > 0);
}

#[tokio::test]
async fn execute_decomposition_continues_with_internal_result_context() {
    let mut engine = decomposition_engine(budget_config(20, 6), 0);
    let plan = decomposition_plan(&["first", "second"]);
    let decision = Decision::Decompose(plan.clone());
    let llm = ScriptedLlm::new(vec![
        Ok(text_response("first-ok")),
        Ok(text_response("second-ok")),
    ]);

    let action = engine
        .execute_decomposition(&decision, &plan, &llm, &[])
        .await
        .expect("decomposition");

    match action.next_step {
        ActionNextStep::Continue(ActionContinuation {
            partial_response,
            context_message,
            ..
        }) => {
            assert_eq!(partial_response, None);
            let context_message = context_message.expect("context message");
            assert!(context_message.contains("Task decomposition results:"));
            assert!(context_message.contains("first => completed: first-ok"));
            assert!(context_message.contains("second => completed: second-ok"));
        }
        other => panic!("expected continuation, got: {other:?}"),
    }
}

#[test]
fn continue_actions_do_not_treat_response_text_as_partial_output() {
    let action = ActionResult {
        decision: Decision::Respond("keep going".to_string()),
        tool_results: Vec::new(),
        response_text: "Task decomposition results:\n1. step => completed: ok".to_string(),
        tokens_used: TokenUsage::default(),
        next_step: ActionNextStep::Continue(ActionContinuation::new(
            None,
            Some("Task decomposition results:\n1. step => completed: ok".to_string()),
        )),
    };

    assert_eq!(action_partial_response(&action), None);
}

#[test]
fn prepend_accumulated_text_to_action_does_not_invent_partial_response() {
    let action = ActionResult {
        decision: Decision::Respond("keep going".to_string()),
        tool_results: Vec::new(),
        response_text: String::new(),
        tokens_used: TokenUsage::default(),
        next_step: ActionNextStep::Continue(ActionContinuation::new(
            None,
            Some("Task decomposition results:\n1. step => completed: ok".to_string()),
        )),
    };

    let stitched = prepend_accumulated_text_to_action(action, &[String::from("Earlier note")]);

    assert!(stitched.response_text.is_empty());
    match stitched.next_step {
        ActionNextStep::Continue(ActionContinuation {
            partial_response,
            context_message,
            ..
        }) => {
            assert_eq!(partial_response, None);
            assert_eq!(
                context_message.as_deref(),
                Some("Earlier note\n\nTask decomposition results:\n1. step => completed: ok")
            );
        }
        other => panic!("expected continuation, got {other:?}"),
    }
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
async fn sub_goal_below_floor_maps_to_skipped_outcome() {
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
        .contains("budget-limited => skipped (below floor)"));
}

#[tokio::test]
async fn low_budget_decomposition_avoids_budget_exhaustion_signal() {
    let (result, llm_calls) = run_budget_exhausted_decomposition_cycle().await;

    assert!(matches!(&result, LoopResult::Complete { .. }));
    assert_eq!(llm_calls, 1);

    let blocked_budget_signals = signals_from_result(&result)
        .iter()
        .filter(|signal| signal.kind == SignalKind::Blocked && signal.message == "budget exhausted")
        .count();
    assert_eq!(blocked_budget_signals, 0);
}

#[tokio::test]
async fn low_budget_decomposition_skips_sub_goals_without_retry_storm() {
    let (result, _llm_calls) = run_budget_exhausted_decomposition_cycle().await;

    let response = match &result {
        LoopResult::Complete { response, .. } => response,
        other => panic!("expected LoopResult::Complete, got: {other:?}"),
    };
    assert!(response.contains("first => skipped (below floor)"));
    assert!(response.contains("second => skipped (below floor)"));
    assert!(response.contains("third => skipped (below floor)"));

    let progress_signals = signals_from_result(&result)
        .iter()
        .filter(|signal| {
            signal.step == LoopStep::Act
                && signal.kind == SignalKind::Trace
                && signal.message.starts_with("Sub-goal ")
        })
        .count();
    assert_eq!(progress_signals, 3);
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
    let llm = ScriptedLlm::new(vec![
        Ok(text_response("output for first")),
        Ok(text_response("output for second")),
    ]);

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
    let llm = ScriptedLlm::new(vec![
        Ok(text_response("output for first")),
        Ok(text_response("output for second")),
    ]);

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

    let plan = DecompositionPlan {
        sub_goals: vec![
            SubGoal::new("first", Vec::new(), SubGoalContract::default(), None),
            SubGoal::new("second", Vec::new(), SubGoalContract::default(), None),
        ],
        strategy: AggregationStrategy::Parallel,
        truncated_from: None,
    };
    let decision = Decision::Decompose(plan.clone());
    let llm = ScriptedLlm::new(vec![
        Ok(text_response("first complete")),
        Ok(text_response("second complete")),
    ]);

    let _action = engine
        .execute_decomposition(&decision, &plan, &llm, &[])
        .await
        .expect("decomposition");

    let events = collect_internal_events(&mut receiver, 4).await;
    assert_eq!(events.len(), 4);
    assert!(events.iter().any(|event| {
            matches!(event, InternalMessage::SubGoalStarted { index: 0, total: 2, description } if description == "first")
        }));
    assert!(events.iter().any(|event| {
            matches!(event, InternalMessage::SubGoalStarted { index: 1, total: 2, description } if description == "second")
        }));
    assert!(
        events.iter().any(|event| {
            matches!(
                event,
                InternalMessage::SubGoalCompleted {
                    index: 0,
                    total: 2,
                    success: true
                }
            )
        }),
        "{events:?}"
    );
    assert!(
        events.iter().any(|event| {
            matches!(
                event,
                InternalMessage::SubGoalCompleted {
                    index: 1,
                    total: 2,
                    success: true
                }
            )
        }),
        "{events:?}"
    );
}

#[tokio::test]
async fn sequential_execution_emits_progress_events_via_event_bus() {
    let mut engine = decomposition_engine(budget_config(10, 6), 0);
    let bus = fx_core::EventBus::new(16);
    let mut receiver = bus.subscribe();
    engine.set_event_bus(bus);

    let plan = DecompositionPlan {
        sub_goals: vec![
            SubGoal::new("first", Vec::new(), SubGoalContract::default(), None),
            SubGoal::new("second", Vec::new(), SubGoalContract::default(), None),
        ],
        strategy: AggregationStrategy::Sequential,
        truncated_from: None,
    };
    let decision = Decision::Decompose(plan.clone());
    let llm = ScriptedLlm::new(vec![
        Ok(text_response("first complete")),
        Ok(text_response("second complete")),
    ]);

    let _action = engine
        .execute_decomposition(&decision, &plan, &llm, &[])
        .await
        .expect("decomposition");

    let events = collect_internal_events(&mut receiver, 4).await;
    assert_eq!(events.len(), 4);
    assert!(events.iter().any(|event| {
            matches!(event, InternalMessage::SubGoalStarted { index: 0, total: 2, description } if description == "first")
        }));
    assert!(
        events.iter().any(|event| {
            matches!(
                event,
                InternalMessage::SubGoalCompleted {
                    index: 0,
                    total: 2,
                    success: true
                }
            )
        }),
        "{events:?}"
    );
    assert!(events.iter().any(|event| {
            matches!(event, InternalMessage::SubGoalStarted { index: 1, total: 2, description } if description == "second")
        }));
    assert!(
        events.iter().any(|event| {
            matches!(
                event,
                InternalMessage::SubGoalCompleted {
                    index: 1,
                    total: 2,
                    success: true
                }
            )
        }),
        "{events:?}"
    );
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
    let mut engine = decomposition_engine(budget_config(20, 6), 0);
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
                plan.sub_goals[0].completion_contract.definition_of_done,
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
    let request = build_reasoning_request(ReasoningRequestParams::new(
        &sample_perception(),
        "mock-model",
        ToolRequestConfig::new(vec![sample_tool_definition()], true),
        RequestBuildContext::new(None, None, None, false),
    ));

    assert_decompose_tool_present(&request.tools);
}

#[test]
fn decompose_tool_definition_included_in_continuation_request() {
    let request = build_continuation_request(ContinuationRequestParams::new(
        &[Message::assistant("intermediate")],
        "mock-model",
        ToolRequestConfig::new(vec![sample_tool_definition()], true),
        RequestBuildContext::new(None, None, None, false),
    ));

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
            assert_eq!(
                plan.sub_goals[0].completion_contract.definition_of_done,
                None
            );
            assert_eq!(plan.sub_goals[0].complexity_hint, None);
            assert_eq!(plan.strategy, AggregationStrategy::Sequential);
        }
        other => panic!("expected decomposition decision, got: {other:?}"),
    }
}

fn concurrent_plan(descriptions: &[&str]) -> DecompositionPlan {
    DecompositionPlan {
        sub_goals: descriptions
            .iter()
            .map(|d| {
                SubGoal::with_definition_of_done(
                    (*d).to_string(),
                    Vec::new(),
                    Some(&format!("output for {d}")),
                    None,
                )
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

#[test]
fn publish_tool_round_emits_atomic_event_with_provider_ids() {
    let mut engine = decomposition_engine(budget_config(20, 6), 0);
    let bus = fx_core::EventBus::new(16);
    let mut receiver = bus.subscribe();
    engine.set_event_bus(bus);
    engine
        .tool_call_provider_ids
        .insert("call-1".to_string(), "fc-1".to_string());

    let calls = vec![ToolCall {
        id: "call-1".to_string(),
        name: "read_file".to_string(),
        arguments: serde_json::json!({"path": "README.md"}),
    }];
    let results = vec![ToolResult {
        tool_call_id: "call-1".to_string(),
        tool_name: "read_file".to_string(),
        success: true,
        output: "ok".to_string(),
    }];

    engine.publish_tool_round(&calls, &results, CycleStream::disabled());

    let events: Vec<_> = std::iter::from_fn(|| receiver.try_recv().ok()).collect();
    assert!(events.iter().any(|event| matches!(
        event,
        InternalMessage::ToolUse {
            call_id,
            provider_id,
            ..
        } if call_id == "call-1" && provider_id.as_deref() == Some("fc-1")
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        InternalMessage::ToolResult { call_id, .. } if call_id == "call-1"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        InternalMessage::ToolRound { calls, results }
            if calls.len() == 1
                && results.len() == 1
                && calls[0].call_id == "call-1"
                && calls[0].provider_id.as_deref() == Some("fc-1")
                && results[0].call_id == "call-1"
    )));
}

#[test]
fn sequential_adaptive_allocation_gives_more_to_complex_sub_goals() {
    let engine = decomposition_engine(budget_config_with_mode(40, 8, DepthMode::Adaptive), 0);
    let plan = DecompositionPlan {
        sub_goals: vec![
            SubGoal {
                description: "quick note".to_string(),
                required_tools: Vec::new(),
                completion_contract: SubGoalContract::from_definition_of_done(None),
                complexity_hint: Some(ComplexityHint::Trivial),
            },
            SubGoal {
                description: "implement migration plan".to_string(),
                required_tools: vec!["read_file".to_string(), "edit".to_string()],
                completion_contract: SubGoalContract::from_definition_of_done(None),
                complexity_hint: Some(ComplexityHint::Complex),
            },
        ],
        strategy: AggregationStrategy::Sequential,
        truncated_from: None,
    };
    let allocator = BudgetAllocator::new();

    let allocation = allocator.allocate(
        &engine.budget,
        &plan.sub_goals,
        AllocationMode::Sequential,
        current_time_ms(),
    );

    assert!(
        allocation.sub_goal_budgets[1].max_llm_calls > allocation.sub_goal_budgets[0].max_llm_calls
    );
}

#[test]
fn concurrent_adaptive_allocation_distributes_proportionally() {
    let engine = decomposition_engine(budget_config_with_mode(50, 8, DepthMode::Adaptive), 0);
    let plan = DecompositionPlan {
        sub_goals: vec![
            SubGoal {
                description: "quick note".to_string(),
                required_tools: Vec::new(),
                completion_contract: SubGoalContract::from_definition_of_done(None),
                complexity_hint: Some(ComplexityHint::Trivial),
            },
            SubGoal {
                description: "complex migration".to_string(),
                required_tools: vec!["read".to_string(), "edit".to_string(), "test".to_string()],
                completion_contract: SubGoalContract::from_definition_of_done(None),
                complexity_hint: Some(ComplexityHint::Complex),
            },
        ],
        strategy: AggregationStrategy::Parallel,
        truncated_from: None,
    };
    let allocator = BudgetAllocator::new();

    let allocation = allocator.allocate(
        &engine.budget,
        &plan.sub_goals,
        AllocationMode::Concurrent,
        current_time_ms(),
    );

    assert_eq!(allocation.sub_goal_budgets[0].max_llm_calls, 9);
    assert_eq!(allocation.sub_goal_budgets[1].max_llm_calls, 36);
}

#[tokio::test]
async fn budget_floor_skips_non_viable_sub_goals_with_signal() {
    let mut engine = decomposition_engine(budget_config(4, 6), 0);
    let plan = decomposition_plan(&["first", "second", "third"]);
    let decision = Decision::Decompose(plan.clone());
    let llm = ScriptedLlm::new(vec![Ok(text_response("unused"))]);

    let action = engine
        .execute_decomposition(&decision, &plan, &llm, &[])
        .await
        .expect("decomposition");

    assert!(action.response_text.contains("skipped (below floor)"));
    let skipped_signal = engine
        .signals
        .signals()
        .iter()
        .find(|signal| {
            signal.step == LoopStep::Act
                && signal.kind == SignalKind::Friction
                && signal.message.contains("skipped:")
        })
        .expect("skipped signal");
    assert_eq!(
        skipped_signal.metadata["reason"],
        serde_json::json!("below_budget_floor")
    );
}

#[test]
fn parent_continuation_budget_prevents_parent_starvation() {
    let engine = decomposition_engine(budget_config(40, 8), 0);
    let plan = decomposition_plan(&["one", "two"]);
    let allocator = BudgetAllocator::new();
    let remaining = engine.budget.remaining(current_time_ms());

    let allocation = allocator.allocate(
        &engine.budget,
        &plan.sub_goals,
        AllocationMode::Sequential,
        current_time_ms(),
    );

    assert!(allocation.parent_continuation_budget.max_llm_calls >= 4);
    let child_sum = allocation
        .sub_goal_budgets
        .iter()
        .fold(0_u32, |acc, budget| {
            acc.saturating_add(budget.max_llm_calls)
        });
    assert!(
        child_sum
            <= remaining
                .llm_calls
                .saturating_sub(allocation.parent_continuation_budget.max_llm_calls)
    );
}

#[tokio::test]
async fn child_budget_increments_depth_and_inherits_effective_max_depth() {
    let config = budget_config_with_mode(8, 3, DepthMode::Adaptive);
    let engine = decomposition_engine(config, 0);
    let remaining = engine.budget.remaining(current_time_ms());
    let effective_cap = engine.effective_decomposition_depth_cap(&remaining);
    let mut child_budget = budget_config_with_mode(8, 3, DepthMode::Adaptive);
    engine.apply_effective_depth_cap(std::slice::from_mut(&mut child_budget), effective_cap);

    let goal = SubGoal {
        description: "child".to_string(),
        required_tools: Vec::new(),
        completion_contract: SubGoalContract::from_definition_of_done(None),
        complexity_hint: None,
    };
    let llm = ScriptedLlm::new(vec![Ok(text_response("done"))]);
    let execution = engine
        .run_sub_goal(&goal, child_budget, &llm, &[], &[])
        .await;

    assert_eq!(execution.budget.depth(), 1);
    assert_eq!(execution.budget.config().max_recursion_depth, effective_cap);
}

#[test]
fn sub_goal_result_from_loop_preserves_budget_exhausted_partial_response() {
    let goal = SubGoal {
        description: "Research X POST endpoint".to_string(),
        required_tools: vec!["web_search".to_string()],
        completion_contract: SubGoalContract::from_definition_of_done(Some("Endpoint summary")),
        complexity_hint: None,
    };

    let result = sub_goal_result_from_loop(
        goal.clone(),
        LoopResult::BudgetExhausted {
            partial_response: Some("Enough research to proceed with implementation.".into()),
            iterations: 3,
            signals: Vec::new(),
        },
    );

    assert_eq!(result.goal, goal);
    assert!(matches!(
        result.outcome,
        SubGoalOutcome::BudgetExhausted {
            partial_response: Some(ref text)
        } if text == "Enough research to proceed with implementation."
    ));
}

#[test]
fn should_halt_sub_goal_sequence_allows_budget_exhausted_partial_response() {
    let result = SubGoalResult {
        goal: SubGoal {
            description: "Research X API".to_string(),
            required_tools: vec!["web_search".to_string()],
            completion_contract: SubGoalContract::from_definition_of_done(Some("Endpoint summary")),
            complexity_hint: None,
        },
        outcome: SubGoalOutcome::BudgetExhausted {
            partial_response: Some("Enough research to scaffold the skill.".to_string()),
        },
        signals: Vec::new(),
    };

    assert!(
        !should_halt_sub_goal_sequence(&result),
        "useful partial output should allow later sub-goals to continue"
    );
}

#[test]
fn build_sub_goal_snapshot_includes_prior_results_in_conversation_history() {
    let sub_goal = SubGoal {
        description: "Implement the skill".to_string(),
        required_tools: vec!["run_command".to_string()],
        completion_contract: SubGoalContract::from_definition_of_done(Some("Working skill")),
        complexity_hint: None,
    };
    let prior_results = vec![SubGoalResult {
        goal: SubGoal {
            description: "Research X API".to_string(),
            required_tools: vec!["web_search".to_string()],
            completion_contract: SubGoalContract::from_definition_of_done(Some("Spec")),
            complexity_hint: None,
        },
        outcome: SubGoalOutcome::BudgetExhausted {
            partial_response: Some("Endpoint, auth, and rate-limit details confirmed.".into()),
        },
        signals: Vec::new(),
    }];
    let snapshot = build_sub_goal_snapshot(&sub_goal, &prior_results, &[], 42);

    assert_eq!(
        snapshot.user_input.as_ref().expect("user input").text,
        "Implement the skill"
    );
    let last_message = snapshot
        .conversation_history
        .last()
        .expect("prior results context message");
    assert!(message_to_text(last_message).contains("Prior decomposition results for context only"));
    assert!(message_to_text(last_message).contains("Research X API"));
    assert!(
        message_to_text(last_message).contains("Endpoint, auth, and rate-limit details confirmed.")
    );
}

#[tokio::test]
async fn sub_goal_complete_without_required_side_effect_tool_is_rejected() {
    #[derive(Debug, Default)]
    struct SideEffectToolExecutor;

    #[async_trait]
    impl ToolExecutor for SideEffectToolExecutor {
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
                name: "run_command".to_string(),
                description: "Run a command".to_string(),
                parameters: serde_json::json!({"type":"object"}),
            }]
        }

        fn cacheability(&self, tool_name: &str) -> crate::act::ToolCacheability {
            match tool_name {
                "run_command" => crate::act::ToolCacheability::SideEffect,
                _ => crate::act::ToolCacheability::NeverCache,
            }
        }
    }

    let started_at_ms = current_time_ms();
    let engine = LoopEngine::builder()
        .budget(BudgetTracker::new(budget_config(20, 6), started_at_ms, 0))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(4)
        .tool_executor(Arc::new(SideEffectToolExecutor))
        .synthesis_instruction("Summarize tool output".to_string())
        .build()
        .expect("test engine build");
    let goal = SubGoal {
        description: "Scaffold the skill".to_string(),
        required_tools: vec!["run_command".to_string()],
        completion_contract: SubGoalContract::from_definition_of_done(Some("Scaffolded skill")),
        complexity_hint: None,
    };
    let llm = ScriptedLlm::new(vec![
        Ok(text_response(
            "Here's the complete implementation plan and code.",
        )),
        Ok(text_response(
            "I have enough context and would run it next.",
        )),
    ]);

    let execution = engine
        .run_sub_goal(&goal, BudgetConfig::default(), &llm, &[], &[])
        .await;

    let SubGoalOutcome::Incomplete(message) = &execution.result.outcome else {
        panic!("expected incomplete sub-goal outcome")
    };
    assert!(message.contains("completion evidence"), "{message}");
}

#[tokio::test]
async fn sub_goal_missing_required_side_effect_tool_gets_bounded_retry() {
    #[derive(Debug, Default)]
    struct SideEffectToolExecutor;

    #[async_trait]
    impl ToolExecutor for SideEffectToolExecutor {
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
                name: "run_command".to_string(),
                description: "Run a command".to_string(),
                parameters: serde_json::json!({"type":"object"}),
            }]
        }

        fn cacheability(&self, tool_name: &str) -> crate::act::ToolCacheability {
            match tool_name {
                "run_command" => crate::act::ToolCacheability::SideEffect,
                _ => crate::act::ToolCacheability::NeverCache,
            }
        }
    }

    let started_at_ms = current_time_ms();
    let engine = LoopEngine::builder()
        .budget(BudgetTracker::new(budget_config(20, 6), started_at_ms, 0))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(4)
        .tool_executor(Arc::new(SideEffectToolExecutor))
        .synthesis_instruction("Summarize tool output".to_string())
        .build()
        .expect("test engine build");
    let goal = SubGoal {
        description: "Scaffold the skill".to_string(),
        required_tools: vec!["run_command".to_string()],
        completion_contract: SubGoalContract::from_definition_of_done(Some("Scaffolded skill")),
        complexity_hint: None,
    };
    let llm = ScriptedLlm::new(vec![
        Ok(text_response("Scaffolded skill")),
        Ok(CompletionResponse {
            content: Vec::new(),
            tool_calls: vec![ToolCall {
                id: "call-1".to_string(),
                name: "run_command".to_string(),
                arguments: serde_json::json!({"command":"fawx skill create x-post"}),
            }],
            usage: None,
            stop_reason: Some("tool_use".to_string()),
        }),
        Ok(text_response("Scaffolded skill")),
    ]);

    let execution = engine
        .run_sub_goal(&goal, BudgetConfig::default(), &llm, &[], &[])
        .await;

    let SubGoalOutcome::Completed(response) = &execution.result.outcome else {
        panic!("expected completed sub-goal outcome")
    };
    assert_eq!(response, "Scaffolded skill");
    let used_tools = successful_tool_names(&execution.result.signals);
    assert!(used_tools.contains("run_command"));
}

#[tokio::test]
async fn observation_only_run_command_does_not_satisfy_required_side_effect_tool() {
    #[derive(Debug, Default)]
    struct ClassifiedRunCommandExecutor;

    #[async_trait]
    impl ToolExecutor for ClassifiedRunCommandExecutor {
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
                name: "run_command".to_string(),
                description: "Run a command".to_string(),
                parameters: serde_json::json!({"type":"object"}),
            }]
        }

        fn cacheability(&self, tool_name: &str) -> crate::act::ToolCacheability {
            match tool_name {
                "run_command" => crate::act::ToolCacheability::SideEffect,
                _ => crate::act::ToolCacheability::NeverCache,
            }
        }

        fn classify_call(&self, call: &ToolCall) -> ToolCallClassification {
            let command = call
                .arguments
                .get("command")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            if command.starts_with("ls ") || command.starts_with("cat ") {
                ToolCallClassification::Observation
            } else {
                ToolCallClassification::Mutation
            }
        }
    }

    let started_at_ms = current_time_ms();
    let engine = LoopEngine::builder()
        .budget(BudgetTracker::new(budget_config(20, 6), started_at_ms, 0))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(4)
        .tool_executor(Arc::new(ClassifiedRunCommandExecutor))
        .synthesis_instruction("Summarize tool output".to_string())
        .build()
        .expect("test engine build");
    let goal = SubGoal {
        description: "Scaffold the skill".to_string(),
        required_tools: vec!["run_command".to_string()],
        completion_contract: SubGoalContract::from_definition_of_done(Some("Scaffolded skill")),
        complexity_hint: None,
    };
    let llm = ScriptedLlm::new(vec![
        Ok(CompletionResponse {
            content: Vec::new(),
            tool_calls: vec![ToolCall {
                id: "call-1".to_string(),
                name: "run_command".to_string(),
                arguments: serde_json::json!({"command":"ls ~/fawx/skills"}),
            }],
            usage: None,
            stop_reason: Some("tool_use".to_string()),
        }),
        Ok(text_response("I inspected the skill directory.")),
        Ok(text_response("I still need to scaffold it.")),
    ]);

    let execution = engine
        .run_sub_goal(&goal, BudgetConfig::default(), &llm, &[], &[])
        .await;

    let SubGoalOutcome::Incomplete(message) = &execution.result.outcome else {
        panic!("expected incomplete sub-goal outcome")
    };
    assert!(message.contains("scaffold"), "{message}");
    let used_tools = successful_tool_names(&execution.result.signals);
    let used_mutation_tools = successful_mutation_tool_names(&execution.result.signals);
    assert!(used_tools.contains("run_command"));
    assert!(
        !used_mutation_tools.contains("run_command"),
        "read-only run_command should not satisfy required mutation work"
    );
}

#[tokio::test]
async fn backward_compat_no_complexity_hint() {
    let mut engine = decomposition_engine(budget_config(20, 6), 0);
    let response = CompletionResponse {
        content: Vec::new(),
        tool_calls: vec![decompose_tool_call(serde_json::json!({
            "sub_goals": [{"description": "Summarize findings"}],
            "strategy": "Sequential"
        }))],
        usage: None,
        stop_reason: None,
    };
    let decision = engine.decide(&response).await.expect("decision");
    let plan = match decision {
        Decision::Decompose(plan) => plan,
        other => panic!("expected decomposition, got: {other:?}"),
    };
    assert_eq!(plan.sub_goals[0].complexity_hint, None);

    let action = engine
        .execute_decomposition(
            &Decision::Decompose(plan.clone()),
            &plan,
            &ScriptedLlm::new(vec![Ok(text_response("Summary of findings"))]),
            &[],
        )
        .await
        .expect("decomposition");
    assert!(action
        .response_text
        .contains("completed: Summary of findings"));
}

#[test]
fn third_sequential_sub_goal_gets_viable_budget() {
    let engine = decomposition_engine(budget_config(20, 6), 0);
    let plan = decomposition_plan(&["first", "second", "third"]);
    let allocation = BudgetAllocator::new().allocate(
        &engine.budget,
        &plan.sub_goals,
        AllocationMode::Sequential,
        current_time_ms(),
    );
    let floor = crate::budget::BudgetFloor::default();
    let third = &allocation.sub_goal_budgets[2];

    assert!(!allocation.skipped_indices.contains(&2));
    assert!(third.max_llm_calls >= floor.min_llm_calls);
    assert!(third.max_tool_invocations >= floor.min_tool_invocations);
    assert!(third.max_tokens >= floor.min_tokens);
}

#[test]
fn nested_decomposition_all_leaves_get_floor_budget_or_skipped() {
    let root_engine = decomposition_engine(budget_config(20, 6), 0);
    let root_plan = decomposition_plan(&["branch-a", "branch-b"]);
    let allocator = BudgetAllocator::new();
    let root_allocation = allocator.allocate(
        &root_engine.budget,
        &root_plan.sub_goals,
        AllocationMode::Sequential,
        current_time_ms(),
    );
    let floor = crate::budget::BudgetFloor::default();

    for root_budget in root_allocation.sub_goal_budgets {
        let child_tracker = BudgetTracker::new(
            root_budget,
            current_time_ms(),
            root_engine.budget.child_depth(),
        );
        let leaf_goals = decomposition_plan(&["leaf-1", "leaf-2", "leaf-3"]).sub_goals;
        let leaf_allocation = allocator.allocate(
            &child_tracker,
            &leaf_goals,
            AllocationMode::Sequential,
            current_time_ms(),
        );

        for (index, budget) in leaf_allocation.sub_goal_budgets.iter().enumerate() {
            let skipped = leaf_allocation.skipped_indices.contains(&index);
            let viable = budget.max_llm_calls >= floor.min_llm_calls
                && budget.max_tool_invocations >= floor.min_tool_invocations
                && budget.max_tokens >= floor.min_tokens
                && budget.max_cost_cents >= floor.min_cost_cents
                && budget.max_wall_time_ms >= floor.min_wall_time_ms;
            assert!(skipped || viable, "leaf {index} must be viable or skipped");
        }
    }
}

#[tokio::test]
async fn execute_decomposition_blocks_when_effective_cap_zero() {
    let mut engine = decomposition_engine(budget_config_with_mode(6, 8, DepthMode::Adaptive), 0);
    let plan = decomposition_plan(&["depth-capped"]);
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
async fn execute_decomposition_blocks_when_current_depth_meets_effective_cap() {
    let mut engine = decomposition_engine(budget_config_with_mode(20, 8, DepthMode::Adaptive), 2);
    let plan = decomposition_plan(&["depth-capped"]);
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

#[test]
fn child_budget_inherits_effective_cap_in_adaptive_mode() {
    let engine = decomposition_engine(budget_config_with_mode(8, 8, DepthMode::Adaptive), 0);
    let remaining = engine.budget.remaining(current_time_ms());
    let effective_cap = engine.effective_decomposition_depth_cap(&remaining);
    let plan = decomposition_plan(&["single-child"]);
    let allocator = BudgetAllocator::new();
    let mut allocation = allocator.allocate(
        &engine.budget,
        &plan.sub_goals,
        AllocationMode::Sequential,
        current_time_ms(),
    );

    engine.apply_effective_depth_cap(&mut allocation.sub_goal_budgets, effective_cap);

    assert_eq!(effective_cap, 1);
    assert_eq!(allocation.sub_goal_budgets[0].max_recursion_depth, 1);
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

    let allocation = AllocationPlan {
        sub_goal_budgets: Vec::new(),
        parent_continuation_budget: budget_config(20, 6),
        skipped_indices: Vec::new(),
    };
    let results = engine
        .execute_sub_goals_concurrent(&plan, &allocation, &llm, &[])
        .await;

    assert!(results.is_empty());
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "unexpected missing result at index 0")]
fn collect_concurrent_results_panics_for_unexpected_missing_slot() {
    let mut engine = decomposition_engine(budget_config(20, 6), 0);
    let plan = decomposition_plan(&["missing"]);

    let _ = engine.collect_concurrent_results(&plan, Vec::new(), &[false]);
}
