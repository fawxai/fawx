package ai.citros.core

/**
 * Detects when the agent loop is stuck — either because the screen hasn't changed
 * across multiple actions, or because consecutive wait calls aren't helping.
 *
 * Extracted from ChatViewModel to live in :core (no Android UI dependencies).
 *
 * Two detection modes:
 * 1. Screen hash repetition — all hashes in rolling window are identical
 * 2. Consecutive waits with no screen change — waiting isn't helping
 *
 * Screen-stuck warning takes precedence over wait warning (more actionable).
 */
class StuckDetector(
    /** Number of identical consecutive screen hashes that trigger a stuck warning. */
    private val screenThreshold: Int = DEFAULT_SCREEN_THRESHOLD,
    /** Number of consecutive wait calls with no screen change that trigger a warning. */
    private val waitThreshold: Int = DEFAULT_WAIT_THRESHOLD
) {
    companion object {
        const val DEFAULT_SCREEN_THRESHOLD = 3
        const val DEFAULT_WAIT_THRESHOLD = 2
    }

    /** Mutable state for stuck detection, reset per tool loop. */
    data class State(
        val recentScreenHashes: MutableList<Int> = mutableListOf(),
        var consecutiveWaits: Int = 0,
        var uniqueScreens: Int = 0
    )

    /**
     * Check for stuck conditions and return a warning to inject into the tool result,
     * or null if the agent is making progress.
     *
     * @param state Mutable detection state (lives for the duration of one tool loop)
     * @param toolName The name of the tool that just executed
     * @param screenHash Hash of the current screen content, or null if unavailable
     * @return Warning string to append to tool result, or null if not stuck
     */
    fun check(
        state: State,
        toolName: String,
        screenHash: Int?
    ): String? {
        var warning: String? = null

        // Track screen hashes
        if (screenHash != null) {
            val isNew = state.recentScreenHashes.lastOrNull() != screenHash
            if (isNew) state.uniqueScreens++
            state.recentScreenHashes.add(screenHash)
            if (state.recentScreenHashes.size > screenThreshold) {
                state.recentScreenHashes.removeAt(0)
            }
            if (state.recentScreenHashes.size >= screenThreshold &&
                state.recentScreenHashes.distinct().size == 1
            ) {
                warning = "\n\n⚠️ STUCK: The screen has not changed in $screenThreshold actions " +
                    "(only ${state.uniqueScreens} unique screen${if (state.uniqueScreens == 1) "" else "s"} seen). " +
                    "Try a different approach (scroll, tap a different element, press back) " +
                    "or tell the user what's blocking you."
            }
        }

        // Track consecutive waits. Screen-stuck warning takes precedence.
        if (toolName == "wait") {
            state.consecutiveWaits++
            val screenIsStuck = state.recentScreenHashes.size >= 2 &&
                state.recentScreenHashes.distinct().size == 1
            if (state.consecutiveWaits >= waitThreshold && screenIsStuck && warning == null) {
                warning = "\n\n⚠️ Waiting more won't help — the screen hasn't changed. " +
                    "Take a different action."
            }
        } else {
            state.consecutiveWaits = 0
        }

        return warning
    }
}
