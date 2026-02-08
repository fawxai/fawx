//! Policy types for action evaluation.
//!
//! This module defines the core types used by the policy engine to evaluate
//! actions against security policies loaded from TOML configuration files.

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
#[derive(Debug, Clone, Deserialize)]
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
/// **Note:** Conditions are currently parsed from TOML but not yet evaluated
/// by the policy engine. They will be implemented in a future update.
/// For now, rules match based on action patterns only.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum Condition {
    /// Time of day constraint
    #[serde(rename = "time_of_day")]
    TimeOfDay {
        /// Start time (e.g., "09:00")
        after: String,
        /// End time (e.g., "17:00")
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
#[derive(Debug, Clone, Deserialize)]
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
#[derive(Debug, Clone, Deserialize)]
pub struct DefaultPolicy {
    /// Default decision type: "allow", "deny", or "confirm"
    pub decision: String,
}
