use super::*;
use crate::act::{ToolExecutor, ToolResult};
use crate::cancellation::CancellationToken;
use async_trait::async_trait;
use fx_llm::ToolDefinition;
use std::sync::Arc;

#[derive(Debug, Default)]
struct PrCommentToolExecutor;

#[async_trait]
impl ToolExecutor for PrCommentToolExecutor {
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
                output: match call.name.as_str() {
                    "comment_pr" => "{\"success\":true,\"comment_id\":1}".to_string(),
                    _ => "ok".to_string(),
                },
                failure_class: None,
            })
            .collect())
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "run_command".to_string(),
                description: "Run a command".to_string(),
                parameters: serde_json::json!({"type":"object"}),
            },
            ToolDefinition {
                name: "comment_pr".to_string(),
                description: "Post a GitHub PR comment".to_string(),
                parameters: serde_json::json!({"type":"object"}),
            },
        ]
    }
}

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

    let pr_create_contract = extract_root_turn_contract(
        "Branch off loop-tuning, fix the harness contract, and open a PR against loop-tuning.",
    )
    .contract
    .expect("PR create contract");
    assert!(pr_create_contract.deliverables.iter().any(|deliverable| {
        matches!(
            deliverable,
            RootTurnDeliverable::ExternalAction {
                kind: RootTurnExternalActionKind::GitHubPrCreate,
                label,
                satisfied: false,
            } if label == "Open the GitHub pull request"
        )
    }));
    assert!(pr_create_contract.deliverables.iter().any(|deliverable| {
        matches!(
            deliverable,
            RootTurnDeliverable::MutationWork {
                label,
                satisfied: false,
            } if label == "Complete the requested code or file changes"
        )
    }));
}

#[test]
fn root_turn_contract_progress_marks_typed_issue_review_pr_create_and_push_actions() {
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
            RootTurnDeliverable::ExternalAction {
                kind: RootTurnExternalActionKind::GitHubPrCreate,
                label: external_action_label(RootTurnExternalActionKind::GitHubPrCreate)
                    .to_string(),
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
        ToolCall {
            id: "pr-create".to_string(),
            name: "create_pr".to_string(),
            arguments: serde_json::json!({
                "base": "loop-tuning",
                "head": "codex/loop-harness-contract"
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
        ToolResult {
            tool_call_id: "pr-create".to_string(),
            tool_name: "create_pr".to_string(),
            success: true,
            output: "https://github.com/fawxai/fawx/pull/1875".to_string(),
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
fn root_turn_contract_progress_marks_legacy_gh_pr_create_command() {
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    engine.root_turn_contract = Some(RootTurnContract {
        deliverables: vec![RootTurnDeliverable::ExternalAction {
            kind: RootTurnExternalActionKind::GitHubPrCreate,
            label: external_action_label(RootTurnExternalActionKind::GitHubPrCreate).to_string(),
            satisfied: false,
        }],
        blocked_terminal_attempts: 0,
    });

    let calls = vec![ToolCall {
        id: "pr-create".to_string(),
        name: "run_command".to_string(),
        arguments: serde_json::json!({
            "argv": ["gh", "pr", "create", "--base", "loop-tuning", "--head", "codex/loop-harness-contract"]
        }),
    }];
    let results = vec![ToolResult {
        tool_call_id: "pr-create".to_string(),
        tool_name: "run_command".to_string(),
        success: true,
        output: "https://github.com/fawxai/fawx/pull/1875".to_string(),
        failure_class: None,
    }];

    engine.record_root_turn_contract_progress(&calls, &results);

    let contract = engine
        .root_turn_contract
        .as_ref()
        .expect("root turn contract");
    assert!(contract.deliverables.iter().any(|deliverable| {
        matches!(
            deliverable,
            RootTurnDeliverable::ExternalAction {
                kind: RootTurnExternalActionKind::GitHubPrCreate,
                satisfied: true,
                ..
            }
        )
    }));
}

#[test]
fn root_turn_contract_progress_clears_pending_external_action_target_when_matching_kind_satisfied()
{
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    engine.pending_external_action_target = Some(RootTurnExternalActionKind::GitHubPrComment);
    engine.root_turn_contract = Some(RootTurnContract {
        deliverables: vec![RootTurnDeliverable::ExternalAction {
            kind: RootTurnExternalActionKind::GitHubPrComment,
            label: external_action_label(RootTurnExternalActionKind::GitHubPrComment).to_string(),
            satisfied: false,
        }],
        blocked_terminal_attempts: 0,
    });

    let calls = vec![ToolCall {
        id: "comment-1".to_string(),
        name: "comment_pr".to_string(),
        arguments: serde_json::json!({
            "owner": "abbudjoe",
            "repo": "fawx",
            "pr_number": 1872,
            "body": "Review posted"
        }),
    }];
    let results = vec![ToolResult::success(
        "comment-1",
        "comment_pr",
        r#"{"success":true,"comment_id":1}"#,
    )];
    engine.pending_tool_result_diagnostics.insert(
        "comment-1".to_string(),
        ToolExecutionDiagnostics::Tool(ToolDiagnostics {
            external_actions: vec![ExternalActionEvidence::github_pr_comment(Some(
                "https://github.com/fawxai/fawx/pull/1872#issuecomment-1".to_string(),
            ))],
        }),
    );

    engine.record_root_turn_contract_progress(&calls, &results);

    assert_eq!(engine.pending_external_action_target, None);
}

#[test]
fn root_turn_contract_progress_preserves_pending_external_action_target_for_non_matching_kind() {
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    engine.pending_external_action_target = Some(RootTurnExternalActionKind::GitHubPrComment);
    engine.root_turn_contract = Some(RootTurnContract {
        deliverables: vec![RootTurnDeliverable::ExternalAction {
            kind: RootTurnExternalActionKind::GitHubIssueComment,
            label: external_action_label(RootTurnExternalActionKind::GitHubIssueComment)
                .to_string(),
            satisfied: false,
        }],
        blocked_terminal_attempts: 0,
    });

    let calls = vec![ToolCall {
        id: "comment-1".to_string(),
        name: "comment_issue".to_string(),
        arguments: serde_json::json!({
            "owner": "abbudjoe",
            "repo": "fawx",
            "issue_number": 1872,
            "body": "Issue note posted"
        }),
    }];
    let results = vec![ToolResult::success(
        "comment-1",
        "comment_issue",
        r#"{"success":true,"comment_id":1}"#,
    )];
    engine.pending_tool_result_diagnostics.insert(
        "comment-1".to_string(),
        ToolExecutionDiagnostics::Tool(ToolDiagnostics {
            external_actions: vec![ExternalActionEvidence::github_issue_comment(Some(
                "https://github.com/fawxai/fawx/issues/1872#issuecomment-1".to_string(),
            ))],
        }),
    );

    engine.record_root_turn_contract_progress(&calls, &results);

    assert_eq!(
        engine.pending_external_action_target,
        Some(RootTurnExternalActionKind::GitHubPrComment)
    );
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

#[tokio::test]
async fn external_action_closure_blocks_more_inspection_until_pr_comment_attempt() {
    let mut engine = run_command_observation_engine(BudgetConfig::default());
    engine.pending_external_action_target = Some(RootTurnExternalActionKind::GitHubPrComment);

    let calls = vec![
        ToolCall {
            id: "more-diff".to_string(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({
                "argv": ["gh", "pr", "diff", "98", "--repo", "abbudjoe/autoproject"]
            }),
        },
        ToolCall {
            id: "post-comment".to_string(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({
                "argv": [
                    "gh",
                    "pr",
                    "comment",
                    "98",
                    "--repo",
                    "abbudjoe/autoproject",
                    "--body",
                    "Review posted"
                ]
            }),
        },
    ];

    let batch = engine
        .execute_tool_calls_batch_with_stream(&calls, CycleStream::disabled())
        .await
        .expect("execute tool batch");

    let diff_result = batch
        .results
        .iter()
        .find(|result| result.tool_call_id == "more-diff")
        .expect("diff result");
    assert!(!diff_result.success);
    assert_eq!(
        diff_result.failure_class,
        Some(FailureClass::PolicyDeferred)
    );
    assert!(diff_result
        .output
        .contains("pending external action gate requires"));

    let comment_result = batch
        .results
        .iter()
        .find(|result| result.tool_call_id == "post-comment")
        .expect("comment result");
    assert!(comment_result.success);
}

#[tokio::test]
#[ignore = "legacy harness behavior replaced by simple agent loop"]
async fn pending_pr_comment_retry_does_not_spend_closure_on_more_diffing() {
    let mut engine = run_command_observation_engine(BudgetConfig::default());
    let final_response = "Review posted on PR #98.";
    let llm = RecordingLlm::ok(vec![
        text_response("I reviewed the PR but did not post a comment."),
        tool_use_response(vec![ToolCall {
            id: "more-diff".to_string(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({
                "argv": ["gh", "pr", "diff", "98", "--repo", "abbudjoe/autoproject"]
            }),
        }]),
        tool_use_response(vec![ToolCall {
            id: "post-comment".to_string(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({
                "argv": [
                    "gh",
                    "pr",
                    "comment",
                    "98",
                    "--repo",
                    "abbudjoe/autoproject",
                    "--body",
                    "Review posted"
                ]
            }),
        }]),
        text_response(final_response),
    ]);

    let result = engine
        .run_cycle(
            test_snapshot("Review PR 98 and post findings in a comment."),
            &llm,
        )
        .await
        .expect("run cycle");

    assert_eq!(complete_response(result), final_response);
    assert!(request_contains_text(
        &llm.requests()[2],
        "pending external action gate requires"
    ));
}

#[test]
fn available_external_action_tool_names_prefers_typed_pr_comment_tool() {
    let executor = PrCommentToolExecutor;
    let names = available_external_action_tool_names(
        &executor.tool_definitions(),
        RootTurnExternalActionKind::GitHubPrComment,
    );

    assert_eq!(names, vec!["comment_pr".to_string()]);
}

#[tokio::test]
async fn external_action_closure_prefers_typed_pr_comment_tool_when_available() {
    let mut engine =
        mixed_tool_engine_with_executor(BudgetConfig::default(), Arc::new(PrCommentToolExecutor));
    engine.pending_external_action_target = Some(RootTurnExternalActionKind::GitHubPrComment);

    let calls = vec![
        ToolCall {
            id: "more-diff".to_string(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({
                "argv": ["gh", "pr", "diff", "98", "--repo", "abbudjoe/autoproject"]
            }),
        },
        ToolCall {
            id: "post-comment".to_string(),
            name: "comment_pr".to_string(),
            arguments: serde_json::json!({
                "owner": "abbudjoe",
                "repo": "autoproject",
                "pr_number": 98,
                "body": "Review posted"
            }),
        },
    ];

    let batch = engine
        .execute_tool_calls_batch_with_stream(&calls, CycleStream::disabled())
        .await
        .expect("execute tool batch");

    let diff_result = batch
        .results
        .iter()
        .find(|result| result.tool_call_id == "more-diff")
        .expect("diff result");
    assert!(!diff_result.success);
    assert_eq!(
        diff_result.failure_class,
        Some(FailureClass::PolicyDeferred)
    );
    assert!(diff_result.output.contains("Use one of [comment_pr]"));

    let comment_result = batch
        .results
        .iter()
        .find(|result| result.tool_call_id == "post-comment")
        .expect("comment result");
    assert!(comment_result.success);
}

#[tokio::test]
#[ignore = "legacy harness behavior replaced by simple agent loop"]
async fn pending_pr_comment_retry_names_typed_comment_tool_in_retry_context() {
    let mut engine =
        mixed_tool_engine_with_executor(BudgetConfig::default(), Arc::new(PrCommentToolExecutor));
    let final_response = "Review posted on PR #98.";
    let llm = RecordingLlm::ok(vec![
        text_response("I reviewed the PR but did not post a comment."),
        tool_use_response(vec![ToolCall {
            id: "more-diff".to_string(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({
                "argv": ["gh", "pr", "diff", "98", "--repo", "abbudjoe/autoproject"]
            }),
        }]),
        tool_use_response(vec![ToolCall {
            id: "post-comment".to_string(),
            name: "comment_pr".to_string(),
            arguments: serde_json::json!({
                "owner": "abbudjoe",
                "repo": "autoproject",
                "pr_number": 98,
                "body": "Review posted"
            }),
        }]),
        text_response(final_response),
    ]);

    let result = engine
        .run_cycle(
            test_snapshot("Review PR 98 and post findings in a comment."),
            &llm,
        )
        .await
        .expect("run cycle");

    assert_eq!(complete_response(result), final_response);
    assert!(request_contains_text(&llm.requests()[2], "comment_pr"));
    assert!(request_contains_text(
        &llm.requests()[2],
        "Use one of [comment_pr]"
    ));
}

#[test]
fn external_action_gate_lifts_after_consecutive_failures() {
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    engine.pending_external_action_target = Some(RootTurnExternalActionKind::GitPush);
    engine.pending_external_action_consecutive_failures = 0;

    // Below threshold: gate should filter to only run_command
    let tools = engine.apply_pending_external_action_gate(engine.tool_executor.tool_definitions());
    let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(
        !tool_names.contains(&"read_file"),
        "gate should block inspection tools below failure threshold, got: {:?}",
        tool_names
    );

    // Simulate two consecutive failures
    engine.pending_external_action_consecutive_failures = 2;

    // At threshold: gate should lift and return full tool scope
    let tools = engine.apply_pending_external_action_gate(engine.tool_executor.tool_definitions());
    let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(
        tool_names.contains(&"read_file"),
        "gate should lift after consecutive failures, returning full tool scope, got: {:?}",
        tool_names
    );
}

#[test]
fn external_action_failure_counter_increments_on_failed_push_attempt() {
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    engine.root_turn_contract = Some(RootTurnContract {
        deliverables: vec![RootTurnDeliverable::ExternalAction {
            kind: RootTurnExternalActionKind::GitPush,
            label: external_action_label(RootTurnExternalActionKind::GitPush).to_string(),
            satisfied: false,
        }],
        blocked_terminal_attempts: 0,
    });
    engine.pending_external_action_target = Some(RootTurnExternalActionKind::GitPush);
    engine.pending_external_action_consecutive_failures = 0;

    // First failed push attempt
    let calls = vec![ToolCall {
        id: "push-1".to_string(),
        name: "run_command".to_string(),
        arguments: serde_json::json!({
            "argv": ["git", "push", "origin", "main"]
        }),
    }];
    let results = vec![ToolResult {
        tool_call_id: "push-1".to_string(),
        tool_name: "run_command".to_string(),
        success: false,
        output: "Authentication failed".to_string(),
        failure_class: None,
    }];
    engine.pending_tool_result_diagnostics.insert(
        "push-1".to_string(),
        ToolExecutionDiagnostics::RunCommand(RunCommandDiagnostics {
            exit_code: Some(128),
            stderr_snippet: Some("Authentication failed".to_string()),
            duration_ms: 100,
            shell: false,
            timed_out: false,
            external_actions: vec![ExternalActionEvidence::git_push(
                Some("origin".to_string()),
                vec!["main".to_string()],
            )],
        }),
    );

    engine.record_root_turn_contract_progress(&calls, &results);
    assert_eq!(
        engine.pending_external_action_consecutive_failures, 1,
        "counter should increment after first failed push"
    );

    // Second failed push attempt
    let calls2 = vec![ToolCall {
        id: "push-2".to_string(),
        name: "run_command".to_string(),
        arguments: serde_json::json!({
            "argv": ["git", "push", "origin", "main"]
        }),
    }];
    let results2 = vec![ToolResult {
        tool_call_id: "push-2".to_string(),
        tool_name: "run_command".to_string(),
        success: false,
        output: "Authentication failed".to_string(),
        failure_class: None,
    }];
    engine.pending_tool_result_diagnostics.insert(
        "push-2".to_string(),
        ToolExecutionDiagnostics::RunCommand(RunCommandDiagnostics {
            exit_code: Some(128),
            stderr_snippet: Some("Authentication failed".to_string()),
            duration_ms: 100,
            shell: false,
            timed_out: false,
            external_actions: vec![ExternalActionEvidence::git_push(
                Some("origin".to_string()),
                vec!["main".to_string()],
            )],
        }),
    );

    engine.record_root_turn_contract_progress(&calls2, &results2);
    assert_eq!(
        engine.pending_external_action_consecutive_failures, 2,
        "counter should increment after second failed push"
    );

    // Gate should now be lifted
    let tools = engine.apply_pending_external_action_gate(engine.tool_executor.tool_definitions());
    let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(
        tool_names.contains(&"read_file"),
        "gate should lift after 2 consecutive failures, got: {:?}",
        tool_names
    );
}

#[test]
fn external_action_gate_lifts_at_configured_single_failure_threshold() {
    let config = BudgetConfig {
        termination: TerminationConfig {
            external_action_gate_failure_lift_threshold: 1,
            ..TerminationConfig::default()
        },
        ..BudgetConfig::default()
    };
    let mut engine = mixed_tool_engine(config);
    engine.root_turn_contract = Some(RootTurnContract {
        deliverables: vec![RootTurnDeliverable::ExternalAction {
            kind: RootTurnExternalActionKind::GitPush,
            label: external_action_label(RootTurnExternalActionKind::GitPush).to_string(),
            satisfied: false,
        }],
        blocked_terminal_attempts: 0,
    });
    engine.pending_external_action_target = Some(RootTurnExternalActionKind::GitPush);

    let gated_tools =
        engine.apply_pending_external_action_gate(engine.tool_executor.tool_definitions());
    assert!(
        gated_tools.iter().all(|tool| tool.name != "read_file"),
        "gate should restrict inspection tools before the first failed push"
    );

    let calls = vec![ToolCall {
        id: "push-1".to_string(),
        name: "run_command".to_string(),
        arguments: serde_json::json!({
            "argv": ["git", "push", "origin", "main"]
        }),
    }];
    let results = vec![ToolResult {
        tool_call_id: "push-1".to_string(),
        tool_name: "run_command".to_string(),
        success: false,
        output: "Authentication failed".to_string(),
        failure_class: None,
    }];
    engine.pending_tool_result_diagnostics.insert(
        "push-1".to_string(),
        ToolExecutionDiagnostics::RunCommand(RunCommandDiagnostics {
            exit_code: Some(128),
            stderr_snippet: Some("Authentication failed".to_string()),
            duration_ms: 100,
            shell: false,
            timed_out: false,
            external_actions: vec![ExternalActionEvidence::git_push(
                Some("origin".to_string()),
                vec!["main".to_string()],
            )],
        }),
    );

    engine.record_root_turn_contract_progress(&calls, &results);
    assert_eq!(engine.pending_external_action_consecutive_failures, 1);

    let lifted_tools =
        engine.apply_pending_external_action_gate(engine.tool_executor.tool_definitions());
    assert!(
        lifted_tools.iter().any(|tool| tool.name == "read_file"),
        "gate should lift after one failed push when threshold is configured to 1"
    );
}

#[test]
fn external_action_failure_counter_resets_on_success() {
    let mut engine = mixed_tool_engine(BudgetConfig::default());
    engine.root_turn_contract = Some(RootTurnContract {
        deliverables: vec![RootTurnDeliverable::ExternalAction {
            kind: RootTurnExternalActionKind::GitPush,
            label: external_action_label(RootTurnExternalActionKind::GitPush).to_string(),
            satisfied: false,
        }],
        blocked_terminal_attempts: 0,
    });
    engine.pending_external_action_target = Some(RootTurnExternalActionKind::GitPush);
    engine.pending_external_action_consecutive_failures = 1;

    // Successful push
    let calls = vec![ToolCall {
        id: "push-ok".to_string(),
        name: "run_command".to_string(),
        arguments: serde_json::json!({
            "argv": ["git", "push", "origin", "main"]
        }),
    }];
    let results = vec![ToolResult {
        tool_call_id: "push-ok".to_string(),
        tool_name: "run_command".to_string(),
        success: true,
        output: "Everything up-to-date".to_string(),
        failure_class: None,
    }];
    engine.pending_tool_result_diagnostics.insert(
        "push-ok".to_string(),
        ToolExecutionDiagnostics::RunCommand(RunCommandDiagnostics {
            exit_code: Some(0),
            stderr_snippet: None,
            duration_ms: 50,
            shell: false,
            timed_out: false,
            external_actions: vec![ExternalActionEvidence::git_push(
                Some("origin".to_string()),
                vec!["main".to_string()],
            )],
        }),
    );

    engine.record_root_turn_contract_progress(&calls, &results);
    assert_eq!(
        engine.pending_external_action_consecutive_failures, 0,
        "counter should reset on success"
    );
    assert_eq!(
        engine.pending_external_action_target, None,
        "target should clear on success"
    );
}
