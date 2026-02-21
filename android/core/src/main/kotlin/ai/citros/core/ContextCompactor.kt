package ai.citros.core

import android.util.Log

/**
 * First stage of two-stage context compaction: strips stale content from older tool results.
 *
 * **Two-stage compaction pipeline** (in [PhoneAgentApi]):
 * 1. **[ContextCompactor]** (this class) — category-aware pass that strips stale
 *    screen dumps and trims old tool results based on [TrimmingPolicy].
 * 2. **[ContextManager]** — step-threshold-based pass that summarizes remaining
 *    old messages into compact descriptions.
 *
 * **Category-aware trimming:**
 * Uses [OutputClassifier.categoryOf] to determine per-tool trimming rules.
 * Research results (web_search, web_fetch) are never trimmed because they
 * contain the answer. Mechanical results (taps, scrolls) are trimmed aggressively
 * because old screen element IDs are invalid after UI changes.
 *
 * **Idempotency:** Already-trimmed messages (containing [TRIM_MARKER]) are
 * detected and skipped. Safe to call multiple times on the same message list.
 *
 * **Safety:** Messages are never deleted — only content is replaced. This preserves
 * Anthropic's tool_use/tool_result pairing requirement.
 *
 * @param policy Configurable trimming rules. Defaults are conservative.
 */
class ContextCompactor(
    private val policy: TrimmingPolicy = TrimmingPolicy()
) {
    companion object {
        private const val TAG = "ContextCompactor"

        /** Marker appended to trimmed tool results for idempotency detection. */
        const val TRIM_MARKER = "[screen content trimmed]"

        /**
         * Minimum character savings to justify trimming a non-screen tool result.
         * If removing content would save fewer than this many characters, the
         * result is kept as-is. Prevents pointless trimming of short results.
         */
        private const val MIN_TRIM_SAVINGS = 50

        /** Delimiter for SCREEN sections appended by [PhoneAgentApi.formatToolResult]. */
        private const val SCREEN_DELIMITER = "\n\nSCREEN:\n"

        /**
         * Legacy static method for backward compatibility.
         * Uses default [TrimmingPolicy] with STRIP_SCREEN_ONLY mode.
         */
        fun compact(messages: List<Message>, maxTokenEstimate: Int = 60_000): List<Message> {
            return ContextCompactor(
                TrimmingPolicy(maxTokenEstimate = maxTokenEstimate)
            ).compact(messages)
        }
    }

    /**
     * Compact conversation messages by trimming stale tool result content.
     *
     * Returns a new list with old tool results trimmed per [policy].
     * The original list is not modified.
     *
     * @param messages Full conversation history
     * @return Compacted message list (may be the same list if under threshold)
     */
    fun compact(messages: List<Message>): List<Message> {
        if (!policy.enabled) return messages
        if (messages.size < policy.minMessagesBeforeTrim) return messages

        // Token estimate using char_count / 3. At ~3 chars/token this slightly
        // overestimates vs actual ~3.5-4 chars/token, ensuring we trigger
        // trimming before hitting the context window limit.
        val estimatedTokens = messages.sumOf { it.content.length } / 3
        if (estimatedTokens <= policy.maxTokenEstimate) return messages

        // Count tool results per category from the END of the list.
        // Each category has its own counter — "keep last 2 mechanical"
        // is independent of "keep last 3 prominent".
        val categoryCounters = mutableMapOf<OutputToolCategory, Int>()

        // First pass: count per-category from the end, mark which indices to trim
        val trimIndices = mutableSetOf<Int>()
        for (i in messages.indices.reversed()) {
            val msg = messages[i]
            if (msg.role != Message.ROLE_TOOL) continue
            if (msg.content.contains(TRIM_MARKER)) continue // Already trimmed

            val category = resolveCategory(msg)
            val count = categoryCounters.getOrDefault(category, 0) + 1
            categoryCounters[category] = count

            val keepFull = policy.keepFullFor(category)
            if (keepFull != Int.MAX_VALUE && count > keepFull) {
                trimIndices.add(i)
            }
        }

        if (trimIndices.isEmpty()) return messages

        // Second pass: create new list with trimmed messages
        val result = messages.mapIndexed { index, message ->
            if (index in trimIndices) {
                trimMessage(message)
            } else {
                message
            }
        }

        // Log compaction metrics for debugging
        val inputChars = messages.sumOf { it.content.length }
        val outputChars = result.sumOf { it.content.length }
        val tokensSaved = (inputChars - outputChars) / 3
        Log.d(TAG, "Compacted: ${trimIndices.size} messages trimmed, ~$tokensSaved tokens saved")

        return result
    }

    /**
     * Compact messages and return structured metrics alongside the result.
     *
     * @param messages Full conversation history
     * @return Pair of compacted message list and metrics (null if no compaction was needed)
     */
    fun compactWithMetrics(messages: List<Message>): Pair<List<Message>, CompactionMetrics?> {
        val result = compact(messages)
        if (result === messages) return Pair(messages, null)

        val inputChars = messages.sumOf { it.content.length }
        val outputChars = result.sumOf { it.content.length }
        val compactedCount = messages.indices.count { i ->
            i < result.size && messages[i].content != result[i].content
        }

        val metrics = CompactionMetrics(
            stage = "context_compactor",
            inputMessages = messages.size,
            outputMessages = result.size,
            messagesCompacted = compactedCount,
            estimatedTokensBefore = inputChars / 3,
            estimatedTokensAfter = outputChars / 3
        )
        Log.d(TAG, "CompactionMetrics: $metrics")
        return Pair(result, metrics)
    }

    /**
     * Resolve the tool category for a tool result message.
     *
     * Uses [Message.toolName] if available (set by [AgentExecutor]).
     * Falls back to content-based heuristic for messages created before
     * the toolName field was added.
     */
    internal fun resolveCategory(msg: Message): OutputToolCategory {
        // Prefer explicit tool name
        msg.toolName?.let { name ->
            return try {
                OutputClassifier.categoryOf(name)
            } catch (e: Exception) {
                Log.w(TAG, "categoryOf('$name') failed, falling back to OTHER", e)
                OutputToolCategory.OTHER
            }
        }

        // Fallback: infer from content
        val firstLine = msg.content.lineSequence().firstOrNull()?.lowercase() ?: ""
        val inferredName = when {
            firstLine.startsWith("tapped") -> "tap"
            firstLine.startsWith("long-pressed") || firstLine.startsWith("long pressed") -> "long_press"
            firstLine.startsWith("scrolled") -> "scroll"
            firstLine.startsWith("swiped") -> "swipe"
            firstLine.startsWith("typed") || firstLine.startsWith("entered") -> "type_text"
            firstLine.startsWith("opened") || firstLine.startsWith("launched") -> "open_app"
            firstLine.startsWith("pressed home") -> "press_home"
            firstLine.startsWith("pressed back") || firstLine.startsWith("went back") -> "press_back"
            firstLine.startsWith("results for") -> "web_search"
            firstLine.startsWith("content from") || firstLine.startsWith("fetched") -> "web_fetch"
            firstLine.startsWith("web automation") -> "web_browse"
            firstLine.startsWith("thought:") -> "think"
            firstLine.startsWith("screenshot") -> "screenshot"
            else -> "unknown"
        }
        if (inferredName == "unknown") {
            Log.w(TAG, "Could not infer tool category for message: ${msg.content.take(80)}")
        }
        return OutputClassifier.categoryOf(inferredName)
    }

    /**
     * Trim a tool result message based on the configured [TrimMode].
     */
    internal fun trimMessage(msg: Message): Message {
        val trimmed = when (policy.trimMode) {
            TrimMode.ACTION_SUMMARY -> {
                val firstLine = msg.content.lineSequence().firstOrNull() ?: ""
                "$firstLine\n$TRIM_MARKER"
            }
            TrimMode.STRIP_SCREEN_ONLY -> {
                val delimiterIndex = msg.content.indexOf(SCREEN_DELIMITER)
                if (delimiterIndex >= 0) {
                    // Found SCREEN section — strip it, keep everything before
                    val stripped = msg.content.substring(0, delimiterIndex)
                    "$stripped\n$TRIM_MARKER"
                } else {
                    // No SCREEN section found — fall back to ACTION_SUMMARY only
                    // if the content is long enough to justify trimming. This prevents
                    // accidentally trimming short non-screen results (e.g., web_search
                    // outputs that should be preserved via category rules). Results
                    // shorter than first line + MIN_TRIM_SAVINGS chars are kept as-is.
                    val firstLine = msg.content.lineSequence().firstOrNull() ?: ""
                    if (msg.content.length > firstLine.length + MIN_TRIM_SAVINGS) {
                        "$firstLine\n$TRIM_MARKER"
                    } else {
                        msg.content // Short enough, keep as-is
                    }
                }
            }
        }

        return msg.withContent(trimmed)
    }

    /**
     * Strip the SCREEN section from a tool result message.
     * Convenience method for use outside the full compaction pipeline.
     */
    fun stripScreenSection(message: Message): Message {
        val delimiterIndex = message.content.indexOf(SCREEN_DELIMITER)
        return if (delimiterIndex >= 0) {
            message.withContent(message.content.substring(0, delimiterIndex))
        } else {
            message
        }
    }
}
