use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Maximum file size for full-content snapshots (10 MB default).
const MAX_SNAPSHOT_SIZE: u64 = 10 * 1024 * 1024;

/// Stores before-state file snapshots for ripcord rollback.
pub struct SnapshotStore {
    dir: PathBuf,
}

impl SnapshotStore {
    pub fn new(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
        }
    }

    /// Snapshot a file before modification. Returns the content hash.
    /// Returns None if the file doesn't exist (new file creation).
    /// Returns hash-only if file exceeds size threshold.
    pub async fn snapshot(&self, path: &Path) -> Result<Option<SnapshotResult>, SnapshotError> {
        let metadata = match tokio::fs::metadata(path).await {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(SnapshotError::Io(error)),
        };

        if metadata.len() > MAX_SNAPSHOT_SIZE {
            return Ok(Some(oversized_snapshot_result(&metadata)));
        }

        let content = tokio::fs::read(path).await.map_err(SnapshotError::Io)?;
        let hash = hex_hash(&content);
        self.store_snapshot(&hash, &content).await?;
        Ok(Some(SnapshotResult {
            hash,
            stored: true,
            size_bytes: metadata.len(),
        }))
    }

    /// Restore a file from its snapshot.
    pub async fn restore(&self, hash: &str, target: &Path) -> Result<(), SnapshotError> {
        let snapshot_path = self.snapshot_path(hash);
        let content = tokio::fs::read(&snapshot_path)
            .await
            .map_err(SnapshotError::Io)?;
        let actual_hash = hex_hash(&content);
        if actual_hash != hash {
            return Err(SnapshotError::IntegrityError {
                expected: hash.to_string(),
                actual: actual_hash,
            });
        }

        self.ensure_parent_dir(target).await?;
        tokio::fs::write(target, content)
            .await
            .map_err(SnapshotError::Io)?;
        Ok(())
    }

    /// Delete a snapshot by hash.
    pub async fn remove(&self, hash: &str) -> Result<(), SnapshotError> {
        let path = self.snapshot_path(hash);
        match tokio::fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(SnapshotError::Io(error)),
        }
    }

    /// Check if a snapshot exists.
    pub async fn exists(&self, hash: &str) -> bool {
        tokio::fs::metadata(self.snapshot_path(hash)).await.is_ok()
    }

    fn snapshot_path(&self, hash: &str) -> PathBuf {
        self.dir.join(format!("{hash}.snapshot"))
    }

    async fn store_snapshot(&self, hash: &str, content: &[u8]) -> Result<(), SnapshotError> {
        let snapshot_path = self.snapshot_path(hash);
        if tokio::fs::metadata(&snapshot_path).await.is_ok() {
            return Ok(());
        }

        self.ensure_parent_dir(&snapshot_path).await?;
        write_restricted(&snapshot_path, content).await
    }

    async fn ensure_parent_dir(&self, path: &Path) -> Result<(), SnapshotError> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(SnapshotError::Io)?;
        }
        Ok(())
    }
}

/// Result of a snapshot operation.
#[derive(Debug, Clone)]
pub struct SnapshotResult {
    /// Content hash of the file.
    pub hash: String,
    /// Whether the full content was stored (false = hash-only, file too large).
    pub stored: bool,
    /// File size in bytes.
    pub size_bytes: u64,
}

/// Snapshot errors.
#[derive(Debug)]
pub enum SnapshotError {
    Io(std::io::Error),
    IntegrityError { expected: String, actual: String },
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "snapshot I/O error: {error}"),
            Self::IntegrityError { expected, actual } => {
                write!(
                    f,
                    "snapshot integrity error: expected {expected}, got {actual}"
                )
            }
        }
    }
}

impl std::error::Error for SnapshotError {}

fn oversized_snapshot_result(metadata: &std::fs::Metadata) -> SnapshotResult {
    SnapshotResult {
        hash: format!("size_{}_{}", metadata.len(), file_mtime_nanos(metadata)),
        stored: false,
        size_bytes: metadata.len(),
    }
}

fn file_mtime_nanos(metadata: &std::fs::Metadata) -> u128 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

#[cfg(unix)]
async fn write_restricted(path: &Path, content: &[u8]) -> Result<(), SnapshotError> {
    use std::os::unix::fs::PermissionsExt;

    tokio::fs::write(path, content)
        .await
        .map_err(SnapshotError::Io)?;
    let permissions = std::fs::Permissions::from_mode(0o600);
    tokio::fs::set_permissions(path, permissions)
        .await
        .map_err(SnapshotError::Io)?;
    Ok(())
}

#[cfg(not(unix))]
async fn write_restricted(path: &Path, content: &[u8]) -> Result<(), SnapshotError> {
    tokio::fs::write(path, content)
        .await
        .map_err(SnapshotError::Io)?;
    Ok(())
}

fn hex_hash(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::{hex_hash, SnapshotError, SnapshotStore, MAX_SNAPSHOT_SIZE};
    use tempfile::TempDir;

    async fn create_file(path: &std::path::Path, content: &[u8]) {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .expect("create parent dirs");
        }
        tokio::fs::write(path, content)
            .await
            .expect("write test file");
    }

    #[tokio::test]
    async fn snapshot_nonexistent_file_returns_none() {
        let temp_dir = TempDir::new().expect("temp dir");
        let store = SnapshotStore::new(temp_dir.path());
        let missing = temp_dir.path().join("missing.txt");

        let result = store
            .snapshot(&missing)
            .await
            .expect("snapshot missing file");

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn snapshot_stores_content_and_hash() {
        let temp_dir = TempDir::new().expect("temp dir");
        let snapshot_dir = temp_dir.path().join("snapshots");
        let store = SnapshotStore::new(&snapshot_dir);
        let source = temp_dir.path().join("source.txt");
        create_file(&source, b"hello ripcord").await;

        let result = store
            .snapshot(&source)
            .await
            .expect("snapshot source file")
            .expect("snapshot result");

        assert!(result.stored);
        assert_eq!(result.size_bytes, 13);
        assert!(store.exists(&result.hash).await);
    }

    #[tokio::test]
    async fn snapshot_large_file_returns_hash_only_without_reading() {
        let temp_dir = TempDir::new().expect("temp dir");
        let snapshot_dir = temp_dir.path().join("snapshots");
        let store = SnapshotStore::new(&snapshot_dir);
        let source = temp_dir.path().join("large.bin");
        let oversized = vec![b'x'; (MAX_SNAPSHOT_SIZE + 1) as usize];
        create_file(&source, &oversized).await;

        let metadata = tokio::fs::metadata(&source).await.expect("source metadata");
        let result = store
            .snapshot(&source)
            .await
            .expect("snapshot large file")
            .expect("snapshot result");

        assert!(!result.stored);
        assert_eq!(result.size_bytes, metadata.len());
        assert_eq!(
            result.hash,
            format!(
                "size_{}_{}",
                metadata.len(),
                super::file_mtime_nanos(&metadata)
            )
        );
        assert!(!store.exists(&result.hash).await);
    }

    #[tokio::test]
    async fn snapshot_deduplicates_same_content() {
        let temp_dir = TempDir::new().expect("temp dir");
        let snapshot_dir = temp_dir.path().join("snapshots");
        let store = SnapshotStore::new(&snapshot_dir);
        let first = temp_dir.path().join("first.txt");
        let second = temp_dir.path().join("second.txt");
        create_file(&first, b"same content").await;
        create_file(&second, b"same content").await;

        let first_result = store
            .snapshot(&first)
            .await
            .expect("snapshot first")
            .expect("first result");
        let second_result = store
            .snapshot(&second)
            .await
            .expect("snapshot second")
            .expect("second result");
        let entries = std::fs::read_dir(&snapshot_dir)
            .expect("read snapshot dir")
            .count();

        assert_eq!(first_result.hash, second_result.hash);
        assert_eq!(entries, 1);
    }

    #[tokio::test]
    async fn restore_recreates_file() {
        let temp_dir = TempDir::new().expect("temp dir");
        let snapshot_dir = temp_dir.path().join("snapshots");
        let store = SnapshotStore::new(&snapshot_dir);
        let source = temp_dir.path().join("source.txt");
        let target = temp_dir.path().join("nested/target.txt");
        create_file(&source, b"restore me").await;
        let snapshot = store
            .snapshot(&source)
            .await
            .expect("snapshot source")
            .expect("snapshot result");

        store
            .restore(&snapshot.hash, &target)
            .await
            .expect("restore snapshot");
        let restored = tokio::fs::read(&target).await.expect("read restored file");

        assert_eq!(restored, b"restore me");
    }

    #[tokio::test]
    async fn restore_detects_corrupted_snapshot() {
        let temp_dir = TempDir::new().expect("temp dir");
        let snapshot_dir = temp_dir.path().join("snapshots");
        let store = SnapshotStore::new(&snapshot_dir);
        let source = temp_dir.path().join("source.txt");
        let target = temp_dir.path().join("restored.txt");
        create_file(&source, b"restore me").await;
        let snapshot = store
            .snapshot(&source)
            .await
            .expect("snapshot source")
            .expect("snapshot result");
        let snapshot_path = snapshot_dir.join(format!("{}.snapshot", snapshot.hash));
        tokio::fs::write(&snapshot_path, b"corrupted")
            .await
            .expect("corrupt snapshot");

        let error = store
            .restore(&snapshot.hash, &target)
            .await
            .expect_err("restore should fail on corrupted snapshot");

        match error {
            SnapshotError::IntegrityError { expected, actual } => {
                assert_eq!(expected, snapshot.hash);
                assert_eq!(actual, hex_hash(b"corrupted"));
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[tokio::test]
    async fn remove_deletes_snapshot() {
        let temp_dir = TempDir::new().expect("temp dir");
        let snapshot_dir = temp_dir.path().join("snapshots");
        let store = SnapshotStore::new(&snapshot_dir);
        let source = temp_dir.path().join("source.txt");
        create_file(&source, b"delete me").await;
        let snapshot = store
            .snapshot(&source)
            .await
            .expect("snapshot source")
            .expect("snapshot result");

        store.remove(&snapshot.hash).await.expect("remove snapshot");

        assert!(!store.exists(&snapshot.hash).await);
    }

    #[tokio::test]
    async fn exists_checks_snapshot() {
        let temp_dir = TempDir::new().expect("temp dir");
        let snapshot_dir = temp_dir.path().join("snapshots");
        let store = SnapshotStore::new(&snapshot_dir);
        let source = temp_dir.path().join("source.txt");
        create_file(&source, b"presence check").await;
        let snapshot = store
            .snapshot(&source)
            .await
            .expect("snapshot source")
            .expect("snapshot result");

        assert!(store.exists(&snapshot.hash).await);
        assert!(!store.exists("missing").await);
    }
}
