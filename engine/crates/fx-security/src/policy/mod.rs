//! Policy subsystem for action evaluation and security.
//!
//! This module provides policy-based security controls for action execution.
//! Policies are loaded from TOML files and can enforce allow/deny/confirm
//! decisions for actions, with support for wildcard matching, rate limiting,
//! and HMAC-based policy file signing.
//!
//! # Architecture
//!
//! The policy subsystem has two main components that work independently:
//!
//! 1. **PolicyEngine**: Evaluates actions against TOML-defined rules
//!    - Returns: `Allow`, `Deny`, or `Confirm` decisions
//!    - Based on action pattern matching
//!
//! 2. **RateLimiter**: Enforces rate limits per action
//!    - Returns: `Allow` or `RateLimit` decisions
//!    - Based on sliding window counters
//!
//! **Note:** `PolicyDecision::RateLimit` variant is only returned by
//! `RateLimiter`, not by `PolicyEngine`. Applications must integrate both
//! components manually if rate limiting is needed alongside policy evaluation.

pub mod engine;
pub mod rate_limit;
pub mod signing;
pub mod types;
pub mod util;

#[cfg(test)]
mod tests;

// Re-export main types for public API
pub use engine::PolicyEngine;
pub use rate_limit::RateLimiter;
pub use signing::{sign_policy, verify_policy};
pub use types::PolicyDecision;

// Note: Internal types (Condition, DefaultPolicy, PolicyConfig, PolicyRule from types::*)
// and utility functions (matches_action from util::*) are NOT re-exported, keeping the
// public API surface minimal. They remain accessible within this module via their submodules.
