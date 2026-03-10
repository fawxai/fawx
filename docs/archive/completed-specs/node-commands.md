# Spec: Node Commands (fx-fleet Wiring)

**Status:** Draft  
**Date:** 2026-03-08  

---

## 1. Problem

fx-fleet (node registry, task router, orchestrator) is built but not wired to actual remote execution. Fawx can't dispatch commands to remote nodes (Mac Mini, MacBook, etc.).

## 2. Goals

1. **Remote command execution** — run shell commands on registered nodes
2. **Node discovery** — nodes register via heartbeat, announce capabilities
3. **Result collection** — stdout/stderr/exit code returned to parent
4. **Timeout handling** — commands timeout gracefully

## 3. Architecture

### Wire existing crates

fx-fleet already has:
- `NodeRegistry` — register/query nodes by capability
- `TaskRouter` — select nodes by capability
- `Orchestrator` — dispatch, retry, timeout

### What's missing: Transport Layer

```rust
/// How the orchestrator actually talks to nodes.
pub trait NodeTransport: Send + Sync {
    /// Execute a command on a remote node.
    async fn execute(
        &self,
        node: &NodeInfo,
        command: &str,
        timeout: Duration,
    ) -> Result<CommandResult, TransportError>;

    /// Check if a node is reachable.
    async fn ping(&self, node: &NodeInfo) -> Result<Duration, TransportError>;
}

pub struct CommandResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration: Duration,
}
```

### Transport Implementations

**Phase 1: SSH transport**
```rust
pub struct SshTransport {
    /// SSH key path for authentication.
    key_path: PathBuf,
}
```
Uses `tokio::process::Command` to run `ssh node_addr command`. Simple, works with existing infrastructure (Tailscale + SSH).

**Phase 2: Native Fawx transport** (future)
A Fawx node agent that accepts commands via HTTP/WebSocket. Higher bandwidth, structured results, streaming output.

### Tool

```json
{
  "name": "node_run",
  "description": "Execute a command on a remote Fawx node",
  "parameters": {
    "node": "string — node ID or name",
    "command": "string — shell command to execute",
    "timeout_seconds": "integer — max execution time (default: 60)",
    "cwd": "string — working directory on the remote node"
  }
}
```

### Config

```toml
[fleet]
enabled = true

[[fleet.nodes]]
id = "mac-mini"
name = "Mac Mini"
address = "100.x.y.z"
user = "joseph"
ssh_key = "~/.ssh/id_ed25519"
capabilities = ["build", "test", "gpu"]

[[fleet.nodes]]
id = "macbook"
name = "MacBook Pro"
address = "100.x.y.z"
user = "clawdiobot"
ssh_key = "~/.ssh/id_ed25519"
capabilities = ["build", "test"]
```

## 4. Integration

- `NodeTransport` trait in `fx-fleet`
- `SshTransport` in `fx-fleet` or new `fx-transport-ssh`
- `node_run` tool registered in tool registry
- Orchestrator uses transport for dispatch

## 5. Testing

- Mock transport for unit tests
- SSH transport integration test (localhost)
- Timeout handling
- Node unreachable handling
- Command result parsing

## 6. File Touchpoints

- **Modify:** `engine/crates/fx-fleet/` (add NodeTransport trait, SshTransport)
- **Modify:** `engine/crates/fx-orchestrator/` (wire transport into dispatch)
- **Modify:** `engine/crates/fx-core/src/tools/` (register node_run)
- **Modify:** `engine/crates/fx-config/` (FleetConfig with node definitions)
