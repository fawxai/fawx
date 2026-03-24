//! JSONL-backed signal persistence.
//!
//! Writes signals from each loop cycle to a per-session JSONL file under
//! `~/.fawx/signals/`. One JSON object per line, append-only.

use fx_core::signals::{Signal, SignalKind};
use std::fmt;
use std::fs;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Errors that can occur during signal store operations.
#[derive(Debug)]
pub enum SignalStoreError {
    /// Failed to create or access the signals directory.
    DirectoryAccess(std::io::Error),
    /// Failed to open a signal file for writing.
    FileOpen(std::io::Error),
    /// Failed to write to a signal file.
    FileWrite(std::io::Error),
    /// Failed to read a signal file.
    FileRead(std::io::Error),
    /// Failed to serialize a signal to JSON.
    Serialize(serde_json::Error),
    /// Failed to deserialize a signal from JSON.
    Deserialize(serde_json::Error),
    /// Failed to read the signals directory for cleanup.
    DirectoryRead(std::io::Error),
    /// Session ID was invalid and could escape the signals directory.
    InvalidSessionId(String),
}

impl fmt::Display for SignalStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DirectoryAccess(e) => write!(f, "signal dir access: {e}"),
            Self::FileOpen(e) => write!(f, "signal persist open: {e}"),
            Self::FileWrite(e) => write!(f, "signal persist write: {e}"),
            Self::FileRead(e) => write!(f, "signal read: {e}"),
            Self::Serialize(e) => write!(f, "signal serialize: {e}"),
            Self::Deserialize(e) => write!(f, "signal deserialize: {e}"),
            Self::DirectoryRead(e) => write!(f, "read signals dir: {e}"),
            Self::InvalidSessionId(session_id) => {
                write!(f, "invalid session id: {session_id}")
            }
        }
    }
}

impl std::error::Error for SignalStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::DirectoryAccess(e)
            | Self::FileOpen(e)
            | Self::FileWrite(e)
            | Self::FileRead(e)
            | Self::DirectoryRead(e) => Some(e),
            Self::Serialize(e) | Self::Deserialize(e) => Some(e),
            Self::InvalidSessionId(_) => None,
        }
    }
}

/// Persists signals to JSONL files, one file per session.
#[derive(Debug)]
pub struct SignalStore {
    signals_dir: PathBuf,
    session_id: String,
    write_lock: Mutex<()>,
}

const RETENTION_DAYS: u64 = 30;
const RETENTION_MS: u64 = RETENTION_DAYS * 24 * 60 * 60 * 1000;

/// Filters used when reading signals from storage.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SignalQuery {
    pub kind: Option<SignalKind>,
}

impl SignalQuery {
    pub fn all() -> Self {
        Self { kind: None }
    }

    pub fn by_kind(kind: SignalKind) -> Self {
        Self { kind: Some(kind) }
    }
}

impl SignalStore {
    /// Create a new signal store. Creates the signals directory if needed.
    pub fn new(data_dir: &Path, session_id: &str) -> Result<Self, SignalStoreError> {
        validate_session_id(session_id)?;
        let signals_dir = data_dir.join("signals");
        fs::create_dir_all(&signals_dir).map_err(SignalStoreError::DirectoryAccess)?;
        Ok(Self {
            signals_dir,
            session_id: session_id.to_string(),
            write_lock: Mutex::new(()),
        })
    }

    /// List all persisted signals for this session.
    pub fn list(&self) -> Result<Vec<Signal>, SignalStoreError> {
        self.query(SignalQuery::all())
    }

    /// Query persisted signals for this session.
    pub fn query(&self, query: SignalQuery) -> Result<Vec<Signal>, SignalStoreError> {
        let path = self.session_path();
        let (signals, skipped) = read_signals_from_file(&path, query.kind)?;
        if skipped > 0 {
            tracing::warn!("Skipped {skipped} malformed lines in {}", path.display());
        }
        Ok(signals)
    }

    /// List all session IDs that have persisted signal files.
    pub fn list_all_sessions(&self) -> Result<Vec<String>, SignalStoreError> {
        let entries = fs::read_dir(&self.signals_dir).map_err(SignalStoreError::DirectoryRead)?;
        let mut sessions = Vec::new();

        for entry in entries {
            let path = entry.map_err(SignalStoreError::DirectoryRead)?.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            if let Some(session_id) = path.file_stem().and_then(|stem| stem.to_str()) {
                sessions.push(session_id.to_string());
            }
        }

        sessions.sort();
        Ok(sessions)
    }

    /// Load signals for a session, returning signals, display path, and skip count.
    fn load_session_with_skips(
        &self,
        session_id: &str,
    ) -> Result<(Vec<Signal>, String, usize), SignalStoreError> {
        let path = self.session_path_for(session_id)?;
        let (signals, skipped) = read_signals_from_file(&path, None)?;
        Ok((signals, path.display().to_string(), skipped))
    }

    /// Read all persisted signals for a specific session ID.
    pub fn load_session(&self, session_id: &str) -> Result<Vec<Signal>, SignalStoreError> {
        let (signals, filename, skipped) = self.load_session_with_skips(session_id)?;
        if skipped > 0 {
            tracing::warn!("Skipped {skipped} malformed lines in {filename}");
        }
        Ok(signals)
    }

    /// Read all persisted signals across every known session.
    pub fn load_all(&self) -> Result<Vec<(String, Signal)>, SignalStoreError> {
        let mut all_signals = Vec::new();
        let mut skip_counts: Vec<(String, usize)> = Vec::new();
        for session_id in self.list_all_sessions()? {
            let (signals, filename, skipped) = self.load_session_with_skips(&session_id)?;
            if skipped > 0 {
                skip_counts.push((filename, skipped));
            }
            all_signals.extend(
                signals
                    .into_iter()
                    .map(|signal| (session_id.clone(), signal)),
            );
        }
        for (filename, count) in &skip_counts {
            tracing::warn!("Skipped {count} malformed lines in {filename}");
        }
        Ok(all_signals)
    }

    fn session_path(&self) -> PathBuf {
        self.signals_dir.join(format!("{}.jsonl", self.session_id))
    }

    fn session_path_for(&self, session_id: &str) -> Result<PathBuf, SignalStoreError> {
        validate_session_id(session_id)?;
        Ok(self.signals_dir.join(format!("{session_id}.jsonl")))
    }

    /// Append signals from a loop cycle to the session's JSONL file.
    pub fn persist(&self, signals: &[Signal]) -> Result<(), SignalStoreError> {
        if signals.is_empty() {
            return Ok(());
        }

        let _guard = match self.write_lock.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                tracing::warn!("signal store write lock poisoned; continuing");
                poisoned.into_inner()
            }
        };

        let path = self.session_path();
        let file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(SignalStoreError::FileOpen)?;
        let mut writer = std::io::BufWriter::new(file);

        for signal in signals {
            let redacted = redact_signal_for_persist(signal);
            let json = serde_json::to_string(&redacted).map_err(SignalStoreError::Serialize)?;
            writeln!(writer, "{json}").map_err(SignalStoreError::FileWrite)?;
        }

        writer.flush().map_err(SignalStoreError::FileWrite)
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

fn validate_session_id(session_id: &str) -> Result<(), SignalStoreError> {
    if session_id.is_empty() {
        return Err(SignalStoreError::InvalidSessionId(session_id.to_string()));
    }

    let is_invalid =
        session_id.contains("/") || session_id.contains("\\") || session_id.contains("..");

    if is_invalid {
        Err(SignalStoreError::InvalidSessionId(session_id.to_string()))
    } else {
        Ok(())
    }
}

fn read_signals_from_file(
    path: &Path,
    kind_filter: Option<SignalKind>,
) -> Result<(Vec<Signal>, usize), SignalStoreError> {
    if !path.exists() {
        return Ok((Vec::new(), 0));
    }
    let file = fs::File::open(path).map_err(SignalStoreError::FileRead)?;
    parse_signal_lines(std::io::BufReader::new(file).lines(), kind_filter, path)
}

fn parse_signal_lines(
    lines: impl Iterator<Item = std::io::Result<String>>,
    kind_filter: Option<SignalKind>,
    source_path: &Path,
) -> Result<(Vec<Signal>, usize), SignalStoreError> {
    let mut signals = Vec::new();
    let mut skipped = 0usize;

    for (line_number, line) in lines.enumerate() {
        let line = line.map_err(SignalStoreError::FileRead)?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        match parse_signal_entries(trimmed) {
            Ok(parsed) => extend_matching_signals(&mut signals, parsed, kind_filter),
            Err(error) => {
                tracing::debug!(
                    "Skipping malformed signal line in {}:{}: {error}",
                    source_path.display(),
                    line_number + 1,
                );
                skipped += 1;
            }
        }
    }

    Ok((signals, skipped))
}

fn parse_signal_entries(line: &str) -> Result<Vec<Signal>, serde_json::Error> {
    match serde_json::from_str::<Signal>(line) {
        Ok(signal) => Ok(vec![signal]),
        Err(error) => recover_concatenated_signals(line).ok_or(error),
    }
}

fn recover_concatenated_signals(line: &str) -> Option<Vec<Signal>> {
    let fragments = split_concatenated_json(line)?;
    let mut signals = Vec::with_capacity(fragments.len());

    for fragment in fragments {
        let signal = serde_json::from_str::<Signal>(&fragment).ok()?;
        signals.push(signal);
    }

    Some(signals)
}

/// Split a line containing concatenated JSON objects like `{...}{...}{...}`.
///
/// This uses naive `}{` boundary detection, so it will not recover lines where
/// a JSON string value contains the literal `}{`. That limitation is acceptable
/// because failed recovery falls through to the existing malformed-line skip
/// path instead of producing a partial parse.
fn split_concatenated_json(line: &str) -> Option<Vec<String>> {
    let raw_fragments = line.split("}{").collect::<Vec<_>>();
    if raw_fragments.len() <= 1 {
        return None;
    }

    let last_index = raw_fragments.len() - 1;
    Some(
        raw_fragments
            .into_iter()
            .enumerate()
            .map(|(index, fragment)| normalize_signal_fragment(fragment, index, last_index))
            .collect(),
    )
}

fn normalize_signal_fragment(fragment: &str, index: usize, last_index: usize) -> String {
    let mut normalized = String::with_capacity(fragment.len() + 2);
    if index > 0 {
        normalized.push('{');
    }
    normalized.push_str(fragment);
    if index < last_index {
        normalized.push('}');
    }
    normalized
}

fn extend_matching_signals(
    signals: &mut Vec<Signal>,
    parsed: Vec<Signal>,
    kind_filter: Option<SignalKind>,
) {
    signals.extend(
        parsed
            .into_iter()
            .filter(|signal| matches_kind_filter(signal, kind_filter)),
    );
}

fn matches_kind_filter(signal: &Signal, kind_filter: Option<SignalKind>) -> bool {
    kind_filter.is_none_or(|kind| signal.kind == kind)
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
    use fx_core::signals::{LoopStep, SignalKind};
    use std::io::BufRead;
    use std::path::Path;
    use tempfile::TempDir;

    fn parse_test_path() -> &'static Path {
        Path::new("test-signals.jsonl")
    }

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

    fn persist_mixed_kind_signals(store: &SignalStore) {
        let signals = vec![
            mk_signal(LoopStep::Act, SignalKind::Friction, "slow response", 1),
            mk_signal(LoopStep::Act, SignalKind::Success, "tool ran", 2),
            mk_signal(LoopStep::Decide, SignalKind::Decision, "picked plan", 3),
        ];
        store.persist(&signals).expect("persist mixed kinds");
    }

    fn persist_single_signal(data_dir: &Path, session_id: &str, message: &str, ts: u64) {
        let store = SignalStore::new(data_dir, session_id).expect("new");
        let signal = mk_signal(LoopStep::Act, SignalKind::Friction, message, ts);
        store.persist(&[signal]).expect("persist");
    }

    #[test]
    fn list_all_sessions_returns_session_ids_from_signal_files() {
        let tmp = TempDir::new().expect("tempdir");
        persist_single_signal(tmp.path(), "session-a", "a", 1);
        persist_single_signal(tmp.path(), "session-b", "b", 2);
        fs::write(tmp.path().join("signals/ignore-me.txt"), "ignored").expect("write extra file");

        let store = SignalStore::new(tmp.path(), "session-a").expect("new");
        let sessions = store.list_all_sessions().expect("list all sessions");

        assert_eq!(
            sessions,
            vec!["session-a".to_string(), "session-b".to_string()]
        );
    }

    #[test]
    fn load_session_reads_signals_for_requested_session() {
        let tmp = TempDir::new().expect("tempdir");
        persist_single_signal(tmp.path(), "session-a", "from-a", 1);
        persist_single_signal(tmp.path(), "session-b", "from-b", 2);

        let store = SignalStore::new(tmp.path(), "session-a").expect("new");
        let signals = store.load_session("session-b").expect("load session");

        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0].message, "from-b");
    }

    #[test]
    fn load_session_returns_empty_for_nonexistent_session() {
        let tmp = TempDir::new().expect("tempdir");
        persist_single_signal(tmp.path(), "session-a", "from-a", 1);

        let store = SignalStore::new(tmp.path(), "session-a").expect("new");
        let signals = store
            .load_session("missing-session")
            .expect("missing session should be empty");

        assert!(signals.is_empty());
    }

    #[test]
    fn load_session_rejects_path_traversal_session_ids() {
        let tmp = TempDir::new().expect("tempdir");
        let store = SignalStore::new(tmp.path(), "session-a").expect("new");

        let error = store
            .load_session("../../etc/passwd")
            .expect_err("path traversal session id should fail");

        assert!(matches!(
            error,
            SignalStoreError::InvalidSessionId(ref id) if id == "../../etc/passwd"
        ));
    }

    #[test]
    fn new_rejects_path_traversal_session_id() {
        let tmp = TempDir::new().expect("tempdir");
        let error = SignalStore::new(tmp.path(), "../bad-session")
            .expect_err("constructor should reject traversal session id");

        assert!(matches!(
            error,
            SignalStoreError::InvalidSessionId(ref id) if id == "../bad-session"
        ));
    }

    #[test]
    fn validate_session_id_rejects_empty_string() {
        let error = validate_session_id("").expect_err("empty session id should fail");

        assert!(matches!(
            error,
            SignalStoreError::InvalidSessionId(ref id) if id.is_empty()
        ));
    }

    #[test]
    fn load_all_aggregates_signals_with_malformed_lines_across_sessions() {
        let tmp = TempDir::new().expect("tempdir");
        let store = SignalStore::new(tmp.path(), "session-a").expect("new");

        let valid_a =
            serde_json::to_string(&mk_signal(LoopStep::Act, SignalKind::Success, "good-a", 1))
                .expect("serialize signal");
        let valid_b =
            serde_json::to_string(&mk_signal(LoopStep::Act, SignalKind::Friction, "good-b", 2))
                .expect("serialize signal");

        // session-a: 1 good + 2 bad
        fs::write(
            tmp.path().join("signals/session-a.jsonl"),
            format!(
                "{valid_a}
not-json
{{
"
            ),
        )
        .expect("write a");

        // session-b: 1 good + 1 bad
        fs::write(
            tmp.path().join("signals/session-b.jsonl"),
            format!(
                "{valid_b}
broken
"
            ),
        )
        .expect("write b");

        let signals = store.load_all().expect("load all");
        assert_eq!(signals.len(), 2);
        assert!(signals
            .iter()
            .any(|(sid, s)| sid == "session-a" && s.message == "good-a"));
        assert!(signals
            .iter()
            .any(|(sid, s)| sid == "session-b" && s.message == "good-b"));
    }

    #[test]
    fn load_all_aggregates_signals_across_sessions_with_session_ids() {
        let tmp = TempDir::new().expect("tempdir");
        persist_single_signal(tmp.path(), "session-a", "first", 1);
        persist_single_signal(tmp.path(), "session-b", "second", 2);
        persist_single_signal(tmp.path(), "session-b", "third", 3);

        let store = SignalStore::new(tmp.path(), "session-a").expect("new");
        let signals = store.load_all().expect("load all");

        assert_eq!(signals.len(), 3);
        assert!(signals
            .iter()
            .any(|(session_id, signal)| session_id == "session-a" && signal.message == "first"));
        assert!(signals
            .iter()
            .any(|(session_id, signal)| session_id == "session-b" && signal.message == "second"));
        assert!(signals
            .iter()
            .any(|(session_id, signal)| session_id == "session-b" && signal.message == "third"));
    }

    #[test]
    fn query_without_kind_filter_returns_all_signals() {
        let tmp = TempDir::new().expect("tempdir");
        let store = SignalStore::new(tmp.path(), "sess-query-all").expect("new");
        persist_mixed_kind_signals(&store);

        let signals = store.list().expect("list");
        assert_eq!(signals.len(), 3);
    }

    #[test]
    fn list_and_query_return_empty_when_session_file_is_missing() {
        let tmp = TempDir::new().expect("tempdir");
        let store = SignalStore::new(tmp.path(), "sess-empty").expect("new");

        let session_path = tmp.path().join("signals/sess-empty.jsonl");
        assert!(
            !session_path.exists(),
            "test precondition: session file should not exist"
        );

        let listed = store.list().expect("list");
        let queried = store
            .query(SignalQuery::by_kind(SignalKind::Friction))
            .expect("query");
        assert!(listed.is_empty());
        assert!(queried.is_empty());
    }

    #[test]
    fn query_with_kind_filter_returns_only_matching_signals() {
        let tmp = TempDir::new().expect("tempdir");
        let store = SignalStore::new(tmp.path(), "sess-query-kind").expect("new");
        persist_mixed_kind_signals(&store);

        let filtered = store
            .query(SignalQuery::by_kind(SignalKind::Friction))
            .expect("query");
        assert_eq!(filtered.len(), 1);
        assert!(filtered
            .iter()
            .all(|signal| signal.kind == SignalKind::Friction));
        assert_eq!(filtered[0].message, "slow response");
    }

    #[test]
    fn parse_signal_lines_skips_empty_lines() {
        let valid =
            serde_json::to_string(&mk_signal(LoopStep::Act, SignalKind::Success, "kept", 1))
                .expect("serialize signal");
        let lines = vec![
            Ok(String::new()),
            Ok("   ".to_string()),
            Ok(valid),
            Ok("\t".to_string()),
        ];

        let (parsed, skipped) =
            parse_signal_lines(lines.into_iter(), None, parse_test_path()).expect("parse");

        assert_eq!(skipped, 0);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].message, "kept");
    }

    #[test]
    fn parse_signal_lines_skips_comment_lines() {
        let valid =
            serde_json::to_string(&mk_signal(LoopStep::Act, SignalKind::Friction, "kept", 1))
                .expect("serialize signal");
        let lines = vec![
            Ok("# filtered: by policy".to_string()),
            Ok("   # filtered: duplicate".to_string()),
            Ok(valid),
        ];

        let (parsed, skipped) =
            parse_signal_lines(lines.into_iter(), None, parse_test_path()).expect("parse");

        assert_eq!(skipped, 0);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].message, "kept");
    }

    #[test]
    fn parse_signal_lines_skips_malformed_json_without_error() {
        let lines = vec![
            Ok("{\"kind\":\"friction\",\"broken\": ".to_string()),
            Ok("{\"still\":\"not-a-signal\"}".to_string()),
        ];

        let (parsed, skipped) = parse_signal_lines(lines.into_iter(), None, parse_test_path())
            .expect("malformed lines should be skipped");

        assert_eq!(skipped, 2);
        assert!(parsed.is_empty());
    }

    #[test]
    fn parse_signal_lines_recovers_concatenated_json_objects() {
        let first =
            serde_json::to_string(&mk_signal(LoopStep::Act, SignalKind::Success, "first", 1))
                .expect("serialize signal");
        let second = serde_json::to_string(&mk_signal(
            LoopStep::Decide,
            SignalKind::Decision,
            "second",
            2,
        ))
        .expect("serialize signal");
        let lines = vec![Ok(format!("{first}{second}"))];

        let (parsed, skipped) =
            parse_signal_lines(lines.into_iter(), None, parse_test_path()).expect("parse");

        assert_eq!(skipped, 0);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].message, "first");
        assert_eq!(parsed[1].message, "second");
    }

    #[test]
    fn parse_signal_lines_recovers_three_concatenated_json_objects() {
        let first =
            serde_json::to_string(&mk_signal(LoopStep::Act, SignalKind::Success, "first", 1))
                .expect("serialize signal");
        let second = serde_json::to_string(&mk_signal(
            LoopStep::Decide,
            SignalKind::Decision,
            "second",
            2,
        ))
        .expect("serialize signal");
        let third =
            serde_json::to_string(&mk_signal(LoopStep::Act, SignalKind::Friction, "third", 3))
                .expect("serialize signal");
        let lines = vec![Ok(format!("{first}{second}{third}"))];

        let (parsed, skipped) =
            parse_signal_lines(lines.into_iter(), None, parse_test_path()).expect("parse");

        assert_eq!(skipped, 0);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].message, "first");
        assert_eq!(parsed[1].message, "second");
        assert_eq!(parsed[2].message, "third");
    }

    #[test]
    fn parse_signal_lines_skips_unrecoverable_concatenated_json_objects() {
        let valid =
            serde_json::to_string(&mk_signal(LoopStep::Act, SignalKind::Success, "kept", 1))
                .expect("serialize signal");
        let lines = vec![Ok(format!("{valid}{{\"broken\":")), Ok(valid.clone())];

        let (parsed, skipped) =
            parse_signal_lines(lines.into_iter(), None, parse_test_path()).expect("parse");

        assert_eq!(skipped, 1);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].message, "kept");
    }

    #[test]
    fn parse_signal_lines_returns_skip_count_for_malformed_lines() {
        let good = serde_json::to_string(&mk_signal(LoopStep::Act, SignalKind::Success, "ok", 1))
            .expect("serialize");
        let lines = vec![
            Ok(good),
            Ok("{bad json 1}".to_string()),
            Ok("{bad json 2}".to_string()),
            Ok("{bad json 3}".to_string()),
        ];

        let (signals, skipped) =
            parse_signal_lines(lines.into_iter(), None, parse_test_path()).expect("parse");

        assert_eq!(signals.len(), 1);
        assert_eq!(skipped, 3);
    }

    #[test]
    fn read_signals_from_file_returns_skip_count_for_malformed_file() {
        let tmp = TempDir::new().expect("tempdir");
        fs::create_dir_all(tmp.path().join("signals")).expect("create signals dir");
        let session_path = tmp.path().join("signals/session-a.jsonl");
        let valid =
            serde_json::to_string(&mk_signal(LoopStep::Act, SignalKind::Success, "kept", 1))
                .expect("serialize signal");
        fs::write(&session_path, format!("{valid}\nnot-json\n{{\n")).expect("write session file");

        let (signals, skipped) = read_signals_from_file(&session_path, None).expect("read signals");

        assert_eq!(skipped, 2);
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0].message, "kept");
    }

    #[test]
    fn load_session_keeps_valid_signals_when_file_has_malformed_lines() {
        let tmp = TempDir::new().expect("tempdir");
        let store = SignalStore::new(tmp.path(), "session-a").expect("new");
        let session_path = tmp.path().join("signals/session-a.jsonl");
        let valid =
            serde_json::to_string(&mk_signal(LoopStep::Act, SignalKind::Success, "kept", 1))
                .expect("serialize signal");
        fs::write(&session_path, format!("{valid}\nnot-json\n{{\n")).expect("write session file");

        let loaded = store.load_session("session-a").expect("load session");

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].message, "kept");
    }

    #[test]
    fn persist_writes_snake_case_kind_labels() {
        let tmp = TempDir::new().expect("tempdir");
        let store = SignalStore::new(tmp.path(), "sess-kind-label").expect("new");
        store
            .persist(&[mk_signal(LoopStep::Act, SignalKind::Friction, "msg", 10)])
            .expect("persist");

        let content =
            fs::read_to_string(tmp.path().join("signals/sess-kind-label.jsonl")).expect("read");
        let parsed: serde_json::Value =
            serde_json::from_str(content.lines().next().expect("line")).expect("parse value");
        assert_eq!(parsed["kind"], serde_json::json!("friction"));
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
