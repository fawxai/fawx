# Budget Redesign: Graceful Termination as First-Class Concept

## Status: SPEC REVIEW — Not approved for implementation

## Problem Statement

Fawx's budget system reliably enforces resource limits and allocates budgets across sub-goals. What it lacks is a graceful exit path: when any limit fires, the loop returns `BudgetExhausted` with stale `partial_response` and the user gets silence.

The quick fix (PR for `fix/graceful-loop-termination`) bolts `forced_synthesis_turn()` onto the existing budget system. This spec proposes a cleaner integration: making graceful termination a first-class concept alongside enforcement and allocation, without dismantling the existing budget infrastructure.

## Current State — Full Audit

### BudgetConfig (15 fields)

| Field | Purpose | Actively enforced | Used in allocation | Verdict |
|---|---|---|---|---|
| `max_llm_calls` | Iteration cap | Yes (`check_resources`) | Yes (distributed by weight) | **Keep** |
| `max_tool_invocations` | Tool call cap | Yes (`check_resources`) | Yes (distributed, floor-checked) | **Keep** — 51 non-test refs, deeply wired into allocation/decomposition |
| `max_tokens` | Token cap | Yes (`check_resources`) | Yes (distributed) | **Keep** |
| `max_cost_cents` | Cost cap | Yes (`check_resources`) | Yes (distributed, floor-checked) | **Keep** — enforced on every check, distributed in allocation. Not wired to real pricing, but the enforcement path exists and is load-bearing in tests/decomposition |
| `max_wall_time_ms` | Wall clock cap | Yes (`check_at`) | Yes (distributed) | **Keep** |
| `max_recursion_depth` | Decomposition depth | Yes (`check_resources`) | No (checked directly) | **Keep** — part of safety boundary |
| `decompose_depth_mode` | Static vs. adaptive depth | No (read in decompose logic) | No | **Relocatable** — strategy concern, not enforcement. Could move to decomposition config in a future PR |
| `soft_ceiling_percent` | Triggers `BudgetState::Low` at N% | Yes (`state()`) | No | **Keep** — drives the wrap-up directive injection |
| `max_fan_out` | Max parallel tool calls per turn | Yes (in tool execution) | No | **Keep** — safety guard |
| `max_tool_result_bytes` | Per-result truncation | Yes (in tool execution) | No | **Keep** — prevents context blowup |
| `max_aggregate_result_bytes` | Triggers `BudgetState::Low` | Yes (`state()`) | No | **Keep** — context pressure signal |
| `max_synthesis_tokens` | Synthesis prompt token limit | Yes (in prompt assembly) | No | **Relocatable** — prompt assembly concern. Could move to prompt config in a future PR |
| `max_consecutive_failures` | Per-tool retry limit | Yes (via `RetryPolicyConfig`) | No | **Consolidate** — PR #1569 introduced `RetryPolicyConfig`. This field exists for backward compat serde |
| `max_cycle_failures` | Cycle-wide failure limit | Yes (via `RetryPolicyConfig`) | No | **Consolidate** — same as above |
| `max_tool_retries` | Legacy retry field | Yes (deserialized into retry config) | No | **Consolidate** — backward compat shim |

### Key Finding

The previous spec incorrectly labeled `max_tool_invocations` and `max_cost_cents` as "unused." Both are actively enforced in `check_resources()` on every iteration and distributed across sub-goals in `BudgetAllocator::allocate()`. Removing them would break budget decomposition, floor enforcement, and resource tracking.

### What Can Actually Change

**Phase 1 (this spec):** Add graceful termination. Touch no existing fields.

**Phase 2 (future, optional):** Consolidate retry fields into `RetryPolicyConfig` sub-struct (removing legacy shims). Relocate `decompose_depth_mode` and `max_synthesis_tokens` to their owning concerns. These are pure refactors with no behavioral change.

## Proposed Change: Graceful Termination

### New: `TerminationConfig`

Added to `BudgetConfig` as a sub-struct:

```rust
/// Controls how the loop exits when a budget limit fires.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminationConfig {
    /// When true, make one final LLM call with tools stripped to synthesize
    /// findings before returning. When false, return immediately with
    /// whatever partial response exists.
    #[serde(default = "default_synthesize_on_exhaustion")]
    pub synthesize_on_exhaustion: bool,

    /// Consecutive tool-only turns before injecting a progress nudge.
    /// 0 disables the nudge.
    #[serde(default = "default_nudge_after_tool_turns")]
    pub nudge_after_tool_turns: u16,
}

fn default_synthesize_on_exhaustion() -> bool { true }
fn default_nudge_after_tool_turns() -> u16 { 6 }

impl Default for TerminationConfig {
    fn default() -> Self {
        Self {
            synthesize_on_exhaustion: true,
            nudge_after_tool_turns: 6,
        }
    }
}
```

Why a bool instead of an enum with `Synthesize/Immediate/Static` variants: the quick fix showed that `Static` is just the fallback when synthesis fails, not a user choice. And `Immediate` is `synthesize_on_exhaustion: false`. The enum added design surface for zero practical value.

### Existing: `soft_ceiling_percent` Is the Soft Landing

The spec originally proposed a `soft_landing_threshold` at 70%. This already exists as `soft_ceiling_percent` (default 80%). When `BudgetTracker::state()` returns `BudgetState::Low`, the loop already injects `BUDGET_LOW_WRAP_UP_DIRECTIVE` into the context:

```
"You are running low on budget. Do not call any tools. Do not decompose.
Summarize what you have accomplished and what remains undone. Be concise."
```

This is the soft landing. It already works. The gap is only what happens when the model ignores the directive and the hard limit fires — that's where forced synthesis comes in.

### Integration Points

**`handle_budget_check()` change:**

Currently:
```rust
fn handle_budget_check(&mut self, cost: ActionCost, partial_response: Option<String>) -> Option<LoopResult> {
    if self.budget.check_at(current_time_ms(), &cost).is_ok() {
        return None;
    }
    // Immediately return BudgetExhausted with stale partial_response
    Some(LoopResult::BudgetExhausted { partial_response, ... })
}
```

Proposed (requires LLM provider access):
```rust
fn handle_budget_check(&mut self, cost: ActionCost, partial_response: Option<String>, llm: &dyn LlmProvider, messages: &[Message]) -> Option<LoopResult> {
    if self.budget.check_at(current_time_ms(), &cost).is_ok() {
        return None;
    }
    
    let response = if self.termination_config.synthesize_on_exhaustion {
        self.forced_synthesis_turn(llm, messages).await
            .unwrap_or_else(|| partial_response.unwrap_or_else(|| BUDGET_EXHAUSTED_FALLBACK_RESPONSE.to_string()))
    } else {
        partial_response.unwrap_or_else(|| BUDGET_EXHAUSTED_FALLBACK_RESPONSE.to_string())
    };
    
    Some(LoopResult::BudgetExhausted { partial_response: Some(response), ... })
}
```

**Signature change concern:** `handle_budget_check` is called from multiple sites in `run_iteration()`. Each call site already has access to the LLM provider and messages. The cleanest approach: keep `handle_budget_check` pure (returns the decision), and let the call site in `run_iteration()` handle the synthesis call before constructing the Terminal result. This avoids making `handle_budget_check` async.

**Pattern:**
```rust
if let Some(exhaustion) = self.handle_budget_check(cost, state.partial_response.clone()) {
    // NEW: attempt synthesis before returning
    let final_response = if self.termination_config.synthesize_on_exhaustion {
        match self.forced_synthesis_turn(llm, &perception.context_window).await {
            Ok(text) => text,
            Err(_) => exhaustion.partial_response_or_fallback(),
        }
    } else {
        exhaustion.partial_response_or_fallback()
    };
    return Ok(IterationStep::Terminal(LoopResult::BudgetExhausted {
        partial_response: Some(final_response),
        ..exhaustion_fields
    }));
}
```

**`consecutive_tool_only_turns` in CycleState:**

```rust
struct CycleState {
    learnings: Vec<Learning>,
    tokens: TokenUsage,
    partial_response: Option<String>,
    consecutive_tool_only_turns: u16,  // NEW
}
```

Incremented in the tool-call path when the model produces tool calls without text. Reset when the model produces text. At threshold, inject `TOOL_ONLY_TURN_NUDGE` as a system message in the next perception step.

### Result Type: Keep `BudgetExhausted`

The forced synthesis response goes into `BudgetExhausted.partial_response`. This preserves the semantic distinction for callers (orchestrators, fleet dispatchers, signals) that need to know the agent was terminated by budget, not by natural completion. The user-facing layer can present it identically to a `Complete` — that's a rendering concern, not a loop concern.

## Files Affected

- `engine/crates/fx-kernel/src/budget.rs`:
  - Add `TerminationConfig` struct
  - Add `termination` field to `BudgetConfig` (with `#[serde(default)]`)
  - Update `Default`, `permissive()`, `conservative()` constructors
  - Update `From<BudgetConfig> for BudgetTracker` (pass through config)
  - ~20 lines new code, ~10 lines modified

- `engine/crates/fx-kernel/src/loop_engine.rs`:
  - Add `forced_synthesis_turn()` method to `LoopRunner`
  - Add `consecutive_tool_only_turns` to `CycleState`
  - Modify budget-check call sites to attempt synthesis (3-4 sites)
  - Add nudge injection in perception step
  - ~50-70 lines new code

- Tests in both files:
  - New: forced synthesis on budget exhaustion, synthesis failure fallback, nudge threshold, nudge reset
  - Modified: tests constructing `BudgetConfig` need `..Default::default()` for new field
  - Estimated: ~8 new tests, ~15 test constructor updates (mechanical)

## What This Spec Does NOT Change

- No field removals from `BudgetConfig`
- No changes to `check_resources()` or `BudgetAllocator::allocate()`
- No changes to `BudgetState::Low` / soft ceiling behavior
- No changes to retry policy or `RetryPolicyConfig`
- No changes to `BudgetRemaining` or resource tracking
- No serde breaking changes (new field has `#[serde(default)]`)

## Comparison to Quick Fix

| Aspect | Quick Fix | This Spec |
|---|---|---|
| Forced synthesis | Inline in `handle_budget_check` | Same mechanism, structured as `TerminationConfig` |
| Nudge | Hardcoded threshold (6) | Configurable via `nudge_after_tool_turns` |
| Config | No config changes | `TerminationConfig` sub-struct, serde-defaulted |
| Testability | Tests against hardcoded behavior | Tests against configurable behavior |
| Risk | Minimal — additive only | Low — additive, no removals, serde-compatible |
| Complexity delta | ~30 lines | ~70 lines + config struct |

## Open Questions

1. **Should `forced_synthesis_turn` have its own timeout?** The main loop has `max_wall_time_ms`. If budget exhausts at 590s of a 600s wall time, is there enough time for synthesis? Proposal: use `min(30s, remaining_wall_time - 2s)` as the synthesis timeout. If <2s remain, skip synthesis.

2. **Should the nudge be a system message or appended to the last tool result?** System message is cleaner (it's an instruction, not a tool output). Hermes stuffs it into tool results, which is a hack. Proposal: system message.

3. **Should `TerminationConfig` be separate from `BudgetConfig`?** It could live on `LoopConfig` instead. But it's budget-triggered behavior, so `BudgetConfig` is the natural home. Proposal: keep it in `BudgetConfig`.

## Migration Path

1. **Ship quick fix first** — validates the UX improvement in production
2. **If quick fix works well**, this spec replaces the hardcoded behavior with `TerminationConfig`, making it configurable and testable
3. **Future (optional)**: consolidate retry fields, relocate `decompose_depth_mode` and `max_synthesis_tokens` — separate PRs, separate specs
