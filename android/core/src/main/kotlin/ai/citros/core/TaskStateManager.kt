package ai.citros.core

/**
 * Persists task state so the agent can recover after process death.
 */
interface TaskStateManager {
    /** Save current task state (called after each tool step/checkpoint). */
    suspend fun checkpoint(state: TaskState)

    /**
     * Load pending task state, if any. Returns null when no resumable task exists.
     * Implementations should discard stale checkpoints older than [staleThresholdMs].
     */
    suspend fun loadPending(staleThresholdMs: Long = STALE_THRESHOLD_MS): TaskState?

    /** Clear persisted task state (on completion/cancel/discard). */
    suspend fun clear()

    companion object {
        const val STALE_THRESHOLD_MS = 30 * 60 * 1000L
    }
}

enum class TaskStatus {
    ACTIVE,
    INTERRUPTED,
    COMPLETED,
    FAILED
}

/**
 * Tool call payload persisted in checkpoints.
 * [inputJson] stores [ToolCall.input] as JSON because Map<String, Any> isn't directly serializable.
 */
data class SerializedToolCall(
    val id: String,
    val name: String,
    val inputJson: String
)

data class TaskState(
    val taskId: String,
    val userMessage: String,
    val conversationHistory: List<Message>,
    val currentStep: Int,
    val maxSteps: Int,
    val startedAtMs: Long,
    val lastCheckpointMs: Long,
    val pendingToolCalls: List<SerializedToolCall>,
    val status: TaskStatus,
    val subtaskInProgress: Boolean = false,
    val subtaskGoal: String? = null,
    val subtaskDepth: Int = 0
)
