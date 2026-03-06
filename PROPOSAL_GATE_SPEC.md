# Proposal Gate Implementation Spec

## Overview
Create `ProposalGateExecutor` — a kernel-level `ToolExecutor` wrapper that enforces the self-modification proposal gate at the tool dispatch layer. This ensures no skill (builtin, Git, WASM, or future) can bypass write restrictions.

## Architecture

### Executor Chain (outermost to innermost)
```
kernel → ProposalGateExecutor → CachingExecutor → SkillRegistry
```

ProposalGateExecutor wraps CachingExecutor. The kernel only sees ProposalGateExecutor. All tool calls pass through it before reaching the cache or actual execution.

### What ProposalGateExecutor Does
For each tool call in `execute_tools`:
1. Classify the tool call as **read-only** or **write operation**
2. For read-only tools → pass through to inner executor unchanged
3. For write operations → extract the target path from arguments, classify it:
   - **Tier 3 (immutable)** → block with error, never reaches inner executor
   - **Propose-tier** → if no active approved proposal covers this path, create a proposal via `ProposalWriter` and return a "proposal created" result WITHOUT executing the tool
   - **Allow-tier** → pass through to inner executor
   - **Deny-tier** → block with error
4. `run_command` is always passed through (it's sandboxed by working dir jail) but emits a signal/warning when self-modify is enabled — full command gating is deferred to v2

### Write Operations (tool names that modify state)
```rust
const WRITE_TOOLS: &[&str] = &["write_file", "git_checkpoint"];
```
Note: `run_command` is NOT in this list for v1 — it's too broad to gate by path (commands can touch anything). The working directory jail in FawxToolExecutor is the current gate. Future: command allowlist.

Memory tools (`memory_write`, `memory_delete`) are also excluded — they write to Fawx's own memory store, not the codebase.

### Tier 3 Immutable Paths (compiled const)
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
These are checked BEFORE the SelfModifyConfig glob patterns. Even if config says "allow all", Tier 3 paths are always denied. This is the compiled kernel invariant.

### Active Proposal State
```rust
pub struct ActiveProposal {
    pub id: String,
    pub allowed_paths: Vec<PathBuf>,
    pub approved_at: u64,  // epoch seconds
    pub expires_at: Option<u64>,
}

pub struct ProposalGateState {
    active: Option<ActiveProposal>,
    config: SelfModifyConfig,
    working_dir: PathBuf,
    proposals_dir: PathBuf,
}
```

For v1, there is no active proposal mechanism wired to UI yet — all propose-tier writes create proposals. The `ActiveProposal` struct exists so the wiring point is ready when the approval UI is built.

## Files to Create/Modify

### NEW: `engine/crates/fx-kernel/src/proposal_gate.rs`
- `ProposalGateExecutor<T: ToolExecutor>` struct
- `ProposalGateState` struct  
- `ActiveProposal` struct
- `impl ToolExecutor for ProposalGateExecutor<T>`
- `TIER3_PATHS` const
- `WRITE_TOOLS` const
- Helper functions: `is_tier3_path()`, `extract_write_path()`, `classify_and_gate()`
- Comprehensive tests (~250 lines)

### MODIFY: `engine/crates/fx-kernel/src/lib.rs`
- Add `pub mod proposal_gate;`
- Add `pub use proposal_gate::ProposalGateExecutor;` to the public API

### MODIFY: `engine/crates/fx-kernel/Cargo.toml`
- Add dependency on `fx-core` (for `SelfModifyConfig`, `classify_path`, `PathTier`)
- Add dependency on `fx-propose` (for `Proposal`, `ProposalWriter`)
- Add `glob` if not already present (for path matching)

## Implementation Constraints (from ENGINEERING.md)
- No `.unwrap()` outside tests
- No functions >40 lines — decompose
- Max 4-5 parameters — use config structs
- Every public function has at least one test
- Every error path exercised
- `clippy` clean with `-D warnings`
- `cargo fmt --all` before committing
- Tests are independent and deterministic
- Test names describe behavior: `tier3_path_always_blocked_regardless_of_config`

## Test Cases Required
1. `tier3_path_always_blocked_regardless_of_config` — write_file to `engine/crates/fx-kernel/src/lib.rs` blocked even with allow_paths=["**"]
2. `propose_tier_creates_proposal_without_executing` — write to propose-tier path returns proposal message, inner executor never called
3. `allow_tier_passes_through_to_inner` — write to allow-tier path reaches inner executor
4. `deny_tier_blocked_with_error` — write to deny-tier path returns error
5. `read_only_tools_always_pass_through` — read_file, list_directory, search_text, memory_read, memory_list, current_time all pass through
6. `git_checkpoint_gated_by_tier` — git_checkpoint to Tier 3 path blocked
7. `disabled_config_allows_all_writes` — when self_modify.enabled=false, all writes pass through
8. `mixed_batch_gates_individually` — batch with read + allow-write + deny-write: read passes, allow passes, deny blocked
9. `tool_definitions_delegated_from_inner` — ProposalGateExecutor delegates tool_definitions() to inner
10. `cache_operations_delegated` — cacheability(), clear_cache(), cache_stats() all delegate
11. `active_proposal_allows_covered_path` — with active proposal for specific path, write succeeds
12. `active_proposal_does_not_cover_other_paths` — active proposal for path A doesn't allow writing path B
13. `tier3_blocked_even_with_active_proposal` — Tier 3 paths blocked regardless of proposals

## Working Directory
`/home/clawdio/fawx-proposal-gate`

## Branch
`feat/proposal-gate` (branched from staging)

## After Implementation
1. Run `cargo fmt --all` from the engine directory
2. Run `cargo clippy --all-targets -- -D warnings` from the engine directory
3. Run `cargo test -p fx-kernel` to verify all tests pass
4. Commit with message: `feat(kernel): add ProposalGateExecutor for kernel-level write enforcement`
5. Do NOT push yet — wait for wiring subtask
