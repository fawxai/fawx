# Spec: Summarize-Before-Slide Compaction

**Status:** Draft
**Author:** Clawdio (architectural analysis), Joe (bug report + Fawx self-diagnosis)
**Date:** 2026-03-25
**Crates:** `fx-kernel` (conversation_compactor, loop_engine), `fx-journal` (flush)

---

## Problem

When Fawx's context window fills during a tool-heavy session, the sliding window compactor (Tier 2, fires at 60% usage) deletes the oldest messages and leaves behind a marker:

```
[context compacted: 42 older messages removed]
```

This marker tells the agent how many messages were lost but nothing about what they contained. The agent loses all decisions, file paths, reasoning, and user requests from those messages. In a real session, this caused the agent to forget a multi-turn conversation and hallucinate that the user's first message was empty.

### Why the current tier cascade fails

```
  0-40%: nothing
   40%: Tier 1 — prune (strip old tool blocks, lossless)
   60%: Tier 2 — SLIDE (lossy delete, count-only marker)
   80%: Tier 3 — SUMMARIZE (LLM summary, rich marker)
   95%: Tier 4 — EMERGENCY (aggressive delete)
```

- Sliding fires first (60%), deleting messages with no summary.
- Summarization fires only at 80%, which rarely happens because sliding already reduced usage.
- In practice, most compaction events are pure lossy deletion.

### Secondary failures

1. **Journal flush is truncated:** Pre-eviction flush writes to journal entries capped at 4KB total / 500B per tool result. Tool-heavy sessions hit this cap immediately. The richest context gets the worst preservation.

2. **Session memory extraction is a separate LLM call** on raw evicted messages (tool results truncated to 200 chars). It produces structured memory but is capped at 2,000 tokens / 20 items. Fills up after a few compaction rounds, then new extractions are silently rejected.

3. **Recall is opt-in:** Flushed journal entries are only searchable via `recall_session_context`. The agent must explicitly call it with a query. No automatic re-injection after compaction.

---

## Solution: Summarize-before-slide

Make summarization a **pre-step** of sliding eviction, not a separate tier. Before deleting ANY messages, first extract what they contain.

### Architecture

One LLM call produces a summary that feeds three outputs:

```
Evictable messages
       │
       ▼
  LLM Summarizer
       │
       ├──▶ In-context summary marker (replaces count-only marker)
       ├──▶ Journal entry (replaces truncated raw dump)
       └──▶ Session memory extraction input (replaces second LLM call)
```

### Tier cascade (new)

```
  0-40%: nothing
   40%: Tier 1 — prune (strip old tool blocks, lossless)  [unchanged]
   60%: Tier 2 — SUMMARIZE-THEN-SLIDE
                  Step 1: Identify evictable messages (same zone logic as current slide)
                  Step 2: LLM summarizes evictable messages
                  Step 3: Replace evicted messages with summary marker
                  Step 4: If still over target, slide remaining (rare — summary is compact)
   80%: [removed as separate tier — summarization now built into slide]
   95%: Tier 4 — EMERGENCY (aggressive delete)  [unchanged]
```

The threshold ordering becomes: `prune (0.40) < slide (0.60) < emergency (0.95)`. The `summarize_threshold` field is removed; summarization is always attempted before sliding.

When `use_summarization: false`, `compaction_llm` is None, or the LLM call fails, behavior falls back to current sliding (lossy delete + count marker). This preserves backward compat and handles offline/no-LLM/BYOK-without-compaction scenarios.

### Config changes

```rust
pub struct CompactionConfig {
    pub(crate) prune_threshold: f32,       // 0.40 (unchanged)
    pub(crate) slide_threshold: f32,       // 0.60 (unchanged)
    // summarize_threshold: REMOVED
    pub(crate) emergency_threshold: f32,   // 0.95 (unchanged)
    pub(crate) use_summarization: bool,    // true (unchanged, now gates slide pre-step)
    pub(crate) max_summary_tokens: usize,  // 1024 (unchanged)
    // ... rest unchanged
}
```

Validation: `prune < slide < emergency` (no more 4-threshold monotonicity).

Backward compat: Old configs with `summarize_threshold` are accepted via `#[serde(default)]` and ignored. `use_summarization` keeps its meaning: controls whether the LLM summary pre-step runs.

### CompactionTier enum

```rust
enum CompactionTier {
    Prune,      // unchanged
    Slide,      // now includes summarize pre-step
    Emergency,  // unchanged
}
// Summarize variant removed
```

### `highest_compaction_tier` (simplified)

```rust
fn highest_compaction_tier(&self, messages: &[Message]) -> Option<CompactionTier> {
    if self.conversation_budget.at_tier(messages, self.compaction_config.emergency_threshold) {
        return Some(CompactionTier::Emergency);
    }
    if self.conversation_budget.at_tier(messages, self.compaction_config.slide_threshold) {
        return Some(CompactionTier::Slide);
    }
    None
}
```

### `apply_slide_tier` (revised)

```rust
async fn apply_slide_tier(&self, current, scope, iteration) -> Result<...> {
    let target = self.conversation_budget.compaction_target();
    let bounds = zone_bounds(current, preserve_recent_turns);
    let evictable = identify_evictable_messages(current, &bounds);

    // Gate: summarization requires both config flag AND available LLM
    let can_summarize = self.compaction_config.use_summarization
        && self.compaction_llm.is_some();

    // Step 1: Try to summarize evictable messages
    let summary = if can_summarize {
        match self.generate_eviction_summary(&evictable).await {
            Ok(summary) => Some(summary),
            Err(err) => {
                tracing::warn!(error = %err, "pre-slide summarization failed; falling back to lossy slide");
                None
            }
        }
    } else {
        None
    };

    // Step 2: Build compacted messages with summary marker
    let result = if let Some(ref summary) = summary {
        assemble_summarized_slide(current, &bounds, summary, &evictable)
    } else {
        // Fallback: lossy slide with count marker (current behavior)
        self.run_sliding_compaction(current, scope, target).await?
    };

    // Step 3: Flush summary to journal (not raw truncated messages)
    if let Some(ref summary) = summary {
        self.flush_summary_to_journal(summary, &evictable, scope).await;
    }

    // Step 4: Extract session memory from summary (no second LLM call)
    if let Some(ref summary) = summary {
        self.extract_memory_from_summary(summary).await;
    }

    // Step 5: If still over target after summary replacement, slide the excess
    // This produces ONLY a count marker for the additionally-removed messages.
    // The summary marker from Step 2 is treated as a system-like message and
    // protected from further eviction (message_is_system_like returns true
    // for summary markers).
    if ConversationBudget::estimate_tokens(&result.messages) > target {
        self.run_sliding_compaction(&result.messages, scope, target).await?
    } else {
        Ok(result)
    }
}
```

### Dual-marker composition

When the summary is too large and a fallback slide fires (Step 5), the result contains both markers:

```
[system prompt]
[context summary]                    ← from Step 2 (protected, not evictable)
Decisions: ...
Files modified: ...
[context compacted: 8 messages removed]  ← from Step 5 (additional slide)
[recent messages...]
```

The summary marker is protected because `message_is_system_like()` already returns true for messages starting with `[context summary]` (it checks for `SUMMARY_MARKER_PREFIX`). This means the summary survives the follow-up slide. The count marker only covers the additional messages removed by Step 5, not the already-summarized ones.

This is the correct behavior: the summary captures the important context, and the additional slide only removes overflow that didn't fit. The user loses some detail but retains the structured summary.

### In-context marker (new)

Before:
```
[context compacted: 42 older messages removed]
```

After:
```
[context summary]
Decisions: User requested turboquant.py for parameter optimization. Agreed on scipy.optimize approach.
Files modified: src/optimizer.py (created), tests/test_optimizer.py (created)
Task state: Implementation complete, user said "Yes, proceed" to run tests.
Key context: Working in ~/parameter-golf repo. Python 3.11 environment.
```

The marker uses the existing `summary_message()` function (already implemented for the summarizing compactor). The prompt is the existing `SummarizingCompactor::summary_prompt()`.

---

## Journal flush changes

### Current (`fx-journal/src/flush.rs`)

Writes raw truncated messages:
- 4KB total cap
- 500B per tool result
- Images → `[image]`, documents → `[document]`

### New

When a summary was generated, the journal flush writes the summary text instead of raw messages. The summary is already structured (Decisions / Files / Task state / Key context) and fits well under 4KB.

When no summary is available (fallback path), current behavior is preserved.

```rust
impl CompactionMemoryFlush for JournalCompactionFlush {
    async fn flush(&self, evicted: &[Message], scope: &str) -> Result<()> {
        // Called with either raw messages (fallback) or a single summary message
        // Implementation unchanged — just receives better input
    }
}
```

The change is in the caller (`flush_evicted` in loop_engine), not in the journal flush trait itself.

---

## Session memory changes

### Current

`extract_memory_from_evicted` makes a SECOND LLM call on raw evicted messages (tool results at 200 chars) to extract structured memory:
```json
{
    "project": "turboquant optimizer",
    "current_state": "implementation complete",
    "key_decisions": ["use scipy.optimize", "Python 3.11"],
    "active_files": ["src/optimizer.py"],
    "custom_context": ["working in ~/parameter-golf"]
}
```

### New

`extract_memory_from_summary` parses the already-generated summary text to extract the same structured fields. No second LLM call needed. The summary prompt already produces the same sections (Decisions, Files modified, Task state, Key context).

If parsing fails, falls back to the current LLM extraction call.

### Cap increase

- Token cap: 2,000 → 4,000 tokens
- Item cap per list: 20 → 40 items

These are proportional to model context sizes. A 200K context session generates more facts worth remembering than a 32K session. Future: make caps proportional to `model_context_limit`.

### `preserve_recent_turns` increase

- Default: 6 → 12

In tool-heavy sessions, a single multi-tool turn can consume 3-4 message slots (assistant with tool_use, tool result, assistant response, etc.). With `preserve_recent_turns: 6`, only 1-2 complete conversation rounds are protected. At 12, we protect 3-4 complete rounds, which gives the agent enough recent context to maintain coherence.

This is configurable, so users with smaller context windows can reduce it.

---

## Scope and non-goals

### In scope
- [x] Summarize-before-slide architecture in `compact_if_needed` / `apply_slide_tier`
- [x] Summary-as-marker (replace count marker with LLM summary)
- [x] Journal flush receives summary instead of raw truncated messages
- [x] Session memory extraction from summary (eliminate second LLM call)
- [x] Remove `CompactionTier::Summarize` and `summarize_threshold`
- [x] Raise session memory caps
- [x] Backward-compatible config handling

### Not in scope (future)
- Auto-recall when summary markers are themselves evicted (multi-round compaction)
- Dynamic session memory caps proportional to model context
- Streaming summarization (generate summary while still processing tool calls)
- Compaction-aware tool retry (avoid re-running tools whose results were just evicted)
- Dedicated lightweight compaction model (currently uses `compaction_llm`, which is typically the session model)

---

## Testing

1. **Unit: summary-before-slide fires at 60%** — verify summarizer is called before any message deletion
2. **Unit: fallback to lossy slide when summarization fails** — verify graceful degradation
3. **Unit: fallback when `use_summarization: false`** — verify current behavior preserved
4. **Unit: summary marker replaces count marker** — verify `[context summary]` prefix in result
5. **Unit: journal receives summary** — verify flush gets structured summary, not raw truncated messages
6. **Unit: session memory extraction from summary** — verify no second LLM call, structured fields extracted
7. **Unit: session memory cap increase** — verify 4,000 token / 40 item limits
8. **Unit: backward-compatible config** — verify old configs with `summarize_threshold` still parse
9. **Integration: multi-round compaction** — verify summary markers survive into tail zone for subsequent compaction rounds
10. **Integration: tool-heavy session** — verify user messages are preserved (as summary) after compaction of 20+ tool calls

### Regression test for the original bug

A session where:
1. User sends a multi-turn request
2. Agent makes many tool calls (file reads, searches)
3. Context hits 60%, triggering compaction
4. After compaction, agent should still know what the user asked and what decisions were made

This test should FAIL on the old code (count marker) and PASS on the new code (summary marker).

---

## Migration

- `summarize_threshold` field accepted but ignored in config via `#[serde(default)]`
- `CompactionTier::Summarize` removed from enum
- `SummarizingCompactor` struct retained (used by the pre-step), but no longer a standalone tier
- `apply_summarize_tier` removed
- `highest_compaction_tier` simplified (3 tiers, not 4)
- Config validation: 3-threshold monotonicity instead of 4
- `ThresholdsNotMonotonic` error variant updated (3 fields instead of 4)
- `summarize_target()` on `ConversationBudget` removed (dead code)
- `build_strategy()` no longer needs to construct `SummarizingCompactor` as a standalone strategy; the summarizer is constructed inline in `apply_slide_tier` when `compaction_llm` is available

### Test impact

~15 existing tests reference `CompactionTier::Summarize` or the 4-tier cascade:
- Tests for `highest_compaction_tier` at summarize threshold → remove or update
- Tests for `apply_summarize_tier` → remove (function removed)
- Tests for config validation with 4 monotonic thresholds → update to 3
- Tests for `summarize_target()` → remove
- Tests for `CompactionTier::as_str()` → update

New tests (see Testing section) replace removed tests and cover the new behavior.

---

## Cost and latency analysis

### Cost

**Before:** 0-2 LLM calls per compaction event (extraction + potential parse retry)
**After:** 1 LLM call per compaction event (summary), 0 for extraction (parsed from summary)

The summary call generates ~1K tokens via `compaction_llm`. This is currently the same model as the main session. On frontier models (Claude Opus, GPT-4), this costs $0.02-0.06 per compaction. On lighter models (Sonnet, GPT-4o-mini), $0.001-0.005.

Future optimization: configure a dedicated lightweight compaction model. The summary prompt is simple structured extraction; it doesn't need the most capable model.

Compaction events are infrequent (every ~20-50 tool calls). The cost of losing user context mid-session is orders of magnitude higher than the summarization cost.

If `compaction_llm` is None (no LLM configured), the cost is zero: fallback to current lossy sliding.

### Latency

The summarization LLM call adds 1-3 seconds to the compaction path. This happens mid-turn (during `compact_tool_continuation` or `compact_if_needed` in perceive). During this time, the agent appears to pause.

Mitigations:
- Emit a `context_compacting` SSE event before the summary call so the UI can show a brief "Organizing context..." indicator
- The summary call uses `max_summary_tokens: 1024`, which bounds generation time
- If latency is unacceptable for a specific deployment, `use_summarization: false` disables it entirely

This is acceptable for v1. Streaming summarization (generating summary tokens while continuing the main turn) is a future optimization.
