# Spec: Cross-Session Send Fix

## Problem

When session A is streaming a response, sending a message in session B incorrectly queues it instead of sending immediately. The user sees "Message queued" even though session B has no active turn.

## Root Cause

In `ChatViewModel.swift`, `sendDraft()` checks the global `isStreaming` flag:

```swift
if isStreaming {
    queuedMessage = trimmed
    queuedMessageSessionID = currentSessionID
    return
}
```

`isStreaming` is `true` whenever ANY session is streaming. It should only queue if the CURRENT session is streaming.

## Fix

One-line change in `sendDraft()`:

```swift
// BEFORE:
if isStreaming {

// AFTER:
if isCurrentSessionStreaming {
```

`isCurrentSessionStreaming` already exists and is defined as:
```swift
var isCurrentSessionStreaming: Bool {
    isStreaming && currentSessionID == streamingSessionID
}
```

This lets messages in non-streaming sessions send immediately while still queuing in the active streaming session.

## Files Changed

| File | Change |
|------|--------|
| `app/Fawx/ViewModels/ChatViewModel.swift` | `sendDraft()`: `isStreaming` → `isCurrentSessionStreaming` |

## Testing

### Unit Tests (add to `ChatViewModelTests.swift`)

1. **Send while other session streams:** Start streaming in session A, switch to session B, call `sendDraft()`. Verify: message sends immediately (not queued).
2. **Queue in streaming session:** Start streaming in session A, stay in session A, call `sendDraft()`. Verify: message is queued.
3. **Send in new session while streaming:** Start streaming in session A, create session B (new), send a message. Verify: sends immediately.

### Manual Testing
1. Start a long tool-use chain in session A.
2. Switch to session B.
3. Type and send a message.
4. Message should appear in session B immediately, not show "queued."
