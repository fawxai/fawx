//! Security subsystem: capabilities, crypto, policy, audit.
//!
//! Implements the security boundary between the agent's plans and
//! their execution on the device.

use nv_core::types::ActionPlan;

/// Action policy engine.
///
/// Evaluates action plans against security policies and determines
/// whether they should be allowed, require confirmation, or be denied.
pub struct PolicyEngine {
    // Placeholder - will be implemented in Epic 5
}

impl PolicyEngine {
    /// Create a new policy engine.
    pub fn new() -> Self {
        Self {}
    }

    /// Evaluate an action plan against the policy.
    pub fn evaluate(&self, _plan: &ActionPlan) -> PolicyDecision {
        // Placeholder implementation
        PolicyDecision::Allow
    }
}

impl Default for PolicyEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Policy decision for an action plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    /// Action is allowed without confirmation
    Allow,
    /// Action requires user confirmation
    Confirm(String),
    /// Action is denied
    Deny(String),
}
