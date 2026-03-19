# Spec: AX Phase 2 — Tripwire + Ripcord

**Status:** Design spec  
**Depends on:** Phase 1 (Capability Gate — #1471, merged)  
**Architecture ref:** `docs/architecture/ax-kernel-security-model.md`

---

## Overview

Tripwires are boundaries WITHIN the capability space that, when crossed, silently activate journaled monitoring. The ripcord lets the user atomically undo everything since the crossing.

The agent never knows. Zero AX impact.

---

## 1. Tripwire Definition (Config)

### Config format
```toml
[security.tripwires]
# Path-based tripwires
outside_project = { kind = "path", pattern = "!~/project/**", description = "Writes outside project directory" }
credential_read = { kind = "path", pattern = "~/.ssh/** | ~/.aws/** | ~/.gnupg/**", description = "Credential file access" }

# Action-based tripwires  
sudo_use = { kind = "action", category = "shell", pattern = "*sudo*", description = "Sudo command" }
git_push = { kind = "action", category = "git", pattern = "push", description = "Git push to remote" }

# Custom tripwires
large_delete = { kind = "action", category = "file_delete", min_count = 5, description = "Bulk file deletion" }
```

### Tripwire struct
```rust
pub struct TripwireConfig {
    pub id: String,
    pub kind: TripwireKind,
    pub description: String,
    pub enabled: bool,
}

pub enum TripwireKind {
    /// Matches file paths against glob patterns.
    Path { pattern: String },
    /// Matches tool action category + optional command pattern.
    Action { category: String, pattern: Option<String> },
    /// Fires after N actions of a category in one session.
    Threshold { category: String, min_count: u32 },
}
```

### Default tripwires (shipped with Standard preset)
1. Writes outside the configured working directory
2. Any credential file read (~/.ssh, ~/.aws, ~/.gnupg, ~/.config/gh)
3. Git push to remote
4. Bulk file deletion (5+ files in one tool call)

---

## 2. Tripwire Evaluation

### Where it hooks
In the executor chain, AFTER `PermissionGateExecutor` allows the action:

```
PermissionGateExecutor (allow/deny) → TripwireEvaluator (monitor) → ProposalGateExecutor → CachingExecutor → SkillRegistry
```

The TripwireEvaluator wraps the next executor. On every tool call:
1. Execute the tool (action happens)
2. After execution, check if any tripwire matched
3. If matched and ripcord not already active: activate ripcord, start journaling
4. If ripcord already active: journal this action
5. Return the real result to the agent (no modification)

### Why post-execution
The tripwire fires AFTER the action completes. This is deliberate:
- The agent gets the real result (invisible enforcement)
- The journal captures the actual outcome, not just the intent
- The user can review what actually happened, not what was attempted

### Matching logic
```rust
pub struct TripwireEvaluator<T: ToolExecutor> {
    inner: T,
    tripwires: Vec<TripwireConfig>,
    journal: Arc<RipcordJournal>,
    notifier: Arc<dyn TripwireNotifier>,  // async notification to user
}
```

For each completed tool call:
- Extract action metadata: tool name → category, file paths from arguments, command text
- Match against each enabled tripwire
- On first match: activate ripcord if not already active

---

## 3. Ripcord Journal

### Journal storage
```
~/.fawx/data/ripcord/
  session-{id}/
    journal.json          # Ordered list of journaled actions
    snapshots/
      {hash}.snapshot     # Before-state file snapshots
```

### Journal entry
```rust
pub struct JournalEntry {
    pub id: u64,
    pub timestamp: SystemTime,
    pub tool_name: String,
    pub tool_call_id: String,
    pub action: JournalAction,
    pub reversible: bool,
}

pub enum JournalAction {
    FileWrite {
        path: PathBuf,
        snapshot_hash: Option<String>,  // None if file was created (no prior state)
        size_bytes: u64,
    },
    FileDelete {
        path: PathBuf,
        snapshot_hash: String,  // Full content stored
    },
    FileMove {
        from: PathBuf,
        to: PathBuf,
    },
    GitCommit {
        repo: PathBuf,
        pre_ref: String,  // HEAD before commit
        commit_sha: String,
    },
    GitBranchCreate {
        repo: PathBuf,
        branch: String,
    },
    GitPush {
        repo: PathBuf,
        remote: String,
        branch: String,
        pre_ref: String,
        // Partially reversible — force push warning
    },
    ShellCommand {
        command: String,
        exit_code: i32,
        // NOT reversible — audit only
    },
    NetworkRequest {
        url: String,
        method: String,
        status: u16,
        // NOT reversible — audit only
    },
}
```

### Snapshot management
- Before a file write: read current content, hash it, store in snapshots/
- Size threshold: 10MB default (configurable). Files above threshold get hash-only tracking.
- Snapshots are deduplicated by content hash.
- On journal compaction (TTL expiry): delete snapshot files, retain journal.json as audit log.

---

## 4. Ripcord Mechanism

### Pull the ripcord
```rust
impl RipcordJournal {
    /// Atomically revert all reversible actions since the tripwire fired.
    /// Returns a report of what was reverted and what couldn't be.
    pub async fn pull(&self) -> RipcordReport {
        let entries = self.entries_since_tripwire();
        let mut reverted = Vec::new();
        let mut skipped = Vec::new();
        
        // Reverse order — newest first
        for entry in entries.iter().rev() {
            match self.revert_entry(entry).await {
                Ok(()) => reverted.push(entry.id),
                Err(reason) => skipped.push((entry.id, reason)),
            }
        }
        
        RipcordReport { reverted, skipped }
    }
}
```

### Revert operations
| Action | Revert method |
|--------|--------------|
| FileWrite (existing) | Restore from snapshot |
| FileWrite (created) | Delete file |
| FileDelete | Restore from snapshot |
| FileMove | Move back |
| GitCommit | `git reset --hard {pre_ref}` |
| GitBranchCreate | `git branch -D {branch}` |
| GitPush | Log warning + optional force-push (requires explicit flag) |
| ShellCommand | Skip (audit only) |
| NetworkRequest | Skip (audit only) |

### Ripcord TTL
- Default: end of session
- Hard cap: 24 hours
- Config: `security.ripcord.ttl = "session"` or `security.ripcord.ttl = "1h"`
- After TTL: snapshots deleted, journal.json retained as audit log

---

## 5. User Notification

### On tripwire crossing
Send async notification through the active channel:
```
🔔 Tripwire crossed: "Writes outside project directory"
   Tool: write_file → /tmp/helper.sh
   Actions since crossing are journaled. Review: /v1/ripcord/status
   Pull ripcord: /v1/ripcord/pull
```

Notification is:
- Async — does not block the agent
- Delivered through whatever channel the user is on (TUI status bar, Swift notification, Telegram message)
- Includes the tripwire description and triggering action

### API endpoints
```
GET  /v1/ripcord/status    → { active: bool, tripwire: string, entries: [...], since: timestamp }
POST /v1/ripcord/pull      → { reverted: [...], skipped: [...] }
POST /v1/ripcord/approve   → { cleared: true }  (dismiss the ripcord, keep changes)
GET  /v1/ripcord/journal   → { entries: [...] }  (full audit log)
```

---

## 6. Fleet Cascade (v1)

When agent A's ripcord is pulled:
1. Identify all downstream agents that consumed A's output (orchestrator tracks this)
2. Pull their ripcords too (full cascade)
3. Report includes per-agent revert status

Selective cascade (only A-dependent actions) is deferred to v2.

---

## 7. Crate Structure

### Option A: Extend fx-journal
The journal crate already exists. Add ripcord journaling as a new module.
- Pro: reuses existing persistence infrastructure
- Con: fx-journal is currently reflection/memory focused, not security focused

### Option B: New fx-ripcord crate
Dedicated crate for tripwire evaluation + ripcord journaling.
- Pro: clean separation of concerns
- Con: another crate to maintain

**Recommendation:** Option B. Security journaling has different requirements (atomic rollback, snapshot management, TTL) than reflective memory journaling. Keep them separate.

### New crate: fx-ripcord
```
engine/crates/fx-ripcord/
  src/
    lib.rs              # Public API
    tripwire.rs         # TripwireConfig, matching logic
    journal.rs          # RipcordJournal, JournalEntry, JournalAction
    snapshot.rs         # File snapshot management
    revert.rs           # Revert operations
    evaluator.rs        # TripwireEvaluator (ToolExecutor wrapper)
    report.rs           # RipcordReport
  Cargo.toml
```

---

## 8. Test Plan

### Unit tests (fx-ripcord)
- Tripwire matching: path globs, action categories, thresholds
- Journal: append, read, TTL compaction
- Snapshot: store, retrieve, size threshold, dedup
- Revert: file write, file delete, file move, git commit, git branch
- Revert skips: shell commands, network requests
- Ripcord pull: correct reverse order, partial success report
- Journal entry serialization round-trip

### Integration tests
- Full flow: tool call → tripwire match → journal → ripcord pull → state restored
- Multiple tripwires: only first activates ripcord
- Ripcord already active: new entries append to existing journal
- TTL: journal compacts after expiry, audit log retained
- Large file: hash-only tracking above threshold

### TUI smoke test
- Tripwire notification appears in status bar
- Ripcord pull reverts visible changes
