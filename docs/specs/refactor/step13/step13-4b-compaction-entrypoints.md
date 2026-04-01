# Step 13.4b — Compaction Entrypoints and State Handoff

## Branch
`codex/step13-4b-compaction-entrypoints`

## Goal
Finish the compaction extraction by moving the mutation-heavy entrypoints and stateful compaction flow into `loop_engine/compaction.rs`.

## Why this slice exists
After 13.4a, only the risky part remains: code that mutates history, applies cooldowns, selects compaction paths, and coordinates memory flush behavior. This should be isolated in its own PR.

## Expected targets
- `engine/crates/fx-kernel/src/loop_engine/mod.rs`
- `engine/crates/fx-kernel/src/loop_engine/compaction.rs`
- related tests

## Move
- `compact_if_needed`
- sliding compaction entrypoints
- compaction application helpers
- cooldown mutation wiring
- history mutation wiring
- memory flush integration needed by compaction

## Interface constraint
Do not pass broad `&mut LoopEngine` into everything if a narrower state bundle will do. If needed, introduce a small compaction-specific context/state struct.

## Keep in `mod.rs`
- orchestration call sites
- top-level perceive → reason → decide → act → compact → terminate flow

## Non-goals
- No compaction redesign
- No new compaction tiers
- No policy changes to when compaction triggers
- No mixing this PR with decomposition or tool-execution work

## Acceptance criteria
- orchestrator calls a compaction subsystem entrypoint
- behavior remains unchanged across existing compaction regressions
- cooldown and history mutation remain correct
- the new interface is narrower than ambient full-engine mutation where practical

## Validation
```bash
cargo fmt --all
cargo clippy --workspace --tests -- -D warnings
cargo test --workspace
```

This slice should also preserve any targeted compaction regression tests already in the repo.

## Reviewer focus
- Did the PR preserve history mutation semantics?
- Is the interface clean, or did it just move ambient state behind a different file?
- Are compaction side effects still explicit and testable?
