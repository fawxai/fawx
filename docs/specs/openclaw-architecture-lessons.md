# OpenClaw Architecture Deep Dive — Lessons for Citros

## What OpenClaw Gets Right (and Citros Should Learn From)

### 1. The Agent Loop is Not Just "Call LLM → Execute Tool → Repeat"

OpenClaw's loop is a **lifecycle pipeline** with well-defined phases:

```
Intake → Context Assembly → Model Inference → Tool Execution → Streaming → Persistence
```

Each phase has hook points (`before_agent_start`, `before_tool_call`, `after_tool_call`, `agent_end`). This means any component can intercept and modify behavior without touching the core loop.

**Citros today:** The loop in `ChatViewModel.sendMessage()` is a monolithic `while` loop. There are no hook points. Adding stuck detection, overlay hide/show, logging, and compaction all require editing the same function. This doesn't scale.

**What to do:** Extract the tool loop into its own class (`ToolLoop` or `AgentExecutor`) with defined lifecycle callbacks. The `ChatViewModel` should orchestrate, not implement.

### 2. Session Serialization Prevents Races

OpenClaw guarantees **one active run per session** through a lane-aware queue. Messages arriving during an active run are queued (with configurable modes: `collect`, `steer`, `followup`).

**Citros today:** The "Queue" button exists but doesn't work (#445). If a user sends a message during tool execution, it's unclear what happens. There's a `queuedMessage` field but the dispatch logic is fragile.

**What to do:** Implement proper run serialization. When a tool loop is active:
- New messages go to a queue
- Queue drains after the loop ends
- User can see queued messages and cancel them

This is simpler on-device (single user, single session) than OpenClaw's multi-session design.

### 3. Context Assembly is Explicit and Inspectable

OpenClaw builds the system prompt from discrete, labeled sections:
- **Tooling** (tool list + descriptions)
- **Safety** (guardrails)
- **Skills** (capability list with on-demand loading)
- **Workspace Files** (AGENTS.md, SOUL.md, etc. — injected into context)
- **Runtime** (host, model, time)
- **Heartbeats** (background check behavior)

Each section is separate, can be toggled, and the user can inspect what's injected via `/context list` and `/context detail`.

**Citros today:** The system prompt is a single hardcoded string in `PhoneAgentPrompts.kt`. There's no way to inspect it, no modularity, and no way to add/remove sections based on context (e.g., different prompts for different models, or different capabilities based on what's enabled).

**What to do:**
- Build the system prompt from composable sections (a `PromptBuilder` pattern)
- Include a runtime section ("Model: opus-4.6, Screen: attached, Overlay: hidden")
- Allow the system prompt to adapt based on state (accessibility attached? which model? what app is foregrounded?)
- Long-term: let users inspect/edit prompt sections (advanced settings)

### 4. Tool Results are Pruned and Managed

OpenClaw has **session pruning** that trims old tool results from the in-memory context before LLM calls. This is separate from compaction (which summarizes). Pruning keeps the context window manageable without losing conversation structure.

**Citros today:** Every tool result (including full screen dumps) stays in the conversation history. By step 10, the context is enormous — full of redundant screen content from 8 steps ago.

**What to do:**
- **Prune old screen content.** After step N, screen content from step N-3 and earlier should be summarized or removed from the conversation sent to the model.
- **Cap tool result size.** Screen content with 62 elements produces a LOT of text. Cap at the most relevant 20-30 elements.
- **Context compaction.** When approaching the model's context window, summarize earlier steps into "Steps 1-5: Opened Gmail, navigated to inbox, Compose button wasn't tappable."

### 5. The Queue Design Handles Real-World Messaging

OpenClaw's queue modes (`collect`, `steer`, `followup`, `steer-backlog`) solve a real problem: what happens when a human sends multiple messages while the agent is working?

- **Collect** (default): coalesce queued messages into a single followup turn
- **Steer**: inject into the current run (cancel pending tool calls at next boundary)
- **Followup**: queue for next turn after current run ends

**Citros today:** Has `queuedMessage` (singular) that dispatches after the loop ends. No coalescing, no steering, no cancel-and-redirect.

**What to do (MVP):**
- Fix #445 (queue button works)
- Support at least `followup` mode (queue message, execute after current loop)
- Add `steer` mode: inject "The user says: {message}" into the next tool result, signaling the agent to adjust

### 6. Sub-Agents Are the Right Abstraction for Complex Tasks

OpenClaw spawns sub-agents in isolated sessions with their own context, model, and tool policy. The result is announced back to the main session.

**Citros implication:** For complex multi-app tasks ("send an email about tomorrow's calendar"), the on-device agent could:
1. Main agent plans the task
2. Spawn a "sub-task" for each app interaction (open Calendar, read events → open Gmail, compose with that info)
3. Sub-tasks share results via a lightweight context

This is Horizon 2, but the architecture should allow it.

### 7. Memory is Plain Files, Not a Database

OpenClaw's memory is Markdown files on disk + optional vector search on top. This is simple, inspectable, and portable.

**Citros today:** No persistent memory at all. Conversations vanish.

**What to do (MVP):**
- Store conversation summaries in a local file (app-internal storage)
- When starting a new session, inject the last session's summary as context
- Simple enough: just a `conversations/` directory with JSON or Markdown files

**Horizon 2:**
- Vector search over past conversations (Room/SQLite + embeddings)
- App navigation patterns stored as learned paths

### 8. The System Prompt Includes Runtime State

OpenClaw injects real-time state into the prompt: current time, timezone, model name, thinking level, OS, host. This means the model knows what it's working with.

**Citros should inject:**
- Current model (Opus vs Sonnet — the agent should know its own capability level)
- Screen reader status (attached/detached)
- Current foreground app (if known)
- Overlay state (hidden during tool loop)
- Available tools (not all tools work without accessibility service)
- Battery/connectivity status (Horizon 2)

### 9. Tool Execution Has Pre/Post Hooks

OpenClaw's `before_tool_call` / `after_tool_call` hooks allow plugins to:
- Modify tool parameters before execution
- Transform tool results before they're sent to the model
- Log/observe every tool call

**Citros today:** Tool execution is inline in the `while` loop. Adding stuck detection requires modifying the loop directly.

**What to do:** Add pre/post tool execution hooks:
```kotlin
interface ToolExecutionHook {
    fun beforeToolCall(call: ToolCall, screenContent: ScreenContent?): ToolCall
    fun afterToolCall(call: ToolCall, result: String, screenContent: ScreenContent?): String
}
```
- `StuckDetectionHook` tracks screen hashes and injects warnings
- `LoggingHook` logs structured data (already done via CitrosLoop/CitrosAgent tags)
- `OverlayHook` manages hide/show (already partially done)

### 10. Streaming and Typing Indicators

OpenClaw streams assistant responses and shows typing indicators immediately when a message is queued. This gives the user instant feedback.

**Citros today:** No streaming. The user sees nothing until the full response comes back. With Opus taking 3-16 seconds per API call, this means the user stares at "Thinking..." for long periods.

**What to do (MVP):**
- Show step-by-step progress in the overlay: "Opening Gmail... Tapping Compose... Typing email..."
- This doesn't require streaming from the API — just surface tool execution names in real-time

**Horizon 2:**
- Actual SSE/streaming from Anthropic API (the streaming parser exists but has bugs, per known issue)

---

## Architectural Recommendations for Citros MVP

### Extract the Agent Loop

```
ChatViewModel (orchestrator)
  └── AgentExecutor (loop logic)
       ├── PromptBuilder (context assembly)
       ├── ToolRunner (execution + hooks)
       ├── ScreenManager (read/refresh/hash)
       └── ProgressReporter (UI updates)
```

### Prompt Assembly Pipeline

```kotlin
class PromptBuilder {
    fun full(): String {
        return buildString {
            append(capabilitiesSection())    // tools + strategy
            append(runtimeSection())         // model, screen status, foreground app
            append(rulesSection())           // disambiguation, type_text doesn't submit
            append(stuckRecoverySection())   // what to do when stuck
        }
    }
    
    fun trimmed(): String {
        // For action loop continuations — smaller prompt
        return buildString {
            append(coreRulesSection())
            append(runtimeSection())
        }
    }
}
```

### Tool Result Pipeline

```
Raw Result → Size Cap → Screen Hash Check → Stuck Detection → Context Prune → Send to Model
```

### Priority Order

1. **System prompt overhaul** (PR 1) — immediate, zero-architecture-change impact
2. **Stuck detection** (PR 2) — simple guards, minimal architecture change
3. **Overlay hide** (PR 3) — already written
4. **Extract AgentExecutor class** — refactor that enables everything below
5. **Context pruning** — remove stale screen dumps from conversation
6. **Queue fix** — proper message queuing during tool loop
7. **Persistent memory** — survive app restarts
8. **PromptBuilder** — composable, state-aware prompts

Items 1-3 ship the MVP. Items 4-8 make the architecture sustainable for Horizon 2.
