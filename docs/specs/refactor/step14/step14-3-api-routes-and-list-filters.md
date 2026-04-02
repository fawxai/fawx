# Step 14.3: API Routes and List Filters

## Branch
`codex/step14-3-api-routes-and-list-filters`

## Goal
Expose archive and unarchive operations, plus archived-session list filtering, through the HTTP API.

## Why this slice exists
The registry layer should already know how to archive, unarchive, and filter sessions by archive state. This slice makes that behavior available to external clients without adding export yet.

That keeps route work focused on request and response contract design.

## Expected targets
- `engine/crates/fx-api/src/router.rs`
- `engine/crates/fx-api/src/handlers/sessions.rs`
- API request/response types adjacent to those handlers if needed

## Required API contract
### List sessions
Extend the existing list endpoint:

`GET /v1/sessions?archived=active|all|only`

Rules:
- omit `archived` or pass `active` to get the current default behavior
- `all` returns active + archived sessions
- `only` returns archived sessions only
- existing `kind` and `limit` query parameters remain supported

### Archive
`POST /v1/sessions/{id}/archive`

Response shape should confirm the session key and resulting archive state.

### Unarchive
`DELETE /v1/sessions/{id}/archive`

Response shape should confirm the session key and resulting archive state.

### Session info
`GET /v1/sessions/{id}` should surface archive metadata so clients do not need to infer it from list placement.

## Error and idempotency rules
- missing session: 404
- archive on already archived session: success
- unarchive on already active session: success
- invalid `archived` filter value: 400 with a clear error

## Rules
- do not add export in this slice
- do not add Swift client code in this slice
- default list behavior must remain active-only
- response shapes should be explicit enough for Step 15 to consume directly

## Acceptance criteria
- routes are wired under the authenticated `/v1/sessions` tree
- list filtering works via the new `archived` query parameter
- archive and unarchive routes map directly onto registry behavior
- session info includes archive metadata
- missing and invalid-input cases return stable errors

## Tests required
1. list sessions defaults to active-only
2. list sessions with `archived=all` includes archived sessions
3. list sessions with `archived=only` excludes active sessions
4. invalid `archived` value returns 400
5. archive route archives the session and returns success payload
6. unarchive route restores active state and returns success payload
7. get session info includes archive metadata for archived session
8. missing session on archive/unarchive returns 404

## Validation
```bash
cargo fmt --all
cargo clippy --workspace --tests -- -D warnings
cargo test --workspace
```

## Done means
- backend archive operations are externally reachable through stable routes
- default and explicit list semantics are defined in the API contract
- Step 15 can consume archive state without inventing route behavior

## Reviewer focus
- Are route shapes simple and durable?
- Is default list behavior preserved?
- Are idempotency and error cases explicit instead of implicit?
