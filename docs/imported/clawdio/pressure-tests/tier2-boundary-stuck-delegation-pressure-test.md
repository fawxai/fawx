# Retroactive Pressure Test: Boundary Checks, Stuck Detection & Tool Execution Delegation

*Pressure test for #478 — Tier 2 retroactive audit (Group A: Executor Infrastructure)*
*Citros: `BoundaryCheck.kt`, `StuckDetector.kt`, `AgentExecutor.kt` | OpenClaw: `agent-loop-core.ts`, `types.ts`*

---

## 1. OpenClaw's Architecture (Source-Level)

### Boundary Checks / Loop Control

OpenClaw's agent loop (`agent-loop-core.ts`) has **no formalized boundary check system**. Loop exit is controlled by:

1. **AbortSignal** — passed through from caller, checked via standard JS abort semantics. Cancellation is cooperative through the signal propagated to `streamAssistantResponse()` and `tool.execute()`.

2. **Stop reasons** — The loop exits when `message.stopReason === "error"` or `message.stopReason === "aborted"` (line ~130). Otherwise it continues as long as there are tool calls.

3. **No step limit** — There is no `maxSteps` counter in `agent-loop-core.ts`. The loop runs until the model stops emitting tool calls or an error/abort occurs. Step limits, if any, are enforced at the application layer (e.g., `agent-session.ts`).

4. **No stuck detection in core** — The core loop has no screen hash tracking or stuck detection. This is domain-specific to phone agents and doesn't apply to OpenClaw's CLI coding context.

### Steering

OpenClaw implements steering via `config.getSteeringMessages()`:

- **After each tool execution** (`agent-loop-core.ts` line ~280): After each tool result, `getSteeringMessages()` is called. If messages are returned, remaining tool calls are **skipped** (with "Skipped due to queued user message" results) and steering messages are injected before the next LLM call.

- **After each turn** (line ~145): Called again after the turn completes, feeding into the next iteration's `pendingMessages`.

- **Follow-up messages** (line ~155): `getFollowUpMessages()` is called when the agent would otherwise stop. If messages exist, the outer loop continues.

- **At loop start** (line ~106): `getSteeringMessages()` is checked once before the first turn.

There is **no pre-batch steer checkpoint** — steering is only checked after individual tool executions, not between the API response and tool execution start.

### Tool Execution Delegation

OpenClaw uses `AgentTool` objects with an `execute` method:

```typescript
interface AgentTool<TParameters, TDetails> extends Tool<TParameters> {
  label: string;
  execute: (toolCallId, params, signal?, onUpdate?) => Promise<AgentToolResult<TDetails>>;
}
```

- Tools are self-contained: each tool knows how to execute itself.
- `executeToolCalls()` (line ~238) iterates tool calls, finds the matching `AgentTool` by name, validates arguments via `validateToolArguments()`, then calls `tool.execute()`.
- Extension wrapping (`ext-wrapper.ts`) adds pre/post hooks: `tool_call` event (can block), `tool_result` event (can modify result).
- Tool results are `AgentToolResult<T>` with `content` (text/image blocks) and `details` (typed metadata).
- Errors are caught and returned as `isError: true` tool results — they don't crash the loop.

### Key Patterns

- **Event-driven**: Every lifecycle event (tool_execution_start/end, message_start/end, turn_start/end) is pushed to an `EventStream`.
- **No output classification**: The core loop doesn't classify tool results for visibility — that's a UI concern handled by the application layer.
- **Parallel tool calls**: Not supported — tools execute sequentially within a batch.

---

## 2. Citros's Architecture

### Boundary Checks

Citros formalizes loop control as a **BoundaryCheck pipeline** (`BoundaryCheck.kt`):

**CheckResult sealed class** — four outcomes with explicit priority:
- `Continue` — no issue
- `Inject(message)` — append text to tool result (e.g., stuck warning)
- `Steer(userMessages)` — inject user messages, skip remaining batch
- `Stop(reason)` — exit loop

**Priority**: Stop > Steer > Inject > Continue. Enforced in `evaluateBoundaryChecks()` (`AgentExecutor.kt` line ~215).

**LoopState** — immutable snapshot passed to checks:
```kotlin
data class LoopState(
    val step: Int,
    val maxSteps: Int,
    val lastToolName: String,
    val lastScreenHash: Int?,
    val isCancelled: Boolean,
    val pendingSteerMessages: List<String>
)
```

**Default check ordering** (`AgentExecutor.defaultBoundaryChecks()`):
1. `CancellationCheck` — exits with "cancelled"
2. `StepLimitCheck` — exits with "max_steps" when `step >= maxSteps`
3. `StuckDetectionCheck` — injects warning when screen unchanged
4. `SteerCheck` — injects user messages from `pendingSteerMessages`

**With accessibility** (`defaultBoundaryChecksWithAccessibility()`):
1. CancellationCheck
2. **AccessibilityGateCheck** — waits for reconnect with timeout, stops with "accessibility_lost"
3. StepLimitCheck
4. StuckDetectionCheck
5. SteerCheck

**evaluateBoundaryChecks()** runs all checks, collecting Inject messages. Stop short-circuits. Steer wins over Inject. Multiple Injects are concatenated.

### Stuck Detection

`StuckDetector.kt` — stateful detector with two modes:

1. **Screen hash repetition**: Rolling window of `screenThreshold` (default 3) hashes. All identical → stuck warning with count of unique screens seen.

2. **Consecutive waits**: Tracks sequential `wait` tool calls. If `waitThreshold` (default 2) consecutive waits AND screen is stuck → warning.

Screen-stuck warning takes precedence over wait warning.

State is held in `StuckDetector.State` (mutable, per-loop-run):
```kotlin
data class State(
    val recentScreenHashes: MutableList<Int> = mutableListOf(),
    var consecutiveWaits: Int = 0,
    var uniqueScreens: Int = 0
)
```

`StuckDetectionCheck` wraps `StuckDetector` as a `BoundaryCheck`, creating fresh `State` per instance.

### Tool Execution Delegation

`ToolExecutionDelegate` interface (`AgentExecutor.kt` line ~250):

```kotlin
interface ToolExecutionDelegate {
    suspend fun executeToolCall(toolCall: ToolCall, screenContent: ScreenContent?): String
    suspend fun refreshScreen(): ScreenContent?
    suspend fun refreshScreenAfterTool(toolName: String, actionResult: String): ScreenContent?
    suspend fun settleDelay(toolName: String, actionResult: String)
    fun formatToolResult(actionSummary: String, screenContent: ScreenContent?): String
    fun isUiMutatingTool(toolName: String): Boolean
    fun isScreenReaderAvailable(): Boolean
    suspend fun waitForAccessibility(timeoutMs: Long): Boolean
    fun accessibilityWaitMs(): Long
    fun outputVerbosity(): OutputVerbosity
    fun addToolResult(toolCallId: String, result: String)
    fun addSteerMessage(text: String)
    fun onStepStarted(step: Int, maxSteps: Int)
}
```

**Actual execution** happens in `PhoneAgentApi.executeToolCall()` — a giant `when` dispatch over 27 tool names. ChatViewModel implements `ToolExecutionDelegate` and bridges to `PhoneAgentApi`.

**AgentExecutor.run()** loop:
1. Pre-batch steer check (drains `steerMessageSource()` before any tool executes)
2. For each tool call in batch:
   - Execute via `delegate.executeToolCall()`
   - Classify output visibility via `OutputClassifier`
   - Settle delay
   - Refresh screen if UI-mutating
   - Format tool result
   - Drain steer messages, build LoopState
   - Run `evaluateBoundaryChecks()`
   - Handle result (Stop/Steer/Inject/Continue)
3. Call `continueAfterTools()` for next model response
4. Error handling: API errors after steer → exit with "api_error_after_steer"

**LoopResult** sealed class:
- `Completed(text, steps, exitReason)` — normal completion with structured reason
- `Error(message, steps)` — unrecoverable error

### Steering Implementation

Citros has **two steer checkpoints** (more than OpenClaw):

1. **Pre-batch** (`AgentExecutor.run()` line ~115): After API returns but before ANY tool executes. Zero wasted actions.
2. **Post-tool** (line ~165): After each tool, via `SteerCheck` in the boundary check pipeline.

Steer semantics:
- Remaining tool calls get explicit "Skipped: user sent a new message" results
- Messages delivered via `delegate.addSteerMessage()` as user-role turns
- Loop continues (doesn't exit)

---

## 3. Comparison Table

| Aspect | OpenClaw | Citros | Notes |
|--------|----------|--------|-------|
| **Boundary check formalism** | None — ad-hoc loop control | `BoundaryCheck` interface + `CheckResult` sealed class | Citros more structured |
| **Check ordering** | N/A | Explicit: Cancel → A11y → StepLimit → Stuck → Steer | Configurable via constructor |
| **Step limit** | Not in core loop | `StepLimitCheck` (default 25) | OpenClaw may enforce elsewhere |
| **Cancellation** | `AbortSignal` propagation | `CancellationCheck` + inline guards | Both effective, different idioms |
| **Stuck detection** | None | `StuckDetector` with screen hash + wait tracking | Phone-specific, N/A for CLI |
| **Steer: pre-batch** | ❌ Not present | ✅ Zero-waste steer before tool execution | Citros advantage |
| **Steer: post-tool** | ✅ After each tool | ✅ After each tool via SteerCheck | Equivalent |
| **Steer: follow-up** | ✅ `getFollowUpMessages()` | ❌ Not present | OpenClaw advantage |
| **Tool skip on steer** | ✅ "Skipped due to queued user message" | ✅ "Skipped: user sent a new message" | Equivalent |
| **Tool execution model** | Self-executing `AgentTool.execute()` | Delegate pattern (`ToolExecutionDelegate`) | Different but valid |
| **Tool extension hooks** | ✅ `ext-wrapper.ts` pre/post hooks | ❌ Not present | Extension system not needed yet |
| **Tool result type** | `AgentToolResult<T>` (typed content+details) | `String` | Citros simpler but less structured |
| **Output classification** | Not in core | `OutputClassifier` (SHOW/SHOW_DIMMED/HIDE) | Citros has built-in UI visibility |
| **Event stream** | ✅ Rich event types | `LoopProgressListener` (2 events) | OpenClaw more granular |
| **Screen refresh after tools** | N/A (no screen) | `refreshScreenAfterTool()` + `isUiMutatingTool()` | Phone-specific |
| **Accessibility gating** | N/A | `AccessibilityGateCheck` with timeout+reconnect | Phone-specific |
| **Loop result** | Returns `AgentMessage[]` | `LoopResult` sealed class with exitReason | Both structured |
| **API error handling** | Stream-level error/abort | try/catch → `ChatResponse` with stopReason="error" | Different but adequate |

---

## 4. Gaps Found

### Critical (Fix Before H2)

**None.** Citros's boundary check system is more formalized than OpenClaw's equivalent. The architecture is sound and well-documented.

### Deferred

#### D1: No Follow-Up Message Support
**Gap:** OpenClaw has `getFollowUpMessages()` for messages that arrive after the agent finishes but should trigger continuation. Citros has no equivalent.
**Impact:** If a user sends a message right as the loop ends, it becomes a new conversation turn rather than seamlessly continuing.
**Recommendation:** H3 — low priority, current UX handles this naturally via new `sendMessage()` calls.

#### D2: Tool Result Type is String
**Gap:** OpenClaw uses typed `AgentToolResult<T>` with structured content blocks (text + images) and typed details. Citros uses plain `String` for all tool results.
**Impact:** Limits structured tool output (e.g., returning images inline, typed metadata for UI rendering). Currently fine because screen content is appended as text.
**Recommendation:** H3 — revisit if tools need to return structured data (images, JSON objects) directly.

#### D3: Limited Event Granularity
**Gap:** OpenClaw emits ~10 event types (agent_start/end, turn_start/end, message_start/update/end, tool_execution_start/update/end). Citros has `LoopProgressListener` with only `onToolResult()` and `onAccessibilityLost()`.
**Impact:** Limits UI reactivity (e.g., can't show tool execution in progress, can't animate per-turn).
**Recommendation:** H3 — expand `LoopProgressListener` when UI needs more granularity.

#### D4: No Tool Extension/Hook System
**Gap:** OpenClaw's `ext-wrapper.ts` allows extensions to intercept tool calls (block) and modify results. Citros has no equivalent.
**Impact:** Can't build plugin-style extensions that modify tool behavior. Not needed now.
**Recommendation:** H3+ — only if extension/plugin architecture is planned.

### Intentional Divergences

#### I1: Formalized BoundaryCheck Pipeline (Citros > OpenClaw)
Citros's `BoundaryCheck` interface with `CheckResult` sealed class is MORE structured than OpenClaw's ad-hoc loop control. This is intentional — the phone agent domain has more boundary conditions (accessibility, stuck detection, screen state) that benefit from a pluggable pipeline.

#### I2: Pre-Batch Steer Checkpoint (Citros > OpenClaw)
Citros checks for steer messages BEFORE executing any tools in a batch. OpenClaw only checks after each tool execution. This means Citros can redirect with zero wasted actions when the user sends a message during model thinking.

#### I3: Stuck Detection (Phone-Specific)
OpenClaw has no stuck detection because CLI tools always produce different output. Screen hash repetition and consecutive wait detection are phone-agent-specific concerns.

#### I4: Accessibility Gating (Phone-Specific)
The accessibility service can disconnect mid-loop on Android. `AccessibilityGateCheck` with wait-for-reconnect is a phone-specific concern with no OpenClaw equivalent.

#### I5: Delegate Pattern vs Self-Executing Tools
Citros uses `ToolExecutionDelegate` (bridge pattern) because execution requires Android platform APIs (ScreenReader, ClipboardHelper, etc.) that live outside `:core`. OpenClaw uses self-executing tools because tool implementations are self-contained Node.js functions. Both are correct for their platforms.

#### I6: Output Classification in Core
Citros includes `OutputClassifier` in the executor loop for UI visibility control. OpenClaw handles this at the application layer. Citros's approach is pragmatic — the phone UI needs to differentiate mechanical taps from meaningful results during the loop, not after.

---

## 5. Recommendations

1. **No blockers for H2.** The boundary check system is well-designed and more rigorous than the reference implementation.

2. **Consider `getFollowUpMessages()` pattern** (D1) for H3 if seamless conversation continuation becomes a UX priority.

3. **Keep the string-based tool result type** (D2) for now. Migration to structured types should only happen when a concrete tool needs to return non-text data (e.g., inline images).

4. **The pre-batch steer checkpoint** (I2) is a genuine improvement over OpenClaw. Document this as a design win.

5. **Boundary check ordering is critical** — the current order (Cancel → A11y → StepLimit → Stuck → Steer) is correct. Any reordering should be pressure-tested against edge cases (e.g., cancel during accessibility wait).
