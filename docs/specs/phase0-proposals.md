# Spec: Phase 0 PR 4 — Proposal Approval Flow

**Gap:** Proposals write to disk but no review/approve/reject UI exists  
**Estimated size:** ~300 lines  
**Risk:** Medium — new TUI commands + file manipulation + state mutation

---

## Problem

The self-improvement pipeline dead-ends:
1. ✅ `/analyze` — analyzes signals, works
2. ✅ `/improve` — generates proposals, writes to `~/.fawx/proposals/`
3. ❌ No way to list pending proposals
4. ❌ No way to approve (calls `set_active_proposal()`)
5. ❌ No way to reject (delete proposal)
6. ❌ No way to apply (write the proposed content to the target file)

`ProposalGateState.set_active_proposal()` and `ActiveProposal` exist but are 
only used in tests.

## Solution

### New slash commands

Add to `KNOWN_SLASH_COMMANDS`:
- `/proposals` — list pending proposals
- `/approve <id>` — approve and apply a proposal
- `/reject <id>` — reject and delete a proposal

Add to `ParsedCommand` enum:
```rust
Proposals,
Approve(String),  // proposal ID (timestamp prefix or filename)
Reject(String),
```

### /proposals — List pending

1. Read `~/.fawx/proposals/` directory
2. Parse each `.md` file: extract title, target path, risk, timestamp
3. Display numbered list:

```
Pending proposals:
  [1] 1710000000 — Modify kernel/loop.rs (risk: low)
  [2] 1710000100 — Add retry to network handler (risk: medium)

Use /approve <number> or /reject <number>
```

If no proposals: "No pending proposals."

### /approve <id> — Approve and apply

1. Resolve `<id>` to proposal file (by number from list, or by timestamp prefix)
2. Parse proposal markdown → extract target_path + proposed_content
3. Validate target path:
   - NOT in TIER3_PATHS (compiled kernel invariant — cannot be overridden even by approval)
   - Path exists (or creation is expected)
   - Path is within working directory
4. Create `ActiveProposal`:
   ```rust
   ActiveProposal {
       paths: vec![target_path.clone()],
       approved_at: current_time_ms(),
       expires_at: Some(current_time_ms() + 300_000), // 5 minute window
   }
   ```
5. Call `proposal_gate_state.set_active_proposal(active_proposal)`
6. Write proposed content to target file
7. Clear active proposal
8. Move proposal file to `~/.fawx/proposals/applied/` (audit trail)
9. Print: "✓ Applied proposal: {title} → {target_path}"

### /reject <id> — Reject and delete

1. Resolve `<id>` to proposal file
2. Move to `~/.fawx/proposals/rejected/` (audit trail, not hard delete)
3. Print: "✗ Rejected proposal: {title}"

### Proposal parsing

Add to a new module `engine/crates/fx-cli/src/proposal_review.rs`:

```rust
pub struct ParsedProposal {
    pub filename: String,
    pub timestamp: u64,
    pub title: String,
    pub target_path: PathBuf,
    pub proposed_content: String,
    pub risk: String,
}

pub fn parse_proposal_file(path: &Path) -> Result<ParsedProposal, String>;
pub fn list_pending_proposals(proposals_dir: &Path) -> Vec<ParsedProposal>;
```

Parse the markdown format that `ProposalWriter::format_proposal()` generates:
```markdown
# Proposal: {title}

## What and Why
{description}

## Proposed Diff
{target_path}:
```
{content}
```

## Risk
{risk}
```

### Access to ProposalGateState

`TuiApp` needs access to the `ProposalGateState` for `set_active_proposal()`.
Currently, `ProposalGateExecutor` wraps the state in a `Mutex<ProposalGateState>`.

Options:
1. **Preferred:** Store `Arc<Mutex<ProposalGateState>>` separately, share between 
   `ProposalGateExecutor` and `TuiApp`
2. Add a method to `ProposalGateExecutor` that exposes `set_active_proposal()`

Option 1 requires refactoring `ProposalGateExecutor::new()` to accept 
`Arc<Mutex<ProposalGateState>>` instead of owned `ProposalGateState`. This is 
the cleaner approach — the state is shared, not hidden.

## Files touched

| File | Change |
|------|--------|
| `tui.rs` | Add slash commands, ParsedCommand variants, handlers |
| `proposal_review.rs` | **New** — proposal parsing + listing |
| `fx-kernel/src/proposal_gate.rs` | Refactor to accept `Arc<Mutex<ProposalGateState>>` |
| Tests | Parse proposal markdown, list proposals, approve/reject flow |

## Security

- **TIER3_PATHS check on approve:** Even human approval cannot override Tier 3 
  immutability. If a proposal targets a Tier 3 path, `/approve` must reject it 
  with a clear message: "Cannot apply: {path} is in Tier 3 (kernel immutable)."
- Proposals are moved, not deleted (audit trail in applied/ and rejected/)
- Active proposal has a 5-minute TTL — auto-expires if not used
- Proposal content is applied as a file write — same security as `write_file` tool
- No proposal can modify TIER3_PATHS regardless of how it was created
