# H2 Context Trimming — Design Doc & Pressure Test

*Created 2026-02-16. Author: Clawdio. Updated after UX review with Joe.*

---

## Problem

Phone agent tool loops generate large amounts of ephemeral screen content. Each screen
dump is ~2000 chars (~500 tokens). A 10-step task carries ~5000 tokens of screen
descriptions where only the latest 1-2 are still valid — old element IDs don't exist
anymore, and stale screen descriptions actively confuse the model.

**This is NOT compaction.** Phone tasks are short and self-contained. Users don't need
deep context across tasks. What they need is:
- The model remembers what the user asked (user messages preserved)
- The model remembers what it already did and found (assistant messages preserved)
- The model sees the CURRENT screen accurately (latest tool results preserved)
- The model doesn't waste time/money processing stale screen dumps

**Key insight:** The model's assistant response is the "processed summary" of what it
saw. Screen content is the raw input. Once the model has responded, the raw screen
data served its purpose. We keep the summary, strip the raw data.

## Reference: OpenClaw

OpenClaw uses two stages:
1. **History limiting** (`limitHistoryTurns`) — keep last N user turns, cheap
2. **LLM compaction** (`session.compact()`) — summarize old messages via model call

We only need a phone-optimized variant of Stage 1. LLM compaction is overkill for
3-10 step phone tasks. Deferred to Stage 3 if ever needed.

## Design: Configurable Screen Content Garbage Collection

### Core Principle

**Trim tool result CONTENT, never delete tool result MESSAGES.**

Anthropic requires every `tool_use` block to have a matching `tool_result`. Deleting
a tool result without its corresponding tool_use breaks the API. So we always keep the
message structure intact — we just replace verbose content with a compact summary.

### Trimming Policy

```kotlin
/**
 * Configurable policy for trimming tool result content.
 *
 * Different tool categories have different trimming rules because their
 * content has different shelf lives:
 * - Screen content (MECHANICAL/PROMINENT): stale after 1-2 steps
 * - Research results (RESEARCH): permanent value — they ARE the answer
 * - Reasoning (REASONING): ephemeral, useful for last few steps
 * - File operations: may be referenced later
 */
data class TrimmingPolicy(
    /**
     * Minimum total messages in conversation before trimming activates.
     * Prevents trimming in short conversations where everything fits easily.
     * Default: 8 (roughly 2 tool loop iterations).
     */
    val minMessagesBeforeTrim: Int = 8,

    /**
     * Per-category rules for how many recent tool results keep full content.
     * Tool results beyond this count (from the end) get their content trimmed.
     * Int.MAX_VALUE = never trim this category.
     *
     * Categories not listed here use [defaultKeepFull].
     */
    val keepFullByCategory: Map<ToolCategory, Int> = mapOf(
        ToolCategory.MECHANICAL to 2,        // taps, scrolls, press_back/home
        ToolCategory.PROMINENT to 3,         // open_app — may reference app-switch context
        ToolCategory.RESEARCH to Int.MAX_VALUE,  // web_search, web_fetch — never trim
        ToolCategory.REASONING to 1,         // think() — keep only latest reasoning
    ),

    /**
     * Default keep-full count for tool categories not in [keepFullByCategory].
     * Applies to any future tool categories we haven't explicitly configured.
     */
    val defaultKeepFull: Int = 3,

    /**
     * What to keep from trimmed tool results.
     */
    val trimMode: TrimMode = TrimMode.ACTION_SUMMARY,

    /**
     * Whether trimming is enabled at all. Master switch for debugging.
     */
    val enabled: Boolean = true,
)

enum class TrimMode {
    /**
     * Keep only the action summary line (first line of tool result).
     * Append a marker: "[screen content trimmed]"
     * Most aggressive, best for screen-heavy tool loops.
     */
    ACTION_SUMMARY,

    /**
     * Keep the first [truncateChars] characters of the tool result.
     * Useful for tools where partial content might still be useful.
     */
    TRUNCATE,

    /**
     * Keep full content but strip the screen content section specifically.
     * Only affects tool results that contain "App: " screen dumps.
     * Preserves non-screen parts of the result intact.
     */
    STRIP_SCREEN_ONLY,
}
```

### ContextTrimmer Class

```kotlin
/**
 * Trims stale tool result content from conversation history.
 *
 * Designed for phone agent use cases where screen content is ephemeral —
 * old screen descriptions are actively harmful (stale element IDs).
 *
 * Safety guarantees:
 * - Never deletes messages (preserves Anthropic tool_use/tool_result pairing)
 * - Never modifies user messages or assistant messages
 * - Never trims the most recent N tool results per category
 * - Idempotent: already-trimmed messages are detected and skipped
 * - No-op for conversations below [TrimmingPolicy.minMessagesBeforeTrim]
 *
 * Thread safety: Operates on the message list in-place. Caller must ensure
 * exclusive access (AgentExecutor's tool loop is already serialized).
 */
class ContextTrimmer(
    private val policy: TrimmingPolicy = TrimmingPolicy(),
    private val classifier: OutputClassifier = OutputClassifier
) {
    companion object {
        /** Marker appended to trimmed tool results for detection. */
        internal const val TRIM_MARKER = "[screen content trimmed]"

        /** Detects screen content in tool results (our own format). */
        private val SCREEN_CONTENT_REGEX = Regex("""\n\nApp: .+""", RegexOption.DOT_MATCHES_ALL)
    }

    /**
     * Trim the conversation message list in-place.
     *
     * @param messages Mutable conversation message list
     */
    fun trim(messages: MutableList<Message>) {
        if (!policy.enabled) return
        if (messages.size < policy.minMessagesBeforeTrim) return

        // Count tool results per category from the END of the list.
        // Each category has its own counter — "keep last 2 mechanical"
        // is independent of "keep last 3 prominent".
        val categoryCounters = mutableMapOf<ToolCategory, Int>()

        // Iterate backward so we count from most recent
        for (i in messages.indices.reversed()) {
            val msg = messages[i]
            if (msg.role != Message.ROLE_TOOL) continue
            if (msg.content.contains(TRIM_MARKER)) continue  // Already trimmed

            val toolName = resolveToolName(msg)
            val category = classifier.categoryOf(toolName)
            val count = categoryCounters.getOrDefault(category, 0) + 1
            categoryCounters[category] = count

            val keepFull = policy.keepFullByCategory[category]
                ?: policy.defaultKeepFull

            if (count > keepFull) {
                messages[i] = trimMessage(msg)
            }
        }
    }

    private fun trimMessage(msg: Message): Message {
        val trimmed = when (policy.trimMode) {
            TrimMode.ACTION_SUMMARY -> {
                val firstLine = msg.content.lineSequence().firstOrNull() ?: ""
                "$firstLine\n$TRIM_MARKER"
            }
            TrimMode.TRUNCATE -> {
                // Keep first 200 chars + marker
                val truncated = msg.content.take(200)
                if (msg.content.length > 200) "$truncated…\n$TRIM_MARKER"
                else msg.content  // Short enough, don't trim
            }
            TrimMode.STRIP_SCREEN_ONLY -> {
                // Remove only the screen content block, keep everything else
                val stripped = msg.content.replace(SCREEN_CONTENT_REGEX, "")
                if (stripped != msg.content) "$stripped\n$TRIM_MARKER"
                else msg.content  // No screen content found, keep as-is
            }
        }

        return msg.copy(content = trimmed)
    }

    /**
     * Resolve the tool name from a tool result message.
     * Tool results don't store the tool name directly — we extract it
     * from the content's action summary line.
     */
    private fun resolveToolName(msg: Message): String {
        // Tool result content starts with action summary:
        // "Tapped element [5]" → tap
        // "Opened Gmail" → open_app
        // "Scrolled down" → scroll
        // "Results for \"weather\":" → web_search
        // etc.
        val firstLine = msg.content.lineSequence().firstOrNull()?.lowercase() ?: ""
        return when {
            firstLine.startsWith("tapped") -> "tap"
            firstLine.startsWith("opened") || firstLine.startsWith("launched") -> "open_app"
            firstLine.startsWith("scrolled") -> "scroll"
            firstLine.startsWith("pressed home") -> "press_home"
            firstLine.startsWith("pressed back") || firstLine.startsWith("went back") -> "press_back"
            firstLine.startsWith("typed") || firstLine.startsWith("entered") -> "type_text"
            firstLine.startsWith("results for") -> "web_search"
            firstLine.startsWith("content from") || firstLine.startsWith("fetched") -> "web_fetch"
            firstLine.startsWith("thought:") || firstLine.startsWith("i ") -> "think"
            firstLine.startsWith("file") || firstLine.startsWith("wrote") || firstLine.startsWith("read") -> "read_file"
            else -> "unknown"
        }
    }
}
```

### Edge Cases Handled

| Edge Case | How Handled |
|-----------|-------------|
| "What was on that screen?" | Model's assistant response already summarized it — preserved |
| Multi-app data carry | Model extracts data into its response before app switch |
| "Read my last 3 emails" | Each email summarized in assistant response before going back |
| Error recovery | Model should re-read current screen, not reference stale IDs |
| Screen content IS the answer | Latest N results always kept full (per category) |
| web_search/web_fetch results | RESEARCH category: `Int.MAX_VALUE` = never trimmed |
| Short conversations (< 8 msgs) | `minMessagesBeforeTrim` — no-op |
| Idempotency (double-trim) | `TRIM_MARKER` detected → skip already-trimmed messages |
| Anthropic tool_result pairing | Messages never deleted, only content replaced |
| File operations (read_file) | Separate category, configurable keepFull count |
| think() results | REASONING category: keep last 1 only |
| Post-steer irrelevance | Old screen content already stale; trimming helps naturally |
| Model switching | Policy is model-agnostic; could be per-tier in future |
| describeImage results | Falls to defaultKeepFull (3) — kept for recent context |
| Very long tool loops (25 steps) | Mechanical results beyond last 2 get trimmed = huge savings |
| Content with no screen dump | STRIP_SCREEN_ONLY mode leaves non-screen content intact |

### Delegate Interface Changes

```kotlin
interface ToolExecutionDelegate {
    // ... existing methods ...

    /**
     * Get a mutable reference to the conversation messages.
     * Used by ContextTrimmer via transformContext hook.
     */
    fun getMessages(): MutableList<Message>
}
```

### Wiring

```kotlin
// In ChatViewModel — configurable at construction time
val trimmingPolicy = TrimmingPolicy(
    // Defaults are conservative. Tune after real-world testing.
    minMessagesBeforeTrim = 8,
    keepFullByCategory = mapOf(
        ToolCategory.MECHANICAL to 2,
        ToolCategory.PROMINENT to 3,
        ToolCategory.RESEARCH to Int.MAX_VALUE,
        ToolCategory.REASONING to 1,
    ),
    trimMode = TrimMode.ACTION_SUMMARY,
)
val trimmer = ContextTrimmer(policy = trimmingPolicy)

val executor = AgentExecutor(
    delegate = phoneAgentDelegate,
    steerMessageSource = { drainSteerMessages() },
    transformContext = {
        trimmer.trim(phoneAgentDelegate.getMessages())
    }
)
```

### Future Flexibility Points

1. **Per-model-tier policies** — pass different `TrimmingPolicy` based on ModelClassifier tier
2. **Settings UI** — let user choose aggressiveness (off / conservative / aggressive)
3. **Dynamic adjustment** — if TokenUsage shows we're approaching context limit, auto-increase aggressiveness
4. **Per-tool overrides** — override category for specific tools (e.g., keep more open_app results)
5. **Token-budget stage** — add token counting to trigger more aggressive trimming when budget is tight
6. **Logging/metrics** — track how much content was trimmed per conversation for tuning

### What Changes

| File | Change | Risk |
|------|--------|------|
| `ContextTrimmer.kt` (new) | Core trimming logic + policy | Low — new file, no existing code modified |
| `TrimmingPolicy.kt` (new) | Configuration data classes | Low — pure data |
| `ToolExecutionDelegate` | Add `getMessages()` | Low — one new method |
| `ChatViewModel.kt` | Wire trimmer, construct policy | Low — additive |
| `OutputClassifier.kt` | Expose `categoryOf()` (already public) | None — no change needed |
| `Message.kt` | Maybe add `toolName` field for better resolution | Medium — schema change |

### What Doesn't Change

- `AgentExecutor.kt` — `transformContext` hook already exists
- `BaseProviderClient.kt` / `AnthropicClient.kt` — serialization unaffected
- `PhoneAgentPrompts.kt` — prompts unaffected
- API message format — trimming happens before serialization
- `ScreenReader.kt` — screen reading unaffected

### Test Plan

1. **No-op for short conversations** — < 8 messages → nothing trimmed
2. **No-op when disabled** — `enabled = false` → nothing trimmed
3. **Mechanical trimming** — 5 tap results, keepFull=2 → first 3 trimmed, last 2 full
4. **Research never trimmed** — web_search results always preserved regardless of count
5. **Reasoning trimmed to 1** — 3 think results → only last 1 full
6. **Idempotency** — trim same list twice → same result (no double-trimming)
7. **TRIM_MARKER detection** — already-trimmed messages skipped
8. **ACTION_SUMMARY mode** — keeps first line, appends marker
9. **STRIP_SCREEN_ONLY mode** — removes "App: ..." block, keeps rest
10. **TRUNCATE mode** — keeps first 200 chars
11. **Mixed categories** — mechanical + research in same conversation, each trimmed independently
12. **Tool name resolution** — "Tapped element" → tap, "Results for" → web_search, etc.
13. **Per-category counting is independent** — 5 taps + 5 web_searches, only taps trimmed
14. **defaultKeepFull** — unknown tool category uses default (3)
15. **Message structure preserved** — trimmed messages still have correct role, toolCallId, isError

---

## Pressure Test Summary

| Aspect | OpenClaw | Citros Design | Delta |
|--------|----------|---------------|-------|
| History limiting | Turn-counting from end | Category-aware result trimming from end | ✅ More granular |
| Tool result pruning | `stripToolResultDetails` (generic) | Screen-content-aware per-category | ✅ Phone-optimized |
| LLM compaction | Full model-call summarization | Deferred (not needed for phone tasks) | ⚠️ Intentional |
| Token estimation | `estimateTokens()` pre-hoc | `TokenUsage` post-hoc (for future Stage 3) | ⚠️ Different, adequate |
| Configurability | Config file (mode, thresholds) | `TrimmingPolicy` data class | ✅ Equivalent |
| Idempotency | N/A (runs once per compaction) | Marker-based detection | ✅ Required for per-call hook |
| Message safety | Sanitization pipeline | Never delete, only replace content | ✅ Same principle |
| Category awareness | No (treats all tool results same) | Per-tool-category rules | ✅ More precise |

**Verdict:** Our design is more granular than OpenClaw's (per-category rules vs. generic
stripping) and purpose-built for the phone agent use case. Conservative defaults with
full configurability. No critical gaps. LLM compaction intentionally deferred — phone
tasks don't need it.
