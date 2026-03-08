# Spec: Phase 0 PR 3 — node_run Tool Wiring

**Gap:** node_run tool exists but NodeRunState is never attached  
**Estimated size:** ~150 lines  
**Risk:** Low — plumbing from config to tool

---

## Problem

`fx-tools/src/node_run.rs` has a complete `node_run` tool implementation with:
- `NodeRunState` (holds `Arc<RwLock<NodeRegistry>>` + `Arc<dyn NodeTransport>`)
- `handle_node_run()` async handler
- `node_run_tool_definition()` 
- `FawxToolExecutor.with_node_run()` setter

`SshTransport` is implemented in `fx-fleet/src/ssh.rs` with:
- SSH key auth (per-node override or default key)
- `StrictHostKeyChecking=accept-new`, `BatchMode=yes`, `ConnectTimeout=10`
- Auth failure classification (`TransportError::AuthFailed` vs `Unreachable`)
- Timeout handling, kill-on-drop

But `with_node_run()` is **never called** — not in `build_skill_registry()`, 
not in `main.rs`, nowhere. The tool is registered in the definition list but 
execution always returns "node_run not configured."

## Solution

### Config → NodeInfo mapping

`NodeConfig` (fx-config) and `NodeInfo` (fx-fleet) have near-identical fields
with different names:

| NodeConfig | NodeInfo | Notes |
|-----------|----------|-------|
| `id` | `node_id` | rename |
| `name` | `name` | same |
| `endpoint` | `endpoint` | same |
| `auth_token` | `auth_token` | same |
| `capabilities: Vec<String>` | `capabilities: Vec<NodeCapability>` | parse strings to enum |
| `address` | `address` | same (Option) |
| `user` | `ssh_user` | rename |
| `ssh_key` | `ssh_key` | same (Option) |
| — | `status` | default `NodeStatus::Online` |
| — | `last_heartbeat_ms` | default 0 |
| — | `registered_at_ms` | default now |

Add `impl From<&NodeConfig> for NodeInfo` in fx-fleet (~15 lines).

### Default SSH key path

`SshTransport::new(key_path: PathBuf)` takes a default key path. Per-node 
`ssh_key` in NodeConfig overrides this in `base_command()`.

**Decision:** Default to `~/.ssh/id_ed25519`. No fleet-level config field 
needed until someone asks for it.

```rust
let default_key = dirs::home_dir()
    .unwrap_or_else(|| PathBuf::from("."))
    .join(".ssh/id_ed25519");
let transport = Arc::new(SshTransport::new(default_key));
```

### Wire in build_skill_registry

In `build_skill_registry()` (`tui.rs`), after creating `FawxToolExecutor`:

```rust
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
fn build_node_run_state(config: &FawxConfig) -> anyhow::Result<NodeRunState> {
    let mut registry = NodeRegistry::new();
    for node_config in &config.fleet.nodes {
        let node_info = NodeInfo::from(node_config);
        registry.register(node_info);
    }
    
    let default_key = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".ssh/id_ed25519");
    let transport = Arc::new(SshTransport::new(default_key));
    
    Ok(NodeRunState {
        registry: Arc::new(RwLock::new(registry)),
        transport: transport as Arc<dyn NodeTransport>,
    })
}
```

Wire in **both** TUI (`build_skill_registry`) and HTTP (`build_headless_startup`) paths.

### Verification

- Configure `[fleet]` in config.toml with a test node
- Run `fawx tui` → agent should have `node_run` in tool list
- Test: `node_run` with a simple command → verify SSH connection + output
- Without fleet config: `with_node_run()` not called, tool doesn't appear

## Implementation Gate

### Gate 1: NodeCapability parsing
`NodeConfig.capabilities` is `Vec<String>` but `NodeInfo.capabilities` is 
`Vec<NodeCapability>` (an enum). If `NodeCapability` doesn't have a 
`from_str()` or the strings don't map cleanly, **stop and report** — 
we may need to change NodeConfig to use the enum directly.

## Files touched

| File | Change |
|------|--------|
| `fx-fleet/src/lib.rs` | `impl From<&NodeConfig> for NodeInfo` |
| `tui.rs` | `build_node_run_state()`, call `with_node_run()` in `build_skill_registry()` |
| `main.rs` or `http_serve.rs` | Same wiring for headless/HTTP path |
| Tests | `build_node_run_state` with mock config, `From<NodeConfig>` mapping |

## Security

- SSH connections use `BatchMode=yes` (no interactive password prompts)
- `StrictHostKeyChecking=accept-new` (TOFU model — first connect accepted, changes rejected)
- Credentials from config only — agent cannot specify SSH credentials or key paths
- Command execution is guarded by agent's tool calling (same trust model as `run_command`)
- Fleet node config is in config.toml (operator-controlled, not agent-writable)
