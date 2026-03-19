# Track E-2: Persistent Permissions API

**Status:** SPEC
**Priority:** High — unblocks Swift Permissions & Safety screen
**Endpoints:** GET/PATCH `/v1/permissions`

---

## Overview

Expose the existing `PermissionsConfig` (fx-config) over HTTP so the Swift app can read and update persistent tool permissions and switch presets.

The engine already has a full permissions model:
- `PermissionPreset` enum: `Power`, `Cautious`, `Experimental`, `Custom`
- `PermissionAction` enum: 16 actions (read_any, web_search, file_write, shell, etc.)
- `PermissionsConfig` struct: preset + unrestricted list + proposal_required list
- `PermissionsConfig::from_preset_name()` to apply presets
- Config persistence via `ConfigManager`

This PR adds HTTP endpoints to read and patch these.

---

## Endpoints

### GET /v1/permissions

Returns the current persistent permission state.

Response 200:
```json
{
  "preset": "power",
  "permissions": [
    {
      "action": "read_any",
      "level": "allow",
      "title": "Read Any File"
    },
    {
      "action": "web_search",
      "level": "allow",
      "title": "Web Search"
    },
    {
      "action": "file_write",
      "level": "allow",
      "title": "File Write"
    },
    {
      "action": "shell",
      "level": "allow",
      "title": "Shell Commands"
    },
    {
      "action": "credential_change",
      "level": "propose",
      "title": "Credential Change"
    },
    {
      "action": "kernel_modify",
      "level": "propose",
      "title": "Kernel Modify"
    }
  ],
  "available_presets": ["power", "cautious", "experimental", "custom"]
}
```

Notes:
- `level` is derived from the action's membership: if in `unrestricted` → `"allow"`, if in `proposal_required` → `"propose"`, if in neither → `"deny"`.
- Every `PermissionAction` variant MUST appear in the response (exhaustive).
- `title` is a human-readable label derived from the action name (snake_case → Title Case).

### PATCH /v1/permissions

Update permissions. Two modes: apply a preset, or make individual action changes (which sets preset to "custom").

**Mode 1 — Apply preset:**
```json
{
  "preset": "cautious"
}
```

**Mode 2 — Individual changes:**
```json
{
  "changes": [
    { "action": "shell", "level": "propose" },
    { "action": "file_write", "level": "deny" }
  ]
}
```

**Mode 3 — Both (apply preset then override):**
```json
{
  "preset": "power",
  "changes": [
    { "action": "shell", "level": "propose" }
  ]
}
```

Response 200:
```json
{
  "updated": true,
  "preset": "custom",
  "changed_actions": ["shell", "file_write"]
}
```

Notes:
- If `preset` is provided, start from that preset's defaults.
- If `changes` are provided, apply them on top. Any individual change sets preset to `"custom"` (unless the result happens to exactly match a named preset — don't bother checking, just set custom).
- `level` values: `"allow"` (unrestricted), `"propose"` (proposal_required), `"deny"` (neither list).
- Invalid action names → 422.
- Invalid level values → 422.

**Persistence:**
- Read the existing config.toml via toml_edit.
- Update the `[permissions]` section: set `preset`, `unrestricted`, `proposal_required`.
- Write back via `write_config_file` (preserves comments).
- Reload ConfigManager.

---

## Implementation Plan

### Types (in `handlers/permissions.rs`)

```rust
#[derive(Serialize)]
pub struct PermissionEntry {
    pub action: String,
    pub level: String,   // "allow" | "propose" | "deny"
    pub title: String,
}

#[derive(Serialize)]
pub struct PermissionsResponse {
    pub preset: String,
    pub permissions: Vec<PermissionEntry>,
    pub available_presets: Vec<String>,
}

#[derive(Deserialize)]
pub struct PermissionsPatchRequest {
    pub preset: Option<String>,
    pub changes: Option<Vec<PermissionChange>>,
}

#[derive(Deserialize)]
pub struct PermissionChange {
    pub action: String,
    pub level: String,
}

#[derive(Serialize)]
pub struct PermissionsPatchResponse {
    pub updated: bool,
    pub preset: String,
    pub changed_actions: Vec<String>,
}
```

### Level Derivation

```rust
fn action_level(action: PermissionAction, config: &PermissionsConfig) -> &'static str {
    if config.unrestricted.contains(&action) {
        "allow"
    } else if config.proposal_required.contains(&action) {
        "propose"
    } else {
        "deny"
    }
}
```

### Title Derivation

Map each `PermissionAction` variant to a human-readable title:
- `ReadAny` → "Read Any File"
- `WebSearch` → "Web Search"
- `WebFetch` → "Web Fetch"
- `CodeExecute` → "Code Execute"
- `FileWrite` → "File Write"
- `Git` → "Git"
- `Shell` → "Shell Commands"
- `ToolCall` → "Tool Call"
- `SelfModify` → "Self Modify"
- `CredentialChange` → "Credential Change"
- `SystemInstall` → "System Install"
- `NetworkListen` → "Network Listen"
- `OutboundMessage` → "Outbound Message"
- `FileDelete` → "File Delete"
- `OutsideWorkspace` → "Outside Workspace"
- `KernelModify` → "Kernel Modify"

### Config Persistence for PATCH

The PATCH handler needs to:
1. Parse the request.
2. Build a new `PermissionsConfig` (from preset or by mutating current).
3. Serialize the `[permissions]` section to TOML.
4. Use `toml_edit` to update the config file (like `handle_config_patch` does).
5. Reload the ConfigManager.

**Key:** The existing `handle_apply_config_preset` in `handlers/config.rs` already does preset application via config patching. The permissions PATCH can follow the same pattern but be more granular.

---

## Files to Create/Modify

1. **NEW: `engine/crates/fx-api/src/handlers/permissions.rs`** — handlers + types
2. **MODIFY: `engine/crates/fx-api/src/handlers/mod.rs`** — add `pub mod permissions;`
3. **MODIFY: `engine/crates/fx-api/src/router.rs`** — add routes

---

## Tests Required

1. `get_returns_all_actions` — GET returns all 16 PermissionAction variants
2. `get_reflects_power_preset` — default config returns power levels
3. `get_reflects_cautious_preset` — cautious config returns all-propose for writes
4. `patch_applies_preset` — PATCH with preset changes all levels
5. `patch_individual_change_sets_custom` — single action change sets preset to custom
6. `patch_rejects_invalid_action` — unknown action → 422
7. `patch_rejects_invalid_level` — unknown level → 422
8. `patch_preset_then_override` — preset + changes works in order
9. `response_serializes_correctly` — round-trip JSON serialization
10. `all_actions_have_titles` — every PermissionAction has a non-empty title

---

## Acceptance Criteria

- GET returns exhaustive action list with current levels
- PATCH with preset applies preset defaults
- PATCH with individual changes persists to config.toml
- Changes survive restart (persisted)
- All existing tests pass, clippy clean
