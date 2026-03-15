# Phase 2a: Proposals UX Improvement

*From issue #1281. Joe's smoke test revealed broken drill-down flow.*

## Summary

Improve the `/proposals`, `/approve`, and `/reject` command UX. Current issues: hard-to-scan output, no diff preview, broken numbering/mapping, and Fawx can't access `~/.fawx/proposals/` in some modes.

## Current Problems

From Joe's smoke test (screenshot in #1281):
1. **Proposal numbering**: `/proposals` shows numbered list but `/approve <number>` doesn't map correctly
2. **No diff preview**: Must read the full proposal file to understand what it changes
3. **No age/staleness indicator**: Old proposals sit forever
4. **File access**: Fawx agent can't read `~/.fawx/proposals/` directly in some configurations
5. **Format is dense**: Multiple proposals are hard to scan

## Design

### `/proposals` — List View

Current:
```
Pending proposals:
1. src/main.rs - modify error handling
2. src/config.rs - add new field
```

Proposed:
```
📋 Pending Proposals (2)

 #1  src/main.rs                  2h ago
     ╰─ modify error handling
     ╰─ +12 / -3 lines

 #2  src/config.rs               15m ago
     ╰─ add new field
     ╰─ +8 / -0 lines

Use /proposals <id> for details · /approve <id> · /reject <id>
```

Key changes:
- **Stable IDs**: Use the proposal filename hash (first 6 chars), not list position
- **Age display**: Relative time since proposal creation
- **Line diff summary**: +/- line counts from the diff
- **Hints**: Show available actions at the bottom

### `/proposals <id>` — Detail View

```
📋 Proposal #a1b2c3 — src/main.rs

Created: 2h ago (2026-03-09 16:30 UTC)
Target:  src/main.rs
Reason:  modify error handling for config load failure

─── Diff ───
- fn load_config() -> Config {
-     let raw = fs::read_to_string("config.toml").unwrap();
+ fn load_config() -> Result<Config, ConfigError> {
+     let raw = fs::read_to_string("config.toml")
+         .map_err(|e| ConfigError::ReadFailed(e))?;

+12 / -3 lines

/approve a1b2c3 · /reject a1b2c3
```

### `/approve <id>` — Apply

```
✅ Applied proposal #a1b2c3
   src/main.rs — +12 / -3 lines
   Proposal file removed.
```

Behavior unchanged — still applies the diff. UX improvements:
- Confirmation message with what was applied
- Auto-cleanup of the proposal file
- Error message if proposal no longer applies cleanly (file changed since proposal)

### `/reject <id>` — Reject

```
❌ Rejected proposal #a1b2c3
   src/main.rs — proposal file removed.
```

### Stable Proposal IDs

**Problem**: Using list position (1, 2, 3) means IDs change when proposals are added/removed between `/proposals` and `/approve`. Race condition.

**Fix**: Derive ID from the proposal filename:
```rust
fn proposal_id(filename: &str) -> String {
    // Use first 6 chars of the filename (already unique per proposal)
    filename.chars().take(6).collect()
}
```

Or use a short hash of the proposal content. Either way: stable, doesn't change with list reordering.

### Proposal File Access

**Problem**: In embedded mode, the TUI runs in the engine process and can access `~/.fawx/proposals/`. In HTTP mode, the TUI is a client — it calls the server, which accesses the proposals.

**Current**: `/proposals` is handled server-side in `commands/slash.rs` via `CommandHost::proposals()`. The server reads the proposals directory. This should work in both modes.

**Bug investigation**: The smoke test showed broken mapping. Root cause is likely in how `proposals()` returns data and how `parse_approve_command()` resolves the selector. Trace the path:
1. `CommandHost::proposals()` → reads `~/.fawx/proposals/` → formats as string
2. User types `/approve 1`
3. `parse_approve_command()` → extracts "1" as selector
4. `CommandHost::approve("1", false)` → needs to map "1" back to a filename

The mapping from display number → filename is the bug. If `proposals()` returns a formatted string, the number→filename mapping is lost.

**Fix**: Store the proposal list state so `/approve <n>` can resolve to the correct file, OR switch to stable IDs that appear in both the list and the approve command.

## Implementation

### Files to Modify

1. **`engine/crates/fx-cli/src/commands/slash.rs`**
   - Update `ParsedCommand::Proposals` to optionally take an ID (detail view)
   - `parse_proposals_command()` — parse optional `<id>` argument
   - `execute_proposals()` — format list view or detail view
   - `execute_approve()` — resolve stable ID to filename
   - `execute_reject()` — resolve stable ID to filename

2. **`engine/crates/fx-tools/src/tools.rs`** (or wherever proposals are read)
   - Add proposal metadata: age, line counts, stable ID generation
   - `ProposalInfo` struct: `id`, `filename`, `target_path`, `summary`, `age`, `lines_added`, `lines_removed`, `diff_preview`
   - `list_proposals() -> Vec<ProposalInfo>`
   - `get_proposal(id: &str) -> Option<ProposalInfo>` with full diff

3. **`engine/crates/fx-kernel/src/proposal_gate.rs`**
   - Proposal file format may need a header with metadata (creation time, reason)
   - Or extract metadata from file modification time + content parsing

4. **`CommandHost` trait** (`commands/slash.rs`)
   - `proposals()` → `list_proposals() -> Vec<ProposalInfo>` (structured, not pre-formatted string)
   - `approve(id: &str, force: bool)` — accept stable ID
   - `reject(id: &str)` — accept stable ID (new method or overload approve with reject flag)

### Tests Required

- List view format matches spec (with ages, line counts)
- Detail view shows diff
- Stable ID resolves correctly across list/approve/reject
- ID mismatch returns clear error
- Empty proposals list shows helpful message
- Age display: "just now", "5m ago", "2h ago", "3d ago"
- Line count calculation from diff content
- Approve with stale proposal (file changed) returns error
- Reject removes file
- Works in both HTTP and embedded mode

## Size Estimate

~200-250 lines of implementation + ~150 lines of tests. Single PR.

## Dependencies

None.
