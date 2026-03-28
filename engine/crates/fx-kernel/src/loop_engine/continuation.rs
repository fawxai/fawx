use crate::act::{ContinuationToolScope, ProceedUnderConstraints, TurnCommitment};
use crate::decide::Decision;

pub(super) fn commitment_tool_scope(
    commitment: Option<&TurnCommitment>,
) -> Option<ContinuationToolScope> {
    match commitment {
        Some(TurnCommitment::ProceedUnderConstraints(commitment)) => {
            commitment.allowed_tools.clone()
        }
        Some(TurnCommitment::NeedsDirection(_)) | None => None,
    }
}

pub(super) fn turn_commitment_metadata(commitment: &TurnCommitment) -> serde_json::Value {
    match commitment {
        TurnCommitment::ProceedUnderConstraints(commitment) => serde_json::json!({
            "variant": "proceed_under_constraints",
            "goal": commitment.goal,
            "success_target": commitment.success_target,
            "unsupported_items": commitment.unsupported_items,
            "assumptions": commitment.assumptions,
            "allowed_tools": commitment.allowed_tools.as_ref().map(render_tool_scope_label),
        }),
        TurnCommitment::NeedsDirection(commitment) => serde_json::json!({
            "variant": "needs_direction",
            "question": commitment.question,
            "blocking_choice": commitment.blocking_choice,
        }),
    }
}

pub(super) fn render_turn_commitment_directive(commitment: &TurnCommitment) -> String {
    match commitment {
        TurnCommitment::ProceedUnderConstraints(commitment) => {
            let mut directive = String::from(
                "You are operating under a committed constrained execution plan for this turn.\n",
            );
            directive.push_str(&format!("Committed goal: {}\n", commitment.goal));
            directive.push_str(
                "Required behavior:\n- Continue with concrete action instead of reopening broad research or re-verifying already-established facts.\n- Stay within the committed tool surface.\n- Ask the user one concise blocking question only if you cannot proceed within these constraints.\n",
            );
            if let Some(scope) = &commitment.allowed_tools {
                directive.push_str(&format!(
                    "Allowed tool surface: {}\n",
                    render_tool_scope_label(scope)
                ));
            }
            if let Some(success_target) = commitment.success_target.as_deref() {
                directive.push_str(&format!("Success target: {success_target}\n"));
            }
            if !commitment.unsupported_items.is_empty() {
                directive.push_str("Unsupported or provisional items:\n");
                for item in &commitment.unsupported_items {
                    directive.push_str("- ");
                    directive.push_str(item);
                    directive.push('\n');
                }
            }
            if !commitment.assumptions.is_empty() {
                directive.push_str("Current assumptions:\n");
                for assumption in &commitment.assumptions {
                    directive.push_str("- ");
                    directive.push_str(assumption);
                    directive.push('\n');
                }
            }
            directive.trim_end().to_string()
        }
        TurnCommitment::NeedsDirection(commitment) => format!(
            "A blocking decision remains for this turn.\nBlocking choice: {}\nQuestion to ask: {}\nAsk exactly one concise question and stop after asking it. Do not continue broad research or implementation until the user answers.",
            commitment.blocking_choice, commitment.question
        ),
    }
}

pub(super) fn render_tool_scope_label(scope: &ContinuationToolScope) -> String {
    match scope {
        ContinuationToolScope::Full => "full tool surface".to_string(),
        ContinuationToolScope::MutationOnly => {
            "mutation-only tool surface (side-effect-capable tools only)".to_string()
        }
        ContinuationToolScope::Only(names) => {
            format!("named tools only: {}", names.join(", "))
        }
    }
}

fn decision_execution_goal(decision: &Decision) -> String {
    match decision {
        Decision::UseTools(calls) => {
            let tool_names: Vec<&str> = calls.iter().map(|call| call.name.as_str()).collect();
            if tool_names.is_empty() {
                "Continue the active task with concrete execution.".to_string()
            } else {
                format!(
                    "Continue the active task with concrete execution using the selected tools: {}",
                    tool_names.join(", ")
                )
            }
        }
        Decision::Decompose(plan) => format!(
            "Continue executing the active task after decomposing it into {} sub-goals",
            plan.sub_goals.len()
        ),
        Decision::Respond(_) => {
            "Continue the active task and prepare the next user-facing response.".to_string()
        }
        Decision::Clarify(_) => {
            "Resolve the active task by asking one focused clarifying question.".to_string()
        }
        Decision::Defer(_) => {
            "Resolve the active task by clearly explaining the current blocker or deferral."
                .to_string()
        }
    }
}

fn constrained_execution_success_target(scope: &ContinuationToolScope) -> String {
    match scope {
        ContinuationToolScope::Full => {
            "Continue making concrete progress on the active task without reopening broad research."
                .to_string()
        }
        ContinuationToolScope::MutationOnly => {
            "Use a side-effect-capable tool to make concrete forward progress before doing any more broad research."
                .to_string()
        }
        ContinuationToolScope::Only(names) => format!(
            "Continue by using only these committed tools: {}",
            names.join(", ")
        ),
    }
}

pub(super) fn tool_continuation_turn_commitment(
    decision: &Decision,
    next_tool_scope: Option<&ContinuationToolScope>,
) -> Option<TurnCommitment> {
    let allowed_tools = next_tool_scope
        .cloned()
        .filter(|scope| !matches!(scope, ContinuationToolScope::Full))?;
    Some(TurnCommitment::ProceedUnderConstraints(
        ProceedUnderConstraints {
            goal: decision_execution_goal(decision),
            success_target: Some(constrained_execution_success_target(&allowed_tools)),
            unsupported_items: Vec::new(),
            assumptions: Vec::new(),
            allowed_tools: Some(allowed_tools),
        },
    ))
}

pub(super) fn tool_continuation_artifact_write_target(
    requested_artifact_target: Option<&str>,
    next_tool_scope: Option<&ContinuationToolScope>,
) -> Option<String> {
    let requested_artifact_target = requested_artifact_target?;
    match next_tool_scope {
        Some(ContinuationToolScope::MutationOnly) => Some(requested_artifact_target.to_string()),
        Some(ContinuationToolScope::Only(names))
            if names.iter().any(|name| name == "write_file") =>
        {
            Some(requested_artifact_target.to_string())
        }
        Some(ContinuationToolScope::Only(_)) => None,
        Some(ContinuationToolScope::Full) | None => None,
    }
}
