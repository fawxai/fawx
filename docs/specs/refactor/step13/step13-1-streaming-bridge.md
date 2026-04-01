# Step 13.1 — Streaming Bridge Extraction

## Branch
`codex/step13-1-streaming-bridge`

## Goal
Extract the streaming bridge and phase-text infrastructure from `loop_engine/mod.rs` into `loop_engine/streaming.rs` with no behavior change.

## Why this slice exists
This is the lowest-risk extraction in Step 13. The logic is infrastructure, not business policy. It is used by reasoning and tool execution, but it does not conceptually belong to the orchestrator.

## Scope
Move the streaming/event infrastructure only.

## Expected targets
- `engine/crates/fx-kernel/src/loop_engine/mod.rs`
- `engine/crates/fx-kernel/src/loop_engine/streaming.rs`
- related tests moved or added under loop-engine tests if needed

## Move
- `StreamCallbackRef`
- provider stream bridge helpers
- phase-text buffering helpers
- phase-text flush helpers
- other tightly-coupled streaming/event utility functions used only by this subsystem

## Keep in `mod.rs`
- orchestrator entrypoints
- `reason()` itself
- decision logic
- tool execution logic
- compaction logic

## Non-goals
- No change to event semantics
- No change to streaming payload shape
- No new traits or callback abstractions
- No changes to reasoning logic beyond import rewiring

## Acceptance criteria
- `streaming.rs` becomes the single owner of the streaming bridge helpers
- `mod.rs` only imports and uses them
- existing streaming behavior remains identical
- no `.unwrap()` outside tests
- no functions >40 lines without decomposition

## Validation
```bash
cargo fmt --all
cargo clippy --workspace --tests -- -D warnings
cargo test --workspace
```

Also run targeted tests for streaming/event emission if they already exist.

## Reviewer focus
- Is this a pure extraction with no semantic drift?
- Did the slice avoid dragging reasoning/tool policy into the streaming module?
- Are module boundaries based on responsibility rather than line-count reduction?
