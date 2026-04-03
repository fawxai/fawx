//! `node_run` tool — execute a command on a remote Fawx node.

use fx_fleet::{CommandResult, NodeInfo, NodeRegistry, NodeTransport, TransportError};
use fx_llm::ToolDefinition;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

const DEFAULT_TIMEOUT_SECS: u64 = 60;
const MAX_TIMEOUT_SECS: u64 = 3600;

/// Shared state needed by the `node_run` tool at execution time.
#[derive(Clone)]
pub struct NodeRunState {
    pub registry: Arc<RwLock<NodeRegistry>>,
    pub transport: Arc<dyn NodeTransport>,
}

impl std::fmt::Debug for NodeRunState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NodeRunState")
            .field("registry", &"<NodeRegistry>")
            .field("transport", &"<dyn NodeTransport>")
            .finish()
    }
}

#[derive(Deserialize)]
pub(crate) struct NodeRunArgs {
    /// Node ID or name to target.
    node: String,
    /// Shell command to execute on the remote node.
    command: String,
    /// Maximum execution time in seconds (default: 60).
    timeout_seconds: Option<u64>,
    /// Working directory on the remote node.
    cwd: Option<String>,
}

/// Execute the `node_run` tool.
pub(crate) async fn handle_node_run(
    state: &NodeRunState,
    args: &serde_json::Value,
) -> Result<String, String> {
    let parsed: NodeRunArgs =
        serde_json::from_value(args.clone()).map_err(|e| format!("invalid arguments: {e}"))?;

    let timeout_secs = parsed
        .timeout_seconds
        .unwrap_or(DEFAULT_TIMEOUT_SECS)
        .min(MAX_TIMEOUT_SECS);
    let timeout = Duration::from_secs(timeout_secs);

    let registry = state.registry.read().await;
    let node = resolve_node(&registry, &parsed.node)?;

    let command = build_remote_command(&parsed.command, parsed.cwd.as_deref());
    drop(registry);

    let result = state
        .transport
        .execute(&node, &command, timeout)
        .await
        .map_err(format_transport_error)?;

    Ok(format_result(&result))
}

/// Resolve a node by ID or name from the registry.
fn resolve_node(registry: &NodeRegistry, query: &str) -> Result<NodeInfo, String> {
    // Try by ID first
    if let Some(node) = registry.get(query) {
        return Ok(node.clone());
    }

    // Try by name (case-insensitive)
    let by_name: Vec<&NodeInfo> = registry
        .all()
        .into_iter()
        .filter(|n| n.name.eq_ignore_ascii_case(query))
        .collect();

    match by_name.len() {
        0 => Err(format!("node not found: '{query}'")),
        1 => Ok(by_name[0].clone()),
        n => Err(format!(
            "ambiguous node name '{query}' matches {n} nodes; use node ID instead"
        )),
    }
}

/// Wrap the command with a `cd` prefix if a working directory is specified.
fn build_remote_command(command: &str, cwd: Option<&str>) -> String {
    match cwd {
        Some(dir) => format!("cd {} && {}", shell_escape(dir), command),
        None => command.to_string(),
    }
}

/// Minimal shell escaping for directory paths.
fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn format_transport_error(error: TransportError) -> String {
    match error {
        TransportError::Timeout(d) => format!("command timed out after {d:?}"),
        TransportError::Unreachable(msg) => format!("node unreachable: {msg}"),
        TransportError::AuthFailed(msg) => format!("authentication failed: {msg}"),
        TransportError::Other(msg) => format!("transport error: {msg}"),
    }
}

fn format_result(result: &CommandResult) -> String {
    format!(
        "exit_code: {}\nduration: {:.1}s\nstdout:\n{}\nstderr:\n{}",
        result.exit_code,
        result.duration.as_secs_f64(),
        result.stdout,
        result.stderr,
    )
}

/// Tool definition for `node_run`.
pub fn node_run_tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: "node_run".to_string(),
        description: "Execute a command on a remote Fawx node".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "node": {
                    "type": "string",
                    "description": "Node ID or name"
                },
                "command": {
                    "type": "string",
                    "description": "Shell command to execute on the remote node"
                },
                "timeout_seconds": {
                    "type": "integer",
                    "description": "Maximum execution time in seconds (default: 60)"
                },
                "cwd": {
                    "type": "string",
                    "description": "Working directory on the remote node"
                }
            },
            "required": ["node", "command"]
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_fleet::{NodeCapability, NodeStatus};
    use std::sync::Mutex;

    /// Mock transport that records calls and returns configured results.
    struct MockTransport {
        calls: Mutex<Vec<(String, String)>>,
        result: Mutex<Result<CommandResult, TransportError>>,
    }

    impl MockTransport {
        fn succeeding(stdout: &str) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                result: Mutex::new(Ok(CommandResult {
                    stdout: stdout.to_string(),
                    stderr: String::new(),
                    exit_code: 0,
                    duration: Duration::from_millis(42),
                })),
            }
        }

        fn failing(error: TransportError) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                result: Mutex::new(Err(error)),
            }
        }

        fn recorded_calls(&self) -> Vec<(String, String)> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl NodeTransport for MockTransport {
        async fn execute(
            &self,
            node: &NodeInfo,
            command: &str,
            _timeout: Duration,
        ) -> Result<CommandResult, TransportError> {
            self.calls
                .lock()
                .unwrap()
                .push((node.node_id.clone(), command.to_string()));
            let mut guard = self.result.lock().unwrap();
            // Take the result, replacing with a generic error for subsequent calls
            std::mem::replace(
                &mut *guard,
                Err(TransportError::Other("already consumed".to_string())),
            )
        }

        async fn ping(&self, _node: &NodeInfo) -> Result<Duration, TransportError> {
            Ok(Duration::from_millis(1))
        }
    }

    fn make_node(id: &str, name: &str) -> NodeInfo {
        NodeInfo {
            node_id: id.to_string(),
            name: name.to_string(),
            endpoint: format!("https://{id}:8400"),
            auth_token: None,
            capabilities: vec![NodeCapability::Network],
            status: NodeStatus::Online,
            last_heartbeat_ms: 1000,
            registered_at_ms: 1000,
            address: Some("10.0.0.1".to_string()),
            ssh_user: Some("deploy".to_string()),
            ssh_key: None,
        }
    }

    fn make_state(nodes: Vec<NodeInfo>, transport: Arc<dyn NodeTransport>) -> NodeRunState {
        let mut registry = NodeRegistry::new();
        for node in nodes {
            registry.register(node);
        }
        NodeRunState {
            registry: Arc::new(RwLock::new(registry)),
            transport,
        }
    }

    #[tokio::test]
    async fn executes_command_on_node_by_id() {
        let transport = Arc::new(MockTransport::succeeding("hello world\n"));
        let state = make_state(vec![make_node("n1", "Node One")], transport.clone());

        let result = handle_node_run(
            &state,
            &serde_json::json!({"node": "n1", "command": "echo hello"}),
        )
        .await
        .expect("should succeed");

        assert!(result.contains("exit_code: 0"));
        assert!(result.contains("hello world"));
        let calls = transport.recorded_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "n1");
        assert_eq!(calls[0].1, "echo hello");
    }

    #[tokio::test]
    async fn resolves_node_by_name() {
        let transport = Arc::new(MockTransport::succeeding("ok\n"));
        let state = make_state(vec![make_node("n1", "Mac Mini")], transport.clone());

        let result = handle_node_run(
            &state,
            &serde_json::json!({"node": "Mac Mini", "command": "ls"}),
        )
        .await
        .expect("should resolve by name");

        assert!(result.contains("exit_code: 0"));
        let calls = transport.recorded_calls();
        assert_eq!(calls[0].0, "n1");
    }

    #[tokio::test]
    async fn resolves_node_name_case_insensitive() {
        let transport = Arc::new(MockTransport::succeeding("ok\n"));
        let state = make_state(vec![make_node("n1", "MacBook Pro")], transport.clone());

        let result = handle_node_run(
            &state,
            &serde_json::json!({"node": "macbook pro", "command": "ls"}),
        )
        .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn returns_error_for_unknown_node() {
        let transport = Arc::new(MockTransport::succeeding("ok\n"));
        let state = make_state(vec![make_node("n1", "Node One")], transport);

        let error = handle_node_run(
            &state,
            &serde_json::json!({"node": "nonexistent", "command": "ls"}),
        )
        .await
        .expect_err("should fail");

        assert!(error.contains("node not found"));
    }

    #[tokio::test]
    async fn prepends_cd_when_cwd_specified() {
        let transport = Arc::new(MockTransport::succeeding(""));
        let state = make_state(vec![make_node("n1", "N1")], transport.clone());

        handle_node_run(
            &state,
            &serde_json::json!({
                "node": "n1",
                "command": "cargo build",
                "cwd": "/home/user/project"
            }),
        )
        .await
        .expect("should succeed");

        let calls = transport.recorded_calls();
        assert!(calls[0].1.starts_with("cd "));
        assert!(calls[0].1.contains("/home/user/project"));
        assert!(calls[0].1.contains("cargo build"));
    }

    #[tokio::test]
    async fn reports_transport_timeout() {
        let transport = Arc::new(MockTransport::failing(TransportError::Timeout(
            Duration::from_secs(60),
        )));
        let state = make_state(vec![make_node("n1", "N1")], transport);

        let error = handle_node_run(
            &state,
            &serde_json::json!({"node": "n1", "command": "sleep 999"}),
        )
        .await
        .expect_err("should fail");

        assert!(error.contains("timed out"));
    }

    #[tokio::test]
    async fn reports_transport_unreachable() {
        let transport = Arc::new(MockTransport::failing(TransportError::Unreachable(
            "connection refused".to_string(),
        )));
        let state = make_state(vec![make_node("n1", "N1")], transport);

        let error = handle_node_run(&state, &serde_json::json!({"node": "n1", "command": "ls"}))
            .await
            .expect_err("should fail");

        assert!(error.contains("unreachable"));
    }

    #[test]
    fn build_remote_command_without_cwd() {
        assert_eq!(build_remote_command("ls -la", None), "ls -la");
    }

    #[test]
    fn build_remote_command_with_cwd() {
        let result = build_remote_command("ls", Some("/tmp"));
        assert!(result.starts_with("cd "));
        assert!(result.contains("/tmp"));
        assert!(result.ends_with("ls"));
    }

    #[test]
    fn shell_escape_handles_single_quotes() {
        let escaped = shell_escape("it's a test");
        assert_eq!(escaped, "'it'\\''s a test'");
    }

    #[tokio::test]
    async fn clamps_timeout_to_max() {
        let transport = Arc::new(MockTransport::succeeding("ok\n"));
        let state = make_state(vec![make_node("n1", "N1")], transport.clone());

        // Request 9999 seconds — should be clamped to MAX_TIMEOUT_SECS (3600)
        let result = handle_node_run(
            &state,
            &serde_json::json!({
                "node": "n1",
                "command": "echo ok",
                "timeout_seconds": 9999
            }),
        )
        .await
        .expect("should succeed");

        assert!(result.contains("exit_code: 0"));
    }

    #[test]
    fn tool_definition_has_required_fields() {
        let def = node_run_tool_definition();
        assert_eq!(def.name, "node_run");
        let params = def.parameters.as_object().expect("params object");
        let required = params["required"].as_array().expect("required array");
        assert!(required.iter().any(|v| v == "node"));
        assert!(required.iter().any(|v| v == "command"));
    }
}
