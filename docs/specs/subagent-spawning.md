# Spec: Subagent Spawning

**Status:** Draft  
**Author:** Clawdio  
**Date:** 2026-03-08  
**Target:** `engine/crates/fx-subagent/`  

---

## 1. Problem

Fawx can answer questions and use tools, but it cannot delegate work. A capable agent needs to spawn isolated child agents to handle subtasks — parallel research, code implementation, reviews — while the parent agent orchestrates.

Without this, Fawx is a chatbot with tools, not an agentic engine.

## 2. Goals

1. **Spawn isolated agent instances** from within a running Fawx session
2. **Parent-child lifecycle management** — spawn, monitor, get results, cancel
3. **Resource isolation** — each subagent gets its own conversation, memory scope, and tool access
4. **Resource limits** — max concurrent, timeout, token budget per subagent
5. **Two modes:** `run` (one-shot, returns result) and `session` (persistent, interactive)

## 3. Non-Goals (for now)

- Cross-machine spawning (fx-fleet integration — Phase 5)
- Subagent-to-subagent communication
- Nested subagent spawning (subagents spawning their own subagents)
- GUI/TUI for subagent management

## 4. Architecture

### 4.1 New Crate: `fx-subagent`

```
engine/crates/fx-subagent/
├── src/
│   ├── lib.rs          # Public API
│   ├── manager.rs      # SubagentManager — lifecycle orchestration
│   ├── instance.rs     # SubagentInstance — isolated HeadlessApp wrapper
│   ├── config.rs       # SpawnConfig, resource limits
│   └── handle.rs       # SubagentHandle — parent's view of a running subagent
└── Cargo.toml
```

### 4.2 Core Types

```rust
/// Configuration for spawning a subagent.
pub struct SpawnConfig {
    /// Human-readable label for identification.
    pub label: Option<String>,
    /// The task/prompt to send to the subagent.
    pub task: String,
    /// Model override (uses parent's model if None).
    pub model: Option<String>,
    /// Execution mode.
    pub mode: SpawnMode,
    /// Maximum execution time.
    pub timeout: Duration,
    /// Maximum tokens the subagent may consume.
    pub max_tokens: Option<u64>,
    /// Working directory for tool execution.
    pub cwd: Option<PathBuf>,
    /// System prompt override.
    pub system_prompt: Option<String>,
}

pub enum SpawnMode {
    /// One-shot: send task, get result, subagent exits.
    Run,
    /// Persistent: stays alive for follow-up messages.
    Session,
}

/// Parent's handle to a running subagent.
pub struct SubagentHandle {
    pub id: SubagentId,
    pub label: Option<String>,
    pub status: SubagentStatus,
    pub mode: SpawnMode,
    pub started_at: Instant,
}

pub enum SubagentStatus {
    Running,
    Completed { result: String, tokens_used: u64 },
    Failed { error: String },
    Cancelled,
    TimedOut,
}

/// Unique identifier for a subagent instance.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SubagentId(pub String); // UUID
```

### 4.3 SubagentManager

The central coordinator. Owned by the parent `HeadlessApp` (or the HTTP server for headless mode).

```rust
pub struct SubagentManager {
    /// Active subagent instances.
    instances: HashMap<SubagentId, SubagentInstance>,
    /// Configuration limits.
    limits: SubagentLimits,
    /// Shared router for LLM access.
    router: Arc<ModelRouter>,
    /// Shared config for defaults.
    config: Arc<FawxConfig>,
}

pub struct SubagentLimits {
    /// Maximum concurrent subagents (default: 5).
    pub max_concurrent: usize,
    /// Default timeout per subagent (default: 10 min).
    pub default_timeout: Duration,
    /// Maximum total token budget across all subagents (None = unlimited).
    pub total_token_budget: Option<u64>,
}

impl SubagentManager {
    /// Spawn a new subagent. Returns a handle for monitoring.
    pub async fn spawn(&mut self, config: SpawnConfig) -> Result<SubagentHandle, SubagentError>;

    /// Send a follow-up message to a Session-mode subagent.
    pub async fn send(&self, id: &SubagentId, message: &str) -> Result<String, SubagentError>;

    /// Get current status of a subagent.
    pub fn status(&self, id: &SubagentId) -> Option<&SubagentStatus>;

    /// List all subagents (active + recent).
    pub fn list(&self) -> Vec<SubagentHandle>;

    /// Cancel a running subagent.
    pub async fn cancel(&mut self, id: &SubagentId) -> Result<(), SubagentError>;

    /// Clean up completed/timed-out subagents older than `max_age`.
    pub fn gc(&mut self, max_age: Duration);
}
```

### 4.4 SubagentInstance (internal)

Each subagent wraps an isolated `HeadlessApp` running in its own tokio task.

```rust
struct SubagentInstance {
    id: SubagentId,
    config: SpawnConfig,
    /// Channel to send messages to the subagent's task.
    tx: mpsc::Sender<SubagentCommand>,
    /// Channel to receive results/status updates.
    rx: watch::Receiver<SubagentStatus>,
    /// Join handle for the tokio task.
    task_handle: JoinHandle<()>,
    started_at: Instant,
}

enum SubagentCommand {
    /// Send a message for processing.
    Message(String),
    /// Cancel execution.
    Cancel,
}
```

**Isolation guarantees:**
- Own `HeadlessApp` instance (own conversation history, own memory)
- Shares the parent's `ModelRouter` (same API keys, same providers)
- Shares the parent's `LoopEngine` (same tools, same kernel policy)
- Own tokio task — does not block the parent
- Own cancellation token — parent can kill without affecting itself

## 5. Tool Interface

### 5.1 `spawn_agent` tool

Registered as a kernel-level tool (not a skill — it needs SubagentManager access).

```json
{
  "name": "spawn_agent",
  "description": "Spawn an isolated subagent to handle a task. Returns a subagent ID for monitoring.",
  "parameters": {
    "type": "object",
    "properties": {
      "task": {
        "type": "string",
        "description": "The task or prompt for the subagent"
      },
      "label": {
        "type": "string",
        "description": "Human-readable label for identification"
      },
      "model": {
        "type": "string",
        "description": "Model override (e.g. 'claude-sonnet-4-6'). Uses parent model if omitted."
      },
      "mode": {
        "type": "string",
        "enum": ["run", "session"],
        "description": "run = one-shot (default), session = persistent"
      },
      "timeout_seconds": {
        "type": "integer",
        "description": "Maximum execution time in seconds (default: 600)"
      },
      "cwd": {
        "type": "string",
        "description": "Working directory for the subagent"
      }
    },
    "required": ["task"]
  }
}
```

### 5.2 `subagent_status` tool

```json
{
  "name": "subagent_status",
  "description": "Check status of a subagent, list all subagents, or cancel one.",
  "parameters": {
    "type": "object",
    "properties": {
      "action": {
        "type": "string",
        "enum": ["status", "list", "cancel", "send"],
        "description": "Action to perform"
      },
      "id": {
        "type": "string",
        "description": "Subagent ID (required for status/cancel/send)"
      },
      "message": {
        "type": "string",
        "description": "Message to send (required for send action)"
      }
    },
    "required": ["action"]
  }
}
```

## 6. Execution Flow

### 6.1 `run` mode (one-shot)

```
Parent calls spawn_agent(task="Review this PR", mode="run")
  → SubagentManager creates SubagentInstance
  → Spawns tokio task with isolated HeadlessApp
  → HeadlessApp.process_message(task) runs the agentic loop
  → Loop completes → result sent via watch channel
  → Parent reads result via subagent_status(action="status", id=...)
  → SubagentManager marks as Completed, stores result
```

### 6.2 `session` mode (persistent)

```
Parent calls spawn_agent(task="Help me debug this", mode="session")
  → Same as run, but after first response, task stays alive
  → Parent calls subagent_status(action="send", id=..., message="Try X")
  → Subagent processes follow-up, returns response
  → Continues until cancel or timeout
```

### 6.3 Completion notification

When a `run`-mode subagent completes, the manager should inject a system event into the parent's conversation to notify it:

```
[System: Subagent "Review PR" (id: abc123) completed]
Result: APPROVE — all sections clean. No issues found.
```

This lets the parent's LLM react to the completion without polling.

## 7. Resource Management

### 7.1 Limits enforcement

| Limit | Default | Configurable |
|-------|---------|-------------|
| Max concurrent | 5 | `[subagents] max_concurrent` |
| Default timeout | 10 min | `[subagents] default_timeout_seconds` |
| Max timeout | 30 min | `[subagents] max_timeout_seconds` |
| Token budget per subagent | None | Per-spawn `max_tokens` |
| Total token budget | None | `[subagents] total_token_budget` |

### 7.2 Timeout handling

- Each subagent task has a `tokio::time::timeout` wrapper
- On timeout: cancel the subagent's agentic loop, set status to `TimedOut`, notify parent
- Graceful shutdown: send cancel signal, wait 5s, then abort task

### 7.3 Cleanup

- `SubagentManager::gc()` called periodically (every 60s)
- Completed/failed/cancelled subagents kept for 30 min (for result retrieval), then removed
- On parent shutdown: cancel all running subagents

## 8. Config

```toml
[subagents]
enabled = true
max_concurrent = 5
default_timeout_seconds = 600
max_timeout_seconds = 1800
# total_token_budget = 100000  # optional
```

## 9. Testing Requirements

### Unit tests
- Spawn and complete a run-mode subagent
- Spawn and interact with a session-mode subagent
- Max concurrent limit enforced (spawn beyond limit → error)
- Timeout triggers cancellation
- Cancel running subagent
- GC removes old completed instances
- Status tracking through lifecycle (Running → Completed/Failed/TimedOut/Cancelled)

### Integration tests
- Subagent processes a message through a mock LLM provider
- Subagent uses tools (mock tools)
- Parent receives completion notification
- Multiple concurrent subagents don't interfere

## 10. Implementation Order

1. **`fx-subagent` crate** — types, SubagentManager, SubagentInstance
2. **Wire to HeadlessApp** — HeadlessApp holds SubagentManager
3. **`spawn_agent` tool** — register in tool registry
4. **`subagent_status` tool** — register in tool registry
5. **Completion notifications** — inject system events on subagent completion
6. **Config support** — `[subagents]` section in config.toml
7. **HTTP endpoints** — `/subagents` list/status (optional, for monitoring)

## 11. File Touchpoints

- **New:** `engine/crates/fx-subagent/` (entire crate)
- **Modify:** `engine/crates/fx-cli/src/headless.rs` (add SubagentManager)
- **Modify:** `engine/crates/fx-core/src/config.rs` (add SubagentConfig)
- **Modify:** `engine/crates/fx-core/src/tools/` (register spawn_agent, subagent_status)
- **Modify:** `engine/Cargo.toml` (add fx-subagent to workspace)
- **Modify:** `engine/crates/fx-cli/Cargo.toml` (depend on fx-subagent)

## 12. Open Questions

1. Should subagents inherit the parent's conversation history? (Proposed: no — clean slate with just the task)
2. Should subagents have access to all tools or a restricted set? (Proposed: all tools, but no nested spawn_agent)
3. Should subagent results be persisted to disk? (Proposed: no for v1, in-memory only)
4. Token counting — do we track at the router level? (Proposed: yes, router returns token counts from LLM responses)

---

*This spec targets the minimum viable subagent system. Cross-machine spawning (fx-fleet), nested subagents, and persistent subagent sessions are future work.*
