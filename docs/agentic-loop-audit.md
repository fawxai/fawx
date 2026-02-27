# Agentic Loop Architecture Audit (#321)

## Current Architecture

### Overview
The current agentic loop is a **flat linear loop** in `ChatViewModel.sendMessage()`:

```
User message → chatClient (Sonnet) → [tool calls?]
  → execute tools → add results → actionClient (Haiku) → [tool calls?]
  → execute tools → add results → actionClient → ...
  → text response (task complete) OR hit MAX_TOOL_STEPS (10)
```

### Components

| Component | Role | Location |
|-----------|------|----------|
| `ChatViewModel` | Orchestrates loop, manages UI state | `:chat` |
| `PhoneAgentApi` | Sends messages, executes tool calls | `:core` |
| `PhoneTools` | 13 tool definitions (tap, type, swipe, etc.) | `:core` |
| `ScreenReader` | Accessibility service bridge, screen parsing | `:core` |
| `PhoneAgentPrompts` | System prompt | `:core` |
| `AgentPromptBuilder` | Dynamic prompt with agent files | `:core` |

### Flow Detail
1. User sends message → `ChatViewModel.sendMessage(content)`
2. Screen content captured via `ScreenReader.getScreenContent()`
3. If conversational (greeting/question), bypass tools entirely → text-only `chat()`
4. Otherwise, `chatWithTools()` with chat model (Sonnet) — includes screen content in message
5. **Tool loop** (max 10 iterations):
   - Execute all tool calls in response sequentially
   - Add `🤖` prefixed results to UI
   - Add tool results to agent conversation via `addToolResult()`
   - Wait for UI to settle (variable delay: 1500ms app launch, 800ms tap, 500ms default)
   - Re-read screen content
   - Send `[Executed N tool(s)]` to action model (Haiku) with fresh screen
   - Repeat if response has more tool calls
6. On text-only response → task complete, show to user
7. On MAX_TOOL_STEPS → "Hit step limit (10). What next?"

### Cancellation
- `cancelToolExecution()` sets `toolLoopCancelled` flag
- Checked before each iteration and before each tool call
- Reset on next `sendMessage()`

### Queued Messages
- If user sends a message while loop is running, it's stored in `queuedMessage`
- Dispatched after loop completes (if not cancelled)

---

## Strengths

1. **Dual-model is smart** — Sonnet for planning, Haiku for execution saves cost/latency
2. **Screen refresh after each action** — always has fresh UI state
3. **Variable delays** — respects that app launches take longer than taps
4. **Cancellation support** — user can interrupt a runaway loop
5. **Conversational bypass** — doesn't waste tool calls on "hi"
6. **Simple and debuggable** — easy to trace what happened

## Weaknesses & Failure Modes

### 1. No Planning or Goal Tracking
The model gets the full conversation history but has no explicit task decomposition.
For "Book a restaurant on OpenTable for 7pm tonight":
- No breakdown into sub-goals (open app → search → select → book)
- No tracking of which sub-goals are complete
- If it gets lost at step 6, it has no plan to refer back to

### 2. No Verification After Actions
After tapping a button, the model gets the new screen but isn't explicitly prompted to verify the action succeeded. It relies entirely on the model noticing if something went wrong.

### 3. No Error Recovery Strategy
If a tap fails or lands on the wrong element:
- No retry logic
- No "go back and try again" strategy
- Model might continue forward with wrong state

### 4. Context Window Bloat
Each step adds: tool call + tool result + screen content (~40 elements).
By step 8, the context has ~8 screen dumps. The action model (Haiku) has a smaller context window and may lose early context.

### 5. 10-Step Limit May Be Too Low
Complex tasks like "Order food on DoorDash" could easily require 15+ steps:
open app → search → select restaurant → browse menu → add item → customize → add to cart → checkout → enter address → confirm → pay

### 6. No Sub-task Decomposition
Every step is flat. "Send a photo to Mom on WhatsApp" requires finding the photo AND sending it, but these are treated as one continuous loop.

### 7. Screen Content Truncation
`ScreenContent.toPromptText()` caps at 40 elements sorted by interactivity score. Complex screens (Settings, long lists) may hide the target element.

### 8. No Long-Press, No Multi-Touch
Tool set doesn't include long-press (needed for copy/paste, context menus), pinch/zoom, or multi-finger gestures.

---

## Proposed Architecture: Observe-Plan-Act-Verify (OPAV)

### Design Principles
- **Don't over-engineer** — the LLM is already a good planner. Don't build a rigid planner that fights it.
- **Add structure where it helps** — verification, context management, recovery
- **Keep it model-driven** — the model decides what to do, we just give it better tools and prompts
- **Backward compatible** — existing simple tasks should work exactly as before

### Proposed Changes (Incremental)

#### Phase 1: Enhanced Prompts + Verification (Low risk, high impact)
1. **Task framing in system prompt**: Tell the model to think in terms of goals and sub-goals
2. **Explicit verification prompting**: After each action, prompt includes "Verify: did the action succeed?"
3. **Recovery instructions**: "If an action failed, try an alternative approach before giving up"
4. **Bump MAX_TOOL_STEPS to 20**: Complex tasks need more room
5. **Add `think` tool**: Let the model reason about what to do next without taking an action

#### Phase 2: Context Management (Medium risk, high impact)
1. **Screen content summarization**: After 5+ steps, summarize old screen states instead of keeping full dumps
2. **Sliding context window**: Keep last 3 full screen states, summarize older ones
3. **Goal tracking in conversation**: Inject a `[Progress: 3/6 steps, current goal: select restaurant]` marker

#### Phase 3: New Tools (Low risk, medium impact)
1. **`long_press`** — needed for context menus, copy/paste
2. **`wait`** — explicit "wait N seconds for screen to update" (loading screens, animations)
3. **`take_screenshot`** — visual verification for cases where accessibility tree is insufficient
4. **`task_complete`** — explicit signal instead of relying on text-only response

#### Phase 4: Advanced Loop (Higher risk, evaluate after Phase 1-2)
1. **Sub-task spawning** — if needed, but may not be necessary if prompts are good enough
2. **Re-planning** — after N failed attempts, re-read the full screen and re-plan from scratch
3. **User confirmation gates** — for destructive actions (sending money, deleting things)

---

## Recommendation

**Start with Phase 1.** It's the highest ROI:
- Better prompts alone will fix most failure modes
- `think` tool lets the model reason explicitly
- 20-step limit handles complex tasks
- No code architecture changes needed — just prompt engineering + constant change

Then measure: test on 10 real-world tasks, track success rate. Only move to Phase 2+ if Phase 1 doesn't get us to >80% success rate.

---

## Implementation Plan for Phase 1

### 1. Update `PhoneAgentPrompts.SYSTEM_PROMPT`
```
You are Fawx, an AI phone agent. You control the user's Android phone.

APPROACH:
1. OBSERVE: Read the current screen carefully
2. PLAN: Think about what steps are needed (use the think tool for complex tasks)
3. ACT: Execute one action at a time
4. VERIFY: After each action, check the screen to confirm it worked

If an action fails:
- Re-read the screen
- Try an alternative (different element, scroll to find it, go back and retry)
- If stuck after 3 attempts, tell the user what went wrong

SCREEN FORMAT:
[id] "text" (description) [click] [edit]

TOOLS: [existing tool docs]

IMPORTANT:
- type_text does NOT submit — tap send button separately
- Element IDs change after every action — always use fresh IDs
- One action at a time
- When done, respond with text (no tools) to signal completion
```

### 2. Add `think` tool
```kotlin
val THINK = Tool(
    name = "think",
    description = "Think about the current situation and plan next steps. Use for complex tasks. Output is not shown to the user.",
    inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "thought" to mapOf(
                "type" to "string",
                "description" to "Your reasoning about what to do next"
            )
        ),
        "required" to listOf("thought")
    )
)
```

### 3. Add `wait_for_screen` tool
```kotlin
val WAIT = Tool(
    name = "wait",
    description = "Wait for the screen to update (e.g., after launching an app or loading content). Waits 1-5 seconds then reads screen.",
    inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "seconds" to mapOf(
                "type" to "integer",
                "description" to "Seconds to wait (1-5)",
                "minimum" to 1,
                "maximum" to 5
            )
        ),
        "required" to listOf("seconds")
    )
)
```

### 4. Bump MAX_TOOL_STEPS
```kotlin
private const val MAX_TOOL_STEPS = 20
```

### 5. Add `long_press` tool
```kotlin
val LONG_PRESS = Tool(
    name = "long_press",
    description = "Long-press a UI element by its numeric ID (for context menus, copy/paste, etc.)",
    inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "element_id" to mapOf(
                "type" to "integer",
                "description" to "The numeric ID of the element to long-press"
            )
        ),
        "required" to listOf("element_id")
    )
)
```

---

## Files to Modify
- `PhoneAgentPrompts.kt` — new system prompt
- `PhoneTools.kt` — add think, wait, long_press tools
- `PhoneAgentApi.kt` — handle think (no-op), wait (delay + screen read), long_press execution
- `ScreenReader.kt` — add `longPressElement()` method
- `ChatViewModel.kt` — bump MAX_TOOL_STEPS to 20, handle think tool (don't show in UI)
- Tests: update existing + add new for think/wait/long_press tools
