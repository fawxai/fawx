# Spec: `fawx sessions` CLI Commands

## Motivation

No way to inspect session data from the command line. The HTTP API has endpoints
(`GET /sessions`, `GET /sessions/{id}/messages`) but there's no CLI command to
use them. Joe needs to debug Fawx agent behavior by seeing what happened in a
session — tool calls, model responses, decision points.

## Commands

### `fawx sessions list`

List all sessions with summary info.

**Output (human-readable by default):**
```
ID                                   KIND      STATUS   MODEL          MESSAGES  UPDATED            LABEL
a1b2c3d4-...                         main      active   claude-3.5     42        2026-03-19 23:30   primary
e5f6g7h8-...                         subagent  completed gpt-4         8         2026-03-19 22:15   reviewer
```

**Flags:**
- `--json` — output JSON array of `SessionInfo` objects
- `--kind <kind>` — filter by kind (main, subagent, channel, cron)

### `fawx sessions export <id>`

Dump full conversation transcript for a session.

**Output (human-readable by default):**
```
Session: a1b2c3d4-...
Kind: main | Status: active | Model: claude-3.5
Created: 2026-03-19 20:00 | Updated: 2026-03-19 23:30
Messages: 42
---

[user] 2026-03-19 20:00:15
What's the weather?

[assistant] 2026-03-19 20:00:18
I'll check that for you.

[tool_call] weather_lookup {"location": "Denver"}

[tool_result] {"temp": "45F", "condition": "clear"}

[assistant] 2026-03-19 20:00:20
It's 45°F and clear in Denver.
```

**Flags:**
- `--json` — output JSON array of messages
- `--limit <n>` — only show last N messages (default: all)

## Implementation

### 1. Add `Sessions` subcommand to `Commands` enum in `main.rs`

```rust
/// Manage conversation sessions
Sessions {
    #[command(subcommand)]
    command: SessionsCommands,
},
```

### 2. Add `SessionsCommands` enum

```rust
#[derive(Subcommand)]
enum SessionsCommands {
    /// List all sessions
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Filter by session kind
        #[arg(long)]
        kind: Option<String>,
    },
    /// Export full conversation from a session
    Export {
        /// Session ID
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Limit to last N messages
        #[arg(long)]
        limit: Option<usize>,
    },
}
```

### 3. Create `engine/crates/fx-cli/src/commands/sessions.rs`

This module reads directly from the session registry (redb database at
`~/.fawx/sessions.redb`). No running server required — works offline.

**`list()`**: Open registry, call `registry.list(filter)`, format output.

**`export()`**: Open registry, call `registry.history(key, limit)`, format output
including role, content, and timestamp for each message.

### 4. Wire dispatch in `main.rs`

```rust
Commands::Sessions { command } => dispatch_sessions(command),
```

## Key Design Decisions

- **Direct registry access, not HTTP**: The commands should work even when the
  server isn't running. The session database (redb) supports concurrent readers,
  so this is safe alongside a running server.
- **Human-readable default, JSON opt-in**: Matches the pattern of other CLI tools.
  `--json` gives machine-parseable output for piping.
- **No mutations**: These are read-only inspection commands. No create/delete/send
  from the CLI (those exist via HTTP API).

## Files to Create/Modify

1. **Create** `engine/crates/fx-cli/src/commands/sessions.rs` — implementation
2. **Modify** `engine/crates/fx-cli/src/commands/mod.rs` — add `pub mod sessions;`
3. **Modify** `engine/crates/fx-cli/src/main.rs` — add `Sessions` variant + dispatch

## Tests

1. `list_sessions_empty_registry` — returns empty list without error
2. `list_sessions_with_filter` — kind filter works
3. `export_session_shows_messages` — messages render in order with role/content
4. `export_nonexistent_session_returns_error` — clean error message
5. `export_with_limit` — only shows last N messages

## Verification

```bash
cargo fmt --all
cargo clippy --workspace --tests -- -D warnings
cargo test --workspace
```

After push, verify: `git log --oneline origin/<branch> -3`
If your commit is not visible, the push failed — do not report success.
