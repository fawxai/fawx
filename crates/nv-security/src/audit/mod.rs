//! Audit logging for security events and compliance tracking.
//!
//! The audit system provides an **append-only log** with **tamper detection** via HMAC-based
//! hash chains. Each audit entry is cryptographically linked to the previous entry, making
//! it impossible to modify or delete past events without detection.
//!
//! # Features
//!
//! - **Append-only** — Events can only be added, never modified or deleted
//! - **Tamper detection** — HMAC-SHA256 hash chains ensure integrity
//! - **Persistent storage** — Events are written to disk immediately
//! - **Queryable** — Filter events by type, actor, time range, or custom criteria
//! - **Async I/O** — Built on Tokio for non-blocking operations
//!
//! # Security
//!
//! - Each audit log has a unique 256-bit HMAC key stored in `audit.key` (created automatically)
//! - Key file has restrictive permissions (0600 on Unix)
//! - Entries are hashed with `HMAC-SHA256(key, event_data || prev_hash)`
//! - Verification checks the entire chain from genesis to the latest entry
//!
//! # Usage
//!
//! ```rust,no_run
//! use nv_security::audit::{AuditLog, AuditEvent, AuditEventType, AuditFilter};
//! use std::path::Path;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Open or create an audit log
//!     let mut log = AuditLog::open(Path::new("audit.log")).await?;
//!
//!     // Append events
//!     let event = AuditEvent::new(
//!         AuditEventType::ActionExecuted,
//!         "agent",
//!         "User sent SMS message"
//!     )?;
//!     log.append(event).await?;
//!
//!     // Query events
//!     let filter = AuditFilter {
//!         event_type: Some(AuditEventType::ActionExecuted),
//!         after: Some(1704067200000), // Jan 1, 2024
//!         limit: Some(100),
//!         ..Default::default()
//!     };
//!     let events = log.query(&filter)?;
//!
//!     // Verify integrity
//!     assert!(log.verify_integrity()?);
//!
//!     Ok(())
//! }
//! ```
//!
//! # Event Types
//!
//! The audit system supports various event categories:
//! - **Actions** — `ActionExecuted`, `ActionDenied`, `ActionConfirmed`
//! - **Policy** — `PolicyEvaluated`, `PolicyViolation`
//! - **Skills** — `SkillInvoked`, `SkillInstalled`, `SkillRemoved`
//! - **Security** — `AuthAttempt`, `CredentialAccess`
//! - **System** — `SystemStartup`, `SystemShutdown`, `ConfigChanged`

mod log;
mod types;

pub use log::{AuditFilter, AuditLog};
pub use types::{AuditEvent, AuditEventType};
