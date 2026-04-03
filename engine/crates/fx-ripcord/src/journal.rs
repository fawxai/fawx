use crate::snapshot::SnapshotStore;
pub use fx_kernel::act::JournalAction;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::RwLock;

static ENTRY_COUNTER: AtomicU64 = AtomicU64::new(0);

/// The ripcord journal tracks actions after a tripwire is crossed.
pub struct RipcordJournal {
    entries: RwLock<Vec<JournalEntry>>,
    status: RwLock<RipcordStatus>,
    snapshots: Arc<SnapshotStore>,
    /// Per-category action counts for threshold tripwires.
    category_counts: RwLock<HashMap<String, u32>>,
}

/// Current ripcord status.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RipcordStatus {
    /// Whether the ripcord is currently active (tripwire was crossed).
    pub active: bool,
    /// Which tripwire triggered the ripcord.
    pub tripwire_id: Option<String>,
    /// Description of the triggering tripwire.
    pub tripwire_description: Option<String>,
    /// When the tripwire was crossed.
    pub activated_at: Option<SystemTime>,
    /// Number of journaled entries since activation.
    pub entry_count: u64,
}

/// A single journaled action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    pub id: u64,
    pub timestamp: SystemTime,
    pub tool_name: String,
    pub tool_call_id: String,
    pub action: JournalAction,
    pub reversible: bool,
}

impl RipcordJournal {
    /// Create a new journal backed by the given snapshot directory.
    pub fn new(snapshot_dir: &Path) -> Self {
        Self {
            entries: RwLock::new(Vec::new()),
            status: RwLock::new(RipcordStatus::default()),
            snapshots: Arc::new(SnapshotStore::new(snapshot_dir)),
            category_counts: RwLock::new(HashMap::new()),
        }
    }

    /// Get the snapshot store for pre-action snapshotting.
    pub fn snapshots(&self) -> &Arc<SnapshotStore> {
        &self.snapshots
    }

    /// Activate the ripcord (called when a tripwire fires).
    pub async fn activate(&self, tripwire_id: &str, description: &str) {
        let mut status = self.status.write().await;
        if status.active {
            return;
        }
        *status = activated_status(tripwire_id, description);
    }

    /// Whether the ripcord is currently active.
    pub async fn is_active(&self) -> bool {
        self.status.read().await.active
    }

    /// Get current status.
    pub async fn status(&self) -> RipcordStatus {
        self.status.read().await.clone()
    }

    /// Record an action in the journal.
    pub async fn record(&self, tool_name: &str, tool_call_id: &str, action: JournalAction) {
        let reversible = action.is_reversible();
        let entry = JournalEntry {
            id: ENTRY_COUNTER.fetch_add(1, Ordering::Relaxed),
            timestamp: SystemTime::now(),
            tool_name: tool_name.to_string(),
            tool_call_id: tool_call_id.to_string(),
            action,
            reversible,
        };
        let mut entries = self.entries.write().await;
        entries.push(entry);
        let mut status = self.status.write().await;
        status.entry_count = entries.len() as u64;
    }

    /// Increment the action count for a category.
    pub async fn increment_category(&self, category: &str) {
        let mut counts = self.category_counts.write().await;
        *counts.entry(category.to_string()).or_insert(0) += 1;
    }

    /// Get current category counts (for threshold tripwire evaluation).
    pub async fn category_counts(&self) -> HashMap<String, u32> {
        self.category_counts.read().await.clone()
    }

    /// Get all journal entries (for review UI).
    pub async fn entries(&self) -> Vec<JournalEntry> {
        self.entries.read().await.clone()
    }

    /// Clear the journal (after ripcord pull or approval).
    pub async fn clear(&self) {
        self.entries.write().await.clear();
        *self.status.write().await = RipcordStatus::default();
        self.category_counts.write().await.clear();
    }
}

fn activated_status(tripwire_id: &str, description: &str) -> RipcordStatus {
    RipcordStatus {
        active: true,
        tripwire_id: Some(tripwire_id.to_string()),
        tripwire_description: Some(description.to_string()),
        activated_at: Some(SystemTime::now()),
        entry_count: 0,
    }
}

#[cfg(test)]
fn build_entry(tool_name: &str, tool_call_id: &str, action: JournalAction) -> JournalEntry {
    JournalEntry {
        id: ENTRY_COUNTER.fetch_add(1, Ordering::Relaxed),
        timestamp: SystemTime::now(),
        tool_name: tool_name.to_string(),
        tool_call_id: tool_call_id.to_string(),
        reversible: action.is_reversible(),
        action,
    }
}

#[cfg(test)]
mod tests {
    use super::{build_entry, JournalAction, RipcordJournal};
    use tempfile::TempDir;

    fn sample_file_write() -> JournalAction {
        JournalAction::FileWrite {
            path: "file.txt".into(),
            snapshot_hash: Some("abc123".into()),
            size_bytes: 12,
            created: false,
        }
    }

    fn sample_git_push() -> JournalAction {
        JournalAction::GitPush {
            repo: "/repo".into(),
            remote: "origin".into(),
            branch: "dev".into(),
            pre_ref: "HEAD~1".into(),
        }
    }

    #[tokio::test]
    async fn new_journal_is_inactive() {
        let temp_dir = TempDir::new().expect("temp dir");
        let journal = RipcordJournal::new(temp_dir.path());

        assert!(!journal.is_active().await);
        assert_eq!(journal.status().await.entry_count, 0);
    }

    #[tokio::test]
    async fn activate_sets_status() {
        let temp_dir = TempDir::new().expect("temp dir");
        let journal = RipcordJournal::new(temp_dir.path());

        journal.activate("bulk_delete", "Bulk file deletion").await;
        let status = journal.status().await;

        assert!(status.active);
        assert_eq!(status.tripwire_id.as_deref(), Some("bulk_delete"));
        assert_eq!(
            status.tripwire_description.as_deref(),
            Some("Bulk file deletion")
        );
        assert!(status.activated_at.is_some());
    }

    #[tokio::test]
    async fn activate_is_idempotent() {
        let temp_dir = TempDir::new().expect("temp dir");
        let journal = RipcordJournal::new(temp_dir.path());

        journal.activate("first", "first tripwire").await;
        let first = journal.status().await;
        journal.activate("second", "second tripwire").await;
        let second = journal.status().await;

        assert_eq!(second.tripwire_id, first.tripwire_id);
        assert_eq!(second.tripwire_description, first.tripwire_description);
        assert_eq!(second.activated_at, first.activated_at);
    }

    #[tokio::test]
    async fn record_increments_entry_count() {
        let temp_dir = TempDir::new().expect("temp dir");
        let journal = RipcordJournal::new(temp_dir.path());

        journal
            .record("write_file", "call-1", sample_file_write())
            .await;

        assert_eq!(journal.status().await.entry_count, 1);
    }

    #[test]
    fn journal_action_reversibility() {
        assert!(sample_file_write().is_reversible());
        assert!(!sample_git_push().is_reversible());
    }

    #[test]
    fn build_entry_sets_reversible_flag_from_action() {
        let reversible = build_entry("write_file", "call-1", sample_file_write());
        let irreversible = build_entry("git", "call-2", sample_git_push());

        assert!(reversible.reversible);
        assert!(!irreversible.reversible);
    }

    #[tokio::test]
    async fn clear_resets_everything() {
        let temp_dir = TempDir::new().expect("temp dir");
        let journal = RipcordJournal::new(temp_dir.path());

        journal.activate("bulk_delete", "Bulk file deletion").await;
        journal.increment_category("file_delete").await;
        journal
            .record("write_file", "call-1", sample_file_write())
            .await;
        journal.clear().await;

        let status = journal.status().await;
        assert!(!status.active);
        assert_eq!(status.entry_count, 0);
        assert!(journal.entries().await.is_empty());
        assert!(journal.category_counts().await.is_empty());
    }

    #[tokio::test]
    async fn entries_returns_in_order() {
        let temp_dir = TempDir::new().expect("temp dir");
        let journal = RipcordJournal::new(temp_dir.path());

        journal
            .record("write_file", "call-1", sample_file_write())
            .await;
        journal.record("git", "call-2", sample_git_push()).await;
        let entries = journal.entries().await;

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].tool_call_id, "call-1");
        assert_eq!(entries[1].tool_call_id, "call-2");
    }

    #[tokio::test]
    async fn category_counts_increment() {
        let temp_dir = TempDir::new().expect("temp dir");
        let journal = RipcordJournal::new(temp_dir.path());

        journal.increment_category("file_delete").await;
        journal.increment_category("file_delete").await;
        let counts = journal.category_counts().await;

        assert_eq!(counts.get("file_delete"), Some(&2));
    }
}
