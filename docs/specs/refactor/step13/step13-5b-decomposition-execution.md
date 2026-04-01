# Step 13.5b — Decomposition Execution Paths

## Branch
`codex/step13-5b-decomposition-execution`

## Goal
Finish the decomposition extraction by moving recursive sub-goal execution into `loop_engine/decomposition.rs`.

## Why this slice exists
After 13.5a, the remaining work is the execution path: sequential and concurrent sub-goal execution, child engine construction, follow-up execution, and result wiring. This is the riskier half and needs its own PR.

## Expected targets
- `engine/crates/fx-kernel/src/loop_engine/mod.rs`
- `engine/crates/fx-kernel/src/loop_engine/decomposition.rs`
- related tests

## Move
- sequential sub-goal execution
- concurrent sub-goal execution
- bounded sub-goal follow-up execution
- child engine construction path needed for decomposition
- related execution helpers

## Interface constraint
The decomposition module may construct child engines through existing typed builder paths, but it should not create circular module dependencies back into the orchestrator.

## Non-goals
- No new decomposition strategy
- No change to concurrency policy
- No depth-limit behavior change
- No merge with tool execution or compaction work

## Acceptance criteria
- recursive decomposition execution lives in `decomposition.rs`
- sequential and concurrent behavior remains stable
- depth limiting, aggregation, and follow-up behavior remain unchanged
- decomposition tests move with the subsystem or are added alongside it

## Validation
```bash
cargo fmt --all
cargo clippy --workspace --tests -- -D warnings
cargo test --workspace
```

Pay attention to regressions around child-engine follow-up paths and aggregation behavior.

## Reviewer focus
- Did the extraction preserve recursive execution semantics?
- Did the module boundary stay clean, or is it now tightly coupled to unrelated orchestrator state?
- Are sequential/concurrent paths still readable and independently testable?
