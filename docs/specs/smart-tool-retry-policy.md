# Smart Tool Retry Policy

## Problem

The current retry system (`partition_by_retry_budget`) has two design flaws:

1. **Keyed by tool name only** — `tool_attempts: HashMap<String, u8>` counts
   every call to `read_file` against a single counter. If `read_file("/tmp/a")`
   fails, the counter applies to `read_file("/tmp/b")` too. Legitimate diverse
   usage of the same tool gets penalized.

2. **Counts all attempts, not just failures** — A tool that succeeds 8/10 times
   still burns through its budget. The retry cap is meant to detect stuck loops,
   not limit successful tool use.

### Current behavior
- `max_tool_retries: 5` (default) → tool blocked on 7th call, regardless of
  success/failure
- `max_tool_retries: 1` (conservative) → tool blocked on 3rd call
- Key: `call.name` only
- Effect: agent says "2 tools failed" then stops responding

## Design

### Call signature tracking

Replace `HashMap<String, u8>` (name → count) with a smarter tracker:

```rust
struct ToolRetryTracker {
    /// Consecutive failure count per call signature (tool_name, args_hash).
    /// Reset to 0 on success for that signature.
    signature_failures: HashMap<CallSignature, u16>,
    /// Total failures across all tools in this cycle.
    cycle_total_failures: u16,
}

#[derive(Hash, Eq, PartialEq, Clone)]
struct CallSignature {
    tool_name: String,
    args_hash: u64,
}
```

### Decision function

```rust
enum RetryVerdict {
    Allow,
    Block { reason: String },
}

fn should_allow(
    &self,
    call: &ToolCall,
    config: &RetryPolicyConfig,
) -> RetryVerdict
```

### Policy rules (evaluated in order)

1. **Circuit breaker**: if `cycle_total_failures >= config.max_cycle_failures`
   (default: 15), block with "too many total failures this cycle"

2. **Same-signature loop**: if `signature_failures[sig] >=
   config.max_consecutive_failures` (default: 3), block with "same call failed
   N times consecutively"

3. **Otherwise**: Allow

### On result

```rust
fn record_result(&mut self, call: &ToolCall, success: bool)
```

- On **success**: set `signature_failures[sig] = 0` (reset — the call works)
- On **failure**: increment `signature_failures[sig]` and `cycle_total_failures`

This means:
- A tool that fails once then succeeds never accumulates
- A tool called with different args tracks independently
- A tool that keeps failing with the same args gets blocked quickly
- A total failure count catches pathological loops across different tools

### Args hashing

Use a deterministic hash of the serialized arguments:
```rust
fn args_hash(args: &serde_json::Value) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    // Canonicalize: sorted keys, no whitespace
    let canonical = serde_json::to_string(args).unwrap_or_default();
    canonical.hash(&mut hasher);
    hasher.finish()
}
```

Note: JSON key order from `serde_json::to_string` on a `Value` is
deterministic for the same `Value` instance (preserves insertion order).
For our purposes this is sufficient — the same `ToolCall` will always
produce the same hash.

### Configuration

```rust
struct RetryPolicyConfig {
    /// Max consecutive failures on the same (tool, args) before blocking.
    /// Default: 3
    pub max_consecutive_failures: u16,
    /// Max total failures across all tools per cycle before circuit break.
    /// Default: 15
    pub max_cycle_failures: u16,
}
```

These replace `max_tool_retries: u8` in `BudgetConfig`. For backward compat,
keep `max_tool_retries` as a serde field that maps to
`max_consecutive_failures` (so existing configs still work).

## Files to Change

1. `engine/crates/fx-kernel/src/budget.rs`
   - Add `RetryPolicyConfig` struct with defaults
   - Add backward compat: if `max_tool_retries` is set in config, map to
     `max_consecutive_failures = max_tool_retries + 1` (current semantics:
     retries, not attempts)
   - Keep `max_tool_retries` field with `#[serde(default)]` for existing
     configs

2. `engine/crates/fx-kernel/src/loop_engine.rs`
   - Add `ToolRetryTracker` and `CallSignature` types (private)
   - Replace `tool_attempts: HashMap<String, u8>` with
     `tool_retry_tracker: ToolRetryTracker`
   - Replace `partition_by_retry_budget` with
     `partition_by_retry_policy` using the new tracker
   - After tool execution results come back, call
     `tool_retry_tracker.record_results(&calls, &results)`
   - Update `emit_blocked_tool_errors` to use new reason strings
   - Update `build_blocked_tool_results` similarly
   - Clear tracker on cycle reset (line ~1558)

3. Tests in `loop_engine.rs`
   - Update existing `per_tool_retry_budget_tests` module
   - Add new tests:
     - `success_resets_failure_count` — fail, succeed, fail doesn't block
     - `different_args_tracked_independently` — same tool, different args
     - `circuit_breaker_blocks_all_tools` — total failure cap
     - `consecutive_failures_block_specific_signature` — same args blocked
     - `backward_compat_max_tool_retries` — old config field still works

## Migration

- `BudgetConfig::default()` returns `max_consecutive_failures: 3`,
  `max_cycle_failures: 15`
- `BudgetConfig::conservative()` returns `max_consecutive_failures: 2`,
  `max_cycle_failures: 8`
- `BudgetConfig::permissive()` returns `max_consecutive_failures: 10`,
  `max_cycle_failures: 50`
- Old `max_tool_retries: N` in config.toml maps to
  `max_consecutive_failures: N + 1` (preserving total-attempts semantics)

## Branch

`feat/smart-tool-retry` from `origin/dev`
PR targets `dev`.
