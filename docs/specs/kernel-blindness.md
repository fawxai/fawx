# Kernel Blindness Specification

**Status:** DRAFT (R2 — revised per architectural review)  
**Phase:** Pre-Phase 4 (foundational security)  
**Priority:** High — required before any public demo or investor conversation  
**Parent doc:** `docs/architecture/open-core-security-model.md`  
**Review:** [Issue #1418](https://github.com/abbudjoe/fawx/issues/1418)

---

## 1. Goal

Fawx cannot read, inspect, or reverse-engineer its own kernel source code or binary internals. This is enforced in the `ProposalGateExecutor` (kernel layer) with compiled restrictions — not by policy, prompting, or configuration.

---

## 2. Scope

### In scope
- Kernel source path deny list (compiled const, read enforcement)
- Tool-level read filtering (`read_file`, `search_text`, `list_directory`, shell commands, git commands)
- Reverse engineering tool deny list (best-effort shell filtering)
- Error message sanitization (strip internal structure from agent-facing errors)
- Panic path sanitization (abort + strip location detail in release builds)

### Out of scope
- WASM capability restrictions (separate spec: `wasm-security-hardening.md`)
- Proposal gate UX hardening (separate spec: `proposal-gate-hardening.md`)
- Behavioral telemetry (separate spec: `behavioral-telemetry.md`)

---

## 3. Kernel Source Path Deny List

### Existing TIER3_PATHS (write-deny, currently enforced)

The `ProposalGateExecutor` in `engine/crates/fx-kernel/src/proposal_gate.rs` already enforces a compiled const `TIER3_PATHS` that **denies writes**:

```rust
const TIER3_PATHS: &[&str] = &[
    "engine/crates/fx-kernel/",
    "engine/crates/fx-auth/src/crypto/",
    ".github/",
    "fawx-ripcord/",
    "tests/invariant/",
    "prompt-ledger/",
    "snapshots/",
];
```

### New: KERNEL_BLIND_PATHS (read-deny)

Add a new compiled const for **read** enforcement. This is separate from TIER3_PATHS because the threat model is different: TIER3 prevents the agent from modifying safety-critical code; KERNEL_BLIND prevents the agent from reading kernel internals.

```rust
const KERNEL_BLIND_PATHS: &[&str] = &[
    // Kernel core — loop orchestrator, proposal gate, enforcement
    "engine/crates/fx-kernel/",
    // Auth + crypto — credential store internals
    "engine/crates/fx-auth/",
    // Security — audit, security checks
    "engine/crates/fx-security/",
    // Consensus — experiment protocol internals
    "engine/crates/fx-consensus/",
    // Ripcord — safety recovery mechanism
    "fawx-ripcord/",
    // Invariant tests — reveal enforcement boundary details
    "tests/invariant/",
];
```

**Rationale for each path:**
- `fx-kernel/` — loop orchestrator, proposal gate, enforcement logic (the core blind target)
- `fx-auth/` — credential store, crypto primitives (broader than just `src/crypto/`)
- `fx-security/` — security audit implementation
- `fx-consensus/` — experiment protocol internals (scoring, evaluation, tamper detection)
- `fawx-ripcord/` — safety recovery mechanism
- `tests/invariant/` — invariant tests reveal what the enforcement boundaries are

**Explicitly NOT blinded (loadable layer, agent can read):**
- `fx-loadable/`, `fx-skills/`, `fx-tools/`, `fx-journal/`, `fx-memory/`
- `fx-llm/`, `fx-channel-*`, `fx-fleet/`, `fx-cron/`
- `fx-cli/` (user-facing CLI, not enforcement)
- `docs/`, `scripts/`, config files

### Enforcement layer: ProposalGateExecutor

Read enforcement goes in `ProposalGateExecutor`, NOT in `BuiltinToolsSkill` or individual tool implementations. Rationale:
- `ProposalGateExecutor` is kernel code — it wraps any `ToolExecutor` and intercepts all tool calls
- Putting enforcement in the loadable layer means a compromised/replaced skill could bypass it
- The gate already has path matching infrastructure for TIER3 writes — extending to reads is architecturally consistent

### Path normalization

The current `normalize_relative()` in `ProposalGateExecutor` is string-based. For kernel blindness, this is **sufficient** because:
- Fawx runs from a known working directory (the repo root)
- All tool calls go through the gate, which normalizes paths before matching
- Symlink resolution (`std::fs::canonicalize`) would require filesystem access and could fail for non-existent paths

**String normalization rules (existing, extend):**
1. Collapse `./` prefixes
2. Resolve `../` components against the working directory
3. Strip leading `/` if within repo root
4. Prefix match against `KERNEL_BLIND_PATHS`

**Known limitation:** Symlink attacks (create symlink pointing to kernel source, read via symlink) are not blocked by string normalization alone. Mitigation: the agent cannot create symlinks to kernel paths because TIER3 write-deny already prevents writing into those directories. An attacker would need to pre-place symlinks, which is outside the threat model (the threat model is the agent itself, not a pre-compromised filesystem).

---

## 4. Tool-Level Read Filtering

### Tools that need kernel blindness enforcement

| Tool | How it reads | Enforcement |
|------|-------------|-------------|
| `read_file` | Direct path argument | Deny if path matches KERNEL_BLIND_PATHS |
| `search_text` | Path argument + content search | Deny if search path matches; filter results to exclude matches in blind paths |
| `list_directory` | Path argument | Deny if path matches; filter results to exclude blind path entries |
| `shell` / `bash` | Arbitrary commands | Best-effort command filtering (see §4.1) |

### 4.1 Shell command filtering (best-effort)

Shell commands can read files in unlimited ways. **Hard guarantee is infeasible** — variable expansion, base64 encoding, script writing, and aliases can all bypass string matching.

**Best-effort approach:**
- Block obvious direct reads: `cat`, `head`, `tail`, `less`, `more`, `bat` with kernel path arguments
- Block grep/search targeting kernel dirs: `grep -r ... engine/crates/fx-kernel/`
- Block git source access: `git show`, `git log -p`, `git diff`, `git blame` targeting kernel paths
- Log all shell commands for post-hoc audit

**Explicitly scoped as best-effort:** A determined agent with shell access can likely find a way to read kernel source through shell. The primary defense is the structured tool deny (read_file, search_text, list_directory), which covers the normal tool-calling path. Shell filtering is defense-in-depth, not the primary guarantee.

### 4.2 Git history commands

Git can reveal kernel source through history. Block:
- `git show <ref>:<kernel-path>`
- `git log -p -- <kernel-path>`
- `git diff ... -- <kernel-path>`
- `git blame <kernel-path>`

Implementation: scan shell command for git subcommands + kernel path arguments.

### 4.3 /proc filesystem (Linux)

On Linux, `/proc/self/exe` provides a symlink to the running binary. Block:
- `read_file("/proc/self/exe")` and variations (`/proc/<pid>/exe`)
- Shell commands targeting `/proc/self/exe` (same best-effort approach as §4.1)

Add `/proc/self/exe` and `/proc/self/maps` to the read-deny list.

---

## 5. Reverse Engineering Tool Deny List (best-effort)

### Implementation

Best-effort shell filtering for commands targeting the Fawx binary:

```
strings, objdump, otool, nm, readelf, hexdump, xxd,
radare2, r2, gdb, lldb, ida, hopper
```

### Binary path resolution

Use `std::env::current_exe()` at startup, store as a runtime value (not compiled const — the path isn't known at compile time). Also deny targeting:
- Files in the binary's directory: `.dSYM`, `.pdb`, core dumps
- `/proc/self/exe` (Linux)

### Scoping

This is **explicitly best-effort**. A determined agent can bypass shell filtering. The real defense is that the binary is compiled and stripped — even if the agent reads the bytes, it gets a binary blob, not source code. This layer exists to block casual/accidental exposure, not to stop a sophisticated attack.

---

## 6. Error Message Sanitization

### Principle

Kernel error types must not leak internal structure (module paths, function names, enum variant names, budget values) to the agent.

### Implementation

Create a `UserFacingError` trait in `fx-kernel`:

```rust
pub trait UserFacingError {
    /// Safe error message for the agent. No internal details.
    fn user_message(&self) -> String;
    
    /// Full error with internals. Logs only.
    fn internal_message(&self) -> String;
}
```

All kernel error types in `fx-kernel` and `fx-auth` implement this trait. The `ProposalGateExecutor` returns `user_message()` to the agent and logs `internal_message()` to the server log.

### Examples

| Internal error | User-facing message |
|---------------|-------------------|
| `ProposalGateError::Tier3PathViolation { path: "..." }` | `"This path is protected and cannot be modified."` |
| `KernelBlindDenied { tool: "read_file", path: "..." }` | `"This file is not available."` |
| Retry budget exceeded (limit=10, used=10) | `"Tool call budget exceeded for this turn."` |

### What NOT to sanitize

- Loadable layer errors (skill failures, memory errors) — the agent needs these
- Network/API errors — needed for retry logic
- Config errors — the agent may help users fix config

---

## 7. Panic Path Sanitization

### Release profile (Cargo.toml)

```toml
[profile.release]
panic = 'abort'
strip = true
```

### Panic message audit

Audit all kernel crates (`fx-kernel`, `fx-auth`, `fx-security`, `fx-consensus`) for panics with revealing messages. Replace with generic ones:
- `expect("internal error")` instead of `expect("ProposalGate validation failed at step 3")`
- No module paths, function names, or step numbers in panic messages

---

## 8. Implementation Plan

### Subtask breakdown (ordered, each is one PR)

1. **Add `KERNEL_BLIND_PATHS` const + read deny in ProposalGateExecutor** (~150 lines)
   - New const, extend `execute_tools` to check reads, unit tests
   - Files: `engine/crates/fx-kernel/src/proposal_gate.rs`

2. **Filter `search_text` and `list_directory` results** (~100 lines)
   - Filter search results and directory listings to exclude kernel paths
   - Files: `engine/crates/fx-kernel/src/proposal_gate.rs` or wherever these tools are intercepted

3. **Shell command filtering (best-effort)** (~200 lines)
   - Scan shell commands for direct reads, git history, RE tools targeting kernel paths or binary
   - `/proc/self/exe` deny
   - Files: `engine/crates/fx-kernel/src/proposal_gate.rs` (new module or method)

4. **UserFacingError trait + error sanitization** (~150 lines)
   - Trait definition, impl for ProposalGateError and kernel error types
   - Wire into tool result return path
   - Files: `engine/crates/fx-kernel/src/` (new file + proposal_gate.rs changes)

5. **Release profile hardening** (~10 lines)
   - `panic = 'abort'`, `strip = true` in Cargo.toml release profile
   - Audit and replace revealing panic messages in kernel crates

---

## 9. Testing

### Unit tests
- KERNEL_BLIND_PATHS blocks all listed paths (direct, relative, `../` traversal)
- KERNEL_BLIND_PATHS allows loadable layer paths
- `search_text` results filtered to exclude kernel path matches
- `list_directory` results filtered to exclude kernel path entries
- Shell filtering blocks `cat`, `grep`, `git show` targeting kernel paths
- Shell filtering allows the same commands targeting loadable layer paths
- `/proc/self/exe` read denied
- Error sanitization produces generic messages for all kernel error variants
- Error sanitization preserves full detail for loadable layer errors

### Adversarial tests
- Path traversal: `../../engine/crates/fx-kernel/`
- Indirect reads: `grep -r "fn validate" engine/` (results filtered)
- Git history: `git log -p -- engine/crates/fx-kernel/src/proposal_gate.rs`
- `/proc` bypass: `cat /proc/self/exe`
- Shell encoding: base64/variable expansion (document as known limitation, not test expectation)

---

## 10. Acceptance Criteria

1. No structured tool call (`read_file`, `search_text`, `list_directory`) can access kernel source paths — **hard guarantee**
2. Shell command filtering blocks obvious direct reads, git history access, and RE tools — **best-effort**
3. All kernel errors returned to the agent contain no internal module paths, function names, or enum variant names
4. Release binary is stripped with `panic = 'abort'`
5. All enforcement is in `ProposalGateExecutor` (kernel layer) via compiled constants
6. Full test coverage for structured tool deny paths including adversarial cases
7. Shell filtering limitations are documented, not hidden
