# Spec: `fawx setup` — Interactive First-Run Wizard

**Issue:** #1253  
**Priority:** Launch Blocker  
**Estimated size:** ~500 lines (new files + main.rs wiring)  
**Crate:** `fx-cli` (new module: `commands/setup.rs`)

---

## Problem

First-run requires 7+ manual steps across 3 tools. The TUI has an auth wizard 
but it only triggers inside the TUI itself — users must already know to run 
`fawx tui` first. There's no standalone CLI setup path. Pain points from real 
smoke testing (2026-03-08):

- Credential store is machine-specific (HKDF) — old DBs silently fail
- No validation on config.toml — duplicate sections cause silent failures
- Bearer token requires external generation (`openssl rand`)
- No clear error messages for crypto failures

## Solution

`fawx setup` — a standalone CLI command (no TUI, no alternate screen) that 
handles complete first-run configuration interactively.

## Detailed Design

### 1. CLI Integration

Add `Setup` variant to `Commands` enum in `main.rs`:

```rust
/// Interactive first-run setup wizard
Setup {
    /// Re-run setup even if already configured
    #[arg(long)]
    force: bool,
},
```

New file: `commands/setup.rs` with `pub async fn run(force: bool) -> anyhow::Result<i32>`.

### 2. Wizard Flow

```
fawx setup

🦊 Welcome to Fawx setup!

Checking system...
  ✓ Data directory: ~/.fawx
  ✓ Config file: not found (will create)
  ✗ Credential store: corrupt/stale (will recreate)

Step 1/4: LLM Provider
  How would you like to authenticate?
    [1] Claude subscription (setup token)
    [2] ChatGPT subscription (browser sign-in)
    [3] API key (Anthropic, OpenAI, OpenRouter, etc.)
  > 3

  Provider name (e.g. anthropic, openai, openrouter): anthropic
  Enter your anthropic API key: ****
  ✓ API key stored (encrypted)

Step 2/4: Model Selection
  Fetching available models...
  Available models for anthropic:
    [1] claude-sonnet-4-20250514 (recommended)
    [2] claude-opus-4-20250514
    [3] claude-haiku-3-20250307
  > 1
  ✓ Default model: anthropic/claude-sonnet-4-20250514

Step 3/5: HTTP API (for channels, webhooks, remote access)
  Enable HTTP API? [Y/n]: y
  ✓ Bearer token generated and stored (encrypted)
  ✓ Port: 8400 (Tailscale-only binding)

Step 4/5: Channels
  Set up a messaging channel? [y/N]: y
    [1] Telegram
    [2] Webhook (generic HTTP)
    [3] Skip
  > 1

  Telegram bot token (from @BotFather): ****
  Restrict to specific chat IDs? (comma-separated, or Enter to allow all): 
  ✓ Telegram channel configured (token encrypted)
  ✓ Webhook secret generated for validation

  Add another channel? [y/N]: n

Step 5/5: Validation
  Testing API connection...
  ✓ anthropic: connected (claude-sonnet-4-20250514)
  Testing Telegram...
  ✓ Telegram: bot @YourBot connected (getMe OK)
  ✓ Config written: ~/.fawx/config.toml
  ✓ Credential store: healthy

Setup complete! Next steps:
  fawx serve --http    — start the engine
  fawx-tui             — connect the terminal UI (requires engine running)
```

### 3. Implementation Details

#### System check phase
- Check `~/.fawx/` exists, create if not
- Check `config.toml` exists — if exists and `--force` not set, warn and ask to continue
- Check credential store — try to open, detect stale/corrupt (different HKDF salt):
  - If corrupt: delete `auth.db` + `.auth-salt`, recreate
  - Print clear message: "Credential store from a different installation detected. Recreating."

#### Auth phase (Step 1)
- Reuse existing auth logic from `TuiApp::run_auth_selection()` and friends
- Extract into standalone functions that don't require `&mut TuiApp`:
  - `prompt_auth_selection() -> AuthSelection`
  - `prompt_api_key() -> (provider, key)`
  - `prompt_setup_token() -> token`
- Use `AuthManager` directly (same as TUI path)
- Persist immediately after storing

#### Model selection (Step 2)
- Build `ModelRouter` from `AuthManager`
- Fetch available models for the authenticated provider
- Present numbered list, let user choose
- Write to `config.toml` as `default_model`

#### HTTP setup (Step 3)
- Ask if user wants HTTP API enabled
- If yes: auto-generate bearer token (`rand::thread_rng().gen::<[u8; 32]>()` → hex)
- Store bearer token in credential store (same path as `/auth http set-bearer`)
- Write `[http]` section to config with port
- Note: HTTP API is required for channels — if user says no to HTTP but yes to channels, auto-enable HTTP

#### Channel setup (Step 4)
- Only offered if HTTP is enabled (channels require the HTTP server)
- **Telegram:**
  - Prompt for bot token (from @BotFather) — secret input, stored encrypted
  - Optional: restrict to specific chat IDs (comma-separated)
  - Auto-generate webhook secret for request validation and store it encrypted
  - Validate token immediately via `getMe` API call
  - Write `[telegram]` section: `enabled = true`, `allowed_chat_ids`
  - Bot token and webhook secret are stored in the credential store, NOT in `config.toml`
- **Webhook (generic):**
  - Prompt for channel name/ID
  - Prompt for callback URL
  - Write to `[webhook]` section
- Loop: "Add another channel?" until user says no
- Each channel validated on setup (fail → warn but continue, user can fix later)

#### Validation (Step 5)
- Test API connection: send a minimal completion request (similar to `fawx doctor`)
- Verify response parses correctly
- Write final `config.toml` (only if validation passes)
- Print credential store health

#### Config generation
- Use `toml_edit::DocumentMut` to generate config (preserves comments)
- Start from `DEFAULT_CONFIG_TEMPLATE`
- Set only values the user chose, leave rest as commented defaults
- Don't overwrite existing config unless `--force` — merge new values in

### 4. Error handling

- Credential store corrupt → clear error, offer to recreate
- API key invalid → clear error, offer to re-enter
- Network failure → clear error, offer to skip validation and fix later
- Config exists → warn, ask to continue (or use `--force`)
- Ctrl+C at any point → clean exit, partial state is fine (re-run setup to complete)

### 5. Also needed: CLI auth subcommands

For non-interactive use (CI, scripts, power users):

```bash
fawx auth set-token anthropic sk-ant-...
fawx auth set-bearer my-secret-token
fawx auth status
```

Add `Auth` variant to `Commands` with subcommands. These bypass the wizard and 
directly write to the credential store / auth manager.

### 6. Files touched

| File | Change |
|------|--------|
| `main.rs` | Add `Setup` + `Auth` to `Commands`, dispatch |
| `commands/mod.rs` | Add `pub mod setup; pub mod auth;` |
| `commands/setup.rs` | **New** — wizard logic (~250 lines) |
| `commands/auth.rs` | **New** — CLI auth subcommands (~100 lines) |
| `tui.rs` | Extract prompt helpers to shared module |
| `auth_store.rs` | Add corrupt-detection + recreate logic |

### 7. Testing

- Unit tests for prompt parsing helpers (auth selection, provider name validation)
- Unit test for config generation (correct TOML output)
- Unit test for credential store corrupt detection
- Integration test: `fawx setup --force` with mock stdin (if feasible)
- **TUI smoke test**: run `fawx setup` → `fawx tui` → send a message → verify response

### 8. Security considerations

- API keys are never printed back after entry (secret input masking)
- Bearer token auto-generated with CSPRNG (`rand::OsRng`)
- Credential store uses existing AES-256-GCM encryption
- Config.toml never contains secrets (they're in credential store)
- `fawx auth set-token` reads from argv — warn that keys may be visible in shell history
  - Consider: read from stdin if no value provided (`echo KEY | fawx auth set-token anthropic`)

### 9. Out of scope

- Fully automatic OAuth callback handling without a browser-assisted paste-back flow
- Multi-provider setup in single wizard run (run setup again to add more)
- Fleet/node configuration (separate concern)

---

## Acceptance criteria

1. `fawx setup` on a clean install → working config + credentials in < 2 minutes
2. `fawx setup` with corrupt credential store → detects, recreates, succeeds
3. `fawx setup` with existing config → warns, asks before overwriting
4. `fawx auth set-token <provider> <key>` works without TUI
5. After setup, `fawx tui` launches with working model (no additional config needed)
6. After setup with HTTP enabled, `fawx serve --http` works with bearer auth
7. After setup with Telegram, `fawx serve --http` starts polling and bot responds
8. `fawx auth set-token telegram <BOT_TOKEN>` works without wizard
