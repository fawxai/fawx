# Spec: #1641 Phase 1 — Direct Inspection Detection and Typed State

## Status
Planned.

This phase gives deterministic inspection turns a typed identity inside the loop before any tool or continuation behavior is chosen.

## Goal
Introduce a narrow `DirectInspectionProfile` and route matching requests into that profile instead of `Standard`.

## Problem

Today the loop does not know that `read ~/.zshrc and tell me what it says` is a deterministic local inspection request. It enters `Standard`, borrows standard tool-round behavior, and later pays the cost through the wrong continuation contract.

Without typed profile identity:

- tool scope is borrowed from general-purpose logic,
- continuation policy is inherited from research-then-implement turns,
- later code has to infer task shape from prose or side effects.

That is the hidden-contract bug this phase fixes.

## Deliverables

1. Add a new profile type:

```rust
pub(super) enum DirectInspectionProfile {
    ReadLocalPath,
}
```

and a new execution-profile variant:

```rust
TurnExecutionProfile::DirectInspection(DirectInspectionProfile)
```

2. Add narrow detection alongside the existing profile-detection path.

3. Keep detection intentionally strict for the first slice:
   - user requests reading, inspecting, quoting, or summarizing a local file,
   - user supplies an explicit local path token such as `/...` or `~/...`,
   - request is satisfiable from local observation alone,
   - request does not ask to edit, write, run, test, or otherwise mutate.

4. Detection must return typed state, not raw strings or ad hoc booleans.

5. Existing direct utility and bounded local detection must remain unchanged.

## Detection Rules

### Should match

- `read ~/.zshrc and tell me exactly what it says`
- `inspect /Users/joseph/.gitconfig`
- `quote ~/notes/todo.md`
- `summarize /tmp/foo.txt`

### Should not match

- `read this file and then update it`
- `inspect the repo and implement the fix`
- `find auth bugs in this codebase`
- `open the latest logs and diagnose the crash`

## Design Notes

- Do not classify this via prompt text inside a generic system message.
- Do not reuse `DirectUtility` detection; the contract is different.
- The first slice should not try to solve arbitrary repo inspection. Keep it scoped to explicit local-path reads.

## Files expected to change

- `engine/crates/fx-kernel/src/loop_engine.rs`
- a new sibling module if profile detection moves out of the monolith during this phase

## Required Regression Tests

1. Explicit local-path read requests route to `DirectInspection(ReadLocalPath)`.
2. Requests that include mutation verbs do not route to `DirectInspection`.
3. Requests without explicit local paths do not route to `DirectInspection`.
4. Existing `DirectUtility` detection still wins for current direct utility triggers.

## Out of Scope

- Tool surface changes
- Continuation changes
- Terminal completion behavior
