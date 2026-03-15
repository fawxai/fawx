# Subtask 3: TUI Wiring for WASM Hot-Reload

Branch `feat/wasm-hot-reload` on `abbudjoe/fawx` has Subtasks 1 & 2 complete and reviewed. Your job is to wire the `SkillWatcher` into the TUI so hot-reload actually works at runtime.

## What exists (don't modify these)

- `fx_loadable::SkillWatcher` — watches `~/.fawx/skills/`, debounces filesystem events, swaps skills in the registry via `replace_skill()`/`register()`/`remove_skill()`
- `fx_loadable::ReloadEvent` — `Loaded { skill_name, version }`, `Updated { skill_name, old_version, new_version }`, `Removed { skill_name }`, `Error { skill_name, error }`
- `SkillRegistry` is now `RwLock`-based with `Arc<dyn Skill>` — concurrent reads safe
- `CachingExecutor::clear_cache(&self)` — clears the tool result cache (line 420 of `caching_executor.rs`)

## What you need to do (~50 lines across tui.rs)

### 1. Share the SkillRegistry via Arc

In `build_skill_registry()` (tui.rs ~2977), the registry is already created as `SkillRegistry::new()`. Wrap it in `Arc::new()` and clone — one clone goes to `CachingExecutor`, one gets stored for the watcher.

Add an `Arc<SkillRegistry>` field to `SkillRegistryBundle`, pass it through `LoopEngineBundle` → `TuiAppDeps` → `TuiApp`.

Note: `CachingExecutor::new()` takes `T: ToolExecutor`. `SkillRegistry` implements `ToolExecutor`. You can either:
- Pass `Arc<SkillRegistry>` to `CachingExecutor` (if it accepts `T` where `Arc<T>: ToolExecutor`), or
- Keep passing `SkillRegistry` by value and separately store an `Arc` clone. Check the `CachingExecutor` generic constraint.

The key insight: `SkillRegistry` uses interior mutability (`RwLock`) so the watcher and the executor chain can share the same registry instance via `Arc`.

### 2. Spawn the watcher in TuiApp::run()

After the welcome banner (~line 1120), before the main loop:

```rust
let (reload_tx, mut reload_rx) = tokio::sync::mpsc::channel::<fx_loadable::ReloadEvent>(32);
let skills_dir = dirs::home_dir()
    .map(|h| h.join(".fawx").join("skills"))
    .unwrap_or_else(|| PathBuf::from(".fawx/skills"));
let mut watcher = fx_loadable::SkillWatcher::new(
    skills_dir,
    Arc::clone(&self.skill_registry),
    reload_tx,
    self.credential_provider.clone(),
);
watcher.initialize_hashes();
tokio::spawn(async move {
    if let Err(e) = watcher.run().await {
        tracing::error!(error = %e, "skill watcher exited");
    }
});
```

### 3. Poll reload_rx in the main event loop

In the main `while self.running` loop (~line 1140), after `draw_ratatui_frame`, check for reload events (non-blocking):

```rust
while let Ok(event) = reload_rx.try_recv() {
    match &event {
        fx_loadable::ReloadEvent::Loaded { skill_name, version } => {
            app.add_output(format!("🔌 Loaded skill: {skill_name} v{version}"));
        }
        fx_loadable::ReloadEvent::Updated { skill_name, new_version, .. } => {
            app.add_output(format!("🔄 Updated skill: {skill_name} v{new_version}"));
        }
        fx_loadable::ReloadEvent::Removed { skill_name } => {
            app.add_output(format!("🗑️ Removed skill: {skill_name}"));
        }
        fx_loadable::ReloadEvent::Error { skill_name, error } => {
            app.add_output(format!("⚠️ Skill error ({skill_name}): {error}"));
        }
    }
    self.clear_tool_cache();
}
```

### 4. Clear CachingExecutor cache on reload

When skills change, cached tool results may be stale. Call `clear_cache()` on every Loaded, Updated, or Removed event.

You need access to the `CachingExecutor` from the TUI event loop. Options:
- Store an `Arc` reference to the executor chain that exposes `clear_cache()`
- The `ToolExecutor` trait has `fn clear_cache(&self) {}` (default no-op). `CachingExecutor` overrides it. The `ProposalGateExecutor` wraps `CachingExecutor` and delegates `clear_cache()`. So calling `clear_cache()` on the `Arc<dyn ToolExecutor>` that the `LoopEngine` holds should work — but you may need to store a separate reference.
- Simplest: store the `Arc<ProposalGateExecutor<CachingExecutor<SkillRegistry>>>` (or `Arc<dyn ToolExecutor>`) before passing to the engine builder, and call `clear_cache()` on it.

### 5. Credential provider for the watcher

The watcher needs `Option<Arc<dyn CredentialProvider>>` for WASM skills that use `kv_get`. This is the same `CredentialStoreBridge` already built in `build_skill_registry()` (~line 3010). Store it in `SkillRegistryBundle` and pass through to `TuiApp`.

## Files to touch

- `engine/crates/fx-cli/src/tui.rs` — all changes here

## Files NOT to touch

- `engine/crates/fx-loadable/src/watcher.rs`
- `engine/crates/fx-loadable/src/registry.rs`
- `engine/crates/fx-loadable/src/wasm_skill.rs`

## Constraints

- Functions ≤ 40 lines
- No `.unwrap()` outside tests
- `cargo fmt --all` and `cargo clippy -p fx-cli -- -D warnings` clean
- All existing tests must pass (`cargo test -p fx-cli -p fx-loadable`)

## Spec reference

`docs/specs/wasm-hot-reload.md` — Section 3D (TUI Integration)
