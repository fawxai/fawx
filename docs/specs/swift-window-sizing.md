# Spec: macOS Window Sizing on First Launch

## Problem

On first launch (especially clean installs and VMs), the main window opens with
an arbitrary size/position — sometimes tiny, sometimes off-screen. No
`.defaultSize()` or `.windowResizability()` modifiers exist on the `WindowGroup`.

## Fix

In `app/Fawx/FawxApp.swift`, add window modifiers to `mainWindowScene()`:

```swift
private func mainWindowScene(selectedTheme: AppTheme) -> some Scene {
    WindowGroup {
        themedRootView(selectedTheme: selectedTheme)
            // ... existing .task() and .onChange() modifiers
    }
    .defaultSize(width: 1200, height: 800)
    .windowResizability(.contentMinSize)
    #if os(macOS)
    .commands {
        FawxMacCommands(...)
    }
    #endif
}
```

### Details

- `defaultSize(width: 1200, height: 800)` — reasonable default for a chat + sidebar layout
- `.windowResizability(.contentMinSize)` — prevents shrinking below content minimum
- These only affect the initial window frame when no saved state exists (macOS remembers window position after first use)
- iOS is unaffected (these modifiers are macOS-only on WindowGroup)

## Files to Modify

1. `app/Fawx/FawxApp.swift` — add modifiers to `mainWindowScene()`

## Testing

- Build and launch on a clean user account or VM
- Window should open centered at ~1200x800
- Resizing should work normally
- Window position should persist after quit/relaunch
