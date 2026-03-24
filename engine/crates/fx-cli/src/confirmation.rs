//! Interactive confirmation UI for security policy decisions.
//!
//! Provides a CLI interface for prompting users to approve or deny actions
//! based on policy decisions from the security engine.

use anyhow::{Context, Result};
use crossterm::style::{Color, Print, ResetColor, SetForegroundColor};
use crossterm::ExecutableCommand;
use fx_security::PolicyDecision;
use std::io::{self, BufRead, Write};

/// Check if a user response indicates approval.
///
/// Accepts "y", "yes", or empty string (Enter) as approval.
/// Everything else is treated as denial.
fn is_approved(response: &str) -> bool {
    let trimmed = response.trim().to_lowercase();
    trimmed.is_empty() || trimmed == "y" || trimmed == "yes"
}

/// UI for prompting user confirmation of policy decisions.
pub struct ConfirmationUi;

impl ConfirmationUi {
    /// Prompt user for a single action decision.
    ///
    /// # Arguments
    /// * `decision` - The policy decision to present
    /// * `action_description` - Human-readable description of the action
    ///
    /// # Returns
    /// * `Ok(true)` - User approved or action is allowed
    /// * `Ok(false)` - User denied or action is denied/rate-limited
    /// * `Err(_)` - I/O error during prompt
    pub fn prompt_user(decision: &PolicyDecision, action_description: &str) -> Result<bool> {
        let mut stdout = io::stdout();

        match decision {
            PolicyDecision::Allow => {
                // Auto-approve allowed actions
                stdout
                    .execute(SetForegroundColor(Color::Green))?
                    .execute(Print("✓ "))?
                    .execute(ResetColor)?;
                println!("{} - ALLOWED", action_description);
                Ok(true)
            }

            PolicyDecision::Deny { reason } => {
                // Auto-deny denied actions
                stdout
                    .execute(SetForegroundColor(Color::Red))?
                    .execute(Print("✗ "))?
                    .execute(ResetColor)?;
                println!("{} - DENIED", action_description);
                stdout
                    .execute(SetForegroundColor(Color::Red))?
                    .execute(Print("  Reason: "))?
                    .execute(ResetColor)?;
                println!("{}", reason);
                Ok(false)
            }

            PolicyDecision::Confirm { prompt } => {
                // Prompt user for confirmation
                stdout
                    .execute(SetForegroundColor(Color::Yellow))?
                    .execute(Print("? "))?
                    .execute(ResetColor)?;
                println!("{}", action_description);
                stdout
                    .execute(SetForegroundColor(Color::Yellow))?
                    .execute(Print("  "))?
                    .execute(ResetColor)?;
                println!("{}", prompt);

                print!("  Approve? [Y/n]: ");
                stdout.flush()?;

                let stdin = io::stdin();
                let mut line = String::new();
                stdin
                    .lock()
                    .read_line(&mut line)
                    .context("Failed to read user input")?;

                let approved = is_approved(&line);

                if approved {
                    stdout
                        .execute(SetForegroundColor(Color::Green))?
                        .execute(Print("  → Approved\n"))?
                        .execute(ResetColor)?;
                } else {
                    stdout
                        .execute(SetForegroundColor(Color::Red))?
                        .execute(Print("  → Denied\n"))?
                        .execute(ResetColor)?;
                }

                Ok(approved)
            }

            PolicyDecision::RateLimit { wait_ms } => {
                // Display rate limit message
                stdout
                    .execute(SetForegroundColor(Color::Red))?
                    .execute(Print("⏱ "))?
                    .execute(ResetColor)?;
                println!("{} - RATE LIMITED", action_description);
                stdout
                    .execute(SetForegroundColor(Color::Red))?
                    .execute(Print("  Wait: "))?
                    .execute(ResetColor)?;
                println!("{}ms before retry", wait_ms);
                Ok(false)
            }
        }
    }

    /// Prompt user for approval of multiple actions in a plan.
    ///
    /// # Arguments
    /// * `decisions` - List of (action description, policy decision) tuples
    ///
    /// # Returns
    /// List of (action description, approved) tuples for each action
    pub fn prompt_plan(decisions: &[(String, PolicyDecision)]) -> Result<Vec<(String, bool)>> {
        let mut stdout = io::stdout();
        let mut results = Vec::new();

        println!("\n=== Action Plan Review ===\n");

        // First pass: display all decisions
        for (i, (action, decision)) in decisions.iter().enumerate() {
            print!("{}. ", i + 1);
            match decision {
                PolicyDecision::Allow => {
                    stdout
                        .execute(SetForegroundColor(Color::Green))?
                        .execute(Print("[ALLOW] "))?
                        .execute(ResetColor)?;
                    println!("{}", action);
                }
                PolicyDecision::Deny { reason } => {
                    stdout
                        .execute(SetForegroundColor(Color::Red))?
                        .execute(Print("[DENY]  "))?
                        .execute(ResetColor)?;
                    println!("{}", action);
                    println!("   Reason: {}", reason);
                }
                PolicyDecision::Confirm { prompt } => {
                    stdout
                        .execute(SetForegroundColor(Color::Yellow))?
                        .execute(Print("[CONFIRM] "))?
                        .execute(ResetColor)?;
                    println!("{}", action);
                    println!("   {}", prompt);
                }
                PolicyDecision::RateLimit { wait_ms } => {
                    stdout
                        .execute(SetForegroundColor(Color::Red))?
                        .execute(Print("[RATE-LIMITED] "))?
                        .execute(ResetColor)?;
                    println!("{}", action);
                    println!("   Wait: {}ms", wait_ms);
                }
            }
        }

        println!();

        // Check if any confirmations needed
        let needs_confirmation = decisions
            .iter()
            .any(|(_, d)| matches!(d, PolicyDecision::Confirm { .. }));

        if !needs_confirmation {
            // Auto-process if no confirmations needed
            for (action, decision) in decisions {
                let approved = matches!(decision, PolicyDecision::Allow);
                results.push((action.clone(), approved));
            }
            return Ok(results);
        }

        // Prompt for approval
        print!("Approve all confirmations? [Y/n/individual]: ");
        stdout.flush()?;

        let stdin = io::stdin();
        let mut line = String::new();
        stdin
            .lock()
            .read_line(&mut line)
            .context("Failed to read user input")?;

        let response = line.trim().to_lowercase();

        if response == "a" || response == "y" || response == "yes" || response.is_empty() {
            // Approve all
            for (action, decision) in decisions {
                let approved = !matches!(
                    decision,
                    PolicyDecision::Deny { .. } | PolicyDecision::RateLimit { .. }
                );
                results.push((action.clone(), approved));
            }
        } else if response == "n" || response == "no" {
            // Deny all confirmations
            for (action, decision) in decisions {
                let approved = matches!(decision, PolicyDecision::Allow);
                results.push((action.clone(), approved));
            }
        } else {
            // Individual approval
            println!("\nReviewing each action individually:\n");
            for (action, decision) in decisions {
                let approved = Self::prompt_user(decision, action)?;
                results.push((action.clone(), approved));
            }
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allow_decision_format() {
        // This test validates the logic without actually prompting
        let decision = PolicyDecision::Allow;
        assert!(matches!(decision, PolicyDecision::Allow));
    }

    #[test]
    fn test_is_approved_yes_variants() {
        assert!(is_approved("y"));
        assert!(is_approved("Y"));
        assert!(is_approved("yes"));
        assert!(is_approved("YES"));
        assert!(is_approved("Yes"));
        assert!(is_approved(""));
        assert!(is_approved("  "));
        assert!(is_approved("\n"));
    }

    #[test]
    fn test_is_approved_no_variants() {
        assert!(!is_approved("n"));
        assert!(!is_approved("N"));
        assert!(!is_approved("no"));
        assert!(!is_approved("NO"));
        assert!(!is_approved("No"));
        assert!(!is_approved("nope"));
        assert!(!is_approved("anything else"));
    }

    #[test]
    fn test_deny_decision_has_reason() {
        let decision = PolicyDecision::Deny {
            reason: "Security violation".to_string(),
        };
        match decision {
            PolicyDecision::Deny { reason } => {
                assert_eq!(reason, "Security violation");
            }
            _ => panic!("Expected Deny decision"),
        }
    }

    #[test]
    fn test_confirm_decision_has_prompt() {
        let decision = PolicyDecision::Confirm {
            prompt: "This action requires approval".to_string(),
        };
        match decision {
            PolicyDecision::Confirm { prompt } => {
                assert_eq!(prompt, "This action requires approval");
            }
            _ => panic!("Expected Confirm decision"),
        }
    }

    #[test]
    fn test_rate_limit_decision_has_wait_time() {
        let decision = PolicyDecision::RateLimit { wait_ms: 5000 };
        match decision {
            PolicyDecision::RateLimit { wait_ms } => {
                assert_eq!(wait_ms, 5000);
            }
            _ => panic!("Expected RateLimit decision"),
        }
    }

    #[test]
    fn test_plan_with_mixed_decisions() {
        let decisions = &[
            (
                "Send message".to_string(),
                PolicyDecision::Confirm {
                    prompt: "Send to contact?".to_string(),
                },
            ),
            ("Read contacts".to_string(), PolicyDecision::Allow),
            (
                "Delete all data".to_string(),
                PolicyDecision::Deny {
                    reason: "Too dangerous".to_string(),
                },
            ),
        ];

        // Validate structure
        assert_eq!(decisions.len(), 3);
        assert!(matches!(decisions[0].1, PolicyDecision::Confirm { .. }));
        assert!(matches!(decisions[1].1, PolicyDecision::Allow));
        assert!(matches!(decisions[2].1, PolicyDecision::Deny { .. }));
    }

    #[test]
    fn test_plan_no_confirmations_needed() {
        let decisions = &[
            ("Read contacts".to_string(), PolicyDecision::Allow),
            (
                "Delete data".to_string(),
                PolicyDecision::Deny {
                    reason: "Not allowed".to_string(),
                },
            ),
        ];

        let needs_confirmation = decisions
            .iter()
            .any(|(_, d)| matches!(d, PolicyDecision::Confirm { .. }));

        assert!(!needs_confirmation);
    }

    #[test]
    fn test_plan_has_confirmations() {
        let decisions = &[
            ("Read contacts".to_string(), PolicyDecision::Allow),
            (
                "Send message".to_string(),
                PolicyDecision::Confirm {
                    prompt: "Confirm?".to_string(),
                },
            ),
        ];

        let needs_confirmation = decisions
            .iter()
            .any(|(_, d)| matches!(d, PolicyDecision::Confirm { .. }));

        assert!(needs_confirmation);
    }

    #[test]
    fn test_decision_display_formatting() {
        // Test that we can construct all decision types correctly
        let allow = PolicyDecision::Allow;
        let deny = PolicyDecision::Deny {
            reason: "Test reason".to_string(),
        };
        let confirm = PolicyDecision::Confirm {
            prompt: "Test prompt".to_string(),
        };
        let rate_limit = PolicyDecision::RateLimit { wait_ms: 1000 };

        // Verify they all exist and match expected patterns
        assert!(matches!(allow, PolicyDecision::Allow));
        assert!(matches!(deny, PolicyDecision::Deny { .. }));
        assert!(matches!(confirm, PolicyDecision::Confirm { .. }));
        assert!(matches!(rate_limit, PolicyDecision::RateLimit { .. }));
    }
}
