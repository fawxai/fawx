# Loop Refactor Handoff — 2026-03-27

Branch: `codex/provider-owned-loop-refactor`

Base HEAD before current local changes: `d8c5e9cb`

## Current Direction

The loop is being moved toward:

- a Codex-style explicit runner
- typed continuation/finish state instead of narration-driven control
- self-describing component seams instead of kernel name/shape guesses

## Important Recent Fixes

### 1. Decompose gate no longer fabricates invalid tool calls

Problem:

- batch/trivial decomposition gates were synthesizing raw tool calls like:
  - `{"description": "..."}`
- this produced invalid calls such as:
  - `run_command: missing field command`

Fix:

- added `SubGoalToolRoutingRequest` and `ToolExecutor::route_sub_goal_call(...)`
- the kernel now only direct-routes decomposition gates when the executor can safely materialize a valid call
- `FawxToolExecutor` generically direct-routes only zero-required-arg tools
- tools with required arguments, including `run_command`, are no longer guessed by the kernel

Key files:

- `/Users/joseph/fawx/engine/crates/fx-kernel/src/act.rs`
- `/Users/joseph/fawx/engine/crates/fx-kernel/src/loop_engine.rs`
- `/Users/joseph/fawx/engine/crates/fx-tools/src/tools.rs`

### 2. Child subgoals no longer re-advertise `decompose` when they already have required tools

Problem:

- child subgoals were recursively decomposing instead of using their declared required tools
- runs showed repeated `decompose` rows, depth-limit failures, and observation-guard blocking

Fix:

- added `decompose_enabled` to `LoopEngine`
- child engines now call `.allow_decompose(sub_goal.required_tools.is_empty())`
- request builders only inject `decompose` when `decompose_enabled == true`
- `decide()` drops a `decompose` tool call when decomposition is disabled for that runner

Key file:

- `/Users/joseph/fawx/engine/crates/fx-kernel/src/loop_engine.rs`

## Latest Reviewed Recording Before This Handoff

Recording:

- `/Users/joseph/Desktop/Screen Recording 2026-03-27 at 2.23.38 AM.mov`

What it showed:

- the old `run_command` argument-shape bug was gone
- the new failure was recursive child decomposition
- ending summary showed multiple `decompose` rows and depth-limit / required-tool-use failures
- internal trace still logged:
  - `Tool 'decompose' blocked: read-only inspection is disabled after repeated observation-only rounds`

That recording is the reason for the `decompose_enabled` child-runner patch above.

## Current Dirty Files

- `/Users/joseph/fawx/app/Fawx.xcodeproj/xcshareddata/xcschemes/Fawx-iOS.xcscheme`
- `/Users/joseph/fawx/app/Fawx.xcodeproj/xcshareddata/xcschemes/Fawx-macOS.xcscheme`
- `/Users/joseph/fawx/app/Fawx/Models/Message.swift`
- `/Users/joseph/fawx/engine/crates/fx-cli/src/headless.rs`
- `/Users/joseph/fawx/engine/crates/fx-cli/src/helpers.rs`
- `/Users/joseph/fawx/engine/crates/fx-decompose/src/dispatcher.rs`
- `/Users/joseph/fawx/engine/crates/fx-kernel/src/act.rs`
- `/Users/joseph/fawx/engine/crates/fx-kernel/src/budget.rs`
- `/Users/joseph/fawx/engine/crates/fx-kernel/src/loop_engine.rs`
- `/Users/joseph/fawx/engine/crates/fx-tools/src/tools.rs`
- `/Users/joseph/fawx/docs/backlog/claude-thinking-streaming-ui.md`

## Verification Status

Passed after the latest runner changes:

- `cargo test -p fx-kernel decompose_gate_tests -- --nocapture`
- `cargo test -p fx-kernel loop_engine -- --nocapture`
- `cargo test -p fx-tools route_sub_goal_call_ -- --nocapture`
- `cargo test -p fx-kernel child_engine_disables_decompose_when_sub_goal_declares_required_tools -- --nocapture`
- `cargo test -p fx-kernel decide_drops_disallowed_decompose_tool_call_to_text_response -- --nocapture`
- `cargo check -p fx-kernel -p fx-cli -p fx-tools`

## Best Next Step

Stay on the same worktree and branch, but start a fresh conversation.

Primary question for the next live run:

- does the implementation/research child now stay on its required tools instead of recursively decomposing?

If the next run still fails, the next likely seam is:

- subgoals may still have too broad a tool surface beyond `decompose`
- if so, add a scoped child tool executor that exposes only `required_tools` for that subgoal
