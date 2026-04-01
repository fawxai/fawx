# Step 13.6c — Tool Synthesis Fallback and Terminal Wrap-Up

## Branch
`codex/step13-6c-tool-synthesis-fallback`

## Goal
Complete the tool-execution extraction by moving synthesis fallback and terminal tool wrap-up logic into `loop_engine/tool_execution.rs`.

## Why this slice exists
This is the final tool-execution sub-slice. Keeping synthesis fallback separate prevents 13.6a and 13.6b from ballooning into an unreviewable diff.

## Expected targets
- `engine/crates/fx-kernel/src/loop_engine/mod.rs`
- `engine/crates/fx-kernel/src/loop_engine/tool_execution.rs`
- related tests

## Move
- `synthesize_tool_fallback`
- terminal blocked-tool helpers
- final tool-result synthesis glue
- related helpers used only by the fallback path

## Non-goals
- No policy redesign for budget exhaustion or max-iteration fallbacks
- No changes to request-builder behavior outside import rewiring
- No compaction redesign

## Acceptance criteria
- synthesis fallback is owned by `tool_execution.rs`
- fallback behavior remains identical
- budget exhaustion and terminal wrap-up regressions remain green
- `mod.rs` no longer owns tool fallback details

## Validation
```bash
cargo fmt --all
cargo clippy --workspace --tests -- -D warnings
cargo test --workspace
```

Pay attention to fallback/synthesis regressions and any tests around terminal response behavior after tool rounds.

## Reviewer focus
- Did the fallback path move cleanly without policy drift?
- Are terminal tool-wrap-up semantics unchanged?
- Does this complete the tool-execution extraction coherently?
