# Spec: Long Session Context Management

**Status:** Draft  
**Author:** Clawdio  
**Date:** 2026-03-21  
**Crates:** `fx-kernel`, `fx-session`, `fx-journal`, `fx-llm`

---

## Problem

Long-running sessions (2+ days, 300+ messages) degrade catastrophically:

1. **Tool calls time out.** The model receives the entire conversation history on each turn. A 300-message session with tool call/result pairs can be 500k+ tokens. The model spends most of its budget re-reading old file contents and search results.
2. **Retry budgets exhaust.** Tool timeouts trigger retries. Retries timeout again. After 5 failures, the tool is blocked for the session.
3. **Cascade failure.** With core tools blocked (read_file, search_text), the agent falls back to decompose (unregistered), then gives up entirely.
4. **The response quality gate fires.** The user sees "Ask a specific question" instead of useful output.

The user's workaround is "start a new session," but this loses continuity. A 2-day parameter golf session has accumulated context, decisions, and working state that's expensive to reconstruct.

**Competitive opportunity:** No SOTA harness handles this well. Codex, Claude Code, and OpenClaw all degrade on long sessions. The agent that maintains a weeks-long session without degradation wins.

---

## Existing Infrastructure

### What we have

| Component | Location | Status |
|-----------|----------|--------|
| `ConversationBudget` | `conversation_compactor.rs:323` | Tracks token budget, triggers compaction at threshold |
| `CompactionStrategy` trait | `conversation_compactor.rs` | Pluggable compaction (sliding window implemented) |
| `SlidingWindowCompactor` | `conversation_compactor.rs` | Drops oldest middle messages, preserves system + recent turns |
| `CompactionMemoryFlush` trait | `conversation_compactor.rs` | Persists evicted messages before compaction |
| `JournalCompactionFlush` | `fx-journal/flush.rs` | Writes evicted content to journal |
| `Journal` | `fx-journal/journal.rs` | Structured storage with write/search/list |
| `compact_if_needed` | `loop_engine.rs:2908` | Called each iteration, checks budget, runs strategy |
| `estimate_text_tokens` | `conversation_compactor.rs:17` | Char/word heuristic (~4 chars per token) |
| `SessionContentBlock` | `fx-session/session.rs` | Structured content blocks (text, tool_use, tool_result, image) |

### What's missing

1. **Tool block pruning** — old tool_use/tool_result pairs are treated identically to text. A `read_file` result from yesterday with 500 lines occupies the same context as a 2-line text response.
2. **Automatic compaction trigger** — `compact_if_needed` exists but only fires per-iteration. If the conversation grows between iterations (e.g., tool results), it may not trigger soon enough.
3. **Session memory extraction** — no mechanism to extract key facts/decisions into a persistent summary that survives compaction.
4. **Retrieval over history** — no tool for the agent to search old conversation turns on demand.
5. **Summarization strategy** — `SlidingWindowCompactor` only drops messages. There's a `SummarizingCompactor` path but it requires an LLM call and has failure modes (timeout, budget exceeded).

---

## Design

### Layer 1: Aggressive Tool Block Pruning

**The highest-impact, lowest-risk change.**

Tool call/result pairs older than N turns are replaced with a compact summary. This alone cuts 60-80% of context bloat in tool-heavy sessions.

#### Behavior

Before sending messages to the LLM, run a pruning pass:

```
For each message older than `preserve_recent_turns`:
  For each ContentBlock in message.content:
    if ContentBlock::ToolUse { id, name, input }:
      Replace with ContentBlock::Text { text: "[tool: {name}]" }
    if ContentBlock::ToolResult { tool_use_id, content }:
      Summarize content to max 100 chars:
        "[result: {first_100_chars}...]"
      Replace the block.
    if ContentBlock::Image:
      Replace with ContentBlock::Text { text: "[image]" }
```

#### Where it lives

New function in `conversation_compactor.rs`:

```rust
pub fn prune_tool_blocks(
    messages: &mut [Message],
    preserve_recent_turns: usize,
) -> PruneResult {
    // ...
}

pub struct PruneResult {
    pub pruned_count: usize,
    pub tokens_saved: usize,
}
```

Called from `compact_if_needed` BEFORE the compaction strategy runs. This reduces token count enough that compaction may not even trigger.

#### Configuration

```toml
[compaction]
prune_tool_blocks = true           # default: true
tool_block_summary_max_chars = 100 # max chars in summarized tool result
```

#### Tests

- Tool blocks older than N turns are pruned; recent ones preserved
- Pruned tool_use retains name but drops input
- Pruned tool_result retains first N chars
- Image blocks replaced with placeholder
- Active tool chains (in-flight calls) are never pruned
- Token estimate decreases after pruning

---

### Layer 2: Tiered Compaction Strategy

Replace the single sliding window with a multi-tier approach that fires automatically at different thresholds.

#### Tiers

| Tier | Trigger | Action |
|------|---------|--------|
| **Prune** | 40% of budget | Tool block pruning (Layer 1) |
| **Slide** | 60% of budget | Sliding window drops oldest middle messages |
| **Summarize** | 80% of budget | LLM summarizes evicted messages into session memory |
| **Emergency** | 95% of budget | Hard drop of oldest 50% (preserve system + last 5 turns) |

#### Implementation

Extend `CompactionConfig`:

```rust
pub struct CompactionConfig {
    pub model_context_limit: usize,
    pub reserved_system_tokens: usize,
    pub preserve_recent_turns: usize,
    // New fields
    pub prune_threshold: f32,     // 0.40
    pub slide_threshold: f32,     // 0.60 (existing compaction_threshold)
    pub summarize_threshold: f32, // 0.80
    pub emergency_threshold: f32, // 0.95
}
```

`compact_if_needed` checks tiers in order and applies the least aggressive strategy that brings context under the next tier's threshold.

#### Tests

- Prune tier fires at 40%, doesn't trigger slide
- Slide tier fires at 60% when pruning wasn't enough
- Summarize tier fires at 80% when sliding wasn't enough
- Emergency tier fires at 95% and always succeeds
- Tier transitions are logged with before/after token counts

---

### Layer 3: Session Memory Extraction

As conversation progresses, extract key facts into a structured "session memory" block that persists across compaction.

#### Behavior

After each compaction that uses summarization (Tier 3+), extract structured facts:

```json
{
  "session_memory": {
    "project": "parameter golf",
    "current_state": "Best BPB: 3.557, using MLX proxy launcher",
    "key_decisions": [
      "Clip value: 99.9995",
      "Using rowgroup-18 for artifact QAT"
    ],
    "active_files": [
      "experiments/run_mlx_proxy.py",
      "experiments/configs/hillclimb.toml"
    ],
    "last_updated": 1774056000
  }
}
```

This block is prepended to every request as a system message. It's tiny (200-500 tokens) but gives the model full context about the session's purpose and state.

#### Where it lives

New struct in `fx-session`:

```rust
pub struct SessionMemory {
    pub project: Option<String>,
    pub current_state: Option<String>,
    pub key_decisions: Vec<String>,
    pub active_files: Vec<String>,
    pub custom_context: Vec<String>,
    pub last_updated: u64,
}
```

Persisted alongside the session in `sessions.redb`.

#### Extraction mechanism

Two paths:

1. **LLM extraction** (post-compaction): After summarizing evicted messages, also ask the LLM to update the session memory JSON. One extra call during compaction.
2. **Agent-initiated**: Give the agent a `update_session_memory` tool so it can explicitly record important state. This is more reliable than LLM extraction for structured data.

Both paths write to the same `SessionMemory` struct.

#### Tests

- Session memory survives compaction
- Session memory is included as system message in requests
- Agent can update session memory via tool
- Session memory has max size (2000 tokens) to prevent bloat
- Session memory is displayed in session info (CLI + GUI)

---

### Layer 4: Retrieval Over History

Give the agent a `recall` tool to search old conversation turns stored in the journal.

#### Tool definition

```json
{
  "name": "recall_session_context",
  "description": "Search previous conversation history for relevant context. Use when you need information from earlier in this session that may have been compacted.",
  "parameters": {
    "query": "Natural language search query",
    "limit": "Maximum results to return (default: 5)"
  }
}
```

#### Behavior

1. Searches the journal (where `JournalCompactionFlush` stores evicted messages)
2. Returns matching entries with timestamps and relevance scores
3. Agent decides whether to use the retrieved context

#### Where it lives

New method on `JournalSkill` or a new `RecallSkill`:

```rust
impl Skill for RecallSkill {
    fn name(&self) -> &str { "recall_session_context" }
    
    async fn execute(&self, input: Value) -> Result<Value, SkillError> {
        let query = input["query"].as_str()?;
        let limit = input["limit"].as_u64().unwrap_or(5);
        let results = self.journal.search(query, limit);
        // Format and return
    }
}
```

#### Tests

- Recall finds relevant evicted messages
- Recall returns empty when no matches
- Recall respects limit parameter
- Recall results include timestamps
- Journal entries from compaction flush are searchable

---

## Implementation Order

### Phase 1: Tool Block Pruning (highest impact, lowest risk)
- Add `prune_tool_blocks` to `conversation_compactor.rs`
- Call from `compact_if_needed` before strategy
- Add config field + tests
- **Estimated effort:** 1 PR, ~200 lines

### Phase 2: Tiered Compaction
- Extend `CompactionConfig` with tier thresholds
- Refactor `compact_if_needed` to check tiers
- Add emergency compaction fallback
- **Estimated effort:** 1 PR, ~300 lines

### Phase 3: Session Memory
- Add `SessionMemory` struct to `fx-session`
- Add `update_session_memory` tool
- Persist in redb alongside session
- Include as system message in requests
- LLM extraction during compaction (optional, can defer)
- **Estimated effort:** 2 PRs, ~400 lines total

### Phase 4: Recall Tool
- Add `RecallSkill` or extend `JournalSkill`
- Wire into skill registry
- Ensure journal entries from compaction are searchable
- **Estimated effort:** 1 PR, ~150 lines

---

## Success Criteria

1. A 300-message session with heavy tool use maintains sub-5s response times
2. No tool retry exhaustion from context pressure
3. The agent retains awareness of session purpose and key decisions across compaction
4. The agent can retrieve specific details from compacted history via recall
5. Zero data loss: all evicted content is searchable in the journal

---

## Risks & Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Over-aggressive pruning loses important tool output | Agent misses context | Preserve recent N turns, keep tool names, keep first 100 chars of results |
| Summarization LLM call fails | Compaction stalls | Emergency tier hard-drops without summarization |
| Session memory grows unbounded | Becomes its own context problem | Hard cap at 2000 tokens, oldest entries evicted |
| Recall tool adds latency | Slower responses | Make it optional; agent only calls when needed |
| Token estimation inaccuracy | Wrong compaction timing | Conservative estimates (overcount rather than undercount) |

---

## Non-goals (this spec)

- Multi-session memory sharing (different problem, different spec)
- Conversation branching/forking
- User-facing memory management UI (future, after backend is solid)
- Provider-specific context window detection (use configured limit)
