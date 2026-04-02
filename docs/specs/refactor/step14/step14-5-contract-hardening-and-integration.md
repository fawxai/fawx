# Step 14.5: Contract Hardening and Integration

## Branch
`codex/step14-5-contract-hardening-and-integration`

## Goal
Finish Step 14 by tightening the final backend contract, filling any missing docs or response details, and proving the full archive and export flow through live headless API verification.

## Why this slice exists
After slices 14.1 through 14.4 land, the backend should function end to end. The final slice makes sure the contract is durable, documented, and verified as a complete backend feature set before Step 15 starts consuming it.

This is where we catch any last drift between registry behavior, API semantics, and export behavior.

## Expected targets
- API docs or spec references tied to session endpoints
- small backend response-shape or handler cleanups needed to complete the contract
- integration tests that exercise archive, list filtering, unarchive, and export together

## Required work
### Contract hardening
- make sure session list, session info, archive, unarchive, and export all expose archive metadata consistently
- remove any ambiguous or duplicated response-shape logic introduced by earlier slices
- document final archive and export endpoint semantics where the repo currently documents session API behavior

### Integration coverage
Add end-to-end backend tests that cover the lifecycle:
1. create or seed active session
2. archive it
3. confirm default list hides it
4. confirm archived filter reveals it
5. export archived session
6. unarchive it
7. confirm default list shows it again
8. confirm clear and delete still behave distinctly

## Rules
- no Swift UI work
- no new archive product features beyond the agreed contract
- prefer cleanup and hardening over new scope
- if a route or field must change from an earlier slice, update the Step 14 docs in the same PR so the execution pack remains truthful

## Acceptance criteria
- final backend contract is documented and internally consistent
- integration tests cover the full archive and export lifecycle
- live headless API verification passes on a fresh lane
- Step 14 can be declared complete without relying on Step 15 work

## Validation
```bash
cargo fmt --all
cargo clippy --workspace --tests -- -D warnings
cargo test --workspace
```

## Live headless API verification
Use the headless API only. No TUI smoke test is required.

On a fresh verification lane, prove this exact flow:
1. create or select a known active session
2. `GET /v1/sessions` shows it in the default active list
3. `POST /v1/sessions/{id}/archive` succeeds
4. `GET /v1/sessions` no longer includes it
5. `GET /v1/sessions?archived=only` includes it
6. `GET /v1/sessions/{id}` shows archive metadata
7. `GET /v1/sessions/{id}/export?format=json` succeeds and includes archive metadata plus ordered messages
8. `DELETE /v1/sessions/{id}/archive` succeeds
9. `GET /v1/sessions` includes it again
10. verify clear and delete still do what they did before and are not conflated with archive

## Pass criteria
- archive and unarchive are reversible and idempotent
- default list behavior remains active-only
- archived filter and direct lookup behave consistently
- export works for archived sessions without special handling
- clear and delete remain distinct from archive

## Done means
- Step 14 backend archive and export contract is complete
- the execution pack docs match the landed behavior
- Step 15 can start from a stable backend surface

## Reviewer focus
- Is the backend contract now coherent end to end?
- Did the final slice stay disciplined and avoid UI spillover?
- Does the live API verification actually prove the intended user-facing backend semantics?
