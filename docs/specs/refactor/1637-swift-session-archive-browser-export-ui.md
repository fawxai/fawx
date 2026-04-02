# Spec: #1637 Swift Session Archive / Browser / Export UI

## Status
Ready to implement.

## Goal
Add archive, unarchive, archived-session browsing, and per-session export to the Swift app using the Step 14 backend contract that now exists on `dev`.

This step covers the client and UI surfaces for macOS and iOS session browsing. It should consume the backend contract that already shipped in Step 14. It should not invent a second API shape and it should not assume older issue text is still authoritative.

## Why this spec exists
Issue `#1637` was written before Step 14 landed. The issue still references:
- `include_archived=true`
- `POST /sessions/bulk/archive`
- bulk export and bulk archive UI

That is now stale relative to the backend that actually shipped.

Step 14 landed this backend contract instead:
- `GET /v1/sessions?archived=active|all|only`
- `POST /v1/sessions/{id}/archive`
- `DELETE /v1/sessions/{id}/archive`
- `GET /v1/sessions/{id}/export?format=text|json`

Step 15 must follow the shipped backend contract, not the earlier draft issue language.

## Option A decision
This spec follows **Option A**:
- single-session archive / unarchive
- archived-session filter in the browser
- single-session export
- no bulk archive UI
- no bulk export UI
- no multi-select expansion beyond what already exists for delete on macOS

Bulk session hygiene can be revisited later as a separate follow-up if still desired.

## Problems to solve

### P1. Swift client does not expose the new Step 14 endpoints
`FawxClient` currently supports list, create, clear, delete, and message APIs, but not archive, unarchive, archived filtering, or session export.

### P2. Swift session models do not surface archive state
The app's `Session` model does not currently decode archive metadata, so the UI cannot tell whether a session is archived.

### P3. Session browser state assumes one default list only
`SessionViewModel` refreshes only the default list and has no archive filter state or archive/unarchive mutation helpers.

### P4. Session browser UI has no archive affordances
The current macOS sidebar and iOS session list expose clear/delete only. There is no archive or unarchive action and no way to browse archived sessions.

### P5. Export is not yet available in the Swift app
The backend can now export a session as text or JSON, but the client and UI do not expose that capability.

## Scope
### In scope
- Swift client support for archive, unarchive, archived filtering, and export
- session model updates for archive metadata
- session browser filter UI on macOS and iOS
- single-session archive / unarchive actions
- single-session export actions
- Swift manual smoke coverage for the new flows

### Out of scope
- bulk archive
- bulk export
- new backend endpoints
- server contract redesign
- session-browser pagination redesign
- changing existing delete and clear semantics

## Grounded app touchpoints
The current app structure suggests these concrete targets:
- `app/Fawx/Networking/FawxClient.swift`
- `app/Fawx/Models/Session.swift`
- `app/Fawx/ViewModels/SessionViewModel.swift`
- `app/Fawx/Views/Shared/SessionRowView.swift`
- `app/Fawx/Views/macOS/Sidebar.swift`
- `app/Fawx/Views/iOS/SessionListView.swift`

## Product contract for Step 15

### Archive / unarchive
- active sessions are the default browser view
- archived sessions are hidden by default
- when viewing active sessions, each row can archive the session
- when viewing archived sessions, each row can unarchive the session
- archive does not delete or clear history
- archive should feel reversible and low-drama compared to delete

### Archive filter
Use the shipped backend contract directly.

The browser filter should map onto:
- `active`
- `all`
- `only`

Do not retrofit `include_archived=true`. That is stale issue text, not the current contract.

### Export
- export is per-session only in this step
- export should offer text and JSON
- export must work for active and archived sessions
- export should use the backend endpoint as the source of truth, not client-side transcript reconstruction

### Existing destructive actions
- keep clear and delete behavior as-is
- archive should sit alongside them without blurring the semantics
- do not expand existing multi-select delete into a general bulk hygiene system in this step

## UX guidance

### macOS
- keep archive / unarchive / export in the row context menu
- add an archive filter control in the sidebar header or immediately adjacent to the search/new-session controls without crowding the layout
- archived sessions should still use the same grouped list structure
- archived state should be visible enough that the user understands why an item appears in the archived view

### iOS
- expose archive / unarchive and export from row swipe or context actions in a way that does not fight the existing delete/clear affordances
- add a simple archive filter control near the session list header
- preserve the current list/search/navigation flow

## Proposed implementation slices
This spec is intentionally broken into PR-sized slices in `docs/specs/refactor/step15/`.

Recommended order:
1. `step15-1-swift-client-and-models.md`
2. `step15-2-session-view-model-archive-state.md`
3. `step15-3-session-browser-archive-filter-ui.md`
4. `step15-4-session-export-ui-and-final-smoke.md`

## Validation gates
Every slice must pass the standard workspace gates that apply to touched code.

For Swift work, also require a real app build and smoke on the Mac build node before merge to `dev`.

## Final verification goal
By the end of Step 15, a tester should be able to:
1. browse active sessions
2. archive a session
3. switch to archived view and find it
4. unarchive it and see it return to the active list
5. export an active or archived session as text or JSON
6. verify clear and delete still behave separately from archive

## Reviewer focus
- Does the Swift app consume the shipped Step 14 contract rather than the stale issue draft?
- Did the implementation stay in Option A scope with no bulk-action creep?
- Are archive, clear, and delete kept meaningfully distinct in the UI?
- Does export use the backend endpoint rather than rebuilding data client-side?
