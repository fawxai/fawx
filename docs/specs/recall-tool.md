# Spec: Phase 4 — Recall Tool

**Status:** Ready for implementation  
**Author:** Clawdio  
**Date:** 2026-03-21  
**Crate:** `fx-journal`  
**Files:** `engine/crates/fx-journal/src/skill.rs`  
**Parent spec:** `docs/specs/long-session-context-management.md`

---

## Problem

When tiered compaction evicts messages, they're flushed to the journal tagged `compaction-flush`. But the agent has no dedicated way to search this evicted history. The existing `journal_search` tool searches ALL journal entries (lessons, insights, compaction flushes), which mixes conversation history with reflective memory.

The agent needs a focused tool to recall specific details from earlier in the session that were compacted away.

---

## Design

### Tool: recall_session_context

Add `recall_session_context` to `JournalSkill` (alongside `journal_write` and `journal_search`). This keeps all journal-related tools in one skill.

```json
{
  "name": "recall_session_context",
  "description": "Search evicted conversation history for details from earlier in this session. Use when you need to recall something specific that was discussed earlier but may have been compacted away. Searches only compaction-flushed entries, not general journal lessons.",
  "parameters": {
    "type": "object",
    "properties": {
      "query": {
        "type": "string",
        "description": "What to search for in evicted history"
      },
      "limit": {
        "type": "integer",
        "description": "Max results (default 5)"
      }
    },
    "required": ["query"]
  }
}
```

### Implementation

The tool calls `Journal::search()` with the `compaction-flush` tag filter, then formats results as conversation excerpts rather than journal entries.

```rust
fn handle_recall(&self, arguments: &str) -> Result<String, SkillError> {
    let args: RecallArgs = serde_json::from_str(arguments)
        .map_err(|e| format!("invalid arguments: {e}"))?;
    
    let journal = self.journal.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    
    let limit = args.limit.unwrap_or(5);
    let results = journal.search(
        &args.query,
        Some(vec!["compaction-flush".to_string()]),
        limit,
    );
    
    let entries: Vec<serde_json::Value> = results
        .into_iter()
        .map(|e| serde_json::json!({
            "context": e.context,
            "content": e.lesson,
            "timestamp": format_timestamp(e.timestamp),
        }))
        .collect();
    
    serde_json::to_string(&serde_json::json!({
        "count": entries.len(),
        "recalled": entries,
    }))
    .map_err(|e| format!("serialization failed: {e}"))
}
```

### Changes to JournalSkill

1. Add `recall_session_context` to `tool_definitions()` (now returns 3 tools)
2. Add `"recall_session_context"` match arm in `execute()`
3. Add `RecallArgs` struct
4. Add `handle_recall()` method

---

## Tests

1. **`skill_provides_three_tools`**: Update existing test — now 3 tools instead of 2
2. **`recall_finds_compaction_flush_entries`**: Write a compaction-flush entry, search via recall, verify found
3. **`recall_ignores_non_flush_entries`**: Write a regular journal entry, search via recall, verify NOT found
4. **`recall_returns_empty_when_no_matches`**: Search for nonexistent content, verify empty results
5. **`recall_respects_limit`**: Write multiple flush entries, search with limit=1, verify only 1 returned

---

## Non-goals

- Session-scoped recall (filtering by session ID) — all compaction flushes are searched
- Semantic search / embeddings — text substring matching is sufficient for now
- Recall from non-journal sources — only journal entries
