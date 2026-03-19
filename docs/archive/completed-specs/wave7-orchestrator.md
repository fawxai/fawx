# Wave 7 Item #6: Orchestrator (fx-orchestrator)

## Overview

The orchestrator is the coordination layer that ties together channels, fleet, and task routing. It receives messages from channels, routes tasks to nodes, manages node lifecycle, and delivers responses back through the originating channel.

This is a **library crate** (not a binary). It provides the `Orchestrator` struct that fx-cli wires into its runtime. The orchestrator does not own the event loop — it provides methods that fx-cli calls.

## 1. Crate Structure

```
engine/crates/fx-orchestrator/
├── Cargo.toml
└── src/
    └── lib.rs
```

Dependencies: `fx-core`, `fx-fleet`, `fx-channel-webhook`, `serde`, `serde_json`.
NO async runtime (no tokio). NO networking (no reqwest, no axum). Pure coordination logic.

The actual HTTP calls to remote nodes happen in fx-cli. The orchestrator decides WHERE to route — fx-cli handles HOW to deliver.

## 2. Core Types

```rust
/// A task to be routed and executed.
#[derive(Debug, Clone)]
pub struct Task {
    /// Unique task identifier.
    pub task_id: String,
    /// The message/prompt to process.
    pub message: String,
    /// Which channel this task originated from.
    pub source: InputSource,
    /// Required node capabilities for this task.
    pub requirements: TaskRequirements,
    /// Maximum retries on failure (0 = no retry). Each retry re-submits
    /// through TaskRouter::select(), which naturally picks a different
    /// node since the failed one's status will have changed.
    pub max_retries: u32,
    /// Task timeout in milliseconds. If the task hasn't completed within
    /// this window, check_timeouts() marks it as Failed and triggers
    /// retry if retries remain. 0 = no timeout.
    pub timeout_ms: u64,
    /// When this task was submitted (unix ms). Set by submit().
    pub submitted_at_ms: u64,
    /// How many retries have been attempted so far.
    pub retries_attempted: u32,
}

/// Result of task execution.
#[derive(Debug, Clone)]
pub enum TaskResult {
    /// Task completed with a response.
    Completed {
        task_id: String,
        response: String,
        node_id: String,
    },
    /// Task failed.
    Failed {
        task_id: String,
        error: String,
        node_id: Option<String>,
    },
    /// No node available to handle this task.
    NoNode {
        task_id: String,
        reason: String,
    },
}

impl TaskResult {
    pub fn task_id(&self) -> &str;
    pub fn is_success(&self) -> bool;
}

/// What happened when complete() was called.
#[derive(Debug, Clone)]
pub enum CompletionOutcome {
    /// Response delivered to channel.
    Delivered(String),       // channel_id
    /// Task failed but retried on a new node.
    Retried(RoutingDecision),
    /// All retries exhausted, task abandoned.
    Exhausted(String, String), // task_id, final error
}

/// Errors from orchestrator operations.
#[derive(Debug, Clone)]
pub enum OrchestratorError {
    /// No node available for the task requirements.
    NoNodeAvailable(String),
    /// Channel not found for response delivery.
    ChannelNotFound(String),
    /// Task routing failed.
    RoutingFailed(String),
}
```

## 3. Orchestrator

```rust
use fx_fleet::{NodeRegistry, TaskRouter, TaskRequirements};
use fx_core::channel::Channel;
use fx_core::types::InputSource;
use std::collections::HashMap;
use std::sync::Arc;

/// Coordinates task routing, node management, and response delivery.
pub struct Orchestrator {
    /// Node registry — who's online and what they can do.
    registry: NodeRegistry,
    /// Channel map — InputSource → Channel for response routing.
    channels: HashMap<String, Arc<dyn Channel>>,
    /// Pending tasks awaiting results.
    pending: HashMap<String, Task>,
    /// Whether this node is the coordinator.
    is_coordinator: bool,
}
```

### Constructor + Configuration

```rust
impl Orchestrator {
    /// Create a new orchestrator.
    pub fn new(is_coordinator: bool) -> Self;

    /// Register a channel for response routing.
    /// The key is the channel id.
    pub fn register_channel(&mut self, channel: Arc<dyn Channel>);

    /// Remove a channel by id.
    pub fn remove_channel(&mut self, channel_id: &str);

    /// Get a reference to the node registry for external heartbeat/registration.
    pub fn registry(&self) -> &NodeRegistry;

    /// Get a mutable reference to the node registry.
    pub fn registry_mut(&mut self) -> &mut NodeRegistry;
}
```

### Task Lifecycle

```rust
impl Orchestrator {
    /// Submit a task for routing.
    ///
    /// 1. Finds the best node via TaskRouter::select()
    /// 2. Records the task as pending
    /// 3. Returns the routing decision (caller handles actual delivery to node)
    ///
    /// Returns the selected NodeInfo so the caller (fx-cli) can make the HTTP
    /// call. The orchestrator doesn't do networking.
    pub fn submit(&mut self, task: Task) -> Result<RoutingDecision, OrchestratorError>;

    /// Record a task result and route the response to the originating channel.
    ///
    /// On success (Completed):
    /// 1. Looks up the pending task by task_id
    /// 2. Finds the channel matching the task's InputSource
    /// 3. Calls channel.send_response() with the response text
    /// 4. Removes the task from pending
    /// 5. Returns Ok(Delivered(channel_id))
    ///
    /// On failure (Failed) with retries remaining:
    /// 1. Increments retries_attempted
    /// 2. Re-submits through submit() (TaskRouter picks a new node)
    /// 3. Returns Ok(Retried(new_routing_decision))
    ///
    /// On failure with no retries remaining:
    /// 1. Removes from pending
    /// 2. Returns Ok(Exhausted(task_id, error))
    pub fn complete(&mut self, result: TaskResult) -> Result<CompletionOutcome, OrchestratorError>;

    /// Get a pending task by id.
    pub fn pending(&self, task_id: &str) -> Option<&Task>;

    /// Number of pending tasks.
    pub fn pending_count(&self) -> usize;
}
```

### Node Lifecycle

```rust
impl Orchestrator {
    /// Run stale detection on the registry.
    /// Returns list of node IDs that became stale.
    pub fn check_stale(&mut self, now_ms: u64) -> Vec<String>;

    /// Scan pending tasks for timeouts. Same pattern as check_stale():
    /// compare submitted_at_ms + timeout_ms against now_ms.
    ///
    /// Timed-out tasks with retries remaining get re-submitted.
    /// Timed-out tasks with no retries get removed and returned as failed.
    ///
    /// Returns list of (task_id, outcome) for each timed-out task.
    pub fn check_timeouts(&mut self, now_ms: u64) -> Vec<(String, CompletionOutcome)>;

    /// Get a summary of fleet status.
    pub fn fleet_status(&self) -> FleetStatus;
}

/// Summary of fleet health.
#[derive(Debug, Clone)]
pub struct FleetStatus {
    pub online: usize,
    pub stale: usize,
    pub offline: usize,
    pub busy: usize,
    pub total: usize,
}
```

## 4. OrchestratorConfig (add to fx-config)

```toml
[orchestrator]
enabled = false
max_pending_tasks = 100
default_timeout_ms = 30000
default_max_retries = 1
```

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct OrchestratorConfig {
    /// Whether the orchestrator is enabled.
    pub enabled: bool,
    /// Maximum number of pending tasks before rejecting new ones.
    pub max_pending_tasks: usize,
    /// Default task timeout in milliseconds (0 = no timeout).
    pub default_timeout_ms: u64,
    /// Default max retries for tasks (0 = no retry).
    pub default_max_retries: u32,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_pending_tasks: 100,
            default_timeout_ms: 30_000,
            default_max_retries: 1,
        }
    }
}
```

## 5. Key Design Decisions

### Why no async?
The orchestrator is a synchronous state machine. It tracks what's pending, routes tasks, and delivers responses. The async parts (HTTP calls to nodes, webhook delivery) live in fx-cli. Clean separation: orchestrator = decisions, fx-cli = I/O.

### Why not a binary?
It's a library because it needs to be embedded in the `fawx serve` runtime. The coordinator is just a mode of the existing `fawx` binary, not a separate process.

### Why HashMap for channels, not ChannelRegistry from fx-kernel?
The orchestrator needs ownership of channel references for response routing. It uses its own lightweight map keyed by channel ID. ChannelRegistry in fx-kernel serves the kernel's needs; the orchestrator has its own. No coupling.

### What does NOT go here
- HTTP client calls to remote nodes (fx-cli)
- Webhook server/listener setup (fx-cli)
- Authentication/authorization (fx-cli's bearer token layer)
- Task queueing beyond simple pending tracking (future work)
- Load balancing beyond TaskRouter's selection (future work)

## 6. Tests (10 required)

1. `submit_routes_to_capable_node` — submit task with capability requirement, get RoutingDecision::Routed
2. `submit_no_node_returns_error` — no nodes registered, submit returns NoNodeAvailable
3. `complete_delivers_to_channel` — submit task from a channel, complete with Completed result, verify send_response() called with correct message, returns Delivered(channel_id)
4. `complete_unknown_task_returns_error` — complete with unknown task_id returns error
5. `complete_retries_on_failure` — submit task with max_retries=2, complete with Failed, returns Retried with new routing decision, retries_attempted incremented
6. `complete_exhausted_after_max_retries` — submit with max_retries=1, fail twice, second failure returns Exhausted
7. `check_timeouts_marks_expired` — submit task with timeout_ms=1000, advance time by 2000ms, check_timeouts returns the task as timed out
8. `check_timeouts_retries_if_allowed` — submit task with timeout_ms=1000 and max_retries=1, timeout triggers retry (Retried), not Exhausted
9. `check_stale_delegates_to_registry` — register nodes, advance time past threshold, check_stale returns stale node ids
10. `fleet_status_counts_correctly` — register mix of Online/Stale/Offline/Busy nodes, verify FleetStatus

## 7. What This Enables

With the orchestrator, Fawx can:
- Receive a message from any channel (TUI, HTTP, webhook)
- Determine which node should handle it (based on capabilities, load, preference)
- Track the task while it executes
- Deliver the response back to the originating channel

This is the coordination backbone for SuperFawx — distributed agentic computing across multiple nodes.

## 8. File Changes

```
engine/crates/fx-orchestrator/Cargo.toml        — NEW
engine/crates/fx-orchestrator/src/lib.rs         — NEW
engine/crates/fx-config/src/lib.rs               — add OrchestratorConfig
engine/Cargo.toml                                — add fx-orchestrator to workspace
```
