//! Session registry: tracks all active sessions and delegates persistence.

use crate::session::{
    Session, SessionContentBlock, SessionHistoryError, SessionMemory, SessionMessage,
};
use crate::store::SessionStore;
use crate::types::{
    MessageRole, SessionArchiveFilter, SessionConfig, SessionInfo, SessionKey, SessionKind,
    SessionStatus,
};
use fx_core::error::StorageError;
use fx_storage::Storage;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

type Result<T> = std::result::Result<T, SessionError>;

/// Errors specific to session registry operations.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// Session not found by key.
    #[error("session not found: {0}")]
    NotFound(String),

    /// Session already exists with the given key.
    #[error("session already exists: {0}")]
    AlreadyExists(String),

    /// Persistence or storage failure.
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),

    /// Session history violated a causal ordering invariant.
    #[error("invalid session history: {0}")]
    InvalidHistory(#[from] SessionHistoryError),

    /// Persisted session history is corrupted and cannot be replayed safely.
    #[error("corrupted session '{key}': {source}")]
    Corrupted {
        key: SessionKey,
        #[source]
        source: SessionHistoryError,
    },

    /// Internal lock poisoning.
    #[error("internal error: lock poisoned")]
    LockPoisoned,
}

#[derive(Debug, Clone)]
struct CorruptedSession {
    info: SessionInfo,
    source: SessionHistoryError,
}

impl CorruptedSession {
    fn from_session(session: Session, source: SessionHistoryError) -> Self {
        Self {
            info: session.info(),
            source,
        }
    }

    fn matches_filters(
        &self,
        kind_filter: Option<SessionKind>,
        archive_filter: SessionArchiveFilter,
    ) -> bool {
        kind_filter.is_none_or(|kind| self.info.kind == kind)
            && archive_filter.matches(self.info.is_archived())
    }

    fn to_error(&self, key: &SessionKey) -> SessionError {
        SessionError::Corrupted {
            key: key.clone(),
            source: self.source.clone(),
        }
    }
}

#[derive(Default)]
struct HydratedSessions {
    healthy: HashMap<SessionKey, Session>,
    corrupted: HashMap<SessionKey, CorruptedSession>,
}

#[derive(Clone, Copy)]
enum ArchiveOperation {
    Archive,
    Unarchive,
}

impl ArchiveOperation {
    fn apply(self, session: &mut Session) {
        match self {
            Self::Archive => {
                if !session.is_archived() {
                    session.archived_at = Some(current_epoch_secs());
                }
            }
            Self::Unarchive => {
                session.archived_at = None;
            }
        }
    }
}

/// Manages all active sessions, backed by persistent storage.
///
/// The in-memory session map is protected by an `RwLock`, while the
/// store (redb) lives outside the lock so disk I/O never blocks
/// other tasks waiting for the lock.
///
/// Cloneable and thread-safe via `Arc`.
#[derive(Clone)]
pub struct SessionRegistry {
    sessions: Arc<RwLock<HashMap<SessionKey, Session>>>,
    corrupted_sessions: Arc<RwLock<HashMap<SessionKey, CorruptedSession>>>,
    store: SessionStore,
}

impl std::fmt::Debug for SessionRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionRegistry").finish_non_exhaustive()
    }
}

impl SessionRegistry {
    /// Create a registry backed by the given store, loading any
    /// previously persisted sessions.
    pub fn new(store: SessionStore) -> Result<Self> {
        let hydrated = hydrate_sessions(store.load_all()?);
        Ok(Self {
            sessions: Arc::new(RwLock::new(hydrated.healthy)),
            corrupted_sessions: Arc::new(RwLock::new(hydrated.corrupted)),
            store,
        })
    }

    /// Open a registry from the redb database at `path`.
    pub fn open(path: &Path) -> Option<Self> {
        let storage = match Storage::open(path) {
            Ok(storage) => storage,
            Err(error) => {
                tracing::warn!(path = %path.display(), error = %error, "session storage unavailable");
                return None;
            }
        };

        match Self::new(SessionStore::new(storage)) {
            Ok(registry) => Some(registry),
            Err(error) => {
                tracing::warn!(path = %path.display(), error = %error, "session registry unavailable");
                None
            }
        }
    }

    /// List sessions, optionally filtered by kind.
    pub fn list(&self, filter: Option<SessionKind>) -> Result<Vec<SessionInfo>> {
        self.list_with_archive_filter(filter, SessionArchiveFilter::default())
    }

    /// List sessions with explicit archive-state filtering.
    pub fn list_with_archive_filter(
        &self,
        kind_filter: Option<SessionKind>,
        archive_filter: SessionArchiveFilter,
    ) -> Result<Vec<SessionInfo>> {
        let map = self.read()?;
        let corrupted = self.read_corrupted()?;
        let healthy_infos = map
            .values()
            .filter(|session| {
                kind_filter.is_none_or(|kind| session.kind == kind)
                    && archive_filter.matches(session.is_archived())
            })
            .map(Session::info);
        let corrupted_infos = corrupted
            .values()
            .filter(|session| session.matches_filters(kind_filter, archive_filter))
            .map(|session| session.info.clone());
        Ok(healthy_infos.chain(corrupted_infos).collect())
    }

    /// Create a new session. Returns its key.
    ///
    /// Checks uniqueness in-memory first, inserts into the map, then
    /// persists to the store. If persistence fails, the map insertion
    /// is rolled back to keep memory and disk consistent.
    pub fn create(
        &self,
        key: SessionKey,
        kind: SessionKind,
        config: SessionConfig,
    ) -> Result<SessionKey> {
        if self.corrupted_entry(&key)?.is_some() {
            return Err(SessionError::AlreadyExists(key.as_str().to_string()));
        }
        let session = Session::new(key.clone(), kind, config);
        let mut map = self.write()?;
        if map.contains_key(&key) {
            return Err(SessionError::AlreadyExists(key.as_str().to_string()));
        }
        map.insert(key.clone(), session.clone());
        if let Err(e) = self.store.save(&session) {
            map.remove(&key);
            return Err(SessionError::Storage(e));
        }
        Ok(key)
    }

    /// Destroy a session by key.
    pub fn destroy(&self, key: &SessionKey) -> Result<()> {
        let removed_healthy = {
            let mut map = self.write()?;
            map.remove(key)
        };
        let removed_corrupted = {
            let mut map = self.write_corrupted()?;
            map.remove(key)
        };
        if removed_healthy.is_none() && removed_corrupted.is_none() {
            return Err(SessionError::NotFound(key.as_str().to_string()));
        }
        self.store.delete(key)?;
        Ok(())
    }

    /// Record a user message in a session.
    ///
    /// Returns an acknowledgement string. Note: the message is only
    /// recorded in the session history — it is not dispatched to any
    /// model for processing.
    pub fn send(&self, key: &SessionKey, message: &str) -> Result<String> {
        self.record_message(key, MessageRole::User, message)?;
        Ok(format!("message recorded in session {}", key))
    }

    /// Record a message with an explicit role in a session.
    pub fn record_message(&self, key: &SessionKey, role: MessageRole, message: &str) -> Result<()> {
        self.record_message_blocks(
            key,
            role,
            vec![SessionContentBlock::Text {
                text: message.to_string(),
            }],
            None,
        )
    }

    /// Record a structured message with an explicit role in a session.
    pub fn record_message_blocks(
        &self,
        key: &SessionKey,
        role: MessageRole,
        content: Vec<SessionContentBlock>,
        token_count: Option<u32>,
    ) -> Result<()> {
        self.update_session(key, move |session| {
            session.add_message_blocks(role, content, token_count)?;
            Ok(())
        })
    }

    /// Append multiple pre-built session messages in a single save.
    pub fn append_messages(&self, key: &SessionKey, messages: Vec<SessionMessage>) -> Result<()> {
        if messages.is_empty() {
            return Ok(());
        }

        self.update_session(key, move |session| {
            session.extend_messages(messages)?;
            Ok(())
        })
    }

    /// Read the persistent memory for a session.
    pub fn memory(&self, key: &SessionKey) -> Result<SessionMemory> {
        self.fail_if_corrupted(key)?;
        let map = self.read()?;
        let session = map
            .get(key)
            .ok_or_else(|| SessionError::NotFound(key.as_str().to_string()))?;
        Ok(session.memory.clone())
    }

    /// Persist the latest turn messages and session memory together.
    pub fn record_turn(
        &self,
        key: &SessionKey,
        messages: Vec<SessionMessage>,
        memory: SessionMemory,
    ) -> Result<()> {
        self.update_session(key, move |session| {
            if !messages.is_empty() {
                session.extend_messages(messages)?;
            }
            session.set_memory(memory);
            Ok(())
        })
    }

    /// Retrieve conversation history for a session (most recent `limit`).
    pub fn history(&self, key: &SessionKey, limit: usize) -> Result<Vec<SessionMessage>> {
        self.fail_if_corrupted(key)?;
        let map = self.read()?;
        let session = map
            .get(key)
            .ok_or_else(|| SessionError::NotFound(key.as_str().to_string()))?;
        Ok(session.recent_messages(limit).to_vec())
    }

    /// Clear the recorded message history for a session.
    pub fn clear(&self, key: &SessionKey) -> Result<()> {
        self.update_session(key, |session| {
            session.clear_messages();
            Ok(())
        })
    }

    /// Update the status of a session.
    pub fn set_status(&self, key: &SessionKey, status: SessionStatus) -> Result<()> {
        self.update_session(key, |session| {
            session.status = status;
            Ok(())
        })
    }

    /// Archive a session without deleting or clearing its history.
    pub fn archive(&self, key: &SessionKey) -> Result<()> {
        self.update_archive_state(key, ArchiveOperation::Archive)
    }

    /// Unarchive a session and restore it to the active listing.
    pub fn unarchive(&self, key: &SessionKey) -> Result<()> {
        self.update_archive_state(key, ArchiveOperation::Unarchive)
    }

    /// Get a snapshot of a single session's info.
    pub fn get_info(&self, key: &SessionKey) -> Result<SessionInfo> {
        self.fail_if_corrupted(key)?;
        let map = self.read()?;
        let session = map
            .get(key)
            .ok_or_else(|| SessionError::NotFound(key.as_str().to_string()))?;
        Ok(session.info())
    }

    fn read(&self) -> Result<std::sync::RwLockReadGuard<'_, HashMap<SessionKey, Session>>> {
        self.sessions.read().map_err(|_| SessionError::LockPoisoned)
    }

    fn write(&self) -> Result<std::sync::RwLockWriteGuard<'_, HashMap<SessionKey, Session>>> {
        self.sessions
            .write()
            .map_err(|_| SessionError::LockPoisoned)
    }

    fn read_corrupted(
        &self,
    ) -> Result<std::sync::RwLockReadGuard<'_, HashMap<SessionKey, CorruptedSession>>> {
        self.corrupted_sessions
            .read()
            .map_err(|_| SessionError::LockPoisoned)
    }

    fn write_corrupted(
        &self,
    ) -> Result<std::sync::RwLockWriteGuard<'_, HashMap<SessionKey, CorruptedSession>>> {
        self.corrupted_sessions
            .write()
            .map_err(|_| SessionError::LockPoisoned)
    }

    fn corrupted_entry(&self, key: &SessionKey) -> Result<Option<CorruptedSession>> {
        Ok(self.read_corrupted()?.get(key).cloned())
    }

    fn fail_if_corrupted(&self, key: &SessionKey) -> Result<()> {
        if let Some(session) = self.corrupted_entry(key)? {
            return Err(session.to_error(key));
        }
        Ok(())
    }

    fn update_session(
        &self,
        key: &SessionKey,
        update: impl FnOnce(&mut Session) -> Result<()>,
    ) -> Result<()> {
        self.fail_if_corrupted(key)?;
        let snapshot = {
            let mut map = self.write()?;
            let session = map
                .get_mut(key)
                .ok_or_else(|| SessionError::NotFound(key.as_str().to_string()))?;
            update(session)?;
            session.clone()
        };
        self.store.save(&snapshot)?;
        Ok(())
    }

    fn update_archive_state(&self, key: &SessionKey, operation: ArchiveOperation) -> Result<()> {
        self.update_session(key, |session| {
            operation.apply(session);
            Ok(())
        })
    }
}

fn hydrate_sessions(persisted: Vec<Session>) -> HydratedSessions {
    let mut hydrated = HydratedSessions {
        healthy: HashMap::with_capacity(persisted.len()),
        corrupted: HashMap::new(),
    };

    for session in persisted {
        match session.validate_history() {
            Ok(()) => {
                hydrated.healthy.insert(session.key.clone(), session);
            }
            Err(source) => record_corrupted_session(&mut hydrated, session, source),
        }
    }

    hydrated
}

fn record_corrupted_session(
    hydrated: &mut HydratedSessions,
    session: Session,
    source: SessionHistoryError,
) {
    let key = session.key.clone();
    tracing::error!(session_key = %key, error = %source, "corrupted session history loaded from storage");
    hydrated
        .corrupted
        .insert(key, CorruptedSession::from_session(session, source));
}

fn current_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_llm::ContentBlock;
    use fx_storage::Storage;
    use std::thread;
    use std::time::Duration;

    fn test_registry() -> SessionRegistry {
        let storage = Storage::open_in_memory().expect("in-memory storage");
        let store = SessionStore::new(storage);
        SessionRegistry::new(store).expect("registry")
    }

    fn default_config() -> SessionConfig {
        SessionConfig {
            label: Some("test".to_string()),
            model: "gpt-4".to_string(),
        }
    }

    fn poisoned_session(id: &str) -> Session {
        Session {
            key: SessionKey::new(id).expect("session key"),
            kind: SessionKind::Main,
            status: SessionStatus::Idle,
            label: Some("poisoned".to_string()),
            model: "gpt-4".to_string(),
            created_at: 1,
            updated_at: 2,
            archived_at: None,
            messages: vec![
                SessionMessage::structured(
                    MessageRole::Tool,
                    vec![SessionContentBlock::ToolResult {
                        tool_use_id: "call_bad".to_string(),
                        content: serde_json::json!("bad"),
                        is_error: Some(false),
                    }],
                    1,
                    None,
                ),
                SessionMessage::structured(
                    MessageRole::Assistant,
                    vec![SessionContentBlock::ToolUse {
                        id: "call_bad".to_string(),
                        provider_id: Some("fc_bad".to_string()),
                        name: "read_file".to_string(),
                        input: serde_json::json!({"path": "bad.txt"}),
                    }],
                    2,
                    None,
                ),
            ],
            memory: SessionMemory::default(),
        }
    }

    #[test]
    fn create_and_list_sessions() {
        let reg = test_registry();
        reg.create(
            SessionKey::new("a").unwrap(),
            SessionKind::Main,
            default_config(),
        )
        .expect("create a");
        reg.create(
            SessionKey::new("b").unwrap(),
            SessionKind::Subagent,
            default_config(),
        )
        .expect("create b");

        let all = reg.list(None).expect("list all");
        assert_eq!(all.len(), 2);

        let mains = reg.list(Some(SessionKind::Main)).expect("list mains");
        assert_eq!(mains.len(), 1);
        assert_eq!(mains[0].key, SessionKey::new("a").unwrap());
    }

    #[test]
    fn archive_marks_session_archived_and_preserves_messages() {
        let storage = Storage::open_in_memory().expect("in-memory storage");
        let store = SessionStore::new(storage.clone());
        let reg = SessionRegistry::new(store).expect("registry");
        let key = SessionKey::new("archive-preserves").unwrap();
        reg.create(key.clone(), SessionKind::Main, default_config())
            .expect("create");
        reg.record_message(&key, MessageRole::User, "hello")
            .expect("record user");
        reg.record_message(&key, MessageRole::Assistant, "world")
            .expect("record assistant");
        reg.set_status(&key, SessionStatus::Paused)
            .expect("pause session");

        reg.archive(&key).expect("archive");

        let reopened = SessionRegistry::new(SessionStore::new(storage)).expect("reopen registry");
        let info = reopened.get_info(&key).expect("get archived info");
        let history = reopened.history(&key, 10).expect("get archived history");

        assert!(info.archived_at.is_some());
        assert_eq!(info.status, SessionStatus::Paused);
        assert_eq!(info.label.as_deref(), Some("test"));
        assert_eq!(info.model, "gpt-4");
        assert_eq!(info.message_count, 2);
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].render_text(), "hello");
        assert_eq!(history[1].render_text(), "world");
    }

    #[test]
    fn archive_is_idempotent_for_already_archived_session() {
        let reg = test_registry();
        let key = SessionKey::new("archive-idempotent").unwrap();
        reg.create(key.clone(), SessionKind::Main, default_config())
            .expect("create");
        reg.record_message(&key, MessageRole::User, "still here")
            .expect("record user");

        reg.archive(&key).expect("first archive");
        let first_info = reg.get_info(&key).expect("first info");
        thread::sleep(Duration::from_millis(1100));

        reg.archive(&key).expect("second archive");
        let second_info = reg.get_info(&key).expect("second info");
        let history = reg.history(&key, 10).expect("history");

        assert_eq!(second_info.archived_at, first_info.archived_at);
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].render_text(), "still here");
    }

    #[test]
    fn unarchive_restores_active_state() {
        let storage = Storage::open_in_memory().expect("in-memory storage");
        let store = SessionStore::new(storage.clone());
        let reg = SessionRegistry::new(store).expect("registry");
        let key = SessionKey::new("unarchive-restores").unwrap();
        reg.create(key.clone(), SessionKind::Main, default_config())
            .expect("create");
        reg.record_message(&key, MessageRole::User, "persisted")
            .expect("record user");
        reg.archive(&key).expect("archive");
        reg.unarchive(&key).expect("unarchive");

        let reopened = SessionRegistry::new(SessionStore::new(storage)).expect("reopen registry");
        let info = reopened.get_info(&key).expect("get unarchived info");
        let listed = reopened.list(None).expect("list active sessions");
        let history = reopened.history(&key, 10).expect("history");

        assert!(info.archived_at.is_none());
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].render_text(), "persisted");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].key, key);
    }

    #[test]
    fn unarchive_is_idempotent_for_active_session() {
        let reg = test_registry();
        let key = SessionKey::new("unarchive-idempotent").unwrap();
        reg.create(key.clone(), SessionKind::Main, default_config())
            .expect("create");
        reg.record_message(&key, MessageRole::User, "active")
            .expect("record user");

        reg.unarchive(&key).expect("unarchive active session");

        let info = reg.get_info(&key).expect("get active info");
        let history = reg.history(&key, 10).expect("history");

        assert!(info.archived_at.is_none());
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].render_text(), "active");
    }

    #[test]
    fn default_list_excludes_archived_sessions() {
        let reg = test_registry();
        let active_key = SessionKey::new("active-default").unwrap();
        let archived_key = SessionKey::new("archived-default").unwrap();
        reg.create(active_key.clone(), SessionKind::Main, default_config())
            .expect("create active");
        reg.create(archived_key.clone(), SessionKind::Main, default_config())
            .expect("create archived");
        reg.archive(&archived_key).expect("archive");

        let listed = reg.list(None).expect("default list");

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].key, active_key);
    }

    #[test]
    fn archived_only_filter_returns_archived_sessions_only() {
        let reg = test_registry();
        let active_key = SessionKey::new("active-filter").unwrap();
        let archived_key = SessionKey::new("archived-filter").unwrap();
        reg.create(active_key.clone(), SessionKind::Main, default_config())
            .expect("create active");
        reg.create(
            archived_key.clone(),
            SessionKind::Subagent,
            default_config(),
        )
        .expect("create archived");
        reg.archive(&archived_key).expect("archive");

        let listed = reg
            .list_with_archive_filter(None, SessionArchiveFilter::ArchivedOnly)
            .expect("archived-only list");

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].key, archived_key);
        assert!(listed[0].is_archived());
    }

    #[test]
    fn all_filter_includes_active_and_archived_sessions() {
        let reg = test_registry();
        let active_key = SessionKey::new("active-all").unwrap();
        let archived_key = SessionKey::new("archived-all").unwrap();
        reg.create(active_key.clone(), SessionKind::Main, default_config())
            .expect("create active");
        reg.create(archived_key.clone(), SessionKind::Main, default_config())
            .expect("create archived");
        reg.archive(&archived_key).expect("archive");

        let mut listed = reg
            .list_with_archive_filter(None, SessionArchiveFilter::All)
            .expect("all list");
        listed.sort_by(|left, right| left.key.as_str().cmp(right.key.as_str()));

        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].key, active_key);
        assert_eq!(listed[1].key, archived_key);
        assert!(listed[1].is_archived());
    }

    #[test]
    fn direct_lookup_returns_archived_sessions_by_key() {
        let reg = test_registry();
        let key = SessionKey::new("archived-lookup").unwrap();
        reg.create(key.clone(), SessionKind::Main, default_config())
            .expect("create");
        reg.record_message(&key, MessageRole::User, "lookup me")
            .expect("record user");
        reg.archive(&key).expect("archive");

        let info = reg.get_info(&key).expect("get archived info");
        let history = reg.history(&key, 10).expect("get archived history");

        assert!(info.is_archived());
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].render_text(), "lookup me");
    }

    #[test]
    fn archive_missing_session_returns_not_found() {
        let reg = test_registry();
        let error = reg
            .archive(&SessionKey::new("missing-archive").unwrap())
            .expect_err("archive should fail");
        assert!(matches!(error, SessionError::NotFound(_)));
    }

    #[test]
    fn unarchive_missing_session_returns_not_found() {
        let reg = test_registry();
        let error = reg
            .unarchive(&SessionKey::new("missing-unarchive").unwrap())
            .expect_err("unarchive should fail");
        assert!(matches!(error, SessionError::NotFound(_)));
    }

    #[test]
    fn create_duplicate_key_fails() {
        let reg = test_registry();
        reg.create(
            SessionKey::new("dup").unwrap(),
            SessionKind::Main,
            default_config(),
        )
        .expect("first create");
        let err = reg
            .create(
                SessionKey::new("dup").unwrap(),
                SessionKind::Main,
                default_config(),
            )
            .expect_err("duplicate should fail");
        assert!(matches!(err, SessionError::AlreadyExists(_)));
    }

    #[test]
    fn destroy_removes_session() {
        let reg = test_registry();
        reg.create(
            SessionKey::new("del").unwrap(),
            SessionKind::Main,
            default_config(),
        )
        .expect("create");
        reg.destroy(&SessionKey::new("del").unwrap())
            .expect("destroy");

        let all = reg.list(None).expect("list");
        assert!(all.is_empty());
    }

    #[test]
    fn destroy_nonexistent_returns_not_found() {
        let reg = test_registry();
        let err = reg
            .destroy(&SessionKey::new("nope").unwrap())
            .expect_err("should fail");
        assert!(matches!(err, SessionError::NotFound(_)));
    }

    #[test]
    fn send_records_message_in_session() {
        let reg = test_registry();
        reg.create(
            SessionKey::new("chat").unwrap(),
            SessionKind::Main,
            default_config(),
        )
        .expect("create");

        let ack = reg
            .send(&SessionKey::new("chat").unwrap(), "hello")
            .expect("send");
        assert!(ack.contains("chat"));

        let history = reg
            .history(&SessionKey::new("chat").unwrap(), 10)
            .expect("history");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].render_text(), "hello");
    }

    #[test]
    fn send_to_nonexistent_session_fails() {
        let reg = test_registry();
        let err = reg
            .send(&SessionKey::new("missing").unwrap(), "hello")
            .expect_err("should fail");
        assert!(matches!(err, SessionError::NotFound(_)));
    }

    #[test]
    fn history_nonexistent_session_fails() {
        let reg = test_registry();
        let err = reg
            .history(&SessionKey::new("missing").unwrap(), 10)
            .expect_err("should fail");
        assert!(matches!(err, SessionError::NotFound(_)));
    }

    #[test]
    fn history_respects_limit() {
        let reg = test_registry();
        reg.create(
            SessionKey::new("lim").unwrap(),
            SessionKind::Main,
            default_config(),
        )
        .expect("create");
        for i in 0..5 {
            reg.send(&SessionKey::new("lim").unwrap(), &format!("msg-{i}"))
                .expect("send");
        }

        let recent = reg
            .history(&SessionKey::new("lim").unwrap(), 2)
            .expect("history");
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].render_text(), "msg-3");
        assert_eq!(recent[1].render_text(), "msg-4");
    }

    #[test]
    fn set_status_updates_session() {
        let reg = test_registry();
        reg.create(
            SessionKey::new("st").unwrap(),
            SessionKind::Main,
            default_config(),
        )
        .expect("create");

        reg.set_status(&SessionKey::new("st").unwrap(), SessionStatus::Paused)
            .expect("set_status");

        let info = reg
            .get_info(&SessionKey::new("st").unwrap())
            .expect("get_info");
        assert_eq!(info.status, SessionStatus::Paused);
    }

    #[test]
    fn get_info_nonexistent_fails() {
        let reg = test_registry();
        let err = reg
            .get_info(&SessionKey::new("nope").unwrap())
            .expect_err("should fail");
        assert!(matches!(err, SessionError::NotFound(_)));
    }

    #[test]
    fn memory_returns_default_for_new_session() {
        let reg = test_registry();
        let key = SessionKey::new("memory").unwrap();
        reg.create(key.clone(), SessionKind::Main, default_config())
            .expect("create");

        let memory = reg.memory(&key).expect("memory");

        assert!(memory.is_empty());
    }

    #[test]
    fn record_turn_persists_messages_and_memory_together() {
        let reg = test_registry();
        let key = SessionKey::new("turn").unwrap();
        reg.create(key.clone(), SessionKind::Main, default_config())
            .expect("create");

        let messages = vec![SessionMessage::text(MessageRole::Assistant, "saved", 7)];
        let mut memory = SessionMemory::default();
        memory.project = Some("session memory".to_string());
        memory.current_state = Some("testing".to_string());
        reg.record_turn(&key, messages, memory.clone())
            .expect("record turn");

        let history = reg.history(&key, 10).expect("history");
        let stored_memory = reg.memory(&key).expect("memory");

        assert_eq!(history.len(), 1);
        assert_eq!(history[0].render_text(), "saved");
        assert_eq!(stored_memory, memory);
    }

    #[test]
    fn session_persists_turn_scoped_grouped_tool_history() {
        let reg = test_registry();
        let key = SessionKey::new("tool-order").unwrap();
        reg.create(key.clone(), SessionKind::Main, default_config())
            .expect("create");

        reg.record_turn(
            &key,
            vec![
                SessionMessage::structured(
                    MessageRole::Assistant,
                    vec![
                        SessionContentBlock::ToolUse {
                            id: "call_1".to_string(),
                            provider_id: Some("fc_1".to_string()),
                            name: "read_file".to_string(),
                            input: serde_json::json!({"path": "README.md"}),
                        },
                        SessionContentBlock::ToolUse {
                            id: "call_2".to_string(),
                            provider_id: Some("fc_2".to_string()),
                            name: "list_dir".to_string(),
                            input: serde_json::json!({"path": "."}),
                        },
                    ],
                    1,
                    None,
                ),
                SessionMessage::structured(
                    MessageRole::Tool,
                    vec![
                        SessionContentBlock::ToolResult {
                            tool_use_id: "call_1".to_string(),
                            content: serde_json::json!("contents"),
                            is_error: Some(false),
                        },
                        SessionContentBlock::ToolResult {
                            tool_use_id: "call_2".to_string(),
                            content: serde_json::json!(["Cargo.toml", "README.md"]),
                            is_error: Some(false),
                        },
                    ],
                    2,
                    None,
                ),
                SessionMessage::structured(
                    MessageRole::Assistant,
                    vec![SessionContentBlock::Text {
                        text: "Done.".to_string(),
                    }],
                    3,
                    None,
                ),
            ],
            SessionMemory::default(),
        )
        .expect("record turn");

        let history = reg.history(&key, 10).expect("history");
        assert_eq!(history.len(), 3);
        assert!(matches!(
            history[0].content.as_slice(),
            [
                SessionContentBlock::ToolUse { id: first_id, provider_id: first_provider, .. },
                SessionContentBlock::ToolUse { id: second_id, provider_id: second_provider, .. },
            ] if first_id == "call_1"
                && first_provider.as_deref() == Some("fc_1")
                && second_id == "call_2"
                && second_provider.as_deref() == Some("fc_2")
        ));
        assert!(matches!(
            history[1].content.as_slice(),
            [
                SessionContentBlock::ToolResult { tool_use_id: first_id, .. },
                SessionContentBlock::ToolResult { tool_use_id: second_id, .. },
            ] if first_id == "call_1" && second_id == "call_2"
        ));
        assert_eq!(history[2].render_text(), "Done.");
    }

    #[test]
    fn session_rejects_tool_result_before_matching_tool_use() {
        let reg = test_registry();
        let key = SessionKey::new("invalid-tool-write").unwrap();
        reg.create(key.clone(), SessionKind::Main, default_config())
            .expect("create");

        let error = reg
            .record_message_blocks(
                &key,
                MessageRole::Tool,
                vec![SessionContentBlock::ToolResult {
                    tool_use_id: "call_1".to_string(),
                    content: serde_json::json!("missing"),
                    is_error: Some(false),
                }],
                None,
            )
            .expect_err("invalid tool result should fail");

        assert!(matches!(
            error,
            SessionError::InvalidHistory(SessionHistoryError::ToolResultBeforeToolUse {
                tool_use_id,
                message_index: 0,
                block_index: 0,
            }) if tool_use_id == "call_1"
        ));
        assert!(
            reg.history(&key, 10).expect("history").is_empty(),
            "rejected writes must not poison stored history"
        );
    }

    #[test]
    fn poisoned_loaded_history_is_rejected_before_replay() {
        let storage = Storage::open_in_memory().expect("in-memory storage");
        let key = SessionKey::new("poisoned-history").expect("session key");
        SessionStore::new(storage.clone())
            .save(&poisoned_session(key.as_str()))
            .expect("save poisoned session");

        let reg = SessionRegistry::new(SessionStore::new(storage)).expect("registry");

        assert!(matches!(
            reg.history(&key, 10),
            Err(SessionError::Corrupted {
                key: corrupted_key,
                source: SessionHistoryError::ToolResultBeforeToolUse { tool_use_id, .. },
            }) if corrupted_key == key && tool_use_id == "call_bad"
        ));
    }

    #[test]
    fn poisoned_loaded_history_rejects_follow_up_writes() {
        let storage = Storage::open_in_memory().expect("in-memory storage");
        let key = SessionKey::new("poisoned-follow-up").expect("session key");
        SessionStore::new(storage.clone())
            .save(&poisoned_session(key.as_str()))
            .expect("save poisoned session");

        let reg = SessionRegistry::new(SessionStore::new(storage)).expect("registry");
        let error = reg
            .send(&key, "hello again")
            .expect_err("poisoned session should reject follow-up writes");

        assert!(matches!(
            error,
            SessionError::Corrupted {
                key: corrupted_key,
                source: SessionHistoryError::ToolResultBeforeToolUse { tool_use_id, .. },
            } if corrupted_key == key && tool_use_id == "call_bad"
        ));
    }

    #[test]
    fn provider_id_survives_session_roundtrip() {
        let storage = Storage::open_in_memory().expect("in-memory storage");
        let store = SessionStore::new(storage.clone());
        let reg = SessionRegistry::new(store).expect("registry");
        let key = SessionKey::new("provider-id").unwrap();
        reg.create(key.clone(), SessionKind::Main, default_config())
            .expect("create");

        reg.record_turn(
            &key,
            vec![SessionMessage::structured(
                MessageRole::Assistant,
                vec![SessionContentBlock::ToolUse {
                    id: "call_1".to_string(),
                    provider_id: Some("fc_123".to_string()),
                    name: "read_file".to_string(),
                    input: serde_json::json!({"path": "README.md"}),
                }],
                1,
                None,
            )],
            SessionMemory::default(),
        )
        .expect("record turn");

        let reg = SessionRegistry::new(SessionStore::new(storage)).expect("reopen registry");
        let history = reg.history(&key, 10).expect("history");
        let message = history.first().expect("stored tool use");
        assert!(matches!(
            message.content.as_slice(),
            [SessionContentBlock::ToolUse { provider_id, .. }]
                if provider_id.as_deref() == Some("fc_123")
        ));
        let llm_message = message.to_llm_message();
        assert!(matches!(
            llm_message.content.as_slice(),
            [ContentBlock::ToolUse { provider_id, .. }]
                if provider_id.as_deref() == Some("fc_123")
        ));
    }

    #[test]
    fn sessions_survive_registry_recreation() {
        let storage = Storage::open_in_memory().expect("in-memory storage");
        let store = SessionStore::new(storage.clone());
        let reg = SessionRegistry::new(store).expect("registry");

        reg.create(
            SessionKey::new("persist").unwrap(),
            SessionKind::Channel,
            SessionConfig {
                label: Some("persistent".to_string()),
                model: "claude".to_string(),
            },
        )
        .expect("create");
        reg.send(&SessionKey::new("persist").unwrap(), "survive restart")
            .expect("send");

        // Simulate restart: create a new registry from the same storage
        let store2 = SessionStore::new(storage);
        let reg2 = SessionRegistry::new(store2).expect("registry2");

        let all = reg2.list(None).expect("list");
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].key, SessionKey::new("persist").unwrap());
        assert_eq!(all[0].label.as_deref(), Some("persistent"));

        let history = reg2
            .history(&SessionKey::new("persist").unwrap(), 10)
            .expect("history");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].render_text(), "survive restart");
    }

    /// Regression test: creating a duplicate key must NOT corrupt the
    /// original session's persisted data. Previously, `create()` wrote
    /// to the store before checking for duplicates, silently overwriting
    /// the original session on disk.
    #[test]
    fn create_duplicate_does_not_corrupt_stored_data() {
        let storage = Storage::open_in_memory().expect("in-memory storage");
        let store = SessionStore::new(storage.clone());
        let reg = SessionRegistry::new(store).expect("registry");

        // Create the original session and add a message to it.
        reg.create(
            SessionKey::new("dup-persist").unwrap(),
            SessionKind::Main,
            SessionConfig {
                label: Some("original".to_string()),
                model: "gpt-4".to_string(),
            },
        )
        .expect("first create");
        reg.send(&SessionKey::new("dup-persist").unwrap(), "important data")
            .expect("send");

        // Attempt to create a duplicate — this must fail.
        let err = reg
            .create(
                SessionKey::new("dup-persist").unwrap(),
                SessionKind::Subagent,
                SessionConfig {
                    label: Some("impostor".to_string()),
                    model: "claude".to_string(),
                },
            )
            .expect_err("duplicate should fail");
        assert!(matches!(err, SessionError::AlreadyExists(_)));

        // Simulate restart and verify original data survived intact.
        let store2 = SessionStore::new(storage);
        let reg2 = SessionRegistry::new(store2).expect("registry2");

        let info = reg2
            .get_info(&SessionKey::new("dup-persist").unwrap())
            .expect("get_info");
        assert_eq!(info.label.as_deref(), Some("original"));
        assert_eq!(info.model, "gpt-4");
        assert_eq!(info.kind, SessionKind::Main);

        let history = reg2
            .history(&SessionKey::new("dup-persist").unwrap(), 10)
            .expect("history");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].render_text(), "important data");
    }

    #[test]
    fn concurrent_access_does_not_panic() {
        let reg = test_registry();
        reg.create(
            SessionKey::new("concurrent").unwrap(),
            SessionKind::Main,
            default_config(),
        )
        .expect("create");

        let handles: Vec<_> = (0..4)
            .map(|i| {
                let r = reg.clone();
                std::thread::spawn(move || {
                    r.send(
                        &SessionKey::new("concurrent").unwrap(),
                        &format!("thread-{i}"),
                    )
                    .expect("send");
                    r.list(None).expect("list");
                    r.history(&SessionKey::new("concurrent").unwrap(), 10)
                        .expect("history");
                })
            })
            .collect();

        for handle in handles {
            handle.join().expect("thread should not panic");
        }

        let history = reg
            .history(&SessionKey::new("concurrent").unwrap(), 100)
            .expect("history");
        assert_eq!(history.len(), 4);
    }

    #[test]
    fn session_clear_empties_messages_and_persists() {
        let storage = Storage::open_in_memory().expect("in-memory storage");
        let store = SessionStore::new(storage.clone());
        let reg = SessionRegistry::new(store).expect("registry");

        let key = SessionKey::new("clear-persist").unwrap();
        reg.create(key.clone(), SessionKind::Main, default_config())
            .expect("create");
        reg.record_message(&key, MessageRole::User, "hello")
            .expect("record user");
        reg.record_message(&key, MessageRole::Assistant, "world")
            .expect("record assistant");

        reg.clear(&key).expect("clear");

        let store2 = SessionStore::new(storage);
        let reg2 = SessionRegistry::new(store2).expect("registry2");
        let info = reg2.get_info(&key).expect("get info");
        let history = reg2.history(&key, 10).expect("history");

        assert_eq!(info.message_count, 0);
        assert!(history.is_empty());
    }

    #[test]
    fn open_creates_registry_at_database_path() {
        let unique = format!(
            "fx-session-open-{}-{}.redb",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after unix epoch")
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);

        let registry = SessionRegistry::open(&path).expect("registry should open");
        registry
            .create(
                SessionKey::new("open-path").unwrap(),
                SessionKind::Main,
                default_config(),
            )
            .expect("create");

        // Drop the first registry to release the exclusive redb lock before reopening.
        drop(registry);

        let reopened = SessionRegistry::open(&path).expect("registry should reopen");
        let info = reopened
            .get_info(&SessionKey::new("open-path").unwrap())
            .expect("get info");
        assert_eq!(info.label.as_deref(), Some("test"));

        let _ = std::fs::remove_file(path);
    }
}
