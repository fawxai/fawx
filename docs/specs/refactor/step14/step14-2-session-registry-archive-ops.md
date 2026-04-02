# Step 14.2: Session Registry Archive Operations

## Branch
`codex/step14-2-session-registry-archive-ops`

## Goal
Add archive and unarchive behavior to the session registry and store layer, including internal list filtering, while keeping the API surface unchanged for now.

## Why this slice exists
Once session types can represent archive state, the next job is to make the session store actually support that lifecycle:
- archive a session
- unarchive a session
- persist the transition
- filter archived sessions out of the default active list

This work belongs below the HTTP layer so the API slice can stay thin and map directly onto stable registry behavior.

## Expected targets
- `engine/crates/fx-session/src/registry.rs`
- `engine/crates/fx-session/src/types.rs`
- small adjacent store or query helpers as needed

## Required behavior
### Archive
- mark the target session archived
- preserve message history
- preserve session metadata besides archive fields
- operation is idempotent

### Unarchive
- clear archive state for the target session
- preserve message history
- operation is idempotent

### List semantics in the registry
Add explicit internal filtering support for archive state.

Preferred shape:
- `ActiveOnly` as the default
- `All`
- `ArchivedOnly`

This can be an enum or equivalent typed filter. Do not use loosely-coupled booleans throughout the call graph if a typed filter fits cleanly.

## Rules
- default registry listing excludes archived sessions
- direct lookup by session key still works for archived sessions
- delete and clear semantics remain unchanged
- no HTTP route changes in this slice
- no export logic in this slice

## Acceptance criteria
- registry can archive and unarchive any existing session
- archive state persists across reloads/restarts
- default list behavior returns active sessions only
- explicit internal filters can include or isolate archived sessions
- missing session behavior remains a clean not-found error

## Tests required
1. archiving an active session marks it archived and preserves messages
2. archiving an already archived session is idempotent
3. unarchiving an archived session restores active state
4. unarchiving an active session is idempotent
5. default list excludes archived sessions
6. archived-only filter returns archived sessions only
7. direct get/find still returns archived sessions by key
8. delete and clear tests still pass unchanged

## Validation
```bash
cargo fmt --all
cargo clippy --workspace --tests -- -D warnings
cargo test --workspace
```

## Done means
- the registry owns archive lifecycle behavior
- list filtering semantics are explicit and stable
- the API slice can expose archive behavior without inventing backend logic in HTTP handlers

## Reviewer focus
- Is archive lifecycle behavior correctly centered in the registry/store layer?
- Are list filters typed and explicit?
- Did this slice preserve message history and existing delete/clear behavior?
