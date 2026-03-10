# Subtask 1: Wire Signature Verification into WASM Load Path

**Branch:** `feat/wasm-signing` (from `staging`)
**PR target:** `staging`
**Scope:** fx-loadable, fx-config (~120 lines)

---

## Context

Ed25519 signing primitives exist in `fx-skills/src/signing.rs` (sign, verify, keygen) and the `SkillLoader` in `fx-skills/src/loader.rs` already accepts `trusted_keys` and optional `signature` parameters. **None of this is wired up.** The `compile_skill()` function in `fx-loadable/src/wasm_skill.rs` creates `SkillLoader::new(vec![])` (empty trusted keys) and passes `None` for signature. This subtask closes that gap.

---

## Requirements

### 1. Config: `[security]` section in fx-config

Add a new `SecurityConfig` section to `FawxConfig`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct SecurityConfig {
    /// When true, reject any WASM skill without a valid signature.
    /// When false (default), unsigned skills load with a warning.
    /// Invalid signatures are ALWAYS rejected regardless of this setting.
    pub require_signatures: bool,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            require_signatures: false,
        }
    }
}
```

Add `pub security: SecurityConfig` to `FawxConfig`. Update `DEFAULT_CONFIG_TEMPLATE` with:
```toml
# [security]
# require_signatures = false
```

### 2. Trusted key loading

Add a function in `fx-loadable/src/wasm_skill.rs`:

```rust
/// Load Ed25519 public keys from `~/.fawx/trusted_keys/*.pub`.
/// Each file contains raw 32-byte Ed25519 public key.
/// Returns empty vec if directory doesn't exist.
pub fn load_trusted_keys() -> Result<Vec<Vec<u8>>, SkillError>
```

Directory: `~/.fawx/trusted_keys/` — each `.pub` file is a raw 32-byte Ed25519 public key.

### 3. Signature file convention

When loading a skill from directory, look for `{name}.wasm.sig` alongside `{name}.wasm`. The `.sig` file contains raw Ed25519 signature bytes (64 bytes).

### 4. Wire into `load_wasm_skill_from_dir()`

Update the function signature to accept security config and trusted keys:

```rust
pub fn load_wasm_skill_from_dir(
    skill_dir: &Path,
    credential_provider: Option<Arc<dyn CredentialProvider>>,
    trusted_keys: &[Vec<u8>],
    require_signatures: bool,
) -> Result<(WasmSkill, [u8; 32]), SkillError>
```

Logic:
1. Read manifest and WASM bytes (existing)
2. Check for `{name}.wasm.sig`:
   - **Sig exists + trusted keys available:** pass both to `SkillLoader` → verify. Reject on failure.
   - **Sig exists + no trusted keys:** warn "signature found but no trusted keys configured", load anyway (can't verify).
   - **No sig + `require_signatures == true`:** reject with clear error.
   - **No sig + `require_signatures == false`:** warn "loading unsigned skill", load normally.
   - **Sig invalid (verification fails):** ALWAYS reject, regardless of `require_signatures`.
3. Pass trusted keys to `SkillLoader::new(trusted_keys)` or `SkillLoader::with_engine(engine, trusted_keys)`.

### 5. Update `compile_skill()`

Currently:
```rust
fn compile_skill(wasm_bytes: &[u8], manifest: &SkillManifest) -> Result<LoadedSkill, SkillError> {
    fx_skills::loader::SkillLoader::new(vec![])
        .load(wasm_bytes, manifest, None)
}
```

Change to:
```rust
fn compile_skill(
    wasm_bytes: &[u8],
    manifest: &SkillManifest,
    signature: Option<&[u8]>,
    trusted_keys: &[Vec<u8>],
) -> Result<LoadedSkill, SkillError>
```

### 6. Update `load_wasm_skills()`

Pass security config and trusted keys through:

```rust
pub fn load_wasm_skills(
    credential_provider: Option<Arc<dyn CredentialProvider>>,
    trusted_keys: &[Vec<u8>],
    require_signatures: bool,
) -> Result<Vec<Arc<dyn Skill>>, SkillError>
```

### 7. Update callers

**`fx-loadable/src/watcher.rs`**: `SkillWatcher` needs trusted keys and require_signatures. Add to constructor and use in `handle_change()` where it calls `load_wasm_skill_from_dir()`. The watcher should also watch `.sig` file changes (already watches the skill directory recursively, so `.sig` changes should trigger debounced reload).

**`fx-cli/src/tui.rs`** (or wherever `load_wasm_skills` / `SkillWatcher::new` are called): Load config, call `load_trusted_keys()`, pass both through. This is a mechanical caller update.

### 8. Helper: read_signature_file()

```rust
/// Read `{name}.wasm.sig` from a skill directory, if present.
/// Returns None if file doesn't exist, Err on read failure.
fn read_signature_file(skill_dir: &Path, name: &str) -> Result<Option<Vec<u8>>, SkillError>
```

---

## Tests Required

1. **Unsigned skill loads when `require_signatures = false`** — existing behavior preserved
2. **Unsigned skill rejected when `require_signatures = true`** — clear error message
3. **Signed skill with valid signature loads** — keygen, sign, write `.sig`, load
4. **Signed skill with invalid signature always rejected** — even with `require_signatures = false`
5. **Signed skill with tampered WASM rejected** — sign, modify bytes, load fails
6. **Signature present but no trusted keys** — logs warning, loads skill
7. **`load_trusted_keys()` reads `.pub` files from directory**
8. **`load_trusted_keys()` returns empty vec when directory missing**
9. **`read_signature_file()` returns None when no `.sig` file**
10. **`read_signature_file()` reads valid `.sig` file**
11. **`SecurityConfig` defaults and roundtrip serialization**
12. **Config template includes `[security]` section**

---

## Files to Modify

1. `engine/crates/fx-config/src/lib.rs` — add `SecurityConfig`, update `FawxConfig`, update template
2. `engine/crates/fx-loadable/src/wasm_skill.rs` — main changes: `load_trusted_keys()`, `read_signature_file()`, update `compile_skill()`, `load_wasm_skill_from_dir()`, `load_wasm_skills()`
3. `engine/crates/fx-loadable/src/watcher.rs` — add trusted_keys + require_signatures to `SkillWatcher`, pass through in `handle_change()`
4. `engine/crates/fx-cli/src/tui.rs` — caller update to pass config/keys (mechanical)
5. `engine/crates/fx-cli/src/main.rs` — if `load_wasm_skills` is called here too, update caller

---

## What NOT to Do

- Do not modify `fx-skills/src/signing.rs` or `fx-skills/src/loader.rs` — the primitives are complete.
- Do not implement CLI commands (key management, signing) — that's subtask 2+3.
- Do not add key generation or signing to this subtask — verification only.
- Do not use `ring` directly in fx-loadable — call through `fx-skills::signing` and `fx-skills::loader`.
