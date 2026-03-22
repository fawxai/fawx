# Spec: Conversation Context Management Fixes

**Date:** 2026-03-20
**Symptom:** After ~30+ turns in a session, Fawx responds with "What's your question?" — losing all conversational context.
**Root cause:** Three compounding issues in how session history reaches the LLM.

---

## Bug 1: Unbounded History Load

**File:** `engine/crates/fx-api/src/handlers/sessions.rs`
**Function:** `handle_send_message_for_session` (line ~253)

### Problem
```rust
let history = registry
    .history(&key, usize::MAX)  // loads ALL messages
    .map_err(|error| map_session_error(&id, error))?;
let context = session_messages_to_context(&history);
```

The HTTP session handler loads the entire conversation history from redb with no limit. A 308-message session with long structured responses (tables, code blocks, lineage trees) produces a context payload far exceeding any model's window.

The TUI/CLI path has `trim_history(&mut self.conversation_history, self.max_history)` with `max_history` defaulting to 20 messages. The HTTP path has no equivalent.

### Fix
Before passing history to `process_message_with_context`, apply the same trim the CLI uses:

```rust
let history = registry
    .history(&key, usize::MAX)
    .map_err(|error| map_session_error(&id, error))?;
let mut context = session_messages_to_context(&history);
// Apply same trim as CLI path
let max_history = {
    let app = state.app.lock().await;
    app.config().general.max_history
};
trim_history(&mut context, max_history);
```

This is the immediate fix. The kernel compactor is the proper long-term solution (Bug 3), but this stops the bleeding.

### Streaming path too
The same unbounded load happens in `stream_session_message_response`. Both paths need the fix.

---

## Bug 2: Static Context Limit in CompactionConfig

**File:** `engine/crates/fx-kernel/src/conversation_compactor.rs`
**Struct:** `CompactionConfig`

### Problem
```rust
impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            model_context_limit: 128_000,  // hardcoded
            // ...
        }
    }
}
```

The compaction budget is set once at `LoopEngine` build time and never updated. When the user switches models (e.g., from Claude Opus at 200K to GPT-5.4 at potentially different context), the compactor still thinks it has 128K tokens to work with.

### Fix
Add a method to update the context limit dynamically:

```rust
impl LoopEngine {
    pub fn update_context_limit(&mut self, new_limit: usize) {
        self.compaction_config.model_context_limit = new_limit;
        self.conversation_budget = ConversationBudget::new(
            new_limit,
            self.compaction_config.compaction_threshold,
            self.compaction_config.reserved_system_tokens,
        );
    }
}
```

Call this from `HeadlessApp` whenever the active model changes (in `switch_model`, `reload_providers`, etc.). The model catalog already knows each model's context window; wire that through.

### Where model context sizes live
`fx-llm/src/model_catalog.rs` has model metadata. Add a `context_window` field if not already present, and query it when the active model changes.

---

## Bug 3: Summarization Disabled by Default

**File:** `engine/crates/fx-kernel/src/conversation_compactor.rs`
**Default:** `use_summarization: false`

### Problem
With summarization off, compaction uses `SlidingWindowCompactor`, which simply drops old messages. When a user says "Proceed with 1 and 2" after a long conversation about experiment harnesses, the dropped messages contained all the context about what "1 and 2" refer to. The model sees a bare instruction with no preceding discussion and responds "What's your question?"

The `SummarizingCompactor` already exists, is tested, and produces structured summaries with sections: Decisions, Files modified, Task state, Key context. This is exactly what's needed to preserve conversational continuity.

### Fix
Change the default:

```rust
impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            use_summarization: true,  // was: false
            // ...
        }
    }
}
```

Also make it configurable in `config.toml`:

```toml
[general]
use_summarization = true
max_summary_tokens = 1024
```

### Fallback is already handled
The `run_compaction_strategy` method already falls back to sliding window if summarization fails (LLM error, summary too large). So enabling this is safe.

### Cost consideration
Summarization makes an extra LLM call during compaction. This is a tradeoff: one cheap summarization call vs. completely losing context. For sessions that reach compaction threshold, the cost is negligible compared to the main conversation.

---

## Verification

### Test case: reproduce the bug
1. Create a session
2. Send 30+ messages with long structured responses (tables, code blocks)
3. Send a short follow-up referencing earlier discussion ("Proceed with that plan")
4. **Before fix:** Response is "What's your question?" or similarly confused
5. **After fix:** Response references the earlier plan correctly

### What to check in code
- `handle_send_message_for_session` and its streaming variant apply history trimming
- `CompactionConfig.model_context_limit` updates when active model changes
- `use_summarization` defaults to `true`
- Existing compaction tests still pass
- New test: session with history exceeding context window produces a coherent response after compaction

---

## Priority
This is a **release blocker** for dogfooding. Users hitting context limits in normal conversation and getting nonsense responses will not continue using Fawx. Bug 1 is the fastest fix; Bug 3 is the most impactful.

## Implementation order
1. Bug 1 (trim history in HTTP path) — immediate relief
2. Bug 3 (enable summarization) — config change + verify fallback works
3. Bug 2 (dynamic context limit) — hardcoded lookup table for now since ModelInfo/ModelCatalog don't have context_window fields

## Scope: PR A (this spec)
All three bugs ship as one PR. They're tightly coupled: all affect the same code flow (session history → kernel → LLM call). The changes touch:
- `engine/crates/fx-api/src/handlers/sessions.rs` (Bug 1: trim history)
- `engine/crates/fx-kernel/src/conversation_compactor.rs` (Bug 3: enable summarization default)
- `engine/crates/fx-kernel/src/loop_engine.rs` (Bug 2: add `update_context_limit` method)
- `engine/crates/fx-cli/src/headless.rs` (Bug 2: call update on model switch)
- `engine/crates/fx-llm/src/router.rs` or `types.rs` (Bug 2: add context_window_for_model lookup)

## Context window lookup (Bug 2)
ModelInfo and ModelCatalog don't currently carry context window sizes. For this PR, add a simple lookup function:

```rust
pub fn context_window_for_model(model_id: &str) -> usize {
    // Known context windows as of 2026-03
    if model_id.contains("claude-opus") { return 200_000; }
    if model_id.contains("claude-sonnet") { return 200_000; }
    if model_id.contains("claude-haiku") { return 200_000; }
    if model_id.contains("gpt-5") { return 128_000; }
    if model_id.contains("gpt-4") { return 128_000; }
    // Conservative default
    128_000
}
```

This is a stopgap. A future PR should add `context_window` to ModelInfo and populate it from provider APIs.
