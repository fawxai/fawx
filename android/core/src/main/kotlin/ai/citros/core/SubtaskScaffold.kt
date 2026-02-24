package ai.citros.core

/** Sprint 1 scaffold: subtask loop constants and result models. */
object SubtaskScaffold {
    const val MAX_DEPTH: Int = 3
    const val DEFAULT_MAX_STEPS: Int = 10
    const val DEFAULT_MAX_TIME_SECONDS: Int = 60
    const val MIN_MAX_TIME_SECONDS: Int = 10
    const val MAX_MAX_TIME_SECONDS: Int = 300
}

data class SubtaskRequest(
    val goal: String,
    val successCriteria: String,
    val maxSteps: Int = SubtaskScaffold.DEFAULT_MAX_STEPS,
    val maxTimeSeconds: Int = SubtaskScaffold.DEFAULT_MAX_TIME_SECONDS,
    val depth: Int = 0
) {
    init {
        require(maxTimeSeconds in SubtaskScaffold.MIN_MAX_TIME_SECONDS..SubtaskScaffold.MAX_MAX_TIME_SECONDS) {
            "maxTimeSeconds must be between ${SubtaskScaffold.MIN_MAX_TIME_SECONDS} and ${SubtaskScaffold.MAX_MAX_TIME_SECONDS}"
        }
    }
}

data class SubtaskResult(
    val status: SubtaskStatus,
    val result: String,
    val stepsUsed: Int,
    val summary: String
)

enum class SubtaskStatus {
    SUCCESS,
    FAILED,
    PARTIAL,
    CANCELLED,
    TIMEOUT
}

/**
 * Scaffold contract only (execution wiring lands in follow-up PRs).
 */
interface SubtaskExecutor {
    suspend fun executeSubtask(
        request: SubtaskRequest,
        isCancelled: () -> Boolean
    ): SubtaskResult
}
