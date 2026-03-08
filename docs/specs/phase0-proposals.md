# Spec: Phase 0 PR 4 — Proposal Approval Flow

**Gap:** Proposals write to disk but no review/approve/reject UI exists  
**Estimated size:** ~250 lines  
**Risk:** Medium — new TUI commands + file manipulation

---

## Problem

The self-improvement pipeline dead-ends:
1. ✅ `/analyze` — analyzes signals, works
2. ✅ `/improve` — generates proposals, writes to `~/.fawx/proposals/`
3. ❌ No way to list pending proposals
4. ❌ No way to approve and apply a proposal
5. ❌ No way to reject a proposal

`ProposalGateState.set_active_proposal()` and `ActiveProposal` exist but are 
only used in tests.

## Design Correction (from pressure test)

**`/approve` does NOT need ProposalGateState.**

`set_active_proposal()` is designed for the *agent* — it tells the 
ProposalGateExecutor "the next agent write to path X is approved, let it 
through the gate." But `/approve` is a *human* action. The human reads the 
proposal, decides to apply it, and writes the file directly. This bypasses 
the agent's tool executor entirely — no gate involvement needed.

This simplifies the PR significantly:
- No `Arc<Mutex<ProposalGateState>>` threading through TuiApp
- No refactoring `ProposalGateExecutor::new()`  
- Just: read proposal → validate → write file → move to applied/

## Solution

### New slash commands

Add to `KNOWN_SLASH_COMMANDS`:
- `/proposals` — list pending proposals
- `/approve <id>` — approve and apply a proposal
- `/reject <id>` — reject and delete a proposal

Add to `ParsedCommand` enum:
```rust
Proposals,
Approve(String),  // proposal ID (number from list or timestamp prefix)
Reject(String),
```

### /proposals — List pending

1. Read `~/.fawx/proposals/` directory
2. Parse each `.md` file using JSON sidecar (see below)
3. Display numbered list:

```
Pending proposals:
  [1] 1710000000 — Modify kernel/loop.rs (risk: low)
  [2] 1710000100 — Add retry to network handler (risk: medium)

Use /approve <number> or /reject <number>
```

If no proposals: "No pending proposals."

### /approve <id> — Approve and apply

1. Resolve `<id>` to proposal (by number from list, or timestamp prefix)
2. Read JSON sidecar for target_path, proposed_content, file_hash
3. **Staleness check:** compute current hash of target file, compare to 
   `file_hash_at_creation` in sidecar
   - If match (or file didn't exist at creation and still doesn't): proceed
   - If mismatch: warn and require `/approve <id> --force`
     ```
     ⚠ Target file changed since proposal was created.
     Use /approve <id> --force to apply anyway.
     ```
4. **Tier 3 check:** verify target_path is NOT in TIER3_PATHS
   - If Tier 3: reject with "Cannot apply: {path} is Tier 3 (kernel immutable)"
5. Write proposed_content to target_path
6. Move proposal files (.md + .json) to `~/.fawx/proposals/applied/`
7. Print: "✓ Applied proposal: {title} → {target_path}"

### /reject <id> — Reject and archive

1. Resolve `<id>` to proposal
2. Move to `~/.fawx/proposals/rejected/` (audit trail, not hard delete)
3. Print: "✗ Rejected proposal: {title}"

### JSON sidecar (machine-readable)

`ProposalWriter::write()` currently writes only markdown. Extend it to also 
write a JSON sidecar alongside each proposal:

**Filename:** `{timestamp}-{sanitized-title}.json` (same stem as .md)

```json
{
  "version": 1,
  "timestamp": 1710000000,
  "title": "Modify kernel/loop.rs",
  "description": "Refine loop behavior",
  "target_path": "kernel/loop.rs",
  "proposed_content": "fn tick() {}",
  "risk": "low",
  "file_hash_at_creation": "sha256:abcdef1234..."
}
```

- `file_hash_at_creation`: SHA-256 of the target file at proposal time. 
  `null` if target file doesn't exist yet.
- Markdown stays human-readable. JSON is what `/approve` reads.
- Machine parsing never touches the markdown format — no fragile regex.

**Change to ProposalWriter:** The `write()` method needs the current content 
of the target file to compute the hash. Two options:
- (a) `write()` reads the file itself (adds I/O to a currently I/O-light struct)
- (b) Caller passes `Option<&[u8]>` for current file content (or hash directly)

**Preferred: option (b)** — pass `file_hash: Option<String>` to `write()`. 
The caller (ProposalGateExecutor, which already has the file path) computes 
the hash and passes it in. Keeps ProposalWriter focused on writing.

### Backward compatibility

Old proposals (written before this PR) won't have JSON sidecars. 
`/proposals` and `/approve` must handle this:
- If `.json` sidecar exists: use it
- If only `.md` exists: fall back to markdown parsing (extract title from 
  `# Proposal:` line, target from `## Proposed Diff` section)
- Staleness check skipped for legacy proposals (no hash available)

## Implementation Gate

### Gate 1: ProposalWriter signature change
Adding `file_hash: Option<String>` to `ProposalWriter::write()` changes its 
public API. Check all callers — currently only `ProposalGateExecutor` calls it. 
If other callers exist, **stop and report** before changing the signature.

## Files touched

| File | Change |
|------|--------|
| `fx-propose/src/lib.rs` | Add JSON sidecar to `write()`, accept `file_hash` param |
| `fx-cli/src/tui.rs` | Add slash commands, ParsedCommand variants, handlers |
| `fx-cli/src/proposal_review.rs` | **New** — proposal listing, parsing (JSON + markdown fallback), apply/reject |
| `fx-kernel/src/proposal_gate.rs` | Compute file hash before calling ProposalWriter::write() |
| Tests | JSON sidecar write, staleness check, approve/reject flow, legacy fallback |

## Security

- **TIER3_PATHS check on approve:** Even human approval cannot override Tier 3 
  immutability. Proposals targeting kernel paths are rejected.
- Proposals are moved, not deleted (audit trail in applied/ and rejected/)
- `/approve` writes the file directly as the human — no agent gate involvement
- File hash staleness check prevents silent overwrites of concurrent changes
- No proposal can modify TIER3_PATHS regardless of how it was created
