//! Transport layer for executing commands on remote nodes.

use crate::NodeInfo;
use std::fmt;
use std::time::Duration;
use thiserror::Error;

/// Result of executing a command on a remote node.
#[derive(Debug, Clone)]
pub struct CommandResult {
    /// Standard output from the command.
    pub stdout: String,
    /// Standard error from the command.
    pub stderr: String,
    /// Process exit code.
    pub exit_code: i32,
    /// Wall-clock duration of the command execution.
    pub duration: Duration,
}

impl fmt::Display for CommandResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "exit_code: {}\nstdout:\n{}\nstderr:\n{}",
            self.exit_code, self.stdout, self.stderr
        )
    }
}

/// Errors from transport operations.
#[derive(Debug, Error)]
pub enum TransportError {
    /// The node is unreachable (connection refused, DNS failure, etc.).
    #[error("node unreachable: {0}")]
    Unreachable(String),

    /// The command timed out before completing.
    #[error("command timed out after {0:?}")]
    Timeout(Duration),

    /// Authentication with the node failed.
    #[error("authentication failed: {0}")]
    AuthFailed(String),

    /// The transport encountered an unexpected error.
    #[error("transport error: {0}")]
    Other(String),
}

/// How the orchestrator talks to remote nodes.
///
/// Implementations must be `Send + Sync` for use in async contexts.
#[async_trait::async_trait]
pub trait NodeTransport: Send + Sync {
    /// Execute a command on a remote node.
    async fn execute(
        &self,
        node: &NodeInfo,
        command: &str,
        timeout: Duration,
    ) -> Result<CommandResult, TransportError>;

    /// Check if a node is reachable. Returns round-trip time on success.
    async fn ping(&self, node: &NodeInfo) -> Result<Duration, TransportError>;
}
