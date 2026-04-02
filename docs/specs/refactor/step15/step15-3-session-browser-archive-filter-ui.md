# Step 15.3: Session Browser Archive Filter UI

## Branch
`codex/step15-3-session-browser-archive-filter-ui`

## Goal
Add archive filter controls and per-session archive/unarchive context actions to the macOS sidebar and iOS session list.

## Why this slice exists
The client, model, and view-model plumbing exist from 15.1 and 15.2. This slice connects them to the actual session browser views on both platforms.

## Expected targets
- `app/Fawx/Views/macOS/Sidebar.swift`
- `app/Fawx/Views/iOS/SessionListView.swift`
- `app/Fawx/Views/Shared/SessionRowView.swift` (if archive state affects row presentation)

## Required UI additions

### Archive filter control
Add a control (segmented picker, menu, or equivalent) near the session list header on both platforms.

The control should offer the filter options that map to the backend contract:
- Active (default)
- All
- Archived

Label the options in a user-friendly way. The backend parameter values (`active`, `all`, `only`) should not surface directly in the UI.

When the filter changes, `SessionViewModel.refresh()` should be called with the new filter. Avoid a full-page loading state for filter switches if a simple list transition is possible.

### Context menu additions

#### macOS sidebar
Add to the existing session row context menu:
- "Archive" when viewing an active session
- "Unarchive" when viewing an archived session

Position archive below the existing "Clear History" item and above "Delete Session" so the destructive action stays at the bottom.

#### iOS session list
Add archive/unarchive as a swipe action or context action alongside existing clear/delete affordances.

Prefer a non-destructive visual treatment for archive (no red/destructive styling) so it feels reversible compared to delete.

### Archive state in session rows
If the archive filter is set to "All", the list will contain both active and archived sessions. Consider a subtle visual cue on archived rows so the user can distinguish them (for example, a small label, badge, or dimmed style). Keep it lightweight.

If the filter is "Active" or "Archived", all rows are the same type and no extra indicator is needed.

## Rules
- no export UI in this slice
- no bulk actions in this slice
- keep existing clear/delete/multi-select-delete behavior unchanged
- default filter is `active` so no visible change until the user switches filters
- both macOS and iOS must be covered

## Acceptance criteria
- archive filter control is visible on both platforms
- switching filters updates the session list from the backend
- archive action appears in context menu for active sessions
- unarchive action appears in context menu for archived sessions
- archiving a session removes it from the active view
- unarchiving a session removes it from the archived view
- existing clear/delete flows are unaffected

## Manual smoke test
On a real build:
1. launch app, confirm default view is "Active" and session list looks unchanged
2. archive a session from the context menu
3. switch filter to "Archived" and confirm the session appears
4. unarchive it and confirm it returns to the active list
5. confirm clear and delete still work as before
6. repeat on iOS if both targets are built

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
- archive/unarchive and filtered browsing work on macOS and iOS
- existing browser behavior is unchanged when filter is at the default
- Step 15.4 can add export UI without touching archive plumbing

## Reviewer focus
- Are context menu actions placed to preserve the destructive-action-last pattern?
- Does the filter control feel native on both macOS and iOS?
- Is the default still "Active" so nothing changes until the user acts?
