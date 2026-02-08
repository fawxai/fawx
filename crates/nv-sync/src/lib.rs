//! Cloud sync client (outbound-only).
//!
//! Handles encrypted backups, state sync, and remote command polling.
//! All connections are outbound from the phone to the cloud.

/// Cloud sync client.
///
/// Manages encrypted state backup and remote command queue polling.
pub struct SyncClient {
    // Placeholder - will be implemented in Epic 9 (Sprint 5)
}

impl SyncClient {
    /// Create a new sync client.
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for SyncClient {
    fn default() -> Self {
        Self::new()
    }
}
