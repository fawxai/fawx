# Backlog: Privacy List Persistence

## Summary

Defer privacy-list backup/restore + first-run recovery prompt from PR #688.

## Tracking

- GitHub issue: https://github.com/abbudjoe/fawx/issues/697
- Status: tracked for post-PR #688 follow-up (explicitly out-of-scope for runtime enforcement changes).
- PR comment draft with rationale: `docs/backlog/pr688-r24-defer-comment.md`
- Local reference: `docs/backlog/privacy-list-persistence.md`

## Why Deferred

PR #688 focuses on runtime privacy enforcement (`read_screen`, `screenshot`, and action blocking) and redaction safety contracts.

Persistence behavior requires cross-cutting policy decisions that are not implemented in this branch:

1. Backup policy alignment with the current `android:allowBackup="false"` hardening posture.
2. UX ownership for first-run prompt copy, timing, and one-time persistence semantics.
3. Reliable "app has been used before" signal and tests across process restarts.

## Acceptance Criteria

1. Define explicit backup policy for privacy-list preferences (include or intentionally exclude) with manifest/rules tests.
2. Add one-time startup prompt when privacy list is empty and prior usage signal is true.
3. Persist "prompt shown" state to prevent repeated prompting.
4. Add unit/integration tests for both fresh install and returning-user restore/reset scenarios.
5. Update `docs/specs/h2-privacy-apps.md` to reflect shipped behavior.
