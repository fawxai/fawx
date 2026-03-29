# Spec: #1641 Phase 3 — Terminal Completion from Inspection Evidence

## Status
Planned.

This phase fixes the user-visible bug by making successful direct inspection completion terminal and evidence-backed.

## Goal
When a `DirectInspection` turn has enough tool evidence to answer the request, complete the turn immediately from that evidence instead of re-entering the standard outer loop.

## Problem

The current loop obtains a usable post-tool answer and then throws it back into a second root reasoning pass after stripping the real tool evidence down to assistant context.

That is how a successful `read_file` can later become a false refusal.

## Deliverables

1. For `DirectInspection`, treat the first usable post-tool answer as terminal.
   - Return `ActionTerminal::Complete`.
   - Do not convert it into `ActionContinuation`.

2. Keep tool evidence authoritative until completion.
   - Do not replace evidence with assistant-summary-only context and ask again.

3. If the model produces no usable answer after successful inspection, allow one profile-owned synthesis pass from the current tool evidence.
   - Then terminate.
   - Do not fall back into the standard outer-loop continuation path.

4. Ensure `MutationOnly` continuation scope is never applied to direct inspection turns.

## Design Notes

- This is not a special-case exemption for `read_file`.
- The terminal behavior belongs to the profile contract, not to individual tool names.
- Preserve the clean boundary between `DirectInspection` completion and `Standard` continuation.

## Files expected to change

- `engine/crates/fx-kernel/src/loop_engine.rs`
- `engine/crates/fx-kernel/src/loop_engine/continuation.rs`
- any new direct-inspection sibling module introduced in Phase 2

## Required Regression Tests

1. `read ~/.zshrc and tell me exactly what it says` completes after successful inspection without a second root reasoning pass.
2. A successful direct inspection read cannot end with an outside-working-directory refusal.
3. Direct inspection turns do not receive `MutationOnly` after observation-only tools.
4. If the first post-tool answer is empty, one synthesis pass from current evidence is allowed and then the turn terminates.

## Out of Scope

- Rewriting standard continuation behavior for every profile
- General multi-step observation workflows
