# Step 13.6b — Tool Loop Policy Integration

## Branch
`codex/step13-6b-tool-loop-policy`

## Goal
Move the higher-level tool-loop policy into `loop_engine/tool_execution.rs` after the core execution machinery is already extracted.

## Why this slice exists
This is the highest-risk behavioral slice in Step 13. It contains the logic that ties tool execution to retry policy, continuation policy, nudging, strip-tools thresholds, and budget enforcement.

## Expected targets
- `engine/crates/fx-kernel/src/loop_engine/mod.rs`
- `engine/crates/fx-kernel/src/loop_engine/tool_execution.rs`
- related tests

## Move
- retry integration for tool loops
- nudge / strip-tools integration
- continuation compaction glue
- multi-round tool-loop policy helpers
- other policy code that conceptually belongs to tool execution rather than the top-level orchestrator

## Preconditions
This slice should not start until:
- 13.2 retry policy is merged
- 13.4a / 13.4b compaction extraction is merged
- 13.6a tool execution core is merged

## Non-goals
- No synthesis fallback move yet
- No changes to direct inspection / direct utility routing
- No new policy heuristics
- No hidden behavioral rewrite under the label of extraction

## Acceptance criteria
- tool-loop policy lives with tool execution rather than spread across `mod.rs`
- retry / nudge / continuation semantics remain unchanged
- existing regressions around runaway continuation, tool ordering, and grouped history remain green
- the interface stays explicit and typed

## Validation
```bash
cargo fmt --all
cargo clippy --workspace --tests -- -D warnings
cargo test --workspace
```

This slice should preserve regressions for:
- mixed-text runaway fixes
- tool ordering / replay integrity
- turn-scoped grouped tool history
- multi-round continuation behavior

## Reviewer focus
- Did the PR preserve current loop-policy semantics?
- Is this still a module extraction, not a stealth rewrite?
- Are retry/compaction/continuation boundaries clearer after the move?
