# Spec: #1641 Phase 4 — Standard Profile Boundaries and Regression Lock-In

## Status
Planned.

This phase hardens the boundary between `Standard` and `DirectInspection` so the fix lands as a clean architectural seam for later #1641 phase-trait extraction.

## Goal
Reduce `Standard` back to the tasks that genuinely need iterative continuation, and lock the new profile boundary in place with regression coverage.

## Problem

If Phases 1-3 land without boundary cleanup, the direct inspection fix can still decay into a one-off branch inside the monolith:

- `Standard` keeps hidden responsibilities,
- profile logic remains scattered,
- future loop-engine decomposition has to reverse-engineer the new contract from exceptions.

## Deliverables

1. Narrow standard-only continuation behavior so observation-only wrap-up and `MutationOnly` escalation remain explicitly owned by `Standard`, not globally applied.

2. Extract or centralize the profile-owned continuation/termination decisions introduced in earlier phases so they form a stable seam for later trait-based decomposition.

3. Remove any temporary compatibility branches introduced during the earlier phases.

4. Add regression coverage that protects the boundary across profiles:
   - `DirectInspection`
   - `DirectUtility`
   - `BoundedLocal`
   - `Standard`

5. Document the invariant in code comments only where a type or function boundary is not yet enough to make the contract obvious.

## Design Notes

- This phase should make the architecture cleaner than it started.
- Avoid "except for direct inspection" conditionals spread through unrelated standard-path code.
- If a helper is shared across profiles, make the ownership explicit in types or module boundaries.

## Files expected to change

- `engine/crates/fx-kernel/src/loop_engine.rs`
- `engine/crates/fx-kernel/src/loop_engine/continuation.rs`
- any new profile modules introduced during earlier phases

## Required Regression Tests

1. Standard research-then-implement turns still use their existing continuation contract.
2. Existing `DirectUtility` behavior remains unchanged.
3. Existing `BoundedLocal` behavior remains unchanged.
4. Direct inspection behavior remains terminal and evidence-backed.
5. No profile other than `Standard` inherits observation-only `MutationOnly` escalation unless explicitly designed to do so.

## Out of Scope

- Full phase-trait extraction of all loop-engine concerns
- Broad behavior changes to unrelated profiles
