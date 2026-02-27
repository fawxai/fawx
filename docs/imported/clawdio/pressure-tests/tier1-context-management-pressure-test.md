# Retroactive Pressure Test: Conversation History & Context Management

*Pressure test for #478 — Tier 1 retroactive audit*
*Fawx: `Message`, `Conversation`, `ContextCompactor`, `ContextManager` | OpenClaw: `AgentMessage`, `AgentContext`, `compaction/` module, `transformContext` hook*

---

## 1. OpenClaw's Architecture (Source-Level)

### Message Model

OpenClaw uses a discriminated union `AgentMessage`:

```typescript
type AgentMessage = UserMessage | AssistantMessage | ToolResultMessage | CustomMessage;

interface UserMessage {
  role: "user";
  content: string | ContentBlock[];  // Text or multimodal (image + text)
}

interface AssistantMessage {
  role: "assistant";
  content: ContentBlock[];           // Always blocks (text, tool_use)
  usage?: Usage;                     // Token usage from the API
  stopReason: string;                // "end_turn", "tool_use", "error", "aborted"
  provider?: string;
  model?: string;
  timestamp: number;
}

interface ToolResultMessage {
  role: "user";                      // Anthropic format: tool results are user messages
  content: ToolResultContent[];
}

interface CustomMessage {
  role: "custom";
  customType: string;                // "bashExecution", "compactionSummary", "branchSummary"
  content: string;
  timestamp?: number;
}
```

**Key decisions:**
1. **Rich assistant messages**: Include `usage`, `stopReason`, `provider`, `model`, `timestamp` — all metadata needed for compaction decisions, retry logic, and model switching
2. **Tool results as user messages**: Follows Anthropic API convention
3. **Custom message type**: For non-LLM messages (bash history, compaction summaries, branch summaries) that should be persisted but filtered before LLM calls
4. **`convertToLlm()` filter**: Custom messages and other non-LLM types are stripped before API calls

### Context Management

OpenClaw uses **three layers** of context management:

**Layer 1: `transformContext` hook** (agent-loop.ts)
- Runs before EVERY LLM call
- Pure function: `AgentMessage[] → AgentMessage[]`
- Used for pre-LLM context pruning/injection
- Currently used by `AgentSession` for extension-injected context

**Layer 2: `convertToLlm`** (messages.ts)
- Runs after `transformContext`, before LLM call
- Converts `AgentMessage[]` to LLM-compatible `Message[]`
- Filters out custom messages (bashExecution, compactionSummary, branchSummary)
- Handles multimodal content blocks

**Layer 3: Compaction** (compaction/ module)
- Triggered by threshold or overflow
- LLM-powered summarization (makes an API call to summarize old messages)
- Preserves recent messages, summarizes old ones
- Tracks file operations across compactions
- Extension hook: `session_before_compact` allows extensions to provide custom compaction

### Compaction Architecture

OpenClaw's compaction is sophisticated:

**Trigger conditions:**
1. **Overflow**: LLM returned context overflow error → compact → auto-retry
2. **Threshold**: Context over `contextWindow - reserveTokens` → compact (no retry)

**Token estimation:**
- Primary: `usage.totalTokens` from last assistant message (actual API count)
- Fallback: `chars / 4` heuristic for messages without usage data
- Both inputs and outputs counted: `input + output + cacheRead + cacheWrite`

**Cut point detection:**
- Binary search for where to split history
- Respects turn boundaries (doesn't split mid-turn)
- `keepRecentTokens` setting controls how many recent tokens to preserve
- Can split a long turn: prefix gets summarized, suffix kept

**Summarization:**
- Makes a dedicated LLM call with a summarization system prompt
- Input: serialized conversation history to summarize
- Tracks file operations (reads, edits) across compactions for continuity
- Iterative: new summary builds on previous summary if one exists
- Extension hook allows custom summarization logic

**Post-compaction:**
- Appends compaction entry to session
- Replaces agent's messages with rebuilt context
- Fires `session_compact` extension event

### Session Persistence

OpenClaw uses a **session entry** model (not raw message persistence):

```typescript
type SessionEntry = 
  | { type: "message"; message: AgentMessage; id: string; parentUuid?: string }
  | { type: "compaction"; summary: string; firstKeptEntryId: string; ... }
  | { type: "branch"; ... }
```

- Every message gets a UUID for branching support
- Compaction entries store the summary and pointer to first kept entry
- Context is rebuilt from entries: `buildSessionContext(entries) → messages[]`
- Branching: sessions can fork, creating tree structures

---

## 2. Fawx's Architecture

### Message Model

```kotlin
@Serializable
data class Message(
    val role: String,          // "user", "assistant", or "tool"
    val content: String,
    val timestamp: Long,
    val toolCallId: String?,   // For tool result messages
    val toolCallsJson: String?, // For assistant messages with tool calls
    @Transient
    private val _contentBlocks: List<Map<String, Any>>?
)
```

**Key decisions:**
1. **Flat string content**: `content` is always a string. Tool call data is stored in `toolCallsJson` and reconstructed via `contentBlocks` property
2. **`@Transient` content blocks**: Rich blocks are transient (not serialized). Reconstructed from persisted fields on deserialization. This is clever — avoids complex serialization while maintaining API compatibility
3. **Tool results use `role = "tool"`**: Follows OpenAI convention, but Anthropic API wants them as user messages. The provider client handles this translation
4. **No usage/metadata**: No token usage, stop reason, model, or provider info stored on messages

### Conversation Container

```kotlin
@Serializable
data class Conversation(
    val messages: MutableList<Message>
) {
    fun toApiMessages(maxMessages: Int = 20): List<Map<String, String>>
}
```

- Simple mutable list
- `toApiMessages()` does basic trimming: take last N, drop leading non-user messages
- No turn-aware trimming (cuts at message boundary, not turn boundary)

### Two-Stage Compaction

**Stage 1: `ContextCompactor`** — regex-based SCREEN dump stripping
- Triggers: estimated tokens > 60k (chars/3)
- Action: strips `\n\nSCREEN:\n...` sections from old tool results
- Preserves: last 2 tool results (need recent screen state)
- Pure function, no LLM call, very cheap

**Stage 2: `ContextManager`** — rule-based message summarization
- Triggers: `currentStep >= compactionThreshold (5)` AND `messages.size > recentWindow + 1`
- Action: compacts old messages into bracket format (`[PREVIOUS SCREEN: Gmail, 10 elements]`, `[Thought: ...]`, `[Action: ...]`)
- Preserves: first message (task) + last 6 messages (recent window)
- Pure function, no LLM call, deterministic

---

## 3. Comparison

### 3.1 Message Model

| Aspect | OpenClaw | Fawx | Assessment |
|--------|----------|--------|------------|
| Type safety | Discriminated union (4 types) | Single data class with nullable fields | **Gap**: Fawx uses role string + nullable fields instead of type-safe union. Kotlin sealed classes would be idiomatic |
| Usage tracking | `usage: Usage` on AssistantMessage | Not tracked | **Gap — H2**: Token usage is essential for cost tracking, compaction triggers, and model tier decisions |
| Stop reason | `stopReason` on AssistantMessage | Not stored | **Deferred**: Needed for retry logic (H3) |
| Model/provider | Stored on AssistantMessage | Not stored | **Deferred**: Needed for multi-model sessions |
| Content blocks | Native `ContentBlock[]` | Transient `_contentBlocks` reconstructed from JSON | **Adequate**: Fawx's approach works, avoids complex serialization. Trade-off: reconstruction logic is fragile |
| Custom messages | Dedicated `role: "custom"` type | Not supported | **Intentional**: No need for bash history or branch summaries on phone |
| Serialization | Full serialization of all fields | `@Transient` blocks + `toolCallsJson` string | **Risk**: If `toolCallsJson` parsing fails, blocks can't be reconstructed. No fallback |

### 3.2 Context Trimming Strategy

| Aspect | OpenClaw | Fawx |
|--------|----------|--------|
| When to trim | Based on actual token usage from API response | Based on char/3 estimate OR step count |
| How to trim | LLM-powered summarization (new API call) | Rule-based: regex stripping + bracket format replacement |
| Cost | Expensive (extra API call per compaction) | Free (pure string operations) |
| Quality | High — LLM understands what to preserve | Medium — fixed rules, can lose important context |
| File tracking | Tracks reads/edits across compactions | Not tracked |
| Turn awareness | Respects turn boundaries, can split long turns | Cuts at message index, not turn-aware |
| First message | Not special-cased (included in summarization) | Always preserved (task description) |
| Recovery | Auto-retry after overflow compaction | No overflow detection or recovery |

**Assessment**: OpenClaw's LLM-powered compaction is overkill for a phone agent's ~25-step tasks. Fawx's rule-based approach is the right call:

1. **Phone tasks are short**: 5-25 steps vs potentially hundreds for coding tasks
2. **Screen dumps are the main bloat**: SCREEN sections are large and become useless after one step (IDs change). Stripping them is the 80/20 solution
3. **No LLM cost**: Each compaction in OpenClaw costs tokens. Phone agents are already expensive per-step (vision, tool calls)
4. **Deterministic**: Rule-based compaction is predictable and testable

**However**, Fawx's approach has gaps:

### 3.3 Token Estimation

| Aspect | OpenClaw | Fawx |
|--------|----------|--------|
| Primary signal | `usage.totalTokens` from API response | `chars / 3` or `chars / 4` |
| Accuracy | Exact (from API) | Approximate (~25% error margin) |
| Used for | Compaction trigger, threshold check | Compaction trigger only |

**Gap — Critical for H2**: Without actual token usage tracking, Fawx can't:
- Know when it's approaching context window limits
- Make intelligent compaction decisions
- Report cost to users
- Implement per-step cost budgeting

The Anthropic API returns `usage` in every response. Fawx should store this in `ChatResponse` and `Message`.

### 3.4 Turn-Aware Trimming

| Aspect | OpenClaw | Fawx |
|--------|----------|--------|
| Turn boundaries | Tracked via session entries, respected by cut point detection | Not tracked — `toApiMessages()` trims at message index |
| Mid-turn split | Can split a long turn (prefix summarized, suffix kept) | Cannot — either keeps or drops entire messages |
| First user message | Included in summarization | Always preserved separately |

**Gap — deferred to H1.4**: The `toApiMessages()` method can cut in the middle of a tool use sequence:
```
[assistant: tool_use(tap)] [tool_result: "Opened Gmail"] [tool_result: "Screen:..."]
                                    ^--- trimmed here = broken API contract
```

If `maxMessages=20` cuts between an `assistant+tool_use` and its `tool_result`, the API will reject the request. This is a latent bug.

**Fix needed**: Either:
1. Turn-aware trimming (respect user→assistant→tool_result groupings)
2. Or discard the entire turn if any part would be trimmed

### 3.5 Pre-LLM Context Hook

| Aspect | OpenClaw | Fawx |
|--------|----------|--------|
| Hook | `transformContext: (AgentMessage[], signal) => AgentMessage[]` | None (see #483) |
| Runs | Before every LLM call (both first turn and continuations) | N/A |
| Used for | Context pruning, extension injection, custom filtering | N/A |

**Already tracked**: Issue #483 — add `transformContext` hook to AgentExecutor for H1.4.

### 3.6 Overflow Recovery

| Aspect | OpenClaw | Fawx |
|--------|----------|--------|
| Detection | Checks assistant message for context overflow indicators | No overflow detection |
| Recovery | Remove error message → compact → auto-retry | API error surfaces to user |
| Result | Transparent to user — session continues | User sees error, must retry manually |

**Gap — deferred**: For phone agents with ~25-step limit, overflow is unlikely but not impossible (large screen dumps, many steps). When it happens, auto-recovery would be better than error surfacing.

---

## 4. Gaps Found

### Critical (must address)

1. **`toApiMessages()` can break tool use sequences** (latent bug)
   - Trimming by message count can cut between `assistant+tool_use` and its `tool_result`
   - This will cause API errors when the trimmed conversation is sent
   - Fix: turn-aware trimming that respects message groups
   - **Severity**: Can cause API errors on long conversations

### Deferred (file as issues)

2. **No token usage tracking** (H2)
   - `ChatResponse` and `Message` don't store API usage data
   - Needed for: cost tracking, intelligent compaction, model tier decisions
   - The Anthropic API returns this in every response

3. **Message model not type-safe** (quality improvement)
   - Single `Message` class with nullable fields vs sealed class hierarchy
   - Not blocking anything, but makes the code harder to reason about

4. **No overflow detection/recovery** (H3)
   - When context overflow errors occur, they surface directly to the user
   - OpenClaw handles this transparently with compact + retry

### Intentional Divergences

5. **Rule-based compaction instead of LLM-powered**: Correct for phone agent (short tasks, screen dump bloat, cost sensitivity). The two-stage approach (regex strip → bracket format) is well-designed for this use case.

6. **No session branching**: Phone tasks are linear. No need for tree-structured sessions.

7. **No custom message types**: Phone agent has no bash history, compaction summaries, or branch summaries to persist.

8. **First message always preserved**: Good for phone agent — the original task description provides essential context throughout the entire tool loop. OpenClaw summarizes it away (coding tasks are different — the work product matters more than the original ask).

9. **Two-stage compaction pipeline**: `ContextCompactor` (cheap, regex) → `ContextManager` (rule-based formatting) is a good architecture. Stage 1 handles the 80% case (screen dump bloat) cheaply. Stage 2 provides further compression for long tasks.

---

## 5. Recommendations

### Immediate (next PR)

**Fix turn-aware trimming in `toApiMessages()` or `ContextCompactor`:**

```kotlin
fun toApiMessages(maxMessages: Int = 20): List<Map<String, String>> {
    val snapshot = messages.toList()
    if (snapshot.size <= maxMessages) return snapshot.map { ... }
    
    // Find safe trim point that doesn't break tool use sequences
    var trimIndex = (snapshot.size - maxMessages).coerceAtLeast(0)
    
    // Walk forward to find a safe boundary (start of a user message)
    while (trimIndex < snapshot.size) {
        val msg = snapshot[trimIndex]
        if (msg.role == "user" && msg.toolCallId == null) break
        trimIndex++
    }
    
    // If we couldn't find a safe boundary, fall back to keeping everything
    if (trimIndex >= snapshot.size) return snapshot.map { ... }
    
    return snapshot.subList(trimIndex, snapshot.size)
        .map { mapOf("role" to it.role, "content" to it.content) }
}
```

### H2

**Add token usage to ChatResponse and Message:**

```kotlin
data class TokenUsage(
    val inputTokens: Int,
    val outputTokens: Int,
    val cacheReadTokens: Int = 0,
    val cacheWriteTokens: Int = 0
) {
    val totalTokens: Int get() = inputTokens + outputTokens + cacheReadTokens + cacheWriteTokens
}

data class ChatResponse(
    val text: String?,
    val toolCalls: List<ToolCall>,
    val stopReason: String,
    val usage: TokenUsage? = null  // From API response
)
```

### H3

**Overflow recovery: detect context overflow, compact, retry.**

---

*Pressure test completed 2026-02-16*
*Reference: pi-agent-core `agent-loop.ts` (418 lines), pi-coding-agent `compaction/compaction.ts` (800+ lines), `agent-session.ts` (compaction trigger logic)*
*Fawx: `Message.kt` (~170 lines), `ContextCompactor.kt` (~65 lines), `ContextManager.kt` (~150 lines)*
