//! Security subsystem: capabilities, crypto, policy, audit.
//!
//! Implements the security boundary between the agent's plans and
//! their execution on the device.

pub mod policy;

// Re-export main policy types for convenience
pub use policy::{sign_policy, verify_policy, PolicyDecision, PolicyEngine, RateLimiter};
