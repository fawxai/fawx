//! Policy engine for evaluating actions against security policies.

use super::types::{PolicyConfig, PolicyDecision, PolicyRule};
use nv_core::error::SecurityError;
use nv_core::types::{ActionPlan, ActionStep};
use std::fs;
use std::path::Path;

/// Policy engine that evaluates actions against loaded policies.
pub struct PolicyEngine {
    config: PolicyConfig,
}

impl PolicyEngine {
    /// Load policy from TOML string.
    ///
    /// # Arguments
    /// * `content` - TOML policy configuration
    ///
    /// # Returns
    /// PolicyEngine on success, SecurityError on parse failure
    pub fn from_toml(content: &str) -> Result<Self, SecurityError> {
        let config = toml::from_str(content).map_err(|e| {
            SecurityError::PolicyViolation(format!("Failed to parse policy TOML: {}", e))
        })?;

        Ok(Self { config })
    }

    /// Load policy from TOML file.
    ///
    /// # Arguments
    /// * `path` - Path to TOML policy file
    ///
    /// # Returns
    /// PolicyEngine on success, SecurityError on I/O or parse failure
    pub fn from_file(path: &Path) -> Result<Self, SecurityError> {
        let content = fs::read_to_string(path).map_err(|e| {
            SecurityError::PolicyViolation(format!("Failed to read policy file: {}", e))
        })?;

        Self::from_toml(&content)
    }

    /// Evaluate a single action step against the policy.
    ///
    /// # Arguments
    /// * `action` - Action step to evaluate
    ///
    /// # Returns
    /// PolicyDecision for the action
    ///
    /// # Note
    /// Currently only evaluates action patterns. The following ActionStep fields
    /// are not yet used in evaluation:
    /// - `target` - Future: could be used for condition matching
    /// - `parameters` - Future: could be used for parameter validation
    /// - `confirmation_required` - Future: could override policy decision
    ///
    /// TODO: Implement condition evaluation (PolicyRule.conditions) to check:
    /// - Time of day constraints
    /// - App target matching
    /// - Contact target matching
    pub fn evaluate_action(&self, action: &ActionStep) -> PolicyDecision {
        // Find first matching rule
        // TODO: Check rule.conditions here once condition evaluation is implemented
        for rule in &self.config.rules {
            if matches_pattern(&rule.action, &action.action) {
                return rule_to_decision(rule);
            }
        }

        // No rule matched, use default
        default_to_decision(&self.config.default.decision)
    }

    /// Evaluate an entire action plan against the policy.
    ///
    /// # Arguments
    /// * `plan` - Action plan to evaluate
    ///
    /// # Returns
    /// Vector of (step_id, decision) tuples for each step
    pub fn evaluate_plan(&self, plan: &ActionPlan) -> Vec<(String, PolicyDecision)> {
        plan.steps
            .iter()
            .map(|step| (step.id.clone(), self.evaluate_action(step)))
            .collect()
    }
}

/// Check if an action matches a pattern (simple wildcard support).
///
/// # Arguments
/// * `pattern` - Pattern string (supports "*" wildcard at end only)
/// * `action` - Action name to match
///
/// # Returns
/// `true` if action matches pattern
///
/// # Limitations
/// This is a simple pattern matcher that only supports:
/// - Exact matches: "launch_app" matches "launch_app"
/// - Trailing wildcards: "delete_*" matches "delete_file", "delete_contact"
///
/// NOT supported (may be added in future):
/// - Leading wildcards: "*_file"
/// - Mid-string wildcards: "delete_*_file"
/// - Multiple wildcards: "delete_*_*"
/// - Glob syntax: "delete_{file,contact}"
///
/// TODO: Consider using a full glob library (e.g., `globset`) for richer matching
fn matches_pattern(pattern: &str, action: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix('*') {
        // Wildcard match: "delete_*" matches "delete_file", "delete_contact", etc.
        action.starts_with(prefix)
    } else {
        // Exact match
        pattern == action
    }
}

/// Convert a policy rule to a decision.
fn rule_to_decision(rule: &PolicyRule) -> PolicyDecision {
    match rule.decision.as_str() {
        "allow" => PolicyDecision::Allow,
        "deny" => PolicyDecision::Deny {
            reason: rule
                .reason
                .clone()
                .unwrap_or_else(|| "Denied by policy".to_string()),
        },
        "confirm" => PolicyDecision::Confirm {
            prompt: rule
                .prompt
                .clone()
                .unwrap_or_else(|| "Confirm this action?".to_string()),
        },
        _ => PolicyDecision::Deny {
            reason: format!("Unknown decision type: {}", rule.decision),
        },
    }
}

/// Convert default decision string to PolicyDecision.
fn default_to_decision(decision: &str) -> PolicyDecision {
    match decision {
        "allow" => PolicyDecision::Allow,
        "deny" => PolicyDecision::Deny {
            reason: "Denied by default policy".to_string(),
        },
        "confirm" => PolicyDecision::Confirm {
            prompt: "Confirm this action?".to_string(),
        },
        _ => PolicyDecision::Deny {
            reason: format!("Unknown default decision: {}", decision),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn create_test_step(action: &str) -> ActionStep {
        ActionStep {
            id: "step1".to_string(),
            action: action.to_string(),
            target: "test_target".to_string(),
            parameters: HashMap::new(),
            confirmation_required: false,
        }
    }

    #[test]
    fn test_load_valid_toml() {
        let toml = r#"
            [default]
            decision = "confirm"

            [[rules]]
            action = "launch_app"
            decision = "allow"
        "#;

        let engine = PolicyEngine::from_toml(toml);
        assert!(engine.is_ok());
    }

    #[test]
    fn test_load_invalid_toml() {
        let toml = "this is not valid toml {[}";
        let engine = PolicyEngine::from_toml(toml);
        assert!(engine.is_err());
    }

    #[test]
    fn test_evaluate_allow_rule() {
        let toml = r#"
            [default]
            decision = "deny"

            [[rules]]
            action = "launch_app"
            decision = "allow"
        "#;

        let engine = PolicyEngine::from_toml(toml).unwrap();
        let step = create_test_step("launch_app");

        assert!(matches!(
            engine.evaluate_action(&step),
            PolicyDecision::Allow
        ));
    }

    #[test]
    fn test_evaluate_deny_rule() {
        let toml = r#"
            [default]
            decision = "allow"

            [[rules]]
            action = "delete_file"
            decision = "deny"
            reason = "Deletion not allowed"
        "#;

        let engine = PolicyEngine::from_toml(toml).unwrap();
        let step = create_test_step("delete_file");

        match engine.evaluate_action(&step) {
            PolicyDecision::Deny { reason } => {
                assert_eq!(reason, "Deletion not allowed");
            }
            _ => panic!("Expected Deny decision"),
        }
    }

    #[test]
    fn test_evaluate_confirm_rule() {
        let toml = r#"
            [default]
            decision = "allow"

            [[rules]]
            action = "send_message"
            decision = "confirm"
            prompt = "Send this message?"
        "#;

        let engine = PolicyEngine::from_toml(toml).unwrap();
        let step = create_test_step("send_message");

        match engine.evaluate_action(&step) {
            PolicyDecision::Confirm { prompt } => {
                assert_eq!(prompt, "Send this message?");
            }
            _ => panic!("Expected Confirm decision"),
        }
    }

    #[test]
    fn test_default_decision() {
        let toml = r#"
            [default]
            decision = "confirm"

            [[rules]]
            action = "launch_app"
            decision = "allow"
        "#;

        let engine = PolicyEngine::from_toml(toml).unwrap();
        let step = create_test_step("unknown_action");

        assert!(matches!(
            engine.evaluate_action(&step),
            PolicyDecision::Confirm { .. }
        ));
    }

    #[test]
    fn test_wildcard_match() {
        let toml = r#"
            [default]
            decision = "allow"

            [[rules]]
            action = "delete_*"
            decision = "deny"
            reason = "Deletions forbidden"
        "#;

        let engine = PolicyEngine::from_toml(toml).unwrap();

        let step1 = create_test_step("delete_file");
        let step2 = create_test_step("delete_contact");

        assert!(matches!(
            engine.evaluate_action(&step1),
            PolicyDecision::Deny { .. }
        ));
        assert!(matches!(
            engine.evaluate_action(&step2),
            PolicyDecision::Deny { .. }
        ));
    }

    #[test]
    fn test_wildcard_no_match() {
        let toml = r#"
            [default]
            decision = "allow"

            [[rules]]
            action = "delete_*"
            decision = "deny"
        "#;

        let engine = PolicyEngine::from_toml(toml).unwrap();
        let step = create_test_step("send_message");

        assert!(matches!(
            engine.evaluate_action(&step),
            PolicyDecision::Allow
        ));
    }

    #[test]
    fn test_evaluate_plan() {
        let toml = r#"
            [default]
            decision = "allow"

            [[rules]]
            action = "launch_app"
            decision = "allow"

            [[rules]]
            action = "send_message"
            decision = "confirm"
        "#;

        let engine = PolicyEngine::from_toml(toml).unwrap();

        let plan = ActionPlan {
            id: "plan1".to_string(),
            steps: vec![
                create_test_step("launch_app"),
                create_test_step("send_message"),
            ],
            description: "Test plan".to_string(),
            requires_confirmation: false,
        };

        let results = engine.evaluate_plan(&plan);
        assert_eq!(results.len(), 2);

        assert!(matches!(results[0].1, PolicyDecision::Allow));
        assert!(matches!(results[1].1, PolicyDecision::Confirm { .. }));
    }

    #[test]
    fn test_first_match_wins() {
        let toml = r#"
            [default]
            decision = "allow"

            [[rules]]
            action = "test_action"
            decision = "deny"

            [[rules]]
            action = "test_action"
            decision = "allow"
        "#;

        let engine = PolicyEngine::from_toml(toml).unwrap();
        let step = create_test_step("test_action");

        // First rule (deny) should win
        assert!(matches!(
            engine.evaluate_action(&step),
            PolicyDecision::Deny { .. }
        ));
    }

    #[test]
    fn test_empty_policy() {
        let toml = r#"
            [default]
            decision = "allow"

            [[rules]]
        "#;

        // This should fail to parse due to invalid TOML
        let engine = PolicyEngine::from_toml(toml);
        assert!(engine.is_err());
    }

    #[test]
    fn test_policy_with_no_rules() {
        let toml = r#"[default]
decision = "deny"
"#;

        let engine = PolicyEngine::from_toml(toml).unwrap();
        let step = create_test_step("any_action");

        assert!(matches!(
            engine.evaluate_action(&step),
            PolicyDecision::Deny { .. }
        ));
    }

    #[test]
    fn test_matches_pattern_exact() {
        assert!(matches_pattern("launch_app", "launch_app"));
        assert!(!matches_pattern("launch_app", "launch_app2"));
    }

    #[test]
    fn test_matches_pattern_wildcard() {
        assert!(matches_pattern("delete_*", "delete_file"));
        assert!(matches_pattern("delete_*", "delete_contact"));
        assert!(matches_pattern("delete_*", "delete_"));
        assert!(!matches_pattern("delete_*", "send_message"));
    }
}
