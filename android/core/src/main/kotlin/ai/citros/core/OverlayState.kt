package ai.citros.core

/**
 * Overlay UI state - bridges ChatViewModel state to overlay UI.
 * 
 * This state is derived from ChatViewModel's messages and isLoading state,
 * extracting tool execution progress and conversation lines.
 */
data class OverlayState(
    val runState: OverlayRunState,
    val steps: List<OverlayStep>,
    val lines: List<OverlayLine>,
    val currentStepIndex: Int,
    val totalSteps: Int
) {
    companion object {
        /**
         * Empty state for initial render or when no conversation is active.
         */
        val EMPTY = OverlayState(
            runState = OverlayRunState.IDLE,
            steps = emptyList(),
            lines = emptyList(),
            currentStepIndex = 0,
            totalSteps = 0
        )
    }
}

/**
 * Run state of the overlay - derived from ChatViewModel.isLoading + last message status.
 */
enum class OverlayRunState {
    /** No task running - clean/welcome state (neutral, not an error) */
    IDLE,

    /** Agent is actively executing tool calls */
    EXECUTING,
    
    /** Task completed successfully (last response had no tool calls) */
    COMPLETED,
    
    /** Task failed with error */
    FAILED,
    
    /** Task was explicitly stopped by the user */
    STOPPED
}

/**
 * A single step in the overlay progress indicator.
 * 
 * Derived from tool calls in the conversation.
 */
data class OverlayStep(
    val step: Int,
    val total: Int,
    val label: String
)

/**
 * A line in the overlay transcript.
 * 
 * USER lines are user messages.
 * SYSTEM lines are tool execution results (prefixed with "🤖 ").
 * QUEUED lines are pending user messages (not yet implemented in mapper).
 */
data class OverlayLine(
    val id: Int,
    val type: OverlayLineType,
    val text: String
)

/**
 * Type of overlay line.
 */
enum class OverlayLineType {
    USER,
    SYSTEM,
    QUEUED
}
