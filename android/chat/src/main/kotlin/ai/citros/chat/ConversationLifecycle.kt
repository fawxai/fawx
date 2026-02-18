package ai.citros.chat

/**
 * Manages automatic conversation clearing based on idle timeout and daily reset.
 *
 * - **Idle timeout**: Clears conversation if the app resumes after a configurable
 *   period of inactivity (default 30 minutes). Setting to [TIMEOUT_NEVER] disables.
 * - **Daily reset**: Clears conversation on the first message of a new calendar day.
 *
 * Both mechanisms call [ChatViewModel.clearConversation], which preserves
 * on-device memory (SqliteMemoryProvider data survives clears).
 */
object ConversationLifecycle {

    /** Sentinel value: idle timeout disabled. */
    const val TIMEOUT_NEVER = -1L

    /** Default idle timeout in milliseconds (30 minutes). */
    const val DEFAULT_TIMEOUT_MS = 30L * 60 * 1000

    /** Available timeout options as (label, millis) pairs. */
    val TIMEOUT_OPTIONS: List<Pair<String, Long>> = listOf(
        "15 minutes" to 15L * 60 * 1000,
        "30 minutes" to 30L * 60 * 1000,
        "1 hour" to 60L * 60 * 1000,
        "Never" to TIMEOUT_NEVER
    )

    /**
     * Check whether the conversation should be cleared due to idle timeout.
     *
     * @param lastActivityMs timestamp of last user/agent activity (epoch ms), or 0 if none
     * @param nowMs current time (epoch ms)
     * @param timeoutMs configured timeout threshold (ms), or [TIMEOUT_NEVER] to disable
     * @return true if conversation should be cleared
     */
    fun shouldClearForIdleTimeout(lastActivityMs: Long, nowMs: Long, timeoutMs: Long): Boolean {
        if (timeoutMs == TIMEOUT_NEVER) return false
        if (lastActivityMs <= 0L) return false
        return (nowMs - lastActivityMs) >= timeoutMs
    }

    /**
     * Check whether the conversation should be cleared due to a new calendar day.
     *
     * @param lastDateString the date of the last conversation (ISO format "YYYY-MM-DD"), or null/blank if none
     * @param todayDateString today's date (ISO format "YYYY-MM-DD")
     * @return true if conversation should be cleared
     */
    fun shouldClearForNewDay(lastDateString: String?, todayDateString: String): Boolean {
        if (lastDateString.isNullOrBlank()) return false
        return lastDateString != todayDateString
    }

    /**
     * Format today's date as "YYYY-MM-DD" for storage.
     *
     * @param nowMs current time (epoch ms)
     * @return ISO date string
     */
    fun todayDateString(nowMs: Long = System.currentTimeMillis()): String {
        // Uses device's default timezone — date boundary shifts if user travels.
        // Acceptable trade-off: reset aligns with the user's perceived "new day".
        val instant = java.time.Instant.ofEpochMilli(nowMs)
        return instant.atZone(java.time.ZoneId.systemDefault()).toLocalDate().toString()
    }

    /**
     * Find the timeout label for a given timeout value.
     */
    fun labelForTimeout(timeoutMs: Long): String {
        return TIMEOUT_OPTIONS.firstOrNull { it.second == timeoutMs }?.first ?: "30 minutes"
    }
}
