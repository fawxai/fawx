use super::*;

#[test]
fn root_turn_contract_extracts_issue_review_and_push_external_actions() {
    let issue_contract = extract_root_turn_contract(
        "Inspect issue 1855 and post a concise comment with the findings.",
    )
    .contract
    .expect("issue comment contract");
    assert!(issue_contract.deliverables.iter().any(|deliverable| {
        matches!(
            deliverable,
            RootTurnDeliverable::ExternalAction {
                kind: RootTurnExternalActionKind::GitHubIssueComment,
                label,
                satisfied: false,
            } if label == "Post a comment on the GitHub issue"
        )
    }));

    let review_contract = extract_root_turn_contract("Review PR 1858 and approve it if clean.")
        .contract
        .expect("PR review contract");
    assert!(review_contract.deliverables.iter().any(|deliverable| {
        matches!(
            deliverable,
            RootTurnDeliverable::ExternalAction {
                kind: RootTurnExternalActionKind::GitHubPrReview,
                label,
                satisfied: false,
            } if label == "Submit the GitHub pull request review"
        )
    }));

    let push_contract = extract_root_turn_contract("Commit the fix and push it to the remote.")
        .contract
        .expect("git push contract");
    assert!(push_contract.deliverables.iter().any(|deliverable| {
        matches!(
            deliverable,
            RootTurnDeliverable::ExternalAction {
                kind: RootTurnExternalActionKind::GitPush,
                label,
                satisfied: false,
            } if label == "Push changes to the git remote"
        )
    }));
}

#[test]
fn root_turn_contract_progress_marks_typed_issue_review_and_push_actions() {
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    engine.root_turn_contract = Some(RootTurnContract {
        deliverables: vec![
            RootTurnDeliverable::ExternalAction {
                kind: RootTurnExternalActionKind::GitHubIssueComment,
                label: external_action_label(RootTurnExternalActionKind::GitHubIssueComment)
                    .to_string(),
                satisfied: false,
            },
            RootTurnDeliverable::ExternalAction {
                kind: RootTurnExternalActionKind::GitHubPrReview,
                label: external_action_label(RootTurnExternalActionKind::GitHubPrReview)
                    .to_string(),
                satisfied: false,
            },
            RootTurnDeliverable::ExternalAction {
                kind: RootTurnExternalActionKind::GitPush,
                label: external_action_label(RootTurnExternalActionKind::GitPush).to_string(),
                satisfied: false,
            },
        ],
        blocked_terminal_attempts: 0,
    });

    let calls = vec![
        ToolCall {
            id: "issue-comment".to_string(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({
                "command": "gh issue comment 1855 --body-file /tmp/findings.md"
            }),
        },
        ToolCall {
            id: "pr-review".to_string(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({
                "command": "gh pr review 1858 --approve --body LGTM"
            }),
        },
        ToolCall {
            id: "push".to_string(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({
                "argv": ["git", "push", "origin", "dev"]
            }),
        },
    ];
    let results = vec![
        ToolResult {
            tool_call_id: "issue-comment".to_string(),
            tool_name: "run_command".to_string(),
            success: true,
            output: "https://github.com/fawxai/fawx/issues/1855#issuecomment-1".to_string(),
            failure_class: None,
        },
        ToolResult {
            tool_call_id: "pr-review".to_string(),
            tool_name: "run_command".to_string(),
            success: true,
            output: "Submitted review".to_string(),
            failure_class: None,
        },
        ToolResult {
            tool_call_id: "push".to_string(),
            tool_name: "run_command".to_string(),
            success: true,
            output: "Everything up-to-date".to_string(),
            failure_class: None,
        },
    ];
    engine.pending_tool_result_diagnostics.insert(
        "issue-comment".to_string(),
        ToolExecutionDiagnostics::RunCommand(RunCommandDiagnostics {
            exit_code: Some(0),
            stderr_snippet: None,
            duration_ms: 12,
            shell: true,
            timed_out: false,
            external_actions: vec![ExternalActionEvidence::github_issue_comment(Some(
                "https://github.com/fawxai/fawx/issues/1855#issuecomment-1".to_string(),
            ))],
        }),
    );
    engine.pending_tool_result_diagnostics.insert(
        "pr-review".to_string(),
        ToolExecutionDiagnostics::RunCommand(RunCommandDiagnostics {
            exit_code: Some(0),
            stderr_snippet: None,
            duration_ms: 12,
            shell: true,
            timed_out: false,
            external_actions: vec![ExternalActionEvidence::github_pr_review(None)],
        }),
    );
    engine.pending_tool_result_diagnostics.insert(
        "push".to_string(),
        ToolExecutionDiagnostics::RunCommand(RunCommandDiagnostics {
            exit_code: Some(0),
            stderr_snippet: None,
            duration_ms: 12,
            shell: false,
            timed_out: false,
            external_actions: vec![ExternalActionEvidence::git_push(
                Some("origin".to_string()),
                vec!["dev".to_string()],
            )],
        }),
    );

    engine.record_root_turn_contract_progress(&calls, &results);

    let contract = engine
        .root_turn_contract
        .as_ref()
        .expect("root turn contract");
    assert!(contract.deliverables.iter().all(|deliverable| {
        matches!(
            deliverable,
            RootTurnDeliverable::ExternalAction {
                satisfied: true,
                ..
            }
        )
    }));
}

#[test]
fn root_turn_contract_progress_ignores_typed_push_evidence_on_failed_result() {
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    engine.root_turn_contract = Some(RootTurnContract {
        deliverables: vec![RootTurnDeliverable::ExternalAction {
            kind: RootTurnExternalActionKind::GitPush,
            label: external_action_label(RootTurnExternalActionKind::GitPush).to_string(),
            satisfied: false,
        }],
        blocked_terminal_attempts: 0,
    });

    let calls = vec![ToolCall {
        id: "push".to_string(),
        name: "run_command".to_string(),
        arguments: serde_json::json!({
            "argv": ["git", "push", "origin", "dev"]
        }),
    }];
    let results = vec![ToolResult {
        tool_call_id: "push".to_string(),
        tool_name: "run_command".to_string(),
        success: false,
        output: "remote rejected".to_string(),
        failure_class: Some(FailureClass::Permanent),
    }];
    engine.pending_tool_result_diagnostics.insert(
        "push".to_string(),
        ToolExecutionDiagnostics::RunCommand(RunCommandDiagnostics {
            exit_code: Some(1),
            stderr_snippet: Some("remote rejected".to_string()),
            duration_ms: 12,
            shell: false,
            timed_out: false,
            external_actions: vec![ExternalActionEvidence::git_push(
                Some("origin".to_string()),
                vec!["dev".to_string()],
            )],
        }),
    );

    engine.record_root_turn_contract_progress(&calls, &results);

    let contract = engine
        .root_turn_contract
        .as_ref()
        .expect("root turn contract");
    assert!(contract.deliverables.iter().any(|deliverable| {
        matches!(
            deliverable,
            RootTurnDeliverable::ExternalAction {
                kind: RootTurnExternalActionKind::GitPush,
                satisfied: false,
                ..
            }
        )
    }));
}
