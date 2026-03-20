# Spec: `fawx bootstrap` Command

**Status:** READY FOR IMPLEMENTATION
**PR Target:** `dev`
**Scope:** New CLI subcommand for non-interactive zero-to-one local setup

---

## Problem

On a fresh macOS install, the Swift app's `completeLocalSetup()` tries to call `adoptLocalDevice()` against `http://127.0.0.1:8400`, but nothing has created `~/.fawx/`, generated a bearer token, written a config, or started a server. The API call hits nothing.

## Solution

Add a `fawx bootstrap` subcommand that performs non-interactive, idempotent zero-to-one setup. It creates the data directory, generates credentials, picks a port, writes config, and exits. It does NOT start the server (that's the LaunchAgent's job).

## Contract

```
fawx bootstrap [--json] [--port PORT] [--data-dir DIR]
```

### Behavior

1. Resolve data directory: `--data-dir` flag, or `~/.fawx/` default
2. If `config.toml` already exists AND is valid (has bearer token + port):
   - Output existing config info and exit 0 (idempotent)
3. Otherwise:
   - Create data directory (`~/.fawx/`)
   - Open or create the encrypted credential store (reuse `open_auth_store_with_recovery`)
   - Generate a random 32-byte hex bearer token (reuse `random_hex(32)`)
   - Store the bearer token via `auth_store.store_provider_token("http_bearer", &token)`
   - Scan ports 8400-8410 for the first available one (TCP bind test)
   - Write minimal `config.toml` with HTTP enabled, chosen port, bearer token in `[http]` section
   - Exit 0

### JSON Output (when `--json` flag is present)

```json
{
  "port": 8400,
  "host": "127.0.0.1",
  "bearer_token": "a1b2c3...",
  "data_dir": "/Users/joe/.fawx",
  "config_path": "/Users/joe/.fawx/config.toml",
  "created": true
}
```

When config already existed: `"created": false` and existing values returned.

Without `--json`, print human-readable output:
```
✓ Data directory: /Users/joe/.fawx
✓ Bearer token generated (encrypted)
✓ Port selected: 8400
✓ Config written: /Users/joe/.fawx/config.toml
```

### Exit Codes

- 0: Success (created or already valid)
- 1: Fatal error (permission denied, all ports occupied, etc.)

### Error Output (with `--json`)

```json
{
  "error": "All ports 8400-8410 are in use",
  "port_range": [8400, 8410]
}
```

## Implementation Location

- `engine/crates/fx-cli/src/commands/bootstrap.rs` — new file
- `engine/crates/fx-cli/src/commands/mod.rs` — register the command
- `engine/crates/fx-cli/src/main.rs` or equivalent — add to clap subcommands

## Port Scanning

```rust
fn find_available_port(start: u16, end: u16) -> Option<u16> {
    for port in start..=end {
        if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return Some(port);
        }
    }
    None
}
```

## Config Template

The bootstrap command writes a minimal config. Use `load_config_document` from `setup.rs` to start from `DEFAULT_CONFIG_TEMPLATE`, then set:

```toml
[http]
port = 8400
bearer_token = "generated-hex-token"
```

Reuse `set_integer` and `set_string` helpers from `setup.rs` for TOML manipulation.

## Idempotency Rules

- If `config.toml` exists and has a valid `[http]` section with `bearer_token` and `port`: return existing values, set `created: false`, exit 0
- If `config.toml` exists but is missing bearer_token or port: add the missing fields, preserve everything else
- If no config exists: create from template
- If credential store exists: reuse it; if not: create it
- Never overwrite an existing bearer token or port

## Testing

Required tests (in `bootstrap.rs` or a sibling test module):

1. `bootstrap_creates_config_on_fresh_directory` — tempdir, run bootstrap, verify config.toml exists with port and bearer_token
2. `bootstrap_is_idempotent` — run twice, second run returns same values with `created: false`
3. `bootstrap_preserves_existing_config` — create a config with extra fields, run bootstrap, verify extra fields preserved
4. `bootstrap_picks_next_port_when_default_occupied` — bind 8400, run bootstrap, verify port > 8400 selected
5. `bootstrap_returns_error_when_all_ports_occupied` — bind all ports 8400-8410, verify error
6. `bootstrap_json_output_is_valid` — verify JSON parses correctly
7. `find_available_port_returns_first_free` — unit test for port scanner
8. `find_available_port_returns_none_when_exhausted` — unit test for all-occupied case

## Dependencies

Reuses existing crates only:
- `fx-config` (DEFAULT_CONFIG_TEMPLATE, config types)
- `ring` (via `random_hex` from setup.rs — extract to shared util if needed)
- `toml_edit` (config manipulation)
- `serde_json` (JSON output)
- `anyhow` (error handling)

No new dependencies.

## What This Does NOT Do

- Does not start the server
- Does not install the LaunchAgent
- Does not run any auth/provider setup
- Does not interact with the network (except port scan which is local bind test)
- Does not require user interaction

The LaunchAgent installation and server startup are the Swift app's responsibility after bootstrap completes.
