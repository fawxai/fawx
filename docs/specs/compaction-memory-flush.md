# Spec: Pre-Compaction Memory Flush

**Status**: Ready for implementation  
**Crates touched**: `fx-kernel`, `fx-journal`, `fx-cli`  
**Estimated scope**: ~200 lines production code + ~150 lines tests

---

## Problem

When conversation compaction fires, messages are permanently dropped from the
context window. Their content — decisions made, files modified, task progress,
key context — vanishes. The journal (fx-journal) exists for persistent
cross-session memory, but compaction doesn't write to it. Long conversations
silently lose information.

## Solution

Add an optional **pre-compaction memory flush** hook to the loop engine. Before
compaction drops messages, extract their text content and persist it to the
journal as a structured entry. This makes compaction non-lossy: evicted
messages become searchable journal entries.

---

## Design

### 1. New trait in `fx-kernel` (`conversation_compactor.rs`)

```rust
/// Persists evicted message content before compaction drops them.
///
/// Implementations write to a durable store (e.g., journal) so that
/// information from dropped messages survives across sessions.
#[async_trait]
pub trait CompactionMemoryFlush: Send + Sync + std::fmt::Debug {
    /// Flush content from messages about to be evicted.
    ///
    /// `evicted` contains only the messages that will be removed —
    /// NOT the full conversation, NOT the retained messages.
    /// `scope` identifies which compaction phase triggered this.
    async fn flush(
        &self,
        evicted: &[Message],
        scope: CompactionScope,
    ) -> Result<(), CompactionFlushError>;
}

#[derive(Debug, thiserror::Error)]
pub enum CompactionFlushError {
    #[error("memory flush failed: {reason}")]
    FlushFailed { reason: String },
}
```

### 2. Wire into `LoopEngine` (`loop_engine.rs`)

Add to `LoopEngine` struct:
```rust
memory_flush: Option<Arc<dyn CompactionMemoryFlush>>,
```

Add to `LoopEngineBuilder`:
```rust
memory_flush: Option<Arc<dyn CompactionMemoryFlush>>,

pub fn memory_flush(mut self, flush: Arc<dyn CompactionMemoryFlush>) -> Self {
    self.memory_flush = Some(flush);
    self
}
```

Wire in `build()`: pass through to engine struct. No validation needed (it's optional).

### 3. Call flush from `compact_if_needed` (`loop_engine.rs`)

In `compact_if_needed`, **after** running the compaction strategy but **before**
returning the compacted messages, identify evicted messages and flush them:

```rust
// Inside compact_if_needed, after run_compaction_strategy succeeds:
if result.compacted_count > 0 {
    if let Some(flush) = &self.memory_flush {
        let evicted = identify_evicted_messages(messages, &result.messages);
        if !evicted.is_empty() {
            if let Err(err) = flush.flush(&evicted, scope).await {
                // Non-fatal: log warning but proceed with compaction.
                // Losing the flush is better than blocking the conversation.
                tracing::warn!(
                    scope = scope.as_str(),
                    error = %err,
                    evicted_count = evicted.len(),
                    "pre-compaction memory flush failed; proceeding without flush"
                );
            }
        }
    }
}
```

**`identify_evicted_messages`**: Compare the original `messages` slice against
`result.messages`. Messages present in the original but absent in the result
(excluding the compaction marker and summary messages) are the eviction set.

Implementation approach — positional, not content-matching:
- The compaction strategies preserve prefix (system messages), protect tool
  chains, and keep the tail (recent turns). Evicted messages are the unprotected
  middle that got removed.
- Use the same `zone_bounds` + `protected_middle_indices` logic already in
  `conversation_compactor.rs` to identify the eviction set. However, since
  `compact_if_needed` doesn't have direct access to those internals, the
  simplest correct approach is:
  - Build a `HashSet` of message references (by index or identity) in the
    result.
  - Messages in the original that aren't in the result and aren't compaction
    markers are evicted.
- Since messages don't implement `Hash`/`Eq` by identity, use index-based
  tracking: have the `CompactionResult` carry `evicted_indices: Vec<usize>`.

**Updated `CompactionResult`**:
```rust
pub struct CompactionResult {
    pub(crate) messages: Vec<Message>,
    pub(crate) compacted_count: usize,
    pub(crate) estimated_tokens: usize,
    pub(crate) used_summarization: bool,
    /// Indices (into the original message slice) of messages that were evicted.
    pub(crate) evicted_indices: Vec<usize>,
}
```

Both `SlidingWindowCompactor` and `SummarizingCompactor` already know which
messages they drop — populate `evicted_indices` from the existing logic:
- `SlidingWindowCompactor`: indices where `keep_middle[offset] == false`,
  mapped back to absolute indices via `bounds.prefix_end + offset`.
- `SummarizingCompactor`: the `summarizable_indices` vector already computed.

### 4. Implement `JournalCompactionFlush` in `fx-journal`

New file: `fx-journal/src/flush.rs`

```rust
use std::sync::{Arc, Mutex};
use async_trait::async_trait;
use fx_kernel::conversation_compactor::{CompactionMemoryFlush, CompactionFlushError};
use fx_kernel::loop_engine::CompactionScope;
use fx_llm::Message;
use crate::journal::Journal;

/// Flushes evicted conversation content to the journal before compaction.
#[derive(Debug)]
pub struct JournalCompactionFlush {
    journal: Arc<Mutex<Journal>>,
}

impl JournalCompactionFlush {
    pub fn new(journal: Arc<Mutex<Journal>>) -> Self {
        Self { journal }
    }
}

#[async_trait]
impl CompactionMemoryFlush for JournalCompactionFlush {
    async fn flush(
        &self,
        evicted: &[Message],
        scope: CompactionScope,
    ) -> Result<(), CompactionFlushError> {
        if evicted.is_empty() {
            return Ok(());
        }

        let content = format_evicted_messages(evicted);
        // Truncate to avoid bloating the journal with enormous tool outputs.
        let max_chars = 4_000;
        let truncated = if content.len() > max_chars {
            format!("{}...[truncated]", &content[..max_chars])
        } else {
            content
        };

        let context_text = format!(
            "Auto-flushed during {} compaction. {} messages evicted.",
            scope.as_str(),
            evicted.len(),
        );

        let mut journal = self.journal
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        journal.write(
            truncated,
            vec!["compaction-flush".to_string(), "auto".to_string()],
            "session-memory".to_string(),
            Some(context_text),
        ).map_err(|err| CompactionFlushError::FlushFailed {
            reason: err.to_string(),
        })?;

        Ok(())
    }
}

/// Extract text content from evicted messages, structured by role.
fn format_evicted_messages(messages: &[Message]) -> String {
    messages
        .iter()
        .map(|msg| {
            let role = match msg.role {
                fx_llm::MessageRole::System => "system",
                fx_llm::MessageRole::User => "user",
                fx_llm::MessageRole::Assistant => "assistant",
                fx_llm::MessageRole::Tool => "tool",
            };
            let text = msg.content.iter().filter_map(|block| {
                match block {
                    fx_llm::ContentBlock::Text { text } => Some(text.clone()),
                    fx_llm::ContentBlock::ToolUse { name, input, .. } => {
                        Some(format!("[tool:{name}] {input}"))
                    }
                    fx_llm::ContentBlock::ToolResult { content, .. } => {
                        // Truncate large tool results per-message
                        let s = content.to_string();
                        if s.len() > 500 {
                            Some(format!("{}...", &s[..500]))
                        } else {
                            Some(s)
                        }
                    }
                    fx_llm::ContentBlock::Image { .. } => Some("[image]".to_string()),
                }
            }).collect::<Vec<_>>().join(" ");
            format!("{role}: {text}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}
```

### 5. Wire in `fx-cli/src/startup.rs`

In `build_skill_registry` (around line 796-804 where the journal is loaded):

```rust
// Currently:
let journal_path = data_dir.join("journal.jsonl");
match fx_journal::Journal::load(journal_path) {
    Ok(journal) => {
        let journal = Arc::new(Mutex::new(journal));
        let journal_skill = JournalSkill::new(Arc::clone(&journal));
        registry.register(Arc::new(journal_skill));
        // NEW: store journal Arc for memory flush
        // Return it so it can be passed to the loop engine builder
    }
    Err(e) => tracing::warn!("journal unavailable: {e}"),
}
```

The journal `Arc<Mutex<Journal>>` needs to be accessible when building the
`LoopEngine`. Currently it's created inside `build_skill_registry` (line ~798
of `startup.rs`) and only the `JournalSkill` gets it.

**Concrete change**: Add `journal: Option<Arc<Mutex<fx_journal::Journal>>>` to
`SkillRegistryBundle` (line 685 of `startup.rs`). In the journal loading block
(line ~798), clone the Arc and store it in the bundle. Then in
`build_loop_engine_with_options` (line ~469), after building skills:

```rust
// After building skills, before building the engine:
if let Some(journal) = &skills.journal {
    let flush = Arc::new(fx_journal::flush::JournalCompactionFlush::new(Arc::clone(journal)));
    builder = builder.memory_flush(flush as Arc<dyn CompactionMemoryFlush>);
}
```

---

## What NOT to do

- **No LLM call in the flush.** Raw text extraction only. Zero additional cost.
  If we want LLM-summarized flushes later, that's a follow-up.
- **No blocking on flush failure.** Compaction must proceed even if the journal
  write fails. Log a warning and move on.
- **No separate file for evicted messages.** Journal is the single persistence
  layer. Don't create a parallel store.
- **No changes to CompactionStrategy trait.** The trait stays pure — it compacts.
  The flush hook lives in the loop engine, not inside strategies.

---

## Testing

### Unit tests in `fx-kernel` (conversation_compactor.rs)

1. `evicted_indices_populated_for_sliding_window` — Verify `CompactionResult.evicted_indices`
   contains correct absolute indices after sliding window compaction.
2. `evicted_indices_populated_for_summarizing` — Same for summarizing compactor.
3. `evicted_indices_empty_when_no_compaction_needed` — Below-threshold returns
   empty evicted_indices.

### Unit tests in `fx-kernel` (loop_engine.rs)

4. `compact_if_needed_calls_memory_flush` — Mock `CompactionMemoryFlush`, verify
   `flush()` called with correct evicted messages when compaction triggers.
5. `compact_if_needed_proceeds_on_flush_failure` — Mock flush that returns error,
   verify compaction still succeeds and returns compacted messages.
6. `compact_if_needed_skips_flush_when_none` — No flush configured, compaction
   works normally.

### Unit tests in `fx-journal` (flush.rs)

7. `journal_flush_writes_entry_with_correct_tags` — Flush evicted messages,
   verify journal entry has tags `["compaction-flush", "auto"]`.
8. `journal_flush_truncates_large_content` — Flush messages with total content
   >4000 chars, verify truncation with `...[truncated]` suffix.
9. `journal_flush_formats_roles_correctly` — Verify user/assistant/tool roles
   appear in the formatted output.
10. `journal_flush_handles_empty_eviction` — Empty slice returns Ok without
    writing to journal.
11. `journal_flush_truncates_large_tool_results` — Tool result >500 chars
    truncated in per-message formatting.

### Integration test

12. `compaction_with_flush_persists_to_journal` — Build a LoopEngine with
    journal flush, trigger compaction via large conversation, verify journal
    file contains the flushed entry.

---

## File changes summary

| File | Change |
|------|--------|
| `fx-kernel/src/conversation_compactor.rs` | Add `CompactionMemoryFlush` trait, `CompactionFlushError`, `evicted_indices` to `CompactionResult`, populate in both compactors |
| `fx-kernel/src/loop_engine.rs` | Make `CompactionScope` and `as_str()` public, add `memory_flush` field + builder method, call flush in `compact_if_needed` |
| `fx-journal/src/flush.rs` | New file: `JournalCompactionFlush` implementation |
| `fx-journal/src/lib.rs` | Add `pub mod flush;` export |
| `fx-cli/src/startup.rs` | Thread journal Arc through to loop engine builder, construct flush |

---

## Dependencies

- `fx-journal` already depends on `fx-kernel` (for `ToolCacheability`, `CancellationToken`).
  Adding the `CompactionMemoryFlush` trait import is consistent.
- `fx-kernel` gains no new dependencies. The trait is defined there; implementations live outside.
- `async-trait` already used in both crates.
