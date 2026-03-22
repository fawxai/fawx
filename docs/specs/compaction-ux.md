# Spec: Phase 6 — Compaction UX

**Status:** Ready for implementation  
**Author:** Clawdio  
**Date:** 2026-03-21  
**Platforms:** Swift (macOS + iOS), Rust (fx-kernel, fx-api)  
**Parent spec:** `docs/specs/long-session-context-management.md`

---

## Problem

Compaction works silently in the background (Phases 1-5), but the user has no visibility into:
1. **When compaction happened** — context suddenly drops and the user doesn't know why
2. **What the agent remembers** — session memory is invisible to the user
3. **Current context pressure** — the usage bar doesn't update after compaction

---

## Features

### 6A: Compaction Event via SSE

**Goal:** Subtle, non-intrusive notification when compaction occurs.

#### Rust changes (fx-kernel)

Add a new `StreamEvent` variant:

```rust
// engine/crates/fx-kernel/src/streaming.rs
pub enum StreamEvent {
    // ... existing variants ...
    
    /// Compaction completed — context was optimized.
    ContextCompacted {
        /// Which tier fired (prune, slide, summarize, emergency).
        tier: String,
        /// Messages removed.
        messages_removed: usize,
        /// Tokens before compaction.
        tokens_before: usize,
        /// Tokens after compaction.
        tokens_after: usize,
        /// Current usage ratio (0.0-1.0).
        usage_ratio: f64,
    },
}
```

Emit this event from `finish_tier()` via the error_callback (which is the stream callback). This is the same pattern used for `StreamEvent::Error` emissions. The event carries enough data for the client to:
- Show a banner
- Update the context bar
- Log compaction history

#### Swift changes

**SSEStream.swift:** Add `case contextCompacted(tier: String, messagesRemoved: Int, tokensBefore: Int, tokensAfter: Int, usageRatio: Double)` to `SSEEvent` enum. Add decoder in `SSEParser.decode()` for event name `context_compacted`.

**ChatViewModel.swift:** Handle `.contextCompacted` in the SSE event handler:
1. Update `ContextInfo` with new token values and usage ratio
2. Set a transient `compactionBannerMessage` string (auto-dismisses after 4 seconds)

**ChatDetailView.swift:** Show a subtle banner at the top of the chat when `compactionBannerMessage` is non-nil:
- Light background (fawxSurface), small text
- Example: "Context optimized — 12 messages compacted, 68% → 42%"
- Slides in, auto-dismisses after 4 seconds
- No user action required

### 6B: Session Memory Panel

**Goal:** Let the user view and edit what the agent remembers about the session.

#### Rust changes (fx-api)

Add two endpoints:

```
GET  /v1/sessions/:id/memory → SessionMemory JSON
PUT  /v1/sessions/:id/memory → accepts SessionMemory JSON, replaces memory
```

The GET endpoint reads from the session's stored memory. The PUT endpoint validates, saves to redb, and if the engine has this session loaded, also updates the in-memory arc.

#### Swift changes

**SessionMemoryPanel.swift (NEW):** A sheet/panel view showing:
- Project (editable text field)
- Current state (editable text field)  
- Key decisions (list with delete)
- Active files (list with delete)
- Custom context (list with delete)
- Last updated timestamp
- "Save" and "Cancel" buttons
- Token usage indicator (X / 2000 tokens)

**Integration:** Accessible via a button in the status bar or a menu item. Uses `FawxClient.sessionMemory(id:)` and `FawxClient.updateSessionMemory(id:memory:)`.

### 6C: Context Bar Live Update

**Goal:** The context usage bar in the status bar updates immediately after compaction.

#### Current behavior
The context bar calls `GET /v1/sessions/:id/context` on session load and periodically. After compaction, the cached value is stale until the next poll.

#### Fix
When a `contextCompacted` SSE event arrives, immediately update the `ContextInfo` on `AppState` using the `tokens_after` and `usage_ratio` from the event. No additional API call needed.

This is wired in 6A's ChatViewModel handler — the same event that triggers the banner also updates the context bar.

---

## Implementation Plan

### PR 1: Rust-side compaction event + memory API (~150 lines)
- `StreamEvent::ContextCompacted` variant
- Emit from `finish_tier()`
- `GET /v1/sessions/:id/memory` endpoint
- `PUT /v1/sessions/:id/memory` endpoint
- Tests for event emission + endpoints

### PR 2: Swift-side compaction banner + context bar update (~200 lines)
- `SSEEvent.contextCompacted` variant + parser
- Compaction banner in ChatDetailView
- Context bar live update from SSE event
- Tests for SSE parsing + banner display

### PR 3: Swift-side session memory panel (~250 lines)
- SessionMemoryPanel view
- FawxClient methods for memory API
- Integration into status bar / menu
- Tests

---

## Design Notes

- The banner is intentionally subtle — not a modal, not a toast. Just a thin bar that appears and fades. The user should barely notice it unless they're looking.
- Session memory editing is a power-user feature. It should be accessible but not prominent.
- The context bar color already changes at 60% (yellow) and 85% (red). Compaction events will cause visible drops that reinforce the "system is working" feeling.
- Emergency compaction (95% tier) could use a slightly more prominent banner since it's a significant event.

---

## Non-goals

- Compaction history log (future: could store in journal)
- Automatic memory editing suggestions
- Context budget configuration UI (use config.toml)
- Compaction strategy selection UI
