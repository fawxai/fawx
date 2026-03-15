# Kernel Blindness Specification

**Status:** DRAFT  
**Phase:** Pre-Phase 4 (foundational security)  
**Priority:** High — required before any public demo or investor conversation  
**Parent doc:** `docs/architecture/open-core-security-model.md`

---

## 1. Goal

Fawx cannot read, inspect, or reverse-engineer its own kernel source code or binary internals. This is enforced at the tool executor level with compiled restrictions — not by policy, prompting, or configuration.

---

## 2. Scope

### In scope
- Kernel source path deny list (compiled const)
- Reverse engineering tool deny list (compiled const)
- Error message sanitization (strip internal structure from user-facing errors)
- Panic path sanitization (abort + strip location detail in release builds)

### Out of scope
- WASM capability restrictions (separate spec)
- Proposal gate UX hardening (separate spec)
- Behavioral telemetry (separate spec)

---

## 3. Kernel Source Path Deny List

### Implementation

Extend `TIER3_PATHS` (or create a new `KERNEL_BLIND_PATHS` compiled const) to include all kernel source directories:

```
engine/crates/fx-kernel/
engine/crates/fx-security/
engine/crates/fx-policy/
engine/crates/fx-gate/
engine/crates/fx-consensus/     (experiment protocol internals)
```

The tool executor must deny **read** access to any path matching these prefixes, regardless of how the path is constructed (relative, absolute, symlinked, `../` traversal).

### Path normalization

Before matching, paths must be:
1. Canonicalized (resolve symlinks, `..`, `.`)
2. Checked against the deny list as prefix matches
3. Denied even if accessed indirectly (e.g., `cat`, `grep`, `find` targeting these dirs)

### What this blocks
- `read_file("engine/crates/fx-kernel/src/gate.rs")` → denied
- Shell commands: `cat engine/crates/fx-kernel/src/gate.rs` → denied
- Glob reads: `find engine/ -name "*.rs"` → results filtered to exclude kernel paths
- Git commands: `git show HEAD:engine/crates/fx-kernel/src/gate.rs` → denied

### What this allows
- Reading loadable layer source (`engine/crates/fx-loadable/`, `engine/crates/fx-journal/`, skill SDK)
- Reading documentation, specs, architecture docs
- Reading config files (non-sensitive sections)
- Using kernel APIs through trait interfaces (the normal tool calling path)

---

## 4. Reverse Engineering Tool Deny List

### Implementation

The tool executor must deny shell invocations that target the Fawx binary with reverse engineering tools:

```
strings <fawx-binary-path>
objdump <fawx-binary-path>
otool <fawx-binary-path>
nm <fawx-binary-path>
readelf <fawx-binary-path>
hexdump <fawx-binary-path>
xxd <fawx-binary-path>
radare2 <fawx-binary-path>
r2 <fawx-binary-path>
gdb <fawx-binary-path>
lldb <fawx-binary-path>
```

Detection must handle:
- Absolute and relative paths to the binary
- Piped commands (`cat /path/to/fawx | strings`)
- Arguments in any order (`strings -a /path/to/fawx`)

### Binary path resolution

The deny list must know the path to its own binary. Use `std::env::current_exe()` at startup and store as a runtime constant. Also deny targeting any file in the binary's directory that could be a debug artifact (`.dSYM`, `.pdb`, core dumps).

---

## 5. Error Message Sanitization

### Principle

Kernel error types must not leak internal structure (module paths, function names, enum variant names, budget values) to the agent in release builds.

### Implementation

Create a `UserFacingError` trait:

```rust
pub trait UserFacingError {
    /// Safe error message for the agent. No internal details.
    fn user_message(&self) -> String;
    
    /// Full error with internals. Debug builds and logs only.
    fn internal_message(&self) -> String;
}
```

All kernel error types implement this trait. The tool executor returns `user_message()` to the agent and logs `internal_message()` to the server log.

### Examples

| Internal error | User-facing message |
|---------------|-------------------|
| `ProposalGateError::Tier3PathViolation { path: "engine/crates/fx-kernel/src/gate.rs" }` | `"This path is protected and cannot be accessed."` |
| `PolicyEngine::BudgetExceeded { limit: 10, used: 10 }` | `"Tool call budget exceeded for this turn."` |
| `ToolExecutor::KernelPathDenied { tool: "read_file", path: "..." }` | `"This file is not available."` |

### What NOT to sanitize

- Loadable layer errors (skill failures, memory errors) — these are the agent's domain, full details help it debug
- Network/API errors — the agent needs these to handle retries
- Config errors — the agent may help the user fix config issues

---

## 6. Panic Path Sanitization

### Release profile (Cargo.toml)

```toml
[profile.release]
panic = 'abort'
strip = true
```

### Panic location detail

When `rustc` stabilizes `-Zlocation-detail=none`, enable it for release builds. This strips file paths and line numbers from panic messages.

Until then, audit all kernel crates for panics that contain revealing messages and replace with generic ones. Use `expect("internal error")` instead of `expect("ProposalGate validation failed at step 3")`.

---

## 7. Testing

### Unit tests

- Path deny list correctly blocks all kernel paths (direct, relative, `../` traversal, symlink)
- Path deny list allows loadable layer paths
- RE tool deny list blocks all listed tools targeting the binary
- RE tool deny list allows the same tools targeting other files
- Error sanitization produces generic messages for all kernel error variants
- Error sanitization preserves full detail for loadable layer errors

### Integration tests

- End-to-end: agent tool call attempting to read kernel source → receives generic denial
- End-to-end: agent shell command attempting `strings` on binary → denied
- End-to-end: agent receives sanitized error from proposal gate denial

### Adversarial tests

- Path traversal attempts (`../../engine/crates/fx-kernel/`)
- Symlink attacks (create symlink to kernel source, read via symlink)
- Indirect reads (`grep -r "fn validate" engine/`)  
- Encoded paths (URL encoding, unicode tricks)

---

## 8. Acceptance Criteria

1. No tool call or shell command can read any file under kernel source paths
2. No tool call or shell command can run RE tools against the Fawx binary
3. All kernel errors returned to the agent contain no internal module paths, function names, or enum variant names
4. Release binary contains no panic location info for kernel crates
5. All enforcement is via compiled constants — no config, env var, or flag can disable it
6. Full test coverage for all deny paths including adversarial cases
