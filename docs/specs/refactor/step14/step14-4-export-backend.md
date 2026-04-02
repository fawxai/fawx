# Step 14.4: Export Backend

## Branch
`codex/step14-4-export-backend`

## Goal
Add a canonical backend export endpoint that works for both active and archived sessions.

## Why this slice exists
Step 14 is not complete when archive state exists but export still assumes the session is active or only works through local CLI access.

Step 15 needs a backend export contract that can power UI export actions later without redefining session history semantics.

## Expected targets
- `engine/crates/fx-api/src/router.rs`
- `engine/crates/fx-api/src/handlers/sessions.rs`
- shared session export helpers if needed
- small adjacent response or formatter types as needed

## Required API contract
Add:

`GET /v1/sessions/{id}/export?format=text|json`

### Format rules
- default `format` is `text`
- `format=json` returns a structured export payload
- invalid format returns 400

### Export rules
- export works for active sessions
- export works for archived sessions
- export returns 404 for missing sessions
- archive state does not remove or degrade message history

### Payload expectations
#### Text export
Human-readable transcript suitable for download or sharing.

#### JSON export
Structured payload suitable for clients and tooling. Include enough metadata to identify the session and know whether it is archived.

Suggested top-level fields:
- session key
- session metadata or summary info
- archive metadata using the same field names exposed elsewhere in the API:
  `archived` and `archived_at`
- ordered messages
- total message count

## Relationship to existing work
This slice should align with the existing `fawx sessions export <id>` vocabulary and structured-session storage work. Do not invent a second incompatible notion of what a session export means.

The API endpoint can share formatting or serialization helpers with other export code if that keeps the contract consistent.

## Rules
- do not add Swift UI in this slice
- do not redesign archive routes or list filters here
- keep active and archived export behavior symmetrical
- do not require a session to appear in the default active list to export it by key

## Acceptance criteria
- export endpoint is routed and authenticated
- archived sessions export successfully by key
- active and archived sessions produce the same export shape except for archive metadata values
- text and json formats are both supported or clearly rejected when invalid
- missing sessions return stable not-found errors

## Tests required
1. export active session as text
2. export archived session as text
3. export active session as json
4. export archived session as json
5. invalid format returns 400
6. missing session returns 404
7. archive metadata appears in json export for archived sessions
8. exported message order matches stored history order

## Validation
```bash
cargo fmt --all
cargo clippy --workspace --tests -- -D warnings
cargo test --workspace
```

## Done means
- session export has a stable backend API contract
- archived sessions remain fully exportable
- Step 15 can add UI export affordances without backend guesswork

## Reviewer focus
- Does export stay consistent across active and archived sessions?
- Is the API contract clear enough for clients and tooling?
- Did this slice avoid sneaking in unrelated session-browser work?
