# Step 15.2: Session ViewModel Archive State

## Branch
`codex/step15-2-session-view-model-archive-state`

## Goal
Add archive filter state, archive/unarchive mutation helpers, and export support to `SessionViewModel` so the UI slices can bind directly to view-model behavior without implementing business logic in views.

## Why this slice exists
The macOS sidebar and iOS session list both need the same archive-aware session list behavior. Putting archive filter state, archive/unarchive actions, and export behavior in the shared view model avoids duplicating logic across two platform views.

## Expected targets
- `app/Fawx/ViewModels/SessionViewModel.swift`

## Required view-model additions

### Archive filter state
Add a published/observable property for the current archive filter.

Default: `active` (matching the backend default and current behavior).

When the filter changes, `refresh()` should pass the filter to the client's list call.

### Archive mutation
Add `archiveSession(id:)` and `unarchiveSession(id:)` methods.

After a successful archive:
- if the current filter is `active`, remove the session from the local list (it is now hidden)
- if the selected session was archived, clear the selection

After a successful unarchive:
- if the current filter is `only`, remove the session from the local list (it is no longer archived)
- if the current filter is `active` or `all`, the session should appear back in the list on the next refresh, or be optimistically inserted

### Export
Add an `exportSession(id:format:)` method that calls the client and returns the export content.

The view layer will handle presenting the share/save sheet, but the view model should own the fetch.

### Refresh behavior
`refresh()` should pass the current archive filter to the client so the list result reflects the selected filter.

## Rules
- no view changes in this slice
- keep the view model's existing list/create/clear/delete behavior unchanged
- archive filter defaults to `active` so the app behavior is identical until the UI actually switches filters
- do not add bulk actions

## Acceptance criteria
- view model tracks archive filter state
- refresh uses the current filter
- archive and unarchive mutations update local state correctly for each filter mode
- export method fetches from the backend

## Tests
If the app has existing view-model tests, add targeted coverage for:
1. default filter is `active`
2. archiving a session removes it from the local list when filter is `active`
3. unarchiving a session removes it from the local list when filter is `only`
4. refresh passes the current filter to the client

If no test harness exists, document expected manual verification.

## Validation
Build on the Mac build node:
```bash
xcodebuild -scheme Fawx-macOS -destination 'platform=macOS' build
```

## Done means
- view model is archive-aware and export-capable
- views can bind to filter state and call archive/unarchive/export without implementing business logic
- existing session list behavior is unchanged when filter is `active`

## Reviewer focus
- Does the view model stay the single source of truth for archive state?
- Are optimistic local-state updates correct for each filter mode?
- Is export delegated cleanly to the view model without leaking fetch logic into views?
