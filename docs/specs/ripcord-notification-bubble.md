# Spec: Ripcord Notification Bubble

## Problems

1. **Modal is wrong UX:** The ripcord banner is inserted as a `safeAreaInset(edge: .top)` that pushes chat content down. It should be a non-blocking notification bubble in the top-right corner that can be dismissed.
2. **Approve action broken:** Clicking "Review" → "Approve" in the confirmation dialog doesn't do anything. The `performRipcordAction(.approve)` is called but the result isn't reflected in UI.

## Solution

### 1. Replace banner with floating notification bubble

Remove the `safeAreaInset(edge: .top)` ripcord banner. Replace with an overlay bubble anchored to top-trailing.

In `ChatDetailView.swift`, replace:

```swift
.safeAreaInset(edge: .top, spacing: 0) {
    if let ripcordStatus = appState.activeRipcordStatus {
        RipcordBanner(...)
    }
}
```

With:

```swift
.overlay(alignment: .topTrailing) {
    if let ripcordStatus = appState.activeRipcordStatus {
        RipcordNotification(
            status: ripcordStatus,
            isPerformingAction: ripcordActionInFlight != nil,
            reviewAction: presentRipcordJournal,
            pullAction: { pendingRipcordConfirmation = .pull },
            approveAction: { pendingRipcordConfirmation = .approve },
            dismissAction: { appState.dismissRipcordNotification() }
        )
        .padding(.top, FawxSpacing.paddingLG)
        .padding(.trailing, FawxSpacing.paddingLG)
        .transition(.move(edge: .trailing).combined(with: .opacity))
        .animation(.easeInOut(duration: 0.25), value: ripcordStatus.id)
    }
}
```

### 2. New `RipcordNotification` view

Create `app/Fawx/Views/Ripcord/RipcordNotification.swift`:

Compact notification card (~300px wide):
- Shield icon + "Ripcord Active" title
- One-line summary (e.g., "3 operations tracked")
- Three buttons: Review, Pull Ripcord, Approve
- Dismiss X button in top-right corner
- Drop shadow for floating appearance
- Max width: 320px

### 3. Dismiss behavior

Add `dismissRipcordNotification()` to `AppState`:
- Sets `ripcordNotificationDismissed = true`
- Notification stays dismissed until a NEW ripcord event fires (different `id`)
- `activeRipcordStatus` should check `!ripcordNotificationDismissed`

### 4. Debug the approve action

In `performRipcordAction(.approve)`:
```swift
case .approve:
    try await appState.approveRipcord()
    ripcordReport = nil
    ripcordJournalEntries = []
    isShowingRipcordSheet = false
```

The issue is likely that `approveRipcord()` succeeds but `activeRipcordStatus` isn't cleared. Check that `appState.activeRipcordStatus` is set to `nil` after approval. The `approveRipcord()` call on the server should return success AND the polling/SSE should update the status to reflect no active ripcord.

Debug steps:
1. Add logging to `performRipcordAction(.approve)` — does the try/catch succeed?
2. Check `appState.approveRipcord()` — does it call the right endpoint?
3. After approval, is `activeRipcordStatus` polled/refreshed?

If the server endpoint works but the client doesn't refresh: after `approveRipcord()`, explicitly set `appState.activeRipcordStatus = nil` and trigger a status refresh.

## Files Changed

| File | Change |
|------|--------|
| `app/Fawx/Views/Ripcord/RipcordNotification.swift` | New file: compact floating notification |
| `app/Fawx/Views/Ripcord/RipcordBanner.swift` | Keep for backward compat or delete |
| `app/Fawx/Views/Shared/ChatDetailView.swift` | Replace `safeAreaInset` with `overlay`, both macOS and iOS |
| `app/Fawx/ViewModels/AppState.swift` | `dismissRipcordNotification()`, clear status after approve |

## Testing

### Unit Tests
1. Dismiss sets `ripcordNotificationDismissed = true`
2. New ripcord event resets `ripcordNotificationDismissed`
3. Approve clears `activeRipcordStatus`

### Manual Testing
1. Trigger a ripcord event — notification appears top-right, doesn't push chat down
2. Dismiss — notification slides away, doesn't reappear for same event
3. New ripcord event — notification reappears
4. Click Approve — notification disappears, ripcord status clears
5. Click Pull Ripcord — confirmation dialog, then report sheet
6. Click Review — journal sheet opens
