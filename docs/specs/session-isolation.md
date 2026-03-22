# Spec: Session Input Isolation

## Problem
`ChatViewModel` is a single shared instance across all sessions. Two state variables leak across session boundaries:

1. **`draftMessage`** — Text typed in the composer persists when switching sessions. If you start typing in Session A and click to Session B, your draft appears in Session B's composer.
2. **`queuedMessage`** — When a message is sent while a turn is in progress, it queues globally. When the streaming turn finishes, the queued message fires into whichever session is currently visible, not the session it was typed in.

## Solution
Scope both `draftMessage` and `queuedMessage` to their originating session ID.

## Implementation

### File: `app/Fawx/ViewModels/ChatViewModel.swift`

### Change 1: Per-session draft storage

Replace the single `draftMessage` with a dictionary and a computed property:

```swift
// REMOVE:
var draftMessage = ""

// ADD:
private var draftsBySession: [String: String] = [:]

var draftMessage: String {
    get { draftsBySession[currentSessionID ?? ""] ?? "" }
    set { draftsBySession[currentSessionID ?? ""] = newValue }
}
```

This is transparent to all existing callers. The composer binding, `sendDraft()`, and any other code that reads/writes `draftMessage` works without changes.

### Change 2: Save/restore draft on session switch

In `prepareToDisplaySession(_:)`, the draft is already scoped by the computed property above (it reads/writes based on `currentSessionID`). However, `currentSessionID` is set inside this method, so the ordering matters. The current code sets `currentSessionID = sessionID` early in the method, which means the computed property will read the new session's draft automatically after that line. No additional save/restore logic needed as long as the computed property approach is used.

Verify the ordering in `prepareToDisplaySession`:
```swift
func prepareToDisplaySession(_ sessionID: String?) {
    errorMessage = nil
    pendingTranscriptScrollBehavior = .snap
    currentSessionID = sessionID   // After this, draftMessage reads from new session
    // ... rest of method
}
```

### Change 3: Scope queued message to session

Replace the single `queuedMessage` with a tuple that tracks which session it belongs to:

```swift
// REMOVE:
var queuedMessage: String?

// ADD:
private(set) var queuedMessage: String?
private var queuedMessageSessionID: String?
```

In `sendDraft()`, tag the queue with the current session:

```swift
if isStreaming {
    queuedMessage = trimmed
    queuedMessageSessionID = currentSessionID
    return
}
```

In `sendQueuedMessageIfNeeded()`, only send if the queued message belongs to the session that just finished streaming:

```swift
private func sendQueuedMessageIfNeeded() async {
    guard let queued = queuedMessage?.trimmingCharacters(in: .whitespacesAndNewlines),
          !queued.isEmpty else {
        queuedMessage = nil
        queuedMessageSessionID = nil
        return
    }
    guard appState.connectionStatus == .connected else {
        return
    }

    // Only send if the queued message was for the session that just finished
    let targetSession = queuedMessageSessionID
    queuedMessage = nil
    queuedMessageSessionID = nil

    guard targetSession == streamingSessionID || targetSession == currentSessionID else {
        // Queued message was for a different session; discard it
        return
    }

    await send(queued, forceSessionID: targetSession)
}
```

Note: Check if `send(_:forceSessionID:)` already accepts a `forceSessionID` parameter. Looking at line ~580:

```swift
var targetSessionID = forceSessionID ?? currentSessionID
```

Yes, it does. So passing the original session ID ensures the message goes to the right session even if the user switched away.

### Change 4: Clear queued message on session switch (optional safety)

In `prepareToDisplaySession`, do NOT clear `queuedMessage`. The queued message should survive session switches because it's tagged to a specific session. It will be sent (or discarded) when streaming finishes, regardless of which session is visible.

### Change 5: Clean up stale drafts (optional, nice-to-have)

When a session is deleted or cleared, remove its draft:

In `invalidateSession(_:)`:
```swift
func invalidateSession(_ sessionID: String) {
    removeCachedMessages(for: sessionID)
    draftsBySession.removeValue(forKey: sessionID)
    // ... existing code
}
```

## Files Changed

| File | Change |
|------|--------|
| `app/Fawx/ViewModels/ChatViewModel.swift` | Per-session drafts, scoped queued message |

## Testing

### Unit Tests (add to `ChatViewModelTests.swift` or create if needed)

1. **Draft isolation:** Set draft in session A, switch to session B, verify draft is empty. Switch back to A, verify draft is restored.
2. **Draft independence:** Set different drafts in session A and B, verify each session shows its own draft.
3. **Queued message targets correct session:** Start streaming in session A, queue a message, switch to session B, streaming finishes. Verify the queued message is sent to session A (not B).
4. **Queued message discarded on mismatch:** Queue a message for session A, but streaming finishes for session C. Verify the message is discarded.
5. **Session deletion clears draft:** Set draft in session A, delete session A, verify draft is gone.
6. **Empty session ID:** Verify draft works for the "no session selected" state (nil/empty session ID).

### Manual Testing
1. Type text in session A, click session B. Session B composer should be empty. Click back to A, text should be there.
2. While a response is streaming in session A, type and send a message. Switch to session B. When session A's stream finishes, the queued message should appear in session A's history (not session B's).
3. Create a new session (no ID yet), type a draft, switch away and back. Draft should persist.
