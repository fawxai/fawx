//! Append-only audit log with tamper detection via HMAC hash chains.

use super::types::{AuditEvent, AuditEventType};
use nv_core::error::SecurityError;
use ring::hmac;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Filter for querying audit events
#[derive(Debug, Default)]
pub struct AuditFilter {
    /// Filter by event type
    pub event_type: Option<AuditEventType>,

    /// Filter by actor
    pub actor: Option<String>,

    /// Only events after this timestamp (inclusive)
    pub after: Option<u64>,

    /// Only events before this timestamp (inclusive)
    pub before: Option<u64>,

    /// Maximum number of results
    pub limit: Option<usize>,
}

/// Internal log entry with HMAC hash chain for integrity
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AuditEntry {
    /// The actual event
    event: AuditEvent,

    /// HMAC hash of the previous entry (hex-encoded)
    prev_hash: String,

    /// HMAC hash of this entry (hex-encoded)
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
            SecurityError::AuditLog(format!("Failed to serialize event type: {}", e))
        })?;
        let metadata_json = serde_json::to_string(&event.metadata)
            .map_err(|e| SecurityError::AuditLog(format!("Failed to serialize metadata: {}", e)))?;

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

/// Default key file name stored alongside the audit log
const KEY_FILE_NAME: &str = "audit.key";

impl AuditLog {
    /// Generate or load an HMAC key for the audit log.
    ///
    /// The key is stored in a file next to the audit log. If the key file
    /// doesn't exist, a new 256-bit key is generated and saved.
    async fn load_or_create_key(log_path: &Path) -> Result<hmac::Key, SecurityError> {
        let key_path = log_path
            .parent()
            .unwrap_or(Path::new("."))
            .join(KEY_FILE_NAME);

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

    /// Open or create an audit log file
    pub async fn open(path: &Path) -> Result<Self, SecurityError> {
        let key = Self::load_or_create_key(path).await?;
        let mut entries = Vec::new();
        let mut last_hash = "GENESIS".to_string();

        // Read existing entries if file exists
        if path.exists() {
            let file = fs::File::open(path)
                .await
                .map_err(|e| SecurityError::AuditLog(format!("Failed to open log: {}", e)))?;

            const MAX_LINE_LENGTH: usize = 1_048_576; // 1MB per entry
            let reader = BufReader::new(file);
            let mut lines = reader.lines();

            while let Some(line) = lines
                .next_line()
                .await
                .map_err(|e| SecurityError::AuditLog(format!("Failed to read line: {}", e)))?
            {
                if line.len() > MAX_LINE_LENGTH {
                    return Err(SecurityError::AuditLog(format!(
                        "Audit log entry exceeds maximum length ({} bytes)",
                        line.len()
                    )));
                }

                let entry: AuditEntry = serde_json::from_str(&line).map_err(|e| {
                    SecurityError::AuditLog(format!("Failed to parse entry: {}", e))
                })?;

                last_hash = entry.hash.clone();
                entries.push(entry);
            }
        }

        Ok(Self {
            path: Some(path.to_path_buf()),
            key,
            entries,
            last_hash,
        })
    }

    /// Create an in-memory audit log (for testing)
    pub fn in_memory() -> Self {
        // Use a fixed test key for in-memory mode
        let key = hmac::Key::new(hmac::HMAC_SHA256, b"nova-audit-test-key-do-not-use!");
        Self {
            path: None,
            key,
            entries: Vec::new(),
            last_hash: "GENESIS".to_string(),
        }
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

    /// Append an event to the log
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
            .filter(|entry| {
                // Filter by event type
                if let Some(ref event_type) = filter.event_type {
                    if &entry.event.event_type != event_type {
                        return false;
                    }
                }

                // Filter by actor
                if let Some(ref actor) = filter.actor {
                    if &entry.event.actor != actor {
                        return false;
                    }
                }

                // Filter by time range
                if let Some(after) = filter.after {
                    if entry.event.timestamp < after {
                        return false;
                    }
                }

                if let Some(before) = filter.before {
                    if entry.event.timestamp > before {
                        return false;
                    }
                }

                true
            })
            .map(|entry| entry.event.clone())
            .collect();

        // Apply limit
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
        let log = AuditLog::in_memory();
        assert_eq!(log.count(), 0);
        assert!(log.verify_integrity().unwrap());
    }

    #[tokio::test]
    async fn test_append_and_count() {
        let mut log = AuditLog::in_memory();
        let event =
            AuditEvent::new(AuditEventType::ActionExecuted, "agent", "Test action").unwrap();

        log.append(event).await.unwrap();
        assert_eq!(log.count(), 1);
    }

    #[tokio::test]
    async fn test_hash_chain_integrity() {
        let mut log = AuditLog::in_memory();

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
        let mut log = AuditLog::in_memory();

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
        let mut log = AuditLog::in_memory();

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
        let mut log = AuditLog::in_memory();

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
        let mut log = AuditLog::in_memory();

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
        let mut log = AuditLog::in_memory();

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
        let mut log = AuditLog::in_memory();

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
        let mut log = AuditLog::in_memory();

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
        let mut log = AuditLog::in_memory();

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
        let mut log = AuditLog::in_memory();

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
        fs::write(dir.path().join(KEY_FILE_NAME), &key_bytes)
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
        let key_path = dir.path().join(KEY_FILE_NAME);

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
        let key_path = dir.path().join(KEY_FILE_NAME);

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
        let key_path = dir.path().join(KEY_FILE_NAME);

        let _log = AuditLog::open(&log_path).await.unwrap();

        let metadata = std::fs::metadata(&key_path).unwrap();
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "Key file should be owner-only read/write");
    }
}
