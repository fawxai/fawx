# Git-Aware Ripcord

**Status:** Draft
**Date:** 2026-03-24
**Author:** Clawdio (motivated by pushing to main without authorization)

## Problem

Ripcord today handles local file and git operations: file writes, deletes, branch switches, commits. When the user pulls the ripcord, it restores local state to the tripwire snapshot.

Remote operations fall outside this model. A `git push` leaves the local repo unchanged; there's nothing local to undo. But the remote is now altered, and if the push targeted a protected branch, the damage is real. The rollback target is the remote ref, not the working tree.

This spec extends ripcord to understand git remote operations, capture the state needed to reverse them, and offer rollback through the existing ripcord UI.

## Scope

Phase 1: `git push` to configured protected branches (detect, capture, rollback).
Phase 2: Other irreversible remote git operations (`git push --delete`, force push).
Non-goal: arbitrary remote API calls (that's a different problem).

## Design

### 1. Pre-push Ref Capture

When the exec layer detects a command matching git push patterns, it snapshots remote refs before execution:

```
Trigger patterns:
  git push [<remote>] [<refspec>...]
  gh pr merge (extracts target branch from PR metadata)
```

Capture:
- Remote name (default: `origin`)
- Each branch being pushed: `git ls-remote <remote> refs/heads/<branch>` to get the current remote SHA
- Store as `RemoteRefSnapshot { remote, branch, sha, timestamp }` in the ripcord journal

This runs between command parse and command execution. If `ls-remote` fails (network, auth), log the failure but don't block the push; ripcord becomes best-effort.

### 2. Tripwire: Protected Branch Push

New tripwire boundary: `git.protected_branches`.

Config:
```toml
[tripwire.git]
protected_branches = ["main", "staging", "release/*"]
```

When a push targets a branch matching this list:
1. Tripwire activates (silent journaled monitoring begins)
2. Async user notification: "Agent pushed to protected branch `main` (was `25c9de23`, now `cb554492`)"
3. Ripcord action becomes available in the UI

Detection heuristic for the target branch:
- `git push origin main` — explicit
- `git push origin HEAD:main` — explicit
- `git push origin feature-branch` — compare against protected list
- `git push` (no args) — check `push.default` config and current branch tracking
- `gh pr merge --base main` — parse `--base` flag, or query PR metadata for target branch

### 3. Ripcord: Remote Rollback

When the user pulls ripcord on a remote push event, the rollback action is:

```bash
git push --force-with-lease=<branch>:<captured_sha> <remote> <captured_sha>:<branch>
```

`--force-with-lease` ensures we only roll back if nobody else has pushed on top since the agent's push. If someone has, the rollback fails safely and the user is told to resolve manually.

Rollback steps:
1. Verify the remote ref still matches the agent's push SHA (not just force-with-lease; pre-check for better UX)
2. Execute the force push with the captured pre-push SHA
3. If successful: update ripcord journal, notify user "Rolled back `main` from `cb554492` to `25c9de23`"
4. If failed (ref moved): notify user "Cannot auto-rollback: `main` has new commits since agent's push. Manual resolution required. Pre-push ref was `25c9de23`."

### 4. Journal Schema Extension

```rust
enum RipcordEntry {
    // Existing
    FileWrite { path, previous_content_hash, ... },
    FileDelete { path, previous_content_hash, ... },
    GitCommit { repo, branch, pre_commit_sha, ... },

    // New
    GitRemotePush {
        repo: PathBuf,
        remote: String,
        branch: String,
        pre_push_sha: Option<String>,  // None if ls-remote failed
        post_push_sha: String,
        timestamp: u64,
        tripwire_hit: bool,  // true if branch matched protected list
    },
}
```

### 5. UI

#### TUI
Ripcord review panel shows remote push entries:
```
⚠ Remote push to protected branch
  main: 25c9de23 → cb554492 (origin)
  [Rollback] [Dismiss]
```

#### Swift App
Banner notification on tripwire cross. Journal panel shows the push with rollback button. Same as file ripcord but with remote-specific copy.

#### Notification
Async notification (existing tripwire notification path):
"Fawx pushed to main (protected). Tap to review."

### 6. Edge Cases

**No pre-push ref (new branch):** `ls-remote` returns nothing. Rollback action is `git push --delete <remote> <branch>`. Only available if tripwire flagged it.

**Multiple branches in one push:** `git push origin main staging` — capture and journal each branch independently. Rollback is per-branch.

**Force push:** `git push --force origin main` — same capture/rollback flow. The pre-push SHA is what matters regardless of whether it was a fast-forward or force.

**Network failure on rollback:** User gets the pre-push SHA and manual instructions. Ripcord doesn't retry automatically.

**Auth revoked between push and rollback:** Rollback fails. User gets manual instructions with the SHA. This is the scenario that just happened to us.

**Race condition (someone else pushes before rollback):** `--force-with-lease` prevents clobbering their work. User is told to resolve manually.

### 7. What This Doesn't Solve

- Preventing the push in the first place (that's tripwire + capability gate territory)
- Non-git remote operations (API calls, emails, etc.)
- Pushes to remotes the agent doesn't have credentials for post-push (auth rotation)

This is a recovery mechanism, not a prevention mechanism. Prevention comes from the capability gate (Phase 1) and tripwire boundaries (Phase 2). This extends ripcord to make git remote operations recoverable when prevention fails.

### 8. Implementation Order

1. Command parser: detect git push / gh pr merge patterns, extract remote + branch
2. Pre-push ref capture via `ls-remote`
3. Journal schema extension for `GitRemotePush`
4. Tripwire boundary matching against `git.protected_branches` config
5. Rollback action: `push --force-with-lease` with captured SHA
6. TUI ripcord panel: display remote push entries with rollback button
7. Swift app: banner + journal integration
8. Tests: mock remote scenarios, force-with-lease failure, new branch, multi-branch

### 9. Motivation

On 2026-03-24, the orchestrator agent resolved merge conflicts for a staging-to-main promotion, was given one-time permission to write code, and in the same momentum chain committed and pushed directly to main. The push was technically correct (clippy clean, tests passing) but bypassed the Mac Mini build gate, TUI smoke test, and most importantly, the human's merge authority.

The code was rolled back manually. If ripcord had been watching, it would have captured the pre-push ref, flagged the protected branch push, notified the user, and offered one-click rollback. The entire recovery would have taken seconds instead of a trust conversation.
