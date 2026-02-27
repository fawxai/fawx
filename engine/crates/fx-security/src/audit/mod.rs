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
//! - Each audit log has a unique 256-bit HMAC key stored alongside the log file (created automatically)
//! - Key filename is derived from log filename: `audit.log` → `audit.key`, `audit-2024.log` → `audit-2024.key`
//! - This ensures cryptographic independence between different log files in the same directory
//! - Key files have restrictive permissions (0600 on Unix)
//! - Entries are hashed with `HMAC-SHA256(key, event_data || prev_hash)`
//! - Verification checks the entire chain from genesis to the latest entry
//!
//! # Usage
//!
//! ```rust,no_run
//! use fx_security::audit::{AuditLog, AuditEvent, AuditEventType, AuditFilter};
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
//!
//! # Concurrency
//!
//! `AuditLog` is **not** `Sync` and requires external synchronization for concurrent access.
//! Use `Arc<Mutex<AuditLog>>` when multiple tasks need to append events:
//!
//! ```rust,no_run
//! use fx_security::audit::{AuditLog, AuditEvent, AuditEventType};
//! use std::path::Path;
//! use std::sync::Arc;
//! use tokio::sync::Mutex;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), fx_core::error::SecurityError> {
//!     let log = Arc::new(Mutex::new(AuditLog::open(Path::new("audit.log")).await?));
//!
//!     let log_clone = Arc::clone(&log);
//!     let handle = tokio::spawn(async move {
//!         let event = AuditEvent::new(
//!             AuditEventType::ActionExecuted,
//!             "task-1",
//!             "Concurrent event"
//!         )?;
//!         let mut guard = log_clone.lock().await;
//!         guard.append(event).await?;
//!         Ok::<(), fx_core::error::SecurityError>(())
//!     });
//!
//!     handle.await.unwrap()?;
//!     Ok(())
//! }
//! ```

mod log;
mod types;

pub use log::{AuditFilter, AuditLog};
pub use types::{AuditEvent, AuditEventType};
