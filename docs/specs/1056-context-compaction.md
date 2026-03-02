# Spec: #1056 — Context Window Compaction

**Status**: Draft  
**Author**: Clawdio (scoping agent)  
**Date**: 2026-03-02

---

## 1. Problem Statement

Fawx currently has **no conversation-level context management**. The `conversation_history` field on `PerceptionSnapshot` grows unboundedly as turns accumulate. Each tool round appends assistant (ToolUse) + tool (ToolResult) messages to the `continuation_messages` vec, which is forwarded verbatim to the LLM on every subsequent call.

### Current failure modes

1. **Hard wall**: Long sessions exceed the model's context window. The LLM provider returns a token-limit error, and the loop produces a generic `LoopError` — the user loses all session momentum.
2. **Decompose amplification**: Each `decompose` tool call spawns child `LoopEngine` instances that receive the full `context_messages` slice (`build_sub_goal_snapshot`, currently ~line 1612; line numbers are approximate). Multi-level decomposition creates exponential context duplication.
3. **Tool result bloat**: Tool results (file contents, search results, memory dumps) are embedded as `ContentBlock::ToolResult` values with no size limit. A single `cat` output can consume thousands of tokens.
4. **Silent quality degradation**: Before hitting the hard wall, large context windows cause the model to lose focus on recent instructions — manifesting as repetitive actions, ignored user corrections, and degraded tool selection.

### What exists today

The `ContextCompactor` in `context_manager.rs` operates on `ReasoningContext` (the 7-step loop's internal perception model), **not** on the LLM conversation `Vec<Message>`. It compacts working memory entries, episodic/semantic refs, procedures, and identity context. The `append_compacted_summary` method in `LoopEngine` (currently ~line 1244; approximate) uses this to inject a summary into the context window when the `ReasoningContext` exceeds its threshold — but the conversation history itself (`perception.conversation_history` → `context_window`) is never compacted.

The existing `ContextCompactor` is orthogonal to this issue and should be preserved. This spec addresses the missing **conversation-level** compaction.

---

## 2. Exact Files to Change

### New files

| File | Purpose |
|------|---------|
| `engine/crates/fx-kernel/src/conversation_compactor.rs` | `CompactionStrategy` trait, `SlidingWindowCompactor`, `SummarizingCompactor`, `ConversationBudget`, `CompactionConfig` |
| `engine/crates/fx-kernel/src/conversation_compactor/tests.rs` | Unit tests (or inline `#[cfg(test)] mod tests`) |

### Modified files

> Line numbers below are approximate (verified against commit `e4a9758b`) and may drift as `loop_engine.rs` evolves.

| File | Lines/Areas | Change |
|------|-------------|--------|
| `engine/crates/fx-kernel/src/loop_engine.rs` | `LoopEngine` struct (~line 148) | Add `conversation_compactor: Box<dyn CompactionStrategy>` and `conversation_budget: ConversationBudget` fields |
| `engine/crates/fx-kernel/src/loop_engine.rs` | Constructor region: `LoopEngine::new()` + `new_with_compaction(...)` (~line 279+) | Keep existing constructor stable; add explicit compaction-aware constructor path, validate config, build strategy, initialize budget |
| `engine/crates/fx-kernel/src/loop_engine.rs` | `perceive()` (~line 633) | After building `context_window`, run `compact_if_needed()`, then `ensure_within_hard_limit(...)` before `build_reasoning_request()` |
| `engine/crates/fx-kernel/src/loop_engine.rs` | `act_with_tools()` (~line 1317) | Compact `state.continuation_messages` between tool rounds and run `ensure_within_hard_limit(...)` before `build_continuation_request()` |
| `engine/crates/fx-kernel/src/loop_engine.rs` | `run_sub_goal()` (~line 884) | Run `compact_if_needed(..., CompactionScope::DecomposeChild, ...)` before building child snapshots (this method has `&self` access to compactor/budget) |
| `engine/crates/fx-kernel/src/loop_engine.rs` | `build_child_engine()` (~line 907) | Pass effective `CompactionConfig` to child engines |
| `engine/crates/fx-kernel/src/loop_engine.rs` | `build_sub_goal_snapshot()` (~line 1612) | Build child snapshot from already-compacted messages passed by `run_sub_goal()`; no direct compaction call in this free function |
| `engine/crates/fx-kernel/src/loop_engine.rs` | `build_reasoning_request()` (~line 2166) | Keep as pure free function (no budget access); caller must preflight hard-limit compliance |
| `engine/crates/fx-kernel/src/loop_engine.rs` | `build_continuation_request()` (~line 2027) | Same — keep pure builder; caller validates context size first |
| `engine/crates/fx-kernel/src/perceive.rs` | `estimate_text_tokens()` (~line 340) | Extract shared token estimation helper and update call sites to the centralized implementation |
| `engine/crates/fx-kernel/src/lib.rs` | Module declarations | Add `pub mod conversation_compactor;` |
| `engine/crates/fx-conversation/src/lib.rs` | `ConversationStore` APIs | **Out of scope** for this spec; no `load_within_budget()` change in this PR |

---

## 3. API Design

### 3.1 ConversationBudget

Tracks how much of the model's context window is consumed and how much remains available.

```rust
/// Budget tracker for conversation-level context usage.
#[derive(Debug, Clone)]
pub struct ConversationBudget {
    /// Total context window size for the active model (tokens).
    model_context_limit: usize,
    /// Fraction of context at which compaction triggers (0.0..1.0].
    compaction_threshold: f32,
    /// Reserved token budget for system prompt + memory context.
    /// These are never compacted away.
    reserved_tokens: usize,
    /// Reserved output budget for model completion.
    /// Not available for prompt/history tokens.
    /// Default: 4096.
    output_reserve_tokens: usize,
}

impl ConversationBudget {
    pub const DEFAULT_OUTPUT_RESERVE_TOKENS: usize = 4_096;

    pub fn new(
        model_context_limit: usize,
        compaction_threshold: f32,
        reserved_tokens: usize,
    ) -> Self;

    /// Tokens available for conversation history.
    /// Formula: limit - reserved - output_reserve.
    pub fn conversation_budget(&self) -> usize;

    /// Whether the given message list exceeds the compaction trigger point.
    pub fn needs_compaction(&self, messages: &[Message]) -> bool;

    /// Whether the message list exceeds the absolute conversation hard limit
    /// (`conversation_budget()`), regardless of threshold.
    pub fn exceeds_hard_limit(&self, messages: &[Message]) -> bool;

    /// Estimate token count for a message list using the shared heuristic.
    pub fn estimate_tokens(messages: &[Message]) -> usize;

    /// Target token count after compaction (e.g. 60% of conversation budget).
    pub fn compaction_target(&self) -> usize;
}
```

#### 3.1.1 Token estimation DRY extraction (exact current signatures)

These are the current function signatures to extract into shared utilities:

```rust
// engine/crates/fx-kernel/src/perceive.rs
fn estimate_text_tokens(text: &str) -> usize;

// engine/crates/fx-kernel/src/loop_engine.rs
fn estimate_tokens(text: &str) -> u64;
```

Planned unified behavior:
- Keep the exact token heuristic: `max(chars / 4, words)`.
- Shared helper should preserve this formula exactly (no behavior change).
- Convert/bridge `usize` and `u64` as needed at call sites during extraction.

**Design note**: The heuristic intentionally overestimates in most cases, which is safer than underestimation.

**Multimodal note (TODO)**: Current conversation history only counts text/tool block content. Image/multimodal token accounting is not yet represented in history, so it is out-of-scope for this phase. Add a follow-up TODO to extend `estimate_tokens(...)` once multimodal history is first-class.

### 3.2 CompactionStrategy trait

```rust
use std::error::Error;

/// Strategy for compacting a conversation history that exceeds the context budget.
///
/// This trait is async to support strategies that require I/O (e.g.
/// `SummarizingCompactor`, which calls an LLM).
/// `SlidingWindowCompactor` is synchronous internally and completes immediately,
/// but still implements this async trait for a uniform interface.
#[async_trait]
pub trait CompactionStrategy: Send + Sync + std::fmt::Debug {
    /// Compact messages to fit within the target token budget.
    ///
    /// # Contract
    /// - System and system-like messages are NEVER modified.
    /// - Strategy-owned `preserve_recent_turns` is always honored.
    /// - Active tool call chains are preserved.
    async fn compact(
        &self,
        messages: &[Message],
        target_tokens: usize,
    ) -> Result<CompactionResult, CompactionError>;
}

#[derive(Debug, thiserror::Error)]
pub enum CompactionError {
    #[error("token estimation failed")]
    TokenEstimationFailed,
    #[error("summarization failed")]
    SummarizationFailed {
        source: Box<dyn Error + Send + Sync>,
    },
    #[error("all messages are protected; cannot compact further")]
    AllMessagesProtected,
}

#[derive(Debug, Clone)]
pub struct CompactionResult {
    /// The compacted message list.
    pub messages: Vec<Message>,
    /// Number of original messages that were removed or summarized.
    pub compacted_count: usize,
    /// Token estimate of the compacted result.
    pub estimated_tokens: usize,
    /// Whether summarization was used (vs. pure truncation).
    pub used_summarization: bool,
}
```

**Precedence ruling**: `preserve_recent_turns` is strategy-owned configuration (constructor field), not a runtime argument. `CompactionStrategy::compact(...)` intentionally does **not** accept a `preserve_recent` parameter, so there is no caller/strategy precedence ambiguity.

### 3.3 SlidingWindowCompactor

The default, zero-LLM-call strategy.

```rust
/// Keeps the N most recent turns and drops older ones.
/// Injects a "[context compacted]" marker at the truncation point.
#[derive(Debug, Clone)]
pub struct SlidingWindowCompactor {
    /// Minimum number of recent turns to always preserve.
    preserve_recent_turns: usize,
}

impl SlidingWindowCompactor {
    pub fn new(preserve_recent_turns: usize) -> Self;
}
```

**Algorithm**:
1. Separate messages into zones: protected prefix (system + system-like markers), compactable middle, and recent tail (`preserve_recent_turns`).
2. Identify active tool call chains in the middle zone and mark them protected.
3. Drop compactable middle messages (oldest first) until estimated tokens ≤ target.
4. Insert synthetic compaction marker message at truncation boundary.
5. If no removable message exists and budget is still exceeded, return `Err(CompactionError::AllMessagesProtected)`.

**Marker preservation rule**: prior compaction markers are treated as system-like and preserved across subsequent compactions.

#### 3.3.1 Worked example (before/after)

Assume `preserve_recent_turns = 6` and a 10-message conversation:

- m1: `system` — instructions
- m2: `user` — asks for fix plan
- m3: `assistant` — plan
- m4: `user` — requests file diff
- m5: `assistant` — tool call
- m6: `tool` — tool result
- m7: `assistant` — summarizes tool output
- m8: `user` — asks for final patch
- m9: `assistant` — proposes patch
- m10: `user` — confirms constraints

Compacted result:

- m1: `system` (protected)
- mX: `assistant` synthetic marker: `[context compacted: 6 older messages removed]`
- m5..m10 unchanged (last 6 messages preserved verbatim)

This demonstrates two invariants: protected/system context is retained, and the most recent `preserve_recent_turns` messages survive in order.

### 3.4 SummarizingCompactor

LLM-powered summarization for higher-quality compaction. Uses the agent's LLM provider.

**Trait disambiguation**: `SummarizingCompactor` in this spec uses the **kernel-local** trait `engine/crates/fx-kernel/src/loop_engine.rs::LlmProvider` (the one with `generate()` / `generate_streaming()`), not either `fx-llm` trait with the same name.

```rust
/// Summarizes older turns into structured context using an LLM call.
#[derive(Debug)]
pub struct SummarizingCompactor {
    /// LLM provider for summarization calls.
    llm: Arc<dyn LlmProvider>,
    /// Minimum number of recent turns to always preserve.
    preserve_recent_turns: usize,
    /// Maximum tokens to spend on the summarization prompt itself.
    max_summary_tokens: usize,
}

impl SummarizingCompactor {
    pub const DEFAULT_MAX_SUMMARY_TOKENS: usize = 1_024;

    pub fn new(llm: Arc<dyn LlmProvider>, preserve_recent_turns: usize) -> Self;

    pub fn with_max_summary_tokens(
        llm: Arc<dyn LlmProvider>,
        preserve_recent_turns: usize,
        max_summary_tokens: usize,
    ) -> Self;
}
```

**Algorithm**:
1. Same zone separation as `SlidingWindowCompactor`.
2. Collect compactable middle messages.
3. Build summarization prompt with required sections:
   - Decisions
   - Files modified
   - Task state
   - Key context
4. Call `llm.generate()`.
5. Replace compactable zone with single structured summary message.
6. If LLM call fails (timeout, rate limit, provider error), return:
   `Err(CompactionError::SummarizationFailed { source })`.

**Failure handling contract**: `SummarizingCompactor` does not silently fall back. The caller (`LoopEngine`) decides whether to retry with `SlidingWindowCompactor`.

### 3.5 CompactionConfig

```rust
#[derive(Debug, Clone, thiserror::Error)]
pub enum CompactionConfigError {
    #[error("compaction_threshold must be in (0.0, 1.0], got {0}")]
    InvalidThreshold(f32),
    #[error("model_context_limit must be > 0")]
    ZeroContextLimit,
    #[error("reserved_system_tokens ({reserved}) must be < model_context_limit ({limit})")]
    ReservedExceedsLimit { reserved: usize, limit: usize },
    #[error("preserve_recent_turns must be > 0")]
    ZeroPreserveRecent,
    #[error("recompact_cooldown_turns must be > 0")]
    ZeroRecompactCooldown,
    #[error("max_summary_tokens must be > 0")]
    ZeroMaxSummaryTokens,
    #[error(
        "conversation budget too small ({available_tokens}) for preserve_recent_turns={preserve_recent_turns}; minimum required {min_required_tokens} to avoid compaction thrash"
    )]
    ConversationBudgetTooSmall {
        available_tokens: usize,
        preserve_recent_turns: usize,
        min_required_tokens: usize,
    },
}

/// Configuration for conversation compaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionConfig {
    /// Fraction of model context window that triggers compaction (0.0..1.0].
    /// Default: 0.80
    pub compaction_threshold: f32,
    /// Number of recent turns to always preserve verbatim.
    /// Default: 6 (3 user/assistant pairs)
    pub preserve_recent_turns: usize,
    /// Model context window size in tokens.
    /// Default: 128_000
    pub model_context_limit: usize,
    /// Reserved tokens for system prompt + memory context.
    /// Default: 2_000
    pub reserved_system_tokens: usize,
    /// Minimum number of turns between compaction passes for the same scope
    /// (`perceive`, `tool_continuation`, or `decompose_child`) unless a
    /// hard-limit check requires immediate compaction.
    /// Default: 2
    pub recompact_cooldown_turns: u32,
    /// Whether to use LLM-powered summarization.
    /// Default: false (safe baseline)
    pub use_summarization: bool,
    /// Max token budget for generated summary content.
    /// Default: 1_024
    pub max_summary_tokens: usize,
}

impl CompactionConfig {
    pub fn validate(&self) -> Result<(), CompactionConfigError> {
        if !(0.0 < self.compaction_threshold && self.compaction_threshold <= 1.0) {
            return Err(CompactionConfigError::InvalidThreshold(self.compaction_threshold));
        }
        if self.model_context_limit == 0 {
            return Err(CompactionConfigError::ZeroContextLimit);
        }
        if self.reserved_system_tokens >= self.model_context_limit {
            return Err(CompactionConfigError::ReservedExceedsLimit {
                reserved: self.reserved_system_tokens,
                limit: self.model_context_limit,
            });
        }
        if self.preserve_recent_turns == 0 {
            return Err(CompactionConfigError::ZeroPreserveRecent);
        }
        if self.recompact_cooldown_turns == 0 {
            return Err(CompactionConfigError::ZeroRecompactCooldown);
        }
        if self.max_summary_tokens == 0 {
            return Err(CompactionConfigError::ZeroMaxSummaryTokens);
        }

        let available_tokens = self.model_context_limit.saturating_sub(
            self.reserved_system_tokens + ConversationBudget::DEFAULT_OUTPUT_RESERVE_TOKENS,
        );
        // Conservative floor: preserved recent turns + room for two new turns.
        // 120 tokens/turn is intentionally low to keep this a minimal safety gate.
        let min_required_tokens = (self.preserve_recent_turns + 2) * 120;
        if available_tokens < min_required_tokens {
            return Err(CompactionConfigError::ConversationBudgetTooSmall {
                available_tokens,
                preserve_recent_turns: self.preserve_recent_turns,
                min_required_tokens,
            });
        }

        Ok(())
    }

    pub fn build_strategy(
        &self,
        llm: Option<Arc<dyn LlmProvider>>,
    ) -> Box<dyn CompactionStrategy> {
        if self.use_summarization {
            if let Some(provider) = llm {
                Box::new(SummarizingCompactor::with_max_summary_tokens(
                    provider,
                    self.preserve_recent_turns,
                    self.max_summary_tokens,
                ))
            } else {
                tracing::warn!(
                    "use_summarization=true but no llm provider available; falling back to SlidingWindowCompactor"
                );
                Box::new(SlidingWindowCompactor::new(self.preserve_recent_turns))
            }
        } else {
            Box::new(SlidingWindowCompactor::new(self.preserve_recent_turns))
        }
    }
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            compaction_threshold: 0.80,
            preserve_recent_turns: 6,
            model_context_limit: 128_000,
            reserved_system_tokens: 2_000,
            recompact_cooldown_turns: 2,
            use_summarization: false,
            max_summary_tokens: SummarizingCompactor::DEFAULT_MAX_SUMMARY_TOKENS,
        }
    }
}
```

### 3.6 `LoopError` mapping contract (struct-based)

`LoopError` in `engine/crates/fx-kernel/src/types.rs` is currently a **struct**, not an enum:

```rust
pub struct LoopError {
    pub stage: String,
    pub reason: String,
    pub recoverable: bool,
}
```

This spec does **not** require a struct→enum migration. Compaction failures should map into the
existing shape via the existing helper:

```rust
fn loop_error(stage: &str, reason: &str, recoverable: bool) -> LoopError;
```

Recommended stable reason-code prefixes (useful for tests and ops parsing):
- `invalid_compaction_config: {error}`
- `compaction_failed: scope={scope} error={error}`
- `context_exceeded_after_compaction: scope={scope} estimated_tokens={estimated_tokens} hard_limit_tokens={hard_limit_tokens}`

`context_exceeded_after_compaction` is the explicit failure path when compaction cannot reduce
context below the hard limit.

---

## 4. Implementation Plan

### Phase 1: Token counting and budget tracking (foundation)

1. Create `conversation_compactor.rs` with `ConversationBudget`, `CompactionConfig`, and validation/factory APIs.
2. Extract/centralize token estimation logic with behavior parity:
   - `estimate_text_tokens(text: &str) -> usize`
   - `estimate_tokens(text: &str) -> u64`
   - shared formula: `max(chars / 4, words)`
3. Add `ConversationBudget::needs_compaction()`, `exceeds_hard_limit()`, and
   `conversation_budget()` using `limit - reserved - output_reserve`.
4. Set `ConversationBudget::output_reserve_tokens` default to `4096`.
5. Add anti-thrashing safety validation: reject unusably small effective conversation budgets (`preserve_recent_turns` + 2-turn floor).
6. Tests: budget math, threshold detection, formula parity, and tight-budget rejection behavior.

### Phase 2: SlidingWindowCompactor (zero-risk baseline)

1. Implement `CompactionStrategy` returning `Result<CompactionResult, CompactionError>`.
2. Implement `SlidingWindowCompactor` with system/recent/tool-chain protection.
3. Preserve prior compaction markers as system-like messages.
4. Return `Err(CompactionError::AllMessagesProtected)` when no further compaction is possible.
5. Wire into `LoopEngine` `perceive()`, `act_with_tools()`, and the decomposition path in `run_sub_goal()` before calling `build_sub_goal_snapshot()`.

### Phase 3: SummarizingCompactor (opt-in)

1. Implement `SummarizingCompactor` on the same trait.
2. Return `Err(CompactionError::SummarizationFailed { source })` on LLM failure.
3. Let caller decide fallback behavior (e.g., retry with `SlidingWindowCompactor`).
4. Add explicit **Phase 3** annotation in flow/wiring docs where summarization path is used.

### Phase 4: Model-aware context limits

1. Extend config plumbing to read context limits from `fx-llm::ModelCatalog` (source of truth) when available, with explicit fallback to config value.
2. Ensure request builders consistently deduct reserved + output reserve tokens.
3. Keep this lookup in Phase 4 only; Phases 1-3 continue using the explicit config/default value.

### 4.5 Integration wiring in `LoopEngine`


**Decomposition call-site note**: `build_sub_goal_snapshot()` is a free function (no `&self`), so compaction runs in its caller `run_sub_goal()` before the snapshot is built.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum CompactionScope {
    Perceive,
    ToolContinuation,
    DecomposeChild,
}

impl CompactionScope {
    fn as_str(self) -> &'static str {
        match self {
            Self::Perceive => "perceive",
            Self::ToolContinuation => "tool_continuation",
            Self::DecomposeChild => "decompose_child",
        }
    }
}

pub fn new(
    /* existing positional params unchanged */
) -> Self {
    // Backward-compatible constructor: existing call sites stay positional.
    // Uses default compaction config + no summarization provider.
    match Self::new_with_compaction(/* existing args */, None, None) {
        Ok(engine) => engine,
        Err(error) => panic!("default compaction config must be valid: {error:?}"),
    }
}

pub fn new_with_compaction(
    /* existing positional params */,
    compaction_config: Option<CompactionConfig>,
    llm: Option<Arc<dyn LlmProvider>>,
) -> Result<Self, LoopError> {
    // Phase 1: validate config + initialize budget.
    let compaction_config = compaction_config.unwrap_or_default();
    compaction_config.validate().map_err(|error| {
        loop_error(
            "init",
            &format!("invalid_compaction_config: {error}"),
            false,
        )
    })?;

    let conversation_budget = ConversationBudget::new(
        compaction_config.model_context_limit,
        compaction_config.compaction_threshold,
        compaction_config.reserved_system_tokens,
    );

    // Phase 2: wire baseline strategy (sliding window).
    // Phase 3: summarization path is selected by config inside build_strategy(...).
    let conversation_compactor = compaction_config.build_strategy(llm.clone());

    Ok(Self {
        compaction_config,
        conversation_budget,
        conversation_compactor,
        // Interior mutability keeps `compact_if_needed` on `&self`.
        compaction_last_iteration: Mutex::new(HashMap::new()),
        // ...
    })
}

/// Returns true when compaction for this scope is still inside cooldown.
fn compaction_cooldown_active(
    &self,
    scope: CompactionScope,
    iteration: u32,
    cooldown_turns: u32,
) -> bool {
    let map = self
        .compaction_last_iteration
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    map.get(&scope)
        .map(|last| iteration.saturating_sub(*last) < cooldown_turns)
        .unwrap_or(false)
}

/// Records that compaction ran for this scope/iteration.
fn record_compaction_iteration(&self, scope: CompactionScope, iteration: u32) {
    let mut map = self
        .compaction_last_iteration
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    map.insert(scope, iteration);
}


fn compaction_failed_error(scope: CompactionScope, error: CompactionError) -> LoopError {
    loop_error(
        "compaction",
        &format!(
            "compaction_failed: scope={} error={}",
            scope.as_str(),
            error
        ),
        true,
    )
}

fn context_exceeded_after_compaction_error(
    scope: CompactionScope,
    estimated_tokens: usize,
    hard_limit_tokens: usize,
) -> LoopError {
    loop_error(
        "compaction",
        &format!(
            "context_exceeded_after_compaction: scope={} estimated_tokens={} hard_limit_tokens={}",
            scope.as_str(),
            estimated_tokens,
            hard_limit_tokens
        ),
        true,
    )
}

async fn compact_if_needed(
    &self,
    messages: &[Message],
    scope: CompactionScope,
    iteration: u32,
) -> Result<Vec<Message>, LoopError> {
    if !self.conversation_budget.needs_compaction(messages) {
        return Ok(messages.to_vec());
    }

    let before = ConversationBudget::estimate_tokens(messages);
    let hard_limit_exceeded = self.conversation_budget.exceeds_hard_limit(messages);

    let cooldown_active = self.compaction_cooldown_active(
        scope,
        iteration,
        self.compaction_config.recompact_cooldown_turns,
    );

    if cooldown_active && !hard_limit_exceeded {
        tracing::debug!(
            scope = scope.as_str(),
            iteration,
            cooldown_turns = self.compaction_config.recompact_cooldown_turns,
            "compaction skipped due to cooldown guard"
        );
        return Ok(messages.to_vec());
    }

    if cooldown_active && hard_limit_exceeded {
        tracing::warn!(
            scope = scope.as_str(),
            iteration,
            cooldown_turns = self.compaction_config.recompact_cooldown_turns,
            "cooldown bypassed because conversation is above hard limit"
        );
    }

    let target = self.conversation_budget.compaction_target();

    let result = match self.conversation_compactor.compact(messages, target).await {
        Ok(result) => result,
        Err(CompactionError::SummarizationFailed { source }) => {
            tracing::warn!(
                error = %source,
                scope = scope.as_str(),
                "summarization compaction failed; trying sliding fallback"
            );
            let fallback = SlidingWindowCompactor::new(self.compaction_config.preserve_recent_turns);
            fallback
                .compact(messages, target)
                .await
                .map_err(|error| compaction_failed_error(scope, error))?
        }
        Err(CompactionError::AllMessagesProtected) => {
            if hard_limit_exceeded {
                return Err(context_exceeded_after_compaction_error(
                    scope,
                    before,
                    self.conversation_budget.conversation_budget(),
                ));
            }
            return Ok(messages.to_vec());
        }
        Err(error) => return Err(compaction_failed_error(scope, error)),
    };

    if self.conversation_budget.exceeds_hard_limit(&result.messages) {
        return Err(context_exceeded_after_compaction_error(
            scope,
            result.estimated_tokens,
            self.conversation_budget.conversation_budget(),
        ));
    }

    let saved = before.saturating_sub(result.estimated_tokens);
    tracing::info!(
        scope = scope.as_str(),
        strategy = if result.used_summarization { "summarizing" } else { "sliding_window" },
        before_tokens = before,
        after_tokens = result.estimated_tokens,
        target_tokens = target,
        messages_removed = result.compacted_count,
        tokens_saved = saved,
        "conversation compaction triggered"
    );

    self.record_compaction_iteration(scope, iteration);
    Ok(result.messages)
}

fn ensure_within_hard_limit(
    &self,
    scope: CompactionScope,
    messages: &[Message],
) -> Result<(), LoopError> {
    let estimated_tokens = ConversationBudget::estimate_tokens(messages);
    let hard_limit_tokens = self.conversation_budget.conversation_budget();
    if estimated_tokens > hard_limit_tokens {
        return Err(context_exceeded_after_compaction_error(
            scope,
            estimated_tokens,
            hard_limit_tokens,
        ));
    }
    Ok(())
}
```

**Mutability note**: `compact_if_needed` remains `&self`; cooldown state is stored in
`Mutex<HashMap<CompactionScope, u32>>` via interior mutability.

### 4.5.1 Cooldown state contract (explicit)

- Storage: `LoopEngine.compaction_last_iteration: Mutex<HashMap<CompactionScope, u32>>`.
- Key: one entry per scope (`Perceive`, `ToolContinuation`, `DecomposeChild`).
- Read path: `compaction_cooldown_active(scope, iteration, cooldown_turns)` returns true when
  `iteration - last_iteration < cooldown_turns`.
- Write path: `record_compaction_iteration(scope, iteration)` runs only after a successful
  compaction pass (not on skipped/error attempts).
- Missing map entry means no prior compaction for that scope (cooldown inactive).

### 4.6 Constructor evolution strategy (definitive)

- `LoopEngine::new(...)` stays positional and remains available for all existing call sites.
- New API: `LoopEngine::new_with_compaction(..., compaction_config: Option<CompactionConfig>, llm: Option<Arc<dyn LlmProvider>>) -> Result<Self, LoopError>`.
- No builder pattern in this spec.
- Migration rule: callers that need non-default compaction must opt into `new_with_compaction`; all others remain unchanged.

### 4.7 Ordering contract with existing `append_compacted_summary()`

In `perceive()`, ordering is explicit and fixed:

1. Build `context_window` from `conversation_history` + new user turn.
2. Run conversation compaction (`compact_if_needed`) on that window.
3. Then run existing `append_compacted_summary()` for `ReasoningContext` compaction.

Rationale: this prevents a newly-added ReasoningContext summary from being compacted away on the same turn.
On subsequent turns, existing summaries are treated as system-like/protected content.

### 4.8 Observability contract

Whenever compaction executes, emit one structured `info!` log with at least:
- `scope` (`perceive`, `tool_continuation`, `decompose_child`)
- `strategy` (`sliding_window` or `summarizing`)
- `before_tokens`, `after_tokens`, `target_tokens`, `tokens_saved`
- `messages_removed`

Also emit:
- `warn!` on summarization fallback,
- `warn!` when cooldown is bypassed because the hard limit is exceeded,
- `debug!` when cooldown skips a compaction pass,
- and return a `LoopError` with reason prefix `context_exceeded_after_compaction:`
  when compaction cannot reduce context below the hard limit.

### 4.9 Request-builder hard-limit validation ownership

`build_reasoning_request()` and `build_continuation_request()` remain free functions that return
`CompletionRequest` directly and do not receive a `ConversationBudget` parameter.

Hard-limit validation therefore happens in the callers:
- `perceive()`: call `ensure_within_hard_limit(CompactionScope::Perceive, &context_window)?`
  immediately before `build_reasoning_request(...)`.
- `act_with_tools()`: call
  `ensure_within_hard_limit(CompactionScope::ToolContinuation, &state.continuation_messages)?`
  immediately before `build_continuation_request(...)`.

If validation fails, return `LoopError { stage: "compaction", reason: "context_exceeded_after_compaction: ...", recoverable: true }` and skip the outbound provider call.

---

## 5. Test Plan

### 5.1 ConversationBudget tests

| Test | Assertion |
|------|-----------|
| `budget_with_default_config_has_expected_values` | Default threshold, limit, reserves are wired correctly |
| `conversation_budget_subtracts_reserved_and_output_reserve` | `limit - reserved - output_reserve` exact behavior |
| `needs_compaction_returns_false_below_threshold` | 50% filled → false |
| `needs_compaction_returns_true_at_threshold` | threshold reached → true |
| `needs_compaction_returns_true_above_threshold` | above threshold → true |
| `exceeds_hard_limit_returns_false_within_budget` | Messages at/under conversation budget → false |
| `exceeds_hard_limit_returns_true_above_budget` | Messages over conversation budget → true |
| `estimate_tokens_empty_messages_returns_zero` | `[]` → 0 |
| `estimate_tokens_matches_existing_heuristic` | Matches `max(chars/4, words)` behavior |

### 5.2 SlidingWindowCompactor tests

| Test | Assertion |
|------|-----------|
| `compact_below_target_is_noop` | Messages under budget unchanged |
| `compact_preserves_recent_turns` | Last N turns always present |
| `compact_preserves_system_messages` | System-role messages never removed |
| `compact_preserves_prior_compaction_markers` | Existing compaction markers preserved as system-like |
| `compact_drops_oldest_middle_turns_first` | Oldest compactable content removed first |
| `compact_inserts_truncation_marker` | Marker inserted at truncation boundary |
| `compact_preserves_active_tool_chain` | In-flight ToolUse/ToolResult not split |
| `compact_handles_empty_history` | `[]` → `[]`, no panic |
| `compact_handles_single_message` | Single message preserved |
| `compact_all_messages_protected_returns_error` | Returns `Err(AllMessagesProtected)` |
| `compact_large_tool_result_removed_when_not_active` | Completed old tool payloads can be dropped |
| `compact_result_reports_correct_counts` | Metrics fields are accurate |

### 5.3 SummarizingCompactor tests

| Test | Assertion |
|------|-----------|
| `summarize_produces_structured_output` | Mock LLM summary inserted as one message |
| `summarize_returns_summarization_failed_on_llm_error` | Error mapped to `SummarizationFailed` |
| `summarize_returns_summarization_failed_on_timeout` | Timeout mapped to `SummarizationFailed` |
| `summarize_respects_target_budget` | Output fits target tokens |
| `summary_preserves_key_context_categories` | Decisions/files/task/key-context sections present |

### 5.4 Integration tests (LoopEngine level)

| Test | Assertion |
|------|-----------|
| `long_conversation_triggers_compaction_in_perceive` | History exceeding threshold is compacted |
| `tool_rounds_compact_continuation_messages` | Continuation compaction occurs between rounds |
| `decompose_child_receives_compacted_context` | Child receives compacted context, not unbounded clone |
| `perceive_orders_compaction_before_reasoning_summary` | Verifies `compact_if_needed()` runs before `append_compacted_summary()` |
| `cooldown_skips_compaction_when_within_window` | Second compaction attempt in same scope/within cooldown is skipped |
| `cooldown_allows_compaction_after_window_elapsed` | Compaction resumes once cooldown turns have elapsed |
| `cooldown_bypasses_when_hard_limit_exceeded` | Hard-limit breach overrides cooldown and forces compaction attempt |
| `all_messages_protected_over_hard_limit_returns_context_exceeded` | `AllMessagesProtected` + hard-limit breach maps to `LoopError { stage: "compaction", reason starts_with "context_exceeded_after_compaction:" }` |
| `compaction_preserves_session_coherence` | After compaction, message list contains: system prompt, compaction marker, and last N turns in original order |
| `compaction_coexists_with_existing_context_compactor` | Conversation compaction and ReasoningContext compaction coexist cleanly |
| `compaction_emits_observability_fields` | Log event includes scope/strategy/before/after/target/saved/removed fields |

### 5.5 Edge case tests

| Test | Assertion |
|------|-----------|
| `mid_tool_call_compaction_preserves_in_flight_calls` | Compaction during tool loop keeps active chains intact |
| `compaction_with_all_protected_messages` | Under hard limit: no panic + original messages; over hard limit: caller returns `LoopError` with reason prefix `context_exceeded_after_compaction:` |
| `compaction_with_only_tool_messages` | Behavior is valid and deterministic |
| `concurrent_decompose_children_each_compact_independently` | Children compact independent message copies |

### 5.6 CompactionConfig validation tests

| Test | Assertion |
|------|-----------|
| `config_rejects_threshold_above_one` | `validate()` fails when threshold > 1.0 |
| `config_rejects_zero_context_limit` | `validate()` fails when limit == 0 |
| `config_rejects_reserved_exceeding_limit` | `validate()` fails when reserved >= limit |
| `config_rejects_zero_preserve` | `validate()` fails when preserve_recent_turns == 0 |
| `config_rejects_zero_recompact_cooldown` | `validate()` fails when recompact_cooldown_turns == 0 |
| `config_rejects_zero_max_summary_tokens` | `validate()` fails when max_summary_tokens == 0 |
| `config_rejects_tight_budget_that_would_thrash` | `validate()` fails when effective budget cannot hold preserved turns + 2-turn floor |

---

## 6. Edge Cases and Risks

### 6.1 Compacting mid-tool-call

**Risk**: During `act_with_tools()`, compaction could remove a `ToolUse` message while keeping its `ToolResult` (or vice versa), creating invalid API message sequencing.

**Mitigation**:
- Active tool chains are marked protected.
- Post-compaction validation ensures no orphaned tool IDs.
- If everything is protected, `SlidingWindowCompactor` returns `AllMessagesProtected`; caller
  keeps messages only when still under hard limit, otherwise returns
  `LoopError` with reason prefix `context_exceeded_after_compaction:`.

### 6.2 Losing critical context

**Risk**: An important instruction from older turns could be removed by sliding-window truncation.

**Mitigation**:
- `SummarizingCompactor` preserves decisions and key state in structured output.
- Recent turns remain verbatim.
- Compaction marker signals truncation occurred.
- Persistent instruction memory remains the long-term source of truth.

### 6.3 Summary hallucination

**Risk**: LLM summarization could inject incorrect facts.

**Mitigation**:
- Summarization is opt-in.
- Prompt constrains output to explicit categories and source-grounded facts.
- Summary is assistant context (not system authority).
- On provider failure, strategy returns `SummarizationFailed`; caller can choose fallback.

### 6.4 Token estimation inaccuracy

**Risk**: Heuristic mismatch with provider tokenization can still produce occasional over/under errors.

**Mitigation**:
- Exact heuristic remains `max(chars / 4, words)` for parity.
- Threshold buffer and output reserve reduce hard-limit risk.
- Errors still propagate through existing `LoopError` handling.

### 6.5 Multimodal accounting gap (known limitation)

**Risk**: Image/multimodal tokens are not currently represented in conversation history estimation.

**Mitigation / TODO**:
- Explicitly document as out-of-scope for this phase.
- Add follow-up task to extend history representation + tokenizer accounting for multimodal content.

### 6.6 Compaction of sub-goal context

**Risk**: Parent and child compaction may cascade in decomposition trees.

**Mitigation**:
- Compaction is idempotent for already-compact windows.
- Child receives copied messages; no shared mutable state.

### 6.7 Concurrent decomposition safety

Each child engine compacts its own `Vec<Message>` clone. No shared mutable compaction state, so concurrent child compaction is safe by construction.

### 6.8 Compaction thrashing from overly small budgets

**Risk**: Misconfigured context limits can force compaction on nearly every turn.

**Mitigation**:
- Config validation rejects budgets below a minimum usable floor (`preserve_recent_turns` + room for two new turns).
- Runtime cooldown (`recompact_cooldown_turns`) prevents immediate back-to-back compaction in the same scope.
- Structured logs expose repeated compaction so operators can detect and fix bad limits quickly.

### 6.9 Post-compaction still over hard limit

**Risk**: Even after compaction (or when `AllMessagesProtected`), prompt history can remain larger
than `conversation_budget()`.

**Mitigation / contract**:
- `compact_if_needed()` (or caller preflight via `ensure_within_hard_limit(...)`) returns a
  `LoopError` with reason prefix `context_exceeded_after_compaction:` whenever hard-limit
  compliance cannot be achieved.
- `build_reasoning_request()` and `build_continuation_request()` are only called after that
  preflight succeeds; otherwise the outbound LLM call is skipped for that turn.
- The reason string includes `scope`, `estimated_tokens`, and `hard_limit_tokens` for operator debugging.

---

## 7. Estimated Complexity

| Phase | Scope | Estimated effort | Risk |
|-------|-------|------------------|------|
| Phase 1: Budget + validation + token DRYing | ~180 lines code, ~70 lines tests | Small (0.5-1 day) | Minimal |
| Phase 2: SlidingWindowCompactor + wiring | ~250 lines code, ~300 lines tests | Medium (1-2 days) | Low |
| Phase 3: SummarizingCompactor + error contracts | ~220 lines code, ~220 lines tests | Medium (1-2 days) | Moderate |
| Phase 4: Model-aware limits | ~100 lines modified code, ~50 lines tests | Small (0.5 day) | Low |

**Total**: ~750-900 code lines + substantial test additions. Roughly 4-5 focused implementation days.

**Crate count**: 0 new crates. All code in `fx-kernel`.

**Breaking changes**:
- `CompactionStrategy::compact()` returns `Result<_, CompactionError>` and does not accept a runtime `preserve_recent` parameter.
- `LoopEngine::new_with_compaction(...) -> Result<Self, LoopError>` is added for explicit config injection.
- `LoopEngine::new(...)` remains positional/backward-compatible and delegates to defaults.

---

## Appendix: Message Flow Diagram

```
User input
    │
    ▼
perceive()
    │
    ├── conversation_history.clone() → context_window
    ├── context_window.push(user_message)
    ├── [NEW] compact_if_needed(context_window, budget)
    │         ├── Phase 2: SlidingWindowCompactor (default)
    │         └── Phase 3: SummarizingCompactor (opt-in)
    ├── [NEW] ensure_within_hard_limit(Perceive, context_window)
    ├── append_compacted_summary()  (existing ReasoningContext compaction; runs AFTER conversation compaction)
    │
    ▼
build_reasoning_request()
    │
    ├── system_prompt (always retained)
    ├── memory_context (always retained)
    ├── context_window (potentially compacted)
    │
    ▼
LLM call → CompletionResponse
    │
    ├── Decision::Respond → done
    ├── Decision::UseTools → act_with_tools()
    │       │
    │       ├── execute_tool_calls()
    │       ├── append_tool_round_messages()
    │       ├── [NEW] compact_if_needed(continuation_messages)
    │       │         ├── Phase 2 default
    │       │         └── Phase 3 optional summarization
    │       ├── [NEW] ensure_within_hard_limit(ToolContinuation, continuation_messages)
    │       ├── request_tool_continuation()
    │       └── loop until no more tool calls
    │
    └── Decision::Decompose → execute_decomposition()
            │
            ├── run_sub_goal()
            │   ├── [NEW] compact_if_needed(context_messages, CompactionScope::DecomposeChild, iteration)
            │   │         ├── Phase 2 default
            │   │         └── Phase 3 optional summarization
            │   └── build_sub_goal_snapshot(compacted_context_messages)
            └── child LoopEngine::run_cycle()
```
