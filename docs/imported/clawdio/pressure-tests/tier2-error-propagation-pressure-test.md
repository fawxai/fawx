# Retroactive Pressure Test: Error Propagation & Visibility

*Pressure test for #478 — Tier 2 retroactive audit (#480)*
*Citros: `AgentExecutor.kt`, `PhoneAgentApi.kt`, `BaseProviderClient.kt`, `OutputClassifier.kt` | OpenClaw: `agent-loop-core.ts`, `ext-wrapper.ts`*

---

## 1. OpenClaw's Architecture (Source-Level)

### Error Propagation Paths

OpenClaw has three error propagation layers:

**1. Tool Execution Errors** (`agent-loop-core.ts` line ~260):
```typescript
try {
  result = await tool.execute(toolCallId, validatedArgs, signal, onUpdate);
} catch (e) {
  result = { content: [{ type: "text", text: e.message }], details: {} };
  isError = true;
}
```
- Errors are caught and converted to `isError: true` tool results
- The error message is passed back to the model as tool result content
- The loop **continues** — tool errors don't crash the agent
- `tool_execution_end` event carries `isError` for UI rendering

**2. Streaming/API Errors** (`agent-loop-core.ts` line ~205):
- Stream errors surface as `stopReason: "error"` on the assistant message
- The loop exits immediately on error or abort: `if (message.stopReason === "error" || message.stopReason === "aborted")` → push turn_end + agent_end
- No retry at the core loop level

**3. Extension Hook Errors** (`ext-wrapper.ts`):
- `tool_call` handler errors → re-thrown, blocking tool execution
- `tool_result` handler errors for success → silent (result passes through unmodified, logged at extension layer)
- `tool_result` handler for errors → emitted but error still thrown to normal flow

### Error Visibility

- `isError` flag on tool results enables UI to render errors differently
- `tool_execution_end` event includes `isError` for real-time UI updates
- Error/abort stop reasons trigger `agent_end` for clean UI teardown
- No output classification system in core — visibility is an application concern

### Argument Validation

`validateToolArguments()` runs before execution. Schema validation errors throw before any tool side effects occur.

---

## 2. Citros's Architecture

### Error Propagation Paths

Citros has **five** error propagation layers:

**1. Tool Execution Errors** (`AgentExecutor.kt` line ~160):
```kotlin
val actionResult = try {
    delegate.executeToolCall(toolCall, screenContent)
} catch (e: Exception) {
    "Error: ${e.message?.take(ERROR_MESSAGE_MAX_LENGTH)}"
}
```
- Exceptions caught and converted to `"Error: ..."` string (truncated to 100 chars)
- Loop **continues** — error becomes the tool result passed to the model
- Model sees the error and can decide to retry or report to user

**2. Per-Tool Error Handling** (`PhoneAgentApi.executeToolCall()` outer catch):
```kotlin
return try {
    val result = when (toolCall.name) { ... }
    result
} catch (e: Exception) {
    "Failed: ${toolCall.name}: ${e.message}"
}
```
- Each tool branch can throw (e.g., `IllegalArgumentException` for bad params)
- Outer catch formats as `"Failed: toolName: message"`
- Two-tier pattern: inner tool-specific errors + outer generic catch

**3. Specialized Tool Error Handlers** (`PhoneAgentApi`):
- `fileToolResult()` — catches SecurityException, IllegalArgumentException, IllegalStateException separately, returns JSON `{"ok":false,"tool":"...","error":"..."}`
- `memoryToolResult()` — same pattern for memory tools
- `requireValidNotificationKey()` — validates notification key format before platform API call

**4. API/Provider Errors** (`BaseProviderClient.executeRequest()`):
- 429 rate limit → retry with exponential backoff (up to maxAttempts)
- Daily rate limit → no retry, fail with explanation
- 401/403 → `ProviderException(isAuthFailure=true)` — no retry
- 500+ → `ProviderException` — no retry
- Network errors → `ProviderException(statusCode=null)` — no retry
- All errors formatted via `formatApiErrorMessage()` for human readability

**5. API Error in AgentExecutor** (`AgentExecutor.run()`):
```kotlin
// API error after steer delivery
response = try {
    continueAfterTools()
} catch (e: Exception) {
    return LoopResult.Completed(
        text = "Error: ${e.message?.take(ERROR_MESSAGE_MAX_LENGTH)}",
        steps = toolSteps,
        exitReason = "api_error_after_steer"
    )
}

// Normal continuation — API error becomes a ChatResponse with stopReason="error"
response = try {
    continueAfterTools()
} catch (e: Exception) {
    ChatResponse(
        text = "Error: ${e.message?.take(ERROR_MESSAGE_MAX_LENGTH)}",
        toolCalls = emptyList(),
        stopReason = "error"
    )
}
```
- Post-steer API errors → explicit exit with "api_error_after_steer" reason
- Normal API errors → converted to ChatResponse with empty tool calls, causing loop to exit naturally on next iteration

### Error Visibility

**OutputClassifier** (`OutputClassifier.kt`):
```kotlin
fun classify(toolName: String, result: String): OutputVisibility {
    return when {
        result.startsWith("Failed") || result.startsWith("Error:") -> OutputVisibility.SHOW
        toolName == "think" -> OutputVisibility.SHOW_DIMMED
        toolName in PROMINENT_TOOLS -> OutputVisibility.SHOW
        toolName in MECHANICAL_TOOLS -> OutputVisibility.HIDE
        else -> OutputVisibility.SHOW_DIMMED
    }
}
```

- **Errors always SHOW** — regardless of tool type, if result starts with "Failed" or "Error:", visibility is SHOW
- Three visibility levels: SHOW (prominent), SHOW_DIMMED (italic/dimmed), HIDE (mechanical actions)
- `OutputVerbosity` user preference overrides: VERBOSE shows everything, MINIMAL hides dimmed

**Display formatting:**
```kotlin
fun formatForDisplay(toolName, result, visibility): String? {
    HIDE -> null
    SHOW_DIMMED -> "💭" or "⚙️" prefix
    SHOW -> "🤖" prefix
}
```

### Accessibility Loss Error Path

`AccessibilityGateCheck`:
1. `isAvailable()` returns false
2. `waitForReconnect(timeoutMs)` — waits up to 5s
3. If reconnected → `onReconnected()` + continue
4. If timeout → `onLost()` + `CheckResult.Stop("accessibility_lost")`

This surfaces as `LoopResult.Completed(exitReason = "accessibility_lost")`.

### Error Message Truncation

`AgentExecutor.ERROR_MESSAGE_MAX_LENGTH = 100` — all error messages in the executor are truncated to prevent long stack traces from polluting conversation history.

### PhoneAgentApi.sendMessage() Error Path

```kotlin
val result = client.chatWithTools(...)
return result.fold(
    onSuccess = { response -> /* add to history */ response },
    onFailure = { error ->
        ChatResponse(
            text = "Error: ${error.message}",
            toolCalls = emptyList(),
            stopReason = "error"
        )
    }
)
```

API failures are converted to ChatResponse with `stopReason = "error"` and the error message as text. This appears in the chat as a visible error message to the user.

### Tool Artifact Stripping

`PhoneAgentApi.stripToolArtifacts()` — defense-in-depth against hallucinated tool calls in chat mode. Regex patterns strip `<tool_use>`, `<tool_call>`, `<function_call>` XML and JSON tool objects from plain text responses.

---

## 3. Comparison Table

| Aspect | OpenClaw | Citros | Notes |
|--------|----------|--------|-------|
| **Tool error → model** | ✅ isError flag + error text | ✅ "Failed:/Error:" prefix string | Both let model see errors |
| **Tool error → UI** | isError on event | OutputClassifier: errors always SHOW | Both surface errors |
| **API error handling** | stopReason: "error" → loop exit | Result.failure → ChatResponse(error) | Both exit cleanly |
| **Retry policy** | None in core | 429-only exponential backoff | Citros more resilient |
| **Error message format** | Raw exception message | Human-readable per status code | Citros better UX |
| **Error truncation** | No | 100 char limit | Prevents context pollution |
| **Typed error flag** | ✅ `isError: boolean` | ❌ String prefix convention | See D1 |
| **Output visibility** | Application layer | ✅ `OutputClassifier` in core | Citros baked-in |
| **Accessibility loss** | N/A | ✅ Graceful gate with timeout | Phone-specific |
| **Auth failure detection** | Not explicit | ✅ `isAuthFailure` on ProviderException | Citros can prompt re-auth |
| **Daily limit detection** | Unknown | ✅ Detects and skips retry | Smart optimization |
| **Schema validation errors** | Pre-execution throw | Inline during execution | See D2 |
| **Tool artifact stripping** | N/A | ✅ Regex defense-in-depth | Chat mode safety |
| **Extension error handling** | ✅ Block/pass-through | N/A | No extension system |
| **Verbosity control** | Not in core | ✅ VERBOSE/NORMAL/MINIMAL | User preference |

---

## 4. Gaps Found

### Critical

**None.** Error propagation is comprehensive and well-layered.

### Deferred

#### D1: String-Based Error Detection
**Gap:** Citros detects errors by string prefix (`result.startsWith("Failed")` or `result.startsWith("Error:")`). OpenClaw uses a typed `isError: boolean` flag on tool results.
**Impact:** Fragile — if a tool legitimately returns text starting with "Failed" or "Error:", it would be misclassified as an error. Also, `OutputClassifier` is coupled to the string format.
**Recommendation:** H2 — consider adding an `isError` flag to tool results alongside the string. Could be a simple `Pair<String, Boolean>` or a sealed class `ToolResult(text, isError)`. This decouples error detection from string content.
**File as issue:** Yes.

#### D2: Inconsistent Error Format Between Tool Categories
**Gap:** Three different error formats:
- UI tools: `"Failed: tap: could not tap element 5"` (string)
- File tools: `{"ok":false,"tool":"read_file","error":"Access denied"}` (JSON)
- Memory tools: `{"ok":false,"tool":"remember","error":"..."}` (JSON)
- Executor-level: `"Error: message"` (string)
**Impact:** The model must handle multiple error formats. The `OutputClassifier` only checks for `startsWith("Failed")` and `startsWith("Error:")` — JSON errors starting with `{` are NOT caught and would be classified as SHOW_DIMMED rather than SHOW.
**Recommendation:** H2 — standardize error format. Either all tools return `"Failed: ..."` strings, or all return JSON with `ok` field. The `OutputClassifier` should handle whichever format is chosen.
**File as issue:** Yes — this is a real bug for file/memory tool errors.

#### D3: No Error Recovery / Retry at Tool Level
**Gap:** If a tool fails (e.g., tap fails because element ID is stale), the error is passed to the model but there's no systematic retry mechanism. The model may or may not retry.
**Impact:** Low — the model is generally good at retrying failed actions. But explicit retry guidance (e.g., "retry once with refreshed screen") could improve reliability.
**Recommendation:** H3 — consider adding retry hints to error messages for specific failure modes.

### Intentional Divergences

#### I1: Output Classification in Core
Citros classifies tool output visibility in the executor loop. OpenClaw defers this to the application layer. Citros's approach is pragmatic — the phone UI needs real-time visibility decisions during the loop, not after.

#### I2: Error Message Truncation
Citros truncates error messages to 100 characters in the executor. This prevents verbose stack traces from consuming context window tokens. OpenClaw doesn't truncate, relying on the error itself being concise.

#### I3: Human-Readable API Errors
Citros's `formatApiErrorMessage()` converts HTTP status codes and provider-specific error bodies into user-friendly messages (e.g., "Rate limited: ... Please wait and try again" vs raw JSON). This is important for a consumer mobile app where users aren't developers.

#### I4: Auth Failure Flag
`ProviderException.isAuthFailure` enables the UI to prompt for credential re-entry on 401/403 errors rather than showing a generic error. Good UX pattern for mobile.

---

## 5. Recommendations

1. **Fix the inconsistent error format** (D2) — this is the most actionable finding. File/memory tool JSON errors bypass `OutputClassifier`'s error detection. Either:
   - (a) Update `OutputClassifier.classify()` to also check for `"ok":false` in JSON, or
   - (b) Standardize all tools on `"Failed: ..."` string format, or
   - (c) Introduce a typed `ToolResult(text, isError)` (D1) which makes the format irrelevant.

   Option (c) is the cleanest but most work. Option (a) is the quickest fix.

2. **Add `isError` flag to tool results** (D1) for H2. Even a simple boolean alongside the string would eliminate the fragile string-prefix detection.

3. **The error truncation** (I2) at 100 chars is good. Consider applying it consistently — `PhoneAgentApi.executeToolCall()`'s outer catch doesn't truncate.

4. **Accessibility loss handling** is well-designed — the gate-with-timeout pattern is correct for transient disconnections.

5. **Tool artifact stripping** is a good defense-in-depth measure for chat mode. Keep it.
