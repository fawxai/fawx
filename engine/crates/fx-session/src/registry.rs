//! Session registry: tracks all active sessions and delegates persistence.

use crate::session::{Session, SessionContentBlock, SessionMemory, SessionMessage};
use crate::store::SessionStore;
use crate::types::{
    MessageRole, SessionConfig, SessionInfo, SessionKey, SessionKind, SessionStatus,
};
use fx_core::error::StorageError;
use fx_storage::Storage;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};

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

    /// Internal lock poisoning.
    #[error("internal error: lock poisoned")]
    LockPoisoned,
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
        let persisted = store.load_all()?;
        let mut sessions = HashMap::with_capacity(persisted.len());
        for session in persisted {
            sessions.insert(session.key.clone(), session);
        }
        Ok(Self {
            sessions: Arc::new(RwLock::new(sessions)),
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
        let map = self.read()?;
        let infos = map
            .values()
            .filter(|s| filter.is_none_or(|k| s.kind == k))
            .map(Session::info)
            .collect();
        Ok(infos)
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
        let removed = {
            let mut map = self.write()?;
            map.remove(key)
        };
        if removed.is_none() {
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
        let snapshot = {
            let mut map = self.write()?;
            let session = map
                .get_mut(key)
                .ok_or_else(|| SessionError::NotFound(key.as_str().to_string()))?;
            session.add_message_blocks(role, content, token_count);
            session.clone()
        };
        self.store.save(&snapshot)?;
        Ok(())
    }

    /// Append multiple pre-built session messages in a single save.
    pub fn append_messages(&self, key: &SessionKey, messages: Vec<SessionMessage>) -> Result<()> {
        if messages.is_empty() {
            return Ok(());
        }

        let snapshot = {
            let mut map = self.write()?;
            let session = map
                .get_mut(key)
                .ok_or_else(|| SessionError::NotFound(key.as_str().to_string()))?;
            session.extend_messages(messages);
            session.clone()
        };
        self.store.save(&snapshot)?;
        Ok(())
    }

    /// Read the persistent memory for a session.
    pub fn memory(&self, key: &SessionKey) -> Result<SessionMemory> {
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
        let snapshot = {
            let mut map = self.write()?;
            let session = map
                .get_mut(key)
                .ok_or_else(|| SessionError::NotFound(key.as_str().to_string()))?;
            if !messages.is_empty() {
                session.extend_messages(messages);
            }
            session.set_memory(memory);
            session.clone()
        };
        self.store.save(&snapshot)?;
        Ok(())
    }

    /// Retrieve conversation history for a session (most recent `limit`).
    pub fn history(&self, key: &SessionKey, limit: usize) -> Result<Vec<SessionMessage>> {
        let map = self.read()?;
        let session = map
            .get(key)
            .ok_or_else(|| SessionError::NotFound(key.as_str().to_string()))?;
        Ok(session.recent_messages(limit).to_vec())
    }

    /// Clear the recorded message history for a session.
    pub fn clear(&self, key: &SessionKey) -> Result<()> {
        let snapshot = {
            let mut map = self.write()?;
            let session = map
                .get_mut(key)
                .ok_or_else(|| SessionError::NotFound(key.as_str().to_string()))?;
            session.clear_messages();
            session.clone()
        };
        self.store.save(&snapshot)?;
        Ok(())
    }

    /// Update the status of a session.
    pub fn set_status(&self, key: &SessionKey, status: SessionStatus) -> Result<()> {
        let snapshot = {
            let mut map = self.write()?;
            let session = map
                .get_mut(key)
                .ok_or_else(|| SessionError::NotFound(key.as_str().to_string()))?;
            session.status = status;
            session.clone()
        };
        self.store.save(&snapshot)?;
        Ok(())
    }

    /// Get a snapshot of a single session's info.
    pub fn get_info(&self, key: &SessionKey) -> Result<SessionInfo> {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_storage::Storage;

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
