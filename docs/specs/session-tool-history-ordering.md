# Spec: Preserve Tool Ordering in Session History

## Problem

Session history can persist or replay tool activity out of order. When a `tool_result`
is stored before its matching assistant `tool_use`, the session becomes poisoned.

Confirmed user-facing effects:

- the transcript renderer synthesizes an `Unknown tool` row
- follow-up provider continuations can fail with `No tool output found for function call fc_...`
- the session can no longer be trusted for grouped tool activity or safe replay

This is a control-plane bug, not a SwiftUI-only rendering bug.

## Confirmed Evidence

Observed on the local `8400` server in session `sess-ad629fdf` on 2026-03-29.

Relevant stored message order:

1. assistant `tool_use` for `commit_transaction` with id `call_dhbhy1WT3LhAwBTMuNTMf10Q`
2. tool `tool_result` for id `call_Zqpgy4Tir8RYLGhFLMpZh2sX`
3. assistant `tool_use` for `run_command` with id `call_Zqpgy4Tir8RYLGhFLMpZh2sX`

That ordering is invalid. The `tool_result` references a tool call that does not yet
exist in the stored history.

The UI fallback in `app/Fawx/ViewModels/ChatViewModel.swift` is behaving as designed:
unmatched results become `Unknown tool`.

The provider contract in `engine/crates/fx-llm/src/validation.rs` is also correct:
a `tool_result` must have a matching earlier `tool_use`.

## Invariant

For every persisted or replayed `ToolResult(tool_use_id = X)`:

1. there must be exactly one matching assistant `ToolUse(id = X)`
2. that `ToolUse` must appear earlier in message order
3. continuation serialization must preserve any provider-owned `fc_*` id separately from the
   agent-visible `call_*` id

If this invariant is violated, the session history is invalid.

## Expected Behavior

- tool activity for a turn is persisted in causal order
- grouped transcript reconstruction never has to invent `Unknown tool` for valid history
- provider replay never emits tool outputs for unresolved or future tool calls
- a poisoned session is detected before it reaches the provider

## Root Cause Definition

The system allows tool history to cross a boundary without enforcing ordering integrity.
Some combination of turn finalization, session persistence, or replay normalization can
append or retain a `tool_result` before its matching assistant `tool_use`.

That breaks two downstream consumers:

- transcript grouping, which can only attach results to prior tool uses
- provider continuation, which requires prior `function_call` records before
  `function_call_output`

## Fix Scope

### 1. Enforce ordering at the session write boundary

Do not persist a `tool_result` message unless the matching assistant `tool_use`
already exists in the persisted sequence for that turn.

Preferred behavior:

- buffer or reorder within the turn so persistence is causal
- if causal ordering cannot be proven, fail loudly and mark the turn invalid

Do not rely on the UI fallback to paper over malformed history.

### 2. Validate ordering before provider replay

Before sending stored history back to a provider, validate the full message sequence.

If a stored session is already poisoned:

- do not send it to the provider
- surface a deterministic session-corruption error
- optionally offer a targeted repair path or require a new session

### 3. Keep provider ids and tool ids distinct

OpenAI Responses requires provider-owned `fc_*` ids for tool-output continuation.
Session history and replay must not collapse provider ids into `call_*` ids or lose the
mapping during follow-up turns.

### 4. Repair policy for already-poisoned sessions

One of these must be chosen explicitly:

- deterministic repair when a later matching `tool_use` can be safely reordered
- hard failure with a clear corrupted-session message

Do not silently continue with malformed history.

## Regression Tests

1. Session persistence test:
   a turn with assistant `tool_use`, tool `tool_result`, and final assistant text is
   stored in the same causal order it was executed.

2. Poisoned-history rejection test:
   a stored sequence containing `tool_result(X)` before `tool_use(X)` is rejected before
   provider replay.

3. Provider replay test:
   a valid stored sequence preserves both the assistant-visible `call_*` id and the
   provider-owned `fc_*` id across continuation.

4. Historical transcript test:
   valid fetched history reconstructs one grouped tool activity record without creating
   synthetic `Unknown tool` rows.

## Likely Files

- `engine/crates/fx-session/src/session.rs`
- `engine/crates/fx-kernel/src/loop_engine.rs`
- `engine/crates/fx-llm/src/validation.rs`
- `engine/crates/fx-llm/src/openai_responses.rs`
- `app/Fawx/ViewModels/ChatViewModel.swift`

## Non-goals

- hiding the symptom in SwiftUI without fixing stored history
- weakening provider validation
- treating this as part of the `#1654` tool-registry refactor
