# Step 15.4: Session Export UI and Final Smoke

## Branch
`codex/step15-4-session-export-ui-and-final-smoke`

## Goal
Add per-session export to the macOS and iOS session browsers and complete Step 15 with a final full-feature manual smoke test.

## Why this slice exists
The backend export endpoint, client plumbing, and view-model export method already exist. This slice connects them to the UI and verifies the entire Step 15 feature set end to end.

## Expected targets
- `app/Fawx/Views/macOS/Sidebar.swift`
- `app/Fawx/Views/iOS/SessionListView.swift`
- small export-presentation helpers if needed

## Required UI additions

### Export context action
Add "Export" to the session row context menu on both platforms.

Export should be available for:
- active sessions
- archived sessions

### Export format choice
After the user taps/clicks "Export", present a format choice:
- Text
- JSON

This can be an inline submenu (macOS context menu submenu), an action sheet (iOS), or a simple two-option sheet/popover. Keep it lightweight.

### Export delivery

#### macOS
Use `NSSharingServicePicker` or SwiftUI's `ShareLink` to present the standard macOS share sheet with the exported content. If share integration is complex, a simple save-to-file dialog via `fileExporter` or `NSSavePanel` is acceptable for V1.

#### iOS
Use `ShareLink` or present a `UIActivityViewController` with the exported content. The share sheet should offer standard iOS share targets (copy, save to files, airdrop, etc.).

### Error handling
If the export call fails:
- show a toast or inline error
- do not crash or leave the UI in a broken state

## Rules
- no bulk export in this slice
- no new backend endpoints
- export must use the shipped backend `GET /v1/sessions/{id}/export?format=text|json` contract
- keep existing clear/delete/archive behavior unchanged
- both macOS and iOS must be covered

## Acceptance criteria
- export action is available in context menus on both platforms
- user can choose text or JSON format
- exported content is delivered through the platform share/save mechanism
- export works for active and archived sessions
- error states are handled gracefully

## Final full-feature manual smoke test
This is the exit gate for all of Step 15. On a real app build:

1. launch app, confirm default view is "Active" sessions
2. create or select an active session with some messages
3. archive the session from the context menu
4. confirm it disappears from the active list
5. switch filter to "Archived"
6. confirm the session appears
7. export the archived session as text — verify content
8. export the archived session as JSON — verify content
9. unarchive the session
10. confirm it returns to the active list
11. export the now-active session as text — verify content
12. clear the session history — confirm clear still works and is distinct from archive
13. delete a session — confirm delete still works and is distinct from archive
14. repeat key flows on iOS if built

### Pass criteria
- archive, unarchive, and filter all work
- export produces correct content for both formats
- clear and delete remain distinct from archive
- no regressions in existing session browser behavior

## Validation
Build on the Mac build node:
```bash
xcodebuild -scheme Fawx-macOS -destination 'platform=macOS' build
```

If iOS target is built:
```bash
xcodebuild -scheme Fawx-iOS -destination 'generic/platform=iOS' build
```

## Done means
- Step 15 is complete
- archive, filter, export, and clear/delete distinction all work end to end
- the Swift app consumes the shipped Step 14 backend contract
- Step 16 or follow-up work can build on a stable Swift archive/export surface

## Reviewer focus
- Does export use the backend endpoint rather than reconstructing transcripts client-side?
- Is the format choice simple and not over-engineered?
- Does the final smoke test actually prove the intended user-facing behavior?
- Did the slice stay in Option A scope with no bulk-action creep?
