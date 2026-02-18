package ai.citros.core

/**
 * Second stage of two-stage context compaction: summarizes old messages into compact descriptions.
 *
 * Runs after [ContextCompactor] has stripped SCREEN dumps from old tool results.
 * See [ContextCompactor] for the full two-stage pipeline documentation.
 *
 * Strategy:
 * - Keep the first user message (task description) always visible
 * - Keep the last [RECENT_WINDOW] messages (counted by actual messages, not assumed pairs)
 * - Summarize older screen states into compact descriptions
 * - Tool results older than the recent window get truncated
 *
 * Message sequence note: The action loop produces messages in this pattern:
 *   [user] → [assistant+tools] → [tool_result] → [tool_result] → [user] → ...
 * The sliding window counts actual messages, not assumed pairs, to handle
 * variable-length tool result sequences correctly.
 *
 * This allows complex 20-step tasks without overwhelming the action model's context.
 */
class ContextManager(
    /** Number of recent messages to keep in full (not pairs — actual messages). */
    private val recentWindow: Int = RECENT_MESSAGES,
    /** Step threshold before compaction kicks in. */
    private val compactionThreshold: Int = COMPACTION_THRESHOLD
) {
    companion object {
        /** Keep last 6 messages in full (roughly 2-3 tool execution rounds). */
        const val RECENT_MESSAGES = 6
        /** Start compacting after this many tool steps. */
        const val COMPACTION_THRESHOLD = 5
        /** Max characters for a compacted screen state. */
        const val COMPACTED_SCREEN_MAX_CHARS = 200
        /** Max characters for a compacted tool result. */
        const val COMPACTED_TOOL_RESULT_MAX_CHARS = 100
    }

    /**
     * Compact the message list for sending to the model.
     * Returns a new list with old screen states and tool results summarized.
     * The original list is not modified.
     *
     * @param messages The full conversation history
     * @param currentStep Current tool step (0-based)
     * @return Compacted message list suitable for model input
     */
    fun compact(messages: List<Message>, currentStep: Int): List<Message> {
        // Don't compact if below threshold or message list is small
        if (currentStep < compactionThreshold || messages.size <= recentWindow + 1) {
            return messages
        }

        val result = mutableListOf<Message>()
        
        // Always keep the first message (original user task)
        if (messages.isNotEmpty()) {
            result.add(messages[0])
        }

        // Calculate the cutoff: messages before this index get compacted
        // Uses actual message count, not assumed pairs
        val recentStartIndex = (messages.size - recentWindow).coerceAtLeast(1)

        // Compact old messages (index 1 to recentStartIndex)
        for (i in 1 until recentStartIndex) {
            val msg = messages[i]
            result.add(compactMessage(msg))
        }

        // Keep recent messages in full
        for (i in recentStartIndex until messages.size) {
            result.add(messages[i])
        }

        return result
    }

    /**
     * Compact messages and return structured metrics alongside the result.
     *
     * @param messages The full conversation history
     * @param currentStep Current tool step (0-based)
     * @return Pair of compacted message list and metrics (null if no compaction was needed)
     */
    fun compactWithMetrics(messages: List<Message>, currentStep: Int): Pair<List<Message>, CompactionMetrics?> {
        val result = compact(messages, currentStep)
        if (result === messages) return Pair(messages, null)

        val inputChars = messages.sumOf { it.content.length }
        val outputChars = result.sumOf { it.content.length }
        val compactedCount = messages.indices.count { i ->
            i < result.size && messages[i].content != result[i].content
        }

        val metrics = CompactionMetrics(
            stage = "context_manager",
            inputMessages = messages.size,
            outputMessages = result.size,
            messagesCompacted = compactedCount,
            estimatedTokensBefore = inputChars / 3,
            estimatedTokensAfter = outputChars / 3
        )
        return Pair(result, metrics)
    }

    /**
     * Compact a single message by summarizing screen content and truncating tool results.
     */
    internal fun compactMessage(message: Message): Message {
        val content = message.content ?: return message

        // Compact screen content in user messages
        if (message.role == "user" && content.contains("CURRENT SCREEN:")) {
            val compacted = compactScreenContent(content)
            return message.copy(content = compacted)
        }

        // Compact tool results
        if (message.role == "tool") {
            val compacted = compactToolResult(content)
            return message.copy(content = compacted)
        }

        return message
    }

    /**
     * Summarize screen content into a compact description.
     * Extracts: app name, number of elements, key interactive elements.
     */
    internal fun compactScreenContent(content: String): String {
        val lines = content.lines()
        
        // Find the screen section
        val screenStart = lines.indexOfFirst { it.startsWith("CURRENT SCREEN:") }
        if (screenStart == -1) return content

        // Extract app name
        val appLine = lines.getOrNull(screenStart + 1)
        val appName = if (appLine?.startsWith("App:") == true) appLine else "App: unknown"

        // Count elements and find key ones
        val elementLines = lines.drop(screenStart + 2).filter { it.startsWith("[") }
        val clickableCount = elementLines.count { it.contains("[click]") }
        val editableCount = elementLines.count { it.contains("[edit]") }

        // Extract the non-screen part (user message after screen dump)
        val screenEnd = lines.indexOfLast { it.startsWith("[") && it.contains("]") }
        val userPart = if (screenEnd >= 0 && screenEnd + 1 < lines.size) {
            lines.drop(screenEnd + 1).joinToString("\n").trim()
        } else {
            ""
        }

        val summary = buildString {
            append("[PREVIOUS SCREEN: $appName, ${elementLines.size} elements ($clickableCount clickable, $editableCount editable)]")
            if (userPart.isNotBlank()) {
                append("\n$userPart")
            }
        }

        return truncateAtWordBoundary(summary, COMPACTED_SCREEN_MAX_CHARS)
    }

    /**
     * Truncate a tool result to a compact summary.
     * All compacted results use bracket format for consistency:
     * `[Screen: ...]`, `[Thought: ...]`, `[Waited: ...]`, `[Action: ...]`
     */
    internal fun compactToolResult(content: String): String {
        // Screen refresh → bracket format
        if (content.startsWith("Screen refreshed:")) {
            val lines = content.lines()
            val appLine = lines.getOrNull(1) ?: ""
            val elementCount = lines.count { it.startsWith("[") }
            return "[Screen: $appLine, $elementCount elements]"
        }

        // Thought → bracket format, truncated
        if (content.startsWith("Thought:")) {
            val thought = content.removePrefix("Thought: ")
            return "[Thought: ${truncateAtWordBoundary(thought, COMPACTED_TOOL_RESULT_MAX_CHARS - 12)}]"
        }

        // Wait → bracket format
        if (content.startsWith("Waited")) {
            val lines = content.lines()
            val appLine = lines.find { it.startsWith("App:") } ?: ""
            val waitLine = lines.first()
            return if (appLine.isNotEmpty()) "[$waitLine, $appLine]" else "[$waitLine]"
        }

        // Other results: bracket format with truncation
        return if (content.length > COMPACTED_TOOL_RESULT_MAX_CHARS) {
            "[Action: ${truncateAtWordBoundary(content, COMPACTED_TOOL_RESULT_MAX_CHARS - 12)}]"
        } else {
            content
        }
    }

    /**
     * Truncate a string at a word boundary, appending "..." if truncated.
     * Avoids cutting words in half which could confuse the model.
     */
    internal fun truncateAtWordBoundary(text: String, maxLength: Int): String {
        if (text.length <= maxLength) return text
        
        // Find the last space before the limit
        val truncated = text.take(maxLength - 3)
        val lastSpace = truncated.lastIndexOf(' ')
        
        return if (lastSpace > maxLength / 2) {
            // Found a reasonable word boundary
            truncated.take(lastSpace) + "..."
        } else {
            // No good word boundary — just truncate (better than losing too much)
            truncated + "..."
        }
    }
}
