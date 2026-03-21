# Spec: Phase 3 — Session Memory Extraction

**Status:** Ready for implementation  
**Author:** Clawdio  
**Date:** 2026-03-21  
**Crates:** `fx-session`, `fx-kernel`, `fx-journal`  
**Files:** `engine/crates/fx-session/src/session.rs`, `engine/crates/fx-session/src/store.rs`, `engine/crates/fx-kernel/src/loop_engine.rs`, `engine/crates/fx-journal/src/skill.rs`  
**Parent spec:** `docs/specs/long-session-context-management.md`

---

## Problem

When tiered compaction drops messages (Phase 2), the agent loses context about key decisions, active files, and session purpose. The agent may start asking "What are you working on?" after compaction because the dropped messages contained the entire project context.

Session memory makes compaction *lossless* by extracting key facts into a persistent, compact block that survives compaction and is always included in the system prompt.

---

## Design

### SessionMemory struct

New struct in `fx-session/src/session.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct SessionMemory {
    /// What this session is about.
    pub project: Option<String>,
    /// Current state of work.
    pub current_state: Option<String>,
    /// Key decisions made during this session.
    pub key_decisions: Vec<String>,
    /// Files actively being worked on.
    pub active_files: Vec<String>,
    /// Custom context the agent wants to remember.
    pub custom_context: Vec<String>,
    /// Unix epoch seconds of last update.
    pub last_updated: u64,
}
```

### Storage

Add `memory` field to `Session`:
```rust
pub struct Session {
    // ... existing fields ...
    #[serde(default)]
    pub memory: SessionMemory,
}
```

Since `Session` is serialized to redb as JSON, the new `memory` field with `#[serde(default)]` is backward-compatible: existing sessions without the field deserialize with `SessionMemory::default()`.

### Agent-initiated updates via tool

New tool `update_session_memory` on `JournalSkill` (or a new `SessionMemorySkill`). Since `JournalSkill` already has access to the journal and is registered in the skill registry, adding it there keeps the skill count low. However, `JournalSkill` doesn't have access to `Session`. Better approach: create a standalone `SessionMemorySkill` in a new file.

Actually, the simplest approach: the `update_session_memory` tool writes to a shared `Arc<Mutex<SessionMemory>>` that the `LoopEngine` owns. The skill receives a clone of the arc at registration. When the agent calls the tool, it updates the in-memory struct. The `SessionStore` persists it on the next session save (which already happens after each turn in the loop).

### Tool definition

```json
{
  "name": "update_session_memory",
  "description": "Update persistent session memory with key facts about this session. Use when you learn something important about the project, make a decision, or the state of work changes. This memory survives conversation compaction and keeps you oriented across long sessions.",
  "parameters": {
    "type": "object",
    "properties": {
      "project": {
        "type": "string",
        "description": "What this session is about (e.g., 'parameter golf optimization')"
      },
      "current_state": {
        "type": "string",
        "description": "Current state of work (e.g., 'Best BPB: 3.557, trying hillclimb config')"
      },
      "key_decisions": {
        "type": "array",
        "items": { "type": "string" },
        "description": "Key decisions to remember (appended to existing, max 20)"
      },
      "active_files": {
        "type": "array",
        "items": { "type": "string" },
        "description": "Files actively being worked on (replaces existing list)"
      },
      "custom_context": {
        "type": "array",
        "items": { "type": "string" },
        "description": "Any other context to remember (appended, max 20)"
      }
    }
  }
}
```

**Update semantics:**
- `project`: overwrites (there's only one project per session)
- `current_state`: overwrites (reflects latest state)
- `key_decisions`: **appends** to existing, capped at 20 (oldest evicted)
- `active_files`: **replaces** (agent knows the current file set)
- `custom_context`: **appends**, capped at 20 (oldest evicted)

### System prompt injection

In `perceive()`, after building the context window, if `session.memory` has content, prepend it as a system message:

```rust
fn session_memory_system_message(memory: &SessionMemory) -> Option<Message> {
    if memory.is_empty() {
        return None;
    }
    let mut lines = Vec::new();
    lines.push("[Session Memory]".to_string());
    if let Some(project) = &memory.project {
        lines.push(format!("Project: {project}"));
    }
    if let Some(state) = &memory.current_state {
        lines.push(format!("Current state: {state}"));
    }
    if !memory.key_decisions.is_empty() {
        lines.push("Key decisions:".to_string());
        for decision in &memory.key_decisions {
            lines.push(format!("- {decision}"));
        }
    }
    if !memory.active_files.is_empty() {
        lines.push("Active files:".to_string());
        for file in &memory.active_files {
            lines.push(format!("- {file}"));
        }
    }
    if !memory.custom_context.is_empty() {
        lines.push("Context:".to_string());
        for ctx in &memory.custom_context {
            lines.push(format!("- {ctx}"));
        }
    }
    Some(Message::system(lines.join("\n")))
}
```

This message is inserted after the synthesis instruction system message and before any conversation history. It's tiny (200-500 tokens typically) and gives the model complete orientation.

### Size cap

`SessionMemory` has a hard cap of 2000 tokens (estimated via `estimate_text_tokens`). The `update_session_memory` tool validates total size before applying and returns an error if exceeded, telling the agent to be more concise.

### is_empty helper

```rust
impl SessionMemory {
    pub fn is_empty(&self) -> bool {
        self.project.is_none()
            && self.current_state.is_none()
            && self.key_decisions.is_empty()
            && self.active_files.is_empty()
            && self.custom_context.is_empty()
    }
}
```

---

## Implementation Plan

### Step 1: SessionMemory struct + Session field (fx-session)

1. Add `SessionMemory` struct to `session.rs`
2. Add `memory: SessionMemory` field to `Session` with `#[serde(default)]`
3. Add `is_empty()`, `estimated_tokens()`, `apply_update()` methods
4. Tests: serialization round-trip, backward compat (deserialize without memory field), apply_update semantics, size cap

### Step 2: update_session_memory tool (fx-kernel)

1. Create `SessionMemorySkill` — a new skill type that wraps `Arc<Mutex<SessionMemory>>`
2. Implements `Skill` trait with `update_session_memory` tool
3. Register in startup alongside JournalSkill
4. Tests: tool definition, update semantics, size cap enforcement

Actually, to keep it simpler and avoid cross-crate wiring headaches:

**Alternative approach (recommended):** Add the `update_session_memory` tool directly to `LoopEngine` as a built-in tool handler, similar to how decompose/steer/abort are handled. The engine already has access to the session. No new skill crate needed.

But looking at the code, skills are the clean way. Let me check how skills get session access... The `Skill::execute` method only gets `tool_name`, `arguments`, and `cancel`. No session reference. So either:

a) Add `update_session_memory` as a built-in tool in the engine (alongside decompose), or
b) Pass an `Arc<Mutex<SessionMemory>>` to a new skill at construction time

Option (b) is cleaner: the skill is standalone, testable, and doesn't bloat the engine. The `Arc<Mutex<SessionMemory>>` is created from the active session's memory field when the engine starts, and written back to the session on save.

### Step 3: System prompt injection (fx-kernel)

1. In `perceive()`, after building context_window, insert session memory message
2. The memory message goes right after any system-prefix messages
3. Tests: verify memory appears in context, empty memory produces no message

### Step 4: Wire into startup (fx-cli)

1. Extract `session.memory` into an `Arc<Mutex<SessionMemory>>`
2. Create `SessionMemorySkill` with the arc
3. Register in skill registry
4. On session save, write arc contents back to `session.memory`

---

## Files to create/modify

| File | Change |
|------|--------|
| `engine/crates/fx-session/src/session.rs` | Add `SessionMemory` struct, add `memory` field to `Session` |
| `engine/crates/fx-session/src/lib.rs` | Re-export `SessionMemory` |
| `engine/crates/fx-kernel/src/session_memory_skill.rs` (NEW) | `SessionMemorySkill` implementing `Skill` trait |
| `engine/crates/fx-kernel/src/lib.rs` | Add `pub mod session_memory_skill;` |
| `engine/crates/fx-kernel/src/loop_engine.rs` | Add session memory system message injection in perceive() |
| `engine/crates/fx-cli/src/startup.rs` | Wire SessionMemorySkill into skill registry |

---

## Tests

### fx-session tests (session.rs)

1. `session_memory_default_is_empty`: `SessionMemory::default().is_empty()` is true
2. `session_memory_round_trip`: Serialize + deserialize preserves all fields
3. `session_backward_compat`: Deserialize a `Session` JSON without `memory` field succeeds with default memory
4. `apply_update_overwrites_project`: Setting project replaces previous
5. `apply_update_appends_decisions`: Key decisions append, capped at 20
6. `apply_update_replaces_active_files`: Active files are fully replaced
7. `apply_update_caps_at_20`: Adding >20 decisions evicts oldest
8. `estimated_tokens_returns_reasonable_value`: Non-zero for non-empty memory

### fx-kernel tests (session_memory_skill.rs)

9. `skill_provides_one_tool`: tool_definitions returns update_session_memory
10. `skill_update_modifies_memory`: Execute update_session_memory, verify arc updated
11. `skill_rejects_oversized_memory`: Update that exceeds 2000 token cap returns error
12. `skill_returns_none_for_unknown_tool`: execute with wrong name returns None

### fx-kernel tests (loop_engine.rs)

13. `session_memory_injected_in_context`: When memory is non-empty, context window contains [Session Memory] message
14. `empty_session_memory_not_injected`: When memory is empty, no extra system message

---

## Non-goals

- Automatic LLM extraction during compaction (deferred — agent-initiated is more reliable)
- Session memory UI in Swift app (future, after backend)
- Cross-session memory sharing (different feature)
- Memory conflict resolution (single-writer, agent is the only updater)
