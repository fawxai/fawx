# Step 15.1: Swift Client and Models

## Branch
`codex/step15-1-swift-client-and-models`

## Goal
Wire the Step 14 backend endpoints into `FawxClient` and update the Swift `Session` model to decode archive metadata. No UI changes yet.

## Why this slice exists
The rest of Step 15 needs client plumbing and model support before any view or view-model work can begin. Doing this first keeps the UI slices focused on presentation and interaction rather than API mechanics.

## Expected targets
- `app/Fawx/Networking/FawxClient.swift`
- `app/Fawx/Models/Session.swift`

## Required client additions

### List sessions with archive filter
Extend `listSessions` to accept an optional archive filter parameter.

The backend contract is:
`GET /v1/sessions?archived=active|all|only`

Add the `archived` query item when the caller passes a filter value. Default behavior (omitting the parameter or passing `active`) must remain backward compatible with the current list call.

### Archive a session
`POST /v1/sessions/{id}/archive`

Add a method like `archiveSession(id:)` that returns the server's confirmation payload.

### Unarchive a session
`DELETE /v1/sessions/{id}/archive`

Add a method like `unarchiveSession(id:)` that returns the server's confirmation payload.

### Export a session
`GET /v1/sessions/{id}/export?format=text|json`

Add a method like `exportSession(id:format:)` that returns the export payload.

For the text format, the response is a plain-text transcript. For json, the response is a structured export object. The method should handle both cleanly, potentially returning a typed enum or separate accessors.

## Required model updates

### Session archive metadata
The `Session` struct currently does not decode `archived_at`. Step 14 now includes this field in session info responses.

Add:
- `archivedAt: Int?` with a `CodingKey` mapping to `archived_at`
- use `decodeIfPresent` or `serde(default)` equivalent so existing active sessions without the field still decode cleanly
- a computed `isArchived: Bool` helper

### Archive filter enum
Add a Swift enum like `SessionArchiveFilter` with cases `active`, `all`, `only` that maps to the query parameter values.

### Response types
Add any response types needed for archive, unarchive, and export payloads if they differ from existing response shapes. Keep them minimal and aligned with what the backend actually returns.

## Rules
- no view or view-model changes in this slice
- keep backward compatibility for the default list call
- match the shipped Step 14 backend contract exactly
- do not invent client-side archive semantics that differ from the backend

## Acceptance criteria
- `FawxClient` can list sessions with archive filters
- `FawxClient` can archive and unarchive a session
- `FawxClient` can export a session as text or json
- `Session` model decodes `archived_at` from the server
- existing list/create/clear/delete calls remain unaffected

## Tests
If the app has existing unit tests for `FawxClient` or `Session`, add targeted coverage for:
1. `Session` decoding with and without `archived_at`
2. `isArchived` computed property
3. archive filter query parameter construction

If the app does not have a test harness for client methods, document the expected manual verification steps instead.

## Validation
Build on the Mac build node:
```bash
xcodebuild -scheme Fawx-macOS -destination 'platform=macOS' build
```

## Done means
- client and model plumbing exists for all Step 14 endpoints
- no UI changes landed yet
- later slices can add view-model and view support without touching `FawxClient` or `Session` model internals

## Reviewer focus
- Does the client match the shipped Step 14 contract?
- Is the archive filter wired as a query parameter, not a path change?
- Is the Session model update backward compatible?
