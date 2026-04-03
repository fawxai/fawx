# Git Self-Modification Workflow
## Safe Self-Improvement Through Tiered Path Enforcement

**Date:** 2026-03-01
**Status:** Draft — approved for implementation guidance
**Related:** memory-and-signals-plan.md, fawx-architecture.html §Kernel Invariants

---

## Core Principle

Fawx can improve itself, but safety is non-negotiable. The kernel's immutability is enforced at runtime, not by convention. Self-modification happens through git branches with human oversight — never direct writes to the working tree on the active branch.

---

## Three-Tier Path Enforcement

All file writes by the agent are checked against a tiered allowlist before execution.

### Tier 1: Allow (Loadable Layer)
Agent can write code directly, create branches, and present diffs for approval.

**Paths:**
- `engine/crates/fx-loadable/**`
- `engine/crates/fx-skills/**`
- `~/.fawx/skills/**`
- `~/.fawx/config.toml`

**Gate:** Tests pass + human approval of diff.

### Tier 2: Propose (Kernel + Shell + Platform)
Agent can identify problems and generate proposed patches, but CANNOT write to these files directly. The proposal includes rationale, evidence (signal data), proposed diff, and test cases.

**Paths:**
- `engine/crates/fx-kernel/**`
- `engine/crates/fx-core/**`
- `engine/crates/fx-security/**`
- `engine/crates/fx-llm/**`
- `engine/crates/fx-cli/**`

**Gate:** Human reviews reasoning AND diff. Human explicitly authorizes the write, or applies the patch manually.

**Proposal format (Phase 1 — simplified):** Written to `~/.fawx/proposals/{timestamp}-{description}.md`:
```markdown
# Proposal: {title}

## What and Why
{Description + signal evidence}

## Proposed Diff
{File paths and diff}

## Risk
{One-liner: what could go wrong}
```

**Phase 2 (remote review):** Expand to full template with separate Evidence, Test Cases, and Risk Assessment sections.

### Tier 3: Deny (Hard Block)
Agent cannot write or propose changes. Any attempt is blocked and logged.

**Paths:**
- `.git/**`
- `~/.fawx/credentials/**`
- `~/.fawx/proposals/**` contents of other proposals (no self-referential modification)

---

## Configuration

```toml
[self_modify]
# Master switch — off by default, user must opt in
enabled = false

# Branch prefix for all self-modification work
branch_prefix = "fawx/improve"

# Require tests to pass before presenting diff to user
require_tests = true

[self_modify.paths]
allow = [
    "engine/crates/fx-loadable/**",
    "engine/crates/fx-skills/**",
    "~/.fawx/skills/**",
    "~/.fawx/config.toml",
]
propose = [
    "engine/crates/fx-kernel/**",
    "engine/crates/fx-core/**",
    "engine/crates/fx-security/**",
    "engine/crates/fx-llm/**",
    "engine/crates/fx-cli/**",
]
deny = [
    ".git/**",
    "~/.fawx/credentials/**",
]
# deny > propose > allow (deny always wins)
```

---

## Git Workflow

### Phase 1: Local-Only Self-Modification

All self-improvement happens locally. No remote git operations.

```
Signal analysis detects friction pattern
  │
  ▼
Agent determines fix location
  │
  ├─► Allow tier path?
  │     │
  │     ▼
  │   Create branch: fawx/improve/{description}
  │   Write changes to allowed files
  │   Run tests
  │     │
  │     ├─► Tests pass → Present diff to user
  │     │                  │
  │     │                  ├─► User approves → Merge to current branch
  │     │                  └─► User rejects → Delete branch
  │     │
  │     └─► Tests fail → Report failure, delete branch
  │
  ├─► Propose tier path?
  │     │
  │     ▼
  │   Write proposal to ~/.fawx/proposals/
  │   Present proposal to user
  │     │
  │     ├─► User authorizes → Agent creates branch, writes code, runs tests
  │     │                      Then follows Allow tier flow (diff → approve/reject)
  │     └─► User declines → Proposal archived
  │
  └─► Deny tier path?
        │
        ▼
      Block. Log attempt. Inform user.
```

### Phase 2: Remote Git (Future)

For team scenarios or when Fawx manages its own public repo:
- `git_push` to remote
- `git_pr_create` to open pull request
- Await external review
- CI must pass in addition to local tests

Not in scope for Phase 1.

---

## Git Tools

### Phase 1 (new tools for GitSkill)

| Tool | Purpose | Notes |
|------|---------|-------|
| `git_branch_create` | Create improvement branch | Always prefixed with `fawx/improve/` |
| `git_branch_switch` | Switch between branches | Can switch to any local branch |
| `git_branch_delete` | Clean up rejected branches | Only `fawx/improve/*` branches |
| `git_merge` | Merge improvement branch | **Requires explicit user approval** |
| `git_revert` | Undo a commit | For rollback after bad merge |

### Existing tools (unchanged)
| Tool | Purpose |
|------|---------|
| `git_status` | Branch, staged, unstaged |
| `git_diff` | Working directory diffs |
| `git_checkpoint` | Stage all + commit (now checks path allowlist for staged files) |

### Phase 2 (future)
| Tool | Purpose |
|------|---------|
| `git_push` | Push branch to remote |
| `git_pr_create` | Open pull request |

---

## Enforcement Implementation

### write_file enforcement
```
fn handle_write_file(path, content):
    if !self_modify_enabled:
        return original_write_file(path, content)
    
    tier = classify_path(path)
    match tier:
        Allow  → proceed with write
        Propose → reject with: "This path requires a proposal. 
                   Use /propose to suggest changes to kernel files."
        Deny   → reject with: "This path cannot be modified."
```

### git_checkpoint enforcement
```
fn handle_git_checkpoint():
    staged_files = git_staged_files()
    for file in staged_files:
        tier = classify_path(file)
        if tier == Deny:
            return error("Cannot commit changes to denied path: {file}")
        if tier == Propose and !user_authorized:
            return error("Kernel changes require explicit authorization: {file}")
    proceed_with_commit()
```

### Path classification

**Security requirement:** All paths are canonicalized before classification. Symlinks are resolved to their target. `..` traversal is normalized. Relative paths are resolved against the repo root. This prevents bypass via symlink-to-deny-path or traversal attacks.

```
fn classify_path(path) -> Tier:
    path = canonicalize(resolve_symlinks(path))
    // deny takes precedence
    if matches_any(path, config.deny):
        return Deny
    if matches_any(path, config.propose):
        return Propose
    if matches_any(path, config.allow):
        return Allow
    // default: deny (safe default)
    return Deny
```

Default is **deny** — unlisted paths cannot be modified. This is a whitelist model, not a blacklist.

---

## Rollback

### Automatic (Phase 1)
- If tests fail after merge, the agent can auto-run `git_revert` on the merge commit

### Automatic (Phase 2 — deferred)
- Signal analysis detects regression in subsequent sessions → proposes revert
- Requires reliable regression detection, which requires calibrated analysis. Deferred until signal analysis quality is empirically validated.

### Manual
- User runs `/revert` to undo the last self-modification
- Branch history preserved for forensics

### Pre-modification checkpoint
- Before any self-modification merge, the current state is tagged: `fawx/pre-improve/{timestamp}`
- Tags retained for 30 days (same as signal retention)
- Enables `git reset --hard fawx/pre-improve/{timestamp}` as nuclear rollback

---

## Safety Invariants

1. **self_modify.enabled defaults to false.** User must explicitly opt in.
2. **Deny tier is absolute.** No override, no escalation, no "just this once."
3. **Default classification is deny.** Unlisted paths cannot be modified.
4. **All self-modifications happen on branches.** Never on the active working branch.
5. **Tests must pass before user sees a diff** (when require_tests = true). Test scope: affected crate(s) only. User can request full suite via `/test --all`.
6. **Human always approves merges.** No auto-merge in Phase 1.
7. **Every merge creates a pre-modification tag.** Rollback is always available.
8. **Proposals cannot modify other proposals.** Prevents self-referential loops.
9. **Dirty worktree blocks self-modification.** If the user has uncommitted changes, self-modification aborts rather than risk conflicts.

---

## Audit Trail

All self-modification actions are logged to `~/.fawx/audit/self-modify.jsonl`:

```json
{"ts": 1709283600000, "action": "branch_create", "branch": "fawx/improve/search-regex-fallback", "trigger": "signal_analysis"}
{"ts": 1709283610000, "action": "write", "path": "engine/crates/fx-skills/src/search.rs", "tier": "allow", "result": "ok"}
{"ts": 1709283620000, "action": "test", "result": "pass", "duration_ms": 4500}
{"ts": 1709283630000, "action": "diff_presented", "files": ["engine/crates/fx-skills/src/search.rs"], "lines_changed": 12}
{"ts": 1709283640000, "action": "user_approved", "branch": "fawx/improve/search-regex-fallback"}
{"ts": 1709283641000, "action": "merge", "branch": "fawx/improve/search-regex-fallback", "pre_tag": "fawx/pre-improve/1709283641"}
```

---

## What This Does NOT Cover

- **WASM skill sandboxing** — separate concern, covered by #1001 Phase 2
- **A/B slot mechanics** — requires WASM skills infrastructure
- **Remote CI/CD** — Phase 2
- **Multi-agent self-modification** — future, when Fawx orchestrates sub-agents
- **Model tuning deployment** — covered by Dreaming epic #1004
