# Spec: Phase 2 — Tiered Compaction Strategy

**Status:** Ready for implementation  
**Author:** Clawdio  
**Date:** 2026-03-21  
**Crate:** `fx-kernel`  
**Files:** `engine/crates/fx-kernel/src/conversation_compactor.rs`, `engine/crates/fx-kernel/src/loop_engine.rs`  
**Parent spec:** `docs/specs/long-session-context-management.md`

---

## Problem

The current compaction system has a single threshold (`compaction_threshold`, default 0.80). When token usage crosses this threshold, it runs a single strategy (sliding window or summarization). This is binary: either nothing happens, or an aggressive compaction fires.

Long-running sessions need graduated responses:
1. **Light cleanup** when context starts getting large (prune tool blocks)
2. **Moderate compaction** when cleanup wasn't enough (slide out oldest messages)
3. **Aggressive compaction** when sliding wasn't enough (summarize evicted messages via LLM)
4. **Emergency compaction** when everything else failed (hard drop, keep only system + last N turns)

The current system also has no emergency fallback. If `AllMessagesProtected` fires at the hard limit, the session errors out with `ContextExceededAfterCompaction`.

---

## Design

### Tier Thresholds

Replace the single `compaction_threshold` with four graduated tiers on `CompactionConfig`:

| Field | Default | Behavior |
|-------|---------|----------|
| `prune_threshold` | 0.40 | Run tool block pruning (Layer 1, already exists) |
| `slide_threshold` | 0.60 | Run sliding window compaction (drop oldest middle messages) |
| `summarize_threshold` | 0.80 | Run summarizing compaction (LLM call to summarize evicted messages) |
| `emergency_threshold` | 0.95 | Emergency: hard-drop all middle messages, keep system prefix + last `preserve_recent_turns` |

The existing `compaction_threshold` field is retired. For backward compatibility, if the config TOML still contains `compaction_threshold`, map it to `slide_threshold` and use defaults for the others.

### Tier Execution Logic

Replace the current `compact_if_needed` flow with a tiered approach:

```
fn compact_if_needed(messages, scope, iteration):
    usage_ratio = estimate_tokens(messages) / conversation_budget()
    
    // Tier 1: Prune (already exists, move threshold check here)
    if usage_ratio >= prune_threshold:
        messages = maybe_prune_tool_blocks(messages)
        usage_ratio = recalculate()
    
    // Tier 2: Slide
    if usage_ratio >= slide_threshold:
        messages = sliding_window_compact(messages, target=budget * 0.5)
        usage_ratio = recalculate()
    
    // Tier 3: Summarize  
    if usage_ratio >= summarize_threshold:
        messages = summarize_compact(messages, target=budget * 0.4)
        usage_ratio = recalculate()
    
    // Tier 4: Emergency
    if usage_ratio >= emergency_threshold:
        messages = emergency_compact(messages)
    
    return messages
```

Each tier runs only if the previous tier didn't bring context below the next tier's threshold. The tiers are **cumulative**: if at 0.85, Tier 1 runs, then if still above 0.60, Tier 2 runs, then if still above 0.80, Tier 3 runs.

### Emergency Compaction

New function `emergency_compact`:
- Keeps all system-prefix messages (unchanged)
- Keeps last `preserve_recent_turns` messages (unchanged)
- **Drops everything in between** — no protected-middle logic, no tool chain preservation
- Inserts a compaction marker: `[context compacted: emergency — N messages removed]`
- Flushes evicted messages to journal (if flush available)
- Always succeeds (cannot return `AllMessagesProtected`)

This is the safety net. If a session is at 95% capacity after all other compaction, it's better to lose context than to error out.

### Cooldown Changes

The existing cooldown (`recompact_cooldown_turns`) applies per-scope to the full compaction pipeline. For tiered compaction:
- **Prune** (Tier 1): No cooldown. It's cheap and idempotent (already-pruned blocks stay pruned).
- **Slide** (Tier 2): Apply cooldown.
- **Summarize** (Tier 3): Apply cooldown (expensive LLM call).
- **Emergency** (Tier 4): **No cooldown** — if we're at 95%, we must act immediately regardless.

### Config Changes

```rust
pub struct CompactionConfig {
    // Existing fields:
    pub preserve_recent_turns: usize,
    pub model_context_limit: usize,
    pub reserved_system_tokens: usize,
    pub recompact_cooldown_turns: u32,
    pub use_summarization: bool,
    pub max_summary_tokens: usize,
    pub prune_tool_blocks: bool,
    pub tool_block_summary_max_chars: usize,
    
    // REMOVED: compaction_threshold (replaced by tiered thresholds)
    
    // NEW: tiered thresholds
    pub prune_threshold: f32,       // default 0.40
    pub slide_threshold: f32,       // default 0.60
    pub summarize_threshold: f32,   // default 0.80
    pub emergency_threshold: f32,   // default 0.95
}
```

**Backward compatibility:** Keep `compaction_threshold` as a `serde(alias)` on `slide_threshold`. If only `compaction_threshold` is present in TOML, it populates `slide_threshold`. Old configs work without changes.

### Validation

Add to `CompactionConfig::validate()`:
- `prune_threshold` must be in (0.0, 1.0]
- `slide_threshold` must be > `prune_threshold`
- `summarize_threshold` must be > `slide_threshold`
- `emergency_threshold` must be > `summarize_threshold`
- `emergency_threshold` must be <= 1.0

New error variant:
```rust
CompactionConfigError::ThresholdsNotMonotonic { 
    prune: f32, slide: f32, summarize: f32, emergency: f32 
}
```

### ConversationBudget Changes

Add helper methods:
```rust
impl ConversationBudget {
    pub fn usage_ratio(&self, messages: &[Message]) -> f32 {
        Self::estimate_tokens(messages) as f32 / self.conversation_budget() as f32
    }
    
    pub fn at_tier(&self, messages: &[Message], threshold: f32) -> bool {
        self.usage_ratio(messages) >= threshold
    }
}
```

Retire `needs_compaction()` — replace call sites with `at_tier(messages, config.slide_threshold)` or the tiered flow. (Or keep it as a convenience that delegates to `at_tier` with `slide_threshold`.)

The existing `compaction_target()` method (returns `budget * 3/5`) is used as the target for sliding compaction. For the tiered system, each tier can specify its own target:
- Slide target: `budget * 0.50` (aim for well below slide_threshold)
- Summarize target: `budget * 0.40` (aim for well below summarize_threshold)
- Emergency: drops to whatever system + recent turns costs (no target, just keep minimum)

### Logging

Each tier logs entry/exit with:
- `tier` name (prune/slide/summarize/emergency)
- `before_tokens`, `after_tokens`
- `usage_ratio_before`, `usage_ratio_after`
- `scope` (perceive/tool_continuation/decompose_child)

---

## Implementation Details

### Changes to `conversation_compactor.rs`

1. Add `emergency_compact` function:
```rust
pub fn emergency_compact(
    messages: &[Message],
    preserve_recent_turns: usize,
) -> CompactionResult {
    let bounds = zone_bounds(messages, preserve_recent_turns);
    let evicted_indices: Vec<usize> = (bounds.prefix_end..bounds.tail_start).collect();
    let compacted_count = evicted_indices.len();
    
    let mut result = Vec::new();
    result.extend_from_slice(&messages[..bounds.prefix_end]);
    if compacted_count > 0 {
        result.push(compaction_marker_message_emergency(compacted_count));
    }
    result.extend_from_slice(&messages[bounds.tail_start..]);
    
    CompactionResult {
        estimated_tokens: ConversationBudget::estimate_tokens(&result),
        messages: result,
        compacted_count,
        used_summarization: false,
        evicted_indices,
    }
}
```

2. Add `usage_ratio` and `at_tier` to `ConversationBudget`.

3. Update `CompactionConfig` fields and defaults.

4. Update validation.

5. Add emergency compaction marker variant.

### Changes to `loop_engine.rs`

1. Replace the current `compact_if_needed` body with the tiered flow.

2. The cooldown check moves to apply only to Tier 2 (slide) and Tier 3 (summarize). Prune and emergency bypass cooldown.

3. Update all `needs_compaction` call sites to use `at_tier` or the tiered flow. The one in `append_compacted_summary` and `ensure_within_hard_limit` should keep checking against the hard limit (100%).

4. Journal flush runs for all tiers that evict messages (slide, summarize, emergency).

---

## Tests

### Unit tests in `conversation_compactor.rs`

1. **`emergency_compact_drops_all_middle`**: Verify that emergency compaction keeps only system prefix and recent turns, drops everything in between.

2. **`emergency_compact_preserves_system_prefix`**: System messages at the start survive emergency compaction.

3. **`emergency_compact_preserves_recent_turns`**: Last N messages survive.

4. **`emergency_compact_inserts_marker`**: Result contains emergency compaction marker.

5. **`emergency_compact_populates_evicted_indices`**: All middle indices in evicted list.

6. **`emergency_compact_empty_middle_is_noop`**: When all messages are in prefix + tail, nothing is evicted.

7. **`usage_ratio_correct`**: `ConversationBudget::usage_ratio` returns correct fraction.

8. **`at_tier_detects_threshold_crossing`**: `at_tier` returns true at/above threshold, false below.

9. **`config_validation_rejects_non_monotonic_thresholds`**: Thresholds must be strictly increasing.

10. **`config_validation_accepts_valid_thresholds`**: Default thresholds pass validation.

11. **`backward_compat_compaction_threshold_maps_to_slide`**: Old `compaction_threshold` field populates `slide_threshold`.

### Integration tests in `loop_engine.rs`

12. **`tiered_compaction_prune_only`**: At 45% usage with tool blocks, only pruning fires; no sliding or summarization.

13. **`tiered_compaction_slide_when_prune_insufficient`**: At 65% usage, pruning fires first, then sliding.

14. **`tiered_compaction_summarize_when_slide_insufficient`**: At 85% usage, all three tiers fire in sequence.

15. **`tiered_compaction_emergency_fires_at_95_percent`**: At 96% usage, emergency compaction fires and always succeeds.

16. **`emergency_bypasses_cooldown`**: Even during cooldown, emergency compaction runs.

17. **`cooldown_skips_slide_and_summarize`**: During cooldown period, Tier 2 and 3 are skipped (but Tier 1 and 4 still run).

18. **`tier_transitions_logged`**: Verify tracing output includes tier name, before/after tokens.

---

## Migration

Existing `config.toml` files with `compaction_threshold = 0.80` will seamlessly map to `slide_threshold = 0.80`. The new `prune_threshold`, `summarize_threshold`, and `emergency_threshold` fields get defaults. No breaking changes.

---

## Non-goals

- This spec does NOT implement session memory extraction (Phase 3).
- This spec does NOT implement recall tool (Phase 4).
- This spec does NOT change the summarization prompt (future improvement).
- This spec does NOT add per-tier target configuration to TOML (hardcoded ratios for now).
