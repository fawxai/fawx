use super::*;
use crate::act::ToolResult;
use crate::budget::BudgetConfig;
use async_trait::async_trait;
use fx_decompose::{AggregationStrategy, ComplexityHint, DecompositionPlan, SubGoal};
use fx_llm::{
    CompletionRequest, CompletionResponse, ContentBlock, ProviderError, ToolCall, ToolDefinition,
};

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

/// LLM that returns a text response (needed for act_with_tools continuation).
#[derive(Debug)]
struct TextLlm;

#[async_trait]
impl LlmProvider for TextLlm {
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
        "text-llm"
    }

    async fn complete(&self, _: CompletionRequest) -> Result<CompletionResponse, ProviderError> {
        Ok(CompletionResponse {
            content: vec![ContentBlock::Text {
                text: "done".to_string(),
            }],
            tool_calls: vec![],
            usage: Default::default(),
            stop_reason: None,
        })
    }
}

fn gate_engine(config: BudgetConfig) -> LoopEngine {
    let started_at_ms = current_time_ms();
    LoopEngine::builder()
        .budget(BudgetTracker::new(config, started_at_ms, 0))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(4)
        .tool_executor(Arc::new(PassiveToolExecutor))
        .synthesis_instruction("Summarize tool output".to_string())
        .build()
        .expect("test engine build")
}

fn unroutable_gate_engine(config: BudgetConfig) -> LoopEngine {
    #[derive(Debug, Default)]
    struct UnroutableToolExecutor;

    #[async_trait]
    impl ToolExecutor for UnroutableToolExecutor {
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

    let started_at_ms = current_time_ms();
    LoopEngine::builder()
        .budget(BudgetTracker::new(config, started_at_ms, 0))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(4)
        .tool_executor(Arc::new(UnroutableToolExecutor))
        .synthesis_instruction("Summarize tool output".to_string())
        .build()
        .expect("test engine build")
}

fn sub_goal(description: &str, tools: &[&str], hint: Option<ComplexityHint>) -> SubGoal {
    SubGoal {
        description: description.to_string(),
        required_tools: tools.iter().map(|t| (*t).to_string()).collect(),
        completion_contract: SubGoalContract::from_definition_of_done(None),
        complexity_hint: hint,
    }
}

fn plan(sub_goals: Vec<SubGoal>) -> DecompositionPlan {
    DecompositionPlan {
        sub_goals,
        strategy: AggregationStrategy::Parallel,
        truncated_from: None,
    }
}

// --- Batch detection tests (1-5) ---

/// Test 1: Plan with 5 sub-goals all requiring `["read_file"]` → batch detected.
#[tokio::test]
async fn batch_detected_all_same_single_tool() {
    let config = BudgetConfig::default();
    let mut engine = gate_engine(config);
    let llm = TextLlm;
    let p = plan(vec![
        sub_goal("read a", &["read_file"], None),
        sub_goal("read b", &["read_file"], None),
        sub_goal("read c", &["read_file"], None),
        sub_goal("read d", &["read_file"], None),
        sub_goal("read e", &["read_file"], None),
    ]);
    let decision = Decision::Decompose(p.clone());

    let result = engine
        .evaluate_decompose_gates(&p, &decision, &llm, &[])
        .await;

    assert!(result.is_some(), "batch gate should fire");
    let signals = engine.signals.drain_all();
    assert!(
        signals
            .iter()
            .any(|s| s.message == "decompose_batch_detected"),
        "should emit batch trace signal"
    );
}

/// Test 2: Different tools → batch NOT detected.
#[tokio::test]
async fn batch_not_detected_different_tools() {
    let config = BudgetConfig::default();
    let mut engine = gate_engine(config);
    let llm = TextLlm;
    let p = plan(vec![
        sub_goal("read a", &["read_file"], None),
        sub_goal("read b", &["read_file"], None),
        sub_goal("write c", &["write_file"], None),
    ]);
    let decision = Decision::Decompose(p.clone());

    let _result = engine
        .evaluate_decompose_gates(&p, &decision, &llm, &[])
        .await;

    // Should not fire batch gate; might fire floor or cost or none.
    let signals = engine.signals.drain_all();
    assert!(
        !signals
            .iter()
            .any(|s| s.message == "decompose_batch_detected"),
        "should NOT emit batch trace signal with different tools"
    );
}

/// Test 3: Single sub-goal → NOT a batch (len == 1).
#[tokio::test]
async fn batch_not_detected_single_sub_goal() {
    let config = BudgetConfig::default();
    let mut engine = gate_engine(config);
    let llm = TextLlm;
    let p = plan(vec![sub_goal("read a", &["read_file"], None)]);
    let decision = Decision::Decompose(p.clone());

    let _result = engine
        .evaluate_decompose_gates(&p, &decision, &llm, &[])
        .await;

    let signals = engine.signals.drain_all();
    assert!(
        !signals
            .iter()
            .any(|s| s.message == "decompose_batch_detected"),
        "single sub-goal is not a batch"
    );
}

/// Test 4: Multi-tool per sub-goal → NOT a batch.
#[tokio::test]
async fn batch_not_detected_multi_tool_per_sub_goal() {
    let config = BudgetConfig::default();
    let mut engine = gate_engine(config);
    let llm = TextLlm;
    let p = plan(vec![
        sub_goal("task a", &["search_text", "read_file"], None),
        sub_goal("task b", &["search_text", "read_file"], None),
        sub_goal("task c", &["search_text", "read_file"], None),
        sub_goal("task d", &["search_text", "read_file"], None),
    ]);
    let decision = Decision::Decompose(p.clone());

    let _result = engine
        .evaluate_decompose_gates(&p, &decision, &llm, &[])
        .await;

    let signals = engine.signals.drain_all();
    assert!(
        !signals
            .iter()
            .any(|s| s.message == "decompose_batch_detected"),
        "multi-tool sub-goals are not a batch"
    );
}

#[tokio::test]
async fn batch_gate_skips_direct_route_when_executor_cannot_materialize_calls() {
    let config = BudgetConfig::default();
    let mut engine = unroutable_gate_engine(config);
    let llm = TextLlm;
    let p = plan(vec![
        sub_goal(
            "create skill a",
            &["run_command"],
            Some(ComplexityHint::Trivial),
        ),
        sub_goal(
            "create skill b",
            &["run_command"],
            Some(ComplexityHint::Trivial),
        ),
    ]);
    let decision = Decision::Decompose(p.clone());

    let result = engine
        .evaluate_decompose_gates(&p, &decision, &llm, &[])
        .await;

    assert!(
        result.is_none(),
        "unsupported direct-routing should fall back to normal decomposition"
    );
    let signals = engine.signals.drain_all();
    assert!(
        !signals
            .iter()
            .any(|s| s.message == "decompose_batch_detected"),
        "batch gate should not short-circuit when calls cannot be materialized"
    );
}

#[tokio::test]
async fn child_engine_disables_decompose_when_sub_goal_declares_required_tools() {
    let config = BudgetConfig::default();
    let engine = gate_engine(config.clone());
    let timestamp_ms = current_time_ms();
    let budget = BudgetTracker::new(config, timestamp_ms, 0);
    let required_tool_goal = sub_goal(
        "research the API",
        &["web_search", "web_fetch"],
        Some(ComplexityHint::Moderate),
    );
    let free_form_goal = sub_goal(
        "reason about next steps",
        &[],
        Some(ComplexityHint::Moderate),
    );

    let child = engine
        .build_child_engine(&required_tool_goal, budget.clone())
        .expect("child engine");
    assert_eq!(child.execution_visibility, ExecutionVisibility::Internal);
    assert!(
        !child.decompose_enabled,
        "sub-goals with required tools should not re-advertise decompose"
    );

    let free_form_child = engine
        .build_child_engine(&free_form_goal, budget)
        .expect("free-form child engine");
    assert_eq!(
        free_form_child.execution_visibility,
        ExecutionVisibility::Internal
    );
    assert!(
        free_form_child.decompose_enabled,
        "sub-goals without required tools may still decompose"
    );
}

#[test]
fn internal_child_suppresses_public_event_bus_messages() {
    let config = BudgetConfig::default();
    let bus = fx_core::EventBus::new(16);
    let mut rx = bus.subscribe();
    let started_at_ms = current_time_ms();
    let parent = LoopEngine::builder()
        .budget(BudgetTracker::new(config.clone(), started_at_ms, 0))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(4)
        .tool_executor(Arc::new(PassiveToolExecutor))
        .synthesis_instruction("Summarize tool output".to_string())
        .event_bus(bus)
        .build()
        .expect("test engine build");
    let goal = sub_goal(
        "reason about next steps",
        &[],
        Some(ComplexityHint::Moderate),
    );
    let budget = BudgetTracker::new(config, current_time_ms(), 0);
    let mut child = parent
        .build_child_engine(&goal, budget)
        .expect("child engine");

    child.publish_stream_started(StreamPhase::Reason);
    child.publish_tool_use(&ToolCall {
        id: "call-1".to_string(),
        name: "read_file".to_string(),
        arguments: serde_json::json!({"path":"README.md"}),
    });
    child.publish_tool_result(&ToolResult {
        tool_call_id: "call-1".to_string(),
        tool_name: "read_file".to_string(),
        success: true,
        output: "ok".to_string(),
    });
    child.publish_stream_finished(StreamPhase::Reason);

    assert!(
        rx.try_recv().is_err(),
        "internal child should be silent on the public bus"
    );
}

#[tokio::test]
async fn child_engine_scopes_tool_surface_to_required_tools() {
    #[derive(Debug, Default)]
    struct SurfaceToolExecutor;

    #[async_trait]
    impl ToolExecutor for SurfaceToolExecutor {
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
            vec![
                ToolDefinition {
                    name: "search_text".to_string(),
                    description: "Search repository text".to_string(),
                    parameters: serde_json::json!({
                        "type": "object",
                        "properties": {"pattern": {"type": "string"}},
                        "required": ["pattern"]
                    }),
                },
                ToolDefinition {
                    name: "current_time".to_string(),
                    description: "Get the current time".to_string(),
                    parameters: serde_json::json!({
                        "type": "object",
                        "properties": {},
                        "required": []
                    }),
                },
            ]
        }
    }

    let config = BudgetConfig::default();
    let started_at_ms = current_time_ms();
    let engine = LoopEngine::builder()
        .budget(BudgetTracker::new(config.clone(), started_at_ms, 0))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(4)
        .tool_executor(Arc::new(SurfaceToolExecutor))
        .synthesis_instruction("Summarize tool output".to_string())
        .build()
        .expect("test engine build");
    let child_budget = BudgetTracker::new(config, current_time_ms(), 0);
    let goal = sub_goal(
        "Search for X API endpoints",
        &["search_text"],
        Some(ComplexityHint::Moderate),
    );

    let child = engine
        .build_child_engine(&goal, child_budget)
        .expect("child engine");
    let tool_names: Vec<String> = child
        .tool_executor
        .tool_definitions()
        .into_iter()
        .map(|tool| tool.name)
        .collect();
    assert_eq!(tool_names, vec!["search_text"]);

    let blocked = child
        .tool_executor
        .execute_tools(
            &[ToolCall {
                id: "call-1".to_string(),
                name: "current_time".to_string(),
                arguments: serde_json::json!({}),
            }],
            None,
        )
        .await
        .expect("blocked result");
    assert_eq!(blocked.len(), 1);
    assert!(!blocked[0].success);
    assert!(blocked[0].output.contains("search_text"));
}

#[tokio::test]
async fn decide_drops_disallowed_decompose_tool_call_to_text_response() {
    let config = BudgetConfig::default();
    let started_at_ms = current_time_ms();
    let mut engine = LoopEngine::builder()
        .budget(BudgetTracker::new(config, started_at_ms, 0))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(4)
        .tool_executor(Arc::new(PassiveToolExecutor))
        .synthesis_instruction("Summarize tool output".to_string())
        .allow_decompose(false)
        .build()
        .expect("test engine build");
    let response = CompletionResponse {
        content: vec![ContentBlock::Text {
            text: "Proceed with implementation.".to_string(),
        }],
        tool_calls: vec![ToolCall {
            id: "decompose-1".to_string(),
            name: DECOMPOSE_TOOL_NAME.to_string(),
            arguments: serde_json::json!({
                "sub_goals": [{"description": "nested"}]
            }),
        }],
        usage: Default::default(),
        stop_reason: None,
    };

    let decision = engine.decide(&response).await.expect("decision");
    assert_eq!(
        decision,
        Decision::Respond("Proceed with implementation.".to_string())
    );
}

/// Test 5: Batch with 8 sub-goals and max_fan_out=4 → fan-out cap applied.
#[tokio::test]
async fn batch_respects_fan_out_cap() {
    let config = BudgetConfig {
        max_fan_out: 4,
        ..BudgetConfig::default()
    };
    let mut engine = gate_engine(config);
    let llm = TextLlm;
    let p = plan(vec![
        sub_goal("read 1", &["read_file"], None),
        sub_goal("read 2", &["read_file"], None),
        sub_goal("read 3", &["read_file"], None),
        sub_goal("read 4", &["read_file"], None),
        sub_goal("read 5", &["read_file"], None),
        sub_goal("read 6", &["read_file"], None),
        sub_goal("read 7", &["read_file"], None),
        sub_goal("read 8", &["read_file"], None),
    ]);
    let decision = Decision::Decompose(p.clone());

    let result = engine
        .evaluate_decompose_gates(&p, &decision, &llm, &[])
        .await;

    assert!(result.is_some(), "batch gate should fire");
    let _action = result.unwrap().expect("should succeed");
    // act_with_tools applies fan-out cap — should have deferred some
    let signals = engine.signals.drain_all();
    assert!(
        signals
            .iter()
            .any(|s| s.message == "decompose_batch_detected"),
        "batch detected signal emitted"
    );
    // Fan-out cap of 4 means 4 executed + 4 deferred
    assert!(
        signals
            .iter()
            .any(|s| s.message.contains("fan-out") || s.metadata.get("deferred").is_some()),
        "fan-out cap should have been applied: {signals:?}"
    );
}

// --- Complexity floor tests (6-8) ---

/// Test 6: Trivial sub-goals with different tools → complexity floor triggers.
#[tokio::test]
async fn complexity_floor_triggers_for_trivial_different_tools() {
    let config = BudgetConfig::default();
    let mut engine = gate_engine(config);
    let llm = TextLlm;
    // Short descriptions, exactly 1 tool each, different tools → trivial but not batch
    let p = plan(vec![
        sub_goal("check a", &["tool_a"], Some(ComplexityHint::Trivial)),
        sub_goal("check b", &["tool_b"], Some(ComplexityHint::Trivial)),
        sub_goal("check c", &["tool_c"], Some(ComplexityHint::Trivial)),
    ]);
    let decision = Decision::Decompose(p.clone());

    let result = engine
        .evaluate_decompose_gates(&p, &decision, &llm, &[])
        .await;

    assert!(result.is_some(), "complexity floor should fire");
    let signals = engine.signals.drain_all();
    assert!(
        signals
            .iter()
            .any(|s| s.message == "decompose_complexity_floor"),
        "should emit complexity floor signal"
    );
}

/// Test 7: 2 trivial + 1 moderate → floor does NOT trigger.
#[tokio::test]
async fn complexity_floor_does_not_trigger_with_moderate() {
    let config = BudgetConfig::default();
    let mut engine = gate_engine(config);
    let llm = TextLlm;
    let p = plan(vec![
        sub_goal("check a", &["tool_a"], Some(ComplexityHint::Trivial)),
        sub_goal("check b", &["tool_b"], Some(ComplexityHint::Trivial)),
        sub_goal("big task", &["tool_c"], Some(ComplexityHint::Moderate)),
    ]);
    let decision = Decision::Decompose(p.clone());

    let _result = engine
        .evaluate_decompose_gates(&p, &decision, &llm, &[])
        .await;

    let signals = engine.signals.drain_all();
    assert!(
        !signals
            .iter()
            .any(|s| s.message == "decompose_complexity_floor"),
        "should NOT emit complexity floor signal with moderate sub-goal"
    );
}

/// Test 8: All single-tool but one Complex → floor does NOT trigger.
#[tokio::test]
async fn complexity_floor_does_not_trigger_with_complex() {
    let config = BudgetConfig::default();
    let mut engine = gate_engine(config);
    let llm = TextLlm;
    let p = plan(vec![
        sub_goal("a", &["tool_a"], Some(ComplexityHint::Trivial)),
        sub_goal("b", &["tool_b"], Some(ComplexityHint::Trivial)),
        sub_goal("c", &["tool_c"], Some(ComplexityHint::Complex)),
    ]);
    let decision = Decision::Decompose(p.clone());

    let _result = engine
        .evaluate_decompose_gates(&p, &decision, &llm, &[])
        .await;

    let signals = engine.signals.drain_all();
    assert!(
        !signals
            .iter()
            .any(|s| s.message == "decompose_complexity_floor"),
        "should NOT emit complexity floor signal with complex sub-goal"
    );
}

// --- Cost gate tests (9-13) ---

/// Test 9: Plan at 200 cents, remaining 100 → rejected (200 > 150).
#[tokio::test]
async fn cost_gate_rejects_over_150_percent() {
    let config = BudgetConfig {
        max_cost_cents: 100,
        ..BudgetConfig::default()
    };
    let mut engine = gate_engine(config);
    let llm = TextLlm;
    // 25 moderate sub-goals × 2 tools each = 25*(2*2 + 2*1) = 25*6 = 150 cents
    // We need ~200 cents estimated. 25 complex sub-goals × 1 tool = 25*(4*2+1*1) = 25*9=225
    // Simpler: use complexity hints directly
    // 4 complex sub-goals with 2 tools each: 4*(4*2 + 2*1) = 4*10 = 40? No.
    // Let's be precise: Complex = 4 LLM calls. Each LLM = 2 cents. Each tool = 1 cent.
    // So complex + 2 tools = 4*2 + 2*1 = 10 cents per sub-goal.
    // 20 sub-goals × 10 = 200 cents. Remaining = 100 cents. 200 > 150. ✓
    let sub_goals: Vec<SubGoal> = (0..20)
        .map(|i| {
            sub_goal(
                &format!("task {i}"),
                &["t1", "t2"],
                Some(ComplexityHint::Complex),
            )
        })
        .collect();
    let p = plan(sub_goals);
    let decision = Decision::Decompose(p.clone());

    let result = engine
        .evaluate_decompose_gates(&p, &decision, &llm, &[])
        .await;

    assert!(result.is_some(), "cost gate should fire");
    let action = result.unwrap().expect("should succeed");
    assert!(
        action.response_text.contains("rejected"),
        "response should mention rejection"
    );
}

/// Test 10: Plan at 140 cents, remaining 100 → NOT rejected (140 ≤ 150).
#[tokio::test]
async fn cost_gate_allows_under_150_percent() {
    let config = BudgetConfig {
        max_cost_cents: 100,
        ..BudgetConfig::default()
    };
    let mut engine = gate_engine(config);
    let llm = TextLlm;
    // 14 sub-goals, each complex with 2 tools = 14 * 10 = 140 cents
    let sub_goals: Vec<SubGoal> = (0..14)
        .map(|i| {
            sub_goal(
                &format!("task {i}"),
                &["t1", "t2"],
                Some(ComplexityHint::Complex),
            )
        })
        .collect();
    let p = plan(sub_goals);
    let decision = Decision::Decompose(p.clone());

    let _result = engine
        .evaluate_decompose_gates(&p, &decision, &llm, &[])
        .await;

    let signals = engine.signals.drain_all();
    assert!(
        !signals.iter().any(|s| s.message == "decompose_cost_gate"),
        "cost gate should NOT fire for 140 cents with 100 remaining (140 ≤ 150)"
    );
}

/// Test 11: Boundary test — estimate just above 150% threshold → rejected (151 > 150).
#[tokio::test]
async fn cost_gate_rejects_at_boundary() {
    // remaining=6, threshold=6*3/2=9, estimate=10 (166%) → 10 > 9 → rejected.
    let config = BudgetConfig {
        max_cost_cents: 6,
        ..BudgetConfig::default()
    };
    let mut engine = gate_engine(config);
    let llm = TextLlm;
    // 1 complex sub-goal + 2 tools = 4*2 + 2*1 = 10 cents
    // remaining=6, threshold=6*3/2=9, 10 > 9 → rejected
    let p = plan(vec![sub_goal(
        "big task",
        &["t1", "t2"],
        Some(ComplexityHint::Complex),
    )]);
    let decision = Decision::Decompose(p.clone());

    let result = engine
        .evaluate_decompose_gates(&p, &decision, &llm, &[])
        .await;

    assert!(result.is_some(), "cost gate should fire (10 > 9)");
    let signals = engine.signals.drain_all();
    assert!(
        signals.iter().any(|s| s.message == "decompose_cost_gate"),
        "should emit cost gate blocked signal"
    );
}

/// Test 11b: Boundary — estimate at exactly the threshold → NOT rejected.
///
/// remaining=7, threshold=7*3/2=10, estimate=10 → 10 ≤ 10 → passes.
#[tokio::test]
async fn cost_gate_allows_at_exact_boundary() {
    let config = BudgetConfig {
        max_cost_cents: 7,
        ..BudgetConfig::default()
    };
    let mut engine = gate_engine(config);
    let llm = TextLlm;
    // 1 complex sub-goal + 2 tools = 10 cents
    let p = plan(vec![sub_goal(
        "big task",
        &["t1", "t2"],
        Some(ComplexityHint::Complex),
    )]);
    let decision = Decision::Decompose(p.clone());

    let _result = engine
        .evaluate_decompose_gates(&p, &decision, &llm, &[])
        .await;

    let signals = engine.signals.drain_all();
    assert!(
        !signals.iter().any(|s| s.message == "decompose_cost_gate"),
        "cost gate should NOT fire (10 <= 10)"
    );
}

/// Test 12: Rejected plan produces SignalKind::Blocked with cost metadata.
#[tokio::test]
async fn cost_gate_emits_blocked_signal_with_metadata() {
    let config = BudgetConfig {
        max_cost_cents: 10,
        ..BudgetConfig::default()
    };
    let mut engine = gate_engine(config);
    let llm = TextLlm;
    // 5 complex + 2 tools each = 5*10 = 50 cents. remaining=10, threshold=15. 50>15 ✓
    let sub_goals: Vec<SubGoal> = (0..5)
        .map(|i| {
            sub_goal(
                &format!("task {i}"),
                &["t1", "t2"],
                Some(ComplexityHint::Complex),
            )
        })
        .collect();
    let p = plan(sub_goals);
    let decision = Decision::Decompose(p.clone());

    let _ = engine
        .evaluate_decompose_gates(&p, &decision, &llm, &[])
        .await;

    let signals = engine.signals.drain_all();
    let blocked = signals
        .iter()
        .find(|s| s.kind == SignalKind::Blocked && s.message == "decompose_cost_gate");
    assert!(blocked.is_some(), "should emit Blocked signal");
    let metadata = &blocked.unwrap().metadata;
    assert!(
        metadata.get("estimated_cost_cents").is_some(),
        "metadata should include estimated_cost_cents"
    );
    assert!(
        metadata.get("remaining_cost_cents").is_some(),
        "metadata should include remaining_cost_cents"
    );
}

/// Test 13: Rejected plan's ActionResult text mentions cost rejection.
#[tokio::test]
async fn cost_gate_action_result_mentions_rejection() {
    let config = BudgetConfig {
        max_cost_cents: 10,
        ..BudgetConfig::default()
    };
    let mut engine = gate_engine(config);
    let llm = TextLlm;
    let sub_goals: Vec<SubGoal> = (0..5)
        .map(|i| {
            sub_goal(
                &format!("task {i}"),
                &["t1", "t2"],
                Some(ComplexityHint::Complex),
            )
        })
        .collect();
    let p = plan(sub_goals);
    let decision = Decision::Decompose(p.clone());

    let result = engine
        .evaluate_decompose_gates(&p, &decision, &llm, &[])
        .await;

    let action = result.unwrap().expect("should succeed");
    assert!(
        action.response_text.contains("cost")
            || action.response_text.contains("rejected")
            || action.response_text.contains("budget"),
        "response text should mention cost rejection: {}",
        action.response_text
    );
}

// --- Gate ordering tests (14-15) ---

/// Test 14: Plan triggers both batch detection AND cost gate → batch wins.
#[tokio::test]
async fn batch_gate_takes_precedence_over_cost_gate() {
    let config = BudgetConfig {
        max_cost_cents: 1, // Very low budget to ensure cost gate would fire
        ..BudgetConfig::default()
    };
    let mut engine = gate_engine(config);
    let llm = TextLlm;
    // All same tool → batch. But cost is also over budget.
    let p = plan(vec![
        sub_goal("read 1", &["read_file"], Some(ComplexityHint::Trivial)),
        sub_goal("read 2", &["read_file"], Some(ComplexityHint::Trivial)),
        sub_goal("read 3", &["read_file"], Some(ComplexityHint::Trivial)),
    ]);
    let decision = Decision::Decompose(p.clone());

    let result = engine
        .evaluate_decompose_gates(&p, &decision, &llm, &[])
        .await;

    assert!(result.is_some(), "a gate should fire");
    let signals = engine.signals.drain_all();
    assert!(
        signals
            .iter()
            .any(|s| s.message == "decompose_batch_detected"),
        "batch detection should win over cost gate"
    );
    assert!(
        !signals.iter().any(|s| s.message == "decompose_cost_gate"),
        "cost gate should NOT fire when batch already caught it"
    );
}

/// Test 15: Gates evaluated in order: batch → floor → cost. First match short-circuits.
#[tokio::test]
async fn gates_evaluated_in_order_first_match_wins() {
    let config = BudgetConfig {
        max_cost_cents: 1, // Very low budget
        ..BudgetConfig::default()
    };
    let mut engine = gate_engine(config);
    let llm = TextLlm;
    // Different tools but all trivial → not batch, but floor triggers.
    // Also cost would fire due to low budget.
    let p = plan(vec![
        sub_goal("a", &["tool_a"], Some(ComplexityHint::Trivial)),
        sub_goal("b", &["tool_b"], Some(ComplexityHint::Trivial)),
    ]);
    let decision = Decision::Decompose(p.clone());

    let result = engine
        .evaluate_decompose_gates(&p, &decision, &llm, &[])
        .await;

    assert!(result.is_some(), "a gate should fire");
    let signals = engine.signals.drain_all();
    assert!(
        signals
            .iter()
            .any(|s| s.message == "decompose_complexity_floor"),
        "complexity floor should fire before cost gate"
    );
    assert!(
        !signals.iter().any(|s| s.message == "decompose_cost_gate"),
        "cost gate should NOT fire when floor already caught it"
    );
}

// --- Edge case tests ---

/// Empty plan (0 sub-goals) → estimate returns default cost → passes all gates.
#[tokio::test]
async fn empty_plan_passes_all_gates() {
    let config = BudgetConfig {
        max_cost_cents: 1,
        ..BudgetConfig::default()
    };
    let mut engine = gate_engine(config);
    let llm = TextLlm;
    let p = plan(vec![]);
    let decision = Decision::Decompose(p.clone());

    let result = engine
        .evaluate_decompose_gates(&p, &decision, &llm, &[])
        .await;

    assert!(result.is_none(), "no gate should fire for empty plan");
    let cost = estimate_plan_cost(&p);
    assert_eq!(cost.cost_cents, 0, "empty plan cost should be 0");
}

/// All-trivial sub-goals with Sequential strategy → complexity floor does NOT trigger.
/// Proves the Parallel-only design decision for the floor gate.
#[tokio::test]
async fn sequential_strategy_excludes_complexity_floor() {
    let config = BudgetConfig::default();
    let mut engine = gate_engine(config);
    let llm = TextLlm;
    let p = DecompositionPlan {
        sub_goals: vec![
            sub_goal("a", &["tool_a"], Some(ComplexityHint::Trivial)),
            sub_goal("b", &["tool_b"], Some(ComplexityHint::Trivial)),
            sub_goal("c", &["tool_c"], Some(ComplexityHint::Trivial)),
        ],
        strategy: AggregationStrategy::Sequential,
        truncated_from: None,
    };
    let decision = Decision::Decompose(p.clone());

    let _result = engine
        .evaluate_decompose_gates(&p, &decision, &llm, &[])
        .await;

    let signals = engine.signals.drain_all();
    assert!(
        !signals
            .iter()
            .any(|s| s.message == "decompose_complexity_floor"),
        "complexity floor must NOT trigger for Sequential strategy"
    );
}

// --- estimate_plan_cost unit tests ---
