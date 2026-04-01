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

## Live clean-bisect API smoke test
Use `docs/runbooks/clean-bisect-lane.md` on a fresh detached lane. Use the headless API only.

### Test cases
1. **Failed-read recovery**
   - Prompt: `Read DOES_NOT_EXIST_STEP13.md. If it does not exist, say so, then read README.md and quote the first heading exactly.`
   - Pass if the assistant acknowledges the missing file, recovers cleanly, and returns the real README heading without looping.
2. **Absolute-path failure then recovery**
   - Prompt: `Read /definitely/not/here/STEP13.txt. If that fails, read Cargo.toml and tell me the workspace or package name.`
   - Pass if the assistant does not get stuck retrying the bad path and then answers from the real detached file.
3. **No repeated failure loop**
   - Prompt: `Try to read DOES_NOT_EXIST_STEP13.md once. If it is missing, stop retrying and read README.md instead.`
   - Pass if the turn terminates without repeated failing tool calls.

## Done means
- retry types and helpers are isolated in their module
- workspace validation passes
- the live clean-bisect API smoke test above passes on a fresh lane after implementation

## Reviewer focus
- Is retry policy now cleanly isolated?
- Did this remain a pure extraction rather than a hidden behavior change?
- Are retry types and helpers scoped tightly rather than made broadly public?
