# Spec: #1643 — Decompose fx-config/src/lib.rs

## Status
Not started.

## Goal
Split `engine/crates/fx-config/src/lib.rs` (2,403 lines) into focused modules. Currently mixes struct definitions, Display/FromStr impls, preset behavior, default construction, validation logic, env var parsing, path expansion, and TOML manipulation.

## Current State (codex/provider-owned-loop-refactor branch)

File: `engine/crates/fx-config/src/lib.rs` (2,403 lines)

### Existing module structure
```
fx-config/src/
├── lib.rs          (2,403 lines — everything)
├── manager.rs      (config file watcher / hot-reload)
└── test_support.rs (test helpers, feature-gated)
```

### Content inventory of lib.rs

**Struct definitions (~800 lines):**
- `FawxConfig` (line 133) — top-level config
- `AgentConfig`, `AgentBehaviorConfig` (161, 181)
- `WorkspaceConfig`, `GitConfig` (200, 208)
- `CapabilityMode` enum (216)
- `PermissionPreset` enum, `PermissionAction` enum, `PermissionsConfig` (226, 263, 308)
- `BudgetConfig`, `SandboxConfig`, `ProposalConfig` (458, 480, 502)
- `FleetConfig`, `NodeConfig` (524, 545)
- `WebhookConfig`, `WebhookChannelConfig` (568, 577)
- `OrchestratorConfig` (589)
- `TelegramChannelConfig`, `PreprocessDedup` (614, 633)
- Plus more: auth configs, self-modify configs, experiment configs

**Preset functions (~300 lines):**
- `PermissionsConfig::power()`, `cautious()`, `experimental()`, `open()`, `standard()`, `restricted()`
- `PermissionsConfig::from_preset_name(&str)` — string-matching preset lookup

**Validation (~200 lines):**
- `validate_synthesis_instruction()`
- Various `Default` impls with validation logic embedded
- MIN/MAX constants

**Path/env expansion (~150 lines):**
- Tilde expansion in paths
- Env var substitution

**TOML manipulation (~300 lines):**
- `FawxConfig::from_toml_str()`, `to_toml_string()`
- `set_value()`, `get_value()` for individual keys
- Document manipulation via `toml_edit`

**Display/FromStr impls (~200 lines):**
- `PermissionPreset::as_str()`, `FromStr`
- `PermissionAction::as_str()`
- `CapabilityMode` display

**Default construction (~200 lines):**
- `DEFAULT_CONFIG_TEMPLATE` string
- Default impls for each struct

## Proposed Decomposition

```
fx-config/src/
├── lib.rs           (~100 lines — re-exports, top-level construction)
├── types.rs         (~500 lines — all struct/enum definitions with derives)
├── presets.rs       (~300 lines — preset definitions, from_preset_name)
├── validation.rs    (~200 lines — validation functions, constants)
├── env.rs           (~150 lines — env var parsing, tilde expansion)
├── toml_io.rs       (~300 lines — TOML read/write/set/get)
├── defaults.rs      (~200 lines — Default impls, DEFAULT_CONFIG_TEMPLATE)
├── display.rs       (~200 lines — Display, FromStr, as_str impls)
├── manager.rs       (unchanged)
└── test_support.rs  (unchanged)
```

## Deliverables

1. Create the 7 new modules listed above
2. Move code from lib.rs into appropriate modules
3. lib.rs becomes re-exports only: `pub use types::*; pub use presets::*;` etc.
4. All public API unchanged — every type, function, and constant remains accessible at the same path
5. All existing tests pass with zero modifications (the split is purely organizational)
6. No behavioral changes — only file reorganization
7. Each module has a top-level doc comment explaining its responsibility

## Migration rules
- `pub` items keep their visibility
- `pub(crate)` items become `pub(crate)` in their new module
- Internal helper functions used across modules become `pub(crate)`
- Constants stay with their closest related code (validation constants in validation.rs, template in defaults.rs)

## Files to modify
- `engine/crates/fx-config/src/lib.rs` (gut to re-exports)
- New files: `types.rs`, `presets.rs`, `validation.rs`, `env.rs`, `toml_io.rs`, `defaults.rs`, `display.rs`

## Not in scope
- Changing preset behavior
- Adding new config fields
- Making presets self-describing via traits (that's #1638 territory)
- Changes to config manager or hot-reload
