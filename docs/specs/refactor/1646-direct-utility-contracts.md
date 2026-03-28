# Spec: #1646 — Direct Utility Routing and Visible Tool Contracts

## Status
Foundation laid in PR #1648. The metadata pipeline works end-to-end for one skill (weather). The direct utility subsystem exists but has hardcoded profile variants and 3 legacy skills still on the wrapper contract.

## Goal
Make direct utility routing a generic, manifest-driven kernel subsystem. Eliminate legacy `{"input": "..."}` wrapper contracts from all skills that can expose structured schemas. Ensure legacy-wrapped tools cannot activate direct utility routing.

## Current State (dev branch, post #1648 merge)

### What exists

**Direct utility pipeline (working end-to-end for weather):**
1. `skills/weather-skill/manifest.toml` declares `[[tools]]` with `direct_utility = true` and `trigger_patterns`
2. `SkillToolManifest` (fx-skills/src/manifest.rs:94) stores the `tools` vec
3. `WasmSkill::build_manifest_tool_definition()` (fx-loadable/src/wasm_skill.rs:111) embeds `x-fawx-direct-utility` in the JSON schema
4. `WasmSkill::build_tool_definitions()` (line 160) branches: `manifest.tools.is_empty()` → legacy wrapper, else → manifest tool defs
5. `direct_utility_metadata()` in kernel reads `x-fawx-direct-utility` from schema
6. `detect_direct_utility_profile()` matches trigger patterns from metadata
7. `DirectUtilityProfile` enum drives routing

**Legacy wrapper (still active for 3 skills):**
- `calculator-skill`, `canvas-skill`, `github-skill` have no `[[tools]]` in manifest
- These get `build_legacy_tool_definition()` which exposes `{"input": {"type": "string", "description": "JSON input for the WASM skill"}}` and `required: ["input"]`
- Legacy wrapper goes through `extract_legacy_input()` / `normalize_legacy_router_input()` at runtime

### What's wrong

1. **`DirectUtilityProfile` enum is hardcoded** (direct_utility.rs:4):
```rust
pub(super) enum DirectUtilityProfile {
    Weather,
    CurrentTime,
}
```
Every new direct utility tool requires adding a variant + match arms in 5 functions. This is the string-registry pattern doctrine prohibits.

2. **Profile-specific logic is in the kernel:**
- `direct_utility_directive()` has hardcoded directive strings per profile
- `direct_utility_progress()` has hardcoded progress messages per profile  
- `direct_utility_completion_response()` has profile-specific call building
- `direct_utility_terminal_response()` has profile-specific result extraction

3. **Legacy skills can't be safely excluded** — there's no explicit check that blocks `{"input": "..."}` schemas from direct utility. Currently safe only because legacy skills don't have `x-fawx-direct-utility` metadata. But if someone adds `direct_utility = true` to a legacy manifest without adding `[[tools]]`, the system would try to direct-route through the string wrapper.

4. **Only 5 of 8 skills have `[[tools]]` declarations.** Three skills still rely on the legacy wrapper.

## Deliverables

### Phase 1: Generalize the profile enum (kernel-side)

1. Replace `DirectUtilityProfile` enum with a data-driven struct:
```rust
pub(super) struct DirectUtilityProfile {
    pub tool_name: String,
    pub trigger_patterns: Vec<String>,
    pub progress_kind: ProgressKind,
    pub progress_message: String,
}
```

2. Build profiles dynamically from tool metadata at detection time — no static enum variants.

3. Retain `detect_direct_utility_profile()` signature but make it return the data-driven struct built from `x-fawx-direct-utility` metadata.

4. `direct_utility_directive()`, `direct_utility_progress()`, `direct_utility_completion_response()`, `direct_utility_terminal_response()` operate on the struct fields, not match arms.

5. Add explicit validation: direct utility only activates when the tool schema has real `properties` (not just `{"input": "string"}`). Add a `fn is_structured_tool_schema(schema: &Value) -> bool` check.

### Phase 2: Migrate remaining legacy skills

6. Add `[[tools]]` sections to `calculator-skill/manifest.toml`:
```toml
[[tools]]
name = "calculate"
description = "Evaluate a mathematical expression"
[[tools.parameters]]
name = "expression"
type = "string"
description = "Mathematical expression to evaluate (e.g., '2 + 3 * 4')"
required = true
```

7. Add `[[tools]]` to `github-skill/manifest.toml` — this has multiple actions, so multiple `[[tools]]` entries or a single tool with an `action` parameter.

8. Add `[[tools]]` to `canvas-skill/manifest.toml`.

9. Update `encode_runtime_input()` in wasm_skill.rs to handle manifest-declared tools with structured arguments (route structured JSON instead of the legacy `input` string wrapper).

### Phase 3: Safety gate

10. Add negative test: a skill with `direct_utility = true` in manifest but no `[[tools]]` section should NOT activate direct utility routing (it falls back to legacy wrapper, which has no `x-fawx-direct-utility` metadata).

11. Add negative test: a legacy-wrapped tool (schema with `"input": {"type": "string"}`) should never match `is_structured_tool_schema()`.

## Files to modify
- `engine/crates/fx-kernel/src/loop_engine/direct_utility.rs` (generalize profile)
- `engine/crates/fx-kernel/src/loop_engine.rs` (update callers of profile functions)
- `engine/crates/fx-loadable/src/wasm_skill.rs` (structured arg routing for manifest tools)
- `skills/calculator-skill/manifest.toml` (add [[tools]])
- `skills/github-skill/manifest.toml` (add [[tools]])  
- `skills/canvas-skill/manifest.toml` (add [[tools]])

## Not in scope
- Tool trait decomposition of FawxToolExecutor (that's #1639)
- Skill activation lifecycle / revision management (that's #1647)
- New direct utility skills beyond calculator/current_time
