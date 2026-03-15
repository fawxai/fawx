# Spec: Fix config command schema mismatch (#1333)

## Problem
`commands/config.rs` defines its own `Config` struct with `[llm] model` etc., but the runtime uses `FawxConfig` from `fx-config` with `[model] default_model`. They never agree.

## Solution
Replace the standalone `Config` struct in `commands/config.rs` with `FawxConfig` from `fx-config`.

### Changes required

1. **`commands/config.rs`** — Delete the parallel `Config`, `AgentConfig`, `SecurityConfig`, `LlmConfig` structs and all their `Default` impls / helper fns (`default_name`, `default_model`, etc.)

2. **`load_config()`** — Replace with `FawxConfig::load()` from `fx-config`. The config path is `~/.fawx/config.toml` (use `FawxConfig`'s standard loading).

3. **`fawx config` (display)** — Serialize `FawxConfig` to TOML and display. Redact sensitive fields (API keys, tokens) before display — walk the TOML tree and redact any value whose key contains `key`, `token`, `secret`, `password`, or `credential`.

4. **`fawx config set <key> <value>`** — Load `FawxConfig`, apply the dotted key path (e.g., `model.default_model`), save back to `config.toml`. Use TOML dotted-key traversal — split on `.`, walk the table, set the leaf.

5. **`fawx config get <key>`** — Load `FawxConfig`, traverse to the dotted key, print the value.

### Constraints
- Must not break `fawx config` display when `config.toml` doesn't exist (use `FawxConfig::default()`)
- Sensitive field redaction: use key-name heuristic, not hardcoded field list (future-proof)
- The `get`/`set` key paths must match `FawxConfig`'s serde field names (e.g., `model.default_model`, `general.system_prompt_path`, `http.port`)

### Tests
- Config round-trip: set a value, read it back, verify match
- Display with missing config file: should show defaults
- Redaction: API key fields show `***REDACTED***`
- Set invalid key path: should error gracefully
