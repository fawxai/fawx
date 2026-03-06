# Spec: WASM Hot-Reload

**Status:** Scoped — Ready for Implementation
**Date:** 2026-03-06
**Owner:** Joe + Clawdio
**Issue:** Part of #1130 (Wave 5 tracking)
**Related:** self-improvement-architecture.md, #1171 (individual tool defs), #1136 (epoch interruption)

---

## 0) What This Is

Fawx watches `~/.fawx/skills/` for new, updated, or removed WASM skill binaries and hot-swaps them into the running `SkillRegistry` without restart. This is THE self-improvement headline: Fawx writes a skill → compiles to WASM → drops it in the skills dir → it's live.

## 1) Current State

### Startup-only loading
`build_skill_registry()` in `tui.rs` calls `load_wasm_skills()` once during engine construction. Skills are immutable for the session lifetime.

### Registry is append-only
`SkillRegistry` holds `Vec<Box<dyn Skill>>` with `register(&mut self)` — no `replace()`, `remove()`, or interior mutability. Once wrapped in `Arc<dyn ToolExecutor>`, it's frozen.

### Executor chain
```
SkillRegistry → CachingExecutor → ProposalGateExecutor → LoopEngine
```
ProposalGateExecutor wraps the chain (on staging). The registry is the innermost layer.

### Signing exists but isn't wired
`fx-skills/src/signing.rs` has Ed25519 sign/verify. Not called during loading.

---

## 2) Design

### Architecture

```
┌─────────────────────────────────────────────┐
│ tui.rs startup                              │
│  spawns SkillWatcher as tokio task           │
│  passes Arc<SkillRegistry> + event channel   │
│  holds CachingExecutor ref for invalidation  │
└─────────────────────────────────────────────┘
         │
         ▼
┌─────────────────────────────────────────────┐
│ SkillWatcher (new, fx-loadable/watcher.rs)  │
│  • notify::RecommendedWatcher on skills dir │
│  • debounce (500ms per skill DIRECTORY)     │
│  • on create/modify: validate → swap        │
│  • on remove: unregister skill              │
│  • sends ReloadEvent to TUI via channel     │
└─────────────────────────────────────────────┘
         │
         ▼
┌─────────────────────────────────────────────┐
│ SkillRegistry (modified)                    │
│  • skills: RwLock<Vec<Arc<dyn Skill>>>      │
│  • replace_skill(name, Arc<dyn Skill>)      │
│  • remove_skill(name) -> bool               │
│  • dispatch: read lock → clone Arc → drop   │
│  •           lock → execute on cloned Arc   │
│  • replace/remove: write lock               │
└─────────────────────────────────────────────┘
```

### Validation pipeline (before swap)

```
1. Read manifest.toml — parse, validate required fields
2. Read {name}.wasm — verify file exists and is non-empty
3. (Optional) Signature check — verify_skill() against trusted public key
4. Compile WASM module — wasmtime::Module::new() catches malformed binaries
5. Construct WasmSkill — SkillRuntime creation validates host API linkage
6. Swap into registry — replace_skill() or register()
```

If any step fails, the existing skill (if any) stays loaded. Error is logged and surfaced to TUI.

### Debouncing

File writes often produce multiple events (create → write → write → close). Use a 500ms debounce window **per skill directory** (not per file): after the first event for any file in a skill directory, wait 500ms for the entire directory to become quiescent before triggering reload. This prevents two failure modes:
1. Loading partially-written WASM files (single file: create → write → write → close)
2. Loading when `manifest.toml` arrives before the `.wasm` binary (or vice versa) — both files must be present and settled before reload triggers

### Concurrency safety

Skills are stored as `Arc<dyn Skill>` (not `Box<dyn Skill>`). This is critical for async safety:

- **Clone-and-release dispatch**: `dispatch_call()` takes a read lock, clones the `Arc<dyn Skill>` for the matching skill, **drops the lock**, then calls `.execute().await` on the cloned Arc. The lock is never held across an `.await` point. This avoids both the `!Send` problem with `std::sync::RwLock` guards and the writer-starvation problem with `tokio::sync::RwLock` held during long WASM executions.
- **Read lock for metadata**: `tool_definitions()`, `skill_summaries()`, `cacheability()` take a read lock for the duration of the call (no `.await`, completes synchronously). Multiple concurrent readers allowed.
- **Write lock for swap**: `replace_skill()` and `remove_skill()` take a write lock. Blocks readers briefly (microseconds to swap an Arc pointer).
- **In-flight safety**: if a swap happens while a WASM tool call is executing, the old skill instance survives — the dispatching code holds an `Arc` clone, so the old instance drops only when the last in-flight call completes. No interruption, no data race.

### Change detection

Hash the `.wasm` binary (SHA-256) and store alongside the loaded skill. On file change events, compare hashes — only reload if the binary actually changed. Avoids spurious reloads from touch/copy-same-content.

### Event channel

```rust
enum ReloadEvent {
    Loaded { skill_name: String, version: String },
    Updated { skill_name: String, old_version: String, new_version: String },
    Removed { skill_name: String },
    Error { skill_name: String, error: String },
}
```

TUI receives these via `tokio::sync::mpsc` and can display status (e.g., status bar flash "✅ github-skill v1.1.0 loaded").

---

## 3) Changes Required

### A. SkillRegistry RwLock refactor (`fx-loadable/src/registry.rs`)

**Current:**
```rust
pub struct SkillRegistry {
    skills: Vec<Box<dyn Skill>>,
}
```

**New:**
```rust
pub struct SkillRegistry {
    skills: std::sync::RwLock<Vec<Arc<dyn Skill>>>,
}
```

Why `std::sync::RwLock` (not `tokio::sync::RwLock`): all lock-holding code is synchronous. Dispatch clones the Arc and drops the lock before any `.await`. No async guard crossing. `std::sync::RwLock` is simpler and has no risk of holding across yield points.

Why `Arc<dyn Skill>` (not `Box<dyn Skill>`): dispatch must release the lock before calling `.execute().await`. An `Arc` clone lets the skill outlive the lock scope. Also enables in-flight safety during swaps — old skill instances survive until the last Arc clone drops.

**Methods to add:**
- `replace_skill(&self, name: &str, skill: Arc<dyn Skill>) -> Option<Arc<dyn Skill>>` — replaces skill by name, returns old. Takes write lock. The watcher only calls this for skill names it originally loaded (tracked in its `hashes` map), so builtins are never replaced. The registry method itself is name-matched and general-purpose — builtin protection is an invariant of the caller (watcher), not the registry.
- `remove_skill(&self, name: &str) -> Option<Arc<dyn Skill>>` — removes skill by name, returns removed. Takes write lock.

**Methods to update:**
- `register(&self, skill)` — change from `&mut self` to `&self` (write lock internally). Accepts `Arc<dyn Skill>`. This is a breaking API change; all callers in `tui.rs` update mechanically (`Box::new(skill)` → `Arc::new(skill)`).
- `dispatch_call()` — read lock → find matching skill → clone Arc → drop lock → `.execute().await` on clone.
- All other read methods (`all_tool_definitions`, `skill_summaries`, `owning_skill`, `cacheability`) — read lock, complete synchronously within lock scope.

**Impact:** ~50-80 lines changed. All 25 existing tests must still pass (same behavior, just locked).

### B. SkillWatcher (`fx-loadable/src/watcher.rs` — NEW)

Core struct:
```rust
pub struct SkillWatcher {
    skills_dir: PathBuf,
    registry: Arc<SkillRegistry>,
    event_tx: mpsc::Sender<ReloadEvent>,
    hashes: HashMap<String, [u8; 32]>,  // skill_name -> SHA-256
}
```

Key functions:
- `SkillWatcher::new(skills_dir, registry, event_tx) -> Self`
- `SkillWatcher::run(self) -> Result<(), SkillError>` — async, runs forever. Sets up `notify` watcher, processes events in a loop. If the `notify` watcher fails to initialize or encounters an unrecoverable error, logs the error and exits gracefully — Fawx continues operating with startup-loaded skills (no hot-reload). Transient errors (individual skill load failures) are logged and surfaced via `ReloadEvent::Error` but do not stop the watcher.
- `fn validate_and_load(skill_dir: &Path) -> Result<(WasmSkill, [u8; 32]), SkillError>` — validation pipeline
- Debounce logic: `HashMap<String, Instant>` tracks last event per skill, processes after 500ms gap.

**Estimated:** ~200-250 lines.

### C. WasmSkill validation helper (`fx-loadable/src/wasm_skill.rs`)

Extract the load-from-directory logic into a reusable function:
```rust
pub fn load_wasm_skill_from_dir(skill_dir: &Path) -> Result<WasmSkill, SkillError>
```

Currently `load_wasm_skills()` uses `fx_skills::registry::SkillRegistry` to load all skills. The watcher needs to load individual skills by directory. Extract the per-skill logic.

Both `load_wasm_skills()` (startup) and `SkillWatcher` (hot-reload) use the same `load_wasm_skill_from_dir()` function. Single validation path — no divergence between startup loading and runtime reloading.

**Estimated:** ~20-30 lines.

### D. TUI wiring (`fx-cli/src/tui.rs`)

In `build_loop_engine_with_config()`:
1. Keep `SkillRegistry` in `Arc` before passing to `CachingExecutor`
2. Spawn `SkillWatcher::run()` as a tokio task
3. Forward `ReloadEvent`s to TUI state (optional status bar display)

**Estimated:** ~20-30 lines.

### E. Dependency addition (`fx-loadable/Cargo.toml`)

```toml
notify = { version = "7", features = ["macos_fsevent"] }
sha2 = "0.10"
```

`notify` is the standard Rust filesystem watcher (cross-platform: inotify on Linux, FSEvents on macOS, ReadDirectoryChanges on Windows). Well-maintained, 100M+ downloads.

`sha2` for binary change detection hashing.

---

### CachingExecutor invalidation

When a skill is swapped, cached results from the old version are stale. The `CachingExecutor` wraps the `SkillRegistry` in the executor chain and already exposes `clear_cache()` via the `ToolExecutor` trait.

**Mechanism:** TUI holds the `CachingExecutor` reference. When it receives a `ReloadEvent::Updated` or `ReloadEvent::Removed` from the watcher channel, it calls `clear_cache()` on the `CachingExecutor`. Full cache clear (not per-tool) — swaps are rare events, and cache misses just cost one re-execution. Per-tool invalidation is an optimization that can be added later if needed.

`ReloadEvent::Loaded` (new skill, no prior cache entries) does not trigger cache clear.

---

## 4) What This Does NOT Include

- **Signature enforcement during hot-reload** — signing infra exists in `fx-skills/src/signing.rs` but wiring it into the reload pipeline is a separate task. For V1, any valid WASM loads. This is a known security gap: the self-improvement architecture spec (§2 "WASM Signing Flow") defines the full signing lifecycle. Hot-reload V1 is the plumbing; signing enforcement is the lock. Signature check will be gated behind config (`require_signatures: bool`).
- **WASM compilation caching** — wasmtime supports pre-compiled modules. Future optimization, not needed for V1.
- **Individual tool definitions (#1171)** — WASM skills currently expose one tool per skill. Exposing each action as a separate tool is tracked separately.
- **Epoch interruption (#1136)** — cancellation of in-flight WASM calls. Orthogonal to hot-reload.

---

## 5) Subtask Decomposition (for subagents)

All subtasks are sequential (each builds on the prior):

### Subtask 1: Registry RwLock Refactor
- Files: `fx-loadable/src/registry.rs`
- Scope: Replace `Vec<Box<dyn Skill>>` with `RwLock<Vec<Arc<dyn Skill>>>`, add `replace_skill()` + `remove_skill()`, refactor `dispatch_call()` to clone-and-release pattern, update all methods to use locks
- Tests: All 25 existing tests must pass + new tests for replace/remove + test that dispatch doesn't hold lock during execute
- Lines: ~100 changed, ~80 new test lines
- **Must not break**: `ToolExecutor` trait impl, `register()` signature (now `&self`)
- **Caller updates**: `tui.rs` changes `Box::new(skill)` → `Arc::new(skill)` for all `register()` calls

### Subtask 2: Validation Helper + Watcher
- Files: `fx-loadable/src/wasm_skill.rs`, `fx-loadable/src/watcher.rs` (new), `fx-loadable/src/lib.rs`, `fx-loadable/Cargo.toml`
- Scope: Extract `load_wasm_skill_from_dir()`, implement `SkillWatcher` with notify + debounce + hash tracking + event channel
- Tests: Watcher tests with tempdir + simulated file events, validation tests
- Lines: ~250 new, ~30 changed
- **Depends on**: Subtask 1 (needs `replace_skill()` / `remove_skill()`)

### Subtask 3: TUI Wiring + Cache Invalidation
- Files: `fx-cli/src/tui.rs`
- Scope: Spawn watcher as tokio task, receive `ReloadEvent`s, call `CachingExecutor::clear_cache()` on Updated/Removed events, optional status bar flash
- Tests: Integration test for reload-triggers-cache-clear
- Lines: ~50 new
- **Depends on**: Subtask 2

---

## 6) Risk Assessment

| Risk | Mitigation |
|------|------------|
| RwLock contention under heavy tool use | Clone-and-release pattern: read lock held only during Arc clone (nanoseconds), not during execution. Write locks only during swap (microseconds). No practical contention. |
| Partially-written WASM files loaded | 500ms per-directory debounce waits for all files (manifest + wasm) to settle. wasmtime compilation validates module integrity as additional guard. |
| notify crate platform differences | Using `RecommendedWatcher` — handles Linux/macOS/Windows. Well-tested crate (100M+ downloads). |
| Stale cache entries after swap | TUI calls `CachingExecutor::clear_cache()` on Updated/Removed events |
| Old skill dropped while in-flight | Dispatch holds `Arc<dyn Skill>` clone; old instance survives until last in-flight call completes |
| Registry API break (register &mut self → &self, Box → Arc) | All callers in tui.rs; update is mechanical |
| Unsigned WASM loaded in V1 | Known gap — self-improvement spec §2 defines signing lifecycle. V1 is plumbing; signing enforcement is a follow-up task gated behind `require_signatures` config. |

---

## 7) Success Criteria

1. Drop a new `.wasm` + `manifest.toml` in `~/.fawx/skills/foo/` → Fawx can use it within 1 second, no restart
2. Update an existing `.wasm` → Fawx uses new version on next tool call
3. Remove a skill directory → tool calls to it return "no skill handles tool" error
4. Malformed WASM → error logged, existing skill (if any) stays loaded
5. All existing tests pass with zero behavior change for non-WASM skills
6. TUI shows reload status (loaded/updated/removed/error)
