use super::test_fixtures::{
    budget_config_with_llm_calls, build_engine_with_executor, read_file_call, test_snapshot,
    text_response, tool_use_response, RecordingLlm, ScriptedLlm, StubToolExecutor,
};
use super::*;
use crate::act::{
    ActionNextStep, ActionTerminal, ContinuationToolScope, FinalizeResponse,
    ProceedUnderConstraints, ToolResult, TurnCommitment,
};
use crate::decide::Decision;
use fx_llm::{ToolCall, ToolDefinition};
use std::sync::Arc;

fn install_finalize_response_commitment(engine: &mut LoopEngine) {
    let commitment = TurnCommitment::FinalizeResponse(FinalizeResponse {
        reason: "synthesize from gathered evidence".to_string(),
        success_target: "Produce a consolidated final answer from gathered evidence".to_string(),
    });
    engine.pending_turn_commitment = Some(commitment.clone());
    engine.pending_tool_scope = commitment_tool_scope(Some(&commitment));
}

#[test]
fn transition_table_policy_deferred_does_not_override_completed_evidence() {
    let cases = [
        (
            "successful evidence only",
            turn_control::ToolEvidenceTerminalFacts {
                has_tool_results: true,
                has_policy_deferred_results: false,
                pending_tool_calls: turn_control::PendingToolCallState::None,
                direct_inspection: false,
            },
            turn_control::ToolEvidenceTerminalDecision::ContinueRootSynthesis,
        ),
        (
            "policy-deferred control state only",
            turn_control::ToolEvidenceTerminalFacts {
                has_tool_results: false,
                has_policy_deferred_results: true,
                pending_tool_calls: turn_control::PendingToolCallState::None,
                direct_inspection: false,
            },
            turn_control::ToolEvidenceTerminalDecision::ContinuePolicyDeferred,
        ),
        (
            "successful evidence plus deferred follow-up",
            turn_control::ToolEvidenceTerminalFacts {
                has_tool_results: true,
                has_policy_deferred_results: true,
                pending_tool_calls: turn_control::PendingToolCallState::None,
                direct_inspection: false,
            },
            turn_control::ToolEvidenceTerminalDecision::ContinueRootSynthesis,
        ),
        (
            "resource boundary pending follow-up plus successful evidence",
            turn_control::ToolEvidenceTerminalFacts {
                has_tool_results: true,
                has_policy_deferred_results: false,
                pending_tool_calls: turn_control::PendingToolCallState::ResourceBoundary,
                direct_inspection: false,
            },
            turn_control::ToolEvidenceTerminalDecision::ContinueRootSynthesis,
        ),
        (
            "direct inspection remains terminal",
            turn_control::ToolEvidenceTerminalFacts {
                has_tool_results: true,
                has_policy_deferred_results: true,
                pending_tool_calls: turn_control::PendingToolCallState::None,
                direct_inspection: true,
            },
            turn_control::ToolEvidenceTerminalDecision::CompleteDirectInspectionEmpty,
        ),
    ];

    for (name, facts, expected) in cases {
        assert_eq!(
            turn_control::TurnControlPlane::decide_tool_evidence_terminal(facts),
            expected,
            "{name}"
        );
    }
}

#[test]
fn return_blocks_are_guidance_not_root_turn_gates() {
    let extraction = extract_root_turn_contract(
        "Please review the transcript UI.\n\nReturn:\n- What you inspected\n- Current architecture\n- 3 concrete recommendations",
    );

    assert!(
        extraction.contract.is_none(),
        "`Return:` is user-facing response guidance; only explicit `Deliverables:` should become a hard root-turn gate"
    );
    assert_eq!(
        extraction.deliverable_block_count, 0,
        "`Return:` should not increment the explicit deliverables block count"
    );
}

#[test]
fn explicit_deliverables_remain_hard_terminal_contracts() {
    let extraction = extract_root_turn_contract(
        "Continue the milestone.\n\nDeliverables:\n- Plan\n- Verification Report",
    );
    let contract = extraction.contract.expect("deliverables contract");

    let blocked = root_turn_completion_block(&contract, "**Plan**\n- Keep going.")
        .expect("missing verification report should block terminal completion");
    assert_eq!(
        blocked.missing_response_sections,
        vec!["Verification Report"]
    );
    assert!(blocked.pending_artifact_paths.is_empty());

    assert!(
        root_turn_completion_block(
            &contract,
            "**Plan**\n- Keep going.\n\n**Verification Report**\n- Focused tests pass."
        )
        .is_none(),
        "all explicit deliverable headings should satisfy the root-turn gate"
    );
}

#[test]
fn mutation_requests_create_hard_root_turn_contracts() {
    let extraction = extract_root_turn_contract(
        "Please fix the harness loop issue and open a PR against loop-tuning.",
    );
    let contract = extraction.contract.expect("mutation contract");

    let blocked = root_turn_completion_block(&contract, "I'll fix this by tightening the loop.")
        .expect("mutation work should block terminal completion until a mutation tool succeeds");

    assert_eq!(
        blocked.pending_mutation_work,
        vec!["Complete the requested code or file changes"]
    );
    assert_eq!(
        blocked.pending_external_actions,
        vec!["Open the GitHub pull request"]
    );
}

#[test]
fn mutation_request_detection_covers_common_code_change_language() {
    for message in [
        "Please refactor the module and open a PR.",
        "Add the missing file handling.",
        "Remove the dead crate code.",
        "Rename the function in this package.",
        "Please resolve these issues:\n\n1. discarded_field_note is too broad.\n2. ResultKind::Error regressed headless callers.",
    ] {
        let contract = extract_root_turn_contract(message)
            .contract
            .unwrap_or_else(|| panic!("expected mutation contract for: {message}"));

        assert!(
            contract.deliverables.iter().any(|deliverable| {
                matches!(deliverable, RootTurnDeliverable::MutationWork { .. })
            }),
            "expected mutation deliverable for: {message}"
        );
    }
}

#[test]
fn mutation_request_detection_does_not_convert_read_only_diagnostics() {
    assert!(
        extract_root_turn_contract("Tell me what needs to change in the harness.")
            .contract
            .is_none(),
        "diagnostic requests should not become mutation contracts"
    );
}

#[test]
fn mutation_work_completion_requires_successful_file_write_tool() {
    let calls = vec![
        ToolCall {
            id: "test".to_string(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({"argv": ["cargo", "test"]}),
        },
        ToolCall {
            id: "edit".to_string(),
            name: "edit_file".to_string(),
            arguments: serde_json::json!({"path": "src/lib.rs"}),
        },
    ];

    assert!(
        !mutation_work_completed(
            &calls[..1],
            &[ToolResult::success("test", "run_command", "ok")]
        ),
        "successful command side effects should not prove local code changes happened"
    );
    assert!(
        mutation_work_completed(
            &calls,
            &[ToolResult::success("edit", "edit_file", "edited")]
        ),
        "successful edit_file should satisfy mutation work"
    );
}

#[test]
fn mutation_tool_scope_includes_write_file_before_artifact_gate() {
    let scope = mutation_tool_scope(&[
        ToolDefinition {
            name: "read_file".to_string(),
            description: String::new(),
            parameters: serde_json::json!({"type":"object"}),
        },
        ToolDefinition {
            name: "write_file".to_string(),
            description: String::new(),
            parameters: serde_json::json!({"type":"object"}),
        },
        ToolDefinition {
            name: "edit_file".to_string(),
            description: String::new(),
            parameters: serde_json::json!({"type":"object"}),
        },
    ]);

    assert_eq!(
        scope,
        ContinuationToolScope::Only(vec!["edit_file".to_string(), "write_file".to_string()])
    );
}

#[test]
fn forced_synthesis_cannot_promote_contract_incomplete_text() {
    let contract = extract_root_turn_contract(
        "Inspect the renderer.\n\nDeliverables:\n- What you inspected\n- Current architecture",
    )
    .contract
    .expect("deliverables contract");

    let blocked = root_turn_completion_block(
        &contract,
        "Let me inspect the renderer again before answering fully.",
    )
    .expect("contract-incomplete synthesis should stay blocked");

    assert_eq!(
        blocked.missing_response_sections,
        vec!["What you inspected", "Current architecture"]
    );
}

#[test]
fn finalize_response_commitment_closes_the_tool_surface() {
    let commitment = TurnCommitment::FinalizeResponse(FinalizeResponse {
        reason: "iteration limit reached with usable evidence".to_string(),
        success_target: "Produce a consolidated final answer from gathered evidence".to_string(),
    });

    assert_eq!(
        commitment_tool_scope(Some(&commitment)),
        Some(ContinuationToolScope::NoTools)
    );

    let directive = render_turn_commitment_directive(&commitment);
    assert!(directive.contains("final-response phase"));
    assert!(directive.contains("Do not call tools"));
}

#[test]
fn finalize_response_progress_does_not_expose_internal_success_target() {
    let mut engine = build_engine_with_executor(
        Arc::new(StubToolExecutor),
        budget_config_with_llm_calls(10, 10),
        0,
        3,
    );
    install_finalize_response_commitment(&mut engine);

    let (_kind, message) = engine.current_turn_state_progress();

    assert_eq!(message, "Drafting the final response.");
    assert!(
        !message.contains("Produce a consolidated final answer"),
        "success_target is internal steering, not user-facing progress copy"
    );
}

#[tokio::test]
async fn finalize_response_commitment_blocks_tool_decisions_before_execution() {
    let mut engine = build_engine_with_executor(
        Arc::new(StubToolExecutor),
        budget_config_with_llm_calls(10, 10),
        0,
        3,
    );
    install_finalize_response_commitment(&mut engine);
    let decision = Decision::UseTools(vec![ToolCall {
        id: "call-1".to_string(),
        name: "read_file".to_string(),
        arguments: serde_json::json!({"path":"README.md"}),
    }]);
    let llm = ScriptedLlm::ok(Vec::new());

    let action = engine
        .act(&decision, &llm, &[], CycleStream::disabled())
        .await
        .expect("finalize tool guard should not error");

    assert!(
        action.tool_results.is_empty(),
        "finalize mode must reject tool decisions before executor dispatch"
    );
    assert!(matches!(action.next_step, ActionNextStep::Continue(_)));
    assert_eq!(engine.final_response_blocked_attempts, 1);
}

#[test]
fn finalize_response_commitment_retries_progress_only_terminal_text() {
    let mut engine = build_engine_with_executor(
        Arc::new(StubToolExecutor),
        budget_config_with_llm_calls(10, 0),
        0,
        3,
    );
    install_finalize_response_commitment(&mut engine);

    let first = engine.guard_root_turn_terminal_completion(ActionTerminal::Complete {
        response: "I'm continuing the inspection. Let me read ChatTranscript first.".to_string(),
    });
    assert!(
        matches!(first, ActionNextStep::Continue(_)),
        "progress narration is not a terminal final response"
    );
    let signals = engine.signals.drain_all();
    assert!(
        signals.iter().any(|signal| {
            signal.kind == SignalKind::Trace
                && signal.message == "progress-only final-response heuristic matched"
                && signal.metadata["detector"]
                    == serde_json::json!("looks_like_progress_only_final_response")
        }),
        "progress-only heuristic hits should be visible in telemetry signals"
    );

    let second = engine.guard_root_turn_terminal_completion(ActionTerminal::Complete {
        response: "I'm continuing the inspection. Let me inspect one more file.".to_string(),
    });
    match second {
        ActionNextStep::Finish(ActionTerminal::Incomplete {
            partial_response,
            reason,
        }) => {
            assert!(
                partial_response
                    .as_deref()
                    .is_some_and(|text| text.contains("could not produce a clean final answer")),
                "retry-cap termination must carry user-facing terminal text, got {partial_response:?}"
            );
            assert!(
                reason.contains("final response stayed non-terminal"),
                "diagnostic reason should preserve the protocol failure"
            );
        }
        other => panic!(
            "after the retry cap, invalid final responses should end incomplete, got {other:?}"
        ),
    }
}

#[tokio::test]
async fn finalize_response_commitment_uses_tool_free_terminal_synthesis_request() {
    let mut engine = build_engine_with_executor(
        Arc::new(StubToolExecutor),
        budget_config_with_llm_calls(10, 0),
        0,
        3,
    );
    install_finalize_response_commitment(&mut engine);
    let processed = engine
        .perceive(&test_snapshot("Use evidence and answer."))
        .await
        .expect("perceive");
    let llm = RecordingLlm::ok(vec![text_response("Final answer from gathered evidence.")]);

    let response = engine
        .reason(&processed, &llm, CycleStream::disabled())
        .await
        .expect("final synthesis reason should succeed");

    assert_eq!(
        extract_response_text(&response),
        "Final answer from gathered evidence."
    );
    let requests = llm.requests();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert!(
        request.tools.is_empty(),
        "final-response phase must not advertise a tool surface"
    );
    assert!(
        request
            .system_prompt
            .as_deref()
            .is_some_and(|prompt| prompt.contains(TERMINAL_SYNTHESIS_DIRECTIVE)),
        "final-response phase should use the terminal synthesis prompt"
    );
    let prompt = request.system_prompt.as_deref().unwrap_or_default();
    assert!(
        !prompt.contains("Use tools when you need information"),
        "terminal synthesis must not inherit the tool-planning system prompt"
    );
    assert!(
        !prompt.contains("use the decompose tool"),
        "terminal synthesis must not inherit decomposition guidance"
    );
}

#[tokio::test]
async fn finalize_response_text_with_spurious_tool_calls_retries_terminal_response() {
    let mut engine = build_engine_with_executor(
        Arc::new(StubToolExecutor),
        budget_config_with_llm_calls(10, 0),
        0,
        3,
    );
    install_finalize_response_commitment(&mut engine);
    let mut response = text_response("Final answer from gathered evidence.");
    response.tool_calls = vec![read_file_call("call-1")];

    let decision = engine
        .decide(&response)
        .await
        .expect("final response decision should succeed");

    assert!(
        matches!(decision, Decision::Respond(text) if text == "Final answer from gathered evidence."),
        "final-response phase still extracts the text candidate before terminal validation"
    );
    assert!(
        engine.final_response_attempted_tool_activity,
        "decide must preserve the final-response tool-attempt violation for the terminal gate"
    );

    let next = engine.guard_root_turn_terminal_completion(ActionTerminal::Complete {
        response: "Final answer from gathered evidence.".to_string(),
    });
    match next {
        ActionNextStep::Continue(continuation) => {
            assert_eq!(
                engine.final_response_blocked_attempts, 1,
                "terminal gate should retry instead of accepting text mixed with tool activity"
            );
            assert_eq!(
                continuation.next_tool_scope,
                Some(ContinuationToolScope::NoTools),
                "retry must keep final response closed to tools"
            );
        }
        other => panic!("expected final-response retry, got {other:?}"),
    }
}

#[tokio::test]
async fn finalize_response_tool_only_output_ends_incomplete_without_executing_tools() {
    let mut engine = build_engine_with_executor(
        Arc::new(StubToolExecutor),
        budget_config_with_llm_calls(10, 0),
        0,
        3,
    );
    install_finalize_response_commitment(&mut engine);
    let response = tool_use_response(vec![read_file_call("call-1")]);

    let decision = engine
        .decide(&response)
        .await
        .expect("final response decision should succeed");

    assert!(
        matches!(decision, Decision::Respond(text) if text.is_empty()),
        "tool-only output in final-response mode is a protocol violation, not a normal tool decision"
    );
    assert!(
        engine.final_response_attempted_tool_activity,
        "decide must preserve the final-response tool-attempt violation for the terminal gate"
    );

    let next = engine.guard_root_turn_terminal_completion(ActionTerminal::Complete {
        response: String::new(),
    });
    match next {
        ActionNextStep::Finish(ActionTerminal::Incomplete {
            partial_response,
            reason,
        }) => {
            assert_eq!(partial_response, None);
            assert!(
                reason.contains("no visible assistant response"),
                "empty final-response output should stop incomplete: {reason}"
            );
            assert_eq!(
                engine.final_response_blocked_attempts, 0,
                "empty responses should not consume the final-response retry budget"
            );
        }
        other => panic!("expected final-response incomplete stop, got {other:?}"),
    }
}

#[test]
fn empty_terminal_response_without_finalize_is_incomplete() {
    let mut engine = build_engine_with_executor(
        Arc::new(StubToolExecutor),
        budget_config_with_llm_calls(10, 0),
        0,
        3,
    );

    let next = engine.guard_root_turn_terminal_completion(ActionTerminal::Complete {
        response: String::new(),
    });

    match next {
        ActionNextStep::Finish(ActionTerminal::Incomplete {
            partial_response,
            reason,
        }) => {
            assert_eq!(partial_response, None);
            assert!(
                reason.contains("no visible assistant response"),
                "empty visible responses must not be terminal success: {reason}"
            );
        }
        other => panic!("expected empty terminal response to end incomplete, got {other:?}"),
    }

    let signals = engine.signals.drain_all();
    let signal = signals
        .iter()
        .find(|signal| signal.message == "empty terminal response rejected")
        .expect("kernel should emit a diagnostic signal for provider empty-response bugs");
    assert_eq!(signal.metadata["raw_response_chars"], serde_json::json!(0));
    assert_eq!(
        signal.metadata["raw_response_preview"],
        serde_json::json!("")
    );
}

#[tokio::test]
async fn truncated_final_response_retries_instead_of_accepting_partial_text() {
    let mut engine = build_engine_with_executor(
        Arc::new(StubToolExecutor),
        budget_config_with_llm_calls(10, 0),
        0,
        3,
    );
    install_finalize_response_commitment(&mut engine);
    let mut response = text_response("Partial final answer that never reached a stop reason.");
    response.stop_reason = Some("continuation_budget_exhausted".to_string());

    let decision = engine
        .decide(&response)
        .await
        .expect("final response decision should succeed");

    assert!(
        matches!(decision, Decision::Respond(text) if text == "Partial final answer that never reached a stop reason."),
        "decide can surface the candidate text, but terminal validation must still reject it"
    );
    assert!(
        engine.final_response_candidate_truncated,
        "decide must preserve non-terminal final-response stop reasons for the terminal gate"
    );

    let next = engine.guard_root_turn_terminal_completion(ActionTerminal::Complete {
        response: "Partial final answer that never reached a stop reason.".to_string(),
    });
    match next {
        ActionNextStep::Continue(continuation) => {
            assert_eq!(
                engine.final_response_blocked_attempts, 1,
                "terminal gate should retry truncated final output"
            );
            assert_eq!(
                continuation.next_tool_scope,
                Some(ContinuationToolScope::NoTools),
                "retry must keep final response closed to tools"
            );
        }
        other => panic!("expected final-response retry, got {other:?}"),
    }
}

#[test]
fn truncated_final_response_retry_cap_does_not_publish_partial_text() {
    let mut engine = build_engine_with_executor(
        Arc::new(StubToolExecutor),
        budget_config_with_llm_calls(10, 0),
        0,
        3,
    );
    install_finalize_response_commitment(&mut engine);

    engine.final_response_candidate_truncated = true;
    let first = engine.guard_root_turn_terminal_completion(ActionTerminal::Complete {
        response: "Partial answer cut off by the provider.".to_string(),
    });
    assert!(
        matches!(first, ActionNextStep::Continue(_)),
        "first truncated final answer should get one retry"
    );

    engine.final_response_candidate_truncated = true;
    let second = engine.guard_root_turn_terminal_completion(ActionTerminal::Complete {
        response: "Another partial answer cut off by the provider.".to_string(),
    });
    match second {
        ActionNextStep::Finish(ActionTerminal::Incomplete {
            partial_response,
            reason,
        }) => {
            assert!(
                reason.contains("truncated response"),
                "retry cap should report the typed final-response violation"
            );
            let partial = partial_response.expect("incomplete turn should include guidance");
            assert!(
                !partial.contains("Another partial answer cut off"),
                "retry-cap fallback must not publish a known-truncated final answer"
            );
            assert!(
                partial.contains("could not produce a clean final answer"),
                "retry-cap fallback should make the boundary explicit to the user"
            );
        }
        other => panic!("expected final-response incomplete boundary, got {other:?}"),
    }
}

#[tokio::test]
async fn finalize_response_retry_cap_for_tool_activity_has_terminal_copy() {
    let mut engine = build_engine_with_executor(
        Arc::new(StubToolExecutor),
        budget_config_with_llm_calls(10, 0),
        0,
        3,
    );
    install_finalize_response_commitment(&mut engine);

    for attempt in 1..=2 {
        let action = engine
            .act(
                &Decision::UseTools(vec![read_file_call("call-1")]),
                &ScriptedLlm::ok(Vec::new()),
                &[],
                CycleStream::disabled(),
            )
            .await
            .expect("final-response tool guard should produce a typed terminal state");

        if attempt == 1 {
            assert!(
                matches!(action.next_step, ActionNextStep::Continue(_)),
                "first tool-attempt violation should retry final response"
            );
        } else {
            match action.next_step {
                ActionNextStep::Finish(ActionTerminal::Incomplete {
                    partial_response,
                    reason,
                }) => {
                    let partial = partial_response
                        .as_deref()
                        .expect("retry cap must carry terminal copy");
                    assert!(partial.contains("could not produce a clean final answer"));
                    assert!(
                        !partial.to_ascii_lowercase().contains("tool request:"),
                        "terminal copy should not expose raw tool-call protocol text"
                    );
                    assert!(reason.contains("tool activity attempted"));
                }
                other => panic!("second violation should end incomplete, got {other:?}"),
            }
        }
    }
}

#[tokio::test]
async fn truncated_final_response_continuation_preserves_no_tool_scope() {
    let mut engine = build_engine_with_executor(
        Arc::new(StubToolExecutor),
        budget_config_with_llm_calls(10, 10),
        0,
        3,
    );
    install_finalize_response_commitment(&mut engine);
    let initial = CompletionResponse {
        content: vec![ContentBlock::Text {
            text: "Partial".to_string(),
        }],
        tool_calls: Vec::new(),
        usage: None,
        stop_reason: Some("max_tokens".to_string()),
    };
    let llm = RecordingLlm::ok(vec![text_response(" final answer.")]);

    let stitched = engine
        .continue_truncated_response(
            initial,
            &[Message::user("answer now")],
            &llm,
            LoopStep::Synthesize,
            CycleStream::disabled(),
        )
        .await
        .expect("final-response continuation should succeed");

    assert_eq!(extract_response_text(&stitched), "Partial final answer.");
    let requests = llm.requests();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0].tools.is_empty(),
        "truncated final-response continuation must not reopen tools"
    );
}

#[tokio::test]
async fn truncated_response_continuation_budget_exhaustion_keeps_partial_text() {
    let mut engine = build_engine_with_executor(
        Arc::new(StubToolExecutor),
        budget_config_with_llm_calls(1, 10),
        0,
        3,
    );
    engine.budget.record(&ActionCost {
        llm_calls: 1,
        tool_invocations: 0,
        tokens: 0,
        cost_cents: 0,
    });
    let initial = CompletionResponse {
        content: vec![ContentBlock::Text {
            text: "Partial final answer".to_string(),
        }],
        tool_calls: Vec::new(),
        usage: None,
        stop_reason: Some("max_tokens".to_string()),
    };
    let llm = RecordingLlm::ok(vec![text_response(" should not be requested")]);

    let stitched = engine
        .continue_truncated_response(
            initial,
            &[Message::user("answer now")],
            &llm,
            LoopStep::Synthesize,
            CycleStream::disabled(),
        )
        .await
        .expect("continuation budget exhaustion should be a typed boundary, not a loop error");

    assert_eq!(extract_response_text(&stitched), "Partial final answer");
    assert_eq!(
        stitched.stop_reason.as_deref(),
        Some("continuation_budget_exhausted")
    );
    assert!(
        llm.requests().is_empty(),
        "exhausted continuation budget must not make another provider call"
    );
    assert!(
        engine
            .signals
            .drain_all()
            .iter()
            .any(|signal| signal.kind == SignalKind::Blocked
                && signal.message == "continuation budget exhausted"),
        "the boundary should be visible to the control plane"
    );
}

#[tokio::test]
async fn terminal_final_response_continuation_uses_reserved_budget() {
    let mut engine = build_engine_with_executor(
        Arc::new(StubToolExecutor),
        budget_config_with_llm_calls(1, 10),
        0,
        3,
    );
    install_finalize_response_commitment(&mut engine);
    engine.budget.record(&ActionCost {
        llm_calls: 1,
        tool_invocations: 0,
        tokens: 0,
        cost_cents: 0,
    });
    let initial = CompletionResponse {
        content: vec![ContentBlock::Text {
            text: "Partial final".to_string(),
        }],
        tool_calls: Vec::new(),
        usage: None,
        stop_reason: Some("max_tokens".to_string()),
    };
    let llm = RecordingLlm::ok(vec![text_response(" answer.")]);

    let stitched = engine
        .continue_truncated_response(
            initial,
            &[Message::user("answer now")],
            &llm,
            LoopStep::Synthesize,
            CycleStream::disabled(),
        )
        .await
        .expect("committed final response should get a bounded terminal continuation reserve");

    assert_eq!(extract_response_text(&stitched), "Partial final answer.");
    let requests = llm.requests();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0].tools.is_empty(),
        "terminal continuation reserve must not reopen tools"
    );
    assert!(
        engine
            .signals
            .drain_all()
            .iter()
            .any(|signal| signal.kind == SignalKind::Trace
                && signal.message == "terminal final response continuation using reserved budget"),
        "reserve usage should be visible to the control plane"
    );
}

#[test]
fn continue_decisions_emit_typed_commitments() {
    let tool_call = ToolCall {
        id: "call-1".to_string(),
        name: "read_file".to_string(),
        arguments: serde_json::json!({"path":"README.md"}),
    };
    let decision = Decision::UseTools(vec![tool_call]);

    let full_surface_commitment = tool_continuation_turn_commitment(&decision, None)
        .expect("standard continuation should carry a typed commitment");
    assert!(matches!(
        full_surface_commitment,
        TurnCommitment::ProceedUnderConstraints(ProceedUnderConstraints {
            allowed_tools: None,
            ..
        })
    ));

    let no_tools_commitment =
        tool_continuation_turn_commitment(&decision, Some(&ContinuationToolScope::NoTools))
            .expect("scoped continuation should carry a typed commitment");
    assert_eq!(
        commitment_tool_scope(Some(&no_tools_commitment)),
        Some(ContinuationToolScope::NoTools)
    );
}

#[test]
fn default_termination_policy_nudges_without_stripping_tools() {
    let config = crate::budget::BudgetConfig::default();

    assert_eq!(
        config.termination.strip_tools_after_nudge,
        crate::budget::DISABLE_TOOL_STRIPPING_AFTER_NUDGE,
        "outer loop nudges may guide the model, but default execution must not hide the tool surface"
    );
    assert_eq!(
        config.termination.tool_round_strip_after_nudge,
        crate::budget::DISABLE_TOOL_STRIPPING_AFTER_NUDGE,
        "inner tool continuation must not force a no-tool final answer by default"
    );
    assert_eq!(
        config
            .termination
            .observation_only_round_strip_after_nudge,
        crate::budget::DISABLE_TOOL_STRIPPING_AFTER_NUDGE,
        "review/research turns may need many observation tools; defaults must not force mutation-only routing"
    );
}

#[test]
fn default_tool_round_policy_preserves_tools_after_nudge_pressure() {
    let engine = build_engine_with_executor(
        Arc::new(StubToolExecutor),
        crate::budget::BudgetConfig::default(),
        0,
        10,
    );
    let mut continuation_messages = Vec::new();

    let tools = engine.apply_tool_round_progress_policy(50_000, &mut continuation_messages);

    assert!(
        tools.iter().any(|tool| tool.name == "read_file"),
        "nudges should not silently remove the model's ability to continue tool work"
    );
}

#[tokio::test]
async fn budget_boundary_pending_tool_request_synthesizes_from_observed_evidence() {
    let mut config = budget_config_with_llm_calls(10, 10);
    config.max_tool_invocations = 1;
    let mut engine = build_engine_with_executor(Arc::new(StubToolExecutor), config, 0, 1);
    let llm = RecordingLlm::ok(vec![tool_use_response(vec![read_file_call("call-2")])]);
    let decision = Decision::UseTools(vec![read_file_call("call-1")]);

    let action = engine
        .act(&decision, &llm, &[], CycleStream::disabled())
        .await
        .expect("tool loop should produce a typed terminal state");
    let signals = engine.signals.drain_all();

    match action.next_step {
        ActionNextStep::Continue(continuation) => {
            assert_eq!(
                continuation.next_tool_scope,
                Some(ContinuationToolScope::NoTools),
                "budget-boundary synthesis must answer from observed evidence, not request more tools"
            );
        }
        other => panic!(
            "expected no-tool root synthesis continuation, got {other:?}; signals: {signals:?}"
        ),
    }
}

#[tokio::test]
#[ignore = "legacy harness behavior replaced by simple agent loop"]
async fn outer_budget_gate_does_not_preempt_committed_final_response() {
    let mut config = budget_config_with_llm_calls(2, 10);
    config.max_tool_invocations = 1;
    let mut engine = build_engine_with_executor(Arc::new(StubToolExecutor), config, 0, 3);
    let llm = RecordingLlm::ok(vec![
        tool_use_response(vec![read_file_call("call-1")]),
        tool_use_response(vec![read_file_call("call-2")]),
        text_response("Final answer from observed tool evidence."),
    ]);

    let result = engine
        .run_cycle(test_snapshot("inspect the current repo"), &llm)
        .await
        .expect("run cycle");

    match result {
        LoopResult::Complete { response, .. } => {
            assert_eq!(response, "Final answer from observed tool evidence.");
        }
        other => {
            panic!("budget boundary must reserve the committed final-response pass, got {other:?}")
        }
    }
}
