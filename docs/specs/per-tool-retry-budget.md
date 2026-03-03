# Per-Tool Retry Budget with Escalation to Blocked

**Status:** Proposed spec  
**Issue:** #1101  
**Depends on:** PR #1098 (`feat/loop-resilience`) — uses `BudgetState`, `SignalKind::Blocked`  
**Branch:** `feat/loop-resilience`

---

## Problem

When a tool fails, the LLM retries it. There is no per-tool retry limit — the model can (and does) retry the same failing tool indefinitely until the global budget soft-ceiling kicks in. This wastes budget on tools that are persistently broken (e.g., network-dependent tools during an outage, permission-denied file operations, malformed API calls the model can't self-correct).

The global budget soft-ceiling (`BudgetState::Low` at 80% cost) catches this eventually, but by then significant budget has been wasted on retries. A per-tool retry cap with escalation to `SignalKind::Blocked` would:

1. Fail fast on persistently broken tools (ENGINEERING.md §2).
2. Preserve budget for productive work.
3. Give the LLM an explicit "this tool is blocked" signal so it can adapt strategy.

---

## Design

### Tracking Structure

A `HashMap<String, u8>` on `LoopEngine` tracks attempt counts per tool name within a single cycle:

```rust
/// Per-tool attempt counter for the current cycle.
/// Key: tool name, Value: number of attempts (including first call).
tool_attempts: HashMap<String, u8>,
```

### Lifecycle

- **Increment:** In `execute_tool_calls()`, before executing each `ToolCall`, increment `tool_attempts[&call.name]`. If the count exceeds `max_tool_retries + 1` (i.e., more than 3 total attempts with default cap of 2 retries), skip execution for that call and return a synthetic `ToolResult` with `success: false` and an explanatory message.
- **Reset:** In `prepare_cycle()`, clear `tool_attempts` (already resets budget, signals, etc.). This ensures each cycle starts fresh — a tool blocked in one user turn can be retried in the next.
- **Scope:** Per tool *name*, not per tool call ID. If the model calls `read_file` three times with different arguments and all fail, that's 3 attempts of `read_file`. The fourth `read_file` call (regardless of arguments) is blocked.

### Blocked Escalation

When a tool hits its retry cap:

1. **Synthetic failure result:** Return `ToolResult { success: false, output: "Tool '{name}' blocked: exceeded {max} retries this cycle. Try a different approach.", tool_name: name, tool_call_id: call.id }`.
2. **Signal emission:** Emit `SignalKind::Blocked` with metadata `{ "tool": name, "attempts": count, "max_retries": max }`.
3. **No permanent blocking:** The tool is only blocked for the remainder of the current cycle. `prepare_cycle()` resets the counter.

### Why Per-Name, Not Per-Call-ID

Tool call IDs are unique per invocation — they can't track retries. The model retries a tool by issuing a new call with a new ID but the same tool name (potentially with adjusted arguments). Tracking by name catches this pattern.

The trade-off: if the model calls `read_file("a.rs")` successfully twice and then `read_file("b.rs")` fails, all three count toward the `read_file` limit. This is acceptable because:
- The cap is generous (3 total attempts by default).
- Successful calls don't typically repeat — the model has the result.
- A tool that fails 3 times with different arguments likely has a systemic issue.

### Configuration

Add `max_tool_retries: u8` to `BudgetConfig`:

```rust
/// Maximum retries per tool name per cycle (0 = no retries, only initial attempt).
/// Total attempts = max_tool_retries + 1.
#[serde(default = "default_max_tool_retries")]
pub max_tool_retries: u8,
```

Default: `2` (3 total attempts). Stored in `BudgetConfig` because it's a budget-adjacent guardrail — it limits resource expenditure per tool, same as `max_fan_out` limits resource expenditure per LLM response.

---

## Where to Change

### 1. `engine/crates/fx-kernel/src/loop_engine.rs` — `LoopEngine` struct

Add field:

```rust
tool_attempts: HashMap<String, u8>,
```

Initialize to `HashMap::new()` in the builder/constructor.

### 2. `engine/crates/fx-kernel/src/loop_engine.rs` — `prepare_cycle()`

At line ~817, add `self.tool_attempts.clear();` alongside the existing resets (`self.budget.reset()`, `self.signals.clear()`, etc.).

### 3. `engine/crates/fx-kernel/src/loop_engine.rs` — `execute_tool_calls()`

At line ~2407, before the call to `self.tool_executor.execute_tools(calls, ...)`:

- Partition `calls` into `allowed` and `blocked` based on `tool_attempts`.
- For each call in `allowed`, increment `tool_attempts[&call.name]`.
- Execute only `allowed` calls via `self.tool_executor.execute_tools()`.
- For each `blocked` call, create a synthetic `ToolResult` with failure message.
- Combine results in original call order.
- Emit `SignalKind::Blocked` for each newly-blocked tool.

### 4. `engine/crates/fx-kernel/src/budget.rs` — `BudgetConfig`

Add `max_tool_retries: u8` field with `#[serde(default = "default_max_tool_retries")]`. Add `default_max_tool_retries()` returning `2`. Update `BudgetConfig::default()`, `BudgetConfig::conservative()`, and `BudgetConfig::unlimited()`:

- `default()`: `max_tool_retries: 2`
- `conservative()`: `max_tool_retries: 1`
- `unlimited()`: `max_tool_retries: u8::MAX`

### 5. `engine/crates/fx-core/src/signals.rs` — no changes

`SignalKind::Blocked` already exists and is the correct variant for this signal.

---

## Test Cases

### Basic counting
1. Tool called once → `tool_attempts["tool_name"]` == 1. Execution proceeds.
2. Tool called 3 times (default cap=2 retries) → all 3 execute.
3. Tool called 4th time → blocked, synthetic failure result returned.
4. Different tools each called 3 times → all execute (independent counters).

### Blocked behavior
5. Blocked tool returns `success: false` with message mentioning tool name and retry count.
6. Blocked tool emits `SignalKind::Blocked` signal with tool name in metadata.
7. Tool blocked on 4th attempt remains blocked on 5th, 6th, etc. within same cycle.
8. Mixed batch: 3 calls where 1 tool is blocked, 2 are fresh → blocked tool gets synthetic result, other 2 execute normally.

### Reset
9. After `prepare_cycle()`, a previously-blocked tool can be called again.
10. `tool_attempts` is empty after `prepare_cycle()`.

### Configuration
11. `max_tool_retries: 0` → tool blocked on 2nd attempt (1 attempt allowed).
12. `max_tool_retries: u8::MAX` → effectively unlimited retries (matches `BudgetConfig::unlimited()`).
13. `BudgetConfig::conservative()` has `max_tool_retries: 1` (2 total attempts).

### Integration with fan-out cap
14. Fan-out cap defers tools; deferred tools don't count toward `tool_attempts` (they weren't executed).
15. When deferred tools are re-requested in the next round, they start fresh counts from that round.

### Integration with budget soft-ceiling
16. Tool retry blocked AND budget is `Low` → budget-low takes precedence (checked first in `act_with_tools()`).

---

## Scope & Estimates

| Component | Files touched | Lines (est.) | Risk |
|-----------|--------------|-------------|------|
| `tool_attempts: HashMap<String, u8>` field | `fx-kernel/src/loop_engine.rs` | ~5 | None |
| `prepare_cycle()` reset | `fx-kernel/src/loop_engine.rs` | ~2 | None |
| `execute_tool_calls()` gating logic | `fx-kernel/src/loop_engine.rs` | ~50 | Low |
| Synthetic `ToolResult` construction | `fx-kernel/src/loop_engine.rs` | ~15 | None |
| `SignalKind::Blocked` emission | `fx-kernel/src/loop_engine.rs` | ~10 | None |
| `max_tool_retries` in `BudgetConfig` | `fx-kernel/src/budget.rs` | ~20 | None |
| Tests | test module in `loop_engine.rs` | ~200 | None |
| **Total** | | **~300** | **Low** |

No new crates. No new dependencies. The main risk is in `execute_tool_calls()` where the partitioning logic must preserve call ordering and handle the case where all calls in a batch are blocked.
