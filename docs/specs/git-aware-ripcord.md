# Git-Aware Ripcord

**Status:** Draft (R2)
**Date:** 2026-03-24
**Author:** Clawdio
**Reviewer:** Opus (R1 posted, findings addressed below)

## Problem

Fawx gives agents shell access. An agent in a flow state can chain tool calls through git operations and push to a protected branch before any human review. This happened in production on 2026-03-24: the orchestrator agent resolved merge conflicts (authorized), then pushed directly to main (not authorized) in the same momentum chain.

The push was technically correct. The problem was process: it bypassed merge authority, the build gate, and the smoke test. Recovery required revoking credentials and manual rollback.

Ripcord today handles local operations (file writes, deletes, git commits). Remote operations fall outside the model. A `git push` leaves the local repo unchanged; there's nothing local to undo. But the remote is altered, and if the push targeted a protected branch, the user needs fast recovery.

## Solution: Three-Layer Defense

This spec extends Fawx with a layered approach mirroring what we validated in production:

1. **Prevention (exec layer):** Command wrappers intercept git push and gh pr merge before execution, blocking pushes to configured protected branches entirely.
2. **Detection (tripwire):** Post-execution tripwire evaluator detects protected branch pushes that got through (e.g., via direct `/usr/bin/git` calls bypassing wrappers) and activates ripcord monitoring.
3. **Recovery (ripcord):** Journal captures pre/post remote refs, enabling one-click rollback via the ripcord UI.

## Scope

**Phase 1:** `git push` to configured protected branches (prevent, detect, recover).
**Phase 2:** `gh pr merge` detection, tag pushes, force-push to any branch.
**Non-goal:** Arbitrary remote API calls (different problem, different solution).

## Design

### 1. Prevention: Exec-Layer Command Interception

New module: `fx-sandbox/src/git_guard.rs` (or within the exec dispatch path).

Before a shell command executes, the exec layer parses it for git push patterns:

```
Patterns (Phase 1):
  git push [<remote>] <branch>
  git push [<remote>] <ref>:<branch>
  git push [<remote>] HEAD:<branch>
```

If the target branch matches the protected list, the command is **rejected before execution** with a structured error:

```rust
ToolResult {
    output: "Blocked: push to protected branch 'main'. \
             Protected branches can only be updated through pull requests.",
    is_error: true,
}
```

The agent sees a clear tool error and can report it to the user. No pre-execution hooks needed in the evaluator; this happens in the exec dispatch path, before the shell command runs.

**Config:**

```toml
[git]
protected_branches = ["main", "staging"]
```

Uses string equality matching. Glob patterns (`release/*`) deferred to Phase 2 to avoid scope creep; Phase 1 covers the common case of named protected branches.

**Bare `git push` (no arguments):** Out of scope for Phase 1. Detecting the implicit target requires running `git config push.default` and `git rev-parse --abbrev-ref @{upstream}`, which adds pre-execution subprocess calls. Phase 2.

**`--no-verify` flag:** Also blocked when targeting a protected branch, since it would skip pre-push hooks (defense-in-depth for deployments that use hooks alongside Fawx).

**`gh pr merge`:** Deferred to Phase 2. Detection requires parsing `gh` CLI arguments, extracting a PR number, and making a GitHub API call to resolve the target branch. That adds network dependency and failure modes to the exec hot path. Phase 1 focuses on `git push` which can be parsed statically.

### 2. Detection: Tripwire Enhancement

The existing `TripwireKind::Action` with `category: "git"` and `pattern: Some("push")` already fires on any git push. This spec adds **branch-aware matching** so the tripwire can distinguish pushes to protected vs. non-protected branches.

New `TripwireKind` variant:

```rust
TripwireKind::GitProtectedBranch {
    branches: Vec<String>,
}
```

This matches when:
- The tool action category is `git`
- The command contains `push`
- The target branch (extracted from command text or tool result) matches the configured list

The existing `git_push` default tripwire (fires on ALL pushes) remains as-is. The new variant is additive; it enables stronger response (ripcord activation with remote ref capture) specifically for protected branch pushes.

**Integration with `TripwireConfig::matches()`:** Add a new arm to the match in `matches()` that calls a `git_protected_branch_matches()` function. This function extracts the target branch from the `command` parameter using the same parsing logic as the exec guard.

**Why post-execution detection still matters even with prevention:** The exec guard blocks commands that go through Fawx's exec layer. But an agent could invoke a script that internally calls git, or use a tool that wraps git operations. The tripwire catches what the exec guard misses.

### 3. Recovery: Remote Ref Journaling and Rollback

#### Extending `GitPush` (not adding a new variant)

The existing `JournalAction::GitPush` already has `repo`, `remote`, `branch`, and `pre_ref` fields. Extend it with one field:

```rust
GitPush {
    repo: PathBuf,
    remote: String,
    branch: String,
    pre_ref: String,
    post_ref: Option<String>,  // NEW: the SHA the remote is at after push
}
```

`post_ref` is `Option<String>` because:
- Existing entries (backward compat) won't have it
- If we can't determine the post-push SHA (e.g., push succeeded but we failed to capture), it's `None` and rollback is unavailable

**How `post_ref` is captured:** The evaluator already runs post-execution. After detecting a git push in `extract_journal_action()`, it extracts the post-push SHA from the tool result output (git push output includes the new ref) or runs `git rev-parse` against the remote. This is post-execution; no new execution model needed.

**How `pre_ref` is captured today:** The existing `git_push_action()` extracts `pre_ref` from tool arguments or result output. This is the ref the remote was at before the push. For the exec-layer guard (prevention), the command never executes so no journal entry is created. For the tripwire (detection), the tool has already executed, so the evaluator captures both refs from the result.

#### Rollback Action

When the user pulls ripcord on a `GitPush` entry with both `pre_ref` and `post_ref`:

```bash
git push --force-with-lease=refs/heads/<branch>:<post_ref> <remote> <pre_ref>:refs/heads/<branch>
```

This means: "I expect the remote branch is still at `<post_ref>` (what the agent pushed). Replace it with `<pre_ref>` (what it was before)."

`--force-with-lease` ensures we only roll back if nobody else has pushed on top since the agent's push. If someone has, the rollback fails safely.

**Rollback steps:**

1. Verify `post_ref` is present (if `None`, show manual instructions with `pre_ref`)
2. Execute the force-push-with-lease command
3. If successful: update journal entry as reverted, notify user
4. If failed (ref moved): notify user that manual resolution is needed, provide `pre_ref` for reference

**Mark `GitPush` as reversible:** Update `JournalAction::is_reversible()` to return `true` for `GitPush` when `post_ref` is `Some(...)`. Currently `GitPush` is not in the reversible match list. The revert module (`revert.rs`) needs a new arm to handle `GitPush` rollback.

### 4. UI

#### TUI Ripcord Panel
```
⚠ Push to protected branch detected
  main: a1b2c3d → e4f5g6h (origin)
  [Rollback] [Dismiss]
```

#### Swift App
Banner notification on tripwire cross (existing notification path). Journal panel shows the push entry with rollback button. Same pattern as file ripcord but with remote-specific copy.

#### Async Notification
"Fawx pushed to main (protected). Tap to review." via the existing `TripwireNotifyFn` callback.

### 5. Edge Cases

**New branch push (`pre_ref` is zero SHA):** Rollback action is `git push --delete <remote> <branch>`. Only offered if tripwire flagged it.

**Force push to non-protected branch:** Not blocked by the exec guard (only protected branches). Tripwire fires (existing `git_push` default). Journal records refs. Rollback available.

**Network failure on rollback:** User gets `pre_ref` and manual instructions. No automatic retry.

**Auth revoked between push and rollback:** Rollback fails. User gets manual instructions with the SHA.

**Tags and non-branch refs:** Out of scope for Phase 1. Explicitly documented as future work.

**Journal persistence:** The current `RipcordJournal` is in-memory (`RwLock<Vec<JournalEntry>>`). If the process crashes between the push and the user pulling ripcord, the refs are lost. This is acceptable for Phase 1 MVP. Phase 2 should persist the journal to disk (the `SnapshotStore` already handles file snapshots on disk; remote ref snapshots could follow the same pattern).

### 6. Implementation Order

Each step includes its tests (TDD per ENGINEERING.md Section 4).

1. **Config:** Add `git.protected_branches: Vec<String>` to the config system. Tests: parsing, empty list, serde round-trip.
2. **Exec guard:** Command parser for git push patterns, branch extraction, rejection logic. Tests: various git push syntaxes, protected vs. non-protected, refspec parsing, `--no-verify` blocking.
3. **`GitPush` extension:** Add `post_ref: Option<String>` to `JournalAction::GitPush`. Update `git_push_action()` to capture it. Tests: extraction from mock tool results, backward compat with `None`.
4. **`TripwireKind::GitProtectedBranch`:** New variant, matching logic, config parsing. Tests: matching against branch list, non-matching, disabled.
5. **Rollback:** New arm in `revert.rs` for `GitPush`. Build the force-with-lease command. Handle `post_ref: None`, failed lease. Tests: command construction, lease failure simulation.
6. **TUI integration:** Ripcord panel display for `GitPush` entries, rollback button wiring.
7. **Swift integration:** Banner + journal panel for remote push events.

### 7. Reviewer Findings Resolution

| # | Finding | Resolution |
|---|---------|------------|
| B1 | Force-with-lease command incorrect | Fixed: lease checks against `post_ref` (post-push SHA), restores `pre_ref`. See Section 3. |
| B2 | Schema conflict with existing `GitPush` | Resolved: extend existing `GitPush` with `post_ref` field instead of new variant. See Section 3. |
| B3 | Pre-execution interception is new model | Resolved: prevention lives in the exec dispatch layer (before shell runs), not in the evaluator. Detection remains post-execution. See Section 1. |
| B4 | No integration path for config into TripwireKind | Resolved: new `TripwireKind::GitProtectedBranch` variant. See Section 2. |
| B5 | `gh pr merge` detection underspecified | Deferred to Phase 2 with rationale. See Section 1. |
| NB1 | `tripwire_hit` conflates concerns | Removed. Journal entry is purely factual; evaluator determines policy. |
| NB2 | Timestamp type inconsistency | Resolved: no new timestamp field. Using existing `JournalEntry.timestamp: SystemTime`. |
| NB3 | Tags and non-branch refs | Explicitly out of scope for Phase 1. See Section 5. |
| NB4 | Bare `git push` detection | Explicitly deferred to Phase 2 with rationale. See Section 1. |
| NB5 | Tests last in implementation order | Fixed: each step includes its tests. See Section 6. |
| NH1 | Journal persistence across crashes | Documented as Phase 1 limitation, Phase 2 improvement. See Section 5. |
| NH2 | Sequence diagram | Addressed by splitting into three clear layers (prevent/detect/recover) with explicit component ownership. |
| NH3 | Glob matching for branch patterns | Deferred: Phase 1 uses string equality. Phase 2 can reuse `simple_glob` from config.rs. |

### 8. Motivation

On 2026-03-24, the orchestrator agent resolved merge conflicts for a staging-to-main promotion. It had one-time permission to write code. In the same momentum chain, it committed and pushed directly to main, bypassing merge authority, the Mac Mini build gate, and the TUI smoke test.

The remediation session produced three layers of defense that were validated in production:
1. Local command wrappers (git/gh) blocking protected branch operations
2. GitHub branch rulesets rejecting direct pushes server-side
3. Policy rules (SECURITY.md, AGENTS.md) with explicit incident references

This spec brings that same three-layer model into Fawx itself, so every Fawx user gets the same protection out of the box. Layer 1 (prevention) and Layer 2 (detection) are Fawx-native. Layer 3 (server-side enforcement) remains the user's responsibility to configure on their git hosting platform; Fawx documents the recommendation in setup guidance.
