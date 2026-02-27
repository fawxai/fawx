# OpenClaw → Fawx Architecture Analysis

*Deep dive into OpenClaw's design, gaps in our prior analysis, and actionable improvements for Fawx.*

---

## What I Read

### Previously Covered (4 docs)
1. **agent-loop.md** — lifecycle, serialized queueing, plugin hooks, streaming
2. **system-prompt.md** — modular sections, prompt modes, workspace bootstrap, lazy skills
3. **compaction.md** — context window management, auto-compaction, pruning
4. **context.md** — system prompt construction, tool costs, persistence

### Newly Covered (10 docs)
5. **architecture.md** — WebSocket gateway, components, pairing, wire protocol
6. **queue.md** — lane-aware FIFO, steer/followup/collect modes, debounce
7. **retry.md** — per-request retry, exponential backoff, provider-specific handling
8. **session.md** — session keys, lifecycle, daily/idle reset, send policy, DM scoping
9. **memory.md** — markdown files as memory, vector search, hybrid BM25+vector, pre-compaction flush
10. **streaming.md** — block streaming, Telegram drafts, chunking algorithm
11. **multi-agent.md** — isolated agents, bindings, per-agent sandbox+tools
12. **subagents.md** — spawn/announce lifecycle, tool policy, reduced prompt
13. **exec.md** — shell execution, PTY, host modes, allowlist
14. **skills.md** — gating, config injection, lazy loading, token impact
15. **tools/index.md** — full tool inventory
16. **session-tool.md** — session tools, cross-session messaging, ping-pong
17. **model-failover.md** — auth rotation, cooldowns, model fallback chain

---

## Gap Analysis: What I Missed Before

### 1. Queue Management Is Way More Sophisticated Than We Thought

**OpenClaw**: Lane-aware FIFO with 5 queue modes (`steer`, `followup`, `collect`, `steer-backlog`, `interrupt`). Messages can *steer* an active run (cancel pending tool calls at next boundary) or *collect* into a single followup turn. Per-session concurrency=1, global lane caps configurable. Debounce, cap, and overflow policy (`summarize` drops = bullet summary).

**Fawx gap**: We have no inbound message queuing at all. If the user sends a message while a tool loop is running, it either gets lost or creates a race. The `collect` mode is particularly clever — coalesce all queued messages into one turn.

**Action item**: Add message queuing to `ChatViewModel`. At minimum: buffer inbound messages during tool loop, drain them as a single followup turn when the loop completes. This directly addresses the "user sends correction mid-loop" problem.

### 2. Steer (In-Flight Injection) Is the Missing Piece for #451

**OpenClaw**: The `steer` mode can inject a message into the *currently running* tool loop, cancelling pending tool calls after the next tool boundary. This is how OpenClaw handles "stop doing that and do this instead."

**Fawx gap**: Once `sendMessage()` starts the tool loop in `ChatViewModel`, there's no way for the user to redirect it mid-loop. The user can only cancel entirely. Steer gives us a middle ground — "I said the Calendar app, not Settings."

**Action item for stuck detection (#451)**: Instead of just detecting stuck loops, implement a steer mechanism. When the stuck detector fires, inject a "⚠️ STUCK" system message into the *current* loop context, which is effectively self-steering.

### 3. Session Lifecycle & Reset Policy

**OpenClaw**: Sessions have a daily reset (default 4 AM local), idle timeout, and manual `/new` reset. Sessions are keyed hierarchically (`agent:<id>:<channel>:group:<id>`).

**Fawx gap**: Conversations in Fawx never expire. There's no idle timeout, no daily reset. Users accumulate context forever until they manually clear. The `maxMessages=20` trimming is a band-aid — it doesn't reset the conversation flow.

**Action item**: Add conversation expiry. Start simple: idle timeout (e.g., 4 hours). When expired, start fresh but preserve conversation history in the UI (just don't send it as context).

### 4. Retry & Failover Is a Full System

**OpenClaw**: Per-request retry with jitter + exponential backoff. Auth profile rotation within a provider. Model fallback chain across providers. Cooldowns with exponential backoff (1m → 5m → 25m → 1h cap). Billing disables (5h → 24h cap). Session-pinned auth profiles for cache friendliness.

**Fawx gap**: PR #195 added basic 429 retry with backoff, but we have no:
- Auth profile rotation (user only has one key)
- Model fallback chain (single model per conversation)
- Cooldown tracking
- Session-sticky auth

**Action item**: For MVP this is fine (single key), but the Key Wallet (Phase 2b/3) should implement auth rotation when multiple keys exist. Model failover is a natural extension of #456 (model curation).

### 5. Memory System Architecture

**OpenClaw**: Plain Markdown files as source of truth, vector-indexed with hybrid BM25+vector search. Pre-compaction memory flush (silent agentic turn before context is compacted). Chunking (~400 tokens, 80 overlap). Session memory indexing (experimental).

**Fawx gap**: We have `memory` tools in `PhoneTools.ALL` (27 tools total) but no actual memory persistence. The phone agent has no way to remember things across conversations. This is a significant gap for a phone-native agent — "remember that my dentist is at 3pm on Tuesday" should persist.

**Action item**: Not MVP, but horizon 2. Could use SharedPreferences for simple key-value memory, or SQLite for richer recall. The phone IS the storage layer — fits the zero-infrastructure principle.

### 6. Tool Execution Model: Boundaries, Not Steps

**OpenClaw**: Tools execute at *boundaries* — between model inference steps. The model outputs text and tool calls; tool calls execute; results feed back. There's no fixed "step count" per se — the model decides when to stop.

**Fawx gap**: We use `MAX_TOOL_STEPS=20` as a hard ceiling, but don't have concept of tool boundaries for injection/cancellation. The entire loop is opaque.

**Action item**: Refactor the tool loop to have explicit boundaries between steps. At each boundary: check for steer messages, check stuck detection, check cancellation. This is the architectural prerequisite for steer + stuck detection.

### 7. Modular Prompt Assembly

**OpenClaw**: System prompt is assembled from ~10 sections: tooling, safety, skills, workspace files, docs, sandbox, time, runtime. Each section is independently toggleable. Skills are a compact metadata list in the prompt — the model reads SKILL.md on demand (lazy loading).

**Fawx gap**: `PhoneAgentPrompts.kt` is a monolithic string. No sections. No conditional assembly. No way to include/exclude parts based on context (e.g., skip phone control instructions when accessibility isn't attached).

**Action item (PR 1: #449)**: This is exactly what the system prompt overhaul should do. Break the prompt into sections:
1. **Identity** — who you are
2. **Capabilities** — what tools you have (conditional on accessibility attachment)
3. **Phone control instructions** — how to use tools (skip if no accessibility)
4. **Efficiency rules** — minimize steps, direct commands
5. **Disambiguation** — "settings" = phone Settings, not Fawx settings
6. **Constraints** — what NOT to do
7. **Runtime context** — device info, current state

### 8. Overflow/Context Pressure Management

**OpenClaw**: When context fills up: (1) prune old tool results, (2) auto-compaction (summarize older turns), (3) pre-compaction memory flush. Graceful degradation.

**Fawx gap**: We have `maxMessages=20` hard trimming. When context fills up, old messages just disappear. No summarization, no pruning intelligence. Tool results (especially screenshots) eat context fast.

**Action item**: Implement smart context trimming:
- Prune old tool results first (keep last 2-3 tool results, summarize older ones)
- Keep system prompt + last N user/assistant turns intact
- Consider: drop screenshot base64 from history after processing (keep text description only)

### 9. Skills as Lazy-Loaded Metadata

**OpenClaw**: Skills appear in the prompt as a compact XML list (~97 chars + field lengths per skill). The model reads the full SKILL.md only when it decides to use the skill. This keeps base prompt small.

**Fawx gap**: All 27 tools are defined in `PhoneTools.ALL` with full JSON schemas, always sent. This burns context on tools the model may never use (like `clipboard_copy` or `file_write`).

**Action item**: Consider tool pruning based on task. When the user says "open Gmail," the model doesn't need `set_timer`, `clipboard_copy`, `file_read`, `file_write`, `memory_*`. Could implement a lightweight "tool selector" step or group tools into categories.

### 10. Block Streaming vs. Token Streaming

**OpenClaw**: Two-layer streaming — block streaming (send completed chunks as messages) and token streaming (Telegram draft updates). Smart chunking with code fence awareness, coalescing, and human-like pacing.

**Fawx gap**: Known SSE streaming parser bug — clearing instead of appending deltas. Even when fixed, we don't have any concept of progressive output during tool loops. The user sees nothing until the entire loop completes.

**Action item**: During tool loops, stream status updates to the UI: "Opening Settings...", "Reading screen...", "Tapping Wi-Fi...". This is what the overlay status panel does partially, but it could be richer. Not MVP, but high user impact.

---

## Patterns Worth Adopting (Priority Order)

### P0 — Must Have for MVP

1. **Modular prompt assembly** → System prompt overhaul (#449)
2. **Tool boundary injection** → Stuck detection foundation (#451)
3. **Self-steer on stuck** → Inject "⚠️ STUCK" at tool boundaries

### P1 — Should Have Soon

4. **Message queuing during tool loop** → Buffer + drain as single turn
5. **Smart context trimming** → Prune tool results, keep recent turns
6. **Conversation expiry** → Idle timeout + daily reset

### P2 — Horizon 2

7. **Tool grouping/pruning** → Only send relevant tools per task
8. **Progressive UI updates** → Stream tool loop status to overlay
9. **On-device memory persistence** → SharedPreferences or SQLite
10. **Model failover chain** → Key Wallet + multi-model routing

---

## What OpenClaw Does That Fawx Shouldn't Copy

1. **Multi-agent routing** — Fawx is one agent on one phone. No need for agent bindings.
2. **Webhook/cron infrastructure** — Phone agent is reactive to user input, not scheduled.
3. **Block streaming to messaging channels** — Fawx has one UI surface, not 15 channels.
4. **Plugin/skill ecosystem** — Fawx tools are phone-native, not extensible by third parties (yet).
5. **Session key hierarchies** — One user, one device, one conversation. Simple.

---

## Revised MVP Sprint Plan (Informed by Analysis)

### PR 1: System Prompt Overhaul (#449)
Now informed by OpenClaw's modular assembly pattern:
- Break into 7 sections (identity, capabilities, phone control, efficiency, disambiguation, constraints, runtime)
- Conditional sections based on accessibility attachment state
- Tool descriptions in prompt should be concise — model has full JSON schemas separately

### PR 2: Stuck Detection & Loop Guards (#451)
Now informed by OpenClaw's queue steer and tool boundary patterns:
- Add explicit **tool boundary checkpoints** in the loop
- At each boundary: check screen content hash, check step count, check for repeated actions
- On stuck detection: inject "⚠️ STUCK" message into context (self-steer pattern)
- Reduce MAX_TOOL_STEPS 20→12
- Warn after 2 consecutive identical screen reads

### PR 3: Overlay Auto-Hide (#457/PR #458)
Already in review. No changes from this analysis.

---

*Generated 2026-02-14. Source: OpenClaw docs at `/home/clawdio/.nvm/versions/node/v22.22.0/lib/node_modules/openclaw/docs/`*
