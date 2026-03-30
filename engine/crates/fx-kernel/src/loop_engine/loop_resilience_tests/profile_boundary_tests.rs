use super::*;

#[test]
fn standard_uses_mutation_only_escalation_after_observation() {
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    let mut state = ToolRoundState::new(
        &[],
        &[Message::user("Research first, then implement.")],
        None,
    );
    state.used_observation_tools = true;

    engine.turn_execution_profile = TurnExecutionProfile::Standard;
    assert_eq!(
        engine.continuation_tool_scope_for_round(&state),
        Some(ContinuationToolScope::MutationOnly)
    );

    engine.turn_execution_profile =
        TurnExecutionProfile::DirectInspection(DirectInspectionProfile::ReadLocalPath);
    assert_eq!(engine.continuation_tool_scope_for_round(&state), None);

    engine.turn_execution_profile =
        TurnExecutionProfile::DirectUtility(DirectUtilityProfile::CurrentTime);
    assert_eq!(engine.continuation_tool_scope_for_round(&state), None);

    engine.turn_execution_profile = TurnExecutionProfile::BoundedLocal;
    assert_eq!(engine.continuation_tool_scope_for_round(&state), None);
}

#[test]
fn direct_inspection_completes_terminally() {
    let profile = TurnExecutionProfile::DirectInspection(DirectInspectionProfile::ReadLocalPath);

    assert!(profile.completes_terminally());
    assert!(profile.allows_synthesis_fallback());
}

#[test]
fn direct_utility_completes_terminally() {
    let profile = TurnExecutionProfile::DirectUtility(DirectUtilityProfile::CurrentTime);

    assert!(profile.completes_terminally());
    assert!(!profile.allows_synthesis_fallback());
}

#[tokio::test]
async fn bounded_local_uses_own_terminal_mechanism() {
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
    let llm = RecordingLlm::ok(vec![]);
    let mut state = ToolRoundState::new(&calls, &[Message::user("make one concrete fix")], None);

    assert!(!engine.turn_execution_profile.completes_terminally());

    let outcome = engine
        .execute_tool_round(1, &llm, &mut state, Vec::new(), CycleStream::disabled())
        .await
        .expect("execute_tool_round");

    assert!(matches!(
        outcome,
        ToolRoundOutcome::BoundedLocalTerminal(
            BoundedLocalTerminalReason::NeedsGroundedEditAfterRecovery
        )
    ));
}
