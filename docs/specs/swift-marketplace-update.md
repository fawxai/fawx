# Swift UI Marketplace Update Spec

## Status: DRAFT
## Date: 2026-03-24

## Context

The Swift app has a complete marketplace UI (`MarketplaceView`, `SkillsView`, `SkillsViewModel`) that talks to the Fawx HTTP API. The API handlers (`fx-api/src/handlers/marketplace.rs`) are stubs returning "Marketplace not yet connected."

The `fx-marketplace` crate has a working client (search, install, verify, list) that talks directly to the GitHub-hosted registry. The CLI (`fawx skill search/install/list`) uses this crate and works end to end.

The gap: the HTTP API handlers don't call `fx-marketplace`. The Swift app hits the API, gets stub responses, and shows "Marketplace not yet connected."

## Goal

Wire the HTTP API handlers to `fx-marketplace` so the Swift app gets real search results, can install skills, and can remove installed skills. No Swift changes needed if the API contract doesn't change.

## Current State

### What works (CLI path)
```
User → fx-cli → fx-marketplace → GitHub raw (fawxai/registry) → download + verify + install
```

### What's broken (Swift path)
```
Swift App → HTTP API (fx-api) → stub handler → "not connected"
```

### Target
```
Swift App → HTTP API (fx-api) → fx-marketplace → GitHub raw (fawxai/registry) → download + verify + install
```

## Changes Required

### 1. Add `fx-marketplace` dependency to `fx-api`

In `engine/crates/fx-api/Cargo.toml`:
```toml
fx-marketplace = { path = "../fx-marketplace" }
```

### 2. Add marketplace config to `HttpState`

`fx-api/src/state.rs` — the `HttpState` struct needs access to:
- Registry URL (from config or default)
- Trusted keys (builtin fawxai key + user keys from `~/.fawx/trusted_keys/`)
- Data directory path (for install location)

Option A: Store a `fx_marketplace::RegistryConfig` in `HttpState`.
Option B: Build the config on each request from `data_dir` (simpler, no state).

Recommend Option B since the config is cheap to build and `data_dir` is already in `HttpState`.

### 3. Wire `handle_search_skills`

Current: Returns empty results with `marketplace_available: false`.

Change: Call `fx_marketplace::search(config, query)` and map `SkillEntry` → `MarketplaceSkillSummary`.

Mapping:
```
SkillEntry.name → MarketplaceSkillSummary.name
SkillEntry.name (capitalized) → MarketplaceSkillSummary.title
SkillEntry.description → MarketplaceSkillSummary.description
SkillEntry.author → MarketplaceSkillSummary.publisher
true → MarketplaceSkillSummary.signed (all registry skills are signed)
```

Empty query should return all skills (the full index).

Error handling: If the registry fetch fails (network error), return `marketplace_available: false` with the error message. Don't crash the endpoint.

### 4. Wire `handle_install_skill`

Current: Returns 503 "Marketplace not yet available."

Change: Call `fx_marketplace::install(config, name)`.

On success: Return 200 with `InstallResult` serialized. The SkillLoader/SkillWatcher should pick up the new skill automatically (hot-reload).

On error: Map `MarketplaceError` variants to HTTP status codes:
- `SkillNotFound` → 404
- `SignatureInvalid` → 422
- `NetworkError` → 502
- `InstallError` → 500
- `InsecureRegistry` → 500

### 5. Wire `handle_remove_skill`

Current: Returns 404.

Change: Delete the skill directory at `data_dir/skills/{name}/`. The SkillWatcher should detect the removal and unload the skill.

Steps:
1. Validate skill name (`fx_marketplace::validate_skill_name`)
2. Check `data_dir/skills/{name}/` exists → 404 if not
3. Remove the directory (`std::fs::remove_dir_all`)
4. Return 200

### 6. Load builtin fawxai public key

Same approach as the CLI: embed `FAWXAI_PUBLIC_KEY` constant, append user keys from `data_dir/trusted_keys/`. This can be a shared function between `fx-cli` and `fx-api` (consider extracting to `fx-marketplace` itself).

Refactoring suggestion: Move the `FAWXAI_PUBLIC_KEY` constant and `load_trusted_keys` function into `fx-marketplace` as public API, so both `fx-cli` and `fx-api` use the same code.

## API Contract Verification

The Swift app expects these response shapes (from `MarketplaceSkill.swift` and `Skill.swift`):

**Search:** `SkillSearchResponse` — ✅ no changes needed, handler already returns this shape.

**Install:** Swift decodes response as `JSONValue` (opaque) — ✅ any JSON body works.

**Remove:** Swift decodes response as `JSONValue` — ✅ any JSON body works.

**List installed:** `SkillsResponse` with `[SkillSummary]` — ✅ already wired to real data via `/v1/skills`.

No Swift model changes required. The existing `MarketplaceView` UI will work as-is once the API returns real data instead of stubs.

## What the user sees (before → after)

**Before:** "Marketplace not yet connected" placeholder. Search returns nothing. Install button hits 503.

**After:** Search shows 9 skills with titles, descriptions, publisher badges. Install button downloads, verifies signature, installs. Skill appears in "Installed" tab. Remove button deletes it.

## Testing

1. Unit tests for the new handler logic (mock registry responses)
2. Integration test: search → install → list → verify installed → remove → verify removed
3. Manual: open Swift app, search "weather", install, verify it appears in installed skills, remove

## Files to change

1. `engine/crates/fx-api/Cargo.toml` — add `fx-marketplace` dep
2. `engine/crates/fx-api/src/handlers/marketplace.rs` — wire handlers
3. `engine/crates/fx-api/src/state.rs` — if Option A (probably not needed)
4. `engine/crates/fx-marketplace/src/lib.rs` — extract `FAWXAI_PUBLIC_KEY` + `load_trusted_keys` as public API (optional refactor)

Estimated scope: ~150 lines changed across 2-3 files. No new crates, no Swift changes.

## Out of scope

- Skill detail view (capabilities, tools, version) — future enhancement
- Capability badges in marketplace cards — requires adding `capabilities` to `MarketplaceSkillSummary` (Swift model change)
- Publisher verification / author pages
- Skill ratings / download counts
- Automatic skill updates
