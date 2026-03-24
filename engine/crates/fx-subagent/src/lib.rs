mod config;
mod handle;
mod instance;
mod manager;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

use async_trait::async_trait;
use fx_kernel::cancellation::CancellationToken;
use std::fmt::Debug;
use std::time::Duration;
use thiserror::Error;

pub use config::{SpawnConfig, SpawnMode, SubagentLimits};
pub use handle::{SubagentHandle, SubagentId, SubagentStatus};
pub use manager::{SubagentManager, SubagentManagerDeps};

/// Result of a single subagent turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentTurn {
    pub response: String,
    pub tokens_used: u64,
}

/// Factory output used by the manager to create a new isolated session.
pub struct CreatedSubagentSession {
    pub session: Box<dyn SubagentSession>,
    pub cancel_token: CancellationToken,
}

impl std::fmt::Debug for CreatedSubagentSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CreatedSubagentSession")
            .field("session", &"<subagent-session>")
            .field("cancel_token", &self.cancel_token)
            .finish()
    }
}

/// Session abstraction implemented by the concrete shell layer.
#[async_trait]
pub trait SubagentSession: Send {
    async fn process_message(&mut self, input: &str) -> Result<SubagentTurn, SubagentError>;
}

/// Factory abstraction used to build isolated sessions.
pub trait SubagentFactory: Send + Sync + Debug {
    fn create_session(&self, config: &SpawnConfig)
        -> Result<CreatedSubagentSession, SubagentError>;
}

/// Shared control surface used by tool integrations.
#[async_trait]
pub trait SubagentControl: Send + Sync + Debug {
    async fn spawn(&self, config: SpawnConfig) -> Result<SubagentHandle, SubagentError>;
    async fn status(&self, id: &SubagentId) -> Result<Option<SubagentStatus>, SubagentError>;
    async fn list(&self) -> Result<Vec<SubagentHandle>, SubagentError>;
    async fn get(&self, id: &SubagentId) -> Result<Option<SubagentHandle>, SubagentError> {
        Ok(self
            .list()
            .await?
            .into_iter()
            .find(|handle| handle.id == *id))
    }
    async fn cancel(&self, id: &SubagentId) -> Result<(), SubagentError>;
    async fn send(&self, id: &SubagentId, message: &str) -> Result<String, SubagentError>;
    async fn gc(&self, max_age: Duration) -> Result<(), SubagentError>;
}

/// Errors returned by subagent orchestration.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum SubagentError {
    #[error("subagent not found: {0}")]
    NotFound(String),
    #[error("subagent concurrency limit reached: {0}")]
    MaxConcurrent(usize),
    #[error("subagent spawn failed: {0}")]
    Spawn(String),
    #[error("subagent execution failed: {0}")]
    Execution(String),
    #[error("subagent session is closed: {0}")]
    SessionClosed(String),
}
