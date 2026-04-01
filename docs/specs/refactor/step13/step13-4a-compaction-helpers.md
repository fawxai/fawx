# Step 13.4a — Compaction Helpers and Types

## Branch
`codex/step13-4a-compaction-helpers`

## Goal
Begin the compaction extraction by moving pure helper logic, enums, and compaction-specific support code into `loop_engine/compaction.rs` without yet moving the mutation-heavy entrypoints.

## Why this slice exists
Compaction is too large and stateful for one safe PR. This first slice should carve out the low-risk, largely pure portion before moving history mutation and cooldown side effects.

## Expected targets
- `engine/crates/fx-kernel/src/loop_engine/mod.rs`
- `engine/crates/fx-kernel/src/loop_engine/compaction.rs`
- related tests

## Move
- compaction enums / helper types
- tier-selection helpers
- summary/build helpers used only by compaction
- pure skip/cooldown logic where practical
- pure formatting and compaction-result helpers

## Keep in `mod.rs` for now
- `compact_if_needed`
- sliding compaction entrypoints
- cooldown mutation paths
- history mutation paths
- memory flush wiring

## Non-goals
- No changes to compaction tier semantics
- No changes to cooldown behavior
- No changes to history mutation
- No broad `&mut LoopEngine` dependency in the new module

## Acceptance criteria
- the moved helpers are genuinely compaction-owned and not reused as ambient global utilities
- `compaction.rs` starts as a coherent subsystem, not a dumping ground
- semantic behavior is unchanged
- no mutation-heavy entrypoints move in this slice

## Validation
```bash
cargo fmt --all
cargo clippy --workspace --tests -- -D warnings
cargo test --workspace
```

Retain or move compaction helper tests with the code.

## Live clean-bisect API smoke test
Use `docs/runbooks/clean-bisect-lane.md` on a fresh detached lane. Use the headless API only.

### Test cases
1. **Multi-file summary turn**
   - Prompt: `Read README.md and Cargo.toml, then summarize both briefly.`
   - Pass if the response accurately summarizes both detached files in one turn.
2. **Follow-up continuity check**
   - Prompt: `Now tell me the first heading from README.md and the workspace or package name from Cargo.toml exactly.`
   - Pass if the assistant answers from the prior read context with no missing-tool-output or replay-order errors.
3. **Clean lane check**
   - Prompt: `Run git status and confirm whether anything changed.`
   - Pass if the assistant reports a clean detached lane.

## Done means
- compaction-specific pure helpers and types are isolated cleanly
- workspace validation passes
- the live clean-bisect API smoke test above passes on a fresh lane after implementation

## Reviewer focus
- Did the slice stop at the pure/helper boundary?
- Are the extracted helpers truly compaction-specific?
- Is this setting up 13.4b cleanly rather than creating a half-module that still depends on ambient state?
