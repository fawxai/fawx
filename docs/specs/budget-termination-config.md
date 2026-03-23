# Budget Termination Config — Implementation Spec

## Goal

Replace hardcoded graceful termination constants with configurable `TerminationConfig` sub-struct in `BudgetConfig`. Add escalating enforcement: after nudge is ignored, strip tools entirely on subsequent call.

## Changes

### 1. `budget.rs` — Add `TerminationConfig`

Add this struct anywhere above `BudgetConfig`:

```rust
/// Controls how the loop exits when a budget limit fires and how tool-only
/// turn runs are handled.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminationConfig {
    /// When true, make one final LLM call with tools stripped to force a
    /// text response before returning BudgetExhausted.
    #[serde(default = "default_synthesize_on_exhaustion")]
    pub synthesize_on_exhaustion: bool,

    /// Consecutive tool-only turns before injecting a nudge message telling
    /// the agent to respond to the user.  0 disables the nudge.
    #[serde(default = "default_nudge_after_tool_turns")]
    pub nudge_after_tool_turns: u16,

    /// Additional consecutive tool-only turns *after the nudge fires* before
    /// tools are stripped entirely, forcing a text response.  0 means strip
    /// immediately when the nudge threshold is reached (same as no grace).
    #[serde(default = "default_strip_tools_after_nudge")]
    pub strip_tools_after_nudge: u16,
}

fn default_synthesize_on_exhaustion() -> bool { true }
fn default_nudge_after_tool_turns() -> u16 { 6 }
fn default_strip_tools_after_nudge() -> u16 { 3 }

impl Default for TerminationConfig {
    fn default() -> Self {
        Self {
            synthesize_on_exhaustion: true,
            nudge_after_tool_turns: 6,
            strip_tools_after_nudge: 3,
        }
    }
}
```

Add to `BudgetConfig`:
```rust
    #[serde(default)]
    pub termination: TerminationConfig,
```

Add to `Default for BudgetConfig`, `permissive()`, `conservative()`:
```rust
    termination: TerminationConfig::default(),
```

### 2. `loop_engine.rs` — Replace Constants with Config

**Remove these constants:**
- `TOOL_ONLY_TURN_NUDGE_THRESHOLD` (currently `6`)

**Keep these constants** (they're message text, not thresholds):
- `BUDGET_EXHAUSTED_SYNTHESIS_DIRECTIVE`
- `BUDGET_EXHAUSTED_FALLBACK_RESPONSE`
- `TOOL_ONLY_TURN_NUDGE`

**Replace nudge check** (around line 2012):

Current:
```rust
if self.consecutive_tool_only_turns >= TOOL_ONLY_TURN_NUDGE_THRESHOLD {
    context_window.push(Message::system(TOOL_ONLY_TURN_NUDGE.to_string()));
}
```

New:
```rust
let tc = &self.budget.config().termination;
let nudge_at = tc.nudge_after_tool_turns;
if nudge_at > 0 && self.consecutive_tool_only_turns >= nudge_at {
    context_window.push(Message::system(TOOL_ONLY_TURN_NUDGE.to_string()));
}
```

**Add tool stripping** — in the method that builds the `CompletionRequest` for the
reasoning step (the `reason()` method or wherever tools are passed to the LLM).
Find where `tools` are assembled for the completion request:

```rust
let strip_at = nudge_at.saturating_add(tc.strip_tools_after_nudge);
let should_strip = nudge_at > 0
    && tc.strip_tools_after_nudge > 0
    && self.consecutive_tool_only_turns >= strip_at;

// When building the CompletionRequest:
let tools = if should_strip {
    tracing::info!(
        turns = self.consecutive_tool_only_turns,
        "stripping tools: agent exceeded nudge + grace threshold"
    );
    vec![]
} else {
    self.tool_executor.tool_definitions()
};
```

**Replace synthesis gate** — in the budget exhaustion path (around line 1515):

Current: always calls `forced_synthesis_turn`.

New:
```rust
if self.budget.config().termination.synthesize_on_exhaustion {
    // ... existing forced_synthesis_turn call ...
}
```

### 3. Tests

**New tests needed:**

1. `nudge_threshold_from_config` — set `nudge_after_tool_turns: 4`, verify nudge fires at 4 not 6
2. `nudge_disabled_when_zero` — set `nudge_after_tool_turns: 0`, verify no nudge at any count
3. `tools_stripped_after_nudge_grace` — set `nudge_after_tool_turns: 3, strip_tools_after_nudge: 2`, verify at turn 5 tools are empty in the CompletionRequest
4. `tools_not_stripped_before_grace` — same config, verify at turn 4 tools are still present
5. `synthesis_skipped_when_disabled` — set `synthesize_on_exhaustion: false`, verify forced_synthesis_turn is not called
6. `default_termination_config_matches_current_behavior` — default config produces same behavior as current hardcoded values

**Existing tests to update:**

Any test constructing `BudgetConfig` directly (not via `Default`) needs `termination: TerminationConfig::default()` added. These should all use `..Default::default()` already, but verify.

### 4. Config file example

Users can override in `~/.fawx/config.toml`:
```toml
[budget.termination]
synthesize_on_exhaustion = true
nudge_after_tool_turns = 6
strip_tools_after_nudge = 3
```

## Files Touched

- `engine/crates/fx-kernel/src/budget.rs` — add struct + field
- `engine/crates/fx-kernel/src/loop_engine.rs` — replace constant, add strip logic, gate synthesis
- Tests in both files

## What NOT to Change

- No field removals from BudgetConfig
- No changes to check_resources(), BudgetAllocator, or BudgetState
- No changes to BudgetTracker::state() or soft_ceiling_percent
- No changes to RetryPolicyConfig
- Keep all existing message text constants (just stop using the threshold constant)
- Keep forced_synthesis_turn() method as-is (it's already clean)
