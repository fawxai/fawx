# Agentic Loop v2 — Architecture Specification

> **Status:** Draft
> **Authors:** Joe, Clawdio
> **Date:** 2026-02-13
> **Supersedes:** `agentic-loop-audit.md` (Phase 1 implemented; this spec covers the full vision)

---

## 1. Vision

Citros is an autonomous phone agent. The agentic loop is its core — the system
that turns a user's intent into a sequence of actions on the device.

v1 works but has structural limitations: synthetic user messages pollute context,
the model doesn't know when it's done, cheap action models are vulnerable to
prompt injection, and there's no path to self-improvement.

v2 is a ground-up redesign guided by five principles:

1. **The model drives the loop.** The LLM decides what to do, when to decompose,
   and when to stop. We provide tools and structure; it provides intelligence.
2. **Security is non-negotiable.** The action loop processes untrusted screen
   content. No model weaker than Sonnet-tier touches it. Ever.
3. **Observation is implicit.** The agent sees the screen as a consequence of
   acting, not as a separate step. Explicit reads exist for edge cases.
4. **The agent improves itself.** Every task is a learning opportunity. The agent
   reflects, identifies patterns, and evolves its own strategies.
5. **Elegance over complexity.** The right abstraction eliminates categories of
   bugs. When the architecture is clean, prompt engineering becomes refinement,
   not a crutch.

---

## 2. Architecture Overview

```
┌─────────────────────────────────────────────────────┐
│                    USER MESSAGE                      │
└────────────────────────┬────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────┐
│              METACOGNITIVE LAYER                     │
│  ┌─────────────┐ ┌──────────────┐ ┌──────────────┐ │
│  │ Self-Eval   │ │ Pattern DB   │ │ Strategy     │ │
│  │ (per task)  │ │ (cross-task) │ │ Evolution    │ │
│  └─────────────┘ └──────────────┘ └──────────────┘ │
└────────────────────────┬────────────────────────────┘
                         │ informs
                         ▼
┌─────────────────────────────────────────────────────┐
│              ORCHESTRATION LAYER                     │
│                                                     │
│  User intent → Plan → Execute (iterative loop)      │
│                  │                                   │
│                  ├─ Simple action → tool call         │
│                  └─ Complex sub-goal → subtask tool   │
│                       │                              │
│                       ▼                              │
│              ┌─────────────────┐                     │
│              │  SUBTASK LOOP   │ (recursive)         │
│              │  (isolated ctx) │                     │
│              └─────────────────┘                     │
└────────────────────────┬────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────┐
│              EXECUTION LAYER                         │
│  Tools: tap, type, swipe, open_app, ...             │
│  Screen: ScreenReader (accessibility tree)           │
│  Result: action outcome + implicit screen state      │
└─────────────────────────────────────────────────────┘
```

---

## 3. Core Loop — Clean Iterative Design

### 3.1 Current Problems (v1)

1. **Synthetic user messages.** Each loop iteration injects
   `"[Step X/20 — executed N tool(s)]"` as a user message with screen content
   prepended. The model sees fake "user" turns it never asked for, polluting
   context and confusing turn-taking.
2. **Screen content in wrong place.** Screen state is prepended to user messages
   instead of attached to tool results. The model conflates observation with
   user intent.
3. **No natural stop signal.** The loop checks `toolCalls.isNotEmpty()` but the
   model has no clear architectural signal for "I'm done." It keeps going
   because the conversation keeps growing with fake user messages.
4. **Tool hallucination.** When phone control is unavailable, the model still
   sees a system prompt describing phone agent capabilities. It hallucinates
   XML tool calls in plain text.

### 3.2 v2 Loop Design

The loop follows the standard tool-use protocol. No synthetic messages.

```
1. User sends message
2. Build conversation: [history] + [user message with optional screen context]
3. Call chatWithTools() → model returns response
4. If response has tool calls:
   a. Execute each tool call
   b. For UI-mutating tools: append current screen state to tool result
   c. Add tool results to conversation (as tool_result role)
   d. Call chatWithTools() again — same conversation, NO new user message
   e. Repeat from (4)
5. If response is text only (end_turn):
   a. Display text to user
   b. Trigger post-task reflection (async, non-blocking)
   c. Done
```

**Key difference from v1:** Step 4d does NOT inject a new user message. The
conversation naturally flows: user → assistant (tool_use) → tool_result →
assistant (tool_use or end_turn). The model sees its own actions and their
consequences, and decides when to stop.

### 3.3 New API Surface

```kotlin
/**
 * Continue the conversation after tool results have been added.
 * Does NOT add a new user message — the model sees tool results
 * and decides what to do next.
 *
 * @param screenContext Optional fresh screen state to include as context
 * @return ChatResponse with text and/or tool calls
 */
suspend fun continueAfterTools(screenContext: ScreenContent? = null): ChatResponse
```

The `sendMessage()` method handles the initial user turn. `continueAfterTools()`
handles every subsequent iteration. The ChatViewModel loop becomes:

```kotlin
var response = agent.sendMessage(userMessage, screenContent)

while (response.toolCalls.isNotEmpty() && steps < MAX_STEPS) {
    for (toolCall in response.toolCalls) {
        val result = agent.executeToolCall(toolCall, screenContent)

        // Refresh screen after UI-mutating actions
        if (toolCall.name in UI_MUTATING_TOOLS) {
            delay(settleTime(toolCall.name))
            screenContent = ScreenReader.getScreenContent()
            agent.addToolResult(toolCall.id, "$result\n\nSCREEN:\n${screenContent.toPromptText()}")
        } else {
            agent.addToolResult(toolCall.id, result)
        }
    }

    response = agent.continueAfterTools(screenContent)
    steps++
}

// Model returned text — done
display(response.text)
reflection.evaluateAsync(task, steps, response)
```

### 3.4 UI-Mutating vs Non-Mutating Tools

| UI-Mutating (append screen) | Non-Mutating (result only) |
|----|---|
| `tap`, `tap_text`, `long_press` | `think` |
| `type_text` | `remember`, `recall`, `list_memories` |
| `swipe`, `scroll` | `read_file`, `write_file`, `list_files` |
| `press_back`, `press_home` | `copy`, `set_clipboard` |
| `open_app`, `open_notifications` | `screenshot` (returns description) |
| `wait` (reads screen internally) | `paste` (UI-mutating but text result suffices) |

`read_screen` remains available as an explicit tool for cases where the model
needs observation without action (loading screens, async UI changes, ambiguous
state). It is not removed — the model can still request it when needed.

### 3.5 Context Compaction

Screen dumps consume significant tokens and go stale immediately (element IDs
change per action). Compaction strategy:

- **Last 2 tool results:** Full screen state preserved
- **Older tool results:** Compressed to action summary only
  - Before: `"Tapped element 5\n\nSCREEN:\n[1] 'Settings'...50 lines..."`
  - After: `"Tapped element 5 → opened Settings"`
- **System prompt:** Always preserved in full
- **User's original message:** Always preserved

Compaction runs before each `chatWithTools()` call when total message tokens
exceed a configurable threshold (e.g., 60% of model context window).

---

## 4. Security — Model Floor Policy

### 4.1 Threat Model

The action loop reads **untrusted screen content** from arbitrary applications.
A malicious app, website, or notification can inject adversarial text designed
to hijack the agent's behavior. The agent has capabilities to:

- Send messages and emails
- Make purchases
- Access financial apps
- Tap any UI element on screen
- Read and share sensitive information

This is among the highest-risk AI deployment scenarios. The model processing
screen content must be robust against prompt injection.

### 4.2 Policy

**No model weaker than Sonnet-tier may be used in the action loop.**

This applies across all providers:

| Provider | Chat Model (planning) | Action Model (execution) | Minimum |
|---|---|---|---|
| Anthropic | Opus or Sonnet | Sonnet | Sonnet |
| OpenAI | GPT-5.2 or GPT-4o | GPT-4o | GPT-4o |
| OpenRouter | Equivalent tier | Equivalent tier | Sonnet-equivalent |

- Haiku, GPT-4o-mini, and other small models are **prohibited** from the action
  loop regardless of user configuration.
- The UI should allow Opus+Sonnet or Sonnet+Sonnet selection. The "action model"
  selector must not offer models below the floor.
- This is enforced in code, not just UI — `PhoneAgentApi` validates the action
  model tier at construction time.

### 4.3 Rationale

- Smaller models are empirically weaker at resisting prompt injection
- The cost savings (~10x cheaper per token) are negligible compared to the
  potential damage from a successful injection
- The speed difference (~500ms per call) is invisible to the user because UI
  settle delays (1-2s) dominate total step time

### 4.4 Defense in Depth

Model floor is the primary defense. Additional layers:

- **Screen content sanitization:** Strip known injection patterns before sending
  to model (future enhancement, not Phase 1)
- **Dangerous action gates:** User confirmation required before actions flagged
  as high-risk (send money, delete data, share credentials)
- **Action auditing:** All tool calls and results logged for review

---

## 5. Observation — Implicit by Default

### 5.1 Design

The model should not need to explicitly "look at the screen" during an action
loop. After every UI-mutating tool call, the fresh screen state is automatically
appended to the tool result. The model receives observation as a consequence of
acting, not as a separate step.

This eliminates an entire category of wasted steps (redundant `read_screen`
calls) by architecture, not prompting.

### 5.2 Tool Result Format

```
ACTION: Opened Gmail
STATUS: success

SCREEN:
[1] "Primary" (tab) [click]
[2] "Meeting tomorrow - Sarah Chen" (unread, 2m ago) [click]
[3] "Your order has shipped - Amazon" (3h ago) [click]
[4] "Compose" (floating action button) [click]
```

The format is:
- **ACTION:** What happened (human-readable summary)
- **STATUS:** `success` or `failed: <reason>`
- **SCREEN:** Fresh accessibility tree (only for UI-mutating actions)

### 5.3 When Explicit Observation Is Needed

`read_screen` remains in the tool set for legitimate cases:

- **Initial turn:** The model may want to see the screen before deciding on
  the first action (though screen context is also provided in the initial
  user message for common cases)
- **Async UI changes:** After a `wait`, the screen may have changed
  unpredictably (loading completed, notification appeared)
- **Ambiguous state:** The tool result said "success" but the model suspects
  the UI didn't update as expected
- **Non-action observation:** The user asks "what's on my screen?" — no action
  needed, just observation

The model decides when explicit observation is valuable. Over time, the
metacognitive layer learns which apps/contexts benefit from explicit reads and
adjusts accordingly.

---

## 6. Subtask Decomposition

### 6.1 Design

A `subtask` tool enables the model to decompose complex goals into isolated
sub-loops. The orchestrating model defines **what** needs to happen; the
subtask handles **how**.

```kotlin
val SUBTASK = Tool(
    name = "subtask",
    description = """Decompose a complex goal into a focused sub-task.
        Use when a task has distinct phases that benefit from isolated context
        (e.g., "find info" then "compose message with that info").
        The sub-task runs in its own context and returns a structured result.
        For simple linear tasks, just use regular tools directly.""",
    inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "goal" to mapOf(
                "type" to "string",
                "description" to "Clear description of what the sub-task should accomplish"
            ),
            "success_criteria" to mapOf(
                "type" to "string",
                "description" to "How to determine if the sub-task succeeded. What should the result contain?"
            ),
            "max_steps" to mapOf(
                "type" to "integer",
                "description" to "Maximum tool steps for this sub-task (default: 10)",
                "default" to 10
            )
        ),
        "required" to listOf("goal", "success_criteria")
    )
)
```

### 6.2 Execution

When the orchestrator calls `subtask`:

1. A new `PhoneAgentApi` instance is created with:
   - Fresh, empty conversation history (context isolation)
   - The sub-task goal as the user message
   - Success criteria included in the system prompt
   - Same model configuration as the parent (inherits model floor)
   - Shared `ScreenReader` reference (same physical screen)
   - Its own step counter, capped at `max_steps`
2. The sub-loop runs the standard iterative loop (Section 3)
3. On completion, returns a structured result to the parent:

```json
{
    "status": "success|failed|partial",
    "result": "ANA flight $450, departing Feb 20",
    "steps_used": 4,
    "summary": "Opened Google Flights, searched Tokyo, found cheapest option"
}
```

4. The orchestrator receives this as a tool result and decides what to do next:
   retry, reformulate, proceed to next subtask, or report to user.

### 6.3 Depth and Resource Limits

- **Maximum recursion depth:** 3 levels (orchestrator → subtask → sub-subtask)
- **Step budget:** Each level has its own `max_steps`. The parent's step counter
  increments by 1 per subtask call (not by the subtask's internal steps).
- **Cancellation propagation:** The parent's cancellation token is shared with
  all child loops. User cancellation kills everything.
- **Timeout:** Each subtask has a wall-clock timeout (default: 60 seconds) in
  addition to the step limit. Whichever fires first wins.

### 6.4 The Model Decides

The orchestrator is not forced to use `subtask`. Simple tasks naturally use
regular tools directly — the model won't decompose "open settings" into a
subtask because that would be absurd. Complex tasks trigger decomposition
because the model recognizes the benefit.

This is not a heuristic we build. It's a capability we offer. The model's
planning ability is the router.

---

## 7. User-Facing Output

### 7.1 Principle

Users care about **what the agent is thinking** and **what it accomplished**.
They do not care about mechanical details of execution.

### 7.2 Output Classification

| Category | Example | Display | Audio |
|---|---|---|---|
| **Thinking** | "I need to find the compose button" | Show (dimmed/italic) | Optional |
| **Progress** | "Opening Gmail..." | Show | Announce |
| **Result** | "Email sent to Sarah" | Show (prominent) | Announce |
| **Mechanical** | "Tapped element 5" | Hide | Silent |
| **Error** | "Can't find the app" | Show (warning style) | Announce |

### 7.3 Implementation

The ChatViewModel classifies each tool result before adding to the UI:

```kotlin
enum class OutputVisibility { SHOW, SHOW_DIMMED, HIDE }

fun classifyOutput(toolCall: ToolCall, result: String): OutputVisibility {
    return when {
        toolCall.name == "think" -> OutputVisibility.SHOW_DIMMED
        toolCall.name == "subtask" -> OutputVisibility.SHOW
        result.startsWith("Failed:") -> OutputVisibility.SHOW
        toolCall.name in setOf("tap", "tap_text", "long_press",
            "swipe", "scroll", "press_back", "type_text") -> OutputVisibility.HIDE
        toolCall.name in setOf("open_app", "screenshot") -> OutputVisibility.SHOW
        else -> OutputVisibility.SHOW_DIMMED
    }
}
```

User preferences override defaults: a "verbose" mode shows everything; a
"minimal" mode shows only results and errors.

---

## 8. Metacognitive Layer — Self-Awareness and Self-Improvement

### 8.1 Vision

The agent is not just a task executor. It continuously evaluates its own
performance, identifies patterns, and evolves its strategies. Every task is a
data point. Every failure is a lesson. Every success is a pattern to reinforce.

This is what separates a tool from an agent.

### 8.2 Three Timescales of Reflection

#### In-Task (Real-Time Self-Monitoring)

During execution, the agent monitors its own behavior for signs of inefficiency
or failure loops:

- **Loop detection:** "I've attempted the same action 3 times — I'm stuck."
- **Efficiency awareness:** "I've used 8 steps for what should be a 3-step task."
- **Strategy pivoting:** "Scrolling isn't finding the element. Let me try search."

This is implemented through the `think` tool and through the system prompt's
framing. The model is instructed to periodically assess not just "what should I
do next?" but "am I being effective?"

**Prompt framing:**
```
SELF-MONITORING:
During execution, periodically assess your efficiency:
- Am I making progress toward the goal, or repeating steps?
- Is there a faster path than what I'm currently doing?
- If I've attempted the same approach twice without success, try something different.
```

#### Post-Task (Structured Reflection)

After every task completes or fails, the agent performs a brief self-evaluation.
This runs asynchronously and does not block the user response.

**Reflection structure:**
```json
{
    "task": "Send email to Sarah about the meeting",
    "outcome": "success",
    "steps_used": 7,
    "efficiency_rating": "moderate",
    "observations": [
        "Gmail loaded slowly — waited 3 seconds unnecessarily on first try",
        "Could have used compose FAB directly instead of navigating to inbox first"
    ],
    "learned": [
        {"app": "Gmail", "pattern": "compose_button_always_visible", "confidence": 0.8},
        {"app": "Gmail", "pattern": "needs_load_wait_after_open", "confidence": 0.9}
    ]
}
```

This reflection is generated by the model itself (using the `think` tool or a
dedicated reflection prompt after task completion) and stored in the memory
system.

#### Cross-Task (Pattern Evolution)

Over many tasks, the agent aggregates reflections into higher-order patterns:

- **App-specific knowledge:** "Gmail takes 2s to load. DoorDash checkout is
  reliably 8 steps. Settings is organized alphabetically."
- **Strategy preferences:** "Search is faster than scrolling for finding items
  in long lists. The compose button is always visible in email apps."
- **Efficiency baselines:** "Simple open-app tasks: 1-2 steps. Compose-and-send:
  5-8 steps. Multi-app workflows: 10-15 steps."
- **Failure patterns:** "Calendar widget sometimes doesn't respond to taps —
  open the full app instead."

These patterns inform future behavior:
- The orchestrator sets better `max_steps` budgets for subtasks because it knows
  how long similar tasks typically take.
- Observation becomes adaptive — apps known to have loading delays get explicit
  `wait` or `read_screen` calls; responsive apps don't.
- The system prompt can include a "known patterns" section populated from
  memory, giving the model a head start on familiar tasks.

### 8.3 Self-Improvement Loop

The agent doesn't just observe patterns — it reasons about opportunities to
improve its own logic:

```
SELF-IMPROVEMENT:
After completing a task, briefly consider:
- What would I do differently if I did this task again?
- Is there a pattern here that applies to other tasks?
- Are any of my current strategies consistently inefficient?

Store insights using the remember tool with tag "self-improvement".
When starting a task, recall relevant self-improvement notes.
```

**Concrete examples of self-improvement:**

1. **Prompt refinement:** "I keep reading the screen after open_app even though
   the tool result already includes it. I should trust implicit observation for
   open_app." → Stored as a behavioral adjustment.
2. **Strategy evolution:** "For 'send a message' tasks, I used to: open app →
   read screen → find compose → tap → type → find send → tap. Now I know: open
   app → tap compose (always element near bottom-right) → type → tap send. 
   Saves 2 steps consistently." → Stored as an optimized playbook.
3. **Error prevention:** "Last time I tried to send money via Venmo, I tapped
   the wrong amount field. I should use the think tool to double-check amounts
   before confirming financial transactions." → Stored as a safety rule.

### 8.4 Storage and Retrieval

Reflections and patterns are stored through the existing memory system:

- **Tag taxonomy:**
  - `self-reflection` — post-task evaluations
  - `self-improvement` — behavioral adjustment insights
  - `app-pattern:<app_name>` — app-specific knowledge
  - `strategy:<category>` — task strategy patterns
  - `failure-analysis` — root cause analysis of failures

- **Retrieval:** Before starting a task, the agent recalls:
  - Patterns for the target app (if identifiable from the user message)
  - Relevant self-improvement notes
  - Failure analyses for similar past tasks

- **Pruning:** Old reflections with low confidence or that have been superseded
  by newer observations are periodically archived or deleted during maintenance.

### 8.5 Guardrails on Self-Modification

The agent's self-improvement operates within boundaries:

- **Prompt changes:** The agent can suggest prompt modifications but cannot
  unilaterally rewrite its own system prompt. Suggestions are stored as
  proposals that can be reviewed and applied.
- **Strategy changes:** The agent freely adjusts its own approach to tasks
  (this is just using memory to inform decisions — no code changes).
- **Safety rules:** Self-improvement cannot relax safety constraints. The model
  floor, confirmation gates, and action auditing are immutable regardless of
  what the agent "learns."
- **Transparency:** All self-improvement insights are stored in the memory
  system and are visible/auditable by the user.

---

## 9. Tool Gating — Phone Control Availability

### 9.1 Problem (Current)

When accessibility service is not attached, `PhoneAgentApi.sendMessage()` still
passes `PhoneTools.ALL` to the model. The system prompt still describes phone
agent capabilities. The model hallucinates tool calls as XML in plain text.

### 9.2 Solution

Gate tool availability on `ScreenReader.isAttached()`:

- **Phone control available:** Full tool set, standard system prompt.
- **Phone control unavailable:** No tools passed to model. System prompt
  switches to conversational mode. The model is explicitly told it cannot
  control the phone and should suggest enabling the accessibility service.
- **Safety net:** Chat-mode responses are filtered for tool-like artifacts
  (XML tags, JSON tool blocks) as a defense-in-depth measure. If the model
  hallucinates tool syntax despite not having tools, it's stripped before
  display.

### 9.3 Message Classification

The `isLikelyConversationalMessage` classifier determines whether a message
should bypass the tool loop entirely (even when tools are available):

**Fix (v1 bug):** Action hints must be checked **before** the question-mark
heuristic. "What's on my calendar?" contains action-context words and should
route to tools, not chat mode.

**Priority order:**
1. Known conversational phrases → chat mode
2. Contains action hint → tool mode (even if ends with `?`)
3. Ends with `?` and no action hints → chat mode
4. Short message (≤3 words, no special chars) → chat mode
5. Default → tool mode

**Extended action hints:** Include context words that imply phone interaction
(calendar, email, notification, alarm, weather, message, photo, camera, wifi,
bluetooth, brightness) in addition to action verbs.

---

## 10. Implementation Phases

### Phase 1 — Clean Loop Foundation
**Goal:** Fix the structural issues. Remove synthetic messages. Get the basic
loop right.

**Scope:**
- [ ] `PhoneAgentApi.continueAfterTools()` — new method, no user message injection
- [ ] `ChatViewModel.sendMessage()` — refactored loop using `continueAfterTools()`
- [ ] Screen content in tool results for UI-mutating actions
- [ ] Tool gating on accessibility state (#390)
- [ ] `isLikelyConversationalMessage` fix (action hints before `?`)
- [ ] Strip tool artifacts from chat-mode responses
- [ ] Model floor enforcement — reject Haiku/mini in action loop
- [ ] Context compaction — trim old screen dumps
- [ ] Output classification — hide mechanical actions, show thinking
- [ ] Updated system prompt with TASK COMPLETION and SELF-MONITORING sections
- [ ] Updated action prompt with implicit observation guidance
- [ ] Tests for all new behavior

**Expected impact:** 70% → 85% task completion rate

### Phase 2 — Subtask Decomposition
**Goal:** Enable the model to break complex tasks into focused sub-loops.

**Scope:**
- [ ] `subtask` tool definition and execution
- [ ] Isolated PhoneAgentApi instances for sub-loops
- [ ] Structured result format (status, result, steps_used, summary)
- [ ] Depth limiting (max 3 levels)
- [ ] Cancellation propagation
- [ ] Wall-clock timeout per subtask
- [ ] Tests: simple task doesn't trigger subtask, complex task decomposes

**Expected impact:** 85% → 93% task completion rate

### Phase 3 — Metacognitive Layer
**Goal:** The agent reflects, learns, and improves over time.

**Scope:**
- [ ] Post-task reflection (async, after each task)
- [ ] Reflection storage with tag taxonomy
- [ ] Pre-task pattern retrieval (recall relevant memories before starting)
- [ ] In-task self-monitoring prompt framing
- [ ] Cross-task pattern aggregation (periodic maintenance)
- [ ] Self-improvement proposals (stored, auditable)
- [ ] Adaptive orchestrator budgeting (max_steps based on learned patterns)
- [ ] Adaptive observation (explicit reads for apps that need them)

**Expected impact:** 93% → 97% task completion rate

### Phase 4 — Refinement and the Long Tail
**Goal:** Close the gap to 98%+.

**Scope:**
- [ ] Screen content quality improvements (better parsing, semantic grouping)
- [ ] Dangerous action confirmation gates
- [ ] Screen content sanitization (injection pattern stripping)
- [ ] Parallel subtask execution
- [ ] App-specific playbooks (learned, not hardcoded)
- [ ] User-in-the-loop for low-confidence actions
- [ ] Error recovery intelligence (model learns what recovery strategies work)

**Expected impact:** 97% → 98%+

---

## 11. File Impact (Phase 1)

| File | Changes |
|---|---|
| `PhoneAgentApi.kt` | New `continueAfterTools()`, tool gating, `isLikelyConversationalMessage` fix, `stripToolArtifacts()`, model floor validation |
| `PhoneAgentPrompts.kt` | Updated SYSTEM_PROMPT (task completion, self-monitoring, implicit observation), updated ACTION_PROMPT |
| `ChatViewModel.kt` | Refactored loop (no synthetic messages, `continueAfterTools()`), output classification, async reflection hook |
| `PhoneTools.kt` | Tool set variants (full vs gated) |
| `ContextManager.kt` | Aggressive screen dump compaction |
| `ModelConfig.kt` | Model floor enforcement per provider |
| `PhoneAgentApiTest.kt` | Tests for all new behavior |
| `ChatViewModelTest.kt` | Loop refactor tests |

---

## 12. Integration with SPEC.md Architecture

The agentic loop does not exist in isolation. It must integrate cleanly with
the broader Citros architecture defined in `SPEC.md`. This section addresses
the touchpoints.

### 12.1 Action Policy Engine (SPEC §3.5.3)

The loop must consult the policy engine **before executing each tool call**.
The policy engine is the hard security boundary between what the model wants
and what actually happens.

```kotlin
for (toolCall in response.toolCalls) {
    val policyResult = policyEngine.evaluate(toolCall)
    when (policyResult) {
        ALLOW -> execute(toolCall)
        CONFIRM -> {
            val approved = requestUserConfirmation(toolCall)
            if (approved) execute(toolCall)
            else agent.addToolResult(toolCall.id, "User denied action")
        }
        DENY -> agent.addToolResult(toolCall.id, "Action blocked by policy")
        RATE_LIMITED -> {
            pause()
            agent.addToolResult(toolCall.id, "Rate limited — too many actions")
        }
    }
}
```

**Tool-to-policy mapping:**

| Tool | Default Policy | Notes |
|---|---|---|
| `tap`, `tap_text`, `swipe`, `scroll` | ALLOW | Standard UI navigation |
| `type_text` | ALLOW | Text entry (content reviewed by model) |
| `open_app` | ALLOW | First-time app: CONFIRM |
| `press_back`, `press_home` | ALLOW | Navigation |
| `read_screen`, `screenshot` | ALLOW | Observation |
| `think`, `wait` | ALLOW | Internal reasoning |
| Messages/email (via tap) | CONFIRM | Detected by screen context analysis |
| Financial actions (via tap) | DENY (v1) | Unlockable with biometric gate in v2 |
| Install/uninstall (via tap) | CONFIRM | |
| `subtask` | ALLOW | Subtask inherits parent policy context |

Phase 1 implements a lightweight policy check. The full policy engine (signed
config, capability grants) comes with the Rust daemon integration.

### 12.2 Voice I/O and Input Modality

The loop is input-modality-agnostic. Voice and text both produce a user message
string that enters the same orchestration path:

```
Voice: wake word → STT → text → loop
Text:  keyboard input → text → loop
```

Output routing is handled by the output classifier (Section 7). When voice
mode is active, results classified as `SHOW` are also sent to TTS. Mechanical
actions (`HIDE`) are never spoken. This prevents the agent from narrating
"tapped element 5" through the speaker.

### 12.3 Local LLM Routing — Future Consideration

SPEC §3.5.7 envisions a three-tier LLM architecture including local models
for intent classification and offline use. **Local models are not considered
safe at this time** — they are significantly more vulnerable to prompt
injection and lack the reasoning depth needed for reliable tool-use decisions.

For the v2 loop:

- **All inference is cloud-only.** The Sonnet-floor policy (Section 4) applies
  to every API call in the loop — planning, action, and reflection.
- **Intent classification** uses the heuristic-based
  `isLikelyConversationalMessage` classifier, not a local model.
- **Offline mode**: When no cloud provider is available, the agent tells the
  user it cannot perform tasks until connectivity is restored. No local
  fallback for phone control.
- **Future**: Local models may be reconsidered when they demonstrate sufficient
  robustness against prompt injection and reliable instruction-following in
  tool-use scenarios. This would require dedicated security evaluation before
  any local model is permitted in the loop.

### 12.4 Proactive Agent Behavior

SPEC Phase 4 describes the agent initiating its own tasks (morning briefings,
calendar reminders, notification summaries). The agentic loop must support
**agent-initiated entries**, not just user-initiated ones:

```kotlin
sealed class LoopTrigger {
    data class UserMessage(val text: String) : LoopTrigger()
    data class Notification(val content: NotificationContent) : LoopTrigger()
    data class Schedule(val trigger: ScheduledTrigger) : LoopTrigger()
    data class ContextChange(val event: ContextEvent) : LoopTrigger()
}
```

Proactive loops run the same orchestration path but with different entry
context. The model receives "A calendar event is in 30 minutes" instead of a
user message, and decides whether to alert the user, take preparatory action,
or do nothing.

Proactive behaviors are **off by default** and individually opt-in per SPEC
§3.4 Phase 4 guidance.

### 12.5 Sensor Context

The agent should be aware of device state when making decisions:

- **Battery level**: Don't start a 20-step task at 5%. Warn the user if
  battery is critically low.
- **Connectivity**: Warn if cloud-dependent task requested while offline.
  (Future: local model fallback may be considered when safety is proven.)
- **Location**: Contextualizes requests ("nearby restaurants" needs GPS).
- **Time of day**: Affects proactive behavior suppression (quiet hours).

Sensor context is injected as a lightweight prefix in the system prompt, not
in every tool result:

```
DEVICE STATE: Battery 72%, WiFi connected, Location: Denver CO, 4:15 PM MST
```

### 12.6 Privacy-Sensitive App Handling

SPEC §6.1 defines "selective screen blindness" for apps on a privacy list
(banking, health, etc.). The observation layer must respect this:

- When a privacy-listed app is in the foreground, screen content is **not**
  appended to tool results.
- The agent receives: `"SCREEN: [Privacy mode — screen content hidden for
  this app. Ask the user for guidance if needed.]"`
- The agent can still execute blind actions (press_back, press_home) but
  cannot observe results.
- The user configures the privacy list in Settings.

### 12.7 Web Search Tool (#345)

The agent needs to gather information mid-task without touching the phone UI.
A `web_search` tool enables this:

```kotlin
val WEB_SEARCH = Tool(
    name = "web_search",
    description = "Search the web for information. Use when you need facts, " +
        "prices, hours, or other information to complete a task.",
    inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "query" to mapOf(
                "type" to "string",
                "description" to "Search query"
            )
        ),
        "required" to listOf("query")
    )
)
```

This is a non-UI tool (doesn't need screen access) and can work even when
phone control is unavailable, extending the agent's usefulness in chat-only
mode.

### 12.8 Cost Tracking and Budgets

API calls cost money. The loop must be cost-aware:

- **Per-task token tracking**: Count input/output tokens across all API calls
  in a task (including subtasks and reflections).
- **Budget limits**: User-configurable daily/monthly spending cap. The loop
  refuses to start new tasks when the budget is exhausted.
- **Reflection cost control**: Post-task reflection is skipped when remaining
  budget is below a threshold. Self-improvement is a luxury, not a necessity.
- **Subtask cost inheritance**: Subtask token usage counts toward the parent
  task's total. The orchestrator can see cumulative cost when deciding whether
  to spawn additional subtasks.
- **Transparent reporting**: The user can see cost-per-task in the chat UI
  (opt-in, not default).

### 12.9 User Interruption Detection

The agent must detect when the user takes control mid-task:

- **Screen change detection**: If the foreground app changes without an agent
  action, the user switched apps. Pause the loop.
- **User touch detection**: The accessibility service can distinguish agent-
  injected events from user touch events. Any user touch during execution
  triggers a pause.
- **Interruption protocol**: On detection, the agent pauses and asks:
  "I was working on [task]. Want me to continue or cancel?"
- **Graceful state**: The conversation history and step progress are preserved
  so the agent can resume if the user says "continue."

### 12.10 Rust Daemon Migration Path

The v2 loop is implemented in Kotlin for Horizon 1. When the Rust daemon
(ct-agent) takes over orchestration in Horizon 2:

- `PhoneAgentApi.continueAfterTools()` maps to `orchestrator::continue_loop()`
- The tool execution layer routes through Unix socket IPC to the Kotlin
  companion app (for accessibility) or directly to `/dev/input` (for root)
- The metacognitive layer translates directly — reflection prompts and memory
  storage are LLM-agnostic
- The policy engine in Rust (ct-security) replaces the lightweight Kotlin check

Design the Kotlin interfaces so the Rust equivalents are obvious. Same method
signatures, same data flow, same contracts.

### 12.11 Related Issues

The following open issues map to this spec and should be updated with
references:

| Issue | Spec Section |
|---|---|
| #342 — Selective screen region reading | §5 Observation |
| #345 — Web search tool | §12.7 |
| #348 — Cross-session task memory | §8 Metacognitive Layer |
| #349 — App navigation maps | §8.2 Cross-Task Patterns |
| #350 — Agentic telemetry | §14 Success Metrics |
| #351 — Think tool visibility toggle | §7 User-Facing Output |
| #390 — Tool hallucination | §9 Tool Gating |

---

## 13. Open Questions (Deferred)

1. **Reflection model:** Should post-task reflection use the chat model or a
   dedicated call? Using the chat model adds cost per task. A cheaper model
   could reflect, but see Section 4 (security) — reflection doesn't process
   untrusted content, so a lighter model may be acceptable here.

2. **Pattern storage format:** Should learned patterns be structured JSON
   (queryable) or natural language (flexible)? Structured is better for
   programmatic use (auto-setting max_steps); natural language is better for
   prompt injection into system messages.

3. **Subtask parallelism:** Android has one screen. Parallel subtasks would
   need to share the screen sequentially (one pauses while the other acts) or
   be limited to non-UI tasks. Is this worth the complexity?

4. **Self-improvement review flow:** Should the agent autonomously apply its
   own behavioral adjustments, or should significant changes require user
   approval? The spec currently says: strategies are free, prompt changes are
   proposals. Is that the right line?

5. **Max steps default:** v1 uses 20. With implicit observation removing
   wasted `read_screen` calls, effective capacity increases. Should we lower
   the default to 15 to encourage efficiency, or keep 20 for complex tasks?

---

## 14. Success Metrics

| Metric | v1 Baseline | Phase 1 Target | Phase 4 Target |
|---|---|---|---|
| Task completion rate | ~70% | 85% | 98% |
| Avg steps per simple task | 4-6 | 1-3 | 1-2 |
| Avg steps per complex task | 12-20 | 8-12 | 6-10 |
| Hallucinated tool calls | Common | Zero | Zero |
| Redundant read_screen calls | 2-3 per task | 0-1 per task | 0 per task |
| User interruptions needed | Frequent | Occasional | Rare |
| Avg cost per simple task | Unknown | < $0.02 | < $0.01 |
| Avg cost per complex task | Unknown | < $0.15 | < $0.10 |
| Policy-blocked dangerous actions | N/A | 100% | 100% |

---

*This spec is a living document. Update it as implementation reveals new
insights — that's the agent's self-improvement principle applied to our own
process.*
