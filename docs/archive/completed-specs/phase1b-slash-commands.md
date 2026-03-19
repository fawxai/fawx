# Spec: Phase 1b — Server-Side Slash Commands + /config reload (#1064)

**Gap:** Slash commands only work in legacy TUI (`tui.rs`). New ratatui TUI (`fawx-tui`) sends everything via HTTP `/message`, which passes all input to the agentic loop. No commands work.
**Also resolves:** #1064 — `/config reload` for hot-reloading config.toml
**Also resolves:** #1186 — Config defaults (validate via /config reload path)
**Estimated size:** ~500 lines (extraction + new handlers + tests)
**Risk:** Medium — refactors command parsing out of tui.rs monolith

---

## Problem

`tui.rs` (3,600+ lines) contains:
- `parse_command()` — parses `/foo` into `ParsedCommand` enum
- `ParsedCommand` enum — ~25 variants
- Handler methods on `TuiApp` — each variant dispatches to a handler

`http_serve.rs` `handle_message()` sends everything to `process_and_route_message()`. No slash command interception.

Result: `/proposals`, `/approve`, `/model`, `/config`, `/help`, `/status`, `/thinking` — none work from `fawx-tui` or Telegram.

---

## Solution

### New module: `engine/crates/fx-cli/src/commands.rs`

Extract from `tui.rs`:
1. `ParsedCommand` enum (move, not copy)
2. `parse_command()` function (move, not copy)
3. Helper parsers: `parse_approve_command()`, `parse_reject_command()`, `parse_improve_flags()`

Add new:
4. `CommandResult` — response type for server-side command execution
5. `execute_command()` — dispatches `ParsedCommand` variants that can run server-side

```rust
pub struct CommandResult {
    pub response: String,
}

pub struct CommandContext<'a> {
    pub app: &'a mut HeadlessApp,
}

/// Returns Some(CommandResult) for server-handled commands,
/// None for client-only commands (Quit, Clear, New, History)
/// or plain messages (Unknown with no slash prefix).
pub fn execute_command(
    ctx: &mut CommandContext<'_>,
    command: &ParsedCommand,
) -> Option<Result<CommandResult, anyhow::Error>>
```

### Commands that move server-side

These return text responses — no TUI state needed:

| Command | Current handler | Notes |
|---------|----------------|-------|
| `/proposals` | `handle_proposals_command()` | Uses `proposal_review::render_pending()` — needs `ReviewContext` from config |
| `/approve <id> [--force]` | `handle_approve_command()` | Uses `proposal_review::approve_pending()` |
| `/reject <id>` | `handle_reject_command()` | Uses `proposal_review::reject_pending()` |
| `/config` | `handle_config_command()` | Shows config values — use `ConfigManager::get("all")` |
| `/config init` | `init_config_file()` | Creates template config.toml |
| `/config reload` | **NEW** (#1064) | Re-read config.toml via ConfigManager, update active config |
| `/model` | `show_model_menu()` | List available models — returns text list |
| `/model <name>` | `set_active_model_with_refresh()` | Switch model + persist |
| `/status` | `show_status()` | Engine status (model, memory, iterations, etc.) |
| `/budget` | `show_budget_status()` | Show tool/iteration budget |
| `/thinking [level]` | `handle_thinking_command()` | Get/set thinking level |
| `/help` | `show_help()` | Print command list |
| `/signals` | `show_signals_summary()` | Signal quality summary |

### Commands that stay client-only (TUI handles locally)

| Command | Reason |
|---------|--------|
| `/quit`, `/exit` | Process lifecycle — TUI exits |
| `/clear`, `/cls` | Terminal screen clear |
| `/new` | Conversation management (TUI-local state) |
| `/history` | Conversation list (TUI-local state) |
| `/auth` | Credential store — interactive prompts |
| `/keys` | Key management — interactive |
| `/sign` | Signing — interactive |

For client-only commands, `execute_command()` returns `None`. The caller (HTTP handler or TUI) decides what to do.

### Wire into `handle_message()` in http_serve.rs

```rust
async fn handle_message(
    State(state): State<HttpState>,
    Json(request): Json<MessageRequest>,
) -> Result<Json<MessageResponse>, (StatusCode, Json<ErrorBody>)> {
    // ... existing validation ...
    
    let parsed = parse_command(&request.message);
    
    // Try server-side command execution first
    if !matches!(parsed, ParsedCommand::Unknown(_)) {
        let mut guard = state.app.lock().await;
        let mut ctx = CommandContext { app: &mut *guard };
        if let Some(result) = execute_command(&mut ctx, &parsed) {
            let response = result.map_err(internal_error)?;
            return Ok(Json(MessageResponse {
                response: response.response,
                model: None,
                iterations: 0,
            }));
        }
        // Client-only command sent via HTTP — return helpful error
        return Ok(Json(MessageResponse {
            response: format!("/{} is a client-side command (only available in the TUI)", ...),
            model: None,
            iterations: 0,
        }));
    }
    
    // Not a command — process as chat message (existing flow)
    let result = process_and_route_message(...).await...;
}
```

### Wire into `tui.rs`

Replace the massive `match parse_command(input)` block:

```rust
let parsed = parse_command(input);  // now from commands module

// Try server-side first
let mut ctx = CommandContext { app: &mut self.headless_app };
if let Some(result) = execute_command(&mut ctx, &parsed) {
    match result {
        Ok(cr) => self.tui_println(cr.response),
        Err(e) => self.tui_println(format!("Error: {e}")),
    }
    return Ok(());
}

// Handle TUI-only commands
match parsed {
    ParsedCommand::Quit => { self.running = false; ... }
    ParsedCommand::Clear => { ... }
    ParsedCommand::New => { ... }
    ParsedCommand::History => { ... }
    ParsedCommand::Auth { .. } => { ... }
    ParsedCommand::Keys { .. } => { ... }
    ParsedCommand::Sign { .. } => { ... }
    _ => self.tui_println(format!("Unknown command")),
}
```

### /config reload implementation (#1064)

```rust
// In execute_command():
ParsedCommand::Config(Some(action)) if action == "reload" => {
    let manager = ctx.app.config_manager()
        .ok_or_else(|| anyhow!("config manager not available"))?;
    let mut guard = manager.lock()
        .map_err(|_| anyhow!("config manager lock poisoned"))?;
    guard.reload()?;
    Some(Ok(CommandResult {
        response: "Configuration reloaded from ~/.fawx/config.toml".to_string(),
    }))
}
```

If `ConfigManager` doesn't have a `reload()` method yet, add one — it should re-read the TOML file and update internal state. Check `fx-config/src/manager.rs`.

---

## Implementation Gates

### Gate 1: HeadlessApp access
`execute_command()` needs access to `HeadlessApp` internals: config, router, config_manager, proposal review context. Verify these are accessible via public methods (some may be `pub(crate)`). If not, add accessor methods rather than making fields public.

**Check:** `HeadlessApp` methods: `config_manager()`, `config()`, router access, `active_model()`, data dir.

### Gate 2: TuiApp compatibility  
`tui.rs` handlers access `self.config`, `self.router`, `self.conversation_store`, etc. After extraction, TUI must use the shared `execute_command()` for server-side commands and keep its own handlers for TUI-only commands. The TUI currently operates on its OWN config/state (not HeadlessApp) — verify the command handlers produce equivalent output when using HeadlessApp state instead.

**Check:** Compare `TuiApp.config` vs `HeadlessApp.config` — are they the same `FawxConfig`? If TUI modifies its own copy (e.g., `/model` updates `self.config.model.default_model`), the server-side handler must update `HeadlessApp`'s config equivalently.

---

## File Changes

| File | Change |
|------|--------|
| `engine/crates/fx-cli/src/commands.rs` | **NEW** — ParsedCommand, parse_command(), execute_command(), CommandContext, CommandResult |
| `engine/crates/fx-cli/src/http_serve.rs` | Import commands, intercept in handle_message() |
| `engine/crates/fx-cli/src/tui.rs` | Remove parse_command/ParsedCommand, import from commands, delegate server-side commands |
| `engine/crates/fx-cli/src/headless.rs` | Add any missing accessor methods on HeadlessApp |
| `engine/crates/fx-cli/src/main.rs` | Add `mod commands;` |
| `engine/crates/fx-config/src/manager.rs` | Add `reload()` if missing |

---

## Tests

1. **parse_command** — move existing tests from tui.rs to commands.rs (they test the parser)
2. **execute_command** — test each server-side command variant returns correct CommandResult
3. **HTTP integration** — POST `/message` with `/help` returns help text (not sent to LLM)
4. **HTTP integration** — POST `/message` with `/proposals` returns proposal list
5. **HTTP integration** — POST `/message` with `/config reload` returns success
6. **HTTP integration** — POST `/message` with `/quit` returns client-only message
7. **HTTP integration** — POST `/message` with normal text still goes to agentic loop
8. **/config reload** — verify config changes are picked up after reload
9. **TUI compatibility** — verify TUI still handles /quit, /clear, /new locally

---

## Acceptance Criteria

- [ ] `fawx-tui` users can run `/help`, `/proposals`, `/approve`, `/model`, `/config`, `/status`, `/thinking`, `/config reload`
- [ ] Telegram users get command responses (messages starting with `/` are intercepted)
- [ ] TUI-only commands (/quit, /clear, /new, /history) still work in legacy TUI
- [ ] TUI-only commands sent via HTTP return a helpful "client-side only" message
- [ ] `/config reload` re-reads config.toml without restart (#1064)
- [ ] No regression in legacy TUI command handling
- [ ] `cargo clippy -p fx-cli -p fx-config -- -D warnings` clean
- [ ] All existing parse_command tests pass (moved to new location)
