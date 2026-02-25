package ai.citros.core

/**
 * Observable state of the agent execution lifecycle.
 *
 * Emitted by AgentService and consumed by UI components (ChatActivity, OverlayService).
 * Lives in :core so both modules can reference it without circular dependencies.
 *
 * See docs/specs/sprint-0-service-architecture.md §2
 */
sealed class AgentState {
    /** No active task. Service may be running but idle. */
    data object Idle : AgentState()

    /** Processing a user message (initial LLM API call). */
    data class Thinking(val taskId: String) : AgentState()

    /**
     * Executing a tool call within the agentic loop.
     *
     * [totalSteps] is nullable because the agentic loop's total step count
     * isn't known upfront — it's determined dynamically. When null, the
     * notification shows indeterminate progress.
     */
    data class Executing(
        val taskId: String,
        val toolName: String,
        val stepIndex: Int,
        val totalSteps: Int? = null
    ) : AgentState()

    /**
     * Waiting for user input (policy confirmation, disambiguation, etc.).
     *
     * NB1: Spec (sprint-0-service-architecture.md §2) defines this as
     * `WaitingConfirmation`. Implementation uses `WaitingForInput` with
     * an `InputType` enum because the agent waits for more than just
     * policy confirmations — disambiguation, authentication, and free-text
     * input also pause the loop. This is a conscious generalization.
     */
    data class WaitingForInput(
        val taskId: String,
        val requestId: String,
        val reason: String,
        val inputType: InputType,
        val pills: List<ActionPill> = emptyList()
    ) : AgentState()

    /** Task completed successfully. */
    data class Complete(val taskId: String, val result: String) : AgentState()

    /** Task failed with an error. */
    data class Failed(val taskId: String, val error: String) : AgentState()

    /** Resuming from durable checkpoint after process restart. */
    data class Resuming(val taskId: String) : AgentState()

    /** Returns a human-readable label for UI status indicators. */
    fun statusLabel(): String = when (this) {
        is Idle -> "Ready"
        is Thinking -> "Thinking..."
        is Executing -> toolName
        is WaitingForInput -> reason
        is Complete -> "Done"
        is Failed -> "Error"
        is Resuming -> "Resuming..."
    }

    /** Whether this state represents an active (non-idle, non-terminal) task. */
    fun isActive(): Boolean = when (this) {
        is Idle, is Complete, is Failed -> false
        is Thinking, is Executing, is WaitingForInput, is Resuming -> true
    }
}

/**
 * Type of user input the agent is waiting for.
 * Determines which UI treatment is shown (pill buttons, free-text, biometric, etc.).
 *
 * See docs/specs/h2-action-policy-engine.md §8a (State-Dependent Pill Buttons)
 */
enum class InputType {
    /** Policy engine CONFIRM gate — user must approve or deny a tool call. */
    POLICY_CONFIRMATION,

    /** Agent is asking a clarifying question with discrete choices. */
    DISAMBIGUATION,

    /** Biometric/PIN authentication required before proceeding. */
    AUTHENTICATION,

    /** Generic free-text input needed from user. */
    FREE_TEXT
}
