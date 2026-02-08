//! Security subsystem: capabilities, crypto, policy, audit.
//!
//! Implements the security boundary between the agent's plans and
//! their execution on the device.

pub mod audit;
pub mod policy;

// Re-export main types for convenience
pub use audit::{AuditEvent, AuditEventType, AuditFilter, AuditLog};
pub use policy::{sign_policy, verify_policy, PolicyDecision, PolicyEngine, RateLimiter};
