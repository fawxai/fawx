//! Policy engine for evaluating actions against security policies.

use super::signing::verify_policy;
use super::types::{Condition, PolicyConfig, PolicyDecision, PolicyRule};
use super::util::matches_action;
use fx_core::error::SecurityError;
use fx_core::types::{ActionPlan, ActionStep};
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
    pub async fn from_file(path: &Path) -> Result<Self, SecurityError> {
        let content = tokio::fs::read_to_string(path).await.map_err(|e| {
            SecurityError::PolicyViolation(format!("Failed to read policy file: {}", e))
        })?;

        Self::from_toml(&content)
    }

    /// Load policy from a signed TOML file with verification.
    ///
    /// Reads the policy file and its `.sig` sidecar file, verifies the HMAC
    /// signature, then loads the policy.
    ///
    /// # Arguments
    /// * `path` - Path to TOML policy file (signature file should be at `path.sig`)
    /// * `key` - Secret key for signature verification
    ///
    /// # Returns
    /// PolicyEngine on success, SecurityError if signature verification fails
    ///
    /// # Example
    ///
    /// ```ignore
    /// let engine = PolicyEngine::from_signed_file(
    ///     Path::new("policy.toml"),
    ///     b"secret_key"
    /// ).await?;
    /// ```
    pub async fn from_signed_file(path: &Path, key: &[u8]) -> Result<Self, SecurityError> {
        // Read policy file
        let content = tokio::fs::read(path).await.map_err(|e| {
            SecurityError::PolicyViolation(format!("Failed to read policy file: {}", e))
        })?;

        // Read signature file (.sig sidecar)
        let mut sig_path = path.to_path_buf();
        sig_path.as_mut_os_string().push(".sig");
        let signature = tokio::fs::read(&sig_path).await.map_err(|e| {
            SecurityError::SignatureVerification(format!("Failed to read signature file: {}", e))
        })?;

        // Verify signature
        if !verify_policy(&content, &signature, key) {
            return Err(SecurityError::SignatureVerification(
                "Policy signature verification failed".to_string(),
            ));
        }

        // Load the policy
        let content_str = String::from_utf8(content).map_err(|e| {
            SecurityError::PolicyViolation(format!("Policy file is not valid UTF-8: {}", e))
        })?;

        Self::from_toml(&content_str)
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
    /// This method does NOT consider `ActionStep.confirmation_required`.
    /// Use `evaluate_action_with_step` if you need that behavior.
    pub fn evaluate_action(&self, action: &ActionStep) -> PolicyDecision {
        self.evaluate_action_internal(action, None)
    }

    /// Evaluate an action step with full integration.
    ///
    /// This method considers both the policy rules AND the ActionStep's
    /// `confirmation_required` flag:
    /// - If policy says Allow and `confirmation_required` is true → upgrade to Confirm
    /// - If policy says Confirm or Deny → keep the stricter decision
    ///
    /// # Arguments
    /// * `action` - Action step to evaluate
    /// * `current_time` - Optional current time in "HH:MM" format (for time-based conditions).
    ///   If `None`, time-based conditions will fail to match (evaluated with empty string).
    ///
    /// # Returns
    /// PolicyDecision for the action
    pub fn evaluate_action_with_step(
        &self,
        action: &ActionStep,
        current_time: Option<&str>,
    ) -> PolicyDecision {
        self.evaluate_action_internal(action, current_time)
    }

    /// Internal evaluation logic.
    fn evaluate_action_internal(
        &self,
        action: &ActionStep,
        current_time: Option<&str>,
    ) -> PolicyDecision {
        // Find first matching rule
        for rule in &self.config.rules {
            if matches_action(&rule.action, &action.action) {
                // Check conditions if present
                if let Some(conditions) = &rule.conditions {
                    if !evaluate_conditions(conditions, action, current_time.unwrap_or("")) {
                        continue; // Conditions don't match, try next rule
                    }
                }

                let decision = rule_to_decision(rule);

                // Apply confirmation_required upgrade
                return apply_confirmation_required(decision, action.confirmation_required);
            }
        }

        // No rule matched, use default
        let decision = default_to_decision(&self.config.default.decision);
        apply_confirmation_required(decision, action.confirmation_required)
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

/// Evaluate a list of conditions against an action step.
///
/// All conditions must match for the function to return true (AND logic).
///
/// # Arguments
/// * `conditions` - List of conditions to check
/// * `action` - Action step being evaluated
/// * `current_time` - Current time in "HH:MM" format
///
/// # Returns
/// `true` if all conditions match, `false` otherwise
fn evaluate_conditions(conditions: &[Condition], action: &ActionStep, current_time: &str) -> bool {
    conditions.iter().all(|condition| match condition {
        Condition::TimeOfDay { after, before } => check_time_of_day(current_time, after, before),
        Condition::AppTarget { app } => action.target == *app,
        Condition::ContactTarget { contact } => action
            .parameters
            .get("contact")
            .map(|c| c == contact)
            .unwrap_or(false),
    })
}

/// Check if current time falls within a time range.
///
/// Handles midnight crossing (e.g., after="22:00", before="06:00" means
/// 22:00-23:59 and 00:00-06:00).
///
/// # Arguments
/// * `current` - Current time in "HH:MM" format
/// * `after` - Start time in "HH:MM" format
/// * `before` - End time in "HH:MM" format
///
/// # Returns
/// `true` if current time is within the range, `false` if time parsing fails
/// or time is outside range
///
/// # Note
/// Invalid time formats (e.g., "25:00", "12:70") will log a warning and return false.
fn check_time_of_day(current: &str, after: &str, before: &str) -> bool {
    // Parse time strings as minutes since midnight
    let parse_time = |s: &str| -> Option<u32> {
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() != 2 {
            return None;
        }
        let hours: u32 = parts[0].parse().ok()?;
        let minutes: u32 = parts[1].parse().ok()?;
        // Validate time ranges
        if hours > 23 || minutes > 59 {
            return None;
        }
        Some(hours * 60 + minutes)
    };

    let current_mins = match parse_time(current) {
        Some(m) => m,
        None => {
            tracing::warn!("Invalid current time format: '{}'", current);
            return false;
        }
    };
    let after_mins = match parse_time(after) {
        Some(m) => m,
        None => {
            tracing::warn!("Invalid 'after' time format in policy: '{}'", after);
            return false;
        }
    };
    let before_mins = match parse_time(before) {
        Some(m) => m,
        None => {
            tracing::warn!("Invalid 'before' time format in policy: '{}'", before);
            return false;
        }
    };

    if after_mins <= before_mins {
        // Normal range (e.g., 09:00-17:00)
        current_mins >= after_mins && current_mins < before_mins
    } else {
        // Midnight crossing (e.g., 22:00-06:00)
        current_mins >= after_mins || current_mins < before_mins
    }
}

/// Apply confirmation_required upgrade to a policy decision.
///
/// # Logic
/// - If decision is Allow and confirmation_required is true → upgrade to Confirm
/// - If decision is already Confirm or Deny → keep it (stricter wins)
///
/// # Arguments
/// * `decision` - Original policy decision
/// * `confirmation_required` - Whether the action step requires confirmation
///
/// # Returns
/// Final policy decision
fn apply_confirmation_required(
    decision: PolicyDecision,
    confirmation_required: bool,
) -> PolicyDecision {
    if confirmation_required {
        match decision {
            PolicyDecision::Allow => PolicyDecision::Confirm {
                prompt: "This action requires confirmation".to_string(),
            },
            // Keep stricter decisions unchanged
            other => other,
        }
    } else {
        decision
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
        assert!(matches_action("launch_app", "launch_app"));
        assert!(!matches_action("launch_app", "launch_app2"));
    }

    #[test]
    fn test_matches_pattern_wildcard() {
        assert!(matches_action("delete_*", "delete_file"));
        assert!(matches_action("delete_*", "delete_contact"));
        assert!(matches_action("delete_*", "delete_"));
        assert!(!matches_action("delete_*", "send_message"));
    }

    // Condition evaluation tests

    #[test]
    fn test_time_of_day_within_range() {
        let toml = r#"
            [default]
            decision = "deny"

            [[rules]]
            action = "browse_web"
            decision = "allow"
            
            [[rules.conditions]]
            type = "time_of_day"
            after = "09:00"
            before = "17:00"
        "#;

        let engine = PolicyEngine::from_toml(toml).unwrap();
        let step = create_test_step("browse_web");

        // Test with time inside range (14:00)
        let decision = engine.evaluate_action_internal(&step, Some("14:00"));
        assert!(matches!(decision, PolicyDecision::Allow));
    }

    #[test]
    fn test_time_of_day_outside_range() {
        let toml = r#"
            [default]
            decision = "allow"

            [[rules]]
            action = "browse_web"
            decision = "deny"
            
            [[rules.conditions]]
            type = "time_of_day"
            after = "09:00"
            before = "17:00"
        "#;

        let engine = PolicyEngine::from_toml(toml).unwrap();
        let step = create_test_step("browse_web");

        // Test with time outside range (20:00)
        let decision = engine.evaluate_action_internal(&step, Some("20:00"));
        assert!(matches!(decision, PolicyDecision::Allow)); // Falls through to default
    }

    #[test]
    fn test_time_of_day_midnight_crossing_inside() {
        let toml = r#"
            [default]
            decision = "deny"

            [[rules]]
            action = "security_patrol"
            decision = "allow"
            
            [[rules.conditions]]
            type = "time_of_day"
            after = "22:00"
            before = "06:00"
        "#;

        let engine = PolicyEngine::from_toml(toml).unwrap();
        let step = create_test_step("security_patrol");

        // Test with time inside midnight-crossing range (02:00)
        let decision = engine.evaluate_action_internal(&step, Some("02:00"));
        assert!(matches!(decision, PolicyDecision::Allow));
    }

    #[test]
    fn test_time_of_day_midnight_crossing_outside() {
        let toml = r#"
            [default]
            decision = "allow"

            [[rules]]
            action = "security_patrol"
            decision = "deny"
            
            [[rules.conditions]]
            type = "time_of_day"
            after = "22:00"
            before = "06:00"
        "#;

        let engine = PolicyEngine::from_toml(toml).unwrap();
        let step = create_test_step("security_patrol");

        // Test with time outside midnight-crossing range (12:00)
        let decision = engine.evaluate_action_internal(&step, Some("12:00"));
        assert!(matches!(decision, PolicyDecision::Allow)); // Falls through to default
    }

    #[test]
    fn test_app_target_matching() {
        let toml = r#"
            [default]
            decision = "deny"

            [[rules]]
            action = "launch_app"
            decision = "allow"
            
            [[rules.conditions]]
            type = "app_target"
            app = "spotify"
        "#;

        let engine = PolicyEngine::from_toml(toml).unwrap();

        let mut step = create_test_step("launch_app");
        step.target = "spotify".to_string();

        let decision = engine.evaluate_action(&step);
        assert!(matches!(decision, PolicyDecision::Allow));
    }

    #[test]
    fn test_app_target_not_matching() {
        let toml = r#"
            [default]
            decision = "allow"

            [[rules]]
            action = "launch_app"
            decision = "deny"
            
            [[rules.conditions]]
            type = "app_target"
            app = "spotify"
        "#;

        let engine = PolicyEngine::from_toml(toml).unwrap();

        let mut step = create_test_step("launch_app");
        step.target = "chrome".to_string();

        let decision = engine.evaluate_action(&step);
        assert!(matches!(decision, PolicyDecision::Allow)); // Falls through to default
    }

    #[test]
    fn test_contact_target_matching() {
        let toml = r#"
            [default]
            decision = "deny"

            [[rules]]
            action = "send_message"
            decision = "allow"
            
            [[rules.conditions]]
            type = "contact_target"
            contact = "alice"
        "#;

        let engine = PolicyEngine::from_toml(toml).unwrap();

        let mut step = create_test_step("send_message");
        step.parameters
            .insert("contact".to_string(), "alice".to_string());

        let decision = engine.evaluate_action(&step);
        assert!(matches!(decision, PolicyDecision::Allow));
    }

    #[test]
    fn test_contact_target_missing_param() {
        let toml = r#"
            [default]
            decision = "allow"

            [[rules]]
            action = "send_message"
            decision = "deny"
            
            [[rules.conditions]]
            type = "contact_target"
            contact = "alice"
        "#;

        let engine = PolicyEngine::from_toml(toml).unwrap();

        let step = create_test_step("send_message");
        // No contact parameter

        let decision = engine.evaluate_action(&step);
        assert!(matches!(decision, PolicyDecision::Allow)); // Falls through to default
    }

    #[test]
    fn test_multiple_conditions_all_match() {
        let toml = r#"
            [default]
            decision = "deny"

            [[rules]]
            action = "send_message"
            decision = "allow"
            
            [[rules.conditions]]
            type = "time_of_day"
            after = "09:00"
            before = "17:00"
            
            [[rules.conditions]]
            type = "contact_target"
            contact = "alice"
        "#;

        let engine = PolicyEngine::from_toml(toml).unwrap();

        let mut step = create_test_step("send_message");
        step.parameters
            .insert("contact".to_string(), "alice".to_string());

        // Both conditions match
        let decision = engine.evaluate_action_internal(&step, Some("14:00"));
        assert!(matches!(decision, PolicyDecision::Allow));
    }

    #[test]
    fn test_multiple_conditions_one_fails() {
        let toml = r#"
            [default]
            decision = "allow"

            [[rules]]
            action = "send_message"
            decision = "deny"
            
            [[rules.conditions]]
            type = "time_of_day"
            after = "09:00"
            before = "17:00"
            
            [[rules.conditions]]
            type = "contact_target"
            contact = "alice"
        "#;

        let engine = PolicyEngine::from_toml(toml).unwrap();

        let mut step = create_test_step("send_message");
        step.parameters
            .insert("contact".to_string(), "bob".to_string()); // Wrong contact

        // One condition fails (contact doesn't match)
        let decision = engine.evaluate_action_internal(&step, Some("14:00"));
        assert!(matches!(decision, PolicyDecision::Allow)); // Falls through to default
    }

    // ActionStep.confirmation_required integration tests

    #[test]
    fn test_confirmation_required_upgrades_allow() {
        let toml = r#"
            [default]
            decision = "allow"

            [[rules]]
            action = "send_payment"
            decision = "allow"
        "#;

        let engine = PolicyEngine::from_toml(toml).unwrap();

        let mut step = create_test_step("send_payment");
        step.confirmation_required = true;

        let decision = engine.evaluate_action_with_step(&step, None);
        assert!(matches!(decision, PolicyDecision::Confirm { .. }));
    }

    #[test]
    fn test_confirmation_required_keeps_confirm() {
        let toml = r#"
            [default]
            decision = "allow"

            [[rules]]
            action = "send_payment"
            decision = "confirm"
            prompt = "Custom prompt"
        "#;

        let engine = PolicyEngine::from_toml(toml).unwrap();

        let mut step = create_test_step("send_payment");
        step.confirmation_required = true;

        let decision = engine.evaluate_action_with_step(&step, None);
        match decision {
            PolicyDecision::Confirm { prompt } => {
                assert_eq!(prompt, "Custom prompt");
            }
            _ => panic!("Expected Confirm decision"),
        }
    }

    #[test]
    fn test_confirmation_required_keeps_deny() {
        let toml = r#"
            [default]
            decision = "allow"

            [[rules]]
            action = "delete_system"
            decision = "deny"
            reason = "Not allowed"
        "#;

        let engine = PolicyEngine::from_toml(toml).unwrap();

        let mut step = create_test_step("delete_system");
        step.confirmation_required = true;

        let decision = engine.evaluate_action_with_step(&step, None);
        match decision {
            PolicyDecision::Deny { reason } => {
                assert_eq!(reason, "Not allowed");
            }
            _ => panic!("Expected Deny decision"),
        }
    }

    #[test]
    fn test_confirmation_required_false_no_upgrade() {
        let toml = r#"
            [default]
            decision = "allow"

            [[rules]]
            action = "browse_web"
            decision = "allow"
        "#;

        let engine = PolicyEngine::from_toml(toml).unwrap();

        let mut step = create_test_step("browse_web");
        step.confirmation_required = false;

        let decision = engine.evaluate_action_with_step(&step, None);
        assert!(matches!(decision, PolicyDecision::Allow));
    }

    // Signed file tests

    #[tokio::test]
    async fn test_from_signed_file_valid() {
        use crate::policy::signing::sign_policy;
        use std::io::Write;
        use tempfile::NamedTempFile;

        let toml = r#"
            [default]
            decision = "allow"

            [[rules]]
            action = "launch_app"
            decision = "allow"
        "#;

        let key = b"test_secret_key";
        let signature = sign_policy(toml.as_bytes(), key);

        // Create temporary files
        let mut policy_file = NamedTempFile::new().unwrap();
        policy_file.write_all(toml.as_bytes()).unwrap();

        // Create sig file with .sig extension appended
        let mut sig_path = policy_file.path().to_path_buf();
        sig_path.as_mut_os_string().push(".sig");
        std::fs::write(&sig_path, &signature).unwrap();

        // Load from signed file
        let engine = PolicyEngine::from_signed_file(policy_file.path(), key).await;
        assert!(engine.is_ok());
    }

    #[tokio::test]
    async fn test_from_signed_file_invalid_signature() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let toml = r#"
            [default]
            decision = "allow"
        "#;

        let key = b"test_secret_key";
        let wrong_signature = vec![0u8; 32];

        // Create temporary files
        let mut policy_file = NamedTempFile::new().unwrap();
        policy_file.write_all(toml.as_bytes()).unwrap();

        // Create sig file with .sig extension appended (matching implementation)
        let mut sig_path = policy_file.path().to_path_buf();
        sig_path.as_mut_os_string().push(".sig");
        std::fs::write(&sig_path, &wrong_signature).unwrap();

        // Should fail verification
        let engine = PolicyEngine::from_signed_file(policy_file.path(), key).await;
        assert!(engine.is_err());
        match engine {
            Err(SecurityError::SignatureVerification(_)) => (),
            _ => panic!("Expected SignatureVerification error"),
        }
    }

    #[tokio::test]
    async fn test_from_signed_file_missing_sig() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let toml = r#"
            [default]
            decision = "allow"
        "#;

        let key = b"test_secret_key";

        // Create temporary file (no .sig file)
        let mut policy_file = NamedTempFile::new().unwrap();
        policy_file.write_all(toml.as_bytes()).unwrap();

        // Should fail to read signature file
        let engine = PolicyEngine::from_signed_file(policy_file.path(), key).await;
        assert!(engine.is_err());
        match engine {
            Err(SecurityError::SignatureVerification(_)) => (),
            _ => panic!("Expected SignatureVerification error"),
        }
    }
}
