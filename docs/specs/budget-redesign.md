# Budget Redesign: Simplified Model + Graceful Termination

## Status: SPEC REVIEW — Not approved for implementation

## Problem Statement

Fawx's budget system has 11 configuration knobs, many redundant or unused, and lacks the one mechanism that actually matters: graceful termination. The agent goes silent when any budget limit fires because there's no forced synthesis turn.

The quick fix (PR for `fix/graceful-loop-termination`) adds forced synthesis on top of the existing 11-knob system. This spec proposes the clean version: simplify the budget model down to its essential concerns, then build graceful termination as a first-class concept rather than a bolt-on.

## Current State (11 knobs)

```rust
pub struct BudgetConfig {
    pub max_llm_calls: u32,              // iteration cap
    pub max_tool_invocations: u32,       // REDUNDANT — tools fire within LLM turns
    pub max_tokens: u64,                 // token cap
    pub max_cost_cents: u64,             // UNUSED — never wired to real pricing
    pub max_wall_time_ms: u64,           // wall clock cap
    pub max_recursion_depth: u32,        // decomposition depth
    pub max_synthesis_tokens: u32,       // UNCLEAR — not meaningfully used
    pub max_consecutive_failures: u16,   // retry policy (now in RetryPolicyConfig)
    pub max_cycle_failures: u16,         // retry policy (now in RetryPolicyConfig)
    pub max_tool_retries: u8,            // retry policy (backward compat shim)
    pub decompose_depth_mode: DepthMode, // WRONG PLACE — decomposition concern, not budget
}
```

### Why each removal is safe

- **`max_tool_invocations`**: Tools are called within LLM turns. If `max_llm_calls` (iterations) is 64, and each turn produces 1-3 tool calls, the iteration cap already bounds tool usage. A separate tool invocation cap adds confusion without safety value.

- **`max_cost_cents`**: The `estimate_cost()` function exists but is never called from real pricing data. No provider integration feeds actual costs. This is speculative infrastructure that was never completed.

- **`max_synthesis_tokens`**: Only referenced in `synthesis_budget()` which caps token allocation for synthesis/custom instructions. This is a prompt assembly concern, not a loop budget concern. It should live in prompt config, not budget config.

- **`decompose_depth_mode`**: Controls whether decomposition depth is static or adaptive. This is a decomposition strategy parameter, not a budget parameter. It should live in decomposition config.

- **`max_consecutive_failures` / `max_cycle_failures` / `max_tool_retries`**: PR #1569 introduced `RetryPolicyConfig` as a sub-struct. The legacy fields exist for backward compatibility via serde. The redesign promotes `RetryPolicyConfig` as the single source of truth and removes the legacy fields from `BudgetConfig`, handling migration in deserialization.

## Proposed State (5 concerns)

```rust
pub struct BudgetConfig {
    /// Maximum LLM call iterations before forced termination.
    pub max_iterations: u32,

    /// Maximum wall clock time for the entire cycle.
    pub max_wall_time_ms: u64,

    /// Maximum total tokens (input + output) across the cycle.
    pub max_tokens: u64,

    /// Retry/failure policy (consecutive failures, cycle failures, per-tool retries).
    pub retry_policy: RetryPolicyConfig,

    /// Number of consecutive tool-only turns before injecting a progress nudge.
    /// Set to 0 to disable.
    pub tool_nudge_threshold: u16,
}
```

### Backward Compatibility

Serde handles the migration:

```rust
#[serde(alias = "max_llm_calls")]
pub max_iterations: u32,
```

Old configs with `max_llm_calls` deserialize into `max_iterations`. Removed fields (`max_tool_invocations`, `max_cost_cents`, etc.) are silently ignored via `#[serde(deny_unknown_fields)]` NOT being set (which is already the case).

The `Default` impl and named constructors (`permissive()`, `conservative()`) produce the new shape. Any code reading removed fields gets a compile error, forcing explicit migration.

### What happens to removed fields' logic

| Removed field | Current behavior | Migration |
|---|---|---|
| `max_tool_invocations` | Checked in `BudgetTracker::check_at()` | Remove check. Iteration cap is sufficient. |
| `max_cost_cents` | Checked in `BudgetTracker::check_at()` | Remove check. Never fed real data. |
| `max_synthesis_tokens` | Read by `synthesis_budget()` | Move to prompt/synthesis config. Hardcode reasonable default (4096) at call site until moved. |
| `max_recursion_depth` | Read by decomposition engine | Move to decomposition config. Keep reading from budget config with deprecation warning until moved. |
| `decompose_depth_mode` | Read by decomposition engine | Same as above. |
| Legacy retry fields | Deserialized into RetryPolicyConfig | Remove from BudgetConfig. RetryPolicyConfig is already the canonical source. |

## Graceful Termination (First-Class)

Instead of bolt-on `forced_synthesis_turn()`, termination is a proper phase in the loop:

### TerminationPolicy

```rust
pub struct TerminationPolicy {
    /// How to handle budget exhaustion.
    pub on_budget_exhausted: TerminationAction,

    /// How to handle wall time expiry.
    pub on_wall_time_expired: TerminationAction,

    /// Consecutive tool-only turns before progress nudge.
    pub nudge_after_tool_turns: u16,

    /// Budget progress threshold for "start wrapping up" hint (0.0-1.0).
    pub soft_landing_threshold: f32,
}

pub enum TerminationAction {
    /// Make one final LLM call with tools stripped to synthesize findings.
    Synthesize,

    /// Return immediately with whatever partial response exists.
    Immediate,

    /// Return a static message.
    Static(String),
}

impl Default for TerminationPolicy {
    fn default() -> Self {
        Self {
            on_budget_exhausted: TerminationAction::Synthesize,
            on_wall_time_expired: TerminationAction::Synthesize,
            nudge_after_tool_turns: 6,
            soft_landing_threshold: 0.7,
        }
    }
}
```

### Soft Landing Zone

At `soft_landing_threshold` (default 70% of iterations consumed), inject system-context into the next LLM call:

```
You have N iterations remaining. Begin working toward a conclusion.
```

At 90%:

```
Final iterations. Wrap up and respond to the user.
```

This uses the existing system prompt assembly path (not tool result injection like Hermes). The model sees it as part of its instructions, not as metadata stuffed into a tool response.

### Synthesis Turn

When termination fires and `TerminationAction::Synthesize` is configured:

1. Build a `CompletionRequest` with:
   - Same model as the main loop
   - Current conversation messages
   - `tools: vec![]` (empty — forces text response)
   - System prompt append: "Your iteration budget is exhausted. Provide a final response summarizing your findings."
2. Call `llm.complete()` with 30s timeout
3. On success: return synthesized text as `LoopResult::Complete` (not `BudgetExhausted` — from the user's perspective, the agent completed)
4. On failure: fall back to `partial_response` or structured fallback message

### Why `LoopResult::Complete` instead of `BudgetExhausted`

The distinction between "completed" and "budget exhausted" matters to the orchestrator/caller, but not to the user. If we successfully synthesize, the response is complete from the user's perspective. The caller can check iteration count to know it was a forced completion.

If we fail to synthesize, then it's truly `BudgetExhausted` with a fallback message.

## Comparison: Quick Fix vs. Elegant Solution

| Aspect | Quick Fix | Elegant Solution |
|---|---|---|
| Forced synthesis | Bolted onto `handle_budget_check()` | First-class `TerminationPolicy` |
| Budget config | 11 knobs unchanged | 5 concerns, clean separation |
| Soft landing | Not included | System-context injection at 70%/90% |
| Nudge | Hardcoded threshold (6) | Configurable in `TerminationPolicy` |
| Result type | `BudgetExhausted` with synthesized text | `Complete` on success, `BudgetExhausted` on synthesis failure |
| Backward compat | Full (no config changes) | Serde aliases, removed fields silently ignored |
| Complexity | ~30-40 lines added | ~200 lines changed, ~50 tests updated |
| Risk | Low — additive only | Medium — touches serialization, budget tracker, test expectations |

## Migration Plan

### Phase 1: Ship quick fix
- Forced synthesis + nudge counter on current budget model
- Validates the UX improvement in production

### Phase 2: Budget simplification (this spec)
- Remove unused fields, consolidate retry policy
- Add `TerminationPolicy` as first-class concept
- Update tests, verify serde backward compat
- Benchmark against quick fix: same behavior, cleaner internals

### Phase 3: Move displaced concerns
- `max_synthesis_tokens` → prompt config
- `max_recursion_depth` + `decompose_depth_mode` → decomposition config
- Clean deprecation path

## Open Questions

1. **Should `TerminationPolicy` live in `BudgetConfig` or alongside it?** Termination is a budget concern (it fires when budget is exhausted), but the synthesis turn involves the LLM provider which is not a budget concept. Leaning toward: `BudgetConfig` owns the thresholds, `LoopRunner` owns the synthesis logic, `TerminationPolicy` bridges them.

2. **Should soft landing injection be visible to the user?** The system-context injection is invisible to the user (it's in the system prompt). But should the TUI show a "wrapping up..." indicator? Probably yes for AX transparency.

3. **Should `TerminationAction::Synthesize` be the default for all termination triggers?** Wall time expiry might not leave enough time for a synthesis call. Maybe `on_wall_time_expired` should default to `Immediate` with a timeout reserve check.

4. **Is `LoopResult::Complete` the right result type for forced synthesis?** It means callers can't distinguish "agent decided to stop" from "budget forced it to stop." Could add a `forced: bool` field, or a `LoopResult::ForcedComplete` variant.

## Files Affected

- `engine/crates/fx-kernel/src/budget.rs` — BudgetConfig simplification, TerminationPolicy
- `engine/crates/fx-kernel/src/loop_engine.rs` — synthesis turn, nudge, soft landing injection
- `engine/crates/fx-kernel/src/budget_tracker.rs` — remove checks for removed fields
- Tests across both files (~50 test updates expected)
- Config serialization tests for backward compat
