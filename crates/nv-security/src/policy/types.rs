//! Policy types for action evaluation.

use serde::Deserialize;

/// Decision made by the policy engine for an action.
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
#[derive(Debug, Clone, Deserialize)]
pub struct PolicyConfig {
    /// Default policy when no rules match
    pub default: DefaultPolicy,
    /// List of policy rules
    #[serde(default)]
    pub rules: Vec<PolicyRule>,
}

/// Default policy decision.
#[derive(Debug, Clone, Deserialize)]
pub struct DefaultPolicy {
    /// Default decision type: "allow", "deny", or "confirm"
    pub decision: String,
}
