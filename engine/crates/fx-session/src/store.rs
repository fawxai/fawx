//! Persistence layer for sessions, backed by fx-storage (redb).

use crate::session::Session;
use crate::types::SessionKey;
use fx_core::error::StorageError;
use fx_storage::Storage;

type Result<T> = std::result::Result<T, StorageError>;

/// Table name for session data in the redb database.
const SESSIONS_TABLE: &str = "sessions";

/// Persists session state to a redb-backed `Storage`.
#[derive(Clone)]
pub struct SessionStore {
    storage: Storage,
}

impl SessionStore {
    /// Create a store backed by the given `Storage` instance.
    pub fn new(storage: Storage) -> Self {
        Self { storage }
    }

    /// Save a session to persistent storage.
    pub fn save(&self, session: &Session) -> Result<()> {
        let bytes = serde_json::to_vec(session)
            .map_err(|e| StorageError::Database(format!("failed to serialize session: {e}")))?;
        self.storage
            .put(SESSIONS_TABLE, session.key.as_str(), &bytes)
    }

    /// Load a session by key. Returns `None` if not found.
    pub fn load(&self, key: &SessionKey) -> Result<Option<Session>> {
        let bytes = self.storage.get(SESSIONS_TABLE, key.as_str())?;
        match bytes {
            Some(data) => {
                let session: Session = serde_json::from_slice(&data).map_err(|e| {
                    StorageError::Database(format!("failed to deserialize session: {e}"))
                })?;
                Ok(Some(session))
            }
            None => Ok(None),
        }
    }

    /// Delete a session from persistent storage. Returns `true` if it existed.
    pub fn delete(&self, key: &SessionKey) -> Result<bool> {
        self.storage.delete(SESSIONS_TABLE, key.as_str())
    }

    /// List all persisted session keys.
    pub fn list_keys(&self) -> Result<Vec<SessionKey>> {
        let keys = self.storage.list_keys(SESSIONS_TABLE)?;
        Ok(keys.into_iter().map(SessionKey).collect())
    }

    /// Load all persisted sessions.
    pub fn load_all(&self) -> Result<Vec<Session>> {
        let keys = self.list_keys()?;
        let mut sessions = Vec::with_capacity(keys.len());
        for key in &keys {
            if let Some(session) = self.load(key)? {
                sessions.push(session);
            }
        }
        Ok(sessions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{SessionConfig, SessionKind};

    fn test_store() -> SessionStore {
        let storage = Storage::open_in_memory().expect("in-memory storage");
        SessionStore::new(storage)
    }

    fn make_session(key: &str) -> Session {
        Session::new(
            SessionKey::new(key).unwrap(),
            SessionKind::Main,
            SessionConfig {
                label: Some("test".to_string()),
                model: "gpt-4".to_string(),
            },
        )
    }

    #[test]
    fn save_and_load_round_trips() {
        let store = test_store();
        let mut session = make_session("s1");
        session.add_message(crate::types::MessageRole::User, "hello");

        store.save(&session).expect("save");
        let loaded = store
            .load(&SessionKey::new("s1").unwrap())
            .expect("load")
            .expect("should exist");
        assert_eq!(loaded.key, session.key);
        assert_eq!(loaded.messages.len(), 1);
        assert_eq!(loaded.messages[0].render_text(), "hello");
    }

    #[test]
    fn load_nonexistent_returns_none() {
        let store = test_store();
        let result = store
            .load(&SessionKey::new("missing").unwrap())
            .expect("load");
        assert!(result.is_none());
    }

    #[test]
    fn delete_removes_session() {
        let store = test_store();
        let session = make_session("del");
        store.save(&session).expect("save");

        let existed = store
            .delete(&SessionKey::new("del").unwrap())
            .expect("delete");
        assert!(existed);

        let loaded = store.load(&SessionKey::new("del").unwrap()).expect("load");
        assert!(loaded.is_none());
    }

    #[test]
    fn delete_nonexistent_returns_false() {
        let store = test_store();
        let existed = store
            .delete(&SessionKey::new("nope").unwrap())
            .expect("delete");
        assert!(!existed);
    }

    #[test]
    fn list_keys_returns_all_saved_sessions() {
        let store = test_store();
        store.save(&make_session("a")).expect("save a");
        store.save(&make_session("b")).expect("save b");
        store.save(&make_session("c")).expect("save c");

        let mut keys: Vec<String> = store
            .list_keys()
            .expect("list")
            .into_iter()
            .map(|k| k.as_str().to_string())
            .collect();
        keys.sort();
        assert_eq!(keys, vec!["a", "b", "c"]);
    }

    #[test]
    fn load_all_returns_all_sessions() {
        let store = test_store();
        store.save(&make_session("x")).expect("save x");
        store.save(&make_session("y")).expect("save y");

        let sessions = store.load_all().expect("load_all");
        assert_eq!(sessions.len(), 2);
    }

    #[test]
    fn save_overwrites_existing_session() {
        let store = test_store();
        let mut session = make_session("overwrite");
        store.save(&session).expect("first save");

        session.add_message(crate::types::MessageRole::User, "new message");
        store.save(&session).expect("second save");

        let loaded = store
            .load(&SessionKey::new("overwrite").unwrap())
            .expect("load")
            .expect("should exist");
        assert_eq!(loaded.messages.len(), 1);
        assert_eq!(loaded.messages[0].render_text(), "new message");
    }
}
