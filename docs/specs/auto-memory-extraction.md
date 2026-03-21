# Spec: Phase 5 — Automatic Memory Extraction During Compaction

**Status:** Ready for implementation  
**Author:** Clawdio  
**Date:** 2026-03-21  
**Crate:** `fx-kernel`  
**Primary file:** `engine/crates/fx-kernel/src/loop_engine.rs`  
**Parent spec:** `docs/specs/long-session-context-management.md`

---

## Problem

Phase 3 added `update_session_memory` as an agent-initiated tool. But the agent might not call it before compaction fires. If the agent forgets, key facts are lost — you're relying on Phase 4 recall to dig them back out of the journal.

Codex handles this transparently: memory extraction happens automatically during compaction, with zero agent involvement. We should do the same.

---

## Design

### When: During `flush_evicted()` in `finish_tier()`

The extraction runs in the same place where evicted messages are already flushed to the journal. After flushing to journal, extract key facts from the evicted messages and merge them into session memory.

### How: Simple LLM call with structured output

Use the `compaction_llm` (already on the engine builder) to extract structured facts from evicted messages. Parse the JSON response into a `SessionMemoryUpdate` and apply it.

### Changes to `LoopEngine`

#### 1. Store `compaction_llm` on the engine (not just in the strategy)

Currently `compaction_llm` is consumed by `build_compaction_components()` to build the strategy. We also need a reference on the engine for direct extraction calls.

Add field to `LoopEngine`:
```rust
/// LLM used for compaction-time memory extraction (Phase 5).
compaction_llm: Option<Arc<dyn LlmProvider>>,
```

Wire through builder:
```rust
// In LoopEngineBuilder, already has compaction_llm field
// In build(), clone the Arc before passing to build_compaction_components:
let compaction_llm_for_engine = self.compaction_llm.as_ref().map(Arc::clone);
// Pass original to build_compaction_components as before
// Store clone on engine:
compaction_llm: compaction_llm_for_engine,
```

#### 2. Add extraction method

```rust
/// Extract key facts from evicted messages and merge into session memory.
/// Best-effort: failures are logged but do not block compaction.
async fn extract_memory_from_evicted(&self, evicted: &[Message]) {
    let Some(llm) = &self.compaction_llm else {
        return;
    };

    if evicted.is_empty() {
        return;
    }

    let prompt = build_extraction_prompt(evicted);
    match llm.generate(&prompt, 512).await {
        Ok(response) => {
            if let Some(update) = parse_extraction_response(&response) {
                let mut memory = self.session_memory.lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if let Err(err) = memory.apply_update(update) {
                    tracing::warn!(
                        error = %err,
                        "auto-extracted memory update rejected (token cap)"
                    );
                }
            }
        }
        Err(err) => {
            tracing::warn!(
                error = %err,
                "memory extraction from evicted messages failed"
            );
        }
    }
}
```

#### 3. Wire into `flush_evicted()`

After the existing journal flush, call extraction:

```rust
async fn flush_evicted(
    &self,
    messages: &[Message],
    result: &CompactionResult,
    scope: CompactionScope,
) {
    if result.compacted_count == 0 {
        return;
    }

    // ... existing journal flush code ...

    // Phase 5: extract memory from evicted messages
    let evicted: Vec<Message> = result
        .evicted_indices
        .iter()
        .filter_map(|&index| messages.get(index).cloned())
        .collect();
    // (evicted already computed above for journal flush — reuse)
    self.extract_memory_from_evicted(&evicted).await;
}
```

Note: `flush_evicted` already computes the `evicted` vec for journal flush. Refactor to compute it once and pass to both the journal flush and memory extraction.

#### 4. Prompt and parser functions

```rust
fn build_extraction_prompt(messages: &[Message]) -> String {
    let formatted = messages
        .iter()
        .filter_map(|msg| {
            let role = match &msg.role {
                MessageRole::User => "user",
                MessageRole::Assistant => "assistant",
                MessageRole::System => return None, // skip system messages
                MessageRole::Tool => "tool",
            };
            let text: String = msg.content.iter()
                .filter_map(|block| match block {
                    ContentBlock::Text { text } => Some(text.clone()),
                    ContentBlock::ToolUse { name, .. } => Some(format!("[tool: {name}]")),
                    ContentBlock::ToolResult { content, .. } => {
                        let s = content.to_string();
                        Some(if s.len() > 200 { format!("{}...", &s[..200]) } else { s })
                    }
                    ContentBlock::Image { .. } => Some("[image]".to_string()),
                })
                .collect::<Vec<_>>()
                .join(" ");
            Some(format!("{role}: {text}"))
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        concat!(
            "Extract key facts from this conversation excerpt that is being removed from context.\n",
            "Return a JSON object with these optional fields:\n",
            "- \"project\": what the session is about (string, only if clearly identifiable)\n",
            "- \"current_state\": current state of work (string, only if clear)\n",
            "- \"key_decisions\": important decisions made (array of short strings)\n",
            "- \"active_files\": files being worked on (array of paths)\n",
            "- \"custom_context\": other important facts to remember (array of short strings)\n\n",
            "Only include fields where the conversation clearly contains relevant information.\n",
            "Keep each string under 100 characters. Return ONLY valid JSON, no markdown.\n\n",
            "Conversation:\n{}"
        ),
        formatted
    )
}

fn parse_extraction_response(response: &str) -> Option<SessionMemoryUpdate> {
    // Try to parse directly
    if let Ok(update) = serde_json::from_str::<SessionMemoryUpdate>(response) {
        return Some(update);
    }
    // Try extracting JSON from markdown code block
    let trimmed = response.trim();
    if let Some(json_start) = trimmed.find('{') {
        if let Some(json_end) = trimmed.rfind('}') {
            if let Ok(update) = serde_json::from_str::<SessionMemoryUpdate>(
                &trimmed[json_start..=json_end]
            ) {
                return Some(update);
            }
        }
    }
    tracing::warn!(
        response_len = response.len(),
        "failed to parse memory extraction response as JSON"
    );
    None
}
```

---

## Tests

### 1. `extract_memory_from_evicted_updates_session_memory`
Set up engine with a mock LLM that returns valid JSON. Call `extract_memory_from_evicted` with sample messages. Verify session memory was updated.

### 2. `extract_memory_skipped_without_compaction_llm`
Engine without `compaction_llm`. Call `extract_memory_from_evicted`. Verify no panic, no memory change.

### 3. `extract_memory_handles_llm_failure_gracefully`
Mock LLM that returns error. Call `extract_memory_from_evicted`. Verify no panic, no memory change.

### 4. `extract_memory_handles_malformed_response`
Mock LLM that returns non-JSON. Call `extract_memory_from_evicted`. Verify no panic, no memory change.

### 5. `extract_memory_respects_token_cap`
Mock LLM returns update that would exceed 2000 token cap. Verify memory is NOT updated (cap enforced).

### 6. `build_extraction_prompt_formats_messages`
Unit test on `build_extraction_prompt` — verify output format.

### 7. `parse_extraction_response_handles_code_block`
Verify parser handles `\`\`\`json\n{...}\n\`\`\`` wrapping.

### 8. `parse_extraction_response_returns_none_for_garbage`
Verify parser returns None for unparseable input.

### 9. `flush_evicted_triggers_extraction`
Integration test: engine with mock flush + mock LLM. Run finish_tier. Verify both journal flush AND memory extraction happened.

---

## Non-goals

- Using a different/faster model for extraction (reuses compaction_llm)
- Deduplication against existing session memory (apply_update handles append-capped lists)
- Configurable extraction (always runs when compaction_llm is available)
- Extraction during prune tier (only slide/summarize/emergency evict messages)
