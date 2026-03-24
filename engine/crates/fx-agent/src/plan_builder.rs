//! Convert Claude tool calls to ActionPlan.

use crate::claude::error::{AgentError, Result};
use crate::claude::types::ToolUse;
use fx_core::types::{ActionPlan, ActionStep};
use std::collections::HashMap;

/// Builder for creating ActionPlans from tool calls.
pub struct PlanBuilder;

impl PlanBuilder {
    /// Convert Claude tool uses into an ActionPlan.
    pub fn from_tool_calls(tool_uses: &[ToolUse]) -> Result<ActionPlan> {
        if tool_uses.is_empty() {
            return Err(AgentError::ToolExecution(
                "No tool calls to convert".to_string(),
            ));
        }

        let steps = tool_uses
            .iter()
            .enumerate()
            .map(|(idx, tool_use)| Self::tool_use_to_step(idx, tool_use))
            .collect::<Result<Vec<_>>>()?;

        let description = Self::generate_description(&steps);

        Ok(ActionPlan {
            id: plan_id::generate().to_string(),
            steps,
            description,
            requires_confirmation: false,
        })
    }

    /// Convert a single ToolUse to an ActionStep.
    fn tool_use_to_step(idx: usize, tool_use: &ToolUse) -> Result<ActionStep> {
        let action = tool_use.name.clone();
        let target = Self::extract_target(tool_use)?;
        let parameters = Self::extract_parameters(tool_use)?;

        Ok(ActionStep {
            id: format!("step_{}", idx + 1),
            action,
            target,
            parameters,
            confirmation_required: false,
        })
    }

    /// Extract the target from a tool use.
    fn extract_target(tool_use: &ToolUse) -> Result<String> {
        // For most tools, target is in the input
        if let Some(target) = tool_use.input.get("target") {
            return Ok(target
                .as_str()
                .ok_or_else(|| AgentError::InvalidResponse("target must be a string".to_string()))?
                .to_string());
        }

        // For launch_app, the target is "name"
        if let Some(name) = tool_use.input.get("name") {
            return Ok(name
                .as_str()
                .ok_or_else(|| AgentError::InvalidResponse("name must be a string".to_string()))?
                .to_string());
        }

        // For tools without specific targets (go_home, go_back, read_screen)
        Ok(String::new())
    }

    /// Extract parameters from tool use input.
    fn extract_parameters(tool_use: &ToolUse) -> Result<HashMap<String, String>> {
        let mut params = HashMap::new();

        if let Some(obj) = tool_use.input.as_object() {
            for (key, value) in obj {
                // Skip keys that are used as targets
                if key == "target" || key == "name" {
                    continue;
                }

                // Convert value to string
                let value_str = match value {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Number(n) => n.to_string(),
                    serde_json::Value::Bool(b) => b.to_string(),
                    _ => value.to_string(),
                };

                params.insert(key.clone(), value_str);
            }
        }

        Ok(params)
    }

    /// Generate a human-readable description of the plan.
    fn generate_description(steps: &[ActionStep]) -> String {
        if steps.is_empty() {
            return "Empty action plan".to_string();
        }

        if steps.len() == 1 {
            return Self::step_description(&steps[0]);
        }

        let step_descs: Vec<String> = steps.iter().map(Self::step_description).collect();
        format!(
            "Execute {} steps: {}",
            steps.len(),
            step_descs.join(", then ")
        )
    }

    /// Generate a description for a single step.
    fn step_description(step: &ActionStep) -> String {
        match step.action.as_str() {
            "tap" => format!("tap on {}", step.target),
            "swipe" => {
                if let Some(dir) = step.parameters.get("direction") {
                    format!("swipe {}", dir)
                } else {
                    "swipe".to_string()
                }
            }
            "type_text" => {
                if let Some(text) = step.parameters.get("text") {
                    format!("type '{}'", text)
                } else {
                    "type text".to_string()
                }
            }
            "launch_app" => format!("launch {}", step.target),
            "go_home" => "go home".to_string(),
            "go_back" => "go back".to_string(),
            "read_screen" => "read screen".to_string(),
            _ => step.action.clone(),
        }
    }
}

// Simple UUID generation for plan IDs
// Note: Uses timestamp-based IDs (nanoseconds since UNIX epoch as hex).
// This is sufficient for plan identification in a single-agent context.
// For distributed systems, consider using a proper UUID library.
mod plan_id {
    use std::fmt;
    use std::time::{SystemTime, UNIX_EPOCH};

    pub fn generate() -> PlanId {
        PlanId::new()
    }

    pub struct PlanId(String);

    impl PlanId {
        fn new() -> Self {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_else(|_| std::time::Duration::from_secs(0))
                .as_nanos();
            Self(format!("{:032x}", now))
        }
    }

    impl fmt::Display for PlanId {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}", self.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_from_tool_calls_single_tap() {
        let tool_uses = vec![ToolUse {
            id: "call_1".to_string(),
            name: "tap".to_string(),
            input: json!({ "target": "Search button" }),
        }];

        let plan = PlanBuilder::from_tool_calls(&tool_uses).unwrap();
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].action, "tap");
        assert_eq!(plan.steps[0].target, "Search button");
        assert!(plan.description.contains("tap on Search button"));
    }

    #[test]
    fn test_from_tool_calls_multiple_steps() {
        let tool_uses = vec![
            ToolUse {
                id: "call_1".to_string(),
                name: "launch_app".to_string(),
                input: json!({ "name": "Chrome" }),
            },
            ToolUse {
                id: "call_2".to_string(),
                name: "tap".to_string(),
                input: json!({ "target": "Address bar" }),
            },
            ToolUse {
                id: "call_3".to_string(),
                name: "type_text".to_string(),
                input: json!({ "text": "example.com" }),
            },
        ];

        let plan = PlanBuilder::from_tool_calls(&tool_uses).unwrap();
        assert_eq!(plan.steps.len(), 3);
        assert_eq!(plan.steps[0].action, "launch_app");
        assert_eq!(plan.steps[0].target, "Chrome");
        assert_eq!(plan.steps[1].action, "tap");
        assert_eq!(plan.steps[2].action, "type_text");
        assert!(plan.description.contains("Execute 3 steps"));
    }

    #[test]
    fn test_from_tool_calls_empty() {
        let tool_uses: Vec<ToolUse> = vec![];
        let result = PlanBuilder::from_tool_calls(&tool_uses);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AgentError::ToolExecution(_)));
    }

    #[test]
    fn test_tool_use_to_step_swipe() {
        let tool_use = ToolUse {
            id: "call_1".to_string(),
            name: "swipe".to_string(),
            input: json!({ "direction": "up" }),
        };

        let step = PlanBuilder::tool_use_to_step(0, &tool_use).unwrap();
        assert_eq!(step.action, "swipe");
        assert_eq!(step.parameters.get("direction"), Some(&"up".to_string()));
    }

    #[test]
    fn test_tool_use_to_step_no_target() {
        let tool_use = ToolUse {
            id: "call_1".to_string(),
            name: "go_home".to_string(),
            input: json!({}),
        };

        let step = PlanBuilder::tool_use_to_step(0, &tool_use).unwrap();
        assert_eq!(step.action, "go_home");
        assert_eq!(step.target, "");
    }

    #[test]
    fn test_extract_target_from_target_field() {
        let tool_use = ToolUse {
            id: "call_1".to_string(),
            name: "tap".to_string(),
            input: json!({ "target": "Button" }),
        };

        let target = PlanBuilder::extract_target(&tool_use).unwrap();
        assert_eq!(target, "Button");
    }

    #[test]
    fn test_extract_target_from_name_field() {
        let tool_use = ToolUse {
            id: "call_1".to_string(),
            name: "launch_app".to_string(),
            input: json!({ "name": "Gmail" }),
        };

        let target = PlanBuilder::extract_target(&tool_use).unwrap();
        assert_eq!(target, "Gmail");
    }

    #[test]
    fn test_extract_target_none() {
        let tool_use = ToolUse {
            id: "call_1".to_string(),
            name: "go_back".to_string(),
            input: json!({}),
        };

        let target = PlanBuilder::extract_target(&tool_use).unwrap();
        assert_eq!(target, "");
    }

    #[test]
    fn test_extract_parameters() {
        let tool_use = ToolUse {
            id: "call_1".to_string(),
            name: "type_text".to_string(),
            input: json!({ "text": "Hello world", "extra": "value" }),
        };

        let params = PlanBuilder::extract_parameters(&tool_use).unwrap();
        assert_eq!(params.get("text"), Some(&"Hello world".to_string()));
        assert_eq!(params.get("extra"), Some(&"value".to_string()));
    }

    #[test]
    fn test_extract_parameters_excludes_target_fields() {
        let tool_use = ToolUse {
            id: "call_1".to_string(),
            name: "tap".to_string(),
            input: json!({ "target": "Button", "extra": "param" }),
        };

        let params = PlanBuilder::extract_parameters(&tool_use).unwrap();
        assert!(!params.contains_key("target"));
        assert_eq!(params.get("extra"), Some(&"param".to_string()));
    }

    #[test]
    fn test_generate_description_single_step() {
        let steps = vec![ActionStep {
            id: "step_1".to_string(),
            action: "tap".to_string(),
            target: "Search".to_string(),
            parameters: HashMap::new(),
            confirmation_required: false,
        }];

        let desc = PlanBuilder::generate_description(&steps);
        assert_eq!(desc, "tap on Search");
    }

    #[test]
    fn test_generate_description_multiple_steps() {
        let steps = vec![
            ActionStep {
                id: "step_1".to_string(),
                action: "launch_app".to_string(),
                target: "Chrome".to_string(),
                parameters: HashMap::new(),
                confirmation_required: false,
            },
            ActionStep {
                id: "step_2".to_string(),
                action: "go_home".to_string(),
                target: "".to_string(),
                parameters: HashMap::new(),
                confirmation_required: false,
            },
        ];

        let desc = PlanBuilder::generate_description(&steps);
        assert!(desc.contains("Execute 2 steps"));
        assert!(desc.contains("launch Chrome"));
        assert!(desc.contains("go home"));
    }

    #[test]
    fn test_step_description() {
        let step = ActionStep {
            id: "step_1".to_string(),
            action: "swipe".to_string(),
            target: "".to_string(),
            parameters: {
                let mut map = HashMap::new();
                map.insert("direction".to_string(), "left".to_string());
                map
            },
            confirmation_required: false,
        };

        let desc = PlanBuilder::step_description(&step);
        assert_eq!(desc, "swipe left");
    }
}
