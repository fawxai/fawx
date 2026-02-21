# Citros Architecture Roadmap

*From "demo that sometimes works" to "phone agent that just works."*
*Synthesized from OpenClaw architecture deep dive + gap analysis + MVP sprint spec.*

---

## Where We Are (updated 2026-02-21)

Horizons 0 and 1 are **COMPLETE**. Horizon 2 is **~60% complete** (7/12 items shipped, 5 new items added from v2 spec). The agent has a proper architecture:

- **AgentExecutor** with boundary checkpoints (stuck detection, steer, cancellation, action verification)
- **Modular system prompt** assembled from sections (identity, tools, strategy, recovery, disambiguation, rules, runtime)
- **Voice I/O** working (SherpaOnnx on-device STT + Android TTS, VoiceAccumulator, mic in ChatActivity)
- **On-device memory** (SQLite learn/recall), **conversation lifecycle** (idle timeout, daily reset)
- **Context trimming** (category-aware ContextCompactor, transformContext hook)
- **Steer UI** — user can redirect mid-task via message injection at tool boundaries
- **Per-action verification** — catches failed UI actions on first attempt
- **API tools** (web_search, web_fetch via Citros proxy + Brave), **TinyFish web_browse** for browser automation
- **SSE streaming**, **wallet hardening**, **persona files** (SOUL.md/USER.md/IDENTITY.md in prompt)
- **Confidence-gated disambiguation** — think before asking, calibrate by stakes
- **Progressive status updates** — human-readable tool output in chat/overlay
- 3 providers: Anthropic, OpenAI, OpenRouter

**Remaining H2 gaps:** Tool Grouping (#557), Model-Aware Prompt Tuning (#558), plus 5 features from `agentic-loop-v2.md` not previously tracked (Action Policy Engine, User Interruption Detection, Sensor Context, Cost Budgets, Privacy-Sensitive Apps).

## Agentic Loop v2 Spec Status

`docs/agentic-loop-v2.md` is the detailed architectural spec for the agent brain. Cross-reference with roadmap:

| v2 Section | Description | Roadmap | Status |
|-----------|-------------|---------|--------|
| §3 Clean Loop | continueAfterTools, no synthetic messages | H1.1 | ✅ Shipped |
| §4 Model Floor | Sonnet-minimum for action loop security | H1 (PR #496) | ✅ Shipped |
| §5 Implicit Observation | Screen content in UI-mutating tool results | H1.1 | ✅ Shipped |
| §6 Subtask Decomposition | `subtask` tool, isolated context, depth limits | H3.1 | 🔲 Not started |
| §7 Output Classification | SHOW/SHOW_DIMMED/HIDE, TTS routing | H2.3 (PR #670) | ✅ Shipped |
| §8 Metacognitive Layer | Three-timescale reflection, self-improvement | H3.3 | 🔲 Not started |
| §9 Tool Gating | Gate tools on accessibility state | H1 (PR #496) | ✅ Shipped |
| §12.1 Action Policy Engine | Per-tool ALLOW/CONFIRM/DENY | H2.7 | 🔲 Not started |
| §12.2 Voice I/O | STT/TTS integration | H2 Voice | ✅ Shipped |
| §12.3 Local LLM Routing | Security evaluation for local models | Deferred | 🔲 Blocked (security) |
| §12.4 Proactive Behavior | Agent-initiated loops (briefings, reminders) | H3.4 | 🔲 Not started |
| §12.5 Sensor Context | Battery/location/time in prompt | H2.8 | 🔲 Not started |
| §12.6 Privacy-Sensitive Apps | Selective screen blindness | H2.6 | 🔲 Not started |
| §12.7 Web Search | web_search/web_fetch tools | H2 (PR #497) | ✅ Shipped |
| §12.8 Cost Tracking | Token tracking + budget limits | H2.11 | ⚠️ Partial (#531) |
| §12.9 User Interruption | Detect user touch/app switch, pause | H2.5 | 🔲 Not started |
| §12.10 Rust Migration | Kotlin → Rust daemon transition | H3.8 | 🔲 Not started |

**v2 Implementation Phases:**
- Phase 1 (Clean Loop Foundation): ✅ COMPLETE
- Phase 2 (Subtask Decomposition): 🔲 Not started → H3.1
- Phase 3 (Metacognitive Layer): 🔲 Not started → H3.3
- Phase 4 (Refinement & Long Tail): 🔲 Not started → H3+

## Where We Need To Be

Finish H2 token optimization, then move to H3 ecosystem features:
- Cheaper model tiers viable (Haiku/GPT-4o-mini) via tool grouping + prompt tuning
- Model failover chains, multi-step task planning, learned navigation patterns
- Community knowledge sharing (shared intelligence)
- External browser API as primary web interaction path

---

## The Plan: 4 Horizons

### Horizon 0: Ship MVP ✅ COMPLETE

The immediate deliverables. Zero architecture changes — just prompt engineering and safety nets that work within the current monolithic loop.

#### PR 1: System Prompt Overhaul (#449)
**Branch:** `feat/system-prompt-449` | **Files:** `PhoneAgentPrompts.kt` only

Replace the generic tool-listing prompt with a structured, strategy-focused prompt assembled from sections:

| Section | Purpose | Conditional? |
|---------|---------|-------------|
| Identity | "You are Citros, an AI agent that controls this Android phone" | No |
| Tools | Organized by category (navigation, interaction, observation, memory) | Yes — skip phone tools when accessibility detached |
| Strategy | The "always do this" pattern: open app → read screen → act → verify from results | No |
| Recovery | What to do when tap fails, when stuck, when "No target app visible" | No |
| Disambiguation | "Open settings" = Android Settings, not Citros settings | No |
| Rules | type_text doesn't submit, element IDs are ephemeral, be concise | No |
| Runtime | Model name, screen reader status, current time | Yes — assembled at call time |

**Why first:** Highest leverage. Changes zero code but teaches the agent *strategy* instead of just listing tools. The recovery section ("if screen hasn't changed after 2 actions, you're stuck") is the prompt-side of stuck detection — belt AND suspenders with PR 2.

**Informed by OpenClaw:** Their system prompt is assembled from ~10 modular sections with runtime injection. We adopt the same pattern in Kotlin: a `buildSystemPrompt()` function that concatenates sections, with a runtime block that includes model name and accessibility status.

#### PR 2: Stuck Detection & Loop Guards (#451)
**Branch:** `fix/stuck-detection-451` | **Files:** `ChatViewModel.kt`, `PhoneAgentApi.kt`

Client-side safety net for when the prompt instructions aren't enough:

1. **MAX_TOOL_STEPS: 20 → 12** — successful tasks take 3-7 steps; 12 is generous
2. **Screen content hash tracking** — rolling window of last 3 hashes
3. **Self-steer on stuck** — when 3 consecutive screen hashes are identical, inject `⚠️ STUCK: The screen has not changed in 3 actions. Try a different approach or tell the user what's blocking you.` into the tool result
4. **Consecutive wait detection** — after 2+ waits with no screen change, inject `⚠️ Waiting more won't help. Take a different action.`
5. **Progress logging** — `loopMetrics: step=N, uniqueScreens=M, consecutiveWaits=W`

**The key insight from OpenClaw:** This is a "self-steer" pattern. OpenClaw's `steer` queue mode injects user messages into running tool loops at tool boundaries. We're doing the same thing — injecting a system-generated message into the context at the boundary between tool steps. The agent sees the ⚠️ message as part of the tool result and adjusts its behavior. No new architecture needed.

#### PR 3: Overlay Auto-Hide (#457)
**Branch:** `fix/overlay-blocks-touch-457` | **PR #458** in Claude review

Already implemented. Hides overlay for entire tool loop duration, not just screenshots. Hook pattern bridges `:core` ↔ `:chat` module boundary. Double-hide guard in OverlayService makes nested calls safe.

#### MVP Success Criteria
After all 3 PRs, these work on first try with Opus or Sonnet:
- ✅ "What's on my calendar tomorrow?" — < 8 steps
- ✅ "Send a test email to joe@citros.ai" — < 10 steps  
- ✅ "Open Settings" — 1 step
- ✅ "Set a timer for 5 minutes" — < 6 steps
- ✅ "Hey, how are you?" — text response, 0 tools

---

### Horizon 1: Loop Architecture ✅ COMPLETE

Refactor the tool loop from a monolithic `while` in `ChatViewModel` to a proper agent executor with boundaries, hooks, and message injection. This is the prerequisite for everything in Horizons 2-3.

#### 1.1 Extract AgentExecutor

Pull the tool loop out of `ChatViewModel.sendMessage()` into its own class:

```
ChatViewModel (orchestrator — UI state, message dispatch)
  └── AgentExecutor (loop lifecycle)
       ├── PromptBuilder (context assembly — sections, runtime injection)
       ├── ToolRunner (execution + pre/post hooks)
       ├── ScreenManager (read, refresh, hash tracking)
       └── ContextManager (trimming, pruning, token estimation)
```

**Why:** Currently, stuck detection, overlay hooks, logging, tool execution, API calls, and UI state updates are all interleaved in one function. Every new feature means editing that function. The AgentExecutor separates *what* the loop does (execute tools) from *how* it's presented (ChatViewModel UI state).

**Informed by OpenClaw:** Their agent loop has explicit lifecycle phases (intake → context assembly → inference → tool execution → streaming → persistence) with hook points at each boundary. We adopt the boundaries without the full plugin system.

#### 1.2 Tool Boundary Checkpoints

At each boundary between tool execution steps, the AgentExecutor runs a checkpoint:

```kotlin
interface ToolBoundaryCheck {
    /** Return null to continue, or a string to inject into context */
    fun check(state: LoopState): String?
}

data class LoopState(
    val step: Int,
    val maxSteps: Int,
    val screenHashes: List<Int>,
    val consecutiveWaits: Int,
    val lastToolName: String?,
    val pendingUserMessages: List<String>,  // for steer
    val tokenEstimate: Int
)
```

Built-in checks:
- `StuckDetectionCheck` — screen hash repetition, consecutive waits (from Horizon 0 PR 2, now extracted)
- `SteerCheck` — if user sent a message during the loop, inject it as context
- `ContextPressureCheck` — if token estimate is high, trigger pruning
- `CancellationCheck` — if user cancelled, exit loop

**Informed by OpenClaw:** Their `steer` mode cancels pending tool calls at the next tool boundary. Our checkpoints are simpler (inject, don't cancel) but architecturally equivalent.

#### 1.3 Message Queuing

When a tool loop is active, buffer inbound messages and process them at loop boundaries:

- **Steer mode** (default): At the next tool boundary, inject "The user says: {message}" into the tool result. The agent adjusts behavior.
- **Queue mode**: Hold messages until the loop finishes, then dispatch as a single followup turn (coalescing — OpenClaw's `collect` pattern).

This fixes #445 (queue button) and enables the "I said Calendar, not Settings" redirect.

**Informed by OpenClaw:** Their 5 queue modes (`steer`, `followup`, `collect`, `steer-backlog`, `interrupt`) are overkill for a single-user phone agent. We need exactly 2: steer (redirect mid-loop) and collect (hold for after).

#### 1.4 Smart Context Trimming

Replace the hard `maxMessages=20` trim with intelligent pruning:

**Priority order for context budget:**
1. System prompt (never trimmed)
2. Last user message + current tool loop (never trimmed)
3. Recent user/assistant conversation turns (keep last 5-8)
4. Recent tool results (keep last 2-3 full, summarize older ones)
5. Old tool results → replace with one-line summary: "Step 3: Opened Gmail inbox (12 elements)"
6. Screenshot descriptions → keep text, drop any base64 references

**Informed by OpenClaw:** They have separate mechanisms — pruning (trim old tool results in-memory) and compaction (summarize older conversation to disk). We only need pruning for now. Their insight about tool results being the biggest context consumer is dead-on — a single `read_screen` with 60+ elements produces ~2K tokens.

---

### Horizon 2: Intelligence Layer (~60% complete)

Features that make the agent *smarter*, not just *more reliable*.

#### 2.1 On-Device Memory ✅ (PR #504)

Store facts and user preferences that survive conversation resets:

- **Storage:** SQLite (Room) on-device — fits zero-infrastructure principle
- **Write:** `remember(content)` tool already exists — wire it to actual persistence
- **Read:** `recall(query)` tool — simple keyword + recency search (no vector needed at first)
- **Inject:** On conversation start, inject "Here's what you know about this user: ..." from recent memories

**Informed by OpenClaw:** Their memory is plain Markdown files + optional vector search. Phone equivalent: SQLite rows with timestamp + content + optional tags. Keep it simple — vector search is a nice-to-have, not a must-have.

#### 2.2 Conversation Lifecycle ✅ (Jarvis)

- **Idle timeout:** After 4 hours of inactivity, start a fresh conversation context (but keep chat history visible in UI)
- **Daily reset:** Optional setting — new context each day
- **Context summary on reset:** When resetting, generate a 2-3 sentence summary of the old conversation and inject it as "Previous conversation context"

**Informed by OpenClaw:** Their session lifecycle (daily reset at 4 AM + idle timeout + manual `/new`) is well-designed. We adapt: phone users expect persistent chat UI but fresh context. Show all messages in scroll history, but only send recent ones to the model.

#### 2.3 Progressive Status Updates ✅ (PR #670)

Stream tool execution status to the UI during loops:

```
Opening Gmail...          (step 1)
Reading inbox...          (step 2, auto)
Tapping Compose...        (step 3)
Typing recipient...       (step 4)
```

This replaces the current "Thinking..." with real-time progress. Doesn't require API streaming — just surface the tool name being executed.

**Informed by OpenClaw:** Their block streaming and typing indicators give users instant feedback. We don't need streaming from the API — tool execution names are available synchronously.

#### 2.4 Per-Action Verification ✅ (PR #671)

Upgrade from passive stuck detection (screen hash repetition) to **active verification after every action**:

1. **State snapshot** before action — capture screen hash, focused element, package name
2. **Execute action** — tap, type, scroll, etc.
3. **Verify state changed** — re-read screen, compare against pre-action snapshot
4. **React to result:**
   - State changed as expected → continue
   - State unchanged → retry with alternative strategy (coordinate tap → text tap → scroll + retry)
   - Unexpected state → screenshot + reassess before next action

This is the single highest-leverage reliability improvement. Stuck detection catches failure after 3+ repeated screens. Verification catches it on the **first attempt**, cutting wasted steps in half.

**Implementation:** New `ActionVerifier` interface in AgentExecutor, called after every `ToolRunner.execute()`. Lightweight — just a screen hash comparison + optional element check. No extra API calls.

**Informed by real-world testing (2026-02-21):** Flight booking flow on Google Flights showed the agent tapping elements that didn't respond, then continuing without noticing. Per-action verification would have caught this immediately and triggered a retry or alternative strategy.


#### Additional H2-Era Features Shipped (not in original roadmap)

| PR | Feature |
|----|---------|
| #497 | API Tools (web_search + web_fetch via Citros proxy) |
| #501 | Client deduplication |
| #503 | SSE streaming |
| #514, #516 | Wallet hardening |
| #521 | Batch tool results |
| #524 | Prompt fix |
| #530 | transformContext hook (H2 prerequisite) |
| #531 | Token usage tracking (H2 prerequisite) |
| #559 | Onboarding fixes |
| #598 | Agent bones — persona files (SOUL.md/USER.md/IDENTITY.md) in prompt |
| #599 | TinyFish web_browse integration |
| #602 | UX polish batch (API key masking, markdown rendering, voice-steer) |
| #607 | Confidence-gated disambiguation |
| #654 | Voice silence threshold + VoiceAccumulator extraction |
| #656 | Citros search proxy (zero-config web search) |
| #662 | Ghost input fix (accessibility eventTypes) |
| #666 | Orphaned tool_result fix (Message.copy() stale blocks) |
| #667 | JSON tool display verbosity |
| #668 | Notes app navigation fix |
| #670 | Tool output verbosity (OutputClassifier.formatStatus) |
| Voice I/O MVP | SherpaOnnx STT + Android TTS + mic in ChatActivity (#556 closed) |
#### 2.5 User Interruption Detection ✅ (PR #672, #673)

*Source: agentic-loop-v2.md §12.9*

Detect when the user takes control mid-task and handle gracefully:

1. **Screen change detection** — foreground app changes without agent action → user switched apps. Pause loop.
2. **User touch detection** — accessibility service distinguishes agent-injected events from user touches. Any user touch during execution → pause.
3. **Interruption protocol** — agent pauses and asks: "I was working on [task]. Want me to continue or cancel?"
4. **State preservation** — conversation history and step progress preserved for resume.

Builds on existing steer infrastructure (boundary checkpoints). The difference: steer is explicit (user types a message), interruption is implicit (user touches screen).

#### 2.6 Privacy-Sensitive App Handling 🔲

*Source: agentic-loop-v2.md §12.6, SPEC.md §6.1*

Selective screen blindness for apps on a user-configured privacy list (banking, health, etc.):

- When a privacy-listed app is in the foreground, screen content is **not** appended to tool results
- Agent receives: `"SCREEN: [Privacy mode — screen content hidden for this app. Ask the user for guidance if needed.]"`
- Agent can still execute blind actions (press_back, press_home) but cannot observe results
- Privacy list managed in Settings

This is a security/trust feature — users need to feel safe that their banking app screens aren't being sent to cloud LLMs.


---

#### 2.7 Action Policy Engine 🔲

*Source: [agentic-loop-v2.md §12.1](../agentic-loop-v2.md#121-action-policy-engine-spec-353), SPEC.md §3.5.3*

Per-tool security gates that intercept tool calls before execution. The policy engine is the hard boundary between what the model wants and what actually happens.

| Tool Category | Default Policy | Notes |
|--------------|----------------|-------|
| UI navigation (tap, swipe, scroll, back, home) | ALLOW | Standard interaction |
| Text entry (type_text) | ALLOW | Content reviewed by model |
| App launch (open_app) | ALLOW | First-time app: CONFIRM |
| Observation (read_screen, screenshot) | ALLOW | Read-only |
| Internal (think, wait) | ALLOW | No side effects |
| Messages/email (detected by screen context) | CONFIRM | High-stakes outbound |
| Financial actions (detected by screen context) | DENY (v1) | Biometric gate in v2 |
| Install/uninstall | CONFIRM | System modification |
| Subtask | ALLOW | Inherits parent policy |

**Phase 1:** Lightweight check in AgentExecutor. **Full engine:** Signed config, capability grants, comes with Rust daemon.

#### 2.8 Sensor Context 🔲

*Source: agentic-loop-v2.md §12.5. Related: #344 (status bar awareness)*

Inject device state into system prompt as a lightweight prefix:

```
DEVICE STATE: Battery 72%, WiFi connected, Location: Denver CO, 4:15 PM MST
```

Informs agent decisions:
- Don't start a 20-step task at 5% battery
- Warn if cloud-dependent task requested while offline
- Contextualize location-aware requests ("nearby restaurants")
- Affect proactive behavior suppression (quiet hours)

Lightweight — reads Android system APIs, no extra permissions beyond what's already granted.

#### 2.9 Tool Grouping 🔲 (#557)

Divide the 27 tools into categories and only send relevant ones:

| Group | Tools | When to include |
|-------|-------|-----------------|
| Core | open_app, tap, tap_text, type_text, scroll, swipe, press_back, press_home, read_screen | Always (when accessibility attached) |
| Extended | long_press, screenshot, paste, wait | Always |
| Notifications | read_notifications, tap_notification, dismiss_notification, reply_notification | Always |
| Timer/Alarm | set_timer, set_alarm | When user mentions time-related task |
| Files | read_file, write_file, list_files | When user mentions files/notes |
| Memory | remember, recall | Always |
| Clipboard | clipboard_copy, clipboard_read | When user mentions copy/paste |

**Why it matters:** 27 tool schemas is ~3-4K tokens. Dropping to 15 core tools saves ~1.5K tokens per turn — that's meaningful context budget, especially on Haiku.

**Informed by OpenClaw:** Their skills system is lazy-loaded metadata (~97 chars per skill in prompt, full SKILL.md read on demand). Full lazy loading is overkill for us, but grouping achieves 80% of the benefit with 20% of the complexity.

#### 2.10 Model-Aware Prompt Tuning 🔲 (#558)

Different prompts for different model tiers:

- **Opus/GPT-5:** Full prompt with strategy section — model is smart enough to follow complex instructions
- **Sonnet/GPT-4o:** Concise prompt — strip examples, rely on tool schemas more
- **Haiku/GPT-4o-mini:** Minimal prompt — core rules only, fewer tools, tighter step limits

**Informed by OpenClaw:** They have prompt modes (full/minimal/none) for main agents vs sub-agents. Same principle: less capable models need simpler instructions, not more.

#### 2.11 Cost Tracking & Budgets ⚠️ Partial

*Source: agentic-loop-v2.md §12.8. Token tracking shipped: PR #531*

- ✅ **Per-task token tracking** — count input/output tokens across API calls (shipped #531)
- 🔲 **Budget limits** — user-configurable daily/monthly spending cap. Loop refuses new tasks when exhausted.
- 🔲 **Per-task cost display** — opt-in cost-per-task in chat UI (transparent reporting)
- 🔲 **Reflection cost control** — skip post-task reflection when budget is low
- 🔲 **Subtask cost inheritance** — subtask tokens count toward parent task total


---

### Horizon 3: Ecosystem (3+ months out)

#### 3.1 Subtask Decomposition & Multi-Step Planning 🔲

*Source: agentic-loop-v2.md §6 (v2 Phase 2). Full API design + execution model: [agentic-loop-v2.md §6](../agentic-loop-v2.md#6-subtask-decomposition)*

A `subtask` tool enables the model to decompose complex goals into isolated sub-loops:

```kotlin
val SUBTASK = Tool(
    name = "subtask",
    inputSchema = mapOf(
        "goal" to "Clear description of what the sub-task should accomplish",
        "success_criteria" to "How to determine if the sub-task succeeded",
        "max_steps" to "Maximum tool steps (default: 10)"
    )
)
```

**Execution:**
1. New `PhoneAgentApi` instance with fresh context (isolation)
2. Sub-task goal as user message, success criteria in system prompt
3. Same model config as parent, shared `ScreenReader`
4. Returns structured result: `{status, result, steps_used, summary}`
5. Orchestrator decides: retry, reformulate, proceed, or report

**Constraints:**
- Max recursion depth: 3 levels (orchestrator → subtask → sub-subtask)
- Each level has its own step counter; parent increments by 1 per subtask call
- Cancellation propagation via shared token
- Wall-clock timeout: 60s default per subtask

**The model decides** whether to decompose. Simple tasks use regular tools directly. The model's planning ability is the router, not a heuristic we build.

This is NOT OpenClaw's sub-agent system (we don't need isolated sessions). It's a planning layer on top of the tool loop.

#### 3.2 Model Failover Chain
When Key Wallet supports multiple keys/providers: auth rotation with cooldowns, model fallback chain, session-sticky auth. Direct port of OpenClaw's failover system.

#### 3.3 Metacognitive Layer & Learned Patterns 🔲

*Source: agentic-loop-v2.md §8 (v2 Phase 3). Full reflection design + storage: [agentic-loop-v2.md §8](../agentic-loop-v2.md#8-metacognitive-layer--self-awareness-and-self-improvement). Related: #349 (app nav maps), #350 (telemetry), #650 (scoped knowledge)*

The agent reflects, learns, and improves over time. Three timescales:

**In-Task (Real-Time Self-Monitoring):**
- Loop detection: "I've attempted the same action 3 times — I'm stuck."
- Efficiency awareness: "I've used 8 steps for what should be a 3-step task."
- Strategy pivoting: "Scrolling isn't finding it. Let me try search."
- Implemented via `think` tool + system prompt SELF-MONITORING framing

**Post-Task (Structured Reflection):**
- After every task: async self-evaluation (doesn't block response)
- Records: outcome, steps_used, efficiency_rating, observations, learned patterns
- Stored via memory system with tag taxonomy: `self-reflection`, `self-improvement`, `app-pattern:<app>`, `strategy:<category>`, `failure-analysis`

**Cross-Task (Pattern Evolution):**
- App-specific knowledge: "Gmail takes 2s to load. Settings is alphabetical."
- Strategy preferences: "Search > scrolling for long lists."
- Efficiency baselines: "Simple open-app: 1-2 steps. Compose-and-send: 5-8 steps."
- Failure patterns: "Calendar widget doesn't respond to taps — open full app."

**Self-Improvement Guardrails:**
- Strategy changes: free (just using memory to inform decisions)
- Prompt changes: stored as proposals, not auto-applied
- Safety rules: immutable — self-improvement cannot relax model floor, confirmation gates, or audit
- All insights visible/auditable by user

**Learned Navigation Patterns** (original H3.3): Store successful paths ("compose email: open Gmail → tap Compose FAB") in scoped knowledge. Inject into system prompt when agent interacts with that app. Connects to Open Question #7 (scoped agent knowledge).

#### 3.4 Proactive Agent Behavior 🔲

*Source: [agentic-loop-v2.md §12.4](../agentic-loop-v2.md#124-proactive-agent-behavior), SPEC.md §3.4 Phase 4. Issue: #597*

Agent-initiated loops — the agent starts tasks without user prompting:

```kotlin
sealed class LoopTrigger {
    data class UserMessage(val text: String) : LoopTrigger()
    data class Notification(val content: NotificationContent) : LoopTrigger()
    data class Schedule(val trigger: ScheduledTrigger) : LoopTrigger()
    data class ContextChange(val event: ContextEvent) : LoopTrigger()
}
```

Examples: morning briefing (calendar + weather + unread messages), calendar event reminders, notification summaries, low battery warnings.

Proactive loops run the same orchestration path but with different entry context. The model receives "A calendar event is in 30 minutes" instead of a user message.

**Off by default.** Each proactive behavior is individually opt-in in Settings. Quiet hours suppression based on sensor context (H2.8).

#### 3.5 External Browser Automation API (TinyFish) ✅ (PR #599 — basic integration)

Chrome's accessibility layer is fundamentally broken for text input — fields report `active=false, focused=false`, making `type_text` unreliable in web views. Instead of fighting the accessibility layer, delegate web interaction to a purpose-built browser automation API.

- **TinyFish** (or equivalent) provides deterministic web interaction: navigate, click, type, extract content
- Agent uses `web_browse` tool for interactive web tasks (booking flows, form filling)
- `web_search` / `web_fetch` remain the default for information retrieval (no browser needed)
- Chrome on-device is the last-resort fallback, not the primary path

**Policy default:** Information tasks → web_search/web_fetch. Interactive tasks → TinyFish API. Chrome → only when user explicitly requests or both other paths fail.

**Why this matters for scaling:** The agent doesn't need to learn Chrome's quirks on every device. A reliable web API means web interaction "just works" regardless of Chrome version, OEM skin, or accessibility bugs. The agent focuses on *what* to do, not *how* to physically interact with a browser.

**Status (2026-02-21):** In contact with TinyFish (Gargi, Dev Marketing). Demo of on-device accessibility layer requested as proof of working foundation. TinyFish key delivery planned via `/api/keys` endpoint.

#### 3.6 Gateway Integration (Optional)
For power users who want to control their phone from a VPS:
- Phone as an OpenClaw node
- Gateway sends commands, phone executes
- Screen content relayed back

This is the horizon 2-3 escape hatch mentioned in the product principle. NOT the core product.

#### 3.7 Shared Intelligence — Community Knowledge Pool

The scaling flywheel: every user's agent improves every other user's agent.

**The problem:** Individual agents learn slowly. One user discovers that "Google Flights date picker needs a scroll before the date is visible" — that insight dies on their device. Next user hits the same wall.

**The solution:** Anonymous, versioned pattern sharing across the Citros user base.

```
User A's agent learns pattern → upload to community pool (anonymous)
                                       ↓
                              pattern tagged: app=com.google.android.apps.travel
                                             version=15.2.1
                                             confidence=3
                                             category=navigation
                                       ↓
User B's agent encounters same app → pull relevant patterns → inject into prompt
```

**Key design constraints:**
- **Privacy:** Patterns are behavioral ("tap the second row to open date picker"), never personal (no user data, no content, no credentials)
- **Versioning:** Patterns tagged with app package + version code. UI changes invalidate old patterns automatically
- **Confidence:** Community patterns start at low confidence, get reinforced by successful reuse across users
- **Opt-in:** Users choose whether to contribute patterns and/or consume community patterns
- **Staleness:** Patterns that repeatedly fail for new users get auto-deprecated

**Network effect:** This is the moat. The more users, the better every agent gets. New apps get community knowledge within days of release. UI changes get adapted within hours across the user base, not weeks per individual user.

**Tier integration:**
- **BYO/Base:** Consume community patterns (read-only)
- **Super:** Contribute + consume (write + read)
- **Enterprise:** Private pattern pools (org-specific knowledge stays internal)

**Informed by:** OpenClaw's skill sharing model (community skills on clawhub.com), but applied to runtime-learned behavioral patterns instead of authored skill files.

---

#### 3.8 Rust Daemon Migration Path 🔲

*Source: [agentic-loop-v2.md §12.10](../agentic-loop-v2.md#1210-rust-daemon-migration-path), SPEC.md §3.4*

When the Rust daemon (ct-agent) takes over orchestration:

- `PhoneAgentApi.continueAfterTools()` → `orchestrator::continue_loop()`
- Tool execution routes through Unix socket IPC to Kotlin companion (accessibility) or `/dev/input` (root)
- Metacognitive layer translates directly — reflection is LLM-agnostic
- Policy engine in Rust (ct-security) replaces Kotlin lightweight check
- WASM skill system (ct-skills) exposed via Unix socket IPC or JNI

Design Kotlin interfaces so Rust equivalents are obvious. Same method signatures, same data flow, same contracts.

---

## Architecture Diagram: Current vs Target

### Current (Horizon 0)
```
User Message
  → ChatViewModel.sendMessage()           ← everything lives here
      → Build system prompt (monolithic)
      → API call
      → while (hasToolCalls && step < 20)
          → Execute tool
          → Refresh screen
          → API call
      → Display result
```

### Target (Horizon 1)
```
User Message
  → ChatViewModel                          ← UI state only
      → MessageQueue.enqueue()
      → AgentExecutor.run()
          → PromptBuilder.build(state)     ← modular, conditional
          → API call
          → for each tool boundary:
              → ToolRunner.execute(call)
              → ScreenManager.refresh()
              → BoundaryCheckpoint.run()   ← stuck, steer, cancel, context
              → ContextManager.trim()      ← prune old results
              → API call
      → Display result
      → MessageQueue.drain()               ← queued messages → next turn
```


---

## What NOT To Build (Now)

Things OpenClaw does that Citros doesn't need in the same form:

1. **Multi-agent routing** — one user, one phone, one agent. No bindings/routing rules.
2. **Cron/webhook infrastructure** — phone agent is reactive to user input, not scheduled.
3. **Channel abstraction** — one UI surface (the chat + overlay), not 15 messaging platforms.
4. **Block streaming to external channels** — no external channels to stream to.

### Deferred, Not Descoped

These were initially descoped but have since been addressed:

5. **WASM skill system** — already spec’d and partially implemented in Rust crates. Bridges to Kotlin MVP when Rust daemon ships. (See Open Question #6)
6. **Scoped agent knowledge** (replaces session key hierarchies) — ✅ design resolved (Open Question #7), implementation tracked in #650
7. **Persona files (AGENTS.md/SOUL.md)** — ✅ SHIPPED (PR #598, Agent Bones)
8. **Local LLM routing** — deferred until local models demonstrate sufficient security against prompt injection (v2 §12.3). No local model in the action loop until dedicated security evaluation.
---

## Execution Timeline

| Horizon | Scope | Status | Key Metric |
|---------|-------|--------|------------|
| **H0: MVP** | 3 PRs (prompt, stuck, overlay) | ✅ COMPLETE | Calendar + Gmail tasks work < 10 steps |
| **H1: Loop** | AgentExecutor, boundaries, queuing, trimming | ✅ COMPLETE | User can redirect mid-task; context stays clean |
| **H2: Intelligence** | Memory, lifecycle, tool groups, prompts, verification, policy, privacy, sensors, budgets | ~60% COMPLETE (7/12) | Agent remembers; security gates on sensitive actions |
| **H3: Ecosystem** | Subtasks, metacognition, failover, proactive, shared intelligence, Rust daemon | 🔲 NEXT (TinyFish started) | Self-improving agent, community knowledge, multi-provider resilience |
---

## Open Questions

1. **AgentExecutor threading model** — should it be a coroutine with structured concurrency, or a simple sequential executor? Coroutines give us cancellation for free but add complexity.

2. **Context token estimation** — how do we estimate token count on-device without a tokenizer? Options: char count / 4 (rough), tiktoken-lite, or just count messages and cap at N.

3. **Steer vs cancel UX** — RESOLVED. Build all three — the UI primitives already exist:
   - **Send button** → steer (inject "The user says: {message}" into current loop at next tool boundary)
   - **Queue button** → hold message, deliver as followup turn after loop ends (fix #445)
   - **Stop button** → cancel loop, queued messages drain as next turn (already works)
   User picks the right action in the moment. No heuristic needed.

4. **Tool result pruning** — RESOLVED. Same as OpenClaw: full conversation persisted to storage, pruned view sent to the model. Pruning is transient per API request — never rewrites history. Full tool results always recoverable from the persistent layer. This also means the UI can show complete conversation history even when the model only sees a trimmed context.

5. **Memory scope & cloud sync** — RESOLVED. Full-scope sync for backup and device migration (scoped knowledge, conversation history, persona files, guardrails, settings, learned patterns). Multi-device sharing deferred — not a near-term priority. Local SQLite is the always-available layer; cloud is an optional sync target. Backend-swappable storage interface from day 1.
   - **BYO tier**: bring your own cloud DB (Supabase, Turso, etc.). User configures connection. Free.
   - **Base tier**: can add managed cloud storage for an additional fee.
   - **Super tier**: managed cloud storage included.
   This aligns with `ct-sync` and `ct-storage` from SPEC.md. The Kotlin MVP should define a `StorageBackend` interface that SQLite implements locally, with cloud sync adapters added later.

6. **WASM skill/plugin system** — RESOLVED. Already fully spec'd in `docs/SPEC.md` §3.5.4 and Decision #5/#6. WASM binaries with capability manifests (network domain allowlists, storage caps, phone action grants, sensor access). Ed25519 signed, verified on load. wasmtime host-level capability enforcement. `ct-skills` crate already has working implementation (wasmtime runtime, capability enforcement, module compilation/caching, loader/registry/installer with signature verification). Distribution: private skill hub first (Decision #6), vetted public registry when community grows. The Kotlin MVP doesn't have this yet — it's in the Rust crate stack. Bridge plan: expose `ct-skills` via the Unix socket IPC to the Kotlin app, or port the capability manifest + WASM runtime to Android via JNI when the Rust daemon ships.

7. **Scoped agent knowledge (replaces session key hierarchies)** — RESOLVED. Instead of user-managed named sessions, Citros builds an **automatic knowledge base indexed by scope**. Three scope types:
   - `app:<package_name>` — learned navigation patterns, UI quirks, user preferences per Android app. Built automatically from tool interactions (ScreenContent.packageName tags every interaction). Gets injected into system prompt when agent is about to interact with that app.
   - `api:<provider>` — API-specific knowledge (rate limits, model quirks, pricing notes). Built from API interaction history.
   - `mcp:<server>` — MCP server capabilities, tool behavior, user workflow preferences. Built from MCP tool invocations.
   
   Knowledge accumulates implicitly (agent learns from successful interactions) and is managed explicitly through conversation ("In Gmail, always use my work account" / "Forget what you know about Chrome" / "What do you know about my apps?"). User never sees the storage layer — they just talk to the agent. This is the foundation for Horizon 3 "learned navigation patterns" and naturally solves the session hierarchy question with a phone-native approach.

8. **Persona files (AGENTS.md/SOUL.md equivalent)** — RESOLVED. Copy OpenClaw's model directly: markdown files in app-internal storage (SOUL.md, USER.md, optionally AGENTS.md). Injected into system prompt at conversation start. Editable by user as free text. Onboarding seeds USER.md with name + conversation style. Three concerns are strictly separated:
   - **Persona** (SOUL.md/USER.md) = who the agent is, who the user is, tone, personality
   - **Flavors** = visual theme ONLY (Lime=green, Tangerine=orange). No agent behavior impact.
   - **Guardrails** = constraint rules ("ask before sending emails"). Already have infrastructure. Injected as a separate "Constraints" section in system prompt. Does NOT touch persona files.

10. **v2 Phase 4 refinements** — Dangerous action confirmation gates (covered by H2.7), screen content sanitization (injection pattern stripping), parallel subtask execution, app-specific playbooks (covered by H3.3), user-in-the-loop for low-confidence actions, error recovery intelligence. These are refinements that depend on H3 foundations.

9. **Cloud-first vs local-first storage architecture** — RESOLVED. Local-first, cloud-optional. SQLite on-device is always available. Cloud sync is additive (BYO or managed). Full scope — one `StorageBackend` interface covers everything (memory, conversations, persona, guardrails, settings, patterns). Design once, implement SQLite first, add cloud adapters when tiers ship.

---

*Last updated: 2026-02-21 (v2 spec integration + gap closure)*
*Sources: OpenClaw docs (17 files), Citros codebase, agentic-loop-v2.md, SPEC.md, real-world Pixel testing*
*Supersedes: `docs/specs/openclaw-architecture-lessons.md` and `docs/specs/mvp-sprint-spec.md`*
