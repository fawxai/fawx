# Squad v2 Runbook (CLI Swarm)

This runbook defines the repeatable process for parallel issue-fix and PR-review swarms in Citros.

## Scope

Use this for:
- parallel bugfix bundle execution (multiple issues)
- parallel PR review generation

Do not mix this with subagent-based swarms in the same run.

---

## 1) Preflight (Required)

1. Confirm clean orchestration state:
```bash
ps -axo pid=,command= | grep -E 'codex exec --full-auto' | grep -v grep
```
2. Ensure repo is current:
```bash
cd ~/citros-staging
git fetch origin --prune
```
3. Prepare issue/review matrix (issue id, branch name, worktree path, log path).
4. Create one clean worktree per worker from `origin/staging`:
```bash
git worktree add -B <branch> <worktree> origin/staging
```
If worktree exists, force reset:
```bash
git -C <worktree> checkout -B <branch> origin/staging
git -C <worktree> reset --hard origin/staging
```

### 1.1 Scale Planning (Before Launch)

1. Build an issue dependency map (independent vs ordered).
2. Build a rough touched-file map and avoid parallel workers likely to edit the same files.
3. If overlap is unavoidable, assign one lead worker and serialize dependent workers.
4. Create a launch manifest file (e.g. `/tmp/squad-manifest.jsonl`) with:
   - issue/pr id
   - branch
   - worktree
   - log path
   - pid
   - state (`queued|running|done|failed|restarted`)

This manifest is the source of truth for restart-safe orchestration.

Validate manifest before launch:
```bash
scripts/squad/manifest-check.sh --manifest /tmp/squad-manifest.jsonl
```

---

## 2) Worker Classes + Reasoning

- **Fix workers:** `model_reasoning_effort="high"` (default)
- **Review workers:** `model_reasoning_effort="xhigh"`

Escalate fix workers to `xhigh` only for complex architectural bugs.

---

## 3) Worker Prompt Contract

Every worker prompt must include:
- exact issue/PR target
- branch/worktree path
- strict scope boundaries
- mandatory outputs: changed files, tests run/results, assumptions, caveats
- instruction to continue with best assumption (no pause for routine ambiguity)
- no GH actions from worker unless explicitly assigned
- for review-fix workers, an explicit **ALL comments** rule:
  - address blocking, non-blocking, and nice-to-have comments
  - if any item is intentionally deferred, open/link backlog issue and explain deferral in PR comment
  - do not mark ready-for-merge while any review item remains unresolved

---

## 4) Launch Pattern

Launch each worker with nohup + dedicated log:
```bash
# Queue entry first (id-scoped atomic upsert; creates manifest if missing)
scripts/squad/manifest-upsert.sh \
  --manifest /tmp/squad-manifest.jsonl \
  --id <issue-or-pr-id> \
  --branch <branch> \
  --worktree <worktree> \
  --log <log-file> \
  --state queued

nohup bash -lc "cd '<worktree>' && codex exec --full-auto -c model_reasoning_effort='\"high\"' \"\$(cat <task-file>)\"" > <log-file> 2>&1 &
PID=$!

# Transition same id to running with PID + startedAt (atomic upsert, no duplicate id)
scripts/squad/manifest-upsert.sh \
  --manifest /tmp/squad-manifest.jsonl \
  --id <issue-or-pr-id> \
  --branch <branch> \
  --worktree <worktree> \
  --log <log-file> \
  --state running \
  --pid "$PID" \
  --started-at "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
```

For review workers, use `xhigh`.

Recommended launch sequence:
1. add entry as `queued` to manifest
2. launch worker and capture `$!`
3. update same entry to `running` with pid + startedAt
4. run `scripts/squad/manifest-check.sh` before launching next worker

`manifest-upsert.sh` merge behavior for existing ids:
- merge base is the most recent existing line for that `id` (older duplicates are removed)
- `id/branch/worktree/log/state` are always overwritten from flags
- `pid/startedAt/restarts` are overwritten only when their flags are provided
- other legacy/custom fields on the merge-base line are preserved

### Build/Test-heavy workers
If Gradle/socket/git metadata restrictions appear, relaunch worker using an execution mode that allows full build/test and git metadata writes on the trusted node.

---

## 5) Monitoring Loop (10 min default)

At each interval:
```bash
ps -axo pid=,etime=,command= | grep -E 'codex exec --full-auto' | grep -v grep
tail -n 40 /tmp/codex-<id>.log
scripts/squad/manifest-check.sh --manifest /tmp/squad-manifest.jsonl
scripts/squad/monitor.sh --manifest /tmp/squad-manifest.jsonl --stall-minutes 15 --max-runtime-minutes 180
```

Mark worker as stalled if:
- no log progress for 15 minutes, or
- repeated environment/sandbox errors with no workaround.

Stall recovery:
1. kill worker
2. tighten prompt (explicit assumptions + execution mode)
3. relaunch
4. update launch manifest state and restart count

### 5.1 Watchdog Rules

- `no-output timeout`: if log file has no new content for 15 minutes, restart worker.
- `repeat-error threshold`: if same environment error repeats 3 times, escalate to orchestrator/manual mode.
- `runtime cap`: if worker exceeds configured max runtime, collect partial output and relaunch with narrowed scope.
- `duplicate-worker guard`: before any relaunch, verify no existing pid is active for that issue in manifest.

---

## 6) Completion Handling

For each completed worker:
1. Read final log summary.
2. Validate changed files in worktree:
```bash
git -C <worktree> status --short
git -C <worktree> diff --name-only
```
3. Run required local checks if worker could not execute tests.
4. Complete git operations if worker couldn’t:
```bash
git -C <worktree> add -A
git -C <worktree> commit -m "fix: ..."
git -C <worktree> push -u origin <branch>
```
5. Open PR / post review comments.
6. Mark manifest state as `done` with artifact links (PR URL, review comment URL, test log).

Do not claim completion until commit + push + PR/comment actions are confirmed.

### 6.1 Partial-Completion Recovery

Handle these explicitly:

- **code changed, no commit**: orchestrator commits/pushes after verifying test status.
- **commit exists, no push**: push branch and verify remote head.
- **push exists, no PR**: create PR and verify URL.
- **PR exists, no review comment**: post required structured review comment.
- **tests skipped due env restrictions**: rerun required tests in trusted mode before PR ready.

---

## 7) Review Worker Output Handling

### 7.0 GitHub Write Touchpoint (Control Plane)

Default rule in CLI swarms:
- Workers produce artifacts (diff analysis, fix summaries, suggested replies).
- Orchestrator performs GitHub write actions:
  - post PR review comments
  - post review-thread replies ("addressed in commit ...")
  - post readiness/deferral comments

Only deviate from this if the run explicitly delegates a specific GitHub write action to a worker.

For PR review workers:
1. Read generated review markdown/text.
2. Sanity-check for hallucinations or stale API claims.
3. Post review comment with structured sections:
   - Summary
   - Verdict
   - Blocking Issues
   - Non-blocking Issues
   - Nice-to-haves

For review-fix workers (addressing existing PR feedback):
1. Ingest **all** review items (blocking, non-blocking, nice-to-have).
2. Implement fixes for all items in-scope.
3. If an item is deferred, create/link backlog issue and add PR comment documenting deferral rationale.
4. Re-run relevant tests and push updates.
5. Request another review pass; repeat until no unresolved review items remain.
6. Maintain chain-of-custody mapping (review_comment_id -> commit_sha -> reply_url) for each addressed item.

---

## 8) Edge-Case Matrix (Scale)

| Edge case | Detection | Required action |
|---|---|---|
| Duplicate worker for same issue | manifest has duplicate claimed id/branch/worktree | block launch; kill stale pid or skip relaunch |
| Dirty worktree | `git status --short` non-empty prelaunch | hard reset/recreate worktree |
| Branch already exists remotely | `git ls-remote` head match | resume existing branch path; do not fork duplicate branch |
| PR already open | `gh pr list --head <branch>` | attach to existing PR; continue review/fix flow |
| Two issues touch same file | touched-file preflight overlap | serialize workers or designate lead integrator |
| Worker asks clarification | log contains pause/question request | auto-relaunch with no-question assumptions contract |
| No log output 15m | mtime/static tail | watchdog restart |
| Repeated env error | same error signature 3x | switch execution mode; manual orchestrator fallback |
| Gradle/socket sandbox error | `SocketException Operation not permitted` | rerun in trusted execution mode |
| Cannot commit (`index.lock`/git metadata) | commit error in log | orchestrator commit/push step |
| Push rejected/non-fast-forward | git push failure | fetch/rebase/cherry-pick and retry push |
| CI fails after worker done | failing checks on PR | spawn focused fix worker, repeat review loop |
| Review comments incomplete | unresolved comments in PR thread | reopen fix cycle; address all or defer with linked issue |
| API rate limit/github transient | 403/5xx on GH ops | exponential backoff + retry with cap |
| Orchestrator restart/crash | missing in-memory state | rebuild state from manifest + branch/PR queries |
| Secrets leak risk in logs | key-like patterns in output | redact before posting and rotate key if exposed |

---

## 9) Shutdown

When all workers are idle:
1. Final status post to user (per issue/PR).
2. Disable monitoring cron job.
3. Persist lessons learned to memory/docs if process changes are discovered.
4. Archive manifest + logs for the run (`/tmp/squad-runs/<timestamp>/`).

---


## 10) Script Regression Checks

Before claiming runbook/tooling updates are ready:

```bash
scripts/squad/manifest-check.sh --manifest /tmp/squad-manifest.jsonl
scripts/squad/monitor.sh --manifest /tmp/squad-manifest.jsonl --stall-minutes 15 --max-runtime-minutes 180
```

Also run sample-case checks (healthy, duplicate claimed entries, dead pid, stalled log, overdue runtime) and confirm expected exit codes.

---
## 11) Known Failure Modes + Fixes

1. **Worker pauses due pre-existing edits**
   - Fix: hard reset worktree from `origin/staging` before launch.
2. **Gradle socket restriction in sandbox**
   - Fix: run in execution mode allowing full Gradle operation on trusted node.
3. **Cannot commit due worktree git metadata path permissions**
   - Fix: orchestrator performs commit/push, or relaunch with proper write access.
4. **Over-slow swarm due universal xhigh**
   - Fix: high for fixers, xhigh for reviewers only.
5. **Duplicate PR branches from relaunches**
   - Fix: consult manifest + remote branch existence before relaunch.
6. **Review loop drift (not all comments addressed)**
   - Fix: enforce all-comments completion gate and explicit deferral traceability.
