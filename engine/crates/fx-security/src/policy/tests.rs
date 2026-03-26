//! Comprehensive tests for the policy subsystem.

#[cfg(test)]
mod integration_tests {
    use crate::policy::types::{Condition, DefaultPolicy, PolicyConfig, PolicyRule};
    use crate::policy::{sign_policy, verify_policy, PolicyDecision, PolicyEngine, RateLimiter};
    use fx_core::types::{ActionPlan, ActionStep};
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
    fn test_policy_decision_partial_eq() {
        let allow1 = PolicyDecision::Allow;
        let allow2 = PolicyDecision::Allow;
        assert_eq!(allow1, allow2);

        let deny1 = PolicyDecision::Deny {
            reason: "test".to_string(),
        };
        let deny2 = PolicyDecision::Deny {
            reason: "test".to_string(),
        };
        let deny3 = PolicyDecision::Deny {
            reason: "other".to_string(),
        };
        assert_eq!(deny1, deny2);
        assert_ne!(deny1, deny3);

        let confirm1 = PolicyDecision::Confirm {
            prompt: "test?".to_string(),
        };
        let confirm2 = PolicyDecision::Confirm {
            prompt: "test?".to_string(),
        };
        assert_eq!(confirm1, confirm2);

        let rate1 = PolicyDecision::RateLimit { wait_ms: 100 };
        let rate2 = PolicyDecision::RateLimit { wait_ms: 100 };
        let rate3 = PolicyDecision::RateLimit { wait_ms: 200 };
        assert_eq!(rate1, rate2);
        assert_ne!(rate1, rate3);
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
    fn test_rate_limiter_zero_max_count() {
        let mut limiter = RateLimiter::new();
        limiter.add_limit("action".to_string(), 0, 1000);

        // With max_count=0, every check should be rate limited
        match limiter.check("action") {
            PolicyDecision::RateLimit { .. } => {}
            _ => panic!("Expected RateLimit with max_count=0"),
        }
    }

    #[tokio::test]
    async fn test_policy_engine_from_file_missing() {
        use std::path::Path;

        let result = PolicyEngine::from_file(Path::new("/nonexistent/path/policy.toml")).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_mid_string_wildcard_supported() {
        let toml = r#"[default]
decision = "allow"

[[rules]]
action = "delete_*_file"
decision = "deny"
"#;

        let engine = PolicyEngine::from_toml(toml).unwrap();

        // Mid-string wildcards are now supported!
        let step = create_step("1", "delete_temp_file", "target");
        assert!(matches!(
            engine.evaluate_action(&step),
            PolicyDecision::Deny { .. }
        ));
    }

    #[test]
    fn test_leading_wildcard_supported() {
        let toml = r#"[default]
decision = "allow"

[[rules]]
action = "*_file"
decision = "deny"
"#;

        let engine = PolicyEngine::from_toml(toml).unwrap();

        // Leading wildcards are now supported!
        let step = create_step("1", "delete_file", "target");
        assert!(matches!(
            engine.evaluate_action(&step),
            PolicyDecision::Deny { .. }
        ));
    }

    #[test]
    fn test_rate_limiter_very_large_window() {
        let mut limiter = RateLimiter::new();
        // Very large window (1 year in ms)
        limiter.add_limit("action".to_string(), 5, 31_536_000_000);

        // Should not overflow or panic
        assert!(matches!(limiter.check("action"), PolicyDecision::Allow));
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

    // PartialEq tests (Issue #136)

    #[test]
    fn test_condition_partial_eq() {
        let cond1 = Condition::TimeOfDay {
            after: "09:00".to_string(),
            before: "17:00".to_string(),
        };
        let cond2 = Condition::TimeOfDay {
            after: "09:00".to_string(),
            before: "17:00".to_string(),
        };
        let cond3 = Condition::TimeOfDay {
            after: "10:00".to_string(),
            before: "18:00".to_string(),
        };

        assert_eq!(cond1, cond2);
        assert_ne!(cond1, cond3);
    }

    #[test]
    fn test_condition_app_target_partial_eq() {
        let cond1 = Condition::AppTarget {
            app: "telegram".to_string(),
        };
        let cond2 = Condition::AppTarget {
            app: "telegram".to_string(),
        };
        let cond3 = Condition::AppTarget {
            app: "whatsapp".to_string(),
        };

        assert_eq!(cond1, cond2);
        assert_ne!(cond1, cond3);
    }

    #[test]
    fn test_condition_contact_target_partial_eq() {
        let cond1 = Condition::ContactTarget {
            contact: "owner".to_string(),
        };
        let cond2 = Condition::ContactTarget {
            contact: "owner".to_string(),
        };
        let cond3 = Condition::ContactTarget {
            contact: "alice".to_string(),
        };

        assert_eq!(cond1, cond2);
        assert_ne!(cond1, cond3);
    }

    #[test]
    fn test_policy_rule_partial_eq_basic() {
        let rule1 = PolicyRule {
            action: "send_*".to_string(),
            decision: "allow".to_string(),
            reason: None,
            prompt: None,
            conditions: None,
        };
        let rule2 = PolicyRule {
            action: "send_*".to_string(),
            decision: "allow".to_string(),
            reason: None,
            prompt: None,
            conditions: None,
        };
        let rule3 = PolicyRule {
            action: "delete_*".to_string(),
            decision: "deny".to_string(),
            reason: Some("Not allowed".to_string()),
            prompt: None,
            conditions: None,
        };

        assert_eq!(rule1, rule2);
        assert_ne!(rule1, rule3);
    }

    #[test]
    fn test_policy_rule_partial_eq_with_conditions() {
        let cond = Condition::TimeOfDay {
            after: "09:00".to_string(),
            before: "17:00".to_string(),
        };

        let rule1 = PolicyRule {
            action: "send_*".to_string(),
            decision: "allow".to_string(),
            reason: None,
            prompt: None,
            conditions: Some(vec![cond.clone()]),
        };
        let rule2 = PolicyRule {
            action: "send_*".to_string(),
            decision: "allow".to_string(),
            reason: None,
            prompt: None,
            conditions: Some(vec![cond.clone()]),
        };
        let rule3 = PolicyRule {
            action: "send_*".to_string(),
            decision: "allow".to_string(),
            reason: None,
            prompt: None,
            conditions: None,
        };

        assert_eq!(rule1, rule2);
        assert_ne!(rule1, rule3);
    }

    #[test]
    fn test_default_policy_partial_eq() {
        let default1 = DefaultPolicy {
            decision: "allow".to_string(),
        };
        let default2 = DefaultPolicy {
            decision: "allow".to_string(),
        };
        let default3 = DefaultPolicy {
            decision: "deny".to_string(),
        };

        assert_eq!(default1, default2);
        assert_ne!(default1, default3);
    }

    #[test]
    fn test_policy_config_partial_eq() {
        let rule = PolicyRule {
            action: "test".to_string(),
            decision: "allow".to_string(),
            reason: None,
            prompt: None,
            conditions: None,
        };

        let config1 = PolicyConfig {
            default: DefaultPolicy {
                decision: "deny".to_string(),
            },
            rules: vec![rule.clone()],
        };
        let config2 = PolicyConfig {
            default: DefaultPolicy {
                decision: "deny".to_string(),
            },
            rules: vec![rule.clone()],
        };
        let config3 = PolicyConfig {
            default: DefaultPolicy {
                decision: "allow".to_string(),
            },
            rules: vec![rule.clone()],
        };

        assert_eq!(config1, config2);
        assert_ne!(config1, config3);
    }

    #[test]
    fn test_policy_rule_comparison_in_test_helper() {
        // Demonstrate using == in a test helper function
        fn assert_rules_match(expected: &PolicyRule, actual: &PolicyRule) {
            assert_eq!(expected, actual, "PolicyRules should match");
        }

        let rule = PolicyRule {
            action: "launch_*".to_string(),
            decision: "confirm".to_string(),
            reason: None,
            prompt: Some("Launch this app?".to_string()),
            conditions: None,
        };

        let same_rule = PolicyRule {
            action: "launch_*".to_string(),
            decision: "confirm".to_string(),
            reason: None,
            prompt: Some("Launch this app?".to_string()),
            conditions: None,
        };

        // Use the helper function with == comparison
        assert_rules_match(&rule, &same_rule);
    }
}
