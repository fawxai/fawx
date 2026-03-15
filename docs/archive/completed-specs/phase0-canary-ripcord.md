# Spec: Phase 0 PR 2 — Canary → Ripcord Wiring

**Gap:** Canary (signal quality monitor) and Ripcord (rollback binary) exist but neither runs nor connects to the other  
**Estimated size:** ~350 lines  
**Risk:** Medium — three pieces of new wiring, but all built on existing tested primitives

---

## Problem

Three safety components exist independently but none is wired:

1. **fx-canary** (`engine/crates/fx-canary/`): Pure computation — `Canary::evaluate(&[Signal]) -> Verdict`. Has `Healthy`, `Warning`, `Degraded` verdicts with rollback recommendation. 10 tests. **Never called from anywhere.**

2. **SignalCollector** (`engine/crates/fx-kernel/src/signals.rs`): Accumulates signals per loop cycle, max 200. Drained into `LoopResult` at cycle end, then cleared. **Signals vanish after each cycle — no cross-cycle accumulation.**

3. **fawx-ripcord** (`engine/crates/fawx-ripcord/`): Standalone binary. Creates/restores snapshots of `config.toml` + `skills/`. Has `--create`, `restore`, `--list`. 7 tests. **Only triggered manually.**

Result: If a skill install degrades signal quality, nobody notices and nothing rolls back.

## Solution: Three Pieces

### Piece 1: Cross-Cycle Signal Accumulator (~80 lines)

**New struct: `SignalWindow`** in `fx-canary/src/window.rs`

```rust
/// Rolling window of signals across multiple loop cycles.
/// Ring buffer with time-based expiry.
pub struct SignalWindow {
    signals: VecDeque<Signal>,
    max_signals: usize,     // hard cap (default 2000)
    max_age_secs: u64,      // time-based expiry (default 3600 = 1hr)
}

impl SignalWindow {
    pub fn new(max_signals: usize, max_age_secs: u64) -> Self;
    
    /// Ingest signals from a completed loop cycle.
    pub fn ingest(&mut self, signals: Vec<Signal>);
    
    /// Get all signals within the window (pruned on read).
    pub fn signals(&mut self) -> &[Signal];
    
    /// Prune expired signals.
    fn prune(&mut self);
}
```

- Lives in fx-canary (not fx-kernel) — canary owns its own accumulation
- `ingest()` called after every `LoopResult` is returned
- `prune()` drops signals older than `max_age_secs` on every read
- Ring buffer with hard cap prevents unbounded growth

### Piece 2: Canary Evaluation Hook (~120 lines)

**Where to call canary:** After every loop cycle completes, in the caller that processes `LoopResult`.

For HTTP mode, this is `http_serve.rs` — the `/message` handler calls the loop engine and gets back a `LoopResult` with signals.

For TUI mode, this is `TuiApp::process_message()` — same flow.

**New component: `CanaryMonitor`** in `fx-canary/src/monitor.rs`

```rust
/// Manages canary lifecycle: baseline capture, periodic evaluation, rollback trigger.
pub struct CanaryMonitor {
    canary: Canary,
    window: SignalWindow,
    trigger: Option<Arc<dyn RollbackTrigger>>,
    baseline_captured: bool,
    cycles_since_eval: u32,
    eval_interval: u32,       // evaluate every N cycles (default 10)
    min_cycles_for_baseline: u32,  // capture baseline after N cycles (default 20)
}

impl CanaryMonitor {
    pub fn new(config: CanaryConfig, trigger: Option<Arc<dyn RollbackTrigger>>) -> Self;
    
    /// Called after every loop cycle. Ingests signals, evaluates if interval reached.
    /// Returns the verdict if evaluation was performed.
    pub fn on_cycle_complete(&mut self, signals: Vec<Signal>) -> Option<Verdict>;
}
```

**Lifecycle:**
1. First N cycles: accumulate signals, no evaluation (building baseline)
2. After N cycles: capture baseline from window
3. Every M cycles thereafter: evaluate current window against baseline
4. On `Verdict::Warning`: log at WARN level with details
5. On `Verdict::Degraded { rollback_recommended: true }`: trigger rollback if trigger is Some, log at ERROR either way
6. On `Verdict::Degraded { rollback_recommended: false }`: log at ERROR, no rollback

### Piece 3: Rollback Trigger + Auto-Snapshot (~100 lines)

**`RollbackTrigger` trait** in `fx-canary/src/trigger.rs`:

```rust
pub trait RollbackTrigger: Send + Sync {
    fn trigger_rollback(&self, reason: &RollbackReason) -> Result<(), RollbackError>;
}

pub struct RollbackReason {
    pub verdict_message: String,
    pub current_success_rate: f64,
    pub baseline_success_rate: f64,
    pub timestamp_epoch_secs: u64,
}
```

**`RipcordTrigger`** implementation:

```rust
pub struct RipcordTrigger {
    ripcord_path: PathBuf,
    data_dir: PathBuf,
}

impl RollbackTrigger for RipcordTrigger {
    fn trigger_rollback(&self, reason: &RollbackReason) -> Result<(), RollbackError> {
        // 1. Write rollback-reason.json to data_dir (audit trail)
        // 2. Invoke: fawx-ripcord restore --yes
        //    - Uses Command::new with stdout/stderr captured
        //    - Does NOT use .spawn() (fire-and-forget) — we need the exit code
        // 3. If ripcord exits 0: log success, return Ok
        // 4. If ripcord fails: log error with stderr, return Err
    }
}
```

**Auto-snapshot before ripcord:** The ripcord binary already supports `--create`. But the trigger should ensure a recent snapshot exists before restoring. Add to `RipcordTrigger::trigger_rollback()`:
- Check if a snapshot exists from the last 24 hours (read manifest timestamps)
- If not, run `fawx-ripcord --create` first, then `fawx-ripcord restore --yes`
- This way we never restore to a stale snapshot

### Wiring in fx-cli (~50 lines)

**HTTP mode** (`http_serve.rs` or `main.rs`):
```rust
// During startup:
let ripcord_path = which::which("fawx-ripcord").ok()
    .or_else(|| data_dir.join("bin/fawx-ripcord").exists_then());
let trigger: Option<Arc<dyn RollbackTrigger>> = ripcord_path.map(|p| {
    Arc::new(RipcordTrigger::new(p, data_dir.clone())) as Arc<dyn RollbackTrigger>
});
let canary_monitor = Arc::new(Mutex::new(CanaryMonitor::new(
    CanaryConfig::default(),
    trigger,
)));

// After each loop cycle completes:
if let Ok(mut monitor) = canary_monitor.lock() {
    let signals = extract_signals_from_result(&result);
    if let Some(verdict) = monitor.on_cycle_complete(signals) {
        log::info!("canary verdict: {:?}", verdict);
    }
}
```

**TUI mode**: Same pattern. `CanaryMonitor` passed into `TuiApp`, called after `process_message()`.

## Implementation Gates

### Gate 1: LoopResult signal extraction
Verify that all `LoopResult` variants expose their signals in a uniform way. If extracting signals requires matching on all 5 variants, add a `LoopResult::signals(&self) -> &[Signal]` helper method. If adding that method to fx-kernel requires touching many tests, **stop and report** — we'll add it as a separate prep PR.

### Gate 2: Ripcord binary location
The trigger needs to find `fawx-ripcord`. Check: is it built as a separate binary in the workspace? Is it installed alongside `fawx`? If it's never built/installed by default (only `cargo build -p fawx-ripcord` builds it), **note this** — we need to add it to the default build targets or the trigger will always be None.

### Gate 3: Ripcord kills fawx
`fawx-ripcord restore` calls `kill_fawx()` which sends SIGKILL to the fawx process. If canary triggers ripcord from *within* the fawx process, ripcord will kill its own parent mid-restore. Verify this is safe (ripcord is a separate process, should survive parent death). If there's a race condition, **stop and report**.

## Files Touched

| File | Change |
|------|--------|
| `fx-canary/src/lib.rs` | Re-export new modules |
| `fx-canary/src/window.rs` | **New** — `SignalWindow` ring buffer |
| `fx-canary/src/monitor.rs` | **New** — `CanaryMonitor` lifecycle |
| `fx-canary/src/trigger.rs` | **New** — `RollbackTrigger` trait + `RipcordTrigger` impl |
| `fx-cli/src/main.rs` or `http_serve.rs` | Wire CanaryMonitor on startup, feed after cycles |
| `fx-cli/src/tui.rs` | Same wiring for TUI mode |
| Tests | SignalWindow (capacity, expiry, prune), CanaryMonitor (lifecycle, baseline capture, evaluation intervals), RipcordTrigger (mock binary) |

## Testing

- **SignalWindow**: capacity limits, time-based expiry, empty window behavior
- **CanaryMonitor**: baseline not captured until min_cycles, evaluation at interval, Warning/Degraded routing, None trigger logs only
- **RipcordTrigger**: mock binary (shell script that exits 0/1), reason file written, exit code handling
- **Integration**: feed N+M cycles of signals through CanaryMonitor, verify baseline captured and evaluation runs

## Security

- Ripcord binary path discovered at startup, never from agent input
- Rollback reason logged locally, never sent to agent (no information leak)
- Agent cannot invoke ripcord or canary directly — fully automatic
- `fawx-ripcord` binary is in TIER3_PATHS (protected from agent modification)
- Signal window is capped (2000 signals, 1hr) — no memory exhaustion
