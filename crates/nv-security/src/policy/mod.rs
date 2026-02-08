//! Policy subsystem for action evaluation and security.
//!
//! This module provides policy-based security controls for action execution.
//! Policies are loaded from TOML files and can enforce allow/deny/confirm
//! decisions for actions, with support for wildcard matching, rate limiting,
//! and HMAC-based policy file signing.

pub mod engine;
pub mod rate_limit;
pub mod signing;
pub mod types;

#[cfg(test)]
mod tests;

// Re-export main types for convenience
pub use engine::PolicyEngine;
pub use rate_limit::RateLimiter;
pub use signing::{sign_policy, verify_policy};
pub use types::{Condition, DefaultPolicy, PolicyConfig, PolicyDecision, PolicyRule};
