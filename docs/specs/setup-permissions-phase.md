# Spec: Wire Permission Presets into Setup Wizard

## Goal
Add a `run_permissions_phase()` step to the setup wizard that lets the user select one of the three permission presets (Power, Cautious, Experimental). The selected preset's `[permissions]` section gets written to `config.toml`.

## Context
PR #1384 added `PermissionPreset`, `PermissionsConfig`, `BudgetConfig`, `SandboxConfig`, and `ProposalConfig` types to `fx-config`, plus commented template sections in `DEFAULT_CONFIG_TEMPLATE`. But the setup wizard was never updated to present the preset selection UI.

## Files to Change
- `engine/crates/fx-cli/src/commands/setup.rs` — add the new phase + tests

## Requirements

### 1. New wizard phase: `run_permissions_phase()`
Insert between `run_model_phase()` and `run_skills_phase()` in the `run()` function. Renumber subsequent steps accordingly (skills becomes Step 4, skill creds Step 5, etc.).

Display:
```
Step 3: Permissions
  Choose how much autonomy Fawx has:
    [1] 🔥 Power — full workspace autonomy, proposals for external actions (recommended)
    [2] 🔒 Cautious — proposals required for all writes and code execution
    [3] 🧪 Experimental — maximum autonomy including kernel self-modification
  > 
```

Store the selection on `SetupWizard` (add a `permissions_preset: Option<PermissionPreset>` field, defaulting to `None`).

Print confirmation:
```
  ✓ Permissions: Power (full workspace autonomy)
```

### 2. Write preset to config in `write_config()`
If a preset was selected, call `PermissionsConfig::from_preset_name()` to get the config, then write the `[permissions]` section to the config document:
- `preset` — string value (e.g. "power")
- `unrestricted` — array of snake_case action strings
- `proposal_required` — array of snake_case action strings

Use the existing `set_string()` and a new `set_string_array()` helper (similar to `set_integer_array()`).

### 3. Tests
- `parse_permissions_selection` — validates input "1", "2", "3" map to correct presets, rejects "0", "4", "abc"
- `permissions_config_writes_preset_to_document` — creates a preset, writes to a DocumentMut, verifies the TOML contains expected keys and values
- `completion_lines_match_headless_engine_workflow` — update the step count if the completion message mentions it

### 4. Step numbering
Current steps: 1 Auth, 2 Model, 3 Skills, 4 Skill Credentials, 5 HTTP API, 6 Channels, 7 Validation
New steps: 1 Auth, 2 Model, **3 Permissions**, 4 Skills, 5 Skill Credentials, 6 HTTP API, 7 Channels, 8 Validation

Update all `println!("Step N: ...")` lines accordingly.

## 5. Remove vision from SETUP_SKILLS
The vision WASM skill is superseded by native image content blocks (PR #1383). Remove it from the `SETUP_SKILLS` array in `setup.rs` so it's no longer offered during setup. This is the entry with `name: "vision"`. Update any tests that reference the skill count (currently 8 skills).

## Constraints
- Use `PermissionPreset` and `PermissionsConfig::from_preset_name()` from `fx-config` — do not duplicate the preset logic
- Use `prompt_choice_with_surface(PromptSurface::PlainTerminal, ...)` for consistency with other wizard phases
- Use `serde` `rename_all = "snake_case"` is already on `PermissionAction`, so serializing to strings should use `serde_json` or manual formatting — but since we're writing TOML, use the snake_case string representation directly
- Do NOT touch `fx-config/src/lib.rs` — the types are already correct
- Run `cargo fmt --all` before committing
- Run `cargo clippy --workspace --tests -- -D warnings` before committing
- Run `cargo test --workspace` before committing
