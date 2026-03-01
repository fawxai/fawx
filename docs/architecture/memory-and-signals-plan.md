# Memory & Signals Plan
## Signal-Driven Self-Improvement Architecture

**Date:** 2026-03-01
**Status:** Draft — awaiting Joe's review
**Supersedes:** Original "Memory Dreaming" design in fawx-architecture.html §5-6

---

## Core Thesis

Intelligence lives in the signal analysis pipeline, not in the memory storage layer. Memory is an output of learning, not the system that does the learning. The agent improves by analyzing its own behavioral signals — not by reorganizing stored text.

This is inspired by [Factory's Signals](https://factory.ai/news/factory-signals) approach: LLM-as-judge analyzes sessions at scale, discovers friction patterns through clustering (not predefined categories), and closes the loop by auto-filing issues and implementing fixes.

---

## Architecture Overview

```
┌─────────────────────────────────────────────┐
│          SIGNAL ANALYSIS PIPELINE           │  ← The brain
│  Cross-session pattern discovery            │
│  Emergent categories (not predefined)       │
│  Friction / success / regression detection  │
│  Batch: session-boundary or on-demand       │
└──────────┬───────────────────┬──────────────┘
           │                   │
     ┌─────▼──────┐     ┌─────▼──────┐
     │   LOOP     │     │  MEMORY    │  ← Outputs
     │   CLOSURE  │     │  STORE     │
     │            │     │            │
     │  Auto-file │     │  Patterns  │
     │  issues    │     │  Insights  │
     │  Adjust    │     │  User data │
     │  behavior  │     │            │
     └────────────┘     └────────────┘
           ▲
┌──────────┴──────────────────────────────────┐
│          SIGNAL COLLECTION                  │  ← Already built
│  Every loop step emits typed signals        │
│  Friction, success, decision, performance   │
│  With metadata: tool, latency, tokens, etc  │
└──────────┬──────────────────────────────────┘
           │
┌──────────▼──────────────────────────────────┐
│          SIGNAL PERSISTENCE                 │  ← Build first
│  JSONL per session (same as conversation)   │
│  Survives restart, accumulates over time    │
└─────────────────────────────────────────────┘
```

---

## Layer 1: Signal Collection (BUILT)

The loop engine already emits signals on every step of every iteration:

| Signal Type | When | Metadata |
|-------------|------|----------|
| `Perceive/Trace` | Perception built | Context size, user input |
| `Reason/Trace` | LLM called | Latency, token usage, model |
| `Decide/Decision` | Decision made | UseTools vs Respond, tool names |
| `Act/Success` | Tool execution succeeded | Tool name, output size, duration |
| `Act/Friction` | Tool execution failed | Tool name, error, duration |
| `Verify/Decision` | Verification result | Satisfactory/not, discrepancies |
| `Continue/Decision` | Loop continuation | Complete/Continue/NeedsInput |

**Current limitation:** Signals only exist in-memory during a session. They're used for the learning step and TUI display, then discarded.

---

## Layer 2: Signal Persistence (BUILD FIRST)

**Scope:** ~50 lines. Write signals to `~/.fawx/signals/{session-id}.jsonl` as they're emitted.

**Format:** One JSON object per line, same pattern as conversation JSONL:
```json
{"ts": 1709283600000, "step": "act", "kind": "friction", "tool": "search_text", "detail": "regex parse error", "latency_ms": 120, "iteration": 1}
{"ts": 1709283601000, "step": "verify", "kind": "decision", "satisfactory": false, "discrepancies": ["model produced fallback"], "iteration": 1}
{"ts": 1709283602000, "step": "act", "kind": "success", "tool": "run_command", "detail": "grep fallback", "latency_ms": 450, "iteration": 2}
```

**Retention:** Keep last 30 days of signal files. Older files archived or deleted.

**Why JSONL:** Same format as conversation history. Training-ready. Easy to append, easy to stream-read for analysis. No database needed.

---

## Layer 3: Signal Analysis Pipeline (CORE FEATURE)

**Trigger:** On-demand (`/analyze`) or session-boundary (after N sessions).

**Process:**
1. **Collect** — Read signal JSONL files from recent sessions
2. **Cluster** — Group signals by tool, error pattern, iteration count, timing
3. **Analyze** — LLM reviews clusters, identifies:
   - Recurring friction patterns ("search_text fails on regex 40% of the time")
   - Success patterns ("run_command with grep succeeds where search_text fails")
   - Performance regressions ("latency increased after model switch")
   - Missing capabilities ("user tried 3 approaches for X, none worked")
4. **Categorize** — Patterns become named categories. Categories are **emergent**, not predefined. The LLM names them based on what it finds. New clusters that don't match existing categories become new categories automatically.
5. **Output** — Structured findings:

```json
{
  "pattern": "search_text_regex_failure",
  "type": "friction",
  "severity": "medium",
  "frequency": "12 occurrences across 8 sessions",
  "evidence": "search_text called with regex pattern, fails with parse error, user falls back to run_command+grep",
  "recommendation": "Improve search_text regex handling or suggest grep for complex patterns",
  "confidence": 0.85
}
```

**Key design decisions:**
- **Batch, not real-time.** Analyzing every signal in real-time is expensive and produces worse patterns than batch analysis over many sessions. Cost-efficient and higher quality.
- **Emergent categories.** We do NOT predefine "friction types" or "memory types." The LLM discovers what patterns exist. This is how Factory discovered "context churn" — a category they never designed.
- **No vector embeddings (yet).** Start with simple clustering by tool name, error pattern, and timing. Add embeddings later if simple clustering isn't sufficient. YAGNI.
- **Privacy by design.** Analysis operates on signal metadata, not raw user conversations. The signal `message` field contains tool names and error categories, never raw user input or file contents. If a signal's detail could contain sensitive data (e.g., search query text), it is redacted to the error type only (e.g., "regex parse error" not "regex parse error for pattern 'password.*'"). The LLM sees "search_text failed with regex error" not "Joe searched for his API key."

---

## Layer 4: Loop Closure

**What happens with analysis findings:**

| Confidence | Action |
|------------|--------|
| Any (Phase 1) | Surface ALL findings to user: "I've noticed a pattern — want me to address it?" |
| Low (<0.6) | Log for future analysis, don't surface |

**Phase 2 (after empirical validation):** Introduce auto-write for high-confidence patterns once we have data showing LLM confidence scores correlate with actual quality. Until then, every finding goes through the user.

**Procedural memory example:**
```
Key: "tool_preference:regex_search"
Value: "When searching for regex patterns, prefer run_command with grep over search_text. search_text's regex parser fails on complex patterns."
Source: "signal_analysis"
Tags: ["procedural", "tool_preference", "search_text", "grep"]
```

This memory gets injected into future prompts when the user asks for regex searches. The agent's behavior improves without any model tuning.

**Issue filing (future, requires git infrastructure):**
When a friction pattern is severe and frequent enough, auto-file an issue describing the problem, the evidence, and a proposed fix. This is the Factory-style self-improvement loop. **Blocked on git workflow design** — see separate spec.

---

## Layer 5: Memory System (SIMPLE STORE)

**Current:** `HashMap<String, String>` in JSON.

**Enhanced:** `HashMap<String, MemoryEntry>` with lightweight metadata:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    /// The actual content
    pub value: String,
    /// When this memory was created (epoch ms)
    pub created_at_ms: u64,
    /// When this memory was last read (epoch ms)
    pub last_accessed_at_ms: u64,
    /// How many times this memory has been read
    pub access_count: u32,
    /// What created this memory.
    pub source: MemorySource,
    /// Free-form tags — emergent, not predefined types
    pub tags: Vec<String>,
}
```

**What we're NOT building:**
- ❌ MemoryType enum (episodic/semantic/procedural) — tags handle this without rigidity
- ❌ Relevance scoring function — the LLM decides relevance at retrieval time
- ❌ Vector embeddings for retrieval — start with text matching + tag filtering
- ❌ Graph layer — overkill for a local tool
- ❌ External database — JSON files on disk

**Smart injection:**
Instead of injecting all memories into the system prompt:
1. Always inject memories with `signal_analysis` or `procedural` source (high-value insights)
2. Match remaining memories against current query by case-insensitive substring match on key and value, ranked by match count
3. Cap at 20 entries (tunable via config). 20 is a conservative default that fits within ~2K tokens of context budget
4. Access-count bump on injection (tracks what's actually useful)
5. Time-decay: memories not accessed in 30+ days get deprioritized regardless of access count (prevents calcification of early memories)

**Consolidation:**
Not a separate system. When signal analysis runs, it reviews existing memories partitioned by source and tag prefix. Related memories (same tag, overlapping keys) are grouped and the LLM is asked: "These N memories are related. Produce a single consolidated memory that preserves all important information." The consolidated entry replaces the originals. This is bounded: max 20 memories per consolidation call, partitioned by tag to keep LLM context focused. Old, redundant memories identified during analysis get flagged for cleanup. Run consolidation as part of the analysis pipeline, not as an independent process.

**Migration:**
Old `HashMap<String, String>` files auto-migrate on load. Missing fields get defaults:
- `created_at_ms`: file modification time
- `last_accessed_at_ms`: 0
- `access_count`: 0
- `source`: "user"
- `tags`: []

---

## What This Does NOT Cover (Deferred)

### Dreaming (Model Tuning)
LoRA/adapter fine-tuning from signal data. Deferred until:
- Evaluation framework exists (how do you know tuning helped?)
- Local model path is mature (llama-cpp-sys)
- Enough signal data accumulated (months of real usage)
- Git self-modification workflow is designed and enforced

### Git Self-Modification
The loop closure mechanism for auto-filing issues and implementing fixes.
Requires: path enforcement, branching strategy, review gates, rollback.
See separate spec (TBD).

### Anticipation
Proactive actions based on learned patterns. Builds on signal analysis pipeline.
Deferred until signal analysis is working and generating reliable patterns.

---

## Implementation Order

| PR | Scope | Lines | Depends On |
|----|-------|-------|------------|
| **1. Signal Persistence** | Write signals to JSONL as they're emitted | ~50 | Nothing (signal collector exists) |
| **2. Memory Metadata** | MemoryEntry struct, migration, access tracking | ~200 | Nothing (parallel with #1) |
| **3. Smart Injection** | Relevance-filtered memory in system prompt | ~100 | #2 |
| **4. Signal Analysis** | Batch LLM analysis with emergent categories | ~300 | #1 |
| **5. Loop Closure** | Pattern → procedural memory + user surfacing | ~150 | #4 + #2 |
| **6. /analyze command** | TUI command to trigger analysis on-demand | ~50 | #4 |
| **7. Consolidation** | Merge redundant memories during analysis | ~100 | #4 + #2 |

Total: ~950 lines across 7 PRs. Manageable, incremental, each PR delivers value.

---

## Success Criteria

1. After 10 sessions, `/analyze` produces at least 3 meaningful friction patterns
2. Procedural memories from signal analysis measurably improve tool selection
3. Memory stays under 100 entries with consolidation (no unbounded growth)
4. No predefined categories — all patterns discovered by the LLM
5. Analysis runs in <30 seconds and costs <$0.50 per batch

---

## Comparison to Alternatives

| Approach | Storage | Retrieval | Learning | Local-First | Fawx? |
|----------|---------|-----------|----------|-------------|-------|
| Mem0 | KV + Vector + Graph | Intent-aware | Limited (lifecycle mgmt) | ❌ Cloud | ❌ |
| Zep | Temporal graph | Time + relevance | Limited (episodic summaries) | ❌ Cloud | ❌ |
| Letta/MemGPT | Blocks + archival | Agent-controlled | Limited (agent-managed) | ✅ | Partial inspiration |
| A-MEM | Zettelkasten network | Bidirectional links | ❌ None | ✅ | ❌ Overengineered |
| SimpleMem | Compressed JSONL | Adaptive | ❌ None | ✅ | Compression idea useful |
| Factory Signals | N/A (analytics) | N/A | ✅ Self-improving | ❌ Cloud | ✅ Core inspiration |
| **Fawx** | JSON + metadata | LLM-ranked | ✅ Signal-driven | ✅ Local | **This plan** |

Fawx is the only system that combines local-first storage with signal-driven learning. Everyone else is either cloud-dependent, or stores without learning.
