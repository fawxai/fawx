# Spec: #1641 Phase 2 — Direct Inspection Tool Surface and Request Contract

## Status
Planned.

This phase makes `DirectInspection` own its request setup instead of borrowing `Standard` defaults.

## Goal
Give `DirectInspection(ReadLocalPath)` a profile-owned directive, tool scope, and reasoning contract tailored for deterministic local inspection.

## Problem

Even with correct profile detection, the profile is not real until it owns its own request contract.

If `DirectInspection` still inherits standard request setup:

- decomposition remains available when it should not be,
- standard tool scope can leak in,
- the model receives mixed signals about whether it should inspect, mutate, or continue planning.

## Deliverables

1. Add a profile-owned directive for direct inspection turns.

2. Define a minimal tool surface for `ReadLocalPath`.
   - The initial slice should allow only the observation tools required for local file inspection.
   - Prefer `read_file` as the primary tool in the first slice.

3. Make decomposition unavailable by default for this profile.

4. Ensure direct inspection turns never opt into mutation-only or mutation-capable tooling.

5. Keep the profile contract narrow. This is not a general local-research mode.

## Request Contract

The direct inspection directive should communicate:

- the task is to inspect the specified local path,
- the assistant should use only the provided observation tools,
- if tool results answer the request, it should answer from that evidence,
- it should not broaden the task into repo research or code modification.

## Design Notes

- Do not implement this as a prompt-only tweak on top of `Standard`.
- Tool scope should be selected by profile-owned code, not by inherited standard defaults plus post hoc stripping.
- If a future phase expands direct inspection beyond single-file reads, it should do so by adding explicit profile variants, not by widening this first slice opportunistically.

## Files expected to change

- `engine/crates/fx-kernel/src/loop_engine.rs`
- `engine/crates/fx-kernel/src/loop_engine/direct_utility.rs` or a new sibling module for direct inspection
- `engine/crates/fx-kernel/src/loop_engine/continuation.rs` if tool-scope ownership is shared there

## Required Regression Tests

1. Direct inspection turns only expose the intended observation tool surface.
2. Direct inspection turns do not enable decomposition by default.
3. Direct inspection turns do not fall back to mutation-capable tool scope.
4. Standard research/edit turns keep their existing tool surface.

## Out of Scope

- Final completion behavior after tool results
- Evidence preservation across continuation
- Standard-profile cleanup beyond the direct-inspection boundary
