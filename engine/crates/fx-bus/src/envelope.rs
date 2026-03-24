use fx_session::SessionKey;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// A message envelope routed between sessions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Envelope {
    pub id: String,
    pub from: Option<SessionKey>,
    pub to: SessionKey,
    pub payload: Payload,
    pub created_at: u64,
}

impl Envelope {
    pub fn new(from: Option<SessionKey>, to: SessionKey, payload: Payload) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            from,
            to,
            payload,
            created_at: current_time_millis(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Payload {
    /// Plain text to process through the agent loop.
    Text(String),
    /// Result from a completed task.
    TaskResult {
        task_id: String,
        success: bool,
        output: String,
    },
    /// Progress update from a running task.
    StatusUpdate { task_id: String, progress: String },
    /// System event (cron, health, fleet).
    System(String),
}

fn current_time_millis() -> u64 {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
}
