# Phase 2a: Background Process Management

## Summary

Add background process spawning, monitoring, and lifecycle management to the engine. Agents need to run long-lived commands (servers, builds, watchers) without blocking the conversation loop. Currently `run_command` blocks until completion — a 10-minute build freezes the entire agent.

## Motivation

- **Long builds**: `cargo build --release` takes 5+ minutes. Agent should start it and check later.
- **Dev servers**: `fawx serve`, test servers, file watchers — need to run indefinitely.
- **Parallel work**: Start a build, continue conversation, check result when ready.
- **Autonomous loops**: The experiment loop (#1261) needs to run commands, observe, iterate — can't block on each step.
- Every coding agent (OpenClaw, Cursor, Claude Code) has this capability.

## Design

### New Tools

#### `exec_background`

```json
{
  "name": "exec_background",
  "description": "Start a command in the background. Returns a session ID for monitoring.",
  "parameters": {
    "type": "object",
    "properties": {
      "command": { "type": "string", "description": "Shell command to execute" },
      "working_dir": { "type": "string", "description": "Working directory (defaults to project root)" },
      "label": { "type": "string", "description": "Human-readable label for this process" }
    },
    "required": ["command"]
  }
}
```

Returns:
```json
{
  "session_id": "bg_a1b2c3",
  "pid": 12345,
  "label": "cargo build",
  "status": "running"
}
```

#### `exec_status`

```json
{
  "name": "exec_status",
  "description": "Check status of a background process or list all background processes.",
  "parameters": {
    "type": "object",
    "properties": {
      "session_id": { "type": "string", "description": "Process session ID. If omitted, lists all." },
      "tail": { "type": "integer", "description": "Number of output lines to return (default: 20)" }
    },
    "required": []
  }
}
```

Returns:
```json
{
  "session_id": "bg_a1b2c3",
  "status": "running|completed|failed|killed",
  "exit_code": null,
  "runtime_seconds": 45,
  "output_lines": 1234,
  "tail": ["last 20 lines of output..."]
}
```

Or for list mode (no session_id):
```json
{
  "processes": [
    { "session_id": "bg_a1b2c3", "label": "cargo build", "status": "running", "runtime_seconds": 45 },
    { "session_id": "bg_d4e5f6", "label": "test server", "status": "completed", "exit_code": 0, "runtime_seconds": 120 }
  ]
}
```

#### `exec_kill`

```json
{
  "name": "exec_kill",
  "description": "Kill a background process.",
  "parameters": {
    "type": "object",
    "properties": {
      "session_id": { "type": "string", "description": "Process session ID to kill" }
    },
    "required": ["session_id"]
  }
}
```

### ProcessRegistry (Kernel Layer)

The registry lives in the kernel for safety enforcement:

```rust
pub struct ProcessRegistry {
    processes: Arc<Mutex<HashMap<String, ProcessEntry>>>,
    config: ProcessConfig,
}

pub struct ProcessEntry {
    session_id: String,
    label: String,
    pid: u32,
    child: tokio::process::Child,
    output_buffer: VecDeque<String>,  // Ring buffer, max 10K lines
    started_at: Instant,
    status: ProcessStatus,
    working_dir: PathBuf,
}

pub struct ProcessConfig {
    max_concurrent: usize,     // default: 5
    max_lifetime_secs: u64,    // default: 3600 (1 hour)
    max_output_lines: usize,   // default: 10_000
    allowed_dirs: Vec<PathBuf>,  // working_dir must be within these
}

pub enum ProcessStatus {
    Running,
    Completed { exit_code: i32 },
    Failed { exit_code: i32 },
    Killed,
    TimedOut,
}
```

### Lifecycle

1. **Spawn**: Agent calls `exec_background` → registry validates (concurrent limit, working_dir) → spawns tokio child process → captures stdout/stderr to ring buffer → returns session_id
2. **Monitor**: Agent calls `exec_status` → registry returns current status + tail of output
3. **Complete**: Process exits → registry updates status, preserves output buffer
4. **Kill**: Agent calls `exec_kill` → SIGTERM → 5s grace → SIGKILL
5. **Timeout**: Background task checks lifetime → auto-kill processes exceeding max_lifetime
6. **Cleanup**: On engine shutdown, kill all background processes (SIGTERM → SIGKILL)

### Output Capture

- stdout and stderr are merged into a single stream (like a terminal)
- Ring buffer with configurable max lines (default 10K)
- When buffer is full, oldest lines are dropped
- Agent can request any number of tail lines via `exec_status`
- Full output is NOT persisted to disk (too large, too transient)

## Security

### Kernel-Level Enforcement

ProcessRegistry is a kernel component, not a loadable skill:

1. **Concurrent limit**: Max 5 simultaneous background processes. Hard-coded ceiling in kernel, configurable below.
2. **Lifetime limit**: Default 1 hour max. Prevents forgotten processes consuming resources.
3. **Working directory**: Must resolve within allowed directories (project root, home dir). No `/tmp` escape.
4. **No privilege escalation**: Processes run as the same user as the engine. No sudo passthrough.
5. **Cleanup on shutdown**: All background processes are terminated when the engine exits.
6. **Session isolation**: Each background process gets a unique session_id. No PID exposure beyond the initial spawn response.

### ProposalGateExecutor

`exec_background` is NOT a write tool (it doesn't modify files directly). It follows the same security model as `run_command` — the command itself may write files, but that's the command's business, not the tool's.

However: if self-modify policy is active, the agent's background commands could modify protected files. This is an accepted risk — the same risk exists with `run_command` today. Future: integrate process output monitoring with canary signals.

### What This Does NOT Include

- **PTY/terminal emulation**: No interactive terminals. Background processes are non-interactive (stdin closed).
- **Port forwarding/networking**: No exposing process ports. Processes bind what they bind.
- **Process groups**: No grouping, no dependencies between processes.
- **Persistent processes**: Processes don't survive engine restart. This is intentional — persistent daemons are a systemd/launchd concern.

## Implementation

### New Crate: None

This lives in `fx-kernel` (ProcessRegistry) + `fx-tools` (tool handlers):

### Files to Modify

1. **`engine/crates/fx-kernel/src/process_registry.rs`** (new)
   - `ProcessRegistry`, `ProcessEntry`, `ProcessConfig`, `ProcessStatus`
   - `spawn()`, `status()`, `list()`, `kill()`, `cleanup_expired()`
   - Background task for lifetime enforcement
   - Shutdown handler

2. **`engine/crates/fx-kernel/src/lib.rs`**
   - Export `ProcessRegistry`, `ProcessConfig`

3. **`engine/crates/fx-tools/src/tools.rs`**
   - Add `exec_background`, `exec_status`, `exec_kill` to `fawx_tool_definitions()`
   - Add handler methods: `handle_exec_background()`, `handle_exec_status()`, `handle_exec_kill()`
   - `FawxToolExecutor` needs access to `Arc<ProcessRegistry>` — add field or inject via constructor

4. **`engine/crates/fx-cli/src/startup.rs`**
   - Create `ProcessRegistry` during engine setup
   - Pass to `FawxToolExecutor`

5. **Config struct**
   - Add `ProcessConfig` section with defaults

### Tests Required

**ProcessRegistry:**
- Spawn process, verify running status
- Process completes, verify exit code
- Kill process, verify killed status
- Concurrent limit enforced (reject 6th)
- Lifetime timeout kills process
- Output ring buffer captures stdout
- Output ring buffer drops oldest when full
- List returns all processes
- Cleanup on drop kills all
- Working directory validation
- Session ID uniqueness

**Tool handlers:**
- `exec_background` returns session_id
- `exec_status` with session_id returns tail
- `exec_status` without session_id lists all
- `exec_kill` sends SIGTERM
- Invalid session_id returns error
- `exec_background` with working_dir validates path

## Size Estimate

~400-500 lines of implementation + ~300 lines of tests. Single PR, possibly split into 2 (registry + tools).

## Dependencies

None new — uses `tokio::process` (already available).
