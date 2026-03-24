//! Append-only audit log with tamper detection via HMAC hash chains.

use super::types::{AuditEvent, AuditEventType};
use fx_core::error::SecurityError;
use ring::hmac;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Filter for querying audit events.
///
/// All filter fields are optional and combined with AND logic.
/// Use [`Default::default()`] to query all events.
#[derive(Debug, Default)]
pub struct AuditFilter {
    /// Filter by event type (e.g., `ActionExecuted`, `PolicyViolation`).
    ///
    /// If `None`, all event types are included.
    pub event_type: Option<AuditEventType>,

    /// Filter by actor (e.g., `"agent"`, `"user"`, `"skill:camera"`).
    ///
    /// If `None`, all actors are included.
    pub actor: Option<String>,

    /// Only events at or after this timestamp (Unix milliseconds, inclusive).
    ///
    /// If `None`, no lower bound is applied.
    pub after: Option<u64>,

    /// Only events at or before this timestamp (Unix milliseconds, inclusive).
    ///
    /// If `None`, no upper bound is applied.
    pub before: Option<u64>,

    /// Maximum number of results to return.
    ///
    /// If `None`, all matching events are returned.
    pub limit: Option<usize>,
}

/// Internal log entry with HMAC hash chain for integrity.
///
/// Each entry contains:
/// - The audit event data
/// - Hash of the previous entry (linking the chain)
/// - Hash of this entry (computed from event data + prev_hash)
///
/// The hash chain starts with `prev_hash = "GENESIS"` for the first entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AuditEntry {
    /// The actual audit event with all data fields.
    event: AuditEvent,

    /// HMAC-SHA256 hash of the previous entry (hex-encoded).
    ///
    /// For the first entry, this is `"GENESIS"`.
    prev_hash: String,

    /// HMAC-SHA256 hash of this entry (hex-encoded).
    ///
    /// Computed as `HMAC(key, event_data || prev_hash)`.
    hash: String,
}

impl AuditEntry {
    /// Compute HMAC for this entry, including all fields for tamper detection.
    ///
    /// Uses HMAC-SHA256 with a secret key so an attacker cannot recompute
    /// the hash chain without possessing the key.
    fn compute_hash(
        key: &hmac::Key,
        event: &AuditEvent,
        prev_hash: &str,
    ) -> Result<String, SecurityError> {
        let event_type_json = serde_json::to_string(&event.event_type).map_err(|e| {
            SecurityError::AuditLog(format!(
                "Internal error: failed to serialize event type for HMAC computation: {}",
                e
            ))
        })?;
        let metadata_json = serde_json::to_string(&event.metadata).map_err(|e| {
            SecurityError::AuditLog(format!(
                "Internal error: failed to serialize metadata for HMAC computation: {}",
                e
            ))
        })?;

        let data = format!(
            "{}:{}:{}:{}:{}:{}:{}",
            event.id,
            event.timestamp,
            event_type_json,
            event.actor,
            event.description,
            metadata_json,
            prev_hash
        );

        let tag = hmac::sign(key, data.as_bytes());
        Ok(hex::encode(tag.as_ref()))
    }

    /// Create a new entry with HMAC hash chain
    fn new(key: &hmac::Key, event: AuditEvent, prev_hash: String) -> Result<Self, SecurityError> {
        let hash = Self::compute_hash(key, &event, &prev_hash)?;
        Ok(Self {
            event,
            prev_hash,
            hash,
        })
    }

    /// Verify this entry's HMAC hash is correct
    fn verify(&self, key: &hmac::Key) -> Result<bool, SecurityError> {
        let expected_hash = Self::compute_hash(key, &self.event, &self.prev_hash)?;
        Ok(self.hash == expected_hash)
    }
}

/// Append-only audit log with tamper detection via HMAC hash chains
pub struct AuditLog {
    /// Path to the log file (None for in-memory)
    path: Option<PathBuf>,

    /// HMAC signing key
    key: hmac::Key,

    /// Cached entries (for in-memory mode and verification)
    entries: Vec<AuditEntry>,

    /// Last hash in the chain
    last_hash: String,
}

/// Maximum size of a single audit log entry (1 MB).
///
/// This limit protects against memory exhaustion and extremely large entries.
/// Entries exceeding this size will be rejected during log loading.
const MAX_ENTRY_SIZE: usize = 1_048_576;

fn event_matches_filter(event: &AuditEvent, filter: &AuditFilter) -> bool {
    if let Some(ref event_type) = filter.event_type {
        if &event.event_type != event_type {
            return false;
        }
    }

    if let Some(ref actor) = filter.actor {
        if &event.actor != actor {
            return false;
        }
    }

    if let Some(after) = filter.after {
        if event.timestamp < after {
            return false;
        }
    }

    if let Some(before) = filter.before {
        if event.timestamp > before {
            return false;
        }
    }

    true
}

impl AuditLog {
    /// Generate or load an HMAC key for the audit log.
    ///
    /// Each audit log has its own key file derived from the log filename.
    /// For example, `audit.log` uses `audit.key`, `audit-2024.log` uses `audit-2024.key`.
    /// This ensures cryptographic independence between different log files in the same directory.
    ///
    /// If the key file doesn't exist, a new 256-bit key is generated and saved.
    async fn load_or_create_key(log_path: &Path) -> Result<hmac::Key, SecurityError> {
        // Derive key filename from log filename
        let key_filename = format!(
            "{}.key",
            log_path
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| SecurityError::AuditLog("Invalid log path".to_string()))?
        );

        let key_path = log_path
            .parent()
            .unwrap_or(Path::new("."))
            .join(key_filename);

        if key_path.exists() {
            let key_bytes = fs::read(&key_path)
                .await
                .map_err(|e| SecurityError::AuditLog(format!("Failed to read key file: {}", e)))?;

            if key_bytes.len() != 32 {
                return Err(SecurityError::AuditLog(format!(
                    "Invalid key file length: expected 32 bytes, got {}",
                    key_bytes.len()
                )));
            }

            Ok(hmac::Key::new(hmac::HMAC_SHA256, &key_bytes))
        } else {
            // Generate a new 256-bit key
            let key_bytes: [u8; 32] = ring::rand::generate(&ring::rand::SystemRandom::new())
                .map_err(|_| SecurityError::AuditLog("Failed to generate HMAC key".to_string()))?
                .expose();

            // Create parent directory if needed
            if let Some(parent) = key_path.parent() {
                fs::create_dir_all(parent).await.map_err(|e| {
                    SecurityError::AuditLog(format!("Failed to create key directory: {}", e))
                })?;
            }

            fs::write(&key_path, &key_bytes)
                .await
                .map_err(|e| SecurityError::AuditLog(format!("Failed to write key file: {}", e)))?;

            // Set restrictive permissions (owner read/write only)
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let perms = std::fs::Permissions::from_mode(0o600);
                fs::set_permissions(&key_path, perms).await.map_err(|e| {
                    SecurityError::AuditLog(format!("Failed to set key permissions: {}", e))
                })?;
            }

            Ok(hmac::Key::new(hmac::HMAC_SHA256, &key_bytes))
        }
    }

    /// Open or create an audit log file.
    ///
    /// If the log file doesn't exist, it will be created. If it exists, all entries
    /// are loaded into memory for querying and integrity verification.
    ///
    /// The HMAC key is automatically loaded from `audit.key` in the same directory.
    /// If the key doesn't exist, a new 256-bit key is generated and saved with
    /// restrictive permissions (0600 on Unix).
    ///
    /// # Limits
    ///
    /// - Maximum entry size: 1 MB (see [`MAX_ENTRY_SIZE`])
    /// - Empty lines in the log file are silently skipped
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The log file exists but contains invalid JSON
    /// - An entry exceeds the maximum size limit
    /// - The key file cannot be read or created
    /// - File I/O fails
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use fx_security::audit::AuditLog;
    /// use std::path::Path;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     // Open or create audit log
    ///     let log = AuditLog::open(Path::new("/var/log/fawx/audit.log")).await?;
    ///
    ///     // Check if there are existing entries
    ///     println!("Audit log has {} entries", log.count());
    ///
    ///     // Verify integrity of existing entries
    ///     if log.verify_integrity()? {
    ///         println!("Audit log integrity verified ✓");
    ///     }
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn open(path: &Path) -> Result<Self, SecurityError> {
        let key = Self::load_or_create_key(path).await?;
        let (entries, last_hash) = if path.exists() {
            Self::read_entries(path).await?
        } else {
            (Vec::new(), "GENESIS".to_string())
        };

        Ok(Self {
            path: Some(path.to_path_buf()),
            key,
            entries,
            last_hash,
        })
    }

    async fn read_entries(path: &Path) -> Result<(Vec<AuditEntry>, String), SecurityError> {
        let file = fs::File::open(path)
            .await
            .map_err(|e| SecurityError::AuditLog(format!("Failed to open log: {}", e)))?;

        let mut lines = BufReader::new(file).lines();
        let mut entries = Vec::new();
        let mut last_hash = "GENESIS".to_string();

        while let Some(line) = lines
            .next_line()
            .await
            .map_err(|e| SecurityError::AuditLog(format!("Failed to read line: {}", e)))?
        {
            if let Some(entry) = Self::parse_entry_line(&line)? {
                last_hash = entry.hash.clone();
                entries.push(entry);
            }
        }

        Ok((entries, last_hash))
    }

    fn parse_entry_line(line: &str) -> Result<Option<AuditEntry>, SecurityError> {
        if line.trim().is_empty() {
            return Ok(None);
        }

        if line.len() > MAX_ENTRY_SIZE {
            return Err(SecurityError::AuditLog(format!(
                "Audit log entry exceeds maximum size ({} bytes)",
                line.len()
            )));
        }

        let entry: AuditEntry = serde_json::from_str(line)
            .map_err(|e| SecurityError::AuditLog(format!("Failed to parse entry: {}", e)))?;
        Ok(Some(entry))
    }

    /// Create an in-memory audit log (for testing).
    ///
    /// Generates a fresh random HMAC key for each instance to ensure test isolation
    /// and prevent key reuse across test runs.
    ///
    /// # Errors
    ///
    /// Returns `SecurityError::AuditLog` if random key generation fails.
    pub fn in_memory() -> Result<Self, SecurityError> {
        // Generate a fresh random key for each in-memory instance
        let key_bytes: [u8; 32] = ring::rand::generate(&ring::rand::SystemRandom::new())
            .map_err(|_| {
                SecurityError::AuditLog(
                    "Failed to generate random key for in-memory log".to_string(),
                )
            })?
            .expose();

        let key = hmac::Key::new(hmac::HMAC_SHA256, &key_bytes);
        Ok(Self {
            path: None,
            key,
            entries: Vec::new(),
            last_hash: "GENESIS".to_string(),
        })
    }

    /// Create an in-memory audit log with a specific HMAC key (for testing)
    #[cfg(test)]
    pub fn in_memory_with_key(key_bytes: &[u8]) -> Self {
        let key = hmac::Key::new(hmac::HMAC_SHA256, key_bytes);
        Self {
            path: None,
            key,
            entries: Vec::new(),
            last_hash: "GENESIS".to_string(),
        }
    }

    /// Append an event to the log.
    ///
    /// The event is:
    /// 1. Linked to the previous entry via HMAC hash chain
    /// 2. Written to disk immediately (if not in-memory mode)
    /// 3. Added to the in-memory cache for querying
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - HMAC computation fails
    /// - JSON serialization fails
    /// - File I/O fails (disk full, permissions, etc.)
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use fx_security::audit::{AuditLog, AuditEvent, AuditEventType};
    /// use std::path::Path;
    /// use std::collections::BTreeMap;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let mut log = AuditLog::open(Path::new("audit.log")).await?;
    ///
    ///     // Simple event
    ///     let event = AuditEvent::new(
    ///         AuditEventType::ActionExecuted,
    ///         "agent",
    ///         "User sent SMS message"
    ///     )?;
    ///     log.append(event).await?;
    ///
    ///     // Event with metadata
    ///     let mut metadata = BTreeMap::new();
    ///     metadata.insert("recipient".to_string(), "+1234567890".to_string());
    ///     metadata.insert("skill".to_string(), "messages".to_string());
    ///
    ///     let event_with_meta = AuditEvent::with_metadata(
    ///         AuditEventType::SkillInvoked,
    ///         "skill:messages",
    ///         "SMS sent successfully",
    ///         metadata
    ///     )?;
    ///     log.append(event_with_meta).await?;
    ///
    ///     println!("Total events: {}", log.count());
    ///     Ok(())
    /// }
    /// ```
    pub async fn append(&mut self, event: AuditEvent) -> Result<(), SecurityError> {
        let entry = AuditEntry::new(&self.key, event, self.last_hash.clone())?;

        // Write to file if not in-memory
        if let Some(ref path) = self.path {
            let line = serde_json::to_string(&entry).map_err(|e| {
                SecurityError::AuditLog(format!("Failed to serialize entry: {}", e))
            })?;

            let mut file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .await
                .map_err(|e| SecurityError::AuditLog(format!("Failed to open log: {}", e)))?;

            file.write_all(format!("{}\n", line).as_bytes())
                .await
                .map_err(|e| SecurityError::AuditLog(format!("Failed to write entry: {}", e)))?;

            file.flush()
                .await
                .map_err(|e| SecurityError::AuditLog(format!("Failed to flush entry: {}", e)))?;
        }

        self.last_hash = entry.hash.clone();
        self.entries.push(entry);

        Ok(())
    }

    /// Query events matching the filter
    pub fn query(&self, filter: &AuditFilter) -> Result<Vec<AuditEvent>, SecurityError> {
        let mut results: Vec<AuditEvent> = self
            .entries
            .iter()
            .filter(|entry| event_matches_filter(&entry.event, filter))
            .map(|entry| entry.event.clone())
            .collect();

        if let Some(limit) = filter.limit {
            results.truncate(limit);
        }

        Ok(results)
    }

    /// Get total number of events
    pub fn count(&self) -> usize {
        self.entries.len()
    }

    /// Verify the integrity of the entire log (HMAC hash chain)
    pub fn verify_integrity(&self) -> Result<bool, SecurityError> {
        let mut prev_hash = "GENESIS".to_string();

        for entry in &self.entries {
            // Check if prev_hash matches
            if entry.prev_hash != prev_hash {
                return Ok(false);
            }

            // Verify the entry's HMAC hash
            if !entry.verify(&self.key)? {
                return Ok(false);
            }

            prev_hash = entry.hash.clone();
        }

        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_empty_log() {
        let log = AuditLog::in_memory().unwrap();
        assert_eq!(log.count(), 0);
        assert!(log.verify_integrity().unwrap());
    }

    #[tokio::test]
    async fn test_append_and_count() {
        let mut log = AuditLog::in_memory().unwrap();
        let event =
            AuditEvent::new(AuditEventType::ActionExecuted, "agent", "Test action").unwrap();

        log.append(event).await.unwrap();
        assert_eq!(log.count(), 1);
    }

    #[tokio::test]
    async fn test_hash_chain_integrity() {
        let mut log = AuditLog::in_memory().unwrap();

        for i in 0..5 {
            let event = AuditEvent::new(
                AuditEventType::ActionExecuted,
                "agent",
                format!("Action {}", i),
            )
            .unwrap();

            log.append(event).await.unwrap();
        }

        assert!(log.verify_integrity().unwrap());
    }

    #[tokio::test]
    async fn test_tamper_detection() {
        let mut log = AuditLog::in_memory().unwrap();

        let event1 = AuditEvent::new(AuditEventType::ActionExecuted, "agent", "Original").unwrap();
        let event2 = AuditEvent::new(AuditEventType::ActionExecuted, "agent", "Second").unwrap();

        log.append(event1).await.unwrap();
        log.append(event2).await.unwrap();

        // Tamper with the first entry
        log.entries[0].event.description = "Tampered".to_string();

        assert!(!log.verify_integrity().unwrap());
    }

    #[tokio::test]
    async fn test_query_by_event_type() {
        let mut log = AuditLog::in_memory().unwrap();

        let event1 = AuditEvent::new(AuditEventType::ActionExecuted, "agent", "Action").unwrap();
        let event2 =
            AuditEvent::new(AuditEventType::PolicyViolation, "agent", "Violation").unwrap();

        log.append(event1).await.unwrap();
        log.append(event2).await.unwrap();

        let filter = AuditFilter {
            event_type: Some(AuditEventType::PolicyViolation),
            ..Default::default()
        };

        let results = log.query(&filter).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].description, "Violation");
    }

    #[tokio::test]
    async fn test_query_by_actor() {
        let mut log = AuditLog::in_memory().unwrap();

        let event1 = AuditEvent::new(AuditEventType::ActionExecuted, "agent", "A").unwrap();
        let event2 = AuditEvent::new(AuditEventType::ActionExecuted, "user", "B").unwrap();

        log.append(event1).await.unwrap();
        log.append(event2).await.unwrap();

        let filter = AuditFilter {
            actor: Some("user".to_string()),
            ..Default::default()
        };

        let results = log.query(&filter).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].description, "B");
    }

    #[tokio::test]
    async fn test_query_by_time_range() {
        let mut log = AuditLog::in_memory().unwrap();

        // Create events with known timestamps
        let mut event1 = AuditEvent::new(AuditEventType::ActionExecuted, "agent", "Early").unwrap();
        event1.timestamp = 1000;

        let mut event2 = AuditEvent::new(AuditEventType::ActionExecuted, "agent", "Match").unwrap();
        event2.timestamp = 1500;

        let mut event3 = AuditEvent::new(AuditEventType::ActionExecuted, "agent", "Late").unwrap();
        event3.timestamp = 2000;

        log.append(event1).await.unwrap();
        log.append(event2).await.unwrap();
        log.append(event3).await.unwrap();

        let filter = AuditFilter {
            after: Some(1200),
            before: Some(1800),
            ..Default::default()
        };

        let results = log.query(&filter).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].description, "Match");
    }

    #[tokio::test]
    async fn test_query_with_limit() {
        let mut log = AuditLog::in_memory().unwrap();

        for i in 0..10 {
            let event = AuditEvent::new(
                AuditEventType::ActionExecuted,
                "agent",
                format!("Event {}", i),
            )
            .unwrap();

            log.append(event).await.unwrap();
        }

        let filter = AuditFilter {
            limit: Some(3),
            ..Default::default()
        };

        let results = log.query(&filter).unwrap();
        assert_eq!(results.len(), 3);
    }

    #[tokio::test]
    async fn test_query_combined_filters() {
        let mut log = AuditLog::in_memory().unwrap();

        let event1 =
            AuditEvent::new(AuditEventType::ActionExecuted, "agent", "No match type").unwrap();
        let event2 =
            AuditEvent::new(AuditEventType::PolicyViolation, "other", "No match actor").unwrap();
        let event3 = AuditEvent::new(AuditEventType::PolicyViolation, "agent", "Match").unwrap();

        log.append(event1).await.unwrap();
        log.append(event2).await.unwrap();
        log.append(event3).await.unwrap();

        let filter = AuditFilter {
            event_type: Some(AuditEventType::PolicyViolation),
            actor: Some("agent".to_string()),
            ..Default::default()
        };

        let results = log.query(&filter).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].description, "Match");
    }

    #[tokio::test]
    async fn test_query_no_matches() {
        let mut log = AuditLog::in_memory().unwrap();

        let event = AuditEvent::new(AuditEventType::ActionExecuted, "agent", "Test").unwrap();
        log.append(event).await.unwrap();

        let filter = AuditFilter {
            event_type: Some(AuditEventType::PolicyViolation),
            ..Default::default()
        };

        let results = log.query(&filter).unwrap();
        assert_eq!(results.len(), 0);
    }

    #[tokio::test]
    async fn test_query_all_event_types() {
        let mut log = AuditLog::in_memory().unwrap();

        let types: Vec<AuditEventType> = vec![
            AuditEventType::ActionExecuted,
            AuditEventType::ActionDenied,
            AuditEventType::PolicyViolation,
            AuditEventType::SystemStartup,
            AuditEventType::ConfigChanged,
        ];

        for event_type in &types {
            let event =
                AuditEvent::new(event_type.clone(), "agent", format!("{:?}", event_type)).unwrap();
            log.append(event).await.unwrap();
        }

        for event_type in &types {
            let filter = AuditFilter {
                event_type: Some(event_type.clone()),
                ..Default::default()
            };

            let results = log.query(&filter).unwrap();
            assert_eq!(results.len(), 1);
        }
    }

    #[tokio::test]
    async fn test_combined_filter_with_time_range() {
        let mut log = AuditLog::in_memory().unwrap();

        let mut event1 = AuditEvent::new(AuditEventType::ActionExecuted, "agent", "Early").unwrap();
        event1.timestamp = 500;

        let mut event2 = AuditEvent::new(AuditEventType::ActionExecuted, "agent", "Match").unwrap();
        event2.timestamp = 1000;

        let mut event3 =
            AuditEvent::new(AuditEventType::PolicyViolation, "agent", "Wrong type").unwrap();
        event3.timestamp = 1000;

        log.append(event1).await.unwrap();
        log.append(event2).await.unwrap();
        log.append(event3).await.unwrap();

        let filter = AuditFilter {
            event_type: Some(AuditEventType::ActionExecuted),
            actor: Some("agent".to_string()),
            after: Some(800),
            before: Some(1500),
            ..Default::default()
        };

        let results = log.query(&filter).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].description, "Match");
    }

    #[tokio::test]
    async fn test_file_persistence() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("audit.log");

        // Create log and write events
        {
            let mut log = AuditLog::open(&log_path).await.unwrap();
            log.append(AuditEvent::new(AuditEventType::ActionExecuted, "agent", "Test 1").unwrap())
                .await
                .unwrap();

            log.append(AuditEvent::new(AuditEventType::ActionExecuted, "agent", "Test 2").unwrap())
                .await
                .unwrap();
        }

        // Reopen and verify
        {
            let log = AuditLog::open(&log_path).await.unwrap();
            assert_eq!(log.count(), 2);
            assert!(log.verify_integrity().unwrap());
        }
    }

    #[tokio::test]
    async fn test_open_with_malformed_json() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("corrupt.log");

        // Write key file first so open doesn't fail on missing key
        let key_bytes: [u8; 32] = [0u8; 32];
        fs::write(dir.path().join("corrupt.key"), &key_bytes)
            .await
            .unwrap();

        fs::write(&log_path, "not valid json\n").await.unwrap();

        let result = AuditLog::open(&log_path).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_metadata_hash_determinism() {
        use std::collections::BTreeMap;

        let dir = tempdir().unwrap();
        let log_path = dir.path().join("metadata.log");

        // Create event with multiple metadata keys
        let mut metadata = BTreeMap::new();
        metadata.insert("app".to_string(), "messages".to_string());
        metadata.insert("recipient".to_string(), "+1234567890".to_string());
        metadata.insert("action".to_string(), "send_sms".to_string());

        let event = AuditEvent::with_metadata(
            AuditEventType::ActionExecuted,
            "agent",
            "Sent SMS",
            metadata,
        )
        .unwrap();

        // Write event
        {
            let mut log = AuditLog::open(&log_path).await.unwrap();
            log.append(event).await.unwrap();
        }

        // Reopen and verify — BTreeMap ensures deterministic ordering
        {
            let log = AuditLog::open(&log_path).await.unwrap();
            assert_eq!(log.count(), 1);
            assert!(
                log.verify_integrity().unwrap(),
                "Hash verification must succeed with metadata across open/close"
            );
        }
    }

    #[tokio::test]
    async fn test_hmac_key_persistence() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("audit.log");
        let key_path = dir.path().join("audit.key");

        // First open creates key
        {
            let mut log = AuditLog::open(&log_path).await.unwrap();
            log.append(AuditEvent::new(AuditEventType::ActionExecuted, "agent", "Test").unwrap())
                .await
                .unwrap();
        }

        assert!(key_path.exists(), "Key file should be created");

        let key_bytes = fs::read(&key_path).await.unwrap();
        assert_eq!(key_bytes.len(), 32, "Key should be 256 bits");

        // Second open loads same key and can verify
        {
            let log = AuditLog::open(&log_path).await.unwrap();
            assert!(
                log.verify_integrity().unwrap(),
                "Integrity check should pass with same key"
            );
        }
    }

    #[tokio::test]
    async fn test_wrong_key_fails_verification() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("audit.log");
        let key_path = dir.path().join("audit.key");

        // Create log with one key
        {
            let mut log = AuditLog::open(&log_path).await.unwrap();
            log.append(AuditEvent::new(AuditEventType::ActionExecuted, "agent", "Test").unwrap())
                .await
                .unwrap();
        }

        // Replace key with a different one
        let wrong_key = [0xFFu8; 32];
        fs::write(&key_path, &wrong_key).await.unwrap();

        // Verification should fail with wrong key
        {
            let log = AuditLog::open(&log_path).await.unwrap();
            assert!(
                !log.verify_integrity().unwrap(),
                "Integrity check should fail with wrong key"
            );
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_key_file_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let log_path = dir.path().join("audit.log");
        let key_path = dir.path().join("audit.key");

        let _log = AuditLog::open(&log_path).await.unwrap();

        let metadata = std::fs::metadata(&key_path).unwrap();
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "Key file should be owner-only read/write");
    }

    // ========================================================================
    // Edge Case Tests (Issue #167)
    // ========================================================================

    #[tokio::test]
    async fn test_partially_written_json_truncated() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("truncated.log");

        // Write key file first
        let key_bytes: [u8; 32] = [0x42u8; 32];
        fs::write(dir.path().join("truncated.key"), &key_bytes)
            .await
            .unwrap();

        // Write valid entry, then truncated entry
        let valid_entry = r#"{"event":{"id":"123","timestamp":1000,"event_type":"ActionExecuted","actor":"agent","description":"Test","metadata":{}},"prev_hash":"GENESIS","hash":"abc123"}"#;
        let truncated = r#"{"event":{"id":"456","timestamp":2000,"event_type"#; // incomplete JSON

        fs::write(&log_path, format!("{}\n{}", valid_entry, truncated))
            .await
            .unwrap();

        let result = AuditLog::open(&log_path).await;
        assert!(
            result.is_err(),
            "Opening log with truncated JSON should fail"
        );

        if let Err(e) = result {
            assert!(
                e.to_string().contains("Failed to parse entry"),
                "Error should mention parse failure"
            );
        }
    }

    #[tokio::test]
    async fn test_corrupted_hash_field() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("corrupted_hash.log");

        // Create log with valid entries
        {
            let mut log = AuditLog::open(&log_path).await.unwrap();
            log.append(
                AuditEvent::new(AuditEventType::ActionExecuted, "agent", "Event 1").unwrap(),
            )
            .await
            .unwrap();
            log.append(
                AuditEvent::new(AuditEventType::ActionExecuted, "agent", "Event 2").unwrap(),
            )
            .await
            .unwrap();
        }

        // Corrupt the hash field in the first entry
        let content = fs::read_to_string(&log_path).await.unwrap();
        let lines: Vec<&str> = content.lines().collect();
        let mut first_entry: AuditEntry = serde_json::from_str(lines[0]).unwrap();

        // Modify the hash to simulate tampering
        first_entry.hash = "CORRUPTED_HASH_VALUE".to_string();

        let corrupted_line = serde_json::to_string(&first_entry).unwrap();
        let corrupted_content = format!("{}\n{}", corrupted_line, lines[1]);
        fs::write(&log_path, corrupted_content).await.unwrap();

        // Reopen and verify — integrity should fail
        let log = AuditLog::open(&log_path).await.unwrap();
        assert_eq!(log.count(), 2, "Both entries should be loaded");
        assert!(
            !log.verify_integrity().unwrap(),
            "Integrity check should fail with corrupted hash"
        );
    }

    #[tokio::test]
    async fn test_broken_hash_chain_link() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("broken_chain.log");

        // Create log with valid entries
        {
            let mut log = AuditLog::open(&log_path).await.unwrap();
            log.append(
                AuditEvent::new(AuditEventType::ActionExecuted, "agent", "Event 1").unwrap(),
            )
            .await
            .unwrap();
            log.append(
                AuditEvent::new(AuditEventType::ActionExecuted, "agent", "Event 2").unwrap(),
            )
            .await
            .unwrap();
        }

        // Break the chain by modifying second entry's prev_hash
        let content = fs::read_to_string(&log_path).await.unwrap();
        let lines: Vec<&str> = content.lines().collect();
        let mut second_entry: AuditEntry = serde_json::from_str(lines[1]).unwrap();

        // Change prev_hash to something invalid
        second_entry.prev_hash = "INVALID_LINK".to_string();

        let corrupted_line = serde_json::to_string(&second_entry).unwrap();
        let corrupted_content = format!("{}\n{}", lines[0], corrupted_line);
        fs::write(&log_path, corrupted_content).await.unwrap();

        // Reopen and verify
        let log = AuditLog::open(&log_path).await.unwrap();
        assert!(
            !log.verify_integrity().unwrap(),
            "Should detect broken chain link"
        );
    }

    #[tokio::test]
    async fn test_empty_lines_in_log() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("empty_lines.log");

        // Create a valid log with 2 entries first
        {
            let mut log = AuditLog::open(&log_path).await.unwrap();
            log.append(
                AuditEvent::new(AuditEventType::ActionExecuted, "agent", "Event 1").unwrap(),
            )
            .await
            .unwrap();
            log.append(
                AuditEvent::new(AuditEventType::ActionExecuted, "agent", "Event 2").unwrap(),
            )
            .await
            .unwrap();
        }

        // Manually add empty lines to the log file
        let content = fs::read_to_string(&log_path).await.unwrap();
        let lines: Vec<&str> = content.lines().collect();
        let with_empty_lines = format!("{}\n\n\n{}\n\n", lines[0], lines[1]);
        fs::write(&log_path, with_empty_lines).await.unwrap();

        // Reopen — should gracefully skip empty lines
        let log = AuditLog::open(&log_path).await.unwrap();
        assert_eq!(
            log.count(),
            2,
            "Should load 2 entries, skipping empty lines"
        );
        assert!(
            log.verify_integrity().unwrap(),
            "Integrity should still be valid"
        );
    }

    #[tokio::test]
    async fn test_very_large_number_of_entries() {
        let mut log = AuditLog::in_memory().unwrap();

        // Append 1000+ events
        for i in 0..1500 {
            let event = AuditEvent::new(
                AuditEventType::ActionExecuted,
                "agent",
                format!("Event {}", i),
            )
            .unwrap();

            log.append(event).await.unwrap();
        }

        assert_eq!(log.count(), 1500, "Should have 1500 entries");
        assert!(
            log.verify_integrity().unwrap(),
            "Hash chain should be intact for all 1500 entries"
        );

        // Query with limit
        let filter = AuditFilter {
            limit: Some(100),
            ..Default::default()
        };
        let results = log.query(&filter).unwrap();
        assert_eq!(results.len(), 100, "Limit should be respected");
    }

    #[tokio::test]
    async fn test_concurrent_appends() {
        use std::sync::Arc;
        use tokio::sync::Mutex;

        let log = Arc::new(Mutex::new(AuditLog::in_memory().unwrap()));
        let mut handles = vec![];

        // Spawn 10 tasks, each appending 10 events
        for task_id in 0..10 {
            let log_clone = Arc::clone(&log);
            let handle = tokio::spawn(async move {
                for i in 0..10 {
                    let event = AuditEvent::new(
                        AuditEventType::ActionExecuted,
                        format!("task-{}", task_id),
                        format!("Event {} from task {}", i, task_id),
                    )
                    .unwrap();

                    let mut log_guard = log_clone.lock().await;
                    log_guard.append(event).await.unwrap();
                }
            });
            handles.push(handle);
        }

        // Wait for all tasks to complete
        for handle in handles {
            handle.await.unwrap();
        }

        let log_guard = log.lock().await;
        assert_eq!(
            log_guard.count(),
            100,
            "Should have 100 total entries (10 tasks × 10 events)"
        );
        assert!(
            log_guard.verify_integrity().unwrap(),
            "Hash chain should remain intact despite concurrent access"
        );
    }

    #[tokio::test]
    async fn test_append_propagates_write_errors() {
        // Create log in a valid directory first
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("test.log");

        let mut log = AuditLog::open(&log_path).await.unwrap();

        // Write one event successfully
        log.append(
            AuditEvent::new(AuditEventType::ActionExecuted, "agent", "Initial event").unwrap(),
        )
        .await
        .unwrap();

        // Now make the log file read-only to simulate write failure on append
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&log_path).unwrap().permissions();
            perms.set_mode(0o444); // read-only
            std::fs::set_permissions(&log_path, perms).unwrap();

            let event =
                AuditEvent::new(AuditEventType::ActionExecuted, "agent", "Should fail").unwrap();
            let result = log.append(event).await;

            // Reset permissions for cleanup
            let mut perms = std::fs::metadata(&log_path).unwrap().permissions();
            perms.set_mode(0o644);
            std::fs::set_permissions(&log_path, perms).unwrap();

            // Verify that write failure was detected
            assert!(result.is_err(), "Should fail when log file is read-only");
            if let Err(e) = result {
                let error_msg = e.to_string();
                assert!(
                    error_msg.contains("Failed to") || error_msg.contains("failed to"),
                    "Error should indicate write failure, got: {}",
                    error_msg
                );
            }
        }

        // On non-Unix platforms, this test is a no-op
        #[cfg(not(unix))]
        {
            // Test passes automatically on non-Unix platforms
        }
    }
}
