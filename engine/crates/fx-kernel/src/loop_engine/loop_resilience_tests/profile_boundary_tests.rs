use super::*;
use crate::budget::TerminationConfig;

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
        TurnExecutionProfile::DirectUtility(direct_current_time_profile());
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
    let profile = TurnExecutionProfile::DirectUtility(direct_current_time_profile());

    assert!(profile.completes_terminally());
    assert!(!profile.allows_synthesis_fallback());
}

#[test]
fn tightened_termination_config_values_match_expected() {
    let base = TerminationConfig {
        synthesize_on_exhaustion: true,
        nudge_after_tool_turns: 100,
        strip_tools_after_nudge: 100,
        tool_round_nudge_after: 100,
        tool_round_strip_after_nudge: 100,
        observation_only_round_nudge_after: 100,
        observation_only_round_strip_after_nudge: 100,
    };

    let bounded = TurnExecutionProfile::BoundedLocal
        .tightened_termination_config(&base)
        .expect("bounded local tightens termination");
    assert!(bounded.nudge_after_tool_turns <= 3);
    assert!(bounded.tool_round_nudge_after <= 2);
    assert_eq!(bounded.observation_only_round_nudge_after, 1);
    assert_eq!(bounded.observation_only_round_strip_after_nudge, 0);

    let direct_inspection =
        TurnExecutionProfile::DirectInspection(DirectInspectionProfile::ReadLocalPath)
            .tightened_termination_config(&base)
            .expect("direct inspection tightens termination");
    assert!(direct_inspection.nudge_after_tool_turns <= 1);
    assert_eq!(direct_inspection.strip_tools_after_nudge, 0);
    assert!(direct_inspection.tool_round_nudge_after <= 1);
    assert_eq!(direct_inspection.tool_round_strip_after_nudge, 0);
    assert_eq!(direct_inspection.observation_only_round_nudge_after, 0);
    assert_eq!(
        direct_inspection.observation_only_round_strip_after_nudge,
        0
    );

    let direct_utility = TurnExecutionProfile::DirectUtility(direct_current_time_profile())
        .tightened_termination_config(&base)
        .expect("direct utility tightens termination");
    assert!(direct_utility.nudge_after_tool_turns <= 1);
    assert_eq!(direct_utility.strip_tools_after_nudge, 0);
    assert!(direct_utility.tool_round_nudge_after <= 1);
    assert_eq!(direct_utility.tool_round_strip_after_nudge, 0);
    assert_eq!(direct_utility.observation_only_round_nudge_after, 0);
    assert_eq!(direct_utility.observation_only_round_strip_after_nudge, 0);

    assert_eq!(
        TurnExecutionProfile::Standard.tightened_termination_config(&base),
        None
    );
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
