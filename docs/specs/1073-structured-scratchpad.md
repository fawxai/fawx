# #1073 — Structured Scratchpad / Working Memory

**Status:** Implementation Spec  
**Date:** 2026-03-03  
**Crate scope:** New `fx-scratchpad` crate + wiring in `fx-kernel` and `fx-cli`  
**Prerequisites:** None (standalone, but complements #1056 context compaction)

---

## 1. Problem Statement

During multi-iteration loops, Fawx re-reasons from scratch every cycle. The LLM re-reads context, re-derives hypotheses, re-discovers facts. This burns tokens and degrades quality — the model "forgets" what it already figured out because intermediate reasoning only exists in conversation history, which grows and eventually gets compacted away.

### What this solves

A structured scratchpad gives the model a place to **write down** intermediate reasoning that persists across iterations:
- Hypotheses it's testing
- Facts it's discovered
- Conclusions it's reached
- Dead ends it should avoid

Unlike conversation history, scratchpad entries are structured (typed, labeled, updatable) so they compress well and survive context compaction.

### What this does NOT do

- Does NOT replace cross-session memory (that's `fx-memory` / `SignalStore`)
- Does NOT persist after session ends (session-scoped only)
- Does NOT run autonomously — the model explicitly reads/writes scratchpad via tools

---

## 2. Existing Infrastructure

| Component | Location | Relevance |
|-----------|----------|-----------|
| `ReasoningContext` | `fx-kernel/src/types.rs:53` | Has `working_memory: Vec<WorkingMemoryEntry>` — currently populated from `fx-memory` cross-session store. Scratchpad is separate: intra-session, structured, model-managed. |
| `ProcessedPerception` | `fx-kernel/src/perceive.rs` | Carries `working_memory`, `episodic`, `semantic` into reasoning prompt. Scratchpad entries inject here. |
| `ContextCompactor` | `fx-kernel/src/context_manager.rs` | Compacts `ReasoningContext`. Scratchpad entries should be compaction-resistant (they're already summaries). |
| `Skill` trait | `fx-loadable/src/skill.rs` | Scratchpad tools register as a `Skill`. Pattern: `GitSkill`, `BuiltinToolsSkill`. |
| `SkillRegistry` | `fx-loadable/src/registry.rs` | Dispatches tool calls to skills. Scratchpad skill registers here. |

---

## 3. Data Model

### 3.1 Scratchpad Entry

```rust
// fx-scratchpad/src/lib.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScratchpadEntry {
    /// Unique ID within this scratchpad (auto-generated, monotonic).
    pub id: u32,
    /// Entry kind: hypothesis, observation, conclusion, or note.
    pub kind: EntryKind,
    /// Short label (model-assigned, for reference).
    pub label: String,
    /// Content body.
    pub content: String,
    /// Confidence in this entry (model-assigned).
    pub confidence: Confidence,
    /// Status: active, superseded, or invalidated.
    pub status: EntryStatus,
    /// ID of parent entry (for tree structure). None = root.
    pub parent_id: Option<u32>,
    /// Iteration when created.
    pub created_at_iteration: u32,
    /// Iteration when last updated.
    pub updated_at_iteration: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum EntryKind {
    /// A testable claim about the problem.
    Hypothesis,
    /// A factual observation from tools or context.
    Observation,
    /// A resolved conclusion (hypothesis confirmed/denied).
    Conclusion,
    /// Freeform note (dead end, reminder, constraint).
    Note,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum EntryStatus {
    /// Currently relevant.
    Active,
    /// Replaced by a newer entry.
    Superseded,
    /// Proven wrong or no longer applicable.
    Invalidated,
}
```

Note: `Confidence` is reused from `fx-analysis::findings::Confidence` (High/Medium/Low). If that creates an unwanted dependency, duplicate the enum locally.

### 3.2 Scratchpad

```rust
// fx-scratchpad/src/lib.rs

#[derive(Debug, Clone, Default)]
pub struct Scratchpad {
    entries: Vec<ScratchpadEntry>,
    next_id: u32,
}

impl Scratchpad {
    pub fn new() -> Self;

    /// Add a new entry. Returns the assigned ID.
    pub fn add(&mut self, kind: EntryKind, label: String, content: String,
               confidence: Confidence, parent_id: Option<u32>,
               iteration: u32) -> Result<u32, ScratchpadError>;

    /// Update an existing entry's content and/or confidence.
    pub fn update(&mut self, id: u32, content: Option<String>,
                  confidence: Option<Confidence>, status: Option<EntryStatus>,
                  iteration: u32) -> Result<(), ScratchpadError>;

    /// Remove an entry by ID. Returns the removed entry.
    pub fn remove(&mut self, id: u32) -> Result<ScratchpadEntry, ScratchpadError>;

    /// List all active entries (status != Invalidated).
    pub fn active_entries(&self) -> Vec<&ScratchpadEntry>;

    /// List entries by kind.
    pub fn entries_by_kind(&self, kind: EntryKind) -> Vec<&ScratchpadEntry>;

    /// Render scratchpad as structured text for injection into reasoning context.
    pub fn render_for_context(&self) -> String;

    /// Estimated token count of the rendered scratchpad.
    pub fn estimated_tokens(&self) -> usize;

    /// Total entry count (all statuses).
    pub fn len(&self) -> usize;

    pub fn is_empty(&self) -> bool;
}
```

---

## 4. Tool Definitions

### 4.1 ScratchpadSkill

Implements the `Skill` trait, exposes 4 tools:

| Tool | Args | Returns |
|------|------|---------|
| `scratchpad_add` | `kind`, `label`, `content`, `confidence`, `parent_id?` | `"Added entry #N: {label}"` |
| `scratchpad_update` | `id`, `content?`, `confidence?`, `status?` | `"Updated entry #N"` |
| `scratchpad_remove` | `id` | `"Removed entry #N: {label}"` |
| `scratchpad_list` | `kind?`, `active_only?` (default true) | Formatted list of matching entries |

```rust
// fx-scratchpad/src/skill.rs

#[derive(Debug)]
pub struct ScratchpadSkill {
    scratchpad: Arc<Mutex<Scratchpad>>,
}

impl ScratchpadSkill {
    pub fn new(scratchpad: Arc<Mutex<Scratchpad>>) -> Self;
}

#[async_trait]
impl Skill for ScratchpadSkill {
    fn name(&self) -> &str { "scratchpad" }
    fn tool_definitions(&self) -> Vec<ToolDefinition>;
    fn cacheability(&self, _tool_name: &str) -> ToolCacheability {
        ToolCacheability::NeverCache  // state-mutating tools
    }
    async fn execute(&self, tool_name: &str, arguments: &str,
                     cancel: Option<&CancellationToken>) -> Option<Result<String, SkillError>>;
}
```

### 4.2 System Prompt Addition

When scratchpad is non-empty, inject into reasoning prompt:

```
## Scratchpad (your working notes — update as you learn)
[rendered scratchpad entries]
```

When empty, inject hint:

```
## Scratchpad
(empty — use scratchpad_add to track hypotheses, observations, conclusions)
```

---

## 5. Integration Points

### 5.1 Kernel: Perceive Phase

In `perceive()`, after building `ProcessedPerception`, inject scratchpad content:

```rust
// In LoopEngine or perceive pipeline
if let Some(scratchpad) = &self.scratchpad {
    let sp = scratchpad.lock().unwrap_or_else(|p| p.into_inner());
    if !sp.is_empty() {
        // Inject as a dedicated section in reasoning context
        perception.scratchpad_context = Some(sp.render_for_context());
    }
}
```

Add `scratchpad_context: Option<String>` to `ProcessedPerception`.

### 5.2 Kernel: Reasoning Prompt

In `reasoning_user_prompt()`, include scratchpad section between identity context and conversation history (high priority, low churn):

```
[identity context]
[scratchpad]        ← NEW
[memory context]
[conversation history]
[user message]
```

### 5.3 CLI: Wiring

In `TuiApp::handle_message`:
1. Create `Arc<Mutex<Scratchpad>>` per session (not per message)
2. Create `ScratchpadSkill` with shared ref
3. Register with `SkillRegistry`
4. Pass shared ref to `LoopEngine` (via builder, after #1056 builder lands)

### 5.4 Context Compaction Interaction

Scratchpad entries are **compaction-resistant** — they should NOT be dropped by conversation compaction (#1056). The scratchpad is injected separately from conversation history. If the scratchpad itself grows too large (>25% of context budget), the scratchpad should self-compact:
- Drop `Invalidated` entries
- Drop `Superseded` entries older than 5 iterations
- If still over budget, drop lowest-confidence `Note` entries

---

## 6. Implementation Plan

### Phase 1: Core Data Model (fx-scratchpad)

1. Create `fx-scratchpad` crate
2. Implement `ScratchpadEntry`, `EntryKind`, `EntryStatus`, `Scratchpad`
3. Implement `render_for_context()` — structured text output
4. Implement `estimated_tokens()` — word count ÷ 0.75 heuristic
5. Implement self-compaction (drop invalidated/superseded/low-confidence)
6. Unit tests for all operations

### Phase 2: Skill + Tools

1. Implement `ScratchpadSkill` with 4 tools
2. Tool argument parsing + validation
3. Tool output formatting
4. Register with `SkillRegistry` in fx-cli
5. Tests for each tool

### Phase 3: Kernel Integration

1. Add `scratchpad: Option<Arc<Mutex<Scratchpad>>>` to `LoopEngine` (via builder)
2. Inject scratchpad into `ProcessedPerception` during perceive
3. Add scratchpad section to reasoning prompt
4. Wire in TUI app
5. Integration tests: multi-iteration loop with scratchpad reads/writes

---

## 7. Test Plan

### Data Model Tests

| Test | Assertion |
|------|-----------|
| `add_returns_monotonic_ids` | IDs are 0, 1, 2, ... |
| `add_with_parent_validates_parent_exists` | Invalid parent_id → ScratchpadError |
| `update_modifies_content_and_iteration` | Content changed, updated_at_iteration bumped |
| `update_nonexistent_id_returns_error` | Unknown ID → ScratchpadError |
| `remove_returns_entry` | Removed entry returned, no longer in active list |
| `active_entries_excludes_invalidated` | Invalidated entries filtered out |
| `entries_by_kind_filters_correctly` | Only matching kind returned |
| `render_for_context_formats_tree` | Parent-child nesting visible in output |
| `estimated_tokens_scales_with_content` | Larger scratchpad → more tokens |
| `self_compact_drops_invalidated_first` | Invalidated removed before superseded |
| `self_compact_drops_superseded_over_age` | Old superseded entries removed |
| `self_compact_drops_low_confidence_notes` | Notes dropped by confidence when over budget |

### Skill Tests

| Test | Assertion |
|------|-----------|
| `scratchpad_add_creates_entry` | Tool returns success, entry exists |
| `scratchpad_add_validates_kind` | Invalid kind → error message |
| `scratchpad_update_changes_status` | Status transitions work |
| `scratchpad_remove_deletes_entry` | Entry gone after remove |
| `scratchpad_list_default_active_only` | Invalidated not shown by default |
| `scratchpad_list_with_kind_filter` | Only matching kind returned |
| `tool_definitions_returns_four_tools` | Correct count and names |

### Integration Tests

| Test | Assertion |
|------|-----------|
| `scratchpad_survives_across_iterations` | Entry added in iter 1, visible in iter 3 |
| `scratchpad_in_reasoning_prompt` | Rendered text appears in prompt |
| `empty_scratchpad_shows_hint` | Hint text in prompt when empty |
| `scratchpad_compaction_under_budget_pressure` | Self-compacts when over 25% budget |

---

## 8. Estimated Complexity

| Phase | Lines (code) | Lines (tests) | Effort |
|-------|-------------|---------------|--------|
| Phase 1: Data model | ~200 | ~180 | 0.5 day |
| Phase 2: Skill + tools | ~150 | ~120 | 0.5 day |
| Phase 3: Kernel integration | ~80 | ~60 | 0.5 day |
| **Total** | **~430** | **~360** | **1.5 days** |
