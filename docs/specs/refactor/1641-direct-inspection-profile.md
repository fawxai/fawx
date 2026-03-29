# Spec: #1641 — Direct Inspection Profile and Execution Contract

## Status
Planned.

This document is the umbrella spec for an early `loop_engine.rs` decomposition slice under #1641. It defines the execution contract for deterministic read-only inspection turns and maps that work into phased sub-specs.

## Goal
Add a first-class execution profile for deterministic read-only inspection tasks so they do not route through the standard research-then-mutate continuation contract.

For requests like:

- `Please read ~/.zshrc and tell me exactly what it says.`
- `Read /tmp/foo.txt and quote it exactly.`
- `Inspect this local file and summarize it.`

the engine should:

1. classify the request into a typed execution profile,
2. own an observation-only tool surface for that profile,
3. preserve actual tool evidence until the turn terminates,
4. finish on the first usable post-tool answer instead of re-entering the standard outer loop.

## Current State (dev HEAD, 2026-03-29)

File: `engine/crates/fx-kernel/src/loop_engine.rs` (24k+ lines)

### The current contradiction

The kernel already tells the model to answer from tool evidence:

- `REASONING_SYSTEM_PROMPT` says that after using tools, the assistant should respond with the answer.
- `TOOL_CONTINUATION_DIRECTIVE` says that if the current tool results already answer the request, the assistant should answer immediately instead of calling more tools.

But the standard-turn control plane does not terminate there.

### Current failing shape

For a simple read-only local inspection request:

1. `reason()` runs the standard profile.
2. The model often chooses `kernel_manifest`, `config_get`, then `read_file`.
3. `read_file` succeeds.
4. `act_with_tools()` obtains a usable post-tool answer from the continuation pass.
5. Instead of finishing, the loop stores that answer as assistant context and starts another root reasoning pass.
6. The next pass no longer sees the real tool evidence, only the assistant summary plus a turn commitment.
7. Because the round used only observation tools, `continuation_tool_scope_for_round()` forces the next pass to `MutationOnly`.
8. The model can then fabricate a blocker such as "`~/.zshrc` is outside my working directory" even though the file was already read successfully.

### Why this is architectural

The bug is not a `read_file` permission failure.

- `read_file` already expands `~` and enforces jail on the resolved path.
- The failure is that `Standard` mixes incompatible task contracts:
  - research-then-implement,
  - read-only inspection,
  - direct utility calls,
  - bounded local edit flows.

The direct cause is that the standard profile uses one continuation policy for all of them:

- successful observation-only rounds are pushed into `MutationOnly`,
- tool evidence is not preserved as first-class context across the next root pass,
- the first usable post-tool answer is not terminal.

## Design Principles

1. **Typed execution profile, not prompt heuristics**
   - The kernel should know when a turn is deterministic local inspection. This should be represented in typed state, not inferred later from assistant prose.

2. **Profile-owned contracts**
   - Profile selects the tool surface, decomposition policy, continuation policy, and terminal behavior.
   - The loop orchestrator composes those contracts. It should not hard-code one continuation policy for every standard turn.

3. **Deterministic requests use deterministic paths**
   - If a request is satisfiable from local observation alone, the engine should not force it through research-then-mutate continuation.

4. **Evidence remains authoritative**
   - Tool results must remain available until the turn terminates. A summary-only re-entry path is not a safe substitute for actual evidence.

## Proposed Design

Add a new execution profile:

```rust
TurnExecutionProfile::DirectInspection(DirectInspectionProfile)
```

with an initial sub-profile:

```rust
DirectInspectionProfile::ReadLocalPath
```

This profile is for deterministic local inspection requests where the task is to inspect an explicit local path and answer from that evidence.

### Profile contract

`DirectInspection` owns:

1. **Detection**
   - Narrow typed routing for local read/inspect requests with explicit paths.

2. **Tool surface**
   - Observation-only tools needed for inspection.
   - Initial slice should prefer the minimal surface required for local file inspection.

3. **Reasoning contract**
   - No decompose by default.
   - No mutation escalation.
   - No observation-only wrap-up policy borrowed from `Standard`.

4. **Termination contract**
   - If the first post-tool answer is usable, the turn completes immediately.
   - Do not re-enter a second root `reason()` pass for the same request.

5. **Evidence ownership**
   - Tool results remain authoritative until termination.
   - Do not replace tool evidence with assistant-summary-only context and then ask the model to answer again.

## Phase Breakdown

This slice should land as phased work under #1641. Each phase has its own sub-spec.

| Phase | Outcome | Sub-spec |
|---|---|---|
| 1 | Typed direct-inspection profile detection and state ownership | [`1641-phase-1-direct-inspection-detection.md`](1641-phase-1-direct-inspection-detection.md) |
| 2 | Profile-owned request contract and observation-only tool surface | [`1641-phase-2-direct-inspection-tool-surface.md`](1641-phase-2-direct-inspection-tool-surface.md) |
| 3 | Terminal completion from tool evidence, without re-entering standard continuation | [`1641-phase-3-terminal-inspection-completion.md`](1641-phase-3-terminal-inspection-completion.md) |
| 4 | Standard/profile boundary cleanup and regression lock-in for future phase-trait work | [`1641-phase-4-standard-profile-boundaries.md`](1641-phase-4-standard-profile-boundaries.md) |

### Phase intent

Phase 1 gives the loop a typed identity for deterministic inspection turns.

Phase 2 makes that profile own its request directive and minimal tool surface instead of borrowing `Standard` defaults.

Phase 3 fixes the actual completion bug by making usable post-tool answers terminal and by keeping tool evidence authoritative through completion.

Phase 4 narrows `Standard` back to the tasks that actually need iterative continuation, while codifying the regression boundary so later #1641 phase-trait extraction has a stable seam.

## Cross-Phase Acceptance Criteria

1. A request like `read ~/.zshrc and tell me exactly what it says` routes into `DirectInspection(ReadLocalPath)`.
2. A successful direct inspection read cannot end with an outside-working-directory refusal.
3. Direct inspection turns never receive `MutationOnly` continuation scope.
4. Direct inspection turns do not re-enter a second root reasoning pass after a usable post-tool answer.
5. `DirectUtility`, `BoundedLocal`, and standard research-then-implement turns keep their intended behavior.
6. The implementation strengthens #1641 decomposition seams instead of adding special-case branches inside `loop_engine.rs`.

## Why this belongs under #1641

This fix should not be implemented as:

- a special-case `if user_message.contains("read")` branch in `act()`,
- a `skip MutationOnly for read_file` exception,
- a prompt tweak that asks the model more nicely not to hallucinate permissions.

The correct boundary is execution-profile ownership:

- profile selects the tool surface,
- profile selects whether decomposition is available,
- profile selects whether a usable post-tool answer is terminal,
- loop orchestration composes those contracts rather than hard-coding one continuation policy for every standard turn.

That is exactly the decomposition direction of #1641.

## Files expected to change across the slice

- `engine/crates/fx-kernel/src/loop_engine.rs`
- `engine/crates/fx-kernel/src/loop_engine/continuation.rs`
- `engine/crates/fx-kernel/src/loop_engine/direct_utility.rs` or a new sibling module for direct inspection profiles
- `engine/crates/fx-kernel/src/loop_engine/bounded_local.rs`
- related tests in `loop_engine.rs` and any new sibling modules created during extraction

## Not in scope

- General multi-file inspection planning
- Provider/router metadata changes
- Streaming changes
- A broad prompt rewrite
- Rewriting all standard-turn continuation behavior in one PR
- Full phase-trait extraction of `loop_engine.rs` in the same PR

## Follow-on Direction

If this slice lands cleanly, it becomes one of the first concrete extraction seams for #1641:

1. execution-profile detection becomes explicit,
2. profile-owned request/act/observe contracts become movable,
3. standard profile loses responsibilities it should never have absorbed,
4. later phase-trait work can extract around stable boundaries instead of special-case conditionals.
