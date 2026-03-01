use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[cfg(test)]
const MAX_BUFFER_SIZE: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConversationMessage {
    pub role: String,
    pub content: String,
    pub timestamp_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signals: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_usage: Option<TokenUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ActiveConversation {
    id: String,
    created_at: String,
}

#[derive(Debug)]
pub struct ConversationStore {
    conversations_dir: PathBuf,
    active_id: Option<String>,
}

impl ConversationStore {
    pub fn new(data_dir: &Path) -> Result<Self, String> {
        let conversations_dir = data_dir.join("conversations");
        fs::create_dir_all(&conversations_dir).map_err(|error| error.to_string())?;
        let active_id = Self::load_active_id(&conversations_dir);
        Ok(Self {
            conversations_dir,
            active_id,
        })
    }

    pub fn ensure_active(&mut self) -> Result<String, String> {
        if let Some(id) = &self.active_id {
            return Ok(id.clone());
        }
        self.create_new()
    }

    pub fn create_new(&mut self) -> Result<String, String> {
        let id = format!("conv-{}", short_conversation_id());
        self.active_id = Some(id.clone());
        self.save_active_id()?;
        Ok(id)
    }

    pub fn save_message(&self, msg: &ConversationMessage) -> Result<(), String> {
        let Some(id) = &self.active_id else {
            return Err("no active conversation".to_string());
        };
        let path = self.conversations_dir.join(format!("{id}.jsonl"));
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|error| error.to_string())?;
        let line = serde_json::to_string(msg).map_err(|error| error.to_string())?;
        writeln!(file, "{line}").map_err(|error| error.to_string())?;
        file.sync_data().map_err(|error| error.to_string())
    }

    pub fn load_recent(&self, max: usize) -> Vec<ConversationMessage> {
        let Some(id) = &self.active_id else {
            return Vec::new();
        };
        let path = self.conversations_dir.join(format!("{id}.jsonl"));
        let file = match fs::File::open(path) {
            Ok(file) => file,
            Err(_) => return Vec::new(),
        };
        trim_recent_messages(read_messages(file), max)
    }

    pub fn clear_active(&self) -> Result<(), String> {
        let Some(id) = &self.active_id else {
            return Ok(());
        };
        let path = self.conversations_dir.join(format!("{id}.jsonl"));
        if path.exists() {
            fs::write(path, "").map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    pub fn list_conversations(&self) -> Vec<(String, usize)> {
        let entries = match fs::read_dir(&self.conversations_dir) {
            Ok(entries) => entries,
            Err(_) => return Vec::new(),
        };

        let mut conversations = entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("jsonl"))
            .map(|path| (file_stem_or_unknown(&path), count_non_empty_lines(&path)))
            .collect::<Vec<_>>();
        conversations.sort_by(|left, right| left.0.cmp(&right.0));
        conversations
    }

    fn load_active_id(dir: &Path) -> Option<String> {
        let active_path = dir.join("active.json");
        let content = fs::read_to_string(active_path).ok()?;
        let active = serde_json::from_str::<ActiveConversation>(&content).ok()?;
        Some(active.id)
    }

    fn save_active_id(&self) -> Result<(), String> {
        let Some(id) = &self.active_id else {
            return Ok(());
        };
        let active = ActiveConversation {
            id: id.clone(),
            created_at: current_epoch_secs(),
        };
        let json = serde_json::to_string_pretty(&active).map_err(|error| error.to_string())?;
        let path = self.conversations_dir.join("active.json");
        fs::write(path, json).map_err(|error| error.to_string())
    }
}

fn read_messages(file: fs::File) -> Vec<ConversationMessage> {
    let reader = BufReader::new(file);
    reader
        .lines()
        .map_while(Result::ok)
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<ConversationMessage>(&line).ok())
        .collect()
}

fn trim_recent_messages(
    mut messages: Vec<ConversationMessage>,
    max: usize,
) -> Vec<ConversationMessage> {
    if messages.len() > max {
        return messages.split_off(messages.len() - max);
    }
    messages
}

fn file_stem_or_unknown(path: &Path) -> String {
    path.file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("unknown")
        .to_string()
}

fn count_non_empty_lines(path: &Path) -> usize {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return 0,
    };

    BufReader::new(file)
        .lines()
        .filter(|line| line.as_ref().map(|s| !s.trim().is_empty()).unwrap_or(false))
        .count()
}

fn short_conversation_id() -> String {
    Uuid::new_v4()
        .simple()
        .to_string()
        .chars()
        .take(8)
        .collect()
}

fn current_epoch_secs() -> String {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    duration.as_secs().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn new_store_creates_directory() {
        let temp_dir = TempDir::new().expect("tempdir");
        let data_dir = temp_dir.path().join("new-data");

        let store = ConversationStore::new(&data_dir).expect("store creation");

        assert!(data_dir.join("conversations").exists());
        assert!(store.load_recent(MAX_BUFFER_SIZE).is_empty());
    }

    #[test]
    fn save_and_load_messages() {
        let (_temp_dir, mut store) = fresh_store();
        store.ensure_active().expect("active");

        for index in 0..3 {
            store
                .save_message(&message("user", &format!("m-{index}"), index))
                .expect("save message");
        }

        let loaded = store.load_recent(MAX_BUFFER_SIZE);
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded[0].content, "m-0");
        assert_eq!(loaded[2].content, "m-2");
    }

    #[test]
    fn load_recent_caps_at_max() {
        let (_temp_dir, mut store) = fresh_store();
        store.ensure_active().expect("active");

        for index in 0..25 {
            store
                .save_message(&message("assistant", &format!("msg-{index}"), index))
                .expect("save message");
        }

        let loaded = store.load_recent(MAX_BUFFER_SIZE);
        assert_eq!(loaded.len(), MAX_BUFFER_SIZE);
        assert_eq!(loaded[0].content, "msg-5");
        assert_eq!(loaded[19].content, "msg-24");
    }

    #[test]
    fn create_new_conversation() {
        let (_temp_dir, mut store) = fresh_store();
        let first_id = store.create_new().expect("first id");
        store
            .save_message(&message("user", "first", 1))
            .expect("save first message");

        let second_id = store.create_new().expect("second id");

        assert_ne!(first_id, second_id);
        assert!(store.load_recent(MAX_BUFFER_SIZE).is_empty());
    }

    #[test]
    fn clear_active_empties_messages() {
        let (_temp_dir, mut store) = fresh_store();
        store.create_new().expect("active id");
        store
            .save_message(&message("assistant", "hello", 1))
            .expect("save message");

        store.clear_active().expect("clear active");

        assert!(store.load_recent(MAX_BUFFER_SIZE).is_empty());
    }

    #[test]
    fn list_conversations_shows_all() {
        let temp_dir = TempDir::new().expect("tempdir");
        let mut store = ConversationStore::new(temp_dir.path()).expect("store creation");

        let first_id = store.create_new().expect("first id");
        store
            .save_message(&message("user", "first-a", 1))
            .expect("save message");
        store
            .save_message(&message("assistant", "first-b", 2))
            .expect("save message");

        let second_id = store.create_new().expect("second id");
        store
            .save_message(&message("user", "second-a", 3))
            .expect("save message");

        let listed = store.list_conversations();

        assert_eq!(listed.len(), 2);
        assert!(listed.contains(&(first_id, 2)));
        assert!(listed.contains(&(second_id, 1)));
    }

    #[test]
    fn corrupt_jsonl_skips_bad_lines() {
        let (_temp_dir, mut store) = fresh_store();
        let id = store.create_new().expect("active id");
        let path = temp_conversation_path(&store, &id);
        let good = serde_json::to_string(&message("user", "good", 1)).expect("serialize message");
        let payload = format!("{good}\n{{not valid json}}\n{good}\n");
        fs::write(path, payload).expect("write jsonl");

        let loaded = store.load_recent(MAX_BUFFER_SIZE);

        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].content, "good");
    }

    #[test]
    fn first_run_no_directory() {
        let temp_dir = TempDir::new().expect("tempdir");
        let new_path = temp_dir.path().join("missing");

        let mut store = ConversationStore::new(&new_path).expect("store creation");
        let active_id = store.ensure_active().expect("active id");

        assert!(new_path.join("conversations").exists());
        assert!(active_id.starts_with("conv-"));
    }

    fn fresh_store() -> (TempDir, ConversationStore) {
        let temp_dir = TempDir::new().expect("tempdir");
        let store = ConversationStore::new(temp_dir.path()).expect("store creation");
        (temp_dir, store)
    }

    fn message(role: &str, content: &str, timestamp_ms: usize) -> ConversationMessage {
        ConversationMessage {
            role: role.to_string(),
            content: content.to_string(),
            timestamp_ms: timestamp_ms as u64,
            signals: None,
            tool_calls: None,
            token_usage: None,
        }
    }

    fn temp_conversation_path(store: &ConversationStore, id: &str) -> PathBuf {
        store.conversations_dir.join(format!("{id}.jsonl"))
    }
}
