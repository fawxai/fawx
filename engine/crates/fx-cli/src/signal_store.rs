//! JSONL-backed signal persistence.
//!
//! Writes signals from each loop cycle to a per-session JSONL file under
//! `~/.fawx/signals/`. One JSON object per line, append-only.

use fx_kernel::signals::Signal;
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Errors that can occur during signal store operations.
#[derive(Debug)]
pub enum SignalStoreError {
    /// Failed to create or access the signals directory.
    DirectoryAccess(std::io::Error),
    /// Failed to open a signal file for writing.
    FileOpen(std::io::Error),
    /// Failed to write to a signal file.
    FileWrite(std::io::Error),
    /// Failed to serialize a signal to JSON.
    Serialize(serde_json::Error),
    /// Failed to read the signals directory for cleanup.
    DirectoryRead(std::io::Error),
}

impl fmt::Display for SignalStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DirectoryAccess(e) => write!(f, "signal dir access: {e}"),
            Self::FileOpen(e) => write!(f, "signal persist open: {e}"),
            Self::FileWrite(e) => write!(f, "signal persist write: {e}"),
            Self::Serialize(e) => write!(f, "signal serialize: {e}"),
            Self::DirectoryRead(e) => write!(f, "read signals dir: {e}"),
        }
    }
}

impl std::error::Error for SignalStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::DirectoryAccess(e)
            | Self::FileOpen(e)
            | Self::FileWrite(e)
            | Self::DirectoryRead(e) => Some(e),
            Self::Serialize(e) => Some(e),
        }
    }
}

/// Persists signals to JSONL files, one file per session.
#[derive(Debug)]
pub struct SignalStore {
    signals_dir: PathBuf,
    session_id: String,
}

const RETENTION_DAYS: u64 = 30;
const RETENTION_MS: u64 = RETENTION_DAYS * 24 * 60 * 60 * 1000;

impl SignalStore {
    /// Create a new signal store. Creates the signals directory if needed.
    pub fn new(data_dir: &Path, session_id: &str) -> Result<Self, SignalStoreError> {
        let signals_dir = data_dir.join("signals");
        fs::create_dir_all(&signals_dir).map_err(SignalStoreError::DirectoryAccess)?;
        Ok(Self {
            signals_dir,
            session_id: session_id.to_string(),
        })
    }

    /// Append signals from a loop cycle to the session's JSONL file.
    pub fn persist(&self, signals: &[Signal]) -> Result<(), SignalStoreError> {
        if signals.is_empty() {
            return Ok(());
        }
        let path = self.signals_dir.join(format!("{}.jsonl", self.session_id));
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(SignalStoreError::FileOpen)?;
        for signal in signals {
            let redacted = redact_signal_for_persist(signal);
            let json = serde_json::to_string(&redacted).map_err(SignalStoreError::Serialize)?;
            writeln!(file, "{json}").map_err(SignalStoreError::FileWrite)?;
        }
        Ok(())
    }

    /// Remove signal files older than the retention period.
    pub fn cleanup_old_signals(&self) -> Result<usize, SignalStoreError> {
        // as_millis() returns u128; u64::MAX covers ~584 million years so
        // truncation cannot occur for any realistic timestamp.
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
            .unwrap_or(0);
        let cutoff_ms = now_ms.saturating_sub(RETENTION_MS);

        let entries = fs::read_dir(&self.signals_dir).map_err(SignalStoreError::DirectoryRead)?;

        let mut removed = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let modified_ms = path
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
                .unwrap_or(now_ms);
            if modified_ms < cutoff_ms && fs::remove_file(&path).is_ok() {
                removed += 1;
            }
        }
        Ok(removed)
    }
}

/// Redact signal messages that might contain sensitive user input.
///
/// Looks for known separator patterns (` for `, ` with input `, ` on query `,
/// `": "`, `": '`) that typically precede user-provided data such as search
/// queries or file contents. When a separator is found, everything after it
/// is replaced with `[redacted]`. Messages without a recognized separator
/// are returned unchanged.
fn redact_signal_message(message: &str) -> String {
    for separator in [" for ", " with input ", " on query ", ": \"", ": '"] {
        if let Some(pos) = message.find(separator) {
            return format!("{}[redacted]", &message[..pos + separator.len()]);
        }
    }
    message.to_string()
}

fn redact_signal_for_persist(signal: &Signal) -> Signal {
    let mut redacted = signal.clone();
    redacted.message = redact_signal_message(&signal.message);
    redact_signal_metadata_output(&mut redacted.metadata);
    redacted
}

fn redact_signal_metadata_output(metadata: &mut serde_json::Value) {
    let Some(map) = metadata.as_object_mut() else {
        return;
    };
    let Some(output) = map.get("output").cloned() else {
        return;
    };

    let byte_count = redacted_output_byte_count(&output);
    map.insert(
        "output".to_string(),
        serde_json::Value::String(format!("[redacted: {byte_count} bytes]")),
    );
}

fn redacted_output_byte_count(output: &serde_json::Value) -> usize {
    match output {
        serde_json::Value::String(value) => value.len(),
        _ => output.to_string().len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_kernel::signals::{LoopStep, SignalKind};
    use std::io::BufRead;
    use tempfile::TempDir;

    fn mk_signal(step: LoopStep, kind: SignalKind, message: &str, ts: u64) -> Signal {
        Signal {
            step,
            kind,
            message: message.to_string(),
            metadata: serde_json::json!({"tool": "search_text"}),
            timestamp_ms: ts,
        }
    }

    #[test]
    fn creates_signals_directory() {
        let tmp = TempDir::new().expect("tempdir");
        let data_dir = tmp.path().join("nonexistent");
        let _store = SignalStore::new(&data_dir, "sess-1").expect("new");
        assert!(data_dir.join("signals").exists());
    }

    #[test]
    fn persist_writes_jsonl() {
        let tmp = TempDir::new().expect("tempdir");
        let store = SignalStore::new(tmp.path(), "sess-1").expect("new");

        let signals = vec![
            mk_signal(LoopStep::Act, SignalKind::Friction, "regex fail", 100),
            mk_signal(LoopStep::Act, SignalKind::Success, "grep ok", 200),
        ];
        store.persist(&signals).expect("persist");

        let path = tmp.path().join("signals/sess-1.jsonl");
        assert!(path.exists());

        let file = fs::File::open(&path).expect("open");
        let lines: Vec<String> = std::io::BufReader::new(file)
            .lines()
            .collect::<Result<_, _>>()
            .expect("read lines");
        assert_eq!(lines.len(), 2);

        let parsed: Signal = serde_json::from_str(&lines[0]).expect("parse");
        assert_eq!(parsed.message, "regex fail");
        assert_eq!(parsed.kind, SignalKind::Friction);
    }

    #[test]
    fn persist_appends_across_calls() {
        let tmp = TempDir::new().expect("tempdir");
        let store = SignalStore::new(tmp.path(), "sess-2").expect("new");

        store
            .persist(&[mk_signal(LoopStep::Act, SignalKind::Success, "first", 1)])
            .expect("persist 1");
        store
            .persist(&[mk_signal(LoopStep::Act, SignalKind::Friction, "second", 2)])
            .expect("persist 2");

        let path = tmp.path().join("signals/sess-2.jsonl");
        let file = fs::File::open(&path).expect("open");
        let lines: Vec<String> = std::io::BufReader::new(file)
            .lines()
            .collect::<Result<_, _>>()
            .expect("read lines");
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn persist_empty_is_noop() {
        let tmp = TempDir::new().expect("tempdir");
        let store = SignalStore::new(tmp.path(), "sess-3").expect("new");
        store.persist(&[]).expect("persist empty");

        let path = tmp.path().join("signals/sess-3.jsonl");
        assert!(!path.exists(), "no file created for empty signals");
    }

    #[test]
    fn persist_returns_error_when_dir_is_not_writable() {
        let tmp = TempDir::new().expect("tempdir");
        let store = SignalStore::new(tmp.path(), "err-test").expect("new");

        // Replace the signals dir with a regular file so open fails
        let signals_dir = tmp.path().join("signals");
        fs::remove_dir_all(&signals_dir).expect("remove dir");
        fs::write(&signals_dir, "not a directory").expect("create blocker file");

        let signals = vec![mk_signal(LoopStep::Act, SignalKind::Friction, "test", 100)];
        let result = store.persist(&signals);
        assert!(
            result.is_err(),
            "persist should fail when signals path is a file"
        );

        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("signal persist open"),
            "error should describe file open failure: {msg}"
        );
    }

    #[test]
    fn redact_strips_after_for_pattern() {
        let result = redact_signal_message("regex parse error for pattern 'secret'");
        assert_eq!(result, "regex parse error for [redacted]");
    }

    #[test]
    fn redact_preserves_clean_messages() {
        let result = redact_signal_message("tool search_text completed");
        assert_eq!(result, "tool search_text completed");
    }

    #[test]
    fn cleanup_removes_old_files() {
        let tmp = TempDir::new().expect("tempdir");
        let store = SignalStore::new(tmp.path(), "current").expect("new");

        // Create a fake old signal file
        let old_path = tmp.path().join("signals/old-session.jsonl");
        fs::write(&old_path, "{}").expect("write old");
        // Set modification time to 60 days ago
        let sixty_days_ago =
            std::time::SystemTime::now() - std::time::Duration::from_secs(60 * 24 * 60 * 60);
        filetime::set_file_mtime(
            &old_path,
            filetime::FileTime::from_system_time(sixty_days_ago),
        )
        .expect("set mtime");

        // Create a current file
        store
            .persist(&[mk_signal(LoopStep::Act, SignalKind::Success, "recent", 1)])
            .expect("persist");

        let removed = store.cleanup_old_signals().expect("cleanup");
        assert_eq!(removed, 1);
        assert!(!old_path.exists(), "old file should be removed");
        assert!(
            tmp.path().join("signals/current.jsonl").exists(),
            "current file should remain"
        );
    }

    #[test]
    fn persist_redacts_metadata_output_and_preserves_structure() {
        let tmp = TempDir::new().expect("tempdir");
        let store = SignalStore::new(tmp.path(), "metadata-redact-test").expect("new");

        let raw_output = "sensitive file contents";
        let mut signal = mk_signal(LoopStep::Act, SignalKind::Success, "tool search_text", 100);
        signal.metadata = serde_json::json!({
            "output": raw_output,
            "success": true,
        });

        store.persist(&[signal]).expect("persist");

        let path = tmp.path().join("signals/metadata-redact-test.jsonl");
        let content = fs::read_to_string(&path).expect("read");
        let line = content.lines().next().expect("line");
        let parsed: Signal = serde_json::from_str(line).expect("parse");

        let expected_output = format!("[redacted: {} bytes]", raw_output.len());
        assert_eq!(parsed.metadata["success"], serde_json::json!(true));
        assert_eq!(
            parsed.metadata["output"],
            serde_json::json!(expected_output)
        );
        assert!(
            !content.contains(raw_output),
            "raw metadata output should be redacted: {content}"
        );
    }

    #[test]
    fn persist_redacts_sensitive_messages() {
        let tmp = TempDir::new().expect("tempdir");
        let store = SignalStore::new(tmp.path(), "redact-test").expect("new");

        let signals = vec![mk_signal(
            LoopStep::Act,
            SignalKind::Friction,
            "regex parse error for pattern 'password.*'",
            100,
        )];
        store.persist(&signals).expect("persist");

        let path = tmp.path().join("signals/redact-test.jsonl");
        let content = fs::read_to_string(&path).expect("read");
        assert!(
            !content.contains("password"),
            "sensitive data should be redacted: {content}"
        );
        assert!(
            content.contains("[redacted]"),
            "should contain redaction marker: {content}"
        );
    }
}
