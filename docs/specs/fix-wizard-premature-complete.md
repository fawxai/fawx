# Spec: Fix Wizard Prematurely Completing on Provider Step

## Problem

PR #1526 added `ensureProviderServerIsRunning()` to start the server before
the provider step. It calls `completeLocalSetup()` which calls
`adoptAndConnect()`, which sets `isSetupComplete = true` and
`persistence.setSetupComplete(true)`. This causes the app to immediately
transition to the main view, skipping the provider step entirely.

## Root Cause

`adoptAndConnect()` (AppState.swift ~line 1034) unconditionally sets
`isSetupComplete = true`. This is correct for the Ready step's
`finishSetup()`, but wrong when called from the provider step's
`ensureProviderServerIsRunning()`.

## Fix

Split "start the server and connect" from "mark setup complete."

### Option A: Add a parameter to control setup completion (recommended)

Add a `markSetupComplete: Bool = true` parameter to `completeLocalSetup()` and
pass it through to `adoptAndConnect()`:

In `AppState.swift`:

```swift
func completeLocalSetup(
    markSetupComplete: Bool = true,
    progress: @escaping @MainActor @Sendable (String) -> Void = { _ in }
) async throws {
    // ... existing logic ...
    try await adoptAndConnect(
        serverURL: serverURL,
        bearerToken: bearerToken,
        markSetupComplete: markSetupComplete,
        progress: progress
    )
}

private func adoptAndConnect(
    serverURL: String,
    bearerToken: String? = nil,
    markSetupComplete: Bool = true,
    progress: @escaping @MainActor @Sendable (String) -> Void = { _ in }
) async throws {
    // ... existing connect logic ...
    
    if markSetupComplete {
        isSetupComplete = true
        await persistence.setSetupComplete(true)
    }
    progress("Opening Fawx...")
    await bootstrap()
}
```

In `SetupViewModel.swift`, update `ensureProviderServerIsRunning()`:

```swift
private func ensureProviderServerIsRunning() async -> Bool {
    guard !appState.isConfigured else {
        return true
    }

    providerStatusKind = .idle
    providerStatusMessage = nil
    bootstrapProgress = "Starting Fawx server..."

    do {
        try await appState.completeLocalSetup(
            markSetupComplete: false  // <-- Don't mark complete yet
        ) { [weak self] message in
            self?.bootstrapProgress = message
        }
        bootstrapProgress = nil
        return true
    } catch {
        providerStatusKind = .failure
        providerStatusMessage = "Could not start the server: \(error.localizedDescription)"
        bootstrapProgress = nil
        return false
    }
}
```

`finishSetup()` continues to call `completeLocalSetup()` with the default
`markSetupComplete: true`, so the Ready step still marks setup complete.

## What must NOT change

- `finishSetup()` on the Ready step still marks setup complete
- The server still starts on the provider step
- `completeLocalSetup()` remains idempotent
- Existing callers outside the wizard (Settings, etc.) still mark complete by default

## Files to Modify

1. `app/Fawx/ViewModels/AppState.swift` — add `markSetupComplete` param to
   `completeLocalSetup()` and `adoptAndConnect()`
2. `app/Fawx/ViewModels/SetupViewModel.swift` — pass `markSetupComplete: false`
   in `ensureProviderServerIsRunning()`

## Testing

1. Clean teardown → fresh install → setup wizard
2. Skip Tailscale → should land on provider step and STAY there
3. Server should be running (check `ps aux | grep fawx`)
4. Adding API key should work on provider step
5. Clicking Continue → Ready step → Finish → marks setup complete → main view
6. Skipping provider → Ready → Finish → main view (no provider configured)
7. Re-launching app after full setup → goes straight to main view
