# Spec: Synthesis Context Guard (#1110)

## Problem

`synthesize_tool_fallback()` sends ALL accumulated tool results in a single LLM prompt without checking total size. When many tool rounds execute (e.g., 48 files batched at 4/round × 16KB cap), results exceed the model's 200K token context window:

```
✗ loop error: tool synthesis generation failed: client error 400: 
  prompt is too long: 220145 tokens > 200000 maximum
```

Conversation compaction can't help — tool results accumulate in-memory inside `act_with_tools()`, outside the conversation history.

## Design

### Three-layer defense

#### Layer 1: Aggregate size check in tool round loop

At the top of the tool round loop (where #1105 added budget state checks), also check accumulated tool result size:

```rust
// In act_with_tools(), inside the round loop
let result_tokens = estimate_tool_result_tokens(&state.all_tool_results);
if result_tokens > self.max_synthesis_context_tokens() {
    self.emit_signal(SignalKind::Performance, "tool results exceed synthesis context limit");
    break; // fall through to synthesize_tool_fallback
}
```

**Threshold**: 50% of model context window. For 200K context, that's 100K tokens for results, leaving 100K for system prompt + conversation + synthesis output.

#### Layer 2: Eviction before synthesis

In `synthesize_tool_fallback()`, before building the synthesis prompt, check total size and evict if needed:

```rust
fn prepare_results_for_synthesis(
    results: &mut Vec<ToolResult>,
    max_tokens: usize,
) {
    while estimate_tool_result_tokens(results) > max_tokens && results.len() > 1 {
        // Evict oldest result, replace with stub
        let evicted = results.remove(0);
        results.insert(0, ToolResult {
            tool_call_id: evicted.tool_call_id,
            content: format!("[evicted — {} bytes, exceeded synthesis context limit]", 
                evicted.content.len()),
            success: evicted.success,
        });
    }
}
```

**Eviction strategy**: Oldest-first. Rationale:
- Later results are more likely to be what the model was working toward
- Earlier results may have already been incorporated into the model's reasoning
- Simple, deterministic, no heuristics to tune

**Why not summarize instead of evict?** Summarizing requires an LLM call, which costs budget and could itself exceed context. Eviction is zero-cost and guaranteed to fit.

#### Layer 3: Budget state trigger

Add accumulated result size as a third trigger for `BudgetState::Low`:

```rust
// In budget.rs, state() method
pub fn state(&self) -> BudgetState {
    // Existing: cost_cents and llm_calls checks
    // New: accumulated result size check
    if self.accumulated_result_bytes > self.config.max_aggregate_result_bytes {
        return BudgetState::Low;
    }
    // ... existing checks
}
```

This requires `BudgetTracker` to track accumulated result bytes, recorded in `execute_tool_round()` alongside the existing cost recording.

## Configuration

New fields on `BudgetConfig`:

```rust
/// Maximum aggregate tool result size before triggering Low state.
/// Default: 400_000 bytes (~100K tokens). 
pub max_aggregate_result_bytes: usize,

/// Maximum tokens to include in synthesis prompt.
/// Default: 100_000 (50% of 200K context).
pub max_synthesis_tokens: usize,
```

## Token estimation

Reuse the existing `estimate_tokens()` function from `conversation_compactor.rs` (chars / 4 approximation). Good enough for a guard — we're preventing a 220K > 200K crash, not optimizing to the last token.

## What this does NOT do

- **Per-result summarization**: too expensive, requires LLM calls during a budget-constrained path
- **Smart eviction** (relevance-based): YAGNI for v1, oldest-first is sufficient
- **Cross-iteration result memory**: results are scoped to a single `act_with_tools()` call, not persisted
- **Model-specific context limits**: uses a single configurable threshold, not per-provider detection. Can be enhanced later.

## Files changed

- `engine/crates/fx-kernel/src/loop_engine.rs` — Layer 1 (round loop check) + Layer 2 (eviction in `synthesize_tool_fallback`)
- `engine/crates/fx-kernel/src/budget.rs` — Layer 3 (result size tracking + state trigger)

## Tests

1. **Eviction test**: 10 tool results totaling 200K tokens → eviction reduces to under limit, oldest replaced with stubs
2. **Round loop break**: accumulated results exceed threshold → loop breaks, falls through to synthesis
3. **Budget state trigger**: accumulated result bytes exceed config → `state()` returns `Low`
4. **Synthesis succeeds after eviction**: verify the evicted prompt actually fits and produces output
5. **No eviction when under limit**: normal case, all results preserved

## Estimated size

~120 lines code + ~100 lines tests. Single PR, single crate (fx-kernel).
