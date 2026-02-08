//! Append-only audit log with tamper detection via hash chains.

use super::types::{AuditEvent, AuditEventType};
use nv_core::error::SecurityError;
use ring::digest::{digest, SHA256};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

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

/// Internal log entry with hash chain for integrity
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AuditEntry {
    /// The actual event
    event: AuditEvent,

    /// SHA-256 hash of the previous entry (hex-encoded)
    prev_hash: String,

    /// SHA-256 hash of this entry (hex-encoded)
    hash: String,
}

impl AuditEntry {
    /// Compute hash for this entry, including all fields for tamper detection.
    ///
    /// Returns `Err` if event fields cannot be serialized, rather than silently
    /// corrupting the hash chain.
    fn compute_hash(event: &AuditEvent, prev_hash: &str) -> Result<String, SecurityError> {
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

        let hash = digest(&SHA256, data.as_bytes());
        Ok(hex::encode(hash.as_ref()))
    }

    /// Create a new entry with hash chain
    fn new(event: AuditEvent, prev_hash: String) -> Result<Self, SecurityError> {
        let hash = Self::compute_hash(&event, &prev_hash)?;
        Ok(Self {
            event,
            prev_hash,
            hash,
        })
    }

    /// Verify this entry's hash is correct
    fn verify(&self) -> Result<bool, SecurityError> {
        let expected_hash = Self::compute_hash(&self.event, &self.prev_hash)?;
        Ok(self.hash == expected_hash)
    }
}

/// Append-only audit log with tamper detection
pub struct AuditLog {
    /// Path to the log file (None for in-memory)
    path: Option<PathBuf>,

    /// Cached entries (for in-memory mode and verification)
    entries: Vec<AuditEntry>,

    /// Last hash in the chain
    last_hash: String,
}

impl AuditLog {
    /// Open or create an audit log file
    pub fn open(path: &Path) -> Result<Self, SecurityError> {
        let mut entries = Vec::new();
        let mut last_hash = "GENESIS".to_string();

        // Read existing entries if file exists
        if path.exists() {
            let file = File::open(path)
                .map_err(|e| SecurityError::AuditLog(format!("Failed to open log: {}", e)))?;

            let reader = BufReader::new(file);
            for line in reader.lines() {
                let line = line
                    .map_err(|e| SecurityError::AuditLog(format!("Failed to read line: {}", e)))?;

                let entry: AuditEntry = serde_json::from_str(&line).map_err(|e| {
                    SecurityError::AuditLog(format!("Failed to parse entry: {}", e))
                })?;

                last_hash = entry.hash.clone();
                entries.push(entry);
            }
        }

        Ok(Self {
            path: Some(path.to_path_buf()),
            entries,
            last_hash,
        })
    }

    /// Create an in-memory audit log (for testing)
    pub fn in_memory() -> Self {
        Self {
            path: None,
            entries: Vec::new(),
            last_hash: "GENESIS".to_string(),
        }
    }

    /// Append an event to the log
    pub fn append(&mut self, event: AuditEvent) -> Result<(), SecurityError> {
        let entry = AuditEntry::new(event, self.last_hash.clone())?;

        // Write to file if not in-memory
        if let Some(ref path) = self.path {
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .map_err(|e| SecurityError::AuditLog(format!("Failed to open log: {}", e)))?;

            let line = serde_json::to_string(&entry).map_err(|e| {
                SecurityError::AuditLog(format!("Failed to serialize entry: {}", e))
            })?;

            writeln!(file, "{}", line)
                .map_err(|e| SecurityError::AuditLog(format!("Failed to write entry: {}", e)))?;
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

    /// Verify the integrity of the entire log (hash chain)
    pub fn verify_integrity(&self) -> Result<bool, SecurityError> {
        let mut prev_hash = "GENESIS".to_string();

        for entry in &self.entries {
            // Check if prev_hash matches
            if entry.prev_hash != prev_hash {
                return Ok(false);
            }

            // Verify the entry's hash
            if !entry.verify()? {
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

    #[test]
    fn test_create_and_append_event() {
        let mut log = AuditLog::in_memory();
        let event =
            AuditEvent::new(AuditEventType::ActionExecuted, "agent", "Test action").unwrap();

        assert!(log.append(event).is_ok());
        assert_eq!(log.count(), 1);
    }

    #[test]
    fn test_append_multiple_events() {
        let mut log = AuditLog::in_memory();

        for i in 0..5 {
            let event = AuditEvent::new(
                AuditEventType::ActionExecuted,
                "agent",
                format!("Action {}", i),
            )
            .unwrap();
            assert!(log.append(event).is_ok());
        }

        assert_eq!(log.count(), 5);
    }

    #[test]
    fn test_query_by_event_type() {
        let mut log = AuditLog::in_memory();

        log.append(AuditEvent::new(AuditEventType::ActionExecuted, "agent", "Action 1").unwrap())
            .unwrap();

        log.append(AuditEvent::new(AuditEventType::ActionDenied, "agent", "Action 2").unwrap())
            .unwrap();

        log.append(AuditEvent::new(AuditEventType::ActionExecuted, "agent", "Action 3").unwrap())
            .unwrap();

        let filter = AuditFilter {
            event_type: Some(AuditEventType::ActionExecuted),
            ..Default::default()
        };

        let results = log.query(&filter).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].description, "Action 1");
        assert_eq!(results[1].description, "Action 3");
    }

    #[test]
    fn test_query_by_actor() {
        let mut log = AuditLog::in_memory();

        log.append(AuditEvent::new(AuditEventType::ActionExecuted, "agent", "Action 1").unwrap())
            .unwrap();

        log.append(AuditEvent::new(AuditEventType::ActionExecuted, "user", "Action 2").unwrap())
            .unwrap();

        log.append(AuditEvent::new(AuditEventType::ActionExecuted, "agent", "Action 3").unwrap())
            .unwrap();

        let filter = AuditFilter {
            actor: Some("agent".to_string()),
            ..Default::default()
        };

        let results = log.query(&filter).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_query_by_time_range() {
        let mut log = AuditLog::in_memory();

        let event1 = AuditEvent {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: 1000,
            event_type: AuditEventType::ActionExecuted,
            actor: "agent".to_string(),
            description: "Action 1".to_string(),
            metadata: Default::default(),
        };

        let event2 = AuditEvent {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: 2000,
            event_type: AuditEventType::ActionExecuted,
            actor: "agent".to_string(),
            description: "Action 2".to_string(),
            metadata: Default::default(),
        };

        let event3 = AuditEvent {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: 3000,
            event_type: AuditEventType::ActionExecuted,
            actor: "agent".to_string(),
            description: "Action 3".to_string(),
            metadata: Default::default(),
        };

        log.append(event1).unwrap();
        log.append(event2).unwrap();
        log.append(event3).unwrap();

        let filter = AuditFilter {
            after: Some(1500),
            before: Some(2500),
            ..Default::default()
        };

        let results = log.query(&filter).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].description, "Action 2");
    }

    #[test]
    fn test_query_with_limit() {
        let mut log = AuditLog::in_memory();

        for i in 0..10 {
            log.append(
                AuditEvent::new(
                    AuditEventType::ActionExecuted,
                    "agent",
                    format!("Action {}", i),
                )
                .unwrap(),
            )
            .unwrap();
        }

        let filter = AuditFilter {
            limit: Some(3),
            ..Default::default()
        };

        let results = log.query(&filter).unwrap();
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_count_events() {
        let mut log = AuditLog::in_memory();
        assert_eq!(log.count(), 0);

        log.append(AuditEvent::new(AuditEventType::ActionExecuted, "agent", "Test").unwrap())
            .unwrap();

        assert_eq!(log.count(), 1);

        log.append(AuditEvent::new(AuditEventType::ActionExecuted, "agent", "Test 2").unwrap())
            .unwrap();

        assert_eq!(log.count(), 2);
    }

    #[test]
    fn test_hash_chain_integrity_valid() {
        let mut log = AuditLog::in_memory();

        for i in 0..5 {
            log.append(
                AuditEvent::new(
                    AuditEventType::ActionExecuted,
                    "agent",
                    format!("Action {}", i),
                )
                .unwrap(),
            )
            .unwrap();
        }

        assert!(log.verify_integrity().unwrap());
    }

    #[test]
    fn test_hash_chain_integrity_tampered() {
        let mut log = AuditLog::in_memory();

        for i in 0..5 {
            log.append(
                AuditEvent::new(
                    AuditEventType::ActionExecuted,
                    "agent",
                    format!("Action {}", i),
                )
                .unwrap(),
            )
            .unwrap();
        }

        // Tamper with an entry
        log.entries[2].event.description = "TAMPERED".to_string();

        assert!(!log.verify_integrity().unwrap());
    }

    #[test]
    fn test_in_memory_mode() {
        let log = AuditLog::in_memory();
        assert!(log.path.is_none());
        assert_eq!(log.count(), 0);
    }

    #[test]
    fn test_empty_log_query() {
        let log = AuditLog::in_memory();
        let results = log.query(&AuditFilter::default()).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_no_match_query() {
        let mut log = AuditLog::in_memory();

        log.append(AuditEvent::new(AuditEventType::ActionExecuted, "agent", "Test").unwrap())
            .unwrap();

        let filter = AuditFilter {
            event_type: Some(AuditEventType::PolicyViolation),
            ..Default::default()
        };

        let results = log.query(&filter).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_multiple_filters_combined() {
        let mut log = AuditLog::in_memory();

        log.append(AuditEvent {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: 1000,
            event_type: AuditEventType::ActionExecuted,
            actor: "agent".to_string(),
            description: "Match".to_string(),
            metadata: Default::default(),
        })
        .unwrap();

        log.append(AuditEvent {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: 2000,
            event_type: AuditEventType::ActionExecuted,
            actor: "user".to_string(),
            description: "No match - wrong actor".to_string(),
            metadata: Default::default(),
        })
        .unwrap();

        log.append(AuditEvent {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: 3000,
            event_type: AuditEventType::ActionDenied,
            actor: "agent".to_string(),
            description: "No match - wrong type".to_string(),
            metadata: Default::default(),
        })
        .unwrap();

        let filter = AuditFilter {
            event_type: Some(AuditEventType::ActionExecuted),
            actor: Some("agent".to_string()),
            after: Some(500),
            before: Some(1500),
            ..Default::default()
        };

        let results = log.query(&filter).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].description, "Match");
    }

    #[test]
    fn test_file_persistence() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("audit.log");

        // Create log and write events
        {
            let mut log = AuditLog::open(&log_path).unwrap();
            log.append(AuditEvent::new(AuditEventType::ActionExecuted, "agent", "Test 1").unwrap())
                .unwrap();

            log.append(AuditEvent::new(AuditEventType::ActionExecuted, "agent", "Test 2").unwrap())
                .unwrap();
        }

        // Reopen and verify
        {
            let log = AuditLog::open(&log_path).unwrap();
            assert_eq!(log.count(), 2);
            assert!(log.verify_integrity().unwrap());
        }

        #[test]
        fn test_open_with_malformed_json() {
            let dir = tempdir().unwrap();
            let log_path = dir.path().join("corrupt.log");
            std::fs::write(&log_path, "not valid json\n").unwrap();

            let result = AuditLog::open(&log_path);
            assert!(result.is_err());
        }
    }
}
