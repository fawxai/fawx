# Spec: #1647 — Revisioned Skill Activation Lifecycle

## Status
Not started. No revision tracking, activation management, or source-awareness exists.

## Goal
Introduce a proper skill activation lifecycle with revision tracking, source awareness, atomic activation, and rollback. The live kernel must always reason against the actual active artifact contract, not stale installed copies.

## Current State (dev branch, post #1648 merge)

### What exists

**Skill loading pipeline:**
1. `SkillRegistry` (`fx-loadable/src/registry.rs`) — holds loaded skills, provides lookup
2. `SkillWatcher` (`fx-loadable/src/watcher.rs`) — watches `~/.fawx/skills/` for filesystem changes, triggers reload
3. `WasmSkill` (`fx-loadable/src/wasm_skill.rs`) — loads and executes WASM skill artifacts
4. Skills are installed by copying artifacts to `~/.fawx/skills/<name>/`
5. Hot-reload triggers on filesystem events (create, modify, delete in the skills directory)

**What's missing:**
- No concept of where a skill came from (published marketplace, local dev, built-in)
- No revision tracking (content hash, manifest hash, version comparison)
- No activation gate (install = immediately live)
- No signature verification before activation (signatures exist but are checked at install, not at activation)
- No rollback capability
- No way to detect stale installed artifacts vs updated source

### The stale artifact problem (from the weather regression)

1. Repo source manifest gets `[[tools]]` with structured schema
2. Dev rebuilds and tests locally — works
3. Installed artifact in `~/.fawx/skills/weather-skill/` still has the old manifest (no `[[tools]]`)
4. Live kernel loads the installed artifact, not the source
5. Direct utility routing doesn't activate because the live manifest has no tool metadata
6. System silently degrades to legacy planner loop

This is invisible. No error, no warning. The system just behaves differently than source would suggest.

## Design

### New types

```rust
/// Where a skill artifact originated.
pub enum SkillSource {
    /// From the marketplace registry, with publisher info.
    Published { publisher: String, registry_url: String },
    /// Built from local source during development.
    LocalDev { source_path: PathBuf },
    /// Bundled with the Fawx binary.
    Builtin,
}

/// A specific revision of a skill artifact.
pub struct SkillRevision {
    /// Content hash of the WASM binary.
    pub content_hash: String,
    /// Hash of the manifest.toml.
    pub manifest_hash: String,
    /// Version from manifest.
    pub version: String,
    /// Signature verification status.
    pub signature: SignatureStatus,
    /// Extracted tool contracts from this revision's manifest.
    pub tool_contracts: Vec<ToolDefinition>,
    /// When this revision was created/staged.
    pub staged_at: u64,
}

pub enum SignatureStatus {
    Valid { signer: String },
    Invalid,
    Unsigned,
}

/// The currently active revision of a skill.
pub struct SkillActivation {
    /// The active revision.
    pub revision: SkillRevision,
    /// Source of the active artifact.
    pub source: SkillSource,
    /// When activation occurred.
    pub activated_at: u64,
    /// Previous revision for rollback.
    pub previous: Option<Box<SkillRevision>>,
}
```

### `SkillLifecycleManager`

New component that sits between the filesystem/installer and the registry:

```rust
pub struct SkillLifecycleManager {
    activations: HashMap<String, SkillActivation>,
    staged: HashMap<String, SkillRevision>,
}

impl SkillLifecycleManager {
    /// Stage a new revision without activating it.
    pub fn stage(&mut self, name: &str, revision: SkillRevision) -> Result<(), LifecycleError>;

    /// Verify and activate a staged revision. Atomic swap.
    pub fn activate(&mut self, name: &str) -> Result<(), LifecycleError>;

    /// Rollback to previous revision.
    pub fn rollback(&mut self, name: &str) -> Result<(), LifecycleError>;

    /// Get the active revision for a skill.
    pub fn active(&self, name: &str) -> Option<&SkillActivation>;

    /// Check if installed artifact matches expected revision.
    pub fn is_current(&self, name: &str, installed_manifest_hash: &str) -> bool;
}
```

### Integration points

1. **SkillWatcher** calls `lifecycle.stage()` on filesystem events, then `lifecycle.activate()` if auto-activate is enabled (default for local dev)
2. **Marketplace install** calls `lifecycle.stage()`, verifies signature, then `lifecycle.activate()`
3. **SkillRegistry** reads from `lifecycle.active()` instead of directly from filesystem
4. **Startup** logs warnings for skills where installed artifact hash doesn't match the latest known revision

## Deliverables

### Phase 1: Revision tracking

1. Define `SkillSource`, `SkillRevision`, `SignatureStatus`, `SkillActivation` types in new `fx-loadable/src/lifecycle.rs`
2. Compute content hash (SHA-256 of WASM binary) and manifest hash (SHA-256 of manifest.toml) at load time
3. Store activation state in `SkillRegistry` alongside the loaded skill
4. Log at startup when a skill is loaded, including source + revision info

### Phase 2: Lifecycle manager

5. Implement `SkillLifecycleManager` with stage/activate/rollback
6. Wire `SkillWatcher` to go through lifecycle manager instead of directly updating registry
7. Add stale-artifact detection: warn when installed manifest doesn't match source (if source path is known)

### Phase 3: Activation gates

8. Published skills: verify signature before activation
9. Emit `SkillActivated` / `SkillRolledBack` events through the event bus
10. Add `/skills status` command showing: name, version, source, revision hash, activation time, signature status

## Files to modify
- `engine/crates/fx-loadable/src/lifecycle.rs` (new — types + manager)
- `engine/crates/fx-loadable/src/registry.rs` (integrate lifecycle)
- `engine/crates/fx-loadable/src/watcher.rs` (route through lifecycle)
- `engine/crates/fx-loadable/src/lib.rs` (re-exports)
- `engine/crates/fx-cli/src/headless.rs` or commands (add /skills status)

## Depends on
- #1646 (visible tool contracts) — tool contracts are extracted per-revision
- #1639 (tool trait) — unification of built-in and loadable tool contracts

## Not in scope
- Marketplace registry protocol changes
- Automatic update polling
- Multi-version concurrent activation
- Sandbox policy per-revision
