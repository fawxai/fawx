# Reflective Memory — Design Document

**Status:** Proposal  
**Date:** 2026-03-07  
**Authors:** Joe + Clawdio  
**Context:** Wave 7 session — emerged from observing orchestration quality compound within a session but not across sessions

---

## Problem

Fawx is stateless across sessions. Each session starts fresh — the model re-reads memory files and codebase files to rebuild context. What gets lost:

1. **Orchestration intuition** — "≤300 lines + ≤3 files = high probability of clean first-try review" is learned within a session from seeing 12 review cycles. Tomorrow, it's gone unless explicitly written down.

2. **Codebase working knowledge** — "Channel trait has 5 methods, InputSource has 6 variants, fx-fleet's NodeRegistry has register/remove/heartbeat/online/with_capability/mark_stale/all" is known by heart mid-session. Tomorrow, 6 files need re-reading.

3. **Pattern recognition** — "Long-running parallel implementers on shared codebases will delete each other's work to compile" is a hard-won lesson from a specific failure. It survives only if someone writes it into a spec template or memory file.

The current workaround is manual: daily memory logs, spec templates that accumulate warnings, AGENTS.md rules. This works but it's lossy and labor-intensive.

---

## Core Principle: Discovered, Not Forced

**Three prior failures with forced classification/declaration:**

1. `emit_intent` — forced the model to declare intent before acting. Model rebelled — wrote hollow intents, skipped the tool for simple responses. Fix: remove emit_intent, give real tools, let model choose naturally.

2. Improvement pipeline classification — forced CodeFix/SkillDoc/ConfigTune classification on every improvement proposal. Same failure pattern. Fix: real tools + natural choice.

3. (Hypothetical) Forced post-task reflection — if we require reflection after every task, we get "completed successfully, no issues" 80% of the time. The journal fills with noise.

**The principle:** Don't build heuristics to classify what the model can naturally decide. Give real tools, let the model choose, enforce constraints on outputs not classification on inputs.

**Applied to reflection:** Give the model a journal tool. Mention it in the system prompt. Trust it to recognize when something is journal-worthy.

---

## Proposed Architecture

### 1. Journal Tool (Available, Not Forced)

```
journal_write(lesson, tags, applies_to, context?)
journal_search(query, tags?, limit?) -> Vec<JournalEntry>
```

Storage: simple JSON/JSONL. No vector DB. LLM decides relevance at retrieval.

### 2. Codebase Index (Auto-Updated)

Structured, compressed representation of the codebase maintained by the model itself. Updated after implementation work. Read on session start instead of re-reading source files.

```toml
[crates.fx-core]
purpose = "Shared types, traits, errors"
public_types = ["InputSource", "Channel", "ChannelError", "SkillEvent"]
key_traits = ["Channel (id, name, input_source, is_active, send_response)"]

[crates.fx-fleet]
purpose = "Node registry + task routing"
public_types = ["NodeInfo", "NodeRegistry", "TaskRouter", "RoutingDecision"]
```

### 3. Orchestration Playbook (Pull-Based)

Each spec → implement → review cycle becomes a structured training example (scope, constraints, outcome). The model queries this when writing specs — pull-based, not push-based.

---

## System Prompt Integration

Light touch:

```
You have a journal tool for capturing lessons that help future sessions. Use it 
when you notice something worth remembering. Don't force it.

Codebase index at .fawx/codebase-index.toml — read on startup, update when you 
change public interfaces.
```

---

## Success vs Failure

**Success:** Journal has ~15 high-quality entries after 10 sessions. Model's first spec in session N is as good as session N-1's last spec. Codebase index is accurate and current.

**Failure:** Journal has 50 entries after 10 tasks, most saying "completed successfully." Model writes out of obligation, not insight. Codebase index is stale.

The difference is signal-to-noise. Selective writing = useful entries. Obligatory writing = noise.

---

## Open Questions

1. Should journal entries have TTL/decay, or is selective writing sufficient?
2. Codebase index: model-maintained or tooling-generated (cargo doc --json)?
3. Orchestration playbook: queryable or read-on-startup?
4. Multi-agent scenarios: different models journaling differently?

---

*Emerged from a conversation during Wave 7 about why orchestration quality compounds within a session but resets between sessions.*
