# Memory Decay and Pruning

**Status:** Proposed spec  
**Issue:** #1103  
**Prerequisite for:** #1055 (self-improvement trigger)  
**Branch:** `feat/loop-resilience`

---

## Problem

Memory grows monotonically. `JsonFileMemory` has a hard `max_entries` cap (default 1000 in `JsonMemoryConfig`) but no mechanism to remove stale entries. Once full, `MemoryProvider::write()` rejects new entries with `"memory full ({max} entries max)"`. The only way to free space is explicit `delete()` calls, which the agent rarely makes.

This creates two failure modes:

1. **Memory fills up.** The agent can no longer write new memories. Learning stops.
2. **Stale memories pollute context.** `snapshot()` injects all entries into the system prompt, sorted by `access_count`. Old, never-accessed entries accumulate at the bottom but still consume context tokens. With 1000 entries at ~50 tokens each, that's ~50K tokens of memory in every prompt — potentially exceeding the context window.

Memory decay solves both: entries lose weight over time, and entries below a threshold are pruned.

---

## Design

### V1: Time-Decay (This Spec)

Each `MemoryEntry` already has `last_accessed_at_ms: u64` and `access_count: u32` (defined in `engine/crates/fx-core/src/memory.rs`). The `touch()` method in `JsonFileMemory` already updates these fields on read.

**Decay function:**

```rust
fn decayed_weight(entry: &MemoryEntry, now_ms: u64, config: &DecayConfig) -> f64 {
    let days_since_access = if entry.last_accessed_at_ms == 0 {
        // Never accessed — use created_at_ms as baseline
        (now_ms.saturating_sub(entry.created_at_ms)) as f64 / 86_400_000.0
    } else {
        (now_ms.saturating_sub(entry.last_accessed_at_ms)) as f64 / 86_400_000.0
    };
    let base_weight = entry.access_count.max(1) as f64;
    base_weight * config.decay_factor.powf(days_since_access)
}
```

- `base_weight`: `access_count` (minimum 1 so new entries aren't immediately zero).
- `decay_factor`: configurable, default `0.95`. At 0.95, an entry with `access_count=1` drops below 0.5 after ~14 days of non-access. An entry with `access_count=10` takes ~140 days.
- `prune_threshold`: configurable, default `0.1`. Entries with `decayed_weight < prune_threshold` are pruned.

**Pruning triggers:**

1. **On write, when count ≥ `max_entries`.** Before rejecting with "memory full," attempt to prune entries below threshold. If pruning frees space, the write succeeds. If not, reject as before.
2. **On session start.** A `prune()` method that callers (shells) invoke at initialization. This keeps memory lean between sessions.

**Pruning algorithm:**

```rust
fn prune(&mut self) -> usize {
    let now = now_ms();
    let before = self.data.len();
    self.data.retain(|_key, entry| {
        decayed_weight(entry, now, &self.decay_config) >= self.decay_config.prune_threshold
    });
    let pruned = before - self.data.len();
    if pruned > 0 {
        self.persist().ok(); // Best-effort persist after prune
    }
    pruned
}
```

### V2: Relevance-Decay (Future)

Joe's guidance: relevance-based decay is preferred but needs a strong heuristic. Time is a sufficient proxy for v1 because re-visited data stays fresh at nearest tree hops (the `touch()` mechanism keeps active memories alive).

V2 would add:
- Success signals reduce weight of friction signals on the same topic.
- Topic clustering via tag overlap or key-prefix matching.
- Signal-aware decay: `SignalKind::Success` on a topic decays `SignalKind::Friction` entries tagged with the same topic.

This requires the signal store (`engine/crates/fx-memory/src/signal_store.rs`) to feed back into memory decay — a cross-crate dependency that v1 avoids. Deferred to a separate spec.

### Configuration

New `DecayConfig` struct, stored alongside `JsonMemoryConfig`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecayConfig {
    /// Decay factor per day of non-access. Range: (0.0, 1.0].
    /// Lower = faster decay. Default: 0.95.
    pub decay_factor: f64,
    /// Entries with decayed weight below this are pruned. Default: 0.1.
    pub prune_threshold: f64,
    /// Maximum entries before pruning is attempted on write. Default: 1000.
    /// Replaces the hard cap behavior in JsonMemoryConfig.
    pub max_entries: usize,
}
```

Sourced from `config.toml` under a `[memory]` section:

```toml
[memory]
decay_factor = 0.95
prune_threshold = 0.1
max_entries = 1000
```

If the `[memory]` section is absent, defaults apply. The existing `JsonMemoryConfig::max_entries` continues to work as the hard cap — `DecayConfig::max_entries` mirrors it and is used by the prune-on-write trigger.

---

## Where to Change

### 1. `engine/crates/fx-memory/src/json_memory.rs` — `DecayConfig` and `decayed_weight()`

Add `DecayConfig` struct with `decay_factor`, `prune_threshold`, `max_entries` fields and defaults. Add `decayed_weight()` free function computing the decay formula.

Add `decay_config: DecayConfig` field to `JsonFileMemory` struct. Initialize from constructor parameter (with default fallback).

### 2. `engine/crates/fx-memory/src/json_memory.rs` — `prune()` method

Add `pub fn prune(&mut self) -> usize` to `JsonFileMemory`. Iterates `self.data`, computes `decayed_weight()` for each entry, removes entries below `prune_threshold`, persists, returns count of pruned entries.

### 3. `engine/crates/fx-memory/src/json_memory.rs` — `MemoryProvider::write()` modification

In the existing `write()` implementation (currently rejects when `self.data.len() >= self.config.max_entries`): before rejecting, call `self.prune()`. If pruning freed at least one slot, proceed with the write. Otherwise, reject as before.

### 4. `engine/crates/fx-memory/src/json_memory.rs` — `MemoryProvider::snapshot()` sort order

Currently sorts by `access_count` descending. Change to sort by `decayed_weight()` descending. This ensures the system prompt prioritizes recently-active memories over historically-popular but stale ones.

### 5. `engine/crates/fx-core/src/memory.rs` — no structural changes

`MemoryEntry` already has `last_accessed_at_ms`, `access_count`, `created_at_ms`. No new fields needed for v1. The `MemoryProvider` and `MemoryTouchProvider` traits are unchanged.

### 6. `engine/crates/fx-memory/src/json_memory.rs` — `JsonMemoryConfig` update

Add `decay_config: DecayConfig` field (with `#[serde(default)]`) to `JsonMemoryConfig`. Update `new_with_config()` to pass it through.

### 7. Shell / config layer (TUI, CLI entry points)

Wherever `JsonFileMemory::new()` or `new_with_config()` is called, add a `memory.prune()` call after construction for the session-start pruning trigger. The exact call sites depend on the shell (TUI creates memory in its setup). These are thin one-line additions.

---

## Test Cases

### Decay function
1. Entry with `access_count=1`, accessed today → `decayed_weight ≈ 1.0`.
2. Entry with `access_count=1`, accessed 14 days ago, `decay_factor=0.95` → `decayed_weight ≈ 0.488` (below default threshold: no, 0.488 > 0.1).
3. Entry with `access_count=1`, accessed 45 days ago, `decay_factor=0.95` → `decayed_weight ≈ 0.099` (below 0.1 threshold → pruned).
4. Entry with `access_count=10`, accessed 45 days ago → `decayed_weight ≈ 0.99` (still healthy due to high access count).
5. Entry with `last_accessed_at_ms=0` (never accessed) → falls back to `created_at_ms` for age calculation.
6. Entry created 1 second ago with `access_count=0` → weight = `1.0 * 0.95^(~0)` ≈ 1.0 (clamped `access_count.max(1)`).

### Pruning
7. Memory with 5 entries, 2 below threshold → `prune()` removes 2, returns 2.
8. Memory with 5 entries, none below threshold → `prune()` removes 0, no persist call.
9. Memory at `max_entries`, write rejected → prune frees 3 slots → write succeeds.
10. Memory at `max_entries`, prune frees 0 slots → write still rejected with "memory full" error.
11. Prune persists to disk — reload after prune shows pruned entries are gone.

### Snapshot ordering
12. Two entries: A (access_count=10, last accessed 30 days ago) and B (access_count=2, last accessed today). With decay, B's weight > A's weight → B appears first in `snapshot()`.
13. Snapshot order matches `decayed_weight` descending, with key-name tiebreaker.

### Configuration
14. `decay_factor=1.0` → no decay, entries never lose weight (opt-out).
15. `decay_factor=0.5` → aggressive decay, entries halve weight every day.
16. `prune_threshold=0.0` → nothing is ever pruned by threshold (only `max_entries` hard cap).
17. Default `DecayConfig` values match spec (0.95, 0.1, 1000).

### Edge cases
18. Empty memory → `prune()` returns 0, no panic.
19. All entries below threshold → `prune()` removes all, memory is empty.
20. System clock before epoch (returns 0) → `days_since_access` is 0, no decay applied.

---

## Scope & Estimates

| Component | Files touched | Lines (est.) | Risk |
|-----------|--------------|-------------|------|
| `DecayConfig` struct + defaults | `fx-memory/src/json_memory.rs` | ~30 | None |
| `decayed_weight()` function | `fx-memory/src/json_memory.rs` | ~15 | Low |
| `prune()` method | `fx-memory/src/json_memory.rs` | ~20 | Low |
| `write()` prune-before-reject | `fx-memory/src/json_memory.rs` | ~10 | Low |
| `snapshot()` sort by decayed weight | `fx-memory/src/json_memory.rs` | ~10 | Low |
| `JsonMemoryConfig` update | `fx-memory/src/json_memory.rs` | ~10 | None |
| Session-start `prune()` call in shells | TUI/CLI entry points | ~5 | None |
| Tests | `fx-memory/src/json_memory.rs` tests | ~250 | None |
| **Total** | | **~350** | **Low** |

No new crates. No new dependencies. All core changes within `engine/crates/fx-memory/src/json_memory.rs`. The `fx-core` memory traits are unchanged. V2 (relevance-decay) is deferred to a separate spec that will involve `fx-memory/src/signal_store.rs` cross-referencing.

---

## What This Does NOT Cover

- **Relevance-decay (v2):** Success signals reducing friction signal weight. Needs cross-crate signal→memory feedback. Deferred.
- **Tag-based pruning:** Pruning entries by tag category. Could layer on top of decay but adds complexity without clear v1 value.
- **Memory compaction/summarization:** Merging related entries into summaries instead of deleting. Different approach, possibly complementary.
- **Self-improvement trigger (#1055):** This spec is a prerequisite — decay ensures memory doesn't fill up before the self-improvement loop can write new learnings.
