# Track E-1: Synthesis CRUD Endpoints

**Status:** SPEC
**Priority:** High — unblocks Swift Settings screen
**Endpoints:** GET/PUT/DELETE `/v1/synthesis`

---

## Overview

Add three endpoints to fx-api that let clients read, update, and clear the `model.synthesis_instruction` field in config.toml. This is the persistent "custom instructions" feature from Phase 5 §13.

The engine already stores `synthesis_instruction` as `Option<String>` in `ModelConfig` (fx-config). The config manager already supports `set()` and `save()`. This PR wires those to HTTP.

---

## Endpoints

### GET /v1/synthesis

Returns the current synthesis instruction.

Response 200:
```json
{
  "synthesis": "Be concise and direct.",
  "updated_at": 1741977600,
  "source": "config",
  "version": 1,
  "max_length": 500
}
```

When unset:
```json
{
  "synthesis": null,
  "updated_at": null,
  "source": "config",
  "version": 0,
  "max_length": 500
}
```

Notes:
- `version` is a monotonically increasing integer. Starts at 0 (unset). Each PUT/DELETE bumps it. Stored in-memory only (resets on server restart to current state hash or 0).
- `max_length` is the `MAX_SYNTHESIS_INSTRUCTION_LENGTH` constant (currently 500).
- `source` is always `"config"` for now. Future: could be `"chat"` if agent updates it.
- `updated_at` is unix timestamp of last modification. `null` when never set. For initial load from config.toml, use server start time.

### PUT /v1/synthesis

Set or replace the synthesis instruction.

Request:
```json
{
  "synthesis": "Be more concise and ask fewer clarifying questions.",
  "version": 1
}
```

- `version` is optional. If provided and doesn't match current version, return 409 Conflict.
- `synthesis` must be non-empty and ≤ `MAX_SYNTHESIS_INSTRUCTION_LENGTH` (500 chars).

Response 200:
```json
{
  "updated": true,
  "synthesis": "Be more concise and ask fewer clarifying questions.",
  "updated_at": 1741977660,
  "version": 2
}
```

Response 409 (stale version):
```json
{
  "error": "Version mismatch: expected 2, got 1"
}
```

Response 422 (validation):
```json
{
  "error": "synthesis_instruction exceeds 500 characters"
}
```

Implementation:
1. Validate length ≤ 500 and non-empty.
2. If `version` provided, compare against current. 409 if mismatch.
3. Use `ConfigManager::set("model.synthesis_instruction", &value)` to persist.
4. Bump in-memory version counter.
5. Return new state.

### DELETE /v1/synthesis

Clear the synthesis instruction.

Response 200:
```json
{
  "cleared": true,
  "version": 3
}
```

Implementation:
1. Set `model.synthesis_instruction` to empty string via ConfigManager, then remove the key from the TOML document.
2. Actually: use `ConfigManager::set("model.synthesis_instruction", "")` — but this won't work because the config validates non-empty. Instead, we need a new `ConfigManager::remove(key)` method, OR we handle this at the handler level by directly editing the TOML file and reloading.
3. **Preferred approach:** Add a `ConfigManager::clear(key: &str)` method that removes the key from the TOML document and reloads. This is cleaner than working around set() validation.

---

## State Management

Add a `SynthesisState` struct to track version and timestamp:

```rust
pub struct SynthesisState {
    version: AtomicU64,
    updated_at: Mutex<Option<u64>>,
}
```

- Lives in `HttpState` (or as a field on it).
- Initialized from config on startup: if `synthesis_instruction.is_some()`, version=1 and updated_at=now. Otherwise version=0, updated_at=None.
- Each successful PUT/DELETE increments version atomically.

---

## Files to Create/Modify

1. **NEW: `engine/crates/fx-api/src/handlers/synthesis.rs`** — handler functions
2. **MODIFY: `engine/crates/fx-api/src/handlers/mod.rs`** — add `pub mod synthesis;`
3. **MODIFY: `engine/crates/fx-api/src/router.rs`** — add routes
4. **MODIFY: `engine/crates/fx-api/src/state.rs`** — add SynthesisState to HttpState
5. **MODIFY: `engine/crates/fx-api/src/lib.rs`** — wire SynthesisState initialization
6. **MODIFY: `engine/crates/fx-config/src/manager.rs`** — add `clear(key)` method

---

## Tests Required

1. `get_returns_null_when_unset` — GET with no synthesis returns null synthesis
2. `get_returns_current_value` — GET after PUT returns the set value
3. `put_validates_length` — PUT with >500 chars returns 422
4. `put_validates_non_empty` — PUT with empty string returns 422
5. `put_updates_version` — PUT increments version
6. `put_rejects_stale_version` — PUT with wrong version returns 409
7. `put_without_version_always_succeeds` — PUT without version field works
8. `delete_clears_and_bumps_version` — DELETE clears value and increments version
9. `config_manager_clear_removes_key` — unit test on ConfigManager::clear()
10. `config_manager_clear_preserves_comments` — clear doesn't destroy TOML comments
11. `serialization_round_trip` — response types serialize correctly

---

## Acceptance Criteria

- `GET /v1/synthesis` returns current state
- `PUT /v1/synthesis` persists to config.toml, survives restart
- `DELETE /v1/synthesis` removes from config.toml
- Version tracking enables optimistic concurrency
- All existing tests pass, clippy clean
