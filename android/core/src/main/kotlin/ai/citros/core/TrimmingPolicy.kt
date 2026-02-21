package ai.citros.core

/**
 * How to trim tool result content when it exceeds the keep-full threshold.
 */
enum class TrimMode {
    /**
     * Keep only the action summary (first line of tool result).
     * Appends [ContextCompactor.TRIM_MARKER].
     * Most aggressive; best for screen-heavy tool loops.
     */
    ACTION_SUMMARY,

    /**
     * Remove only the SCREEN section, keep everything else.
     * Only affects tool results that contain screen dumps
     * (delimited by `\n\nSCREEN:\n`).
     *
     * **Fallback behavior:** If a tool result has no SCREEN section and its
     * content exceeds the first line by more than [ContextCompactor.MIN_TRIM_SAVINGS]
     * characters, falls back to [ACTION_SUMMARY] (first line only). Short results
     * without a SCREEN section are kept as-is. This fallback ensures verbose
     * non-screen content is still trimmed, while short results remain untouched.
     *
     * **Important:** Category rules ([TrimmingPolicy.keepFullByCategory]) are
     * evaluated *before* the trim mode. If a tool's category has
     * `keepFull = Int.MAX_VALUE` (e.g., RESEARCH), it will never reach the
     * trim step regardless of this mode's fallback behavior.
     */
    STRIP_SCREEN_ONLY,
}

/**
 * Configurable policy for context trimming of tool result content.
 *
 * Different tool categories have different content shelf lives:
 * - **MECHANICAL** (taps, scrolls): screen content stale after 1-2 steps
 * - **PROMINENT** (open_app): may reference app-switch context for a few more steps
 * - **RESEARCH** (web_search, web_fetch): permanent value — the result IS the answer
 * - **REASONING** (think): ephemeral, useful only for recent reasoning chain
 * - **OTHER** (file ops, etc.): may be referenced later
 *
 * Example:
 * ```kotlin
 * val policy = TrimmingPolicy(
 *     minMessagesBeforeTrim = 10,
 *     keepFullByCategory = mapOf(
 *         OutputToolCategory.MECHANICAL to 1,       // Keep last tap/scroll
 *         OutputToolCategory.RESEARCH to Int.MAX_VALUE  // Never trim web results
 *     ),
 *     trimMode = TrimMode.STRIP_SCREEN_ONLY
 * )
 * val compactor = ContextCompactor(policy)
 * val trimmed = compactor.compact(messages)
 * ```
 *
 * @param enabled Master switch. When false, no trimming occurs.
 * @param minMessagesBeforeTrim Minimum conversation size before trimming activates.
 *   Prevents trimming in short conversations where everything fits easily.
 * @param keepFullByCategory Per-category count of recent tool results to keep with
 *   full content. Results beyond this count (from the end) get trimmed.
 *   [Int.MAX_VALUE] means never trim that category.
 * @param defaultKeepFull Fallback keep-full count for categories not in [keepFullByCategory].
 * @param trimMode How to trim content beyond the keep-full threshold.
 * @param maxTokenEstimate Estimated token budget. Trimming only triggers when the
 *   conversation exceeds this estimate (chars / 3). Set to [Int.MAX_VALUE] to
 *   always trim regardless of size (useful for testing).
 */
data class TrimmingPolicy(
    val enabled: Boolean = true,
    val minMessagesBeforeTrim: Int = 8,
    val keepFullByCategory: Map<OutputToolCategory, Int> = DEFAULT_KEEP_FULL,
    val defaultKeepFull: Int = 3,
    val trimMode: TrimMode = TrimMode.STRIP_SCREEN_ONLY,
    val maxTokenEstimate: Int = 60_000,
) {
    companion object {
        /** Conservative defaults: strip screen content, never touch research results. */
        val DEFAULT_KEEP_FULL: Map<OutputToolCategory, Int> = mapOf(
            OutputToolCategory.MECHANICAL to 2,
            OutputToolCategory.PROMINENT to 3,
            OutputToolCategory.RESEARCH to Int.MAX_VALUE,
            OutputToolCategory.REASONING to 1,
        )

        /** Disabled policy — no trimming at all. */
        val DISABLED = TrimmingPolicy(enabled = false)
    }

    /** Get the keep-full count for a given category. */
    fun keepFullFor(category: OutputToolCategory): Int =
        keepFullByCategory[category] ?: defaultKeepFull
}
