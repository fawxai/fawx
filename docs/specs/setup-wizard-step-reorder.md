# Spec: Reorder Setup Wizard Steps (Bootstrap Before Provider)

## Problem

The setup wizard step order is: Welcome → Tailscale → Provider → Ready.

Bootstrap (which installs the LaunchAgent and starts the server) happens in
`finishSetup()` on the Ready step. But the Provider step needs the server
already running to save credentials via HTTP API. Result: "Could not connect
to the server" when trying to add an API key during setup.

## Fix

Move bootstrap to happen before the provider step. New step order:

**Welcome → Tailscale → Bootstrap → Provider → Ready**

Or, simpler: run bootstrap automatically when transitioning from Tailscale to
Provider (no dedicated Bootstrap step in the UI).

### Option A: Silent bootstrap on provider entry (recommended)

When `prepareCurrentStep()` is called for `.provider`, check if the server is
already running. If not, run `completeLocalSetup()` first, then proceed to
provider configuration.

In `SetupViewModel.swift`:

```swift
private func refreshProviderState() async {
    // Ensure the server is running before we try to talk to it
    if !appState.isConfigured {
        bootstrapProgress = "Starting Fawx server..."
        do {
            try await appState.completeLocalSetup { [weak self] message in
                self?.bootstrapProgress = message
            }
        } catch {
            providerStatusKind = .failure
            providerStatusMessage = "Could not start the server: \(error.localizedDescription)"
            bootstrapProgress = nil
            return
        }
        bootstrapProgress = nil
    }

    await appState.refreshPhase4State()

    if !configuredProviderIDs.isEmpty {
        providerStatusKind = .success
        providerStatusMessage = "Provider authentication is ready."
    } else if appState.setupStatus == nil {
        providerStatusKind = .warning
        providerStatusMessage = "Provider status is unavailable until the server reconnects."
    }
}
```

This approach:
- No new UI step needed
- Bootstrap happens silently with progress indicator
- If bootstrap fails, user sees error on provider step
- `completeLocalSetup` is already idempotent (returns existing config if already bootstrapped)

### Option B: Explicit Bootstrap step

Add a new `SetupStep.bootstrap` case between `.tailscale` and `.provider`:

```swift
enum SetupStep: Int, CaseIterable, Identifiable, Sendable {
    case welcome
    case tailscale
    case bootstrap  // NEW
    case provider
    case ready
}
```

This requires a new step view but gives users explicit visibility into what's
happening. More work for less benefit since bootstrap takes <2 seconds.

**Recommendation: Option A** — simpler, less UI churn, bootstrap is fast.

## What must NOT change

- `finishSetup()` still runs on the Ready step for final cleanup (auto-start toggle, etc.)
- `completeLocalSetup()` remains idempotent — calling it twice is safe
- The Tailscale step remains skippable
- The Provider step remains skippable

## Files to Modify

1. `app/Fawx/ViewModels/SetupViewModel.swift` — `refreshProviderState()`

## Testing

1. Clean teardown → fresh install → setup wizard
2. Verify server starts silently when entering provider step
3. Verify API key save works immediately on provider step
4. Verify skipping provider still works
5. Verify running setup a second time doesn't re-bootstrap
