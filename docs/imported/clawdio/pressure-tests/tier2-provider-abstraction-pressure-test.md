# Retroactive Pressure Test: Provider Abstraction

*Pressure test for #478 — Tier 2 retroactive audit*
*Citros: `BaseProviderClient.kt`, `AnthropicClient.kt`, `OpenAiClientImpl.kt`, `OpenRouterClientImpl.kt`, `PhoneAgentApi.kt` | OpenClaw: `types.ts` (Model/StreamFn), pi-ai streaming layer*

---

## 1. OpenClaw's Architecture (Source-Level)

### Provider Model

OpenClaw abstracts providers through the `Model` type and `streamSimple`/`StreamFn` function:

```typescript
type StreamFn = (...args: Parameters<typeof streamSimple>) => ReturnType<typeof streamSimple> | Promise<...>;
```

The `AgentLoopConfig` carries:
- `model: Model<any>` — identifies provider + model ID
- `apiKey: string` — static key (or resolved dynamically via `getApiKey`)
- `getApiKey?: (provider: string) => Promise<string | undefined>` — dynamic key resolution for expiring tokens (e.g., GitHub Copilot OAuth)

**Key design:**
1. **Single streaming interface** — `streamSimple()` handles all providers. Provider-specific protocol differences (Anthropic Messages API vs OpenAI Chat Completions) are handled inside `pi-ai`.
2. **No explicit client classes** — there's no `AnthropicClient` or `OpenAiClient`. The `Model` object identifies the provider, and the streaming function dispatches internally.
3. **Configuration via `SimpleStreamOptions`** — temperature, max_tokens, thinking level, etc. are passed as options, not baked into a client instance.
4. **Dynamic API key resolution** — `getApiKey` is called before each LLM call, supporting token rotation for OAuth-based providers.

### Provider Switching

OpenClaw supports runtime model/provider switching. The `AgentLoopConfig.model` can be changed between runs. There's no concept of "chat client vs action client" — a single model is used for the entire loop.

### Error Handling

Errors from the streaming layer surface as `stopReason: "error"` on the assistant message. The agent loop exits on error without retry — retry logic, if any, lives at the application layer.

### Context Transformation

`transformContext` in `AgentLoopConfig` handles context window management before the LLM call. `convertToLlm` converts `AgentMessage[]` to provider-compatible `Message[]`.

---

## 2. Citros's Architecture

### Provider Abstraction

Citros uses an explicit class hierarchy:

```
ProviderClient (interface)
  └── BaseProviderClient (abstract)
        ├── AnthropicClient
        ├── OpenAiClientImpl
        └── OpenRouterClientImpl
```

**ProviderClient interface** defines:
- `chat(conversation: Conversation): Result<String>` — simple text chat
- `chatWithTools(messages, systemPrompt?, tools, tokenLimit?): Result<ChatResponse>` — tool-capable chat
- `describeImage(base64Image, prompt, maxTokens): Result<String>` — vision
- `provider: Provider` — enum (ANTHROPIC, OPENAI, OPENROUTER)
- `modelId: String`

### BaseProviderClient

Shared HTTP logic (~250 lines):

1. **Shared OkHttpClient** — connection pooling, 30s connect / 60s read / 30s write timeouts.

2. **Retry policy** — Only retries 429 (rate limit) with exponential backoff (1s, 2s, 4s). No retry for auth errors (401/403), server errors (5xx), or network failures. Rationale documented in class KDoc.

3. **Daily rate limit detection** — `isDailyRateLimit()` detects OpenAI/OpenRouter daily quota errors and skips retry (retrying a daily cap is pointless).

4. **Error formatting** — `formatApiErrorMessage()` produces human-readable messages per status code and provider. Handles OpenAI OAuth scope errors specifically.

5. **Template methods** — subclasses implement:
   - `buildChatRequest(conversation): JsonObject`
   - `buildToolRequest(messages, systemPrompt, tools, maxTokens): JsonObject`
   - `parseChatResponse(jsonResponse): String?`
   - `parseToolResponse(jsonResponse): ChatResponse`
   - `buildVisionRequest(base64Image, prompt, maxTokens): JsonObject`

### AnthropicClient

- System prompt as top-level `system` field (not a message)
- Prompt caching: `cache_control: { type: "ephemeral" }` on system prompt and last tool definition
- Tool results sent as `role: "user"` with content blocks (Anthropic protocol)
- Response parsing: content array with `type: "text"` and `type: "tool_use"` blocks
- Token type identification: API key (`sk-ant-api*`) vs setup token (`sk-ant-oat*`)

### OpenAiClientImpl

- System prompt as first `role: "system"` message
- Tools wrapped in `type: "function"` → `function: { name, description, parameters }`
- Tool calls in assistant messages as `tool_calls` array with `function.arguments` as JSON string
- Tool results as `role: "tool"` with `tool_call_id`
- Malformed tool call JSON handled gracefully (skip and continue)

### OpenRouterClientImpl

- Nearly identical to OpenAiClientImpl (OpenAI-compatible API)
- Same request/response format
- Separate class for provider-specific config (headers, base URL)

### Dual-Model Architecture

`PhoneAgentApi` takes two clients:
```kotlin
class PhoneAgentApi(
    private val chatClient: ProviderClient,   // For user-facing (e.g., Sonnet)
    private val actionClient: ProviderClient, // For action loop (e.g., Haiku)
)
```

- `sendMessage(isActionLoop=false)` → `chatClient`
- `sendMessage(isActionLoop=true)` → `actionClient`
- `continueAfterTools()` → always `actionClient`
- `screenshot` vision → always `chatClient` (needs vision-capable model)

### Model Floor Validation

```kotlin
init {
    if (actionModelId != null && !ModelConfig.isModelAboveFloor(actionClient.provider, actionModelId)) {
        throw IllegalArgumentException("Action model below minimum capability floor")
    }
}
```

Prevents weak models from being used in the action loop (processes untrusted screen content).

### ProviderConfig

```kotlin
data class ProviderConfig(
    val provider: Provider,
    val baseUrl: String,
    val chatModelId: String,
    val actionModelId: String,
    val headers: Map<String, String>
)
```

---

## 3. Comparison Table

| Aspect | OpenClaw | Citros | Notes |
|--------|----------|--------|-------|
| **Abstraction style** | Function-based (`streamSimple`) | Class hierarchy (inheritance) | Different paradigms |
| **Provider count** | Many (via pi-ai) | 3 (Anthropic, OpenAI, OpenRouter) | Citros can add more |
| **Dual-model** | ❌ Single model per loop | ✅ chatClient + actionClient | Citros advantage for cost |
| **Model floor** | Not in core | ✅ `ModelConfig.isModelAboveFloor()` | Security measure |
| **Retry policy** | Not in core loop | ✅ 429-only with exponential backoff | Well-designed |
| **Daily limit detection** | Unknown | ✅ Detects OpenAI/OpenRouter daily caps | Avoids pointless retries |
| **Error messages** | Stream-level error | Per-status-code human-readable messages | Citros better UX |
| **Prompt caching** | Via pi-ai | ✅ Anthropic `cache_control` explicit | -90% cost on cache hits |
| **Dynamic API keys** | ✅ `getApiKey()` per call | ❌ Static keys at construction | See D1 |
| **Streaming** | ✅ Token-level streaming | ❌ Request/response (non-streaming) | See D2 |
| **Vision** | Via pi-ai multimodal | ✅ `describeImage()` on ProviderClient | Both support |
| **Context transform** | ✅ `transformContext` hook | ✅ `ContextCompactor` + `ContextManager` | Different patterns |
| **Tool call malformation** | Validated via TypeBox | Graceful skip (OpenAI/OpenRouter) | Both handle |
| **Connection pooling** | Node.js default | ✅ Shared OkHttpClient | Both adequate |
| **Provider config** | Model object | `ProviderConfig` data class | Both work |

---

## 4. Gaps Found

### Critical

**None.** The provider abstraction is solid and covers the three major providers adequately.

### Deferred

#### D1: No Dynamic API Key Resolution
**Gap:** OpenClaw supports `getApiKey()` per LLM call for expiring tokens (OAuth). Citros passes API keys at construction time.
**Impact:** Can't support OAuth-based providers (e.g., future GitHub Copilot integration) or key rotation during long-running sessions.
**Recommendation:** H3 — add optional `apiKeyResolver: suspend (Provider) -> String?` to `ProviderConfig` or `PhoneAgentApi`. Low priority unless OAuth providers are planned.

#### D2: No Streaming Support
**Gap:** OpenClaw streams tokens as they arrive. Citros waits for the complete response.
**Impact:** Perceived latency — user sees nothing during model thinking (can be 5-15 seconds for complex queries). Especially noticeable for the chat client (Sonnet).
**Recommendation:** H2 — streaming for the chat path would significantly improve UX. The action loop (Haiku) is fast enough that streaming is less critical.
**File as issue:** Yes.

#### D3: OpenRouterClientImpl is Nearly Identical to OpenAiClientImpl
**Gap:** ~95% code duplication between `OpenAiClientImpl` and `OpenRouterClientImpl`. Both use OpenAI-compatible format.
**Impact:** Maintenance burden — any fix to OpenAI parsing must be duplicated.
**Recommendation:** H2 — refactor: make `OpenAiClientImpl` handle both, differentiated by `ProviderConfig.provider`. Or extract shared OpenAI-compatible base class.
**File as issue:** Yes.

#### D4: No Provider Failover
**Gap:** If the configured provider fails (outage, key exhaustion), there's no automatic failover to an alternative provider.
**Impact:** Service interruption during provider outages.
**Recommendation:** H3 — consider a `FailoverClient` wrapper that tries providers in order. Low priority — manual provider switching via settings is adequate for now.

### Intentional Divergences

#### I1: Class Hierarchy vs Function-Based
Citros uses OOP class hierarchy; OpenClaw uses function composition. Both are valid. The class hierarchy maps naturally to Kotlin/Android patterns and provides clear extension points via template methods.

#### I2: Dual-Model Architecture
Citros's chatClient/actionClient split enables cost optimization (expensive model for planning, cheap model for action execution). OpenClaw uses a single model. This is a Citros advantage — important for mobile where API costs matter more (user-funded vs developer-funded).

#### I3: Non-Streaming (Current)
Streaming is deferred, not omitted by design. The action loop (Haiku) has sub-second response times where streaming adds complexity without perceived benefit. The chat path would benefit, hence D2.

#### I4: Prompt Caching (Anthropic-Specific)
Citros explicitly manages Anthropic's prompt caching headers. OpenClaw handles this inside pi-ai. Both achieve the same -90% cost benefit. Citros's explicit approach gives more control over what gets cached (system prompt + last tool definition).

---

## 5. Recommendations

1. **Streaming for chat path** (D2) should be an H2 priority — it's the single biggest UX improvement for perceived responsiveness.

2. **Deduplicate OpenRouter/OpenAI clients** (D3) — straightforward refactor, reduces maintenance surface.

3. **The dual-model architecture** (I2) is a genuine advantage. Document the cost savings in architecture docs.

4. **The retry policy** is well-designed — 429-only with daily limit detection is better than naive retry-everything approaches.

5. **Model floor validation** is a good security pattern. Consider extending it to chatClient as well (currently only validates actionClient).
