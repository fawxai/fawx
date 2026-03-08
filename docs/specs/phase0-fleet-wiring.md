# Spec: Phase 0 PR 6 — fx-fleet Wiring

**Gap:** Complete distributed orchestration system exists as pure library code  
**Estimated size:** ~500 lines  
**Risk:** High — largest Phase 0 item, connects distributed system  
**Depends on:** PR 5 (ChannelRegistry) for clean message routing

---

## Problem

Three crates form a complete distributed orchestration system:
- `fx-fleet/NodeRegistry` — node registration, heartbeat, stale detection
- `fx-fleet/TaskRouter` — capability-based node selection (round-robin, least-loaded)
- `fx-fleet/Orchestrator` — task lifecycle (submit → route → complete/retry/timeout)

All are pure library code with comprehensive tests but zero integration in any 
CLI path. The architecture exists on paper and in tests; it's never been run.

## Solution

### 1. NodeRegistry from config

On HTTP server startup:
```rust
let mut node_registry = NodeRegistry::new();
for node_config in &config.fleet.nodes {
    node_registry.register(NodeInfo::from(node_config));
}
```

### 2. Heartbeat endpoint

Add HTTP endpoint for node heartbeats:
```
POST /fleet/heartbeat
{
    "node_id": "mac-mini",
    "status": "healthy",
    "load": 0.3,
    "capabilities": ["rust", "macos", "gpu"]
}
```

Node registry updates node status on heartbeat.
Stale detection runs on a configurable interval (default 5min).

### 3. TaskRouter integration

`TaskRouter` needs:
- Access to `NodeRegistry` (for capability + load info)
- Routing strategy from config (default: capability-match)

Wire into the message handling pipeline:
- When the agent calls `node_run` or `spawn_agent` with fleet routing:
  - TaskRouter selects the best node
  - Orchestrator tracks the task lifecycle

### 4. Orchestrator as background service

The Orchestrator manages task state transitions:
```
Submitted → Routed → Running → Completed/Failed/Timeout
```

Run as a tokio background task alongside the HTTP server:
- Process task queue
- Monitor running tasks for timeout
- Handle retry on transient failures

### 5. Fleet status endpoint

```
GET /fleet/status
```

Returns:
```json
{
    "nodes": [
        {"name": "mac-mini", "status": "healthy", "load": 0.3, "last_heartbeat": "2s ago"},
        {"name": "macbook", "status": "stale", "load": null, "last_heartbeat": "12m ago"}
    ],
    "active_tasks": 2,
    "completed_today": 47
}
```

### 6. Config

Already exists in `FawxConfig`:
```rust
pub struct FleetConfig {
    pub nodes: Vec<NodeConfig>,
}

pub struct OrchestratorConfig {
    // Needs investigation — what fields exist?
}
```

### Pre-investigation needed

1. What's the full `Orchestrator` API? Does it need a channel/transport to dispatch?
2. How do tasks get dispatched to nodes? Via SSH (SshTransport)? HTTP? Both?
3. What's the task result reporting path? Does the node push results or does 
   the orchestrator poll?
4. Does `TaskRouter` maintain its own state or is it stateless (pure function of registry)?
5. How does this integrate with `node_run` tool (PR 3)? Should `node_run` go 
   through TaskRouter, or is it direct?

### Phased approach

Given the complexity, consider splitting into two sub-PRs:
- **6a:** NodeRegistry + heartbeat endpoint + fleet status (~250 lines)
- **6b:** TaskRouter + Orchestrator integration (~250 lines)

## Files touched

| File | Change |
|------|--------|
| `http_serve.rs` | Fleet endpoints (heartbeat, status), orchestrator background task |
| `main.rs` | Build fleet components from config |
| `tui.rs` | Potentially add `/fleet` slash command for status |
| Tests | Registry from config, heartbeat processing, task routing |

## Security

- Heartbeat endpoint requires bearer auth (nodes must authenticate)
- Task dispatch only to configured nodes — agent cannot add fleet nodes
- Orchestrator config is operator-controlled (config.toml)
- Node capabilities are self-reported on heartbeat — potential for spoofing, 
  but nodes are already authenticated
- Task timeout prevents resource exhaustion from hung tasks
- Fleet status endpoint is read-only, behind auth
