# Pressure Test: #494 — Streaming for Chat Path

**Feature:** SSE streaming for conversational chat responses  
**Reference:** OpenClaw streaming architecture (pi-embedded, `streamFn`, Telegram draft streaming)  
**Date:** 2026-02-16  

## 1. How OpenClaw Implements Streaming

### Architecture
- OpenClaw delegates streaming to `@mariozechner/pi-ai` SDK (`streamSimple` function)
- The SDK wraps Vercel AI SDK's streaming primitives (provider-agnostic)
- `streamFn` is a composable function `(model, context, options) => StreamResult`
- Streaming is the **default** — all LLM calls stream. Non-streaming is the exception.
- Extra params (temperature, maxTokens, cacheRetention) are applied via `streamFn` wrappers

### Output Delivery (Telegram)
- `streamMode` config controls how streamed tokens reach the user:
  - `off` — wait for full response, send once
  - `partial` — edit message as tokens arrive (Telegram `editMessageText`)
  - `block` — buffer tokens into chunks, send chunks
- Draft streaming uses Telegram's draft/topics API for lower-latency updates
- Streaming is **decoupled**: LLM streaming (token generation) is independent from output streaming (message delivery)

### Key Patterns
1. **Stream → full text → deliver**: LLM streams tokens, full text is assembled, then delivery layer decides how to chunk/edit
2. **Streaming wrapping**: `streamFn` can be wrapped for logging, extra params, error handling
3. **No streaming for tool calls**: When model returns tool_use, the full response is parsed after streaming completes (SDK handles this internally)

## 2. How Fawx Should Implement Streaming

### Architecture Differences
| Aspect | OpenClaw | Fawx |
|--------|----------|--------|
| HTTP client | Vercel AI SDK / `fetch` | OkHttp (Android) |
| SSE parsing | SDK handles internally | Must implement ourselves |
| Streaming default | All calls stream | Only chat mode streams |
| Output delivery | Telegram edit/draft | Compose UI state update |
| Tool use streaming | SDK assembles tool calls from stream | Not needed (tool mode = non-streaming) |

### Design: Callback-Based Streaming

**Why callback over Flow:**
- Simpler threading model — callback runs inside `withContext(Dispatchers.IO)`, Compose `SnapshotStateList` is thread-safe
- No need for Flow operators (filtering, mapping, etc.)
- Minimal interface changes (one optional parameter)
- The call still returns `Result<String>` with the full text

**Interface addition:**
```kotlin
interface ProviderClient {
    // Existing
    suspend fun chat(conversation: Conversation): Result<String>
    
    // New — streaming variant with default fallback
    suspend fun chatStreaming(
        conversation: Conversation,
        onDelta: (String) -> Unit
    ): Result<String> = chat(conversation).also { result ->
        result.onSuccess { onDelta(it) }
    }
}
```

**SSE transport in BaseProviderClient:**
```kotlin
protected suspend fun executeStreamingRequest(
    requestBody: JsonObject,
    parseSSEDelta: (String) -> String?,
    isDone: (String) -> Boolean,
    onDelta: (String) -> Unit
): Result<String>
```

Adds `"stream": true` to the request body, reads SSE lines, calls `parseSSEDelta` on each `data:` line, emits via `onDelta`.

### SSE Format Parsing

**Anthropic:**
```
event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}
...
event: message_stop
data: {"type":"message_stop"}
```
- Extract text from `delta.text` when type is `content_block_delta`
- Done when type is `message_stop`

**OpenAI/OpenRouter:**
```
data: {"choices":[{"delta":{"content":"Hello"},"finish_reason":null}]}
...
data: [DONE]
```
- Extract text from `choices[0].delta.content`
- Done when data is `[DONE]`

### Call chain (streaming path):
```
ChatViewModel.sendMessage()
  → phoneAgentApi.sendMessage(onTextDelta = callback)
    → chatClient.chatStreaming(conversation, onTextDelta)
      → executeStreamingRequest(body, parseSSEDelta, isDone, onDelta)
        → OkHttp SSE → parse lines → call onDelta
```

### UI Integration:
1. ChatViewModel pre-adds empty assistant message to `messages` list
2. Passes `onDelta` callback that appends delta text to that message via index
3. When streaming completes, message is already fully populated
4. If `chatStreaming()` fails or returns error, remove/update the message

## 3. Comparison

### Same as OpenClaw:
- ✅ Streaming is per-provider (each provider parses its own SSE format)
- ✅ Tool calls are NOT streamed (only chat text)
- ✅ Final result is the complete text (streaming is a transport optimization)
- ✅ Error handling falls back to non-streaming behavior

### Different from OpenClaw (intentional):
- ⚠️ **No streaming SDK**: We implement SSE parsing ourselves (necessary for Android/OkHttp)
- ⚠️ **Callback, not streamFn wrapping**: OpenClaw's composable `streamFn` pattern is SDK-specific; callback is simpler for our use case
- ⚠️ **Chat mode only**: OpenClaw streams everything; we only stream conversational responses (tool mode is fast enough with Haiku <1s)
- ⚠️ **No retry on streaming**: OpenClaw's SDK may handle retries internally; we fail immediately on streaming errors (429 retry only applies to non-streaming `executeRequest`)

### Gaps:
- **DEFERRED**: Streaming for tool responses (low priority — Haiku is <1s)
- **DEFERRED**: Streaming output chunking (like OpenClaw's `block` mode) — not needed for on-device Compose UI
- **DEFERRED**: Connection timeout tuning for streaming (SSE connections are long-lived; may need different timeout than regular requests)

## 4. Security Considerations

- SSE streaming doesn't change the security model — same API keys, same endpoints
- No new untrusted input surface (we're reading from the same API we already trust)
- Streaming doesn't affect tool gating or boundary checks (those only apply to tool mode)

## 5. Test Plan

1. **Unit tests for SSE parsing**: Feed Anthropic and OpenAI SSE lines, verify correct delta extraction
2. **Unit test for done detection**: Verify `message_stop` (Anthropic) and `[DONE]` (OpenAI)
3. **Integration test**: Mock OkHttp response with SSE body, verify `chatStreaming()` emits correct deltas and returns full text
4. **Edge cases**: Empty deltas, malformed JSON in SSE data, connection drop mid-stream
5. **Fallback test**: Verify default `chatStreaming()` falls back to `chat()` when not overridden
