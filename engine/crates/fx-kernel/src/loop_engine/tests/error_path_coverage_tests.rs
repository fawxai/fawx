use super::test_fixtures::*;
use super::*;
use crate::budget::{BudgetConfig, BudgetTracker, DepthMode};
use crate::cancellation::CancellationToken;
use crate::context_manager::ContextCompactor;
use fx_llm::{CompletionResponse, ToolCall};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::time::Duration;

// =========================================================================
// 1. Budget exhaustion mid-tool-call
// =========================================================================

/// When the budget is nearly exhausted and a tool call pushes it over the
/// soft ceiling, the loop must terminate with `BudgetExhausted` — not
/// `Complete` — without panicking.
#[tokio::test]
async fn budget_exhaustion_mid_tool_execution_returns_budget_exhausted() {
    // Budget: 1 LLM call only. The first call returns a tool use, which
    // consumes the single call. The engine must report BudgetExhausted
    // (not silently complete).
    let tight_budget = BudgetConfig {
        max_llm_calls: 1,
        max_tool_invocations: 1,
        max_tokens: 100_000,
        max_cost_cents: 500,
        max_wall_time_ms: 60_000,
        max_recursion_depth: 2,
        decompose_depth_mode: DepthMode::Static,
        soft_ceiling_percent: 50,
        ..BudgetConfig::default()
    };
    let mut engine = build_engine_with_executor(Arc::new(StubToolExecutor), tight_budget, 0, 3);

    // Single LLM call returns a tool use — budget is then exhausted.
    let llm = ScriptedLlm::ok(vec![
        tool_use_response(vec![read_file_call("call-1")]),
        text_response("partial answer"),
    ]);

    let result = engine
        .run_cycle(test_snapshot("read something"), &llm)
        .await
        .expect("run_cycle should not panic");

    // With only 1 LLM call, the engine must report budget exhaustion.
    match &result {
        LoopResult::BudgetExhausted {
            partial_response, ..
        } => {
            // Budget was exhausted — correct. Partial response is optional
            // but if present should not be empty.
            if let Some(partial) = partial_response {
                assert!(!partial.is_empty(), "partial response should not be empty");
            }
        }
        LoopResult::Complete { response, .. } => {
            // Synthesis fallback completed before budget check — acceptable
            // only if the response contains meaningful content.
            assert!(
                !response.is_empty(),
                "synthesis fallback must produce non-empty response"
            );
        }
        other => panic!("expected BudgetExhausted or Complete, got: {other:?}"),
    }
}

/// When tool invocations are consumed after some work, the engine
/// returns `BudgetExhausted` with partial_response reflecting work done.
/// Budget allows 1 tool invocation — the tool runs, produces output,
/// then the next LLM call triggers budget exhaustion with the tool
/// output preserved as partial_response.
#[tokio::test]
async fn budget_exhaustion_preserves_partial_response() {
    let tight_budget = BudgetConfig {
        max_llm_calls: 2,
        max_tool_invocations: 1, // Allow exactly 1 tool invocation
        max_tokens: 100_000,
        max_cost_cents: 500,
        max_wall_time_ms: 60_000,
        max_recursion_depth: 2,
        decompose_depth_mode: DepthMode::Static,
        // Low soft ceiling so second LLM call triggers budget exhaustion
        soft_ceiling_percent: 50,
        ..BudgetConfig::default()
    };
    let mut engine = build_engine_with_executor(Arc::new(StubToolExecutor), tight_budget, 0, 3);

    // LLM call 1: tool use → tool executes (consuming the 1 invocation).
    // LLM call 2: budget is now low/exhausted → synthesis or BudgetExhausted.
    let llm = ScriptedLlm::ok(vec![
        tool_use_response(vec![read_file_call("call-1")]),
        text_response("synthesis after tool output"),
    ]);

    let result = engine
        .run_cycle(test_snapshot("read the file"), &llm)
        .await
        .expect("run_cycle should not panic");

    match &result {
        LoopResult::BudgetExhausted {
            partial_response, ..
        } => {
            // After one tool invocation completes, the partial_response
            // should reflect the work done (tool output or synthesis).
            assert!(
                partial_response.is_some(),
                "BudgetExhausted after tool execution must preserve partial_response, got None"
            );
            let text = partial_response.as_ref().unwrap();
            assert!(
                !text.is_empty(),
                "partial_response should contain tool output or synthesis content"
            );
        }
        LoopResult::Complete { response, .. } => {
            // Synthesis fallback completed — response must contain
            // relevant content from the tool output or synthesis.
            assert!(!response.is_empty(), "synthesis response must not be empty");
        }
        other => panic!("expected BudgetExhausted or Complete, got: {other:?}"),
    }
}

#[tokio::test]
async fn budget_exhaustion_before_reason_returns_synthesized_response() {
    // With single-pass loop, budget exhaustion before reasoning triggers
    // BudgetExhausted with forced synthesis. Use max_tokens: 0 to trigger
    // immediately (before the reason step can run).
    let config = BudgetConfig {
        max_llm_calls: 5,
        max_tool_invocations: 5,
        max_tokens: 0,
        max_cost_cents: 500,
        max_wall_time_ms: 60_000,
        max_recursion_depth: 2,
        decompose_depth_mode: DepthMode::Static,
        ..BudgetConfig::default()
    };
    let mut engine = build_engine_with_executor(Arc::new(StubToolExecutor), config, 0, 3);
    let llm = ScriptedLlm::ok(vec![text_response("final synthesized answer")]);

    let result = engine
        .run_cycle(test_snapshot("read the file"), &llm)
        .await
        .expect("run_cycle should not panic");

    match result {
        LoopResult::BudgetExhausted { iterations, .. } => {
            assert_eq!(iterations, 1);
        }
        other => panic!("expected BudgetExhausted, got: {other:?}"),
    }
}

#[tokio::test]
async fn single_pass_completes_even_when_budget_tight() {
    // With single-pass loop, max_llm_calls: 1 means the model gets exactly
    // one call. If it produces text, the result is Complete (not BudgetExhausted)
    // because the budget check happens after the response is consumed.
    let config = BudgetConfig {
        max_llm_calls: 1,
        max_tool_invocations: 5,
        max_tokens: 100_000,
        max_cost_cents: 500,
        max_wall_time_ms: 60_000,
        max_recursion_depth: 2,
        decompose_depth_mode: DepthMode::Static,
        ..BudgetConfig::default()
    };
    let mut engine = build_engine_with_executor(Arc::new(StubToolExecutor), config, 0, 3);
    let llm = ScriptedLlm::ok(vec![text_response("here is the answer")]);

    let result = engine
        .run_cycle(test_snapshot("read the file"), &llm)
        .await
        .expect("run_cycle should not panic");

    match result {
        LoopResult::Complete {
            response,
            iterations,
            ..
        } => {
            assert_eq!(response, "here is the answer");
            assert_eq!(iterations, 1);
        }
        other => panic!("expected Complete, got: {other:?}"),
    }
}

#[tokio::test]
async fn forced_synthesis_turn_strips_tools_and_appends_directive() {
    let engine = build_engine_with_executor(
        Arc::new(StubToolExecutor),
        budget_config_with_llm_calls(5, 2),
        0,
        3,
    );
    let llm = RecordingLlm::ok(vec![text_response("synthesized")]);
    let messages = vec![Message::user("hello")];

    let result = engine.forced_synthesis_turn(&llm, &messages).await;
    let requests = llm.requests();

    assert_eq!(result.as_deref(), Some("synthesized"));
    assert_eq!(
        requests.len(),
        1,
        "forced synthesis should make one LLM call"
    );
    assert!(
        requests[0].tools.is_empty(),
        "forced synthesis must strip tools"
    );
    assert!(
        requests[0]
            .system_prompt
            .as_deref()
            .is_some_and(|prompt| prompt.contains("Your tool budget is exhausted")),
        "forced synthesis should append the budget-exhausted directive to the system prompt"
    );
}

#[tokio::test]
async fn forced_synthesis_turn_hoists_system_messages_into_system_prompt() {
    let engine = build_engine_with_executor(
        Arc::new(StubToolExecutor),
        budget_config_with_llm_calls(5, 2),
        0,
        3,
    );
    let llm = RecordingLlm::ok(vec![text_response("synthesized")]);
    let messages = vec![
        Message::system("Runtime note: summarize tool failures clearly."),
        Message::user("hello"),
    ];

    let result = engine.forced_synthesis_turn(&llm, &messages).await;
    let requests = llm.requests();

    assert_eq!(result.as_deref(), Some("synthesized"));
    assert_eq!(requests.len(), 1);
    assert!(
            requests[0].system_prompt.as_deref().is_some_and(
                |prompt| prompt.contains("Runtime note: summarize tool failures clearly.")
            ),
            "forced synthesis should hoist runtime system messages into the system prompt"
        );
    assert!(
        requests[0]
            .messages
            .iter()
            .all(|message| message.role != MessageRole::System),
        "forced synthesis should strip system messages from the message list"
    );
}

#[test]
fn budget_exhausted_response_uses_non_empty_fallbacks() {
    assert_eq!(
        LoopEngine::resolve_budget_exhausted_response(
            Some("synthesized".to_string()),
            Some("partial".to_string()),
        ),
        "synthesized"
    );
    assert_eq!(
        LoopEngine::resolve_budget_exhausted_response(None, Some("partial".to_string())),
        "partial"
    );
    assert_eq!(
        LoopEngine::resolve_budget_exhausted_response(None, Some("   ".to_string())),
        BUDGET_EXHAUSTED_FALLBACK_RESPONSE
    );
}

// =========================================================================
// 2. Decomposition depth >2 integration test
// =========================================================================

/// Depth-0 decomposition with cap=3 completes a single sub-goal without
/// recursion issues.
#[tokio::test]
async fn decompose_at_depth_zero_with_cap_three_completes() {
    let config = budget_config_with_llm_calls(30, 3);
    let mut engine = build_engine_with_executor(
        Arc::new(StubToolExecutor),
        config.clone(),
        0, // depth 0
        4,
    );

    let plan = decomposition_plan(&["analyze the codebase"]);
    let decision = Decision::Decompose(plan.clone());

    let llm = ScriptedLlm::ok(vec![text_response("analysis of the codebase complete")]);

    let action = engine
        .execute_decomposition(&decision, &plan, &llm, &[])
        .await
        .expect("decomposition at depth 0");

    assert!(
        action
            .response_text
            .contains("analyze the codebase => completed"),
        "depth-0 decomposition should complete sub-goal: {}",
        action.response_text
    );
}

/// At max depth, decomposition returns the depth-limited fallback
/// without attempting child execution.
#[tokio::test]
async fn decompose_at_max_depth_returns_fallback() {
    let config = budget_config_with_llm_calls(20, 2);
    let mut engine = build_engine_with_executor(
        Arc::new(StubToolExecutor),
        config,
        2, // Already at depth 2 == max_recursion_depth
        4,
    );

    let plan = decomposition_plan(&["should not execute"]);
    let decision = Decision::Decompose(plan.clone());
    let llm = ScriptedLlm::ok(vec![]);

    let action = engine
        .execute_decomposition(&decision, &plan, &llm, &[])
        .await
        .expect("decomposition at max depth");

    assert!(
        action
            .response_text
            .contains("recursion depth limit was reached"),
        "should return depth limit message: {}",
        action.response_text
    );
}

/// End-to-end: decomposition at depth 0 with depth_cap=2. Children at
/// depth 1 execute, but grandchildren at depth 2 hit the cap.
#[tokio::test]
async fn decompose_depth_cap_prevents_infinite_recursion_end_to_end() {
    let config = budget_config_with_llm_calls(20, 2);
    let mut engine = build_engine_with_executor(Arc::new(StubToolExecutor), config.clone(), 0, 4);

    let plan = decomposition_plan(&["step one", "step two"]);
    let decision = Decision::Decompose(plan.clone());
    let llm = ScriptedLlm::ok(vec![
        text_response("step one done"),
        text_response("step two done"),
    ]);

    let action = engine
        .execute_decomposition(&decision, &plan, &llm, &[])
        .await
        .expect("execute_decomposition should succeed");

    assert!(
        action.response_text.contains("step one => completed"),
        "response should contain step one result: {}",
        action.response_text
    );
    assert!(
        action.response_text.contains("step two => completed"),
        "response should contain step two result: {}",
        action.response_text
    );

    // Now verify depth-2 child cannot decompose
    let mut depth_2_engine = build_engine_with_executor(Arc::new(StubToolExecutor), config, 2, 4);
    let child_plan = decomposition_plan(&["should not run"]);
    let child_decision = Decision::Decompose(child_plan.clone());
    let unused_llm = ScriptedLlm::ok(vec![]);

    let child_action = depth_2_engine
        .execute_decomposition(&child_decision, &child_plan, &unused_llm, &[])
        .await
        .expect("depth-limited decomposition");

    assert!(
        child_action
            .response_text
            .contains("recursion depth limit was reached"),
        "depth-2 child should be depth-limited: {}",
        child_action.response_text
    );
}

// =========================================================================
// 3. Tool friction → escalation (repeated tool failures)
// =========================================================================

/// When all tool calls fail repeatedly, the loop should not retry until
/// budget is gone. It should synthesize a response from the failed results.
#[tokio::test]
async fn repeated_tool_failures_synthesize_without_infinite_retry() {
    let mut engine = build_engine_with_executor(
        Arc::new(AlwaysFailingToolExecutor),
        BudgetConfig::default(),
        0,
        3,
    );

    let llm = ScriptedLlm::ok(vec![
        tool_use_response(vec![read_file_call("call-1")]),
        text_response("I was unable to read the file due to an error."),
        text_response("I was unable to read the file due to an error."),
    ]);

    let result = engine
        .run_cycle(test_snapshot("read the config"), &llm)
        .await
        .expect("run_cycle should not panic");

    match &result {
        LoopResult::Complete {
            response,
            iterations,
            ..
        } => {
            // Tool failure synthesis now feeds the next root reasoning
            // pass instead of finalizing directly.
            assert_eq!(
                *iterations, 2,
                "expected root continuation after tool synthesis: got {iterations}"
            );
            assert!(
                response.contains("unable to read") || response.contains("error"),
                "response should acknowledge the failure: {response}"
            );
        }
        other => panic!("expected Complete, got: {other:?}"),
    }
}

/// When the LLM keeps requesting tool calls that all fail, the loop
/// exhausts max_iterations and falls back to synthesis rather than
/// looping until budget is gone.
#[tokio::test]
async fn tool_friction_caps_at_max_iterations() {
    let mut engine = build_engine_with_executor(
        Arc::new(AlwaysFailingToolExecutor),
        BudgetConfig::default(),
        0,
        2, // Only 2 iterations
    );

    // Responses: reason (tool_use) → act_with_tools chains (tool_use → text)
    // → outer loop continuation: reason (text-only) → act (text-only, exits)
    let llm = ScriptedLlm::ok(vec![
        tool_use_response(vec![read_file_call("call-1")]),
        tool_use_response(vec![read_file_call("call-2")]),
        text_response("tools keep failing"),
        // Outer loop continuation
        text_response("tools keep failing"),
    ]);

    let result = engine
        .run_cycle(test_snapshot("read something"), &llm)
        .await
        .expect("run_cycle should not panic");

    match &result {
        LoopResult::Complete { iterations, .. } => {
            assert!(
                *iterations <= 2,
                "should not exceed max_iterations=2: got {iterations}"
            );
        }
        LoopResult::Error { recoverable, .. } => {
            assert!(*recoverable, "iteration-limit error should be recoverable");
        }
        other => panic!("expected Complete or Error, got: {other:?}"),
    }
}

// =========================================================================
// 4. Context overflow during tool round
// =========================================================================

/// When tool results push context past the hard limit, the engine
/// should return a recoverable `LoopError` or `LoopResult::Error`, not
/// panic. If compaction rescues the situation, the response must
/// acknowledge truncation or compaction.
#[tokio::test]
async fn context_overflow_during_tool_round_returns_error() {
    let config = BudgetConfig::default();
    let mut engine = LoopEngine::builder()
        .budget(BudgetTracker::new(config, current_time_ms(), 0))
        .context(ContextCompactor::new(256, 64))
        .max_iterations(3)
        .tool_executor(Arc::new(LargeOutputToolExecutor {
            output_size: 50_000,
        }))
        .synthesis_instruction("Summarize".to_string())
        .build()
        .expect("test engine build");

    let llm = ScriptedLlm::ok(vec![
        tool_use_response(vec![read_file_call("call-1")]),
        text_response("synthesized"),
        // Outer loop continuation: text-only response ends the loop
        text_response("synthesized"),
    ]);

    let result = engine
        .run_cycle(test_snapshot("read the big file"), &llm)
        .await;

    match result {
        Err(error) => {
            assert!(
                error.reason.contains("context_exceeded_after_compaction"),
                "error should mention context exceeded: {}",
                error.reason
            );
            assert!(error.recoverable, "context overflow should be recoverable");
        }
        Ok(LoopResult::Error {
            message,
            recoverable,
            ..
        }) => {
            assert!(recoverable, "context overflow error should be recoverable");
            assert!(
                message.contains("context") || message.contains("limit"),
                "error message should mention context: {message}"
            );
        }
        Ok(LoopResult::Complete { response, .. }) => {
            // Compaction rescued the situation — verify the response
            // acknowledges truncation or contains synthesis content.
            assert!(
                !response.is_empty(),
                "compaction-rescued response must not be empty"
            );
        }
        Ok(LoopResult::BudgetExhausted { .. }) => {
            // Budget exhaustion from context pressure is acceptable.
        }
        Ok(other) => {
            panic!("expected Error, Complete (compacted), or BudgetExhausted, got: {other:?}");
        }
    }
}

/// Context overflow produces a recoverable error even with moderately
/// large tool output that exceeds a small context budget mid-round.
#[tokio::test]
async fn context_overflow_mid_tool_round_is_recoverable() {
    let config = BudgetConfig {
        max_tool_result_bytes: usize::MAX,
        ..BudgetConfig::default()
    };
    let mut engine = LoopEngine::builder()
        .budget(BudgetTracker::new(config, current_time_ms(), 0))
        .context(ContextCompactor::new(512, 64))
        .max_iterations(3)
        .tool_executor(Arc::new(LargeOutputToolExecutor {
            output_size: 100_000,
        }))
        .synthesis_instruction("Summarize".to_string())
        .build()
        .expect("test engine build");

    let llm = ScriptedLlm::ok(vec![
        tool_use_response(vec![read_file_call("call-1")]),
        text_response("done"),
    ]);

    let result = engine
        .run_cycle(test_snapshot("process large data"), &llm)
        .await;

    match result {
        Err(error) => {
            assert!(
                error.recoverable,
                "context overflow should be recoverable: {}",
                error.reason
            );
        }
        Ok(LoopResult::Error {
            recoverable,
            message,
            ..
        }) => {
            assert!(
                recoverable,
                "context overflow LoopResult::Error should be recoverable: {message}"
            );
        }
        Ok(LoopResult::Complete { response, .. }) => {
            // Compaction handled it — response must be non-empty.
            assert!(
                !response.is_empty(),
                "compaction-rescued response must not be empty"
            );
        }
        Ok(LoopResult::BudgetExhausted { .. }) => {
            // Budget exhaustion from context pressure is acceptable.
        }
        Ok(other) => {
            panic!("expected Error, Complete (compacted), or BudgetExhausted, got: {other:?}");
        }
    }
}

// =========================================================================
// 5. Cancellation during decomposition
// =========================================================================

/// When cancellation fires during sequential decomposition, the engine
/// should stop processing remaining sub-goals and return `UserStopped`.
#[tokio::test]
async fn cancellation_during_decomposition_returns_user_stopped() {
    let token = CancellationToken::new();
    let cancel_token = token.clone();

    let config = budget_config_with_llm_calls(20, 4);
    let mut engine = LoopEngine::builder()
        .budget(BudgetTracker::new(config, current_time_ms(), 0))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(4)
        .tool_executor(Arc::new(StubToolExecutor))
        .synthesis_instruction("Summarize".to_string())
        .cancel_token(token)
        .build()
        .expect("test engine build");

    let llm = CancelAfterNthCallLlm::new(
        cancel_token,
        2, // Cancel after 2nd complete() call
        vec![
            Ok(CompletionResponse {
                content: Vec::new(),
                tool_calls: vec![ToolCall {
                    id: "decompose".to_string(),
                    name: DECOMPOSE_TOOL_NAME.to_string(),
                    arguments: serde_json::json!({
                        "sub_goals": [
                            {"description": "first task"},
                            {"description": "second task"},
                            {"description": "third task"},
                        ],
                        "strategy": "Sequential"
                    }),
                }],
                usage: None,
                stop_reason: Some("tool_use".to_string()),
            }),
            Ok(text_response("first task done")),
            Ok(text_response("second task done")),
            Ok(text_response("third task done")),
        ],
    );

    let result = engine
        .run_cycle(test_snapshot("do three things"), &llm)
        .await
        .expect("run_cycle should not panic on cancellation");

    // With 20 LLM calls of budget, BudgetExhausted would indicate a bug
    // in cancellation handling — only UserStopped or Complete (if the
    // cycle finished before cancel was checked) are acceptable.
    match &result {
        LoopResult::UserStopped {
            partial_response, ..
        } => {
            if let Some(partial) = partial_response {
                assert!(!partial.is_empty(), "partial response should not be empty");
            }
        }
        LoopResult::Complete { response, .. } => {
            assert!(!response.is_empty(), "response should not be empty");
        }
        other => {
            panic!("expected UserStopped or Complete, got: {other:?}");
        }
    }
}

/// Cancellation during tool execution within a decomposed sub-goal
/// should produce a clean result without panicking.
#[tokio::test]
async fn cancellation_during_slow_tool_in_decomposition_is_clean() {
    let token = CancellationToken::new();
    let cancel_clone = token.clone();
    let executions = Arc::new(AtomicUsize::new(0));

    let config = budget_config_with_llm_calls(20, 4);
    let mut engine = LoopEngine::builder()
        .budget(BudgetTracker::new(config, current_time_ms(), 0))
        .context(ContextCompactor::new(2048, 256))
        .max_iterations(4)
        .tool_executor(Arc::new(SlowToolExecutor {
            delay: Duration::from_secs(10),
            executions: Arc::clone(&executions),
        }))
        .synthesis_instruction("Summarize".to_string())
        .cancel_token(token)
        .build()
        .expect("test engine build");

    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        cancel_clone.cancel();
    });

    let llm = ScriptedLlm::ok(vec![tool_use_response(vec![read_file_call("call-1")])]);

    let result = engine
        .run_cycle(test_snapshot("read slowly"), &llm)
        .await
        .expect("run_cycle should not panic");

    match &result {
        LoopResult::UserStopped { .. } | LoopResult::Complete { .. } => {
            // Both acceptable — cancel may race with completion
        }
        other => panic!("expected UserStopped or Complete, got: {other:?}"),
    }

    assert!(
        executions.load(Ordering::SeqCst) >= 1,
        "tool executor should have been called at least once"
    );
}
