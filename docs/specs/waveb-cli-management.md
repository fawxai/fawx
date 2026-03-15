# Spec: Wave B — CLI Management Commands (`config` + `reset`)

**Phase:** 3c (Polish)  
**Status:** Ready  
**Date:** 2026-03-10

---

## Scope

This work item bundles the two Wave B CLI tasks because they both touch the top-level CLI command surface and dispatch wiring:

1. **`fawx config` improvements** — issue #1299
2. **`fawx reset`** — issue #1303

Bundle them in one implementation branch to avoid parallel conflicts in `fx-cli` command registration.

---

## Goal A — `fawx config` improvements

Add structured config subcommands:

```bash
fawx config show
fawx config get model.default_model
fawx config set model.default_model anthropic/claude-opus-4-6
```

Notes:
- Keep backward compatibility for bare `fawx config` by treating it as `show`.
- Reuse `fx_config::manager::ConfigManager` rather than reimplementing config mutation logic.
- `set` should preserve existing type coercion / validation behavior already provided by `ConfigManager::set()`.
- `get` should print the selected value cleanly for a single dot-path key/section.
- `show` should keep the current redacted full-config behavior.

### Output expectations
- Human-readable CLI output, not JSON.
- Errors should be actionable and specific.
- Sensitive values must remain redacted anywhere full-config rendering is involved.

---

## Goal B — `fawx reset`

Add a top-level reset command with targeted scopes:

```bash
fawx reset --memory
fawx reset --conversations
fawx reset --config
fawx reset --all
fawx reset --force
```

### Semantics

#### `--memory`
Clear persisted memory data, including:
- memory entries storage
- embedding index artifacts tied to memory

Do **not** delete credentials.

#### `--conversations`
Clear persisted conversation/session history and persisted signals/session artifacts that back conversation/history-like state.

Do **not** delete memory or credentials.

#### `--config`
Reset config to defaults while **preserving credentials**.

#### `--all`
Factory reset of the Fawx data directory state **except credentials** unless the existing codebase already has a stronger notion of full reset that is clearly safe and intended.

Minimum expectation for `--all`:
- includes memory reset
- includes conversation/session reset
- includes config reset
- preserves credentials

If preserving credentials under `--all` would be surprising in CLI wording, make the help/output explicit.

### Confirmation rules
- Every destructive reset mode must require confirmation by default.
- `--force` skips confirmation.
- Invalid combinations should fail clearly (for example, no scope selected, or conflicting scopes if the implementation chooses to reject them).

### UX expectations
- Print a concise summary of what was removed/reset.
- Missing files/directories should not cause failure; treat them as already clean.
- Keep behavior idempotent.

---

## File boundaries

Expected primary files:
- `engine/crates/fx-cli/src/main.rs`
- `engine/crates/fx-cli/src/commands/config.rs`
- `engine/crates/fx-cli/src/commands/mod.rs`
- `engine/crates/fx-cli/src/commands/reset.rs` (new)

You may touch a small shared helper if truly needed, but avoid broad refactors.

---

## Implementation constraints

### Config command
- Prefer clap subcommands/args over ad-hoc parsing.
- Reuse the real configured data dir / config path logic used elsewhere in `fx-cli`.
- Do not duplicate validation rules that already live in `fx-config`.

### Reset command
- Reuse existing runtime-layout knowledge (`RuntimeLayout`, startup helpers, existing auth/config paths) where useful.
- Do not hard-delete credentials.
- Avoid dangerous path handling: all deletes must be rooted inside the detected Fawx data dir / known managed paths.
- Missing paths should be treated as success.

### Safety
- No recursive delete of arbitrary user-provided paths.
- Keep deletes limited to known Fawx-managed directories/files.

---

## Tests

Required regression coverage:

### Config
- bare `fawx config` still behaves like show
- `config get` returns the expected value/section
- `config set` updates persisted config through `ConfigManager`
- invalid set value surfaces the validation error
- redaction behavior for full-config display remains intact

### Reset
- each reset mode deletes only the intended files/directories
- `--config` preserves credentials
- `--all` preserves credentials while resetting the rest
- missing targets are handled gracefully
- confirmation is required unless `--force` is used
- invalid flag combinations / missing scope fail as expected

Use temp directories for filesystem tests.

---

## Done criteria

This task is done when:
- `fawx config show|get|set` all work
- bare `fawx config` still works as an alias for `show`
- `fawx reset` supports the planned scopes with confirmation
- resets are scoped, safe, idempotent, and credential-preserving
- tests cover both command behavior and destructive-scope boundaries
