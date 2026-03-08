use crate::SpawnMode;
use std::fmt::{Display, Formatter};
use std::time::Instant;
use uuid::Uuid;

/// Unique identifier for a subagent instance.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SubagentId(pub String);

impl SubagentId {
    /// Create a fresh UUID-backed subagent ID.
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

impl Default for SubagentId {
    fn default() -> Self {
        Self::new()
    }
}

impl Display for SubagentId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Current subagent lifecycle status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubagentStatus {
    Running,
    Completed { result: String, tokens_used: u64 },
    Failed { error: String },
    Cancelled,
    TimedOut,
}

impl SubagentStatus {
    pub(crate) fn is_terminal(&self) -> bool {
        !matches!(self, Self::Running)
    }
}

/// Parent-visible snapshot of a subagent instance.
#[derive(Clone, Debug)]
pub struct SubagentHandle {
    pub id: SubagentId,
    pub label: Option<String>,
    pub status: SubagentStatus,
    pub mode: SpawnMode,
    pub started_at: Instant,
    pub initial_response: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subagent_id_new_creates_unique_values() {
        let left = SubagentId::new();
        let right = SubagentId::new();

        assert_ne!(left, right);
        assert_eq!(left.0.len(), 36);
        assert_eq!(right.0.len(), 36);
    }

    #[test]
    fn subagent_status_terminal_check_matches_lifecycle_states() {
        assert!(!SubagentStatus::Running.is_terminal());
        assert!(SubagentStatus::Cancelled.is_terminal());
        assert!(SubagentStatus::TimedOut.is_terminal());
    }
}
