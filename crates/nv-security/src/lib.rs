//! Security subsystem: capabilities, crypto, policy, audit.
//!
//! Implements the security boundary between the agent's plans and
//! their execution on the device.

pub mod policy;

// Re-export main policy types
pub use policy::{PolicyDecision, PolicyEngine};
