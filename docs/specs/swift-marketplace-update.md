# Swift UI Marketplace Update Spec

## Status: DRAFT (R2 — reviewer findings incorporated)
## Date: 2026-03-24

## Context

The Swift app has a complete marketplace UI (`MarketplaceView`, `SkillsView`, `SkillsViewModel`) that talks to the Fawx HTTP API. The API handlers (`fx-api/src/handlers/marketplace.rs`) are stubs returning "Marketplace not yet connected."

The `fx-marketplace` crate has a working client (search, install, verify, list) that talks directly to the GitHub-hosted registry. The CLI (`fawx skill search/install/list`) uses this crate and works end to end.

The gap: the HTTP API handlers don't call `fx-marketplace`. The Swift app hits the API, gets stub responses, and shows "Marketplace not yet connected."

## Goal

Wire the HTTP API handlers to `fx-marketplace` so the Swift app gets real search results, can install skills, and can remove installed skills. No Swift changes needed; the API contract is unchanged.

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

### 2. Extract `FAWXAI_PUBLIC_KEY` + `load_trusted_keys` into `fx-marketplace`

**This is mandatory, not optional.** Having the same key constant in `fx-cli` and `fx-api` is a key-rotation hazard.

In `fx-marketplace/src/lib.rs`, add:
```rust
/// Official fawxai publisher Ed25519 public key (32 bytes).
pub const FAWXAI_PUBLIC_KEY: [u8; 32] = [
    62, 38, 70, 230, 12, 59, 226, 179, 11, 150, 52, 48, 238, 181, 159, 188,
    106, 55, 109, 208, 1, 191, 157, 233, 161, 111, 154, 212, 209, 133, 28, 68,
];

/// Load trusted keys: builtin fawxai key + any user-added keys from
/// `{data_dir}/trusted_keys/`.
pub fn load_trusted_keys(data_dir: &Path) -> Result<Vec<Vec<u8>>, MarketplaceError> {
    let mut keys = vec![FAWXAI_PUBLIC_KEY.to_vec()];
    let keys_dir = data_dir.join("trusted_keys");
    if keys_dir.exists() {
        for entry in std::fs::read_dir(&keys_dir)
            .map_err(|e| MarketplaceError::InstallError(format!("read trusted_keys: {e}")))?
        {
            let path = entry
                .map_err(|e| MarketplaceError::InstallError(format!("read entry: {e}")))?
                .path();
            if path.is_file() {
                keys.push(std::fs::read(&path)
                    .map_err(|e| MarketplaceError::InstallError(format!("read key: {e}")))?);
            }
        }
    }
    Ok(keys)
}

/// Build a default `RegistryConfig` for the given data directory.
pub fn default_config(data_dir: &Path) -> Result<RegistryConfig, MarketplaceError> {
    Ok(RegistryConfig {
        registry_url: DEFAULT_REGISTRY_URL.to_string(),
        data_dir: data_dir.to_path_buf(),
        trusted_keys: load_trusted_keys(data_dir)?,
    })
}

pub const DEFAULT_REGISTRY_URL: &str = "https://raw.githubusercontent.com/fawxai/registry/main";
```

Then update `fx-cli/src/commands/marketplace.rs` to use `fx_marketplace::default_config()` instead of its own copy.

### 3. Async/blocking boundary: `spawn_blocking`

**Critical:** `fx-marketplace` uses synchronous `ureq` for HTTP. Calling it directly from async axum handlers blocks the tokio runtime.

All `fx-marketplace` calls from handlers must be wrapped in `tokio::task::spawn_blocking`:

```rust
let config = fx_marketplace::default_config(&state.data_dir)?;
let results = tokio::task::spawn_blocking(move || {
    fx_marketplace::search(&config, &query)
}).await.map_err(|e| /* JoinError handling */)?;
```

This applies to `search`, `install`, and `list_installed`.

### 4. Wire `handle_search_skills`

Current: Returns empty results with `marketplace_available: false`.

Change:
```rust
pub async fn handle_search_skills(
    State(state): State<HttpState>,
    Query(params): Query<SearchQuery>,
) -> Json<SkillSearchResponse> {
```

Call `fx_marketplace::search(config, query)` via `spawn_blocking` and map `SkillEntry` → `MarketplaceSkillSummary`.

Mapping:
```
SkillEntry.name → MarketplaceSkillSummary.name
SkillEntry.name (title-cased) → MarketplaceSkillSummary.title
SkillEntry.description → MarketplaceSkillSummary.description
SkillEntry.author → MarketplaceSkillSummary.publisher
true → MarketplaceSkillSummary.signed (all registry skills are signed)
```

Empty query: return all skills (full index). Use `fx_marketplace::search` with empty string, which returns everything since all entries match.

Error handling: If the registry fetch fails, return `marketplace_available: false` with the error message. Don't crash the endpoint. Log the error server-side.

### 5. Wire `handle_install_skill`

Current: Returns 503.

Change:
```rust
pub async fn handle_install_skill(
    State(state): State<HttpState>,
    Json(request): Json<InstallSkillRequest>,
) -> Result<Json<InstallSkillResponse>, (StatusCode, Json<ErrorBody>)> {
```

Call `fx_marketplace::install(config, name)` via `spawn_blocking`.

**Response type:** Do NOT serialize `InstallResult` directly (it contains `install_path` which is a server-side filesystem path). Create a separate API response:
```rust
#[derive(Serialize)]
pub struct InstallSkillResponse {
    pub name: String,
    pub version: String,
    pub size_bytes: u64,
    pub installed: bool,
}
```

Error mapping for all `MarketplaceError` variants:
- `SkillNotFound` → 404
- `SignatureInvalid` → 422
- `ManifestInvalid` → 422
- `InvalidIndex` → 502
- `NetworkError` → 502
- `InstallError` → 500
- `InsecureRegistry` → 500

### 6. Wire `handle_remove_skill`

Current: Returns 404.

Change:
```rust
pub async fn handle_remove_skill(
    State(state): State<HttpState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorBody>)> {
```

Steps:
1. Validate skill name (`fx_marketplace::validate_skill_name`) → 400 if invalid
2. Build `skill_dir = data_dir/skills/{name}/`
3. **Canonicalize and verify prefix:** resolve `skill_dir` to absolute path and verify it starts with `data_dir/skills/`. This prevents symlink traversal attacks (same pattern `install` already uses).
4. Check exists → 404 if not
5. `std::fs::remove_dir_all` (can be sync; it's local I/O, not a network call, so `spawn_blocking` is optional but recommended for consistency)
6. Return 200 with `{"removed": true, "name": "<name>"}`

### 7. `RegistryConfig` needs `Send`

Since `RegistryConfig` is moved into `spawn_blocking` closures, it must implement `Send`. Verify this is the case (it should be, since it only contains `String`, `PathBuf`, and `Vec<Vec<u8>>`).

## API Contract Verification

The Swift app expects these response shapes:

**Search:** `SkillSearchResponse` — no changes needed, handler already returns this shape.

**Install:** Swift decodes response as `JSONValue` (opaque) — any JSON body works. The new `InstallSkillResponse` is fine.

**Remove:** Swift decodes response as `JSONValue` — any JSON body works.

**List installed:** `SkillsResponse` with `[SkillSummary]` — already wired to real data via `/v1/skills`.

No Swift model changes required.

## What the user sees (before → after)

**Before:** "Marketplace not yet connected" placeholder. Search returns nothing. Install button hits 503.

**After:** Search shows 9 skills with titles, descriptions, publisher badges. Install button downloads, verifies signature, installs. Skill appears in "Installed" tab. Remove button deletes it.

## Testing

1. Unit tests for the new handler logic (mock registry responses via test fixtures)
2. Unit test: `load_trusted_keys` returns builtin + user keys
3. Unit test: `default_config` builds valid config
4. Unit test: error mapping covers all `MarketplaceError` variants
5. Unit test: remove validates name and checks prefix after canonicalize
6. Integration test: search → install → list → verify installed → remove → verify removed
7. Manual: open Swift app, search "weather", install, verify it appears, remove

## Files to change

1. `engine/crates/fx-marketplace/src/lib.rs` — add `FAWXAI_PUBLIC_KEY`, `load_trusted_keys`, `default_config`, `DEFAULT_REGISTRY_URL` as public API
2. `engine/crates/fx-cli/src/commands/marketplace.rs` — remove duplicate key/config, use `fx_marketplace::default_config()`
3. `engine/crates/fx-api/Cargo.toml` — add `fx-marketplace` dep
4. `engine/crates/fx-api/src/handlers/marketplace.rs` — wire handlers with `spawn_blocking`, add `InstallSkillResponse` type

Estimated scope: ~200 lines changed across 4 files. No new crates, no Swift changes.

## Out of scope

- Skill detail view (capabilities, tools, version) — future enhancement
- Capability badges in marketplace cards — requires adding `capabilities` to `MarketplaceSkillSummary` (Swift model change)
- Publisher verification / author pages
- Skill ratings / download counts
- Automatic skill updates
