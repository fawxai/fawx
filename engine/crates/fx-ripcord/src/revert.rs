use crate::journal::{JournalAction, JournalEntry, RipcordJournal};
use crate::snapshot::SnapshotStore;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;

/// Result of pulling the ripcord.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RipcordReport {
    /// Entries that were successfully reverted.
    pub reverted: Vec<RevertedEntry>,
    /// Entries that could not be reverted.
    pub skipped: Vec<SkippedEntry>,
    /// Total entries processed.
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RevertedEntry {
    pub id: u64,
    pub tool_name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkippedEntry {
    pub id: u64,
    pub tool_name: String,
    pub reason: String,
}

/// Pull the ripcord: revert all reversible journal entries in reverse order.
/// Returns a report of what was reverted and what was skipped.
pub async fn pull_ripcord(journal: &Arc<RipcordJournal>) -> RipcordReport {
    let entries = journal.entries().await;
    let snapshots = journal.snapshots();
    let mut reverted = Vec::new();
    let mut skipped = Vec::new();

    for entry in entries.iter().rev() {
        match revert_entry(entry, snapshots.as_ref()).await {
            Ok(description) => reverted.push(build_reverted_entry(entry, description)),
            Err(reason) => skipped.push(build_skipped_entry(entry, reason)),
        }
    }

    let total = entries.len();
    journal.clear().await;
    RipcordReport {
        reverted,
        skipped,
        total,
    }
}

/// Approve the ripcord: clear the journal without reverting.
/// The user reviewed and decided to keep the changes.
pub async fn approve_ripcord(journal: &Arc<RipcordJournal>) {
    journal.clear().await;
}

async fn revert_entry(entry: &JournalEntry, snapshots: &SnapshotStore) -> Result<String, String> {
    match &entry.action {
        JournalAction::FileWrite {
            path,
            snapshot_hash: Some(hash),
            created: false,
            ..
        } => restore_snapshot(snapshots, hash, path, "restored file from snapshot").await,
        JournalAction::FileWrite {
            path,
            created: true,
            ..
        } => remove_created_file(path).await,
        JournalAction::FileWrite { .. } => Err(missing_snapshot_reason()),
        JournalAction::FileDelete {
            path,
            snapshot_hash,
        } => restore_snapshot(snapshots, snapshot_hash, path, "restored deleted file").await,
        JournalAction::FileMove { from, to } => reverse_move(from, to).await,
        JournalAction::GitCommit { repo, pre_ref, .. } => revert_git_commit(repo, pre_ref).await,
        JournalAction::GitBranchCreate { repo, branch } => {
            revert_git_branch_create(repo, branch).await
        }
        JournalAction::GitPush { .. } => Err(git_push_skip_reason().to_string()),
        JournalAction::ShellCommand { .. } => Err(shell_skip_reason().to_string()),
        JournalAction::NetworkRequest { .. } => Err(network_skip_reason().to_string()),
    }
}

fn build_reverted_entry(entry: &JournalEntry, description: String) -> RevertedEntry {
    RevertedEntry {
        id: entry.id,
        tool_name: entry.tool_name.clone(),
        description,
    }
}

fn build_skipped_entry(entry: &JournalEntry, reason: String) -> SkippedEntry {
    SkippedEntry {
        id: entry.id,
        tool_name: entry.tool_name.clone(),
        reason,
    }
}

async fn restore_snapshot(
    snapshots: &SnapshotStore,
    hash: &str,
    path: &Path,
    success: &str,
) -> Result<String, String> {
    snapshots
        .restore(hash, path)
        .await
        .map_err(|error| error.to_string())?;
    Ok(success.to_string())
}

async fn remove_created_file(path: &Path) -> Result<String, String> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok("deleted newly created file".to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok("created file already absent".to_string())
        }
        Err(error) => Err(format!("failed to delete created file: {error}")),
    }
}

async fn reverse_move(from: &Path, to: &Path) -> Result<String, String> {
    ensure_parent_dir(from).await?;
    tokio::fs::rename(to, from)
        .await
        .map_err(|error| format!("failed to reverse file move: {error}"))?;
    Ok("reversed file move".to_string())
}

async fn ensure_parent_dir(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| format!("failed to create parent directory: {error}"))?;
    }
    Ok(())
}

async fn revert_git_commit(repo: &Path, pre_ref: &str) -> Result<String, String> {
    git_command(repo, &["reset", "--hard", pre_ref]).await?;
    Ok(format!("reset git commit to {pre_ref}"))
}

async fn revert_git_branch_create(repo: &Path, branch: &str) -> Result<String, String> {
    git_command(repo, &["branch", "-D", branch]).await?;
    Ok(format!("deleted git branch {branch}"))
}

async fn git_command(repo: &Path, args: &[&str]) -> Result<(), String> {
    let output = tokio::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .await
        .map_err(|error| format!("failed to run git: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git command failed: {stderr}"));
    }

    Ok(())
}

fn missing_snapshot_reason() -> String {
    "File write cannot be reverted without a stored snapshot".to_string()
}

fn git_push_skip_reason() -> &'static str {
    "Git push cannot be safely auto-reverted. Force-push may be needed."
}

fn shell_skip_reason() -> &'static str {
    "Shell command side effects cannot be reverted (audit only)"
}

fn network_skip_reason() -> &'static str {
    "Network request cannot be reverted (audit only)"
}

#[cfg(test)]
mod tests {
    use super::{
        approve_ripcord, git_push_skip_reason, network_skip_reason, pull_ripcord, shell_skip_reason,
    };
    use crate::journal::{JournalAction, RipcordJournal};
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::Arc;
    use tempfile::TempDir;

    struct FileSetup {
        temp_dir: TempDir,
        journal: Arc<RipcordJournal>,
        file_path: PathBuf,
    }

    fn build_file_setup(file_name: &str) -> FileSetup {
        let temp_dir = TempDir::new().expect("temp dir");
        let snapshot_dir = temp_dir.path().join("snapshots");
        let journal = Arc::new(RipcordJournal::new(&snapshot_dir));
        let file_path = temp_dir.path().join(file_name);
        FileSetup {
            temp_dir,
            journal,
            file_path,
        }
    }

    async fn write_file(path: &Path, content: &[u8]) {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .expect("create parent dir");
        }
        tokio::fs::write(path, content).await.expect("write file");
    }

    async fn read_file(path: &Path) -> Vec<u8> {
        tokio::fs::read(path).await.expect("read file")
    }

    async fn head_sha(repo: &Path) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["rev-parse", "HEAD"])
            .output()
            .expect("read git head");
        assert!(output.status.success());
        String::from_utf8(output.stdout)
            .expect("head sha utf8")
            .trim()
            .to_string()
    }

    fn run_git(repo: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .status()
            .expect("run git command");
        assert!(status.success(), "git command failed: {args:?}");
    }

    fn init_git_repo(path: &Path) {
        run_git(path, &["init"]);
        run_git(path, &["config", "user.name", "Ripcord Tester"]);
        run_git(path, &["config", "user.email", "ripcord@example.com"]);
    }

    #[tokio::test]
    async fn revert_file_write_restores_from_snapshot() {
        let setup = build_file_setup("tracked.txt");
        write_file(&setup.file_path, b"original").await;
        let snapshot = setup
            .journal
            .snapshots()
            .snapshot(&setup.file_path)
            .await
            .expect("snapshot original")
            .expect("snapshot result");
        write_file(&setup.file_path, b"changed").await;

        setup
            .journal
            .record(
                "write_file",
                "call-1",
                JournalAction::FileWrite {
                    path: setup.file_path.clone(),
                    snapshot_hash: Some(snapshot.hash),
                    size_bytes: 7,
                    created: false,
                },
            )
            .await;

        let report = pull_ripcord(&setup.journal).await;

        assert_eq!(read_file(&setup.file_path).await, b"original");
        assert_eq!(report.reverted.len(), 1);
        assert!(report.skipped.is_empty());
    }

    #[tokio::test]
    async fn revert_created_file_deletes_it() {
        let setup = build_file_setup("created.txt");
        write_file(&setup.file_path, b"new file").await;

        setup
            .journal
            .record(
                "write_file",
                "call-1",
                JournalAction::FileWrite {
                    path: setup.file_path.clone(),
                    snapshot_hash: None,
                    size_bytes: 8,
                    created: true,
                },
            )
            .await;

        let report = pull_ripcord(&setup.journal).await;

        assert!(!setup.file_path.exists());
        assert_eq!(report.reverted.len(), 1);
        assert!(report.skipped.is_empty());
    }

    #[tokio::test]
    async fn revert_file_delete_restores_content() {
        let setup = build_file_setup("deleted.txt");
        write_file(&setup.file_path, b"restore me").await;
        let snapshot = setup
            .journal
            .snapshots()
            .snapshot(&setup.file_path)
            .await
            .expect("snapshot file")
            .expect("snapshot result");
        tokio::fs::remove_file(&setup.file_path)
            .await
            .expect("delete file");

        setup
            .journal
            .record(
                "delete_file",
                "call-1",
                JournalAction::FileDelete {
                    path: setup.file_path.clone(),
                    snapshot_hash: snapshot.hash,
                },
            )
            .await;

        let report = pull_ripcord(&setup.journal).await;

        assert_eq!(read_file(&setup.file_path).await, b"restore me");
        assert_eq!(report.reverted.len(), 1);
    }

    #[tokio::test]
    async fn revert_file_move_reverses_rename() {
        let temp_dir = TempDir::new().expect("temp dir");
        let snapshot_dir = temp_dir.path().join("snapshots");
        let journal = Arc::new(RipcordJournal::new(&snapshot_dir));
        let from = temp_dir.path().join("from.txt");
        let to = temp_dir.path().join("nested/to.txt");
        write_file(&from, b"moved").await;
        tokio::fs::create_dir_all(to.parent().expect("to parent"))
            .await
            .expect("create move target dir");
        tokio::fs::rename(&from, &to).await.expect("move file");

        journal
            .record(
                "move_file",
                "call-1",
                JournalAction::FileMove {
                    from: from.clone(),
                    to: to.clone(),
                },
            )
            .await;

        let report = pull_ripcord(&journal).await;

        assert_eq!(read_file(&from).await, b"moved");
        assert!(!to.exists());
        assert_eq!(report.reverted.len(), 1);
    }

    #[tokio::test]
    async fn revert_git_commit_resets_to_pre_ref() {
        let temp_dir = TempDir::new().expect("temp dir");
        init_git_repo(temp_dir.path());
        let snapshot_dir = temp_dir.path().join("snapshots");
        let journal = Arc::new(RipcordJournal::new(&snapshot_dir));
        let file_path = temp_dir.path().join("repo.txt");
        write_file(&file_path, b"first").await;
        run_git(temp_dir.path(), &["add", "."]);
        run_git(temp_dir.path(), &["commit", "-m", "first"]);
        let pre_ref = head_sha(temp_dir.path()).await;

        write_file(&file_path, b"second").await;
        run_git(temp_dir.path(), &["add", "."]);
        run_git(temp_dir.path(), &["commit", "-m", "second"]);

        journal
            .record(
                "git_commit",
                "call-1",
                JournalAction::GitCommit {
                    repo: temp_dir.path().to_path_buf(),
                    pre_ref: pre_ref.clone(),
                    commit_sha: head_sha(temp_dir.path()).await,
                },
            )
            .await;

        let report = pull_ripcord(&journal).await;

        assert_eq!(head_sha(temp_dir.path()).await, pre_ref);
        assert_eq!(report.reverted.len(), 1);
    }

    #[tokio::test]
    async fn revert_skips_shell_command() {
        let setup = build_file_setup("noop.txt");

        setup
            .journal
            .record(
                "shell",
                "call-1",
                JournalAction::ShellCommand {
                    command: "echo hi".to_string(),
                    exit_code: 0,
                },
            )
            .await;

        let report = pull_ripcord(&setup.journal).await;

        assert!(report.reverted.is_empty());
        assert_eq!(report.skipped.len(), 1);
        assert_eq!(report.skipped[0].reason, shell_skip_reason());
    }

    #[tokio::test]
    async fn revert_skips_network_request() {
        let setup = build_file_setup("noop.txt");

        setup
            .journal
            .record(
                "network",
                "call-1",
                JournalAction::NetworkRequest {
                    url: "https://example.com".to_string(),
                    method: "POST".to_string(),
                    status_code: 200,
                },
            )
            .await;

        let report = pull_ripcord(&setup.journal).await;

        assert!(report.reverted.is_empty());
        assert_eq!(report.skipped.len(), 1);
        assert_eq!(report.skipped[0].reason, network_skip_reason());
    }

    #[tokio::test]
    async fn revert_skips_git_push() {
        let setup = build_file_setup("noop.txt");

        setup
            .journal
            .record(
                "git_push",
                "call-1",
                JournalAction::GitPush {
                    repo: setup.temp_dir.path().to_path_buf(),
                    remote: "origin".to_string(),
                    branch: "dev".to_string(),
                    pre_ref: "HEAD~1".to_string(),
                },
            )
            .await;

        let report = pull_ripcord(&setup.journal).await;

        assert!(report.reverted.is_empty());
        assert_eq!(report.skipped.len(), 1);
        assert_eq!(report.skipped[0].reason, git_push_skip_reason());
    }

    #[tokio::test]
    async fn revert_processes_in_reverse_order() {
        let temp_dir = TempDir::new().expect("temp dir");
        let snapshot_dir = temp_dir.path().join("snapshots");
        let journal = Arc::new(RipcordJournal::new(&snapshot_dir));
        let first = temp_dir.path().join("first.txt");
        let second = temp_dir.path().join("second.txt");
        write_file(&first, b"one").await;
        write_file(&second, b"two").await;

        journal
            .record(
                "write_file",
                "call-1",
                JournalAction::FileWrite {
                    path: first.clone(),
                    snapshot_hash: None,
                    size_bytes: 3,
                    created: true,
                },
            )
            .await;
        journal
            .record(
                "write_file",
                "call-2",
                JournalAction::FileWrite {
                    path: second.clone(),
                    snapshot_hash: None,
                    size_bytes: 3,
                    created: true,
                },
            )
            .await;

        let report = pull_ripcord(&journal).await;

        assert_eq!(report.reverted.len(), 2);
        assert_eq!(report.reverted[0].tool_name, "write_file");
        assert!(report.reverted[0].id > report.reverted[1].id);
        assert!(!first.exists());
        assert!(!second.exists());
    }

    #[tokio::test]
    async fn approve_clears_without_reverting() {
        let setup = build_file_setup("approved.txt");
        write_file(&setup.file_path, b"keep me").await;

        setup
            .journal
            .record(
                "write_file",
                "call-1",
                JournalAction::FileWrite {
                    path: setup.file_path.clone(),
                    snapshot_hash: None,
                    size_bytes: 7,
                    created: true,
                },
            )
            .await;

        approve_ripcord(&setup.journal).await;

        assert_eq!(read_file(&setup.file_path).await, b"keep me");
        assert!(setup.journal.entries().await.is_empty());
    }

    #[tokio::test]
    async fn pull_clears_journal_after_revert() {
        let setup = build_file_setup("pull-clear.txt");
        setup.journal.activate("tripwire", "desc").await;
        write_file(&setup.file_path, b"clear me").await;

        setup
            .journal
            .record(
                "write_file",
                "call-1",
                JournalAction::FileWrite {
                    path: setup.file_path.clone(),
                    snapshot_hash: None,
                    size_bytes: 8,
                    created: true,
                },
            )
            .await;

        let _ = pull_ripcord(&setup.journal).await;

        assert!(!setup.journal.is_active().await);
        assert!(setup.journal.entries().await.is_empty());
    }
}
