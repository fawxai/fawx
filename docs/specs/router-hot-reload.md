# Spec: Hot-Reload ModelRouter After Credential Changes

## Problem

When a user adds or removes an AI provider credential via the HTTP API, the
`ModelRouter` is not updated. The router is built once at startup from the
`AuthManager` and wrapped in `Arc<ModelRouter>`. After credential save, the
credential store on disk is updated but the in-memory router still has the old
provider set (or no providers at all on fresh install).

Result: after adding an API key via Settings, the model picker shows no models
and chat doesn't work until the server is manually restarted.

## Root Cause

`HeadlessApp.router` is `Arc<ModelRouter>` — immutable after construction.
The credential save path (`save_auth_method()` in `handlers/auth.rs`) writes
to the `AuthStore` but never touches the router.

## Fix

After any credential change (store, delete, OAuth callback), rebuild the
router from the updated credential store and swap it into the `HeadlessApp`.

### Step 1: Make the router swappable

Change `HeadlessApp.router` from `Arc<ModelRouter>` to
`Arc<ArcSwap<ModelRouter>>` (from the `arc-swap` crate) or simpler:
`Arc<RwLock<ModelRouter>>`.

**Simpler approach: `RwLock`**

In `headless.rs`:
```rust
// Before:
router: Arc<ModelRouter>,

// After:
router: Arc<RwLock<ModelRouter>>,
```

All read sites (`self.router.available_models()`, `self.router.active_model()`,
etc.) become `self.router.read().unwrap().available_models()`. There are ~15
call sites in `headless.rs`.

### Step 2: Add a `reload_providers` method to `HeadlessApp`

```rust
pub fn reload_providers(&self) -> anyhow::Result<()> {
    let auth_manager = startup::load_auth_manager()?;
    let mut new_router = startup::build_router(&auth_manager)?;
    seed_headless_router_active_model(&mut new_router, &self.config);
    
    let mut router = self.router.write()
        .map_err(|_| anyhow::anyhow!("router lock poisoned"))?;
    *router = new_router;
    Ok(())
}
```

### Step 3: Expose via `AppEngine` trait

Add to the `AppEngine` trait (in `fx-api`):
```rust
fn reload_providers(&mut self) -> Result<(), anyhow::Error>;
```

### Step 4: Call after credential changes

In `handlers/auth.rs`, after `save_auth_method()` and `delete_provider_auth()`:
```rust
// Reload the router to pick up the new credentials
if let Ok(mut app) = state.app.lock() {
    if let Err(e) = app.reload_providers() {
        tracing::warn!(error = %e, "failed to reload providers after credential change");
    }
}
```

Same for the OAuth callback handler.

### Step 5: Update active_model if it was empty

If `active_model` was empty (fresh install, no providers) and the reload
produces available models, auto-select the first one:

```rust
pub fn reload_providers(&mut self) -> anyhow::Result<()> {
    // ... rebuild router ...
    
    // If we had no active model and now we do, select one
    if self.active_model.is_empty() {
        if let Ok(model) = first_runtime_model(&new_router) {
            self.active_model = model;
        }
    }
    Ok(())
}
```

## Blast Radius

### Files that reference `self.router` in `headless.rs` (~15 sites):
- `seed_runtime_info` — provider lookup
- `active_model` — active model getter
- `available_models` — model list
- `process_message` — `RouterLoopLlmProvider::new()`
- `process_message_with_context` — same
- `apply_active_model_selection` — model switching
- `current_provider_name` — provider name
- `run_analysis` / `run_improvement` — `AnalysisCompletionProvider`
- `apply_http_defaults` — `Arc::get_mut` (needs special handling)
- `reload_config` — `sync_headless_model_from_config`
- `dynamic_model_menu` — `fetch_available_models`

### SubagentManager also holds `Arc<ModelRouter>`
The `SubagentManager` is built during startup with `Arc::clone(&router)`.
With `RwLock`, it would share the same lock. Need to verify subagent code
reads through the lock properly.

## Alternative: SIGHUP restart (quick fix)

Instead of hot-reload, trigger a SIGHUP after credential save:
```rust
// In save_auth_method handler:
unsafe { libc::kill(std::process::id() as i32, libc::SIGHUP); }
```

The existing SIGHUP handler re-execs the process, which rebuilds everything
from scratch. This is simpler but restarts the entire server (dropping
connections, resetting sessions). Not ideal but functional.

**Recommendation**: Implement the `RwLock` approach. It's more work but:
1. No dropped connections
2. No session state loss
3. Cleaner UX (model picker updates immediately)
4. Required anyway for the fresh-install flow

## Tests

1. `reload_providers_adds_new_models` — start with empty router, save a key,
   reload, verify models appear
2. `reload_providers_removes_deleted_provider` — start with provider, delete
   key, reload, verify models gone
3. `reload_providers_preserves_active_model` — reload with same providers,
   verify active_model unchanged
4. `reload_providers_auto_selects_when_empty` — start with empty active_model,
   reload with provider, verify active_model populated

## Verification

```bash
cargo fmt --all
cargo clippy --workspace --tests -- -D warnings
cargo test --workspace
```
