//! Decide-step intent-to-decision translation.

use crate::types::{IntendedAction, ReasonedIntent};
use fx_decompose::DecompositionPlan;
use fx_llm::ToolCall;
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
    /// Execute a decomposition plan with sub-goals.
    Decompose(DecompositionPlan),
}

/// Error surfaced while translating an intent to executable decision(s).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DecideError {
    /// High-level stage for diagnostics.
    pub stage: String,
    /// Human-readable failure reason.
    pub reason: String,
}

impl std::fmt::Display for DecideError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.stage, self.reason)
    }
}

impl std::error::Error for DecideError {}

type ParamSerializer = fn(&std::collections::HashMap<String, String>) -> Result<Value, DecideError>;

pub(crate) const CONFIDENCE_DEFER_THRESHOLD: f64 = 0.15;
pub(crate) const CONFIDENCE_CLARIFY_THRESHOLD: f64 = 0.35;

/// Convert a [`ReasonedIntent`] into a concrete [`Decision`].
pub fn decide_from_intent(intent: &ReasonedIntent) -> Result<Decision, DecideError> {
    decide_from_intent_with_serializer(intent, serialize_params)
}

fn decide_from_intent_with_serializer(
    intent: &ReasonedIntent,
    serializer: ParamSerializer,
) -> Result<Decision, DecideError> {
    if f64::from(intent.confidence) < CONFIDENCE_DEFER_THRESHOLD {
        return Ok(Decision::Defer(
            "I'm not confident enough to act safely yet. Could you rephrase?".to_string(),
        ));
    }

    if f64::from(intent.confidence) < CONFIDENCE_CLARIFY_THRESHOLD {
        return Ok(Decision::Clarify(
            "Can you share a little more detail so I can get this right?".to_string(),
        ));
    }

    map_action_to_decision(&intent.action, serializer)
}

fn map_action_to_decision(
    action: &IntendedAction,
    serializer: ParamSerializer,
) -> Result<Decision, DecideError> {
    match action {
        IntendedAction::Respond { text } => Ok(Decision::Respond(text.clone())),
        IntendedAction::Delegate { skill_id, params } => {
            let call = delegate_tool_call(skill_id, params, "tool", serializer)?;
            Ok(Decision::UseTools(vec![call]))
        }
        IntendedAction::Composite(actions) => {
            let tool_calls = collect_composite_calls(actions, serializer)?;
            Ok(composite_decision(tool_calls))
        }
        _ => map_single_action(action, serializer),
    }
}

fn map_single_action(
    action: &IntendedAction,
    serializer: ParamSerializer,
) -> Result<Decision, DecideError> {
    match action_to_tool_call(action, 0, serializer)? {
        Some(call) => Ok(Decision::UseTools(vec![call])),
        None => Ok(Decision::Clarify(
            "I couldn't map that plan to an executable action yet.".to_string(),
        )),
    }
}

fn collect_composite_calls(
    actions: &[IntendedAction],
    serializer: ParamSerializer,
) -> Result<Vec<ToolCall>, DecideError> {
    actions
        .iter()
        .enumerate()
        .try_fold(Vec::new(), |mut calls, (index, action)| {
            if let Some(call) = action_to_tool_call(action, index, serializer)? {
                calls.push(call);
            }
            Ok(calls)
        })
}

fn composite_decision(tool_calls: Vec<ToolCall>) -> Decision {
    if tool_calls.is_empty() {
        Decision::Clarify("I need a little more context before taking the next step.".to_string())
    } else {
        Decision::UseTools(tool_calls)
    }
}

fn action_to_tool_call(
    action: &IntendedAction,
    index: usize,
    serializer: ParamSerializer,
) -> Result<Option<ToolCall>, DecideError> {
    match action {
        IntendedAction::Tap { target, fallback } => {
            Ok(Some(tap_tool_call(index, target, fallback.as_deref())))
        }
        IntendedAction::Type { text, target } => Ok(Some(type_tool_call(index, text, target))),
        IntendedAction::Swipe { direction, target } => {
            Ok(Some(swipe_tool_call(index, direction, target.as_deref())))
        }
        IntendedAction::LaunchApp { package } => Ok(Some(launch_tool_call(index, package))),
        IntendedAction::Navigate { destination } => {
            Ok(Some(navigate_tool_call(index, destination)))
        }
        IntendedAction::Wait {
            condition,
            timeout_ms,
        } => Ok(Some(wait_tool_call(index, condition, *timeout_ms))),
        IntendedAction::Delegate { skill_id, params } => delegate_tool_call(
            skill_id,
            params,
            &format!("tool-delegate-{index}"),
            serializer,
        )
        .map(Some),
        IntendedAction::Respond { .. } | IntendedAction::Composite(_) => Ok(None),
    }
}

fn tap_tool_call(index: usize, target: &str, fallback: Option<&str>) -> ToolCall {
    ToolCall {
        id: format!("tool-tap-{index}"),
        name: "tap".to_string(),
        arguments: serde_json::json!({
            "target": target,
            "fallback": fallback,
        }),
    }
}

fn type_tool_call(index: usize, text: &str, target: &str) -> ToolCall {
    ToolCall {
        id: format!("tool-type-{index}"),
        name: "type".to_string(),
        arguments: serde_json::json!({
            "text": text,
            "target": target,
        }),
    }
}

fn swipe_tool_call(
    index: usize,
    direction: &fx_core::types::SwipeDirection,
    target: Option<&str>,
) -> ToolCall {
    ToolCall {
        id: format!("tool-swipe-{index}"),
        name: "swipe".to_string(),
        arguments: serde_json::json!({
            "direction": direction,
            "target": target,
        }),
    }
}

fn launch_tool_call(index: usize, package: &str) -> ToolCall {
    ToolCall {
        id: format!("tool-launch-{index}"),
        name: "launch_app".to_string(),
        arguments: serde_json::json!({ "package": package }),
    }
}

fn navigate_tool_call(index: usize, destination: &str) -> ToolCall {
    ToolCall {
        id: format!("tool-nav-{index}"),
        name: "navigate".to_string(),
        arguments: serde_json::json!({ "destination": destination }),
    }
}

fn wait_tool_call(index: usize, condition: &str, timeout_ms: u64) -> ToolCall {
    ToolCall {
        id: format!("tool-wait-{index}"),
        name: "wait".to_string(),
        arguments: serde_json::json!({
            "condition": condition,
            "timeout_ms": timeout_ms,
        }),
    }
}

fn delegate_tool_call(
    skill_id: &str,
    params: &std::collections::HashMap<String, String>,
    id: &str,
    serializer: ParamSerializer,
) -> Result<ToolCall, DecideError> {
    Ok(ToolCall {
        id: id.to_string(),
        name: skill_id.to_string(),
        arguments: serializer(params)?,
    })
}

fn serialize_params(
    params: &std::collections::HashMap<String, String>,
) -> Result<Value, DecideError> {
    serde_json::to_value(params).map_err(|error| DecideError {
        stage: "decide".to_string(),
        reason: format!("failed to serialize delegate params: {error}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Goal;
    use fx_decompose::{AggregationStrategy, SubGoal};
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
        ))
        .expect("decision");

        assert!(matches!(decision, Decision::Respond(text) if text == "hello"));
    }

    #[test]
    fn decide_low_confidence_leads_to_clarify_or_defer() {
        let clarify = decide_from_intent(&intent(
            IntendedAction::Respond {
                text: "hello".to_string(),
            },
            0.2,
        ))
        .expect("clarify decision");
        let defer = decide_from_intent(&intent(
            IntendedAction::Respond {
                text: "hello".to_string(),
            },
            0.1,
        ))
        .expect("defer decision");

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
        ))
        .expect("delegate decision");

        assert!(
            matches!(decision, Decision::UseTools(ref calls) if calls.len() == 1 && calls[0].name == "search")
        );
    }

    #[test]
    fn decide_composite_without_executable_actions_requests_clarification() {
        let decision = decide_from_intent(&intent(
            IntendedAction::Composite(vec![IntendedAction::Respond {
                text: "done".to_string(),
            }]),
            0.95,
        ))
        .expect("composite decision");

        assert!(matches!(decision, Decision::Clarify(_)));
    }

    #[test]
    fn decide_from_intent_returns_error_when_delegate_serialization_fails() {
        fn failing_serializer(
            _params: &std::collections::HashMap<String, String>,
        ) -> Result<Value, DecideError> {
            Err(DecideError {
                stage: "decide".to_string(),
                reason: "forced serializer failure".to_string(),
            })
        }

        let result = decide_from_intent_with_serializer(
            &intent(
                IntendedAction::Delegate {
                    skill_id: "search".to_string(),
                    params: HashMap::from([("q".to_string(), "weather".to_string())]),
                },
                0.9,
            ),
            failing_serializer,
        );

        assert!(matches!(result, Err(error) if error.reason.contains("forced serializer failure")));
    }

    #[test]
    fn decision_decompose_variant_constructs_with_plan() {
        let plan = DecompositionPlan {
            sub_goals: vec![SubGoal::with_definition_of_done(
                "inspect logs",
                vec!["read_file".to_string()],
                Some("log summary"),
                None,
            )],
            strategy: AggregationStrategy::Sequential,
            truncated_from: None,
        };
        let decision = Decision::Decompose(plan.clone());

        assert!(matches!(
            decision,
            Decision::Decompose(ref captured) if captured == &plan
        ));
    }
}
