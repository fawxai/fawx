//! Audit logging for security events and compliance tracking.
//!
//! Provides an append-only audit log with tamper detection via hash chains.

mod log;
mod types;

pub use log::{AuditFilter, AuditLog};
pub use types::{AuditEvent, AuditEventType};
