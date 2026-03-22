# Spec: Tool Activity Persistence And Grouping

**Status:** Ready for implementation
**Priority:** High
**Repo:** `abbudjoe/fawx`, branch from `dev`

---

## Problem

When a session is actively streaming and tool-call cards are visible, switching to another session and then returning causes those tool-call output cards to disappear.

This is a real usability break:

1. The user loses visibility into what the agent is doing mid-turn.
2. Returning to the active session feels like state was lost.
3. The current per-tool-card rendering is noisy even when it works; a single collapsible grouped view would be easier to scan.

### Repro

1. Open session A.
2. Send a prompt that triggers one or more tool calls.
3. Wait until one or more tool cards are visible in the transcript.
4. Switch to session B while session A is still streaming.
5. Return to session A.

### Current behavior

- The assistant/user messages are still there.
- Streaming text can still be there.
- Tool-call cards are gone.

### Expected behavior

- Tool activity should remain visible when returning to the streaming session.
- Completed tool activity should still be reconstructible after refresh/reload.
- Tool activity should render as one collapsible grouped section per assistant tool round instead of many independent cards.

---

## Root Cause

There are two separate gaps in the app:

### 1. Live tool UI state is transient and visibility-gated

In [`app/Fawx/ViewModels/ChatViewModel.swift`](app/Fawx/ViewModels/ChatViewModel.swift), tool events only update the visible transcript when `currentSessionID == sessionID`.

That means:

1. Switching away from the active streaming session stops tool events from updating any session-local state.
2. Returning rebuilds the transcript from cached/fetched messages only, which do not include the in-memory tool cards.

### 2. Historical structured tool data is flattened away in the app model

The backend already stores structured session history with:

- assistant `tool_use` blocks
- tool `tool_result` blocks

But [`app/Fawx/Models/Message.swift`](app/Fawx/Models/Message.swift) currently decodes structured content into a flat display string, dropping the tool input/result payload needed to rebuild richer UI later.

So even after the stream finishes, the app cannot synthesize a grouped tool-activity view from history.

---

## Goals

1. Preserve visible tool activity across session switching during an active stream.
2. Reconstruct tool activity from fetched session history after the stream completes or the app reloads.
3. Replace many standalone tool cards with one collapsible grouped tool-activity item per assistant tool round.
4. Keep current assistant/user message rendering intact.
5. Avoid backend changes for the first implementation pass if possible.

## Non-Goals

1. Redesigning the SSE protocol.
2. Persisting partial live tool state to the server before the turn is recorded.
3. Building nested disclosure trees inside the group card.
4. Perfect historical error fidelity in v1 if the backend does not expose structured `is_error`.

---

## Proposed UX

### Transcript shape

Replace independent `.toolCall` transcript items with a grouped item:

```swift
case toolActivityGroup(ToolActivityGroupRecord)
```

One group represents one assistant tool round.

### Group behavior

1. The group appears as soon as the first tool event for the round arrives.
2. While any tool is still running, the group is expanded by default.
3. Once all tools finish, the group collapses by default.
4. Historical groups loaded from session history are collapsed by default.

### Group header

Example header text:

- `Tools`
- `3 tools`
- `2 running`
- `1 failed`

The header should summarize:

1. tool count
2. running/completed/error state
3. whether output is available

### Group body

When expanded, render a flat list of tool rows:

1. Tool name
2. Status badge
3. Arguments block if present
4. Result or error output block if present

The group itself is the only disclosure control in v1. Individual tool rows are always visible inside the expanded group.

---

## Recommended Design

### A. Keep structured content in the app model

Update [`app/Fawx/Models/Message.swift`](app/Fawx/Models/Message.swift) so `SessionMessage` retains structured content blocks instead of flattening them immediately.

Recommended shape:

```swift
struct SessionMessage {
    let id: UUID
    let role: MessageRole
    let contentBlocks: [SessionContentBlock]
    let timestamp: Int

    var content: String { renderDisplayText(from: contentBlocks) }
}
```

And expand `SessionContentBlock` to include payloads:

```swift
case text(String)
case toolUse(id: String, name: String, input: JSONValue)
case toolResult(toolUseId: String, content: JSONValue)
case image(mediaType: String)
```

This preserves compatibility for text bubbles while enabling transcript synthesis from history.

### B. Track live tool activity per session, not just in the visible transcript

Replace the current transient UI-only tool-call state with session-local state in `ChatViewModel`.

Recommended state:

```swift
struct SessionTranscriptState {
    var messages: [SessionMessage]
    var liveToolGroup: ToolActivityGroupRecord?
}
```

And cache:

```swift
@ObservationIgnored private var transcriptCache: [String: SessionTranscriptState]
```

Tool SSE events should always update the state for `sessionID`, even when that session is not currently visible.

### C. Synthesize transcript items from messages + live overlay

Introduce a transcript builder that:

1. converts text blocks into message bubbles
2. converts assistant `tool_use` + following tool `tool_result` messages into one historical `ToolActivityGroupRecord`
3. overlays any active live tool group for the streaming session

This gives one rendering path for:

1. active streaming turns
2. revisiting a streaming session
3. reloading finished history

### D. Grouping rules

For session history:

1. If an assistant message contains text blocks, emit a message bubble for that text.
2. If that same assistant message contains one or more `tool_use` blocks, emit one `toolActivityGroup` item after the text bubble.
3. Attach immediately following `tool` messages with `tool_result` blocks to the most recent open group.
4. If a tool result arrives without a matching tool use, attach it to a synthetic `Unknown tool` row so the transcript still renders.

This matches the way session history is already recorded by the engine.

### E. Error handling

Live streaming already carries `is_error` over SSE, so active groups can show correct error state.

Historical session storage does not currently expose structured `is_error`; tool errors are stored as result text prefixed with `[ERROR] ...`.

For v1:

1. infer historical error state from that prefix when present
2. otherwise treat the tool as completed successfully

Optional follow-up:

- add structured `is_error` to persisted `tool_result` blocks so historical rendering does not depend on string parsing

---

## Scope

### Recommended MVP

App-only change.

Includes:

1. preserve structured tool payloads in app decoding
2. keep live tool activity per session while hidden
3. replace per-tool transcript cards with one grouped collapsible card per round
4. reconstruct grouped tool activity from fetched history

Does not require:

1. SSE changes
2. API route changes
3. session storage migration

### Optional follow-up

Backend schema cleanup for historical error fidelity:

1. persist `is_error` explicitly in `SessionContentBlock::ToolResult`
2. expose it over `/v1/sessions/:id/messages`
3. stop inferring error state from `[ERROR]` prefixes

### Implementation slices

#### Slice 1: Model + historical decode

1. retain structured content in the app model
2. add transcript synthesis from stored `tool_use` / `tool_result` blocks

Estimated effort: small to medium

#### Slice 2: Live session-switch persistence

1. move live tool activity out of `transcriptItems` and into session-local cached state
2. continue updating hidden streaming sessions from SSE events

Estimated effort: medium

#### Slice 3: Grouped card UX

1. replace standalone tool cards with one grouped collapsible card
2. tune default expansion/collapse behavior
3. add tests for grouped rendering behavior

Estimated effort: medium

#### Optional Slice 4: Backend error metadata cleanup

1. persist explicit `is_error`
2. remove string-prefix error inference from the app

Estimated effort: small

---

## Files Likely To Change

### App model + transcript building

- [`app/Fawx/Models/Message.swift`](app/Fawx/Models/Message.swift)
- [`app/Fawx/Models/ChatTranscript.swift`](app/Fawx/Models/ChatTranscript.swift)
- [`app/Fawx/ViewModels/ChatViewModel.swift`](app/Fawx/ViewModels/ChatViewModel.swift)

### App rendering

- [`app/Fawx/Views/Shared/ChatDetailView.swift`](app/Fawx/Views/Shared/ChatDetailView.swift)
- [`app/Fawx/Views/Shared/ToolCallCard.swift`](app/Fawx/Views/Shared/ToolCallCard.swift)

Recommended:

1. replace `ToolCallCard` with a new `ToolActivityGroupCard`, or
2. keep `ToolCallCard` as the per-row renderer inside the grouped card

### Tests

- [`app/FawxTests/Models/SessionMessageTests.swift`](app/FawxTests/Models/SessionMessageTests.swift)
- [`app/FawxTests/ViewModels/ChatViewModelTests.swift`](app/FawxTests/ViewModels/ChatViewModelTests.swift)

---

## Test Plan

### Unit tests

1. Decoding structured assistant `tool_use` blocks preserves `id`, `name`, and `input`.
2. Decoding structured tool `tool_result` blocks preserves `tool_use_id` and `content`.
3. Transcript synthesis converts `assistant(tool_use) + tool(tool_result)` into one grouped transcript item.
4. Switching away from an active streaming session and back preserves the visible live tool group.
5. Hidden-session tool SSE events still update the correct session-local state.
6. Historical tool results prefixed with `[ERROR]` render as error rows.
7. Duplicate or repeated tool ids do not corrupt grouping.

### Manual verification

1. Start a tool-heavy request in session A.
2. Wait for grouped tool activity to appear.
3. Switch to session B and back to session A before completion.
4. Verify the grouped tool activity is still present and still updating.
5. Let the turn complete, then reload session history.
6. Verify the grouped tool activity is still visible as a collapsed historical group.
7. Verify text-only turns render unchanged.

---

## Risks

1. Transcript synthesis is more stateful than the current flat `messages -> items` mapping.
2. Expansion state should remain local UI state; persisting expansion across reloads is unnecessary scope for v1.
3. Historical error inference from `[ERROR]` is workable but brittle; if that prefix changes, badges regress.

---

## Recommendation

Implement the app-only MVP first.

It fixes the actual session-switching bug and delivers the better grouped UX without waiting on backend schema work. If the grouped history view feels good in practice, follow with the small backend cleanup to persist explicit `is_error` on tool results.
