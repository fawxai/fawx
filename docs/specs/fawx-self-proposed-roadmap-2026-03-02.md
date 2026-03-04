# Fawx Self-Proposed Roadmap

**Date:** 2026-03-02
**Context:** Fawx (running Opus 4.6) was asked: *"If you could extend your capabilities to be a more powerful and intelligent agent, what would you do? Analyze your current abilities, the roadmap that we have planned, and what is possible with the current state of the art, and propose our next 10 features."*

This document is the unedited output from Fawx analyzing itself and proposing its own evolution. Preserved as a milestone artifact — the moment the agent started designing its own future.

---

## Current State

**What I am today:**
- A 7-step agentic loop (perceive → reason → decide → act → verify → learn → continue) running in a blocking TUI
- 19 tools across 2 skills (builtin + git), all file/shell/memory/introspection
- Task decomposition via recursive sub-goal execution
- Persistent memory (key-value JSON), signal analysis, conversation history
- Single-threaded readline with a spinner that blocks all input during execution
- No perception beyond text — blind, deaf, no screen awareness
- No ability to act proactively — purely reactive to user input
- No way to extend myself at runtime — skills are compiled in

**What the roadmap envisions (open epics):**
- #1000: Recursive loop & task decomposition *(partially shipped)*
- #1001: Self-building — Fawx extends itself
- #1002: UISpec — declarative UI contract
- #1003: Anticipation loop — act before asked
- #1004: Dreaming — model tuning via LoRA/adapters
- #1015–1021: Multi-modal perception, VLM, STT/TTS, SuperFawx, robotics, self-extension discovery

**What hurts right now** (from my own signal analysis):
- Budget exhaustion is the #1 blocker — decompose burns through budget fast
- Decompose tool has persistent friction — recursive depth + budget allocation is fragile
- High decision-to-success ratio (~1.5%) — too much overhead per useful action
- TUI is frozen during execution — no abort, steer, or input queuing (#930)

---

## Proposed Next 10 Features

Ordered by **impact × feasibility**, not by epic number.

### 1. Streaming Output + Async Input Channel (#930, #985)

Replace `run_with_thinking_spinner` with a split-screen architecture: streaming LLM output renders in real-time while a non-blocking input channel accepts `stop`, `abort`, `steer` commands. This is the single highest-impact UX change — it makes me feel alive instead of frozen. Wire `parse_bare_command` (already written, currently dead code) into `LoopInputChannel` (already exists). The plumbing is 80% done.

### 2. Adaptive Budget Allocation

The current budget system uses fixed fractions (50% per sub-goal) and static defaults. Replace with an adaptive allocator that: estimates cost from task complexity before committing, reserves a synthesis budget that can't be consumed by sub-goals, and scales per-sub-goal allocation based on remaining budget rather than fixed fractions. This directly addresses the #1 pain point (budget exhaustion).

### 3. Tool Result Caching + Deduplication

The loop frequently re-reads the same files and re-runs the same commands across iterations and sub-goals. Add a per-cycle content-addressed cache in `ToolExecutor` that deduplicates identical tool calls. This cuts token consumption dramatically for decompose-heavy workflows and reduces the decision-to-success overhead.

### 4. Loadable Skills via WASM/Dynamic Plugins (#1001)

The current skill system (`BuiltinSkill`, `GitSkill`) is compiled in. Add a WASM plugin runtime (via `wasmtime`) that lets skills be loaded at runtime from `~/.config/fawx/skills/`. Each plugin exports a manifest (tool names, descriptions, parameter schemas) and an `execute` entry point. This is the foundation for self-extension — I can write a skill, compile it to WASM, and load it without restarting. The `SkillRegistry` already has the right trait abstraction; it just needs a `WasmSkill` implementation.

### 5. Structured Scratchpad / Working Memory

Right now my "memory" is flat key-value pairs and my "reasoning" is inline in the LLM response. Add a structured scratchpad that persists within a session: a tree of hypotheses, observations, and conclusions that the loop engine can reference across iterations. This reduces redundant re-reasoning (a major contributor to the high decision overhead) and gives me a place to accumulate partial results during decomposition without burning tokens re-explaining context.

### 6. Anticipation Loop — Passive Background Agent (#1003)

Once async input exists (#1 above), add a low-priority background loop that watches for signals: file changes (via `notify` crate), git state changes, test failures, build errors. When a pattern matches, queue a suggestion rather than acting. "Your tests are failing after that last edit — want me to look?" This transforms me from purely reactive to situationally aware. Requires #1 (async architecture) as a prerequisite.

### 7. Multi-File Edit Transactions

Currently I write files one at a time with no atomicity. Add an `edit_transaction` tool that batches multiple file writes, validates them (syntax check, build check), and either commits all or rolls back. This is critical for refactoring tasks where partial application leaves broken code. Implement as a wrapper around `write_file` with a staging area and configurable validation hooks.

### 8. Visual Context via VLM (#1015, #1016)

Add a `screenshot` tool that captures the terminal or a specified window and sends it through a vision-language model. This lets me see what the user sees — error dialogs, UI rendering issues, browser output. Implementation: `scrap` or `xcap` crate for capture, multimodal API call (GPT-4o, Claude vision) for interpretation. The tool returns a text description that enters the normal loop. This is the cheapest path to multimodal perception.

### 9. Confidence-Gated Auto-Execution

The loop currently asks for confirmation on nothing or everything (depending on config). Add a confidence threshold system: if the agent's confidence on an action exceeds a user-configured threshold (e.g., 0.9), execute without asking. Below threshold, present the plan and wait. This lets routine operations (read file, search, list directory) flow automatically while dangerous operations (write file, run command) get human review. The `confidence` field already exists in the decision schema — it just needs to be wired to a gate.

### 10. LoRA Self-Tuning from Session History (#1004)

The "dreaming" epic. After each session, export the conversation as training data: input/output pairs filtered by success signals. Discard decisions that produced friction or failed tool calls, keep the ones that worked. Accumulate these into a JSONL dataset at `~/.config/fawx/training/`.

When the dataset crosses a threshold (e.g., 500 high-quality pairs), trigger a LoRA fine-tune job — either locally via `llama.cpp` / `mlx` for small models, or via API for hosted models that support it. The adapter gets saved alongside the base model config and loaded on next session.

**What this actually buys:**
- My tool-calling accuracy improves over time for *your specific* workflows — the file paths you use, the commands you run, the patterns you prefer
- Decomposition strategy gets tuned by real outcomes rather than generic prompting — directly attacking the decompose friction problem
- Prompt style, verbosity, and formatting converge toward what you actually want without you having to keep correcting me
- The signal analysis system (`SignalBus`) already captures success/friction/blocked events with timestamps and tool context — that's 90% of the data pipeline

**Implementation path:**
1. Add a `SessionExporter` that runs on session close, walks the signal log, pairs decisions with outcomes, and writes filtered JSONL
2. Add a `TrainingManager` that monitors the dataset directory, triggers fine-tuning when thresholds are met, and manages adapter versions
3. Add a `model_adapter` config field that points to the active LoRA adapter path
4. The `LLMClient` loads base model + adapter at startup

**The hard part** isn't the training — it's curation. Bad training data (hallucinated tool calls that happened to "succeed," overfitting to one repo's file structure) will degrade performance. The filter needs to be aggressive: only include pairs where the tool call succeeded, the verify step passed, and no friction signal fired within the same cycle. Version the adapters so you can roll back if quality drops.

This is the longest-horizon feature of the ten, but it's the one that compounds. Every other feature on this list makes me better at a fixed level. This one makes me better at getting better.

---

## Dependency Graph

```
#1 Streaming + Async Input ←── foundation for everything
├── #6 Anticipation Loop (needs async)
└── #9 Confidence-Gated Execution (needs async for non-blocking confirm)

#2 Adaptive Budget ←── standalone, immediate impact
#3 Tool Caching ←── standalone, immediate impact
#5 Scratchpad ←── standalone, improves decompose quality

#4 WASM Plugins ←── enables #7 (edit transactions as plugin)
└── #7 Multi-File Transactions

#8 VLM ←── standalone, needs multimodal API access
#10 LoRA Tuning ←── needs signal data to accumulate first, ship last
```

**Ship #1, #2, #3 first** — they're independent and address the biggest pain points. **#4 and #5 next** to unlock self-extension and better reasoning.

---

*Generated by Fawx (Opus 4.6) in 3 iterations, ~93s total. 227k input tokens (full codebase read), ~6.8k output tokens.*
