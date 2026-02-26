//! Decide-step intent-to-decision translation.

use crate::types::{IntendedAction, ReasonedIntent};
use ct_llm::ToolCall;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Decision selected after reasoning and gate checks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Decision {
    /// Return direct text to the user.
    Respond(String),
    /// Execute one or more tools before responding.
    UseTools(Vec<ToolCall>),
    /// Ask the user for clarification.
    Clarify(String),
    /// Decline/defer handling with an explanation.
    Defer(String),
}

/// Convert a [`ReasonedIntent`] into a concrete [`Decision`].
pub fn decide_from_intent(intent: &ReasonedIntent) -> Decision {
    if intent.confidence < 0.15 {
        return Decision::Defer(
            "I'm not confident enough to act safely yet. Could you rephrase?".to_string(),
        );
    }

    if intent.confidence < 0.35 {
        return Decision::Clarify(
            "Can you share a little more detail so I can get this right?".to_string(),
        );
    }

    match &intent.action {
        IntendedAction::Respond { text } => Decision::Respond(text.clone()),
        IntendedAction::Delegate { skill_id, params } => {
            let arguments = serde_json::to_value(params).unwrap_or(Value::Null);
            Decision::UseTools(vec![ToolCall {
                id: format!("tool-{skill_id}"),
                name: skill_id.clone(),
                arguments,
            }])
        }
        IntendedAction::Composite(actions) => {
            let tool_calls = actions
                .iter()
                .enumerate()
                .filter_map(|(index, action)| action_to_tool_call(action, index))
                .collect::<Vec<_>>();

            if tool_calls.is_empty() {
                Decision::Clarify(
                    "I need a little more context before taking the next step.".to_string(),
                )
            } else {
                Decision::UseTools(tool_calls)
            }
        }
        action => {
            if let Some(call) = action_to_tool_call(action, 0) {
                Decision::UseTools(vec![call])
            } else {
                Decision::Clarify(
                    "I couldn't map that plan to an executable action yet.".to_string(),
                )
            }
        }
    }
}

fn action_to_tool_call(action: &IntendedAction, index: usize) -> Option<ToolCall> {
    match action {
        IntendedAction::Tap { target, fallback } => Some(ToolCall {
            id: format!("tool-tap-{index}"),
            name: "tap".to_string(),
            arguments: serde_json::json!({
                "target": target,
                "fallback": fallback,
            }),
        }),
        IntendedAction::Type { text, target } => Some(ToolCall {
            id: format!("tool-type-{index}"),
            name: "type".to_string(),
            arguments: serde_json::json!({
                "text": text,
                "target": target,
            }),
        }),
        IntendedAction::Swipe { direction, target } => Some(ToolCall {
            id: format!("tool-swipe-{index}"),
            name: "swipe".to_string(),
            arguments: serde_json::json!({
                "direction": direction,
                "target": target,
            }),
        }),
        IntendedAction::LaunchApp { package } => Some(ToolCall {
            id: format!("tool-launch-{index}"),
            name: "launch_app".to_string(),
            arguments: serde_json::json!({ "package": package }),
        }),
        IntendedAction::Navigate { destination } => Some(ToolCall {
            id: format!("tool-nav-{index}"),
            name: "navigate".to_string(),
            arguments: serde_json::json!({ "destination": destination }),
        }),
        IntendedAction::Wait {
            condition,
            timeout_ms,
        } => Some(ToolCall {
            id: format!("tool-wait-{index}"),
            name: "wait".to_string(),
            arguments: serde_json::json!({
                "condition": condition,
                "timeout_ms": timeout_ms,
            }),
        }),
        IntendedAction::Delegate { skill_id, params } => Some(ToolCall {
            id: format!("tool-delegate-{index}"),
            name: skill_id.clone(),
            arguments: serde_json::to_value(params).unwrap_or(Value::Null),
        }),
        IntendedAction::Respond { .. } | IntendedAction::Composite(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Goal;
    use std::collections::HashMap;

    fn intent(action: IntendedAction, confidence: f32) -> ReasonedIntent {
        ReasonedIntent {
            action,
            rationale: "test rationale".to_string(),
            confidence,
            expected_outcome: None,
            sub_goals: vec![Goal::new("respond", vec!["done".to_string()], Some(1))],
        }
    }

    #[test]
    fn decide_maps_respond_action_to_respond_decision() {
        let decision = decide_from_intent(&intent(
            IntendedAction::Respond {
                text: "hello".to_string(),
            },
            0.8,
        ));

        assert!(matches!(decision, Decision::Respond(text) if text == "hello"));
    }

    #[test]
    fn decide_low_confidence_leads_to_clarify_or_defer() {
        let clarify = decide_from_intent(&intent(
            IntendedAction::Respond {
                text: "hello".to_string(),
            },
            0.2,
        ));
        let defer = decide_from_intent(&intent(
            IntendedAction::Respond {
                text: "hello".to_string(),
            },
            0.1,
        ));

        assert!(matches!(clarify, Decision::Clarify(_)));
        assert!(matches!(defer, Decision::Defer(_)));
    }

    #[test]
    fn decide_delegate_becomes_tool_call() {
        let mut params = HashMap::new();
        params.insert("q".to_string(), "weather".to_string());

        let decision = decide_from_intent(&intent(
            IntendedAction::Delegate {
                skill_id: "search".to_string(),
                params,
            },
            0.9,
        ));

        assert!(
            matches!(decision, Decision::UseTools(ref calls) if calls.len() == 1 && calls[0].name == "search")
        );
    }
}
