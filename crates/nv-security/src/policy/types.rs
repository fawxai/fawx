//! Policy types for action evaluation.
//!
//! This module defines the core types used by the policy engine to evaluate
//! actions against security policies loaded from TOML configuration files.
//!
//! ## Timezone Note
//!
//! Time-based conditions (`Condition::TimeOfDay`) are evaluated using UTC.
//! This ensures consistent behavior across different deployments and timezones.

use serde::Deserialize;

/// Decision made by the policy engine for an action.
///
/// This enum represents the security verdict for a proposed action.
/// The policy engine evaluates rules and returns one of these decisions.
#[derive(Debug, Clone, PartialEq)]
pub enum PolicyDecision {
    /// Action is allowed without confirmation
    Allow,
    /// Action is denied
    Deny { reason: String },
    /// Action requires user confirmation
    Confirm { prompt: String },
    /// Action is rate-limited
    RateLimit { wait_ms: u64 },
}

/// A policy rule defining how to handle specific actions.
///
/// Rules match action patterns (with optional glob wildcards) and specify
/// the security decision to apply. Rules are evaluated in order, with the
/// first match winning.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PolicyRule {
    /// Action pattern (supports glob wildcards like "delete_*")
    pub action: String,
    /// Decision type: "allow", "deny", "confirm"
    pub decision: String,
    /// Optional reason for deny decisions
    pub reason: Option<String>,
    /// Optional prompt for confirm decisions
    pub prompt: Option<String>,
    /// Optional conditions for the rule
    pub conditions: Option<Vec<Condition>>,
}

/// Conditions that must be met for a rule to apply.
///
/// Conditions are evaluated when `PolicyEngine::evaluate_action()` is called.
/// All conditions in a rule must match for the rule to apply (AND logic).
///
/// # Supported Conditions
///
/// - **TimeOfDay**: Matches if current time falls within the specified range.
///   Supports midnight crossing (e.g., "22:00" to "06:00").
/// - **AppTarget**: Matches if `ActionStep.target` equals the specified app name.
/// - **ContactTarget**: Matches if `ActionStep.parameters["contact"]` equals the
///   specified contact name.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type")]
pub enum Condition {
    /// Time of day constraint (24-hour format, evaluated in UTC)
    ///
    /// **Important:** Times are evaluated in UTC, not local time.
    /// For example, `after: "09:00"` means 09:00 UTC (which is 1:00 AM PST
    /// or 5:00 AM EDT). If you want to restrict actions to local business
    /// hours, you must convert your local time to UTC.
    ///
    /// Supports midnight crossing (e.g., `after: "22:00", before: "06:00"`).
    #[serde(rename = "time_of_day")]
    TimeOfDay {
        /// Start time in 24-hour format (e.g., "09:00")
        after: String,
        /// End time in 24-hour format (e.g., "17:00")
        before: String,
    },
    /// Application target constraint
    #[serde(rename = "app_target")]
    AppTarget {
        /// App name or package
        app: String,
    },
    /// Contact target constraint
    #[serde(rename = "contact_target")]
    ContactTarget {
        /// Contact name or identifier
        contact: String,
    },
}

/// Root policy configuration loaded from TOML.
///
/// This is the top-level structure deserialized from policy TOML files.
/// It contains a default decision and a list of rules to evaluate.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PolicyConfig {
    /// Default policy when no rules match
    pub default: DefaultPolicy,
    /// List of policy rules
    #[serde(default)]
    pub rules: Vec<PolicyRule>,
}

/// Default policy decision.
///
/// Specifies what decision to make when no rules match an action.
/// Common values: "allow", "deny", "confirm".
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct DefaultPolicy {
    /// Default decision type: "allow", "deny", or "confirm"
    pub decision: String,
}
