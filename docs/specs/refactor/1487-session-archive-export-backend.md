# Spec: #1487 Session Archive / Export Backend

## Status
Ready to implement.

## Goal
Add a clean backend contract for session archiving and session export so Step 15 can build Swift archive and export UI on top of a stable API.

Today the session stack supports delete and clear, but not archive. That creates a product gap in session hygiene and forces Step 15 to guess at backend semantics that should be explicit first.

## Why this is a backend step
Roadmap sequencing is already correct:

- Step 14: backend archive / export contract
- Step 15: Swift session archive / browser / export UI

This step should stop at the backend boundary. It should not introduce Swift views, session-browser affordances, or multi-select UI behavior.

## Problems to solve

### P1. No archived session state exists today
`fx-session` does not model a reversible archived session state, so the stack only knows about active, cleared, and deleted sessions.

### P2. List semantics are undefined
The system does not yet define whether archived sessions are hidden by default, recoverable, or filterable through the API.

### P3. Export backend contract is incomplete
Step 15 needs a backend export contract that works for both active and archived sessions. Export should not silently depend on a session remaining active.

### P4. Delete, clear, and archive are not clearly separated
Those actions have different meanings:
- delete removes the session
- clear preserves the session shell but drops message history
- archive preserves the session and message history while hiding it from the default active list

That distinction must be encoded in the backend contract before UI work begins.

## Contract decisions for Step 14

1. **Archive is reversible.**
   - Archiving a session does not delete it.
   - Archiving a session does not clear its message history.
   - Unarchive returns the session to the default active list.

2. **Default session listing excludes archived sessions.**
   - The default list behavior remains focused on active sessions.
   - Archived sessions remain accessible through explicit filter options.

3. **Archived sessions remain exportable.**
   - Archive is storage state, not data loss.
   - Export behavior should be the same for active and archived sessions.

4. **Archive is metadata, not a second storage system.**
   - Do not create a parallel archive database or move archived sessions to a different store.
   - The session store remains the single source of truth.

5. **Delete and clear stay separate.**
   - Do not soften delete into archive.
   - Do not overload clear into archive.
   - Existing delete and clear semantics remain intact.

## Proposed API shape
Keep the existing `/v1/sessions/{id}` naming convention, where `{id}` refers to the session key used elsewhere in the API docs.

### List sessions
`GET /v1/sessions?archived=active|all|only`

- `archived=active` is the default
- `archived=all` includes both active and archived sessions
- `archived=only` returns archived sessions only
- list and direct session-info responses should expose explicit archive fields:
  `archived: bool` and `archived_at: u64 | null`

Existing `kind` and `limit` query parameters remain supported.

### Archive / unarchive
- `POST /v1/sessions/{id}/archive`
- `DELETE /v1/sessions/{id}/archive`

These operations should be idempotent. Repeating archive on an archived session or unarchive on an active session should succeed without changing message history.

Archive and unarchive should return the canonical session summary shape, including the same
`archived` and `archived_at` fields exposed by list and direct lookup.

### Export
`GET /v1/sessions/{id}/export?format=text|json`

- default format: `text`
- `format=json` returns a structured export object suitable for clients and tooling
- export works for active and archived sessions
- JSON export should include archive metadata using the same field names:
  `archive.archived` and `archive.archived_at`

## Proposed implementation slices
This spec is intentionally broken into PR-sized slices in `docs/specs/refactor/step14/`.

Recommended order:
1. `step14-1-session-archive-metadata.md`
2. `step14-2-session-registry-archive-ops.md`
3. `step14-3-api-routes-and-list-filters.md`
4. `step14-4-export-backend.md`
5. `step14-5-contract-hardening-and-integration.md`

## Non-goals
- No Swift session list or browser UI
- No multi-select archive UI
- No archive-specific product polish in the app
- No soft-delete system
- No pagination redesign
- No speculative retention-policy system
- No separate archive storage layer

## Validation gates
Every slice must pass:

```bash
cargo fmt --all
cargo clippy --workspace --tests -- -D warnings
cargo test --workspace
```

The final slice must also pass a live headless API verification run that covers:
- archive an active session
- default list hides archived session
- explicit archived filter reveals it
- unarchive restores it to default list
- archived session export works
- delete and clear remain distinct from archive

## Reviewer focus
- Is archive clearly modeled as reversible metadata?
- Do list semantics stay simple and explicit?
- Does export work for archived sessions without special cases?
- Does the step stop at the backend boundary instead of leaking into Step 15 UI work?
