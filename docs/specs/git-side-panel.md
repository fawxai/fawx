# Spec: Git Side-by-Side Panel

## Problem
Git view is currently a separate full-screen page accessed via the sidebar. Users want to see git status alongside the chat so they can monitor changes while working with the agent.

## Solution

Add a toggleable right-side panel on macOS that shows a compact git view alongside the chat. The full GitView page remains for detailed work.

### Layout (macOS only)

```
┌──────────┬──────────────────────┬──────────────────┐
│ Sidebar  │      Chat            │   Git Panel      │
│          │                      │   (optional)     │
│ Sessions │  Messages...         │   Branch: dev    │
│ Skills   │                      │   3 changed      │
│ Fleet    │  [Composer]          │   [files list]   │
│ Git ←    │                      │   [quick diff]   │
│ Settings │                      │                  │
└──────────┴──────────────────────┴──────────────────┘
```

### Toggle mechanism

Add a toolbar button or keyboard shortcut (Cmd+G) to toggle the git panel. The panel slides in from the right.

In `ContentView.swift`, when a chat session is selected AND git panel is toggled on:

```swift
// Current:
NavigationSplitView {
    Sidebar(...)
} detail: {
    detailView
}

// New (when git panel is open):
NavigationSplitView {
    Sidebar(...)
} detail: {
    HSplitView {
        detailView
            .frame(minWidth: 400)
        
        if showGitPanel {
            CompactGitPanel(viewModel: gitViewModel)
                .frame(minWidth: 280, idealWidth: 340, maxWidth: 420)
        }
    }
}
```

### CompactGitPanel view

New file: `app/Fawx/Views/macOS/CompactGitPanel.swift`

A slimmed-down version of GitView showing:
- Branch name + clean/dirty badge
- Changed files list (staged/unstaged) with tap-to-stage
- Compact diff preview for selected file (truncated)
- Quick actions: Stage All, Commit (with inline message field), Push
- "Open Full View" button to switch to the full GitView page

No commit history (save space). No fetch/pull buttons (use full view for that).

### State management

Add to `ContentView`:
```swift
@AppStorage("show_git_panel") private var showGitPanel = false
```

Toggle via:
- Toolbar button (source control icon)
- Keyboard shortcut: Cmd+Shift+G
- Sidebar: long-press/right-click Git → "Open as Side Panel"

### iOS behavior

No side panel on iOS — screen too small. Git remains a separate full-screen view accessed from the sidebar. No changes needed.

## Files Changed

| File | Change |
|------|--------|
| `app/Fawx/Views/macOS/CompactGitPanel.swift` | New: compact git panel for side-by-side |
| `app/Fawx/Views/macOS/ContentView.swift` | HSplitView wrapping detail + git panel |
| `app/Fawx/Views/macOS/Sidebar.swift` | Optional "Open as Panel" context menu |

## Testing

### Manual Testing
1. Toggle git panel with Cmd+Shift+G — panel slides in from right
2. Chat remains fully functional with panel open
3. File changes appear in real-time in the panel
4. Tap file to stage/unstage
5. Commit from the panel with inline message
6. "Open Full View" navigates to full GitView
7. Toggle off — panel slides away, chat expands to fill
8. Window resize — chat and git panel resize proportionally
9. Panel state persists across app restarts (AppStorage)
10. iOS — no panel visible, git only via sidebar
