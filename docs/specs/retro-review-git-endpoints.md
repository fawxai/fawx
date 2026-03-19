# Retroactive Review: Git Endpoints (unstage/pull/fetch) + ThinkingRegistry Wiring

**Commits:** `31358247` (git endpoints), `472bf9d0` (ThinkingRegistry wiring)  
**Pushed directly to dev without PR — retroactive review requested.**

---

## Git Endpoints (31358247)

### Changes
- `engine/crates/fx-api/src/handlers/git.rs`: +96 lines — 3 new handlers
- `engine/crates/fx-api/src/router.rs`: +3 routes

### POST /git/unstage
- Calls `git reset HEAD [-- paths]`
- Empty paths = unstage all (`git reset HEAD`)
- Reuses `validate_paths()` and same error handling pattern as `/git/stage`
- Response: `{ unstaged: bool, paths: Vec<String> }`

### POST /git/pull  
- Calls `git pull`
- Detects conflicts by checking stdout/stderr for "CONFLICT" string
- Response: `{ pulled: bool, summary: String, conflicts: bool }`

### POST /git/fetch
- Calls `git fetch`
- Response: `{ fetched: bool, summary: String }`

---

## ThinkingRegistry Wiring (472bf9d0)

### Changes
- `engine/crates/fx-cli/src/headless.rs`: +9/-4 lines

### What changed
1. Import: `supported_thinking_levels` → `ThinkingRegistry`
2. New field: `thinking_registry: ThinkingRegistry` on `HeadlessApp`
3. Init: `ThinkingRegistry::with_defaults()` in constructor
4. `thinking_available_levels()`: delegates to `self.thinking_registry.available_levels(&self.active_model)` instead of hardcoded per-provider function
