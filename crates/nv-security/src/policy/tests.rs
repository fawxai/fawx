//! Comprehensive tests for the policy subsystem.

#[cfg(test)]
mod integration_tests {
    use crate::policy::*;
    use nv_core::types::{ActionPlan, ActionStep};
    use std::collections::HashMap;

    fn create_step(id: &str, action: &str, target: &str) -> ActionStep {
        ActionStep {
            id: id.to_string(),
            action: action.to_string(),
            target: target.to_string(),
            parameters: HashMap::new(),
            confirmation_required: false,
        }
    }

    #[test]
    fn test_policy_decision_debug_derive() {
        let decision = PolicyDecision::Allow;
        let debug_str = format!("{:?}", decision);
        assert!(debug_str.contains("Allow"));
    }

    #[test]
    fn test_policy_decision_clone_derive() {
        let decision = PolicyDecision::Deny {
            reason: "test".to_string(),
        };
        let cloned = decision.clone();
        assert!(matches!(cloned, PolicyDecision::Deny { .. }));
    }

    #[test]
    fn test_conditions_time_of_day() {
        let toml = r#"
            [default]
            decision = "allow"

            [[rules]]
            action = "test"
            decision = "deny"
            [[rules.conditions]]
            type = "time_of_day"
            after = "09:00"
            before = "17:00"
        "#;

        let engine = PolicyEngine::from_toml(toml);
        assert!(engine.is_ok());
    }

    #[test]
    fn test_conditions_app_target() {
        let toml = r#"
            [default]
            decision = "allow"

            [[rules]]
            action = "test"
            decision = "deny"
            [[rules.conditions]]
            type = "app_target"
            app = "Gmail"
        "#;

        let engine = PolicyEngine::from_toml(toml);
        assert!(engine.is_ok());
    }

    #[test]
    fn test_conditions_contact_target() {
        let toml = r#"
            [default]
            decision = "allow"

            [[rules]]
            action = "test"
            decision = "deny"
            [[rules.conditions]]
            type = "contact_target"
            contact = "John Doe"
        "#;

        let engine = PolicyEngine::from_toml(toml);
        assert!(engine.is_ok());
    }

    #[test]
    fn test_plan_with_requires_confirmation() {
        let toml = r#"[default]
decision = "allow"
"#;

        let engine = PolicyEngine::from_toml(toml).unwrap();

        let plan = ActionPlan {
            id: "plan1".to_string(),
            steps: vec![create_step("1", "test", "target")],
            description: "Test".to_string(),
            requires_confirmation: true,
        };

        let results = engine.evaluate_plan(&plan);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_large_policy_file() {
        let mut toml = String::from(
            r#"
            [default]
            decision = "confirm"
        "#,
        );

        // Add 50 rules
        for i in 0..50 {
            toml.push_str(&format!(
                r#"
            [[rules]]
            action = "action_{}"
            decision = "allow"
            "#,
                i
            ));
        }

        let engine = PolicyEngine::from_toml(&toml);
        assert!(engine.is_ok());

        let engine = engine.unwrap();
        let step = create_step("1", "action_25", "target");
        assert!(matches!(
            engine.evaluate_action(&step),
            PolicyDecision::Allow
        ));
    }

    #[test]
    fn test_signing_and_verification() {
        let content = b"test policy content";
        let key = b"my_secret_key";

        let signature = sign_policy(content, key);
        assert!(verify_policy(content, &signature, key));
    }

    #[test]
    fn test_rate_limiter_window_expiry() {
        let mut limiter = RateLimiter::new();
        // This test is time-based and would require mocking time
        // For now, we just ensure it doesn't panic
        limiter.add_limit("test".to_string(), 2, 100);
        limiter.check("test");
    }

    #[test]
    fn test_empty_action_name() {
        let toml = r#"
            [default]
            decision = "allow"

            [[rules]]
            action = ""
            decision = "deny"
        "#;

        let engine = PolicyEngine::from_toml(toml).unwrap();
        let step = create_step("1", "", "target");

        assert!(matches!(
            engine.evaluate_action(&step),
            PolicyDecision::Deny { .. }
        ));
    }

    #[test]
    fn test_wildcard_only() {
        let toml = r#"
            [default]
            decision = "allow"

            [[rules]]
            action = "*"
            decision = "deny"
        "#;

        let engine = PolicyEngine::from_toml(toml).unwrap();
        let step = create_step("1", "anything", "target");

        assert!(matches!(
            engine.evaluate_action(&step),
            PolicyDecision::Deny { .. }
        ));
    }

    #[test]
    fn test_confirm_without_prompt() {
        let toml = r#"
            [default]
            decision = "allow"

            [[rules]]
            action = "test"
            decision = "confirm"
        "#;

        let engine = PolicyEngine::from_toml(toml).unwrap();
        let step = create_step("1", "test", "target");

        match engine.evaluate_action(&step) {
            PolicyDecision::Confirm { prompt } => {
                assert_eq!(prompt, "Confirm this action?");
            }
            _ => panic!("Expected Confirm decision"),
        }
    }

    #[test]
    fn test_deny_without_reason() {
        let toml = r#"
            [default]
            decision = "allow"

            [[rules]]
            action = "test"
            decision = "deny"
        "#;

        let engine = PolicyEngine::from_toml(toml).unwrap();
        let step = create_step("1", "test", "target");

        match engine.evaluate_action(&step) {
            PolicyDecision::Deny { reason } => {
                assert_eq!(reason, "Denied by policy");
            }
            _ => panic!("Expected Deny decision"),
        }
    }

    #[test]
    fn test_unknown_decision_type() {
        let toml = r#"
            [default]
            decision = "allow"

            [[rules]]
            action = "test"
            decision = "unknown"
        "#;

        let engine = PolicyEngine::from_toml(toml).unwrap();
        let step = create_step("1", "test", "target");

        match engine.evaluate_action(&step) {
            PolicyDecision::Deny { reason } => {
                assert!(reason.contains("Unknown decision type"));
            }
            _ => panic!("Expected Deny decision for unknown type"),
        }
    }

    #[test]
    fn test_multiple_steps_plan() {
        let toml = r#"
            [default]
            decision = "allow"

            [[rules]]
            action = "step1"
            decision = "allow"

            [[rules]]
            action = "step2"
            decision = "confirm"

            [[rules]]
            action = "step3"
            decision = "deny"
        "#;

        let engine = PolicyEngine::from_toml(toml).unwrap();

        let plan = ActionPlan {
            id: "plan1".to_string(),
            steps: vec![
                create_step("1", "step1", "target"),
                create_step("2", "step2", "target"),
                create_step("3", "step3", "target"),
            ],
            description: "Multi-step plan".to_string(),
            requires_confirmation: false,
        };

        let results = engine.evaluate_plan(&plan);
        assert_eq!(results.len(), 3);

        assert!(matches!(results[0].1, PolicyDecision::Allow));
        assert!(matches!(results[1].1, PolicyDecision::Confirm { .. }));
        assert!(matches!(results[2].1, PolicyDecision::Deny { .. }));
    }

    #[test]
    fn test_rate_limiter_multiple_actions() {
        let mut limiter = RateLimiter::new();
        limiter.add_limit("action1".to_string(), 2, 1000);
        limiter.add_limit("action2".to_string(), 1, 1000);

        assert!(matches!(limiter.check("action1"), PolicyDecision::Allow));
        assert!(matches!(limiter.check("action2"), PolicyDecision::Allow));

        match limiter.check("action2") {
            PolicyDecision::RateLimit { .. } => {}
            _ => panic!("Expected RateLimit for action2"),
        }
    }

    #[test]
    fn test_default_unknown_decision() {
        let toml = r#"[default]
decision = "invalid"
"#;

        let engine = PolicyEngine::from_toml(toml).unwrap();
        let step = create_step("1", "test", "target");

        match engine.evaluate_action(&step) {
            PolicyDecision::Deny { reason } => {
                assert!(reason.contains("Unknown default decision"));
            }
            _ => panic!("Expected Deny for unknown default"),
        }
    }

    #[test]
    fn test_complex_toml_structure() {
        let toml = r#"
            [default]
            decision = "confirm"

            [[rules]]
            action = "send_*"
            decision = "confirm"
            prompt = "Send this?"

            [[rules]]
            action = "delete_*"
            decision = "deny"
            reason = "Deletions not allowed"

            [[rules]]
            action = "launch_*"
            decision = "allow"
        "#;

        let engine = PolicyEngine::from_toml(toml).unwrap();

        let step1 = create_step("1", "send_message", "target");
        let step2 = create_step("2", "delete_file", "target");
        let step3 = create_step("3", "launch_app", "target");

        assert!(matches!(
            engine.evaluate_action(&step1),
            PolicyDecision::Confirm { .. }
        ));
        assert!(matches!(
            engine.evaluate_action(&step2),
            PolicyDecision::Deny { .. }
        ));
        assert!(matches!(
            engine.evaluate_action(&step3),
            PolicyDecision::Allow
        ));
    }
}
