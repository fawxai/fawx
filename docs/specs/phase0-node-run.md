# Spec: Phase 0 PR 3 — node_run Tool Wiring

**Gap:** node_run tool exists but NodeRunState is never attached  
**Estimated size:** ~150 lines  
**Risk:** Low — plumbing from config to tool

---

## Problem

`fx-tools/src/node_run.rs` has a complete `node_run` tool implementation with:
- `NodeRunState` (holds `NodeRegistry` + `NodeTransport`)
- `handle_node_run()` async handler
- `node_run_tool_definition()` 
- `FawxToolExecutor.with_node_run()` setter

`SshTransport` is implemented in `fx-fleet/src/ssh_transport.rs` with:
- SSH key, agent, and password auth detection
- Command execution over SSH
- Timeout handling

But `with_node_run()` is **never called** — not in `build_skill_registry()`, 
not in `main.rs`, nowhere. The tool is registered in the definition list but 
execution always returns "node_run not configured."

## Solution

### Read fleet config

`FawxConfig` already has `fleet: FleetConfig`:

```rust
pub struct FleetConfig {
    pub nodes: Vec<NodeConfig>,
}

pub struct NodeConfig {
    pub name: String,
    pub host: String,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub auth_token: Option<String>,
    pub capabilities: Vec<String>,
}
```

### Wire in build_skill_registry

In `build_skill_registry()` (tui.rs), after creating `FawxToolExecutor`:

```rust
// Wire node_run if fleet nodes are configured
if !config.fleet.nodes.is_empty() {
    match build_node_run_state(config) {
        Ok(state) => {
            executor = executor.with_node_run(state);
        }
        Err(e) => {
            tracing::warn!("node_run unavailable: {e}");
        }
    }
}
```

New function:
```rust
fn build_node_run_state(config: &FawxConfig) -> Result<NodeRunState, String> {
    let mut registry = NodeRegistry::new();
    for node_config in &config.fleet.nodes {
        let node_info = NodeInfo {
            name: node_config.name.clone(),
            host: node_config.host.clone(),
            port: node_config.port.unwrap_or(22),
            user: node_config.user.clone(),
            capabilities: node_config.capabilities.clone(),
        };
        registry.register(node_info);
    }
    let transport = Arc::new(SshTransport::new());
    Ok(NodeRunState { registry, transport })
}
```

### SSH auth detection

`SshTransport` should auto-detect auth method:
1. Check `~/.ssh/id_ed25519`, `~/.ssh/id_rsa` (key auth)
2. Check `SSH_AUTH_SOCK` (agent auth)
3. Fall back to config `auth_token` if provided (password/token)

This is already implemented in `SshTransport` — just needs to be instantiated.

### Verification

- Configure `[fleet]` in config.toml with a test node
- Run `fawx tui` → agent should have `node_run` in tool list
- Test: `node_run` with a simple command → verify SSH connection + output
- Without fleet config: tool should not appear in list (no "not configured" errors)

## Files touched

| File | Change |
|------|--------|
| `tui.rs` | Add `build_node_run_state()`, call `with_node_run()` in `build_skill_registry()` |
| Tests | Unit test for `build_node_run_state` with mock config |

## Security

- SSH connections use existing host key verification (system `known_hosts`)
- Credentials from config only — agent cannot specify SSH credentials
- Working directory enforcement inherited from node config
- Command execution is already guarded by the agent's tool calling (same as `run_command`)
- Fleet node config is in config.toml (operator-controlled, not agent-writable)
