package ai.citros.chat

import ai.citros.core.*

/**
 * Maps ChatViewModel state to OverlayState for the overlay UI.
 * 
 * Converts the message list and loading state into structured overlay data
 * including progress steps, transcript lines, and run state.
 */
object OverlayStateMapper {
    
    // Message prefix constants used for identifying message types and errors
    private const val TOOL_RESULT_PREFIX = "🤖 "
    private const val ERROR_EMOJI_PREFIX = "💥"
    private const val ERROR_TEXT_PREFIX = "Error:"
    
    /**
     * Convert ChatViewModel messages to OverlayState.
     * 
     * Behavior:
     * - **Only includes messages from the last user turn** - finds the last user message
     *   in the conversation history, then includes that message plus all messages after it.
     *   This ensures the overlay shows only the current task, not the entire conversation.
     * - Tool result messages (starting with TOOL_RESULT_PREFIX "🤖 ") become SYSTEM lines and steps
     * - User messages become USER lines
     * - Other assistant messages become SYSTEM lines (but don't count as steps)
     * - Run state is derived from isLoading + last message content
     * - Error detection: Both emoji ("💥") and text ("Error:") prefixes are checked for
     *   backward compatibility with different error formatting styles in the system
     * 
     * @param messages The message list from ChatViewModel
     * @param isLoading Whether the agent is currently executing
     * @return OverlayState for the overlay UI
     */
    fun mapToOverlayState(
        messages: List<Message>,
        isLoading: Boolean,
        actionPills: List<ActionPill> = emptyList()
    ): OverlayState {
        // Find the last user message index
        val lastUserIndex = messages.indexOfLast { it.role == "user" }
        
        // If no user message, return empty state
        if (lastUserIndex == -1) {
            return OverlayState.EMPTY
        }
        
        // Get messages from last user turn onwards
        val currentTurnMessages = messages.subList(lastUserIndex, messages.size)
        
        // Build lines from messages
        val lines = mutableListOf<OverlayLine>()
        var lineId = 1
        
        for (message in currentTurnMessages) {
            when (message.role) {
                "user" -> {
                    lines.add(OverlayLine(
                        id = lineId++,
                        type = OverlayLineType.USER,
                        text = message.content
                    ))
                }
                "assistant" -> {
                    // Tool results start with TOOL_RESULT_PREFIX
                    val text = if (message.content.startsWith(TOOL_RESULT_PREFIX)) {
                        message.content.removePrefix(TOOL_RESULT_PREFIX).trim()
                    } else {
                        message.content
                    }
                    
                    lines.add(OverlayLine(
                        id = lineId++,
                        type = OverlayLineType.SYSTEM,
                        text = text
                    ))
                }
            }
        }
        
        // Build steps from tool results (messages starting with TOOL_RESULT_PREFIX)
        val toolResults = currentTurnMessages.filter { 
            it.role == "assistant" && it.content.startsWith(TOOL_RESULT_PREFIX)
        }
        
        val steps = toolResults.mapIndexed { index, message ->
            val label = message.content.removePrefix(TOOL_RESULT_PREFIX).trim()
            OverlayStep(
                step = index + 1,
                total = toolResults.size,
                label = label
            )
        }
        
        // Determine run state
        val runState = determineRunState(
            messages = currentTurnMessages,
            isLoading = isLoading
        )
        
        // Determine current step index
        val currentStepIndex = when {
            isLoading -> (steps.size - 1).coerceAtLeast(0)
            runState == OverlayRunState.COMPLETED -> steps.size
            else -> 0
        }
        
        return OverlayState(
            runState = runState,
            steps = steps,
            lines = lines,
            currentStepIndex = currentStepIndex,
            totalSteps = steps.size,
            actionPills = actionPills
        )
    }
    
    /**
     * Determine the run state from messages and loading state.
     */
    private fun determineRunState(
        messages: List<Message>,
        isLoading: Boolean
    ): OverlayRunState {
        if (isLoading) {
            return OverlayRunState.EXECUTING
        }

        // No messages means idle (clean state, not an error)
        if (messages.isEmpty()) {
            return OverlayRunState.IDLE
        }

        // Check last message for error indicators
        val lastMessage = messages.lastOrNull()
        if (lastMessage != null && lastMessage.role == "assistant") {
            when {
                lastMessage.content.startsWith(ERROR_EMOJI_PREFIX) -> return OverlayRunState.FAILED
                lastMessage.content.startsWith(ERROR_TEXT_PREFIX) -> return OverlayRunState.FAILED
                // If last message is a tool result but not loading, task has stopped/paused
                lastMessage.content.startsWith(TOOL_RESULT_PREFIX) -> return OverlayRunState.STOPPED
                // Otherwise, if there are tool results before it, task is completed
                messages.any { it.role == "assistant" && it.content.startsWith(TOOL_RESULT_PREFIX) } -> 
                    return OverlayRunState.COMPLETED
            }
        }
        
        return OverlayRunState.IDLE
    }
}
