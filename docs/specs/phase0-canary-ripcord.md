# Spec: Phase 0 PR 2 — Canary → Ripcord Wiring

**Gap:** Signal canary and ripcord exist but aren't connected  
**Estimated size:** ~150 lines  
**Risk:** Low — standalone safety infrastructure

---

## Problem

Two safety components exist independently:
- **fx-canary** (`engine/crates/fx-canary/`): Monitors signal quality ratios 
  post-deploy. 24hr baseline, 2x spike threshold, 1hr grace period.
- **fawx-ripcord** (`fawx-ripcord/`): Standalone binary that copies last 
  known-good snapshot and restarts the process.

Neither knows the other exists. When canary detects a regression, nothing happens.

## Solution

### Canary → ripcord trigger

When canary's signal ratio exceeds the threshold:
1. Log the regression detection with details (signal type, ratio, threshold)
2. Write a pre-rollback snapshot marker (so ripcord knows what to restore)
3. Invoke ripcord binary as a subprocess
4. If ripcord binary not found → log warning, do NOT crash

### Implementation

Add a `RollbackTrigger` trait to fx-canary:

```rust
pub trait RollbackTrigger: Send + Sync {
    fn trigger_rollback(&self, reason: &RollbackReason) -> Result<(), RollbackError>;
}

pub struct RollbackReason {
    pub signal_type: String,
    pub current_ratio: f64,
    pub baseline_ratio: f64,
    pub threshold_multiplier: f64,
    pub timestamp: u64,
}
```

Default implementation `RipcordTrigger`:
```rust
pub struct RipcordTrigger {
    ripcord_path: PathBuf,  // e.g. ~/.fawx/bin/fawx-ripcord
    snapshot_dir: PathBuf,  // e.g. ~/.fawx/snapshots/
}
```

`trigger_rollback()`:
1. Write `rollback-reason.json` to snapshot dir (audit trail)
2. `Command::new(&self.ripcord_path).arg("--auto").spawn()`
3. Return Ok if spawn succeeds (ripcord handles the rest)

### Canary integration

In the canary's threshold check (wherever it runs — needs investigation):
- Accept `Option<Arc<dyn RollbackTrigger>>` 
- On threshold breach: call `trigger.trigger_rollback()` if Some
- On None: log warning only (current behavior, just louder)

### Wiring in fx-cli

In the HTTP server startup (where canary would run as background task):
- Check if ripcord binary exists at expected path
- If yes: create `RipcordTrigger`, pass to canary
- If no: pass None, log that auto-rollback is disabled

### Pre-investigation needed

Before implementation, the implementer needs to determine:
1. Where does the canary currently run? (Background task? On-demand via `/signals`?)
2. Does it run continuously or on-demand? If on-demand only, we may need to add 
   a periodic check loop.
3. Where does ripcord expect its snapshot? Need to verify the snapshot format 
   matches what ripcord reads.

## Files touched

| File | Change |
|------|--------|
| `fx-canary/src/lib.rs` | Add `RollbackTrigger` trait, `RollbackReason` |
| `fx-canary/src/trigger.rs` | **New** — `RipcordTrigger` implementation |
| `fx-cli/src/http_serve.rs` | Wire canary + trigger on startup |
| Tests | Unit test for trigger (mock ripcord binary path) |

## Security

- Ripcord binary path is hardcoded or from config — never from agent input
- Rollback reason is logged but never sent to the agent (no information leak)
- The agent cannot invoke ripcord directly — only canary can trigger it
- Tier 3: ripcord binary is in `TIER3_PATHS` (already protected from agent modification)
