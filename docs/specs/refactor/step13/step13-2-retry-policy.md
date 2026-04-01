# Step 13.2 — Retry Policy Extraction

## Branch
`codex/step13-2-retry-policy`

## Goal
Extract retry-policy state and helpers into `loop_engine/retry.rs` with no behavior change.

## Why this slice exists
The retry subsystem is self-contained stateful logic. It is one of the cleanest remaining seams in `loop_engine/mod.rs` and should move early before heavier tool-execution work.

## Expected targets
- `engine/crates/fx-kernel/src/loop_engine/mod.rs`
- `engine/crates/fx-kernel/src/loop_engine/retry.rs`
- related tests

## Move
- `RetryTracker`
- `RetryVerdict`
- `ToolCallKey`
- retry hash helpers
- retry reason builders
- other helpers used only by retry policy

## Keep in `mod.rs`
- the orchestrator’s decision to invoke retry policy
- tool execution logic that consumes retry verdicts

## Non-goals
- No changes to retry thresholds
- No changes to no-progress detection semantics
- No changes to tool loop behavior
- No coupling this module to streaming/compaction/decomposition

## Acceptance criteria
- `retry.rs` owns retry state and helpers
- call sites are rewired without changing semantics
- retry-specific tests move with the subsystem or are added alongside it
- the module does not require broad access to unrelated engine state

## Validation
```bash
cargo fmt --all
cargo clippy --workspace --tests -- -D warnings
cargo test --workspace
```

Pay special attention to existing regressions around repeated tool failures and no-progress loops.

## Reviewer focus
- Is retry policy now cleanly isolated?
- Did this remain a pure extraction rather than a hidden behavior change?
- Are retry types and helpers scoped tightly rather than made broadly public?
