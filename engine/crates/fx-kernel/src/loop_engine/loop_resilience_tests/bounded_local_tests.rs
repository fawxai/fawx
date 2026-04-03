use super::*;

fn request_tool_names(request: &CompletionRequest) -> Vec<&str> {
    request
        .tools
        .iter()
        .map(|tool| tool.name.as_str())
        .collect()
}

#[test]
fn detect_turn_execution_profile_recognizes_bounded_local_requests() {
    let bounded = "Work only inside /Users/joseph/fawx.\nDo not use web research.\n1. Read the files needed to find the issue.\n2. Make one concrete code change.\n3. Run one focused test.\n4. End with a concise summary.";
    assert_eq!(
        detect_turn_execution_profile(bounded, &[]),
        TurnExecutionProfile::BoundedLocal
    );

    let general = "Research the latest X API behavior and summarize the official docs.";
    assert_eq!(
        detect_turn_execution_profile(general, &[]),
        TurnExecutionProfile::Standard
    );
}

#[tokio::test]
async fn perceive_routes_explicit_local_path_reads_to_direct_inspection() {
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    let _processed = engine
        .perceive(&test_snapshot(
            "Read ~/.zshrc and tell me exactly what it says.",
        ))
        .await
        .expect("perceive");

    assert_eq!(
        engine.turn_execution_profile,
        TurnExecutionProfile::DirectInspection(DirectInspectionProfile::ReadLocalPath)
    );
}

#[tokio::test]
async fn direct_inspection_turns_disable_effective_decompose() {
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    let llm = RecordingLlm::ok(vec![text_response("done")]);
    let processed = engine
        .perceive(&test_snapshot(
            "Read ~/.zshrc and tell me exactly what it says.",
        ))
        .await
        .expect("perceive");

    assert!(!engine.effective_decompose_enabled());

    let _ = engine
        .reason(&processed, &llm, CycleStream::disabled())
        .await
        .expect("reason");

    let requests = llm.requests();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0]
            .tools
            .iter()
            .all(|tool| tool.name != DECOMPOSE_TOOL_NAME),
        "direct inspection turns should not advertise decompose"
    );
}

#[tokio::test]
async fn direct_inspection_turns_own_profile_specific_tool_surface() {
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    engine.pending_tool_scope = Some(ContinuationToolScope::MutationOnly);
    let llm = RecordingLlm::ok(vec![text_response("done")]);
    let processed = engine
        .perceive(&test_snapshot(
            "Read ~/.zshrc and tell me exactly what it says.",
        ))
        .await
        .expect("perceive");

    let _ = engine
        .reason(&processed, &llm, CycleStream::disabled())
        .await
        .expect("reason");

    let requests = llm.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(request_tool_names(&requests[0]), vec!["read_file"]);
    let system_prompt = requests[0].system_prompt.as_deref().expect("system prompt");
    assert!(system_prompt.contains("direct local inspection request"));
    assert!(system_prompt.contains("Use `read_file`"));
}

#[tokio::test]
async fn direct_inspection_blocks_hallucinated_mutation_tool_calls() {
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    let _processed = engine
        .perceive(&test_snapshot(
            "Read ~/.zshrc and tell me exactly what it says.",
        ))
        .await
        .expect("perceive");

    let call = ToolCall {
        id: "w1".to_string(),
        name: "write_file".to_string(),
        arguments: serde_json::json!({
            "path": "/Users/joseph/fawx/.fawx_noop",
            "content": ""
        }),
    };

    let results = engine
        .execute_tool_calls_with_stream(std::slice::from_ref(&call), CycleStream::disabled())
        .await
        .expect("execute");

    assert_eq!(results.len(), 1);
    assert!(!results[0].success);
    assert!(results[0]
        .output
        .contains("direct inspection only allows observation tools"));
}

#[test]
fn detect_turn_execution_profile_supports_quoted_explicit_local_paths() {
    let message = "Inspect \"~/.zshrc\" and summarize it.";

    assert_eq!(
        detect_turn_execution_profile(message, &[]),
        TurnExecutionProfile::DirectInspection(DirectInspectionProfile::ReadLocalPath)
    );
}

#[test]
fn detect_turn_execution_profile_rejects_mutation_verbs_for_direct_inspection() {
    let message = "Read ~/.zshrc and then update it with a new alias.";

    assert_eq!(
        detect_turn_execution_profile(message, &[]),
        TurnExecutionProfile::Standard
    );
}

#[test]
fn detect_turn_execution_profile_requires_explicit_local_path_for_direct_inspection() {
    let message = "Read this file and summarize it for me.";

    assert_eq!(
        detect_turn_execution_profile(message, &[]),
        TurnExecutionProfile::Standard
    );
}

#[test]
fn detect_turn_execution_profile_rejects_mixed_local_and_online_guidance_requests() {
    let message = "Read ~/.zshrc and compare it to the latest online guidance for zsh config.";

    assert_eq!(
        detect_turn_execution_profile(message, &[]),
        TurnExecutionProfile::Standard
    );
}

#[tokio::test]
async fn decomposition_sub_goal_cannot_promote_standard_turn_to_direct_inspection() {
    let prompt = "Read ~/.zshrc and compare it to the latest online guidance for zsh config.";
    let sub_goal = SubGoal::with_definition_of_done(
        "Read the user's ~/.zshrc and summarize its structure, notable settings, plugins, aliases, PATH edits, and any unusual/possibly outdated patterns.".to_string(),
        Vec::new(),
        Some("inspection summary"),
        None,
    );
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    let processed = engine
        .perceive(&test_snapshot(prompt))
        .await
        .expect("perceive");

    assert_eq!(
        engine.turn_execution_profile,
        TurnExecutionProfile::Standard
    );

    let llm = RecordingLlm::ok(vec![text_response("done")]);
    let execution = engine
        .run_sub_goal(
            &sub_goal,
            BudgetConfig::default(),
            &llm,
            &processed.context_window,
            &[],
        )
        .await;

    assert!(
        execution
            .result
            .signals
            .iter()
            .any(|signal| signal.message == "processing user input"),
        "child sub-goal should still execute a real perceive pass"
    );
    assert!(
        execution
            .result
            .signals
            .iter()
            .all(|signal| signal.message != "selected direct inspection execution profile"),
        "standard parent turns must not be promoted to direct inspection during decomposition"
    );
}

#[test]
fn detect_turn_execution_profile_preserves_direct_utility_precedence() {
    let tools = DirectUtilityToolExecutor.tool_definitions();
    let message = "Tell me the current time, then quote ~/notes/todo.md.";

    assert_eq!(
        detect_turn_execution_profile(message, &tools),
        TurnExecutionProfile::DirectUtility(direct_current_time_profile())
    );
}

#[tokio::test]
async fn perceive_preserves_direct_utility_precedence_over_direct_inspection() {
    let mut engine = mixed_tool_engine_with_executor(
        BudgetConfig::default(),
        Arc::new(DirectUtilityToolExecutor),
    );
    let _processed = engine
        .perceive(&test_snapshot(
            "Tell me the current time, then quote ~/notes/todo.md.",
        ))
        .await
        .expect("perceive");

    assert_eq!(
        engine.turn_execution_profile,
        TurnExecutionProfile::DirectUtility(direct_current_time_profile())
    );
}

#[test]
fn detect_turn_execution_profile_preserves_bounded_local_precedence() {
    let message = "Work only inside ~/fawx.\nDo not use web research.\n1. Inspect ~/fawx/engine/crates/fx-kernel/src/loop_engine.rs to find the issue.\n2. Make one concrete code change.\n3. Run one focused test.\n4. End with a concise summary.";

    assert_eq!(
        detect_turn_execution_profile(message, &[]),
        TurnExecutionProfile::BoundedLocal
    );
}

#[tokio::test]
async fn perceive_preserves_bounded_local_precedence_over_direct_inspection() {
    let message = "Work only inside ~/fawx.\nDo not use web research.\n1. Inspect ~/fawx/engine/crates/fx-kernel/src/loop_engine.rs to find the issue.\n2. Make one concrete code change.\n3. Run one focused test.\n4. End with a concise summary.";
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    let _processed = engine
        .perceive(&test_snapshot(message))
        .await
        .expect("perceive");

    assert_eq!(
        engine.turn_execution_profile,
        TurnExecutionProfile::BoundedLocal
    );
}

#[tokio::test]
async fn bounded_local_prompt_disables_decompose_and_injects_fast_path_directive() {
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    let llm = RecordingLlm::ok(vec![CompletionResponse {
        content: vec![ContentBlock::Text {
            text: "done".to_string(),
        }],
        tool_calls: Vec::new(),
        usage: None,
        stop_reason: None,
    }]);

    let prompt = "Work only inside /Users/joseph/fawx.\nDo not use web research.\n1. Read the files needed to find the issue.\n2. Make one concrete code change.\n3. Run one focused test.\n4. End with a concise summary.";
    let processed = engine
        .perceive(&test_snapshot(prompt))
        .await
        .expect("perceive");
    let _ = engine
        .reason(&processed, &llm, CycleStream::disabled())
        .await
        .expect("reason");

    let requests = llm.requests();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0]
            .tools
            .iter()
            .all(|tool| tool.name != DECOMPOSE_TOOL_NAME),
        "bounded local tasks should not advertise decompose"
    );
    let system_prompt = requests[0].system_prompt.as_deref().expect("system prompt");
    assert!(
        system_prompt.contains("bounded local workspace task"),
        "bounded local tasks should carry a direct-execution directive"
    );
}

#[tokio::test]
async fn standard_turns_keep_their_normal_tool_surface() {
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    let llm = RecordingLlm::ok(vec![text_response("done")]);
    let processed = engine
        .perceive(&test_snapshot("Implement it now."))
        .await
        .expect("perceive");

    let _ = engine
        .reason(&processed, &llm, CycleStream::disabled())
        .await
        .expect("reason");

    let requests = llm.requests();
    assert_eq!(requests.len(), 1);
    let tool_names = request_tool_names(&requests[0]);
    assert!(tool_names.contains(&"read_file"));
    assert!(tool_names.contains(&"write_file"));
    assert!(tool_names.contains(&DECOMPOSE_TOOL_NAME));
}

#[test]
fn bounded_local_profile_ignores_generic_observation_round_stripping() {
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    engine.turn_execution_profile = TurnExecutionProfile::BoundedLocal;
    engine.consecutive_observation_only_rounds = 1;

    let tools = engine.apply_tool_round_progress_policy(1, &mut Vec::new());
    let tool_names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();
    assert!(
        tool_names.contains(&"read_file") && tool_names.contains(&"write_file"),
        "bounded local phases should own tool surfaces instead of inheriting generic observation-only stripping"
    );
}

#[test]
fn bounded_local_phase_progress_tracks_phase_specific_status() {
    let (kind, message) = progress_for_turn_state_with_profile(
        None,
        None,
        None,
        &StubToolExecutor,
        &TurnExecutionProfile::BoundedLocal,
        BoundedLocalPhase::Mutation,
    );
    assert_eq!(kind, ProgressKind::Implementing);
    assert_eq!(message, "Applying the local code change...");
}

#[test]
fn bounded_local_phase_progress_tracks_recovery_status() {
    let (kind, message) = progress_for_turn_state_with_profile(
        None,
        None,
        None,
        &StubToolExecutor,
        &TurnExecutionProfile::BoundedLocal,
        BoundedLocalPhase::Recovery,
    );
    assert_eq!(kind, ProgressKind::Implementing);
    assert_eq!(
        message,
        "Reading the exact local context needed to retry the edit..."
    );
}

#[test]
fn bounded_local_recovery_ignores_stale_mutation_only_scope() {
    let mut engine = engine_with_budget(BudgetConfig::default());
    engine.turn_execution_profile = TurnExecutionProfile::BoundedLocal;
    engine.bounded_local_phase = BoundedLocalPhase::Recovery;
    engine.pending_tool_scope = Some(ContinuationToolScope::MutationOnly);

    let tools = engine.current_reasoning_tool_definitions(false);
    let names: Vec<_> = tools.iter().map(|tool| tool.name.as_str()).collect();

    assert!(
        names.contains(&"read_file"),
        "recovery should still expose read_file even if a stale mutation scope exists"
    );
    assert!(
        !names.contains(&"write_file"),
        "recovery should remain phase-owned instead of falling back to mutation tools"
    );
}

#[test]
fn bounded_local_phase_advances_discovery_to_mutation_then_terminal() {
    let mut engine = run_command_observation_engine(BudgetConfig::default());
    engine.turn_execution_profile = TurnExecutionProfile::BoundedLocal;
    engine.bounded_local_phase = BoundedLocalPhase::Discovery;
    let make_call = |id: &str, name: &str, arguments: serde_json::Value| ToolCall {
        id: id.to_string(),
        name: name.to_string(),
        arguments,
    };

    let discovery_call = make_call("d1", "read_file", serde_json::json!({"path": "src/lib.rs"}));
    let discovery_result = ToolResult {
        tool_call_id: "d1".to_string(),
        tool_name: "read_file".to_string(),
        success: true,
        output: "ok".to_string(),
    };
    engine.advance_bounded_local_phase_after_tool_round(
        std::slice::from_ref(&discovery_call),
        std::slice::from_ref(&discovery_result),
    );
    assert_eq!(engine.bounded_local_phase, BoundedLocalPhase::Mutation);

    let mutation_call = make_call(
        "m1",
        "write_file",
        serde_json::json!({"path": "src/lib.rs", "content": "fn main() {}"}),
    );
    let mutation_result = ToolResult {
        tool_call_id: "m1".to_string(),
        tool_name: "write_file".to_string(),
        success: true,
        output: "wrote 12 bytes to src/lib.rs".to_string(),
    };
    engine.advance_bounded_local_phase_after_tool_round(
        std::slice::from_ref(&mutation_call),
        std::slice::from_ref(&mutation_result),
    );
    assert_eq!(engine.bounded_local_phase, BoundedLocalPhase::Verification);

    let verify_call = make_call(
        "v1",
        "run_command",
        serde_json::json!({"command": "cargo test -p fx-kernel -- --list"}),
    );
    let verify_result = ToolResult {
        tool_call_id: "v1".to_string(),
        tool_name: "run_command".to_string(),
        success: true,
        output: "ok".to_string(),
    };
    engine.advance_bounded_local_phase_after_tool_round(
        std::slice::from_ref(&verify_call),
        std::slice::from_ref(&verify_result),
    );
    assert_eq!(engine.bounded_local_phase, BoundedLocalPhase::Terminal);
}

#[test]
fn bounded_local_discovery_does_not_advance_on_search_only_round() {
    let mut engine = run_command_observation_engine(BudgetConfig::default());
    engine.turn_execution_profile = TurnExecutionProfile::BoundedLocal;
    engine.bounded_local_phase = BoundedLocalPhase::Discovery;

    let discovery_call = ToolCall {
        id: "d1".to_string(),
        name: "search_text".to_string(),
        arguments: serde_json::json!({
            "query": "streaming progress",
            "root": "/Users/joseph/fawx"
        }),
    };
    let discovery_result = ToolResult {
        tool_call_id: "d1".to_string(),
        tool_name: "search_text".to_string(),
        success: true,
        output: "found matches in loop_engine.rs".to_string(),
    };

    engine.advance_bounded_local_phase_after_tool_round(
        std::slice::from_ref(&discovery_call),
        std::slice::from_ref(&discovery_result),
    );

    assert_eq!(engine.bounded_local_phase, BoundedLocalPhase::Discovery);
}

#[test]
fn bounded_local_artifact_target_can_advance_after_non_read_discovery() {
    let mut engine = run_command_observation_engine(BudgetConfig::default());
    engine.turn_execution_profile = TurnExecutionProfile::BoundedLocal;
    engine.bounded_local_phase = BoundedLocalPhase::Discovery;
    engine.requested_artifact_target =
        Some("/Users/joseph/fawx/docs/debug/streaming-note.md".to_string());

    let discovery_call = ToolCall {
        id: "d1".to_string(),
        name: "search_text".to_string(),
        arguments: serde_json::json!({
            "query": "streaming progress",
            "root": "/Users/joseph/fawx"
        }),
    };
    let discovery_result = ToolResult {
        tool_call_id: "d1".to_string(),
        tool_name: "search_text".to_string(),
        success: true,
        output: "found matches in ChatViewModel.swift".to_string(),
    };

    engine.advance_bounded_local_phase_after_tool_round(
        std::slice::from_ref(&discovery_call),
        std::slice::from_ref(&discovery_result),
    );

    assert_eq!(engine.bounded_local_phase, BoundedLocalPhase::Mutation);
}

#[tokio::test]
async fn bounded_local_failed_mutation_gets_one_recovery_round_then_terminal() {
    let mut engine = run_command_observation_engine(BudgetConfig::default());
    engine.turn_execution_profile = TurnExecutionProfile::BoundedLocal;
    engine.bounded_local_phase = BoundedLocalPhase::Mutation;

    let failed_edit = ToolCall {
        id: "m1".to_string(),
        name: "edit_file".to_string(),
        arguments: serde_json::json!({
            "path": "/Users/joseph/fawx/app/Fawx/ViewModels/ChatViewModel.swift",
            "old_text": "missing old text",
            "new_text": "replacement"
        }),
    };
    let failed_edit_result = ToolResult {
        tool_call_id: "m1".to_string(),
        tool_name: "edit_file".to_string(),
        success: false,
        output: "old_text not found in file".to_string(),
    };

    engine.advance_bounded_local_phase_after_tool_round(
        std::slice::from_ref(&failed_edit),
        std::slice::from_ref(&failed_edit_result),
    );

    assert_eq!(engine.bounded_local_phase, BoundedLocalPhase::Recovery);
    assert!(engine.bounded_local_recovery_used);
    assert_eq!(
        engine.bounded_local_recovery_focus,
        vec!["/Users/joseph/fawx/app/Fawx/ViewModels/ChatViewModel.swift".to_string()]
    );

    let recovery_call = ToolCall {
        id: "r1".to_string(),
        name: "read_file".to_string(),
        arguments: serde_json::json!({
            "path": "/Users/joseph/fawx/app/Fawx/ViewModels/ChatViewModel.swift"
        }),
    };
    let recovery_results = engine
        .execute_tool_calls_with_stream(
            std::slice::from_ref(&recovery_call),
            CycleStream::disabled(),
        )
        .await
        .expect("execute");

    assert_eq!(recovery_results.len(), 1);
    assert!(recovery_results[0].success);

    engine.advance_bounded_local_phase_after_tool_round(
        std::slice::from_ref(&recovery_call),
        &recovery_results,
    );

    assert_eq!(engine.bounded_local_phase, BoundedLocalPhase::Mutation);
    assert!(engine.bounded_local_recovery_used);
    assert!(engine.bounded_local_recovery_focus.is_empty());

    let second_failed_edit_result = ToolResult {
        tool_call_id: "m1".to_string(),
        tool_name: "edit_file".to_string(),
        success: false,
        output: "old_text still not found in file".to_string(),
    };

    engine.advance_bounded_local_phase_after_tool_round(
        std::slice::from_ref(&failed_edit),
        std::slice::from_ref(&second_failed_edit_result),
    );

    assert_eq!(engine.bounded_local_phase, BoundedLocalPhase::Terminal);
}

#[tokio::test]
async fn bounded_local_terminal_blocker_is_kernel_authored() {
    let mut engine = mixed_tool_engine_with_executor(
        BudgetConfig::default(),
        Arc::new(FailingBoundedLocalEditExecutor),
    );
    engine.turn_execution_profile = TurnExecutionProfile::BoundedLocal;
    engine.bounded_local_phase = BoundedLocalPhase::Mutation;
    engine.bounded_local_recovery_used = true;

    let calls = vec![ToolCall {
        id: "m1".to_string(),
        name: "edit_file".to_string(),
        arguments: serde_json::json!({
            "path": "/Users/joseph/fawx/engine/crates/fx-kernel/src/loop_engine.rs",
            "old_text": "missing old text",
            "new_text": "replacement"
        }),
    }];
    let decision = Decision::UseTools(calls.clone());
    let llm = RecordingLlm::ok(vec![]);
    let context_messages = vec![Message::user("make one concrete fix")];

    let action = engine
        .act_with_tools(
            &decision,
            &calls,
            &llm,
            &context_messages,
            CycleStream::disabled(),
        )
        .await
        .expect("act_with_tools");

    assert!(
        llm.requests().is_empty(),
        "terminal bounded-local blocker should not ask the LLM to synthesize a reason"
    );
    match action.next_step {
        ActionNextStep::Finish(ActionTerminal::Incomplete {
            partial_response: Some(ref partial_response),
            ref reason,
        }) => {
            assert_eq!(
                reason,
                "bounded local run exhausted its one recovery pass before a grounded edit could be made"
            );
            assert!(
                partial_response.contains("File access was available during the run"),
                "{partial_response}"
            );
            assert!(
                partial_response.contains("old_text not found in file"),
                "{partial_response}"
            );
        }
        other => panic!("expected incomplete terminal blocker, got {other:?}"),
    }
}

#[test]
fn bounded_local_semantically_blocked_mutation_still_enters_recovery() {
    let mut engine = run_command_observation_engine(BudgetConfig::default());
    engine.turn_execution_profile = TurnExecutionProfile::BoundedLocal;
    engine.bounded_local_phase = BoundedLocalPhase::Mutation;

    let blocked_write = ToolCall {
        id: "w1".to_string(),
        name: "write_file".to_string(),
        arguments: serde_json::json!({
            "path": "/Users/joseph/fawx/.fawx_noop",
            "content": ""
        }),
    };
    let blocked_result = ToolResult {
        tool_call_id: "w1".to_string(),
        tool_name: "write_file".to_string(),
        success: false,
        output: format!(
            "Tool 'write_file' blocked: {}. Try a different approach.",
            BOUNDED_LOCAL_MUTATION_NOOP_BLOCK_REASON
        ),
    };

    engine.advance_bounded_local_phase_after_tool_round(
        std::slice::from_ref(&blocked_write),
        std::slice::from_ref(&blocked_result),
    );

    assert_eq!(engine.bounded_local_phase, BoundedLocalPhase::Recovery);
    assert!(engine.bounded_local_recovery_used);
}

#[tokio::test]
async fn bounded_local_recovery_bypasses_generic_observation_only_restriction() {
    let mut engine = engine_with_budget(BudgetConfig::default());
    engine.turn_execution_profile = TurnExecutionProfile::BoundedLocal;
    engine.bounded_local_phase = BoundedLocalPhase::Recovery;
    engine.consecutive_observation_only_rounds = 9;
    engine.pending_tool_scope = Some(ContinuationToolScope::MutationOnly);

    let call = ToolCall {
        id: "r1".to_string(),
        name: "read_file".to_string(),
        arguments: serde_json::json!({
            "path": "/Users/joseph/fawx/Cargo.toml"
        }),
    };

    let results = engine
        .execute_tool_calls_with_stream(std::slice::from_ref(&call), CycleStream::disabled())
        .await
        .expect("execute");

    assert_eq!(results.len(), 1);
    assert!(
        results[0].success,
        "recovery read should not be blocked by observation-only stripping"
    );
    assert!(
        !results[0]
            .output
            .contains(OBSERVATION_ONLY_CALL_BLOCK_REASON),
        "recovery should not inherit the generic observation-only block reason"
    );
}

#[tokio::test]
async fn bounded_local_discovery_bypasses_generic_observation_only_restriction() {
    let mut engine = engine_with_budget(BudgetConfig::default());
    engine.turn_execution_profile = TurnExecutionProfile::BoundedLocal;
    engine.bounded_local_phase = BoundedLocalPhase::Discovery;
    engine.consecutive_observation_only_rounds = 9;

    let call = ToolCall {
        id: "d1".to_string(),
        name: "search_text".to_string(),
        arguments: serde_json::json!({
            "query": "streaming progress",
            "root": "/Users/joseph/fawx"
        }),
    };

    let results = engine
        .execute_tool_calls_with_stream(std::slice::from_ref(&call), CycleStream::disabled())
        .await
        .expect("execute");

    assert_eq!(results.len(), 1);
    assert!(
        results[0].success,
        "discovery search should not be blocked by generic observation-only stripping"
    );
    assert!(
        !results[0]
            .output
            .contains(OBSERVATION_ONLY_CALL_BLOCK_REASON),
        "discovery should not inherit the generic observation-only block reason"
    );
}

#[tokio::test]
async fn bounded_local_discovery_blocks_run_command_before_editing() {
    let mut engine = run_command_observation_engine(BudgetConfig::default());
    engine.turn_execution_profile = TurnExecutionProfile::BoundedLocal;
    engine.bounded_local_phase = BoundedLocalPhase::Discovery;
    let call = ToolCall {
        id: "r1".to_string(),
        name: "run_command".to_string(),
        arguments: serde_json::json!({"command": "ls"}),
    };

    let results = engine
        .execute_tool_calls_with_stream(&[call], CycleStream::disabled())
        .await
        .expect("execute");

    assert_eq!(results.len(), 1);
    assert!(!results[0].success);
    assert!(results[0].output.contains("bounded local discovery"));
}

#[tokio::test]
async fn bounded_local_mutation_blocks_noop_scratch_write() {
    let mut engine = run_command_observation_engine(BudgetConfig::default());
    engine.turn_execution_profile = TurnExecutionProfile::BoundedLocal;
    engine.bounded_local_phase = BoundedLocalPhase::Mutation;
    let call = ToolCall {
        id: "w1".to_string(),
        name: "write_file".to_string(),
        arguments: serde_json::json!({
            "path": "/Users/joseph/fawx/.fawx_noop",
            "content": ""
        }),
    };

    let results = engine
        .execute_tool_calls_with_stream(&[call], CycleStream::disabled())
        .await
        .expect("execute");

    assert_eq!(results.len(), 1);
    assert!(!results[0].success);
    assert!(results[0].output.contains("meaningful repo-relevant edit"));
}

#[tokio::test]
async fn bounded_local_mutation_blocks_tmp_scratch_edit() {
    let mut engine = run_command_observation_engine(BudgetConfig::default());
    engine.turn_execution_profile = TurnExecutionProfile::BoundedLocal;
    engine.bounded_local_phase = BoundedLocalPhase::Mutation;
    let call = ToolCall {
        id: "e1".to_string(),
        name: "edit_file".to_string(),
        arguments: serde_json::json!({
            "path": "tmp/should_i_not_edit",
            "old_text": "old",
            "new_text": "new"
        }),
    };

    let results = engine
        .execute_tool_calls_with_stream(&[call], CycleStream::disabled())
        .await
        .expect("execute");

    assert_eq!(results.len(), 1);
    assert!(!results[0].success);
    assert!(results[0].output.contains("meaningful repo-relevant edit"));
}

#[tokio::test]
async fn bounded_local_mutation_blocks_edit_without_old_text() {
    let mut engine = run_command_observation_engine(BudgetConfig::default());
    engine.turn_execution_profile = TurnExecutionProfile::BoundedLocal;
    engine.bounded_local_phase = BoundedLocalPhase::Mutation;
    let call = ToolCall {
        id: "e1".to_string(),
        name: "edit_file".to_string(),
        arguments: serde_json::json!({
            "path": "/Users/joseph/fawx/engine/crates/fx-kernel/src/loop_engine.rs",
            "old_text": "",
            "new_text": "new"
        }),
    };

    let results = engine
        .execute_tool_calls_with_stream(&[call], CycleStream::disabled())
        .await
        .expect("execute");

    assert_eq!(results.len(), 1);
    assert!(!results[0].success);
    assert!(results[0].output.contains("meaningful repo-relevant edit"));
}

#[test]
fn bounded_local_mutation_phase_does_not_advance_on_noop_write() {
    let mut engine = run_command_observation_engine(BudgetConfig::default());
    engine.turn_execution_profile = TurnExecutionProfile::BoundedLocal;
    engine.bounded_local_phase = BoundedLocalPhase::Mutation;
    let call = ToolCall {
        id: "w1".to_string(),
        name: "write_file".to_string(),
        arguments: serde_json::json!({
            "path": "/Users/joseph/fawx/.fawx_noop",
            "content": ""
        }),
    };
    let result = ToolResult {
        tool_call_id: "w1".to_string(),
        tool_name: "write_file".to_string(),
        success: true,
        output: "wrote 0 bytes to /Users/joseph/fawx/.fawx_noop".to_string(),
    };

    engine.advance_bounded_local_phase_after_tool_round(
        std::slice::from_ref(&call),
        std::slice::from_ref(&result),
    );

    assert_eq!(engine.bounded_local_phase, BoundedLocalPhase::Mutation);
}

#[test]
fn bounded_local_mutation_phase_does_not_advance_on_proposal_only_result() {
    let mut engine = run_command_observation_engine(BudgetConfig::default());
    engine.turn_execution_profile = TurnExecutionProfile::BoundedLocal;
    engine.bounded_local_phase = BoundedLocalPhase::Mutation;
    let call = ToolCall {
        id: "w1".to_string(),
        name: "edit_file".to_string(),
        arguments: serde_json::json!({
            "path": "/Users/joseph/fawx/app/Fawx/ViewModels/ChatViewModel.swift",
            "old_text": "old",
            "new_text": "new"
        }),
    };
    let result = ToolResult {
        tool_call_id: "w1".to_string(),
        tool_name: "edit_file".to_string(),
        success: true,
        output:
            "PROPOSAL CREATED: write to '/Users/joseph/fawx/app/Fawx/ViewModels/ChatViewModel.swift' requires approval. Proposal saved to: /tmp/proposal.md"
                .to_string(),
    };

    engine.advance_bounded_local_phase_after_tool_round(
        std::slice::from_ref(&call),
        std::slice::from_ref(&result),
    );

    assert_eq!(engine.bounded_local_phase, BoundedLocalPhase::Mutation);
}

#[tokio::test]
async fn bounded_local_verification_blocks_shell_repo_search() {
    let mut engine = run_command_observation_engine(BudgetConfig::default());
    engine.turn_execution_profile = TurnExecutionProfile::BoundedLocal;
    engine.bounded_local_phase = BoundedLocalPhase::Verification;
    let call = ToolCall {
        id: "v1".to_string(),
        name: "run_command".to_string(),
        arguments: serde_json::json!({
            "command": "rg -n \"streaming\" /Users/joseph/fawx",
            "working_dir": "/Users/joseph/fawx"
        }),
    };

    let results = engine
        .execute_tool_calls_with_stream(&[call], CycleStream::disabled())
        .await
        .expect("execute");

    assert_eq!(results.len(), 1);
    assert!(!results[0].success);
    assert!(results[0].output.contains("focused confirmation commands"));
}

#[tokio::test]
async fn bounded_local_verification_allows_focused_test_command() {
    let mut engine = run_command_observation_engine(BudgetConfig::default());
    engine.turn_execution_profile = TurnExecutionProfile::BoundedLocal;
    engine.bounded_local_phase = BoundedLocalPhase::Verification;
    let call = ToolCall {
        id: "v1".to_string(),
        name: "run_command".to_string(),
        arguments: serde_json::json!({
            "command": "cargo test -p fx-kernel bounded_local_phase_progress_tracks_phase_specific_status -- --nocapture",
            "working_dir": "/Users/joseph/fawx"
        }),
    };

    let results = engine
        .execute_tool_calls_with_stream(&[call], CycleStream::disabled())
        .await
        .expect("execute");

    assert_eq!(results.len(), 1);
    assert!(results[0].success);
}
