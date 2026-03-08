//! SSH transport — executes commands on remote nodes via `ssh`.

use crate::transport::{CommandResult, NodeTransport, TransportError};
use crate::NodeInfo;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::process::Command;

/// SSH auth-failure patterns in stderr output.
const SSH_AUTH_FAILURE_PATTERNS: &[&str] = &[
    "Permission denied",
    "authentication failed",
    "no more authentication methods",
];

/// Classify an SSH exit-255 error as auth failure or unreachable.
fn classify_ssh_error(stderr: String) -> TransportError {
    let is_auth = SSH_AUTH_FAILURE_PATTERNS
        .iter()
        .any(|pattern| stderr.contains(pattern));
    if is_auth {
        TransportError::AuthFailed(stderr)
    } else {
        TransportError::Unreachable(stderr)
    }
}

/// SSH-based transport using `tokio::process::Command` to invoke `ssh`.
#[derive(Debug, Clone)]
pub struct SshTransport {
    /// Path to the SSH private key for authentication.
    key_path: PathBuf,
}

impl SshTransport {
    /// Create a new SSH transport with the given key path.
    pub fn new(key_path: PathBuf) -> Self {
        Self { key_path }
    }

    /// Build the base SSH command with common flags.
    fn base_command(&self, node: &NodeInfo) -> Result<Command, TransportError> {
        let address = node.address.as_deref().ok_or_else(|| {
            TransportError::Other(format!("node '{}' has no SSH address", node.node_id))
        })?;
        let user = node.ssh_user.as_deref().ok_or_else(|| {
            TransportError::Other(format!("node '{}' has no SSH user", node.node_id))
        })?;

        let key = node
            .ssh_key
            .as_deref()
            .map(Path::new)
            .unwrap_or(&self.key_path);

        let mut cmd = Command::new("ssh");
        cmd.kill_on_drop(true);
        cmd.args([
            "-i",
            &key.to_string_lossy(),
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
            "ConnectTimeout=10",
            "-o",
            "BatchMode=yes",
            &format!("{user}@{address}"),
        ]);
        Ok(cmd)
    }
}

#[async_trait::async_trait]
impl NodeTransport for SshTransport {
    async fn execute(
        &self,
        node: &NodeInfo,
        command: &str,
        timeout: Duration,
    ) -> Result<CommandResult, TransportError> {
        let mut cmd = self.base_command(node)?;
        cmd.arg("--").arg(command);

        let start = Instant::now();
        let output = tokio::time::timeout(timeout, cmd.output())
            .await
            .map_err(|_| TransportError::Timeout(timeout))?
            .map_err(|e| TransportError::Unreachable(e.to_string()))?;

        let duration = start.elapsed();
        let exit_code = output.status.code().unwrap_or(-1);

        // SSH exit code 255 indicates connection/auth failure.
        if exit_code == 255 && output.stdout.is_empty() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(classify_ssh_error(stderr));
        }

        Ok(CommandResult {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code,
            duration,
        })
    }

    async fn ping(&self, node: &NodeInfo) -> Result<Duration, TransportError> {
        let timeout = Duration::from_secs(10);
        let result = self.execute(node, "echo pong", timeout).await?;
        if result.exit_code != 0 {
            return Err(TransportError::Unreachable(format!(
                "ping command exited with code {}",
                result.exit_code
            )));
        }
        Ok(result.duration)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NodeCapability, NodeStatus};

    fn test_node(address: Option<&str>, user: Option<&str>) -> NodeInfo {
        NodeInfo {
            node_id: "test-node".to_string(),
            name: "Test Node".to_string(),
            endpoint: "https://test:8400".to_string(),
            auth_token: None,
            capabilities: vec![NodeCapability::Network],
            status: NodeStatus::Online,
            last_heartbeat_ms: 1000,
            registered_at_ms: 1000,
            address: address.map(String::from),
            ssh_user: user.map(String::from),
            ssh_key: None,
        }
    }

    #[tokio::test]
    async fn execute_fails_without_address() {
        let transport = SshTransport::new(PathBuf::from("/tmp/fake_key"));
        let node = test_node(None, Some("user"));
        let result = transport
            .execute(&node, "echo hi", Duration::from_secs(5))
            .await;
        assert!(matches!(result, Err(TransportError::Other(_))));
    }

    #[tokio::test]
    async fn execute_fails_without_user() {
        let transport = SshTransport::new(PathBuf::from("/tmp/fake_key"));
        let node = test_node(Some("10.0.0.1"), None);
        let result = transport
            .execute(&node, "echo hi", Duration::from_secs(5))
            .await;
        assert!(matches!(result, Err(TransportError::Other(_))));
    }

    #[tokio::test]
    #[ignore = "requires SSH to localhost with key auth — not available in CI"]
    async fn execute_localhost_echo() {
        let home = dirs::home_dir().expect("home dir");
        let key_path = home.join(".ssh/id_ed25519");

        let transport = SshTransport::new(key_path);
        let node = NodeInfo {
            node_id: "localhost".to_string(),
            name: "Localhost".to_string(),
            endpoint: "https://localhost:8400".to_string(),
            auth_token: None,
            capabilities: vec![],
            status: NodeStatus::Online,
            last_heartbeat_ms: 1000,
            registered_at_ms: 1000,
            address: Some("127.0.0.1".to_string()),
            ssh_user: Some(whoami().unwrap_or_else(|| "root".to_string())),
            ssh_key: None,
        };

        let result = transport
            .execute(&node, "echo hello", Duration::from_secs(5))
            .await
            .expect("SSH to localhost should succeed when test is not ignored");
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("hello"));
    }

    fn whoami() -> Option<String> {
        std::env::var("USER").ok()
    }

    #[test]
    fn classify_ssh_error_detects_permission_denied() {
        let err = classify_ssh_error("user@host: Permission denied (publickey).".to_string());
        assert!(matches!(err, TransportError::AuthFailed(_)));
    }

    #[test]
    fn classify_ssh_error_detects_no_auth_methods() {
        let err = classify_ssh_error("no more authentication methods to try".to_string());
        assert!(matches!(err, TransportError::AuthFailed(_)));
    }

    #[test]
    fn classify_ssh_error_returns_unreachable_for_connection_refused() {
        let err = classify_ssh_error("Connection refused".to_string());
        assert!(matches!(err, TransportError::Unreachable(_)));
    }

    #[tokio::test]
    async fn timeout_returns_timeout_error() {
        let transport = SshTransport::new(PathBuf::from("/tmp/fake_key"));
        // Use an unroutable IP to force a timeout
        let node = test_node(Some("192.0.2.1"), Some("user"));
        let result = transport
            .execute(&node, "echo hi", Duration::from_millis(100))
            .await;
        // Should be either Timeout or Unreachable depending on OS behavior
        assert!(result.is_err());
    }
}
