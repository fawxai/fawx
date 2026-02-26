package ai.citros.chat

import ai.citros.core.ChatResponse
import ai.citros.core.ToolCall

internal object PolicyWiringTestFixtures {
    fun toolLoopHappyPathResponses(
        toolCall: ToolCall = ToolCall(id = "tool-1", name = "open_app", input = mapOf("app_name" to "Gmail")),
        finalText: String = "Done"
    ): ArrayDeque<ChatResponse> = ArrayDeque(
        listOf(
            ChatResponse(text = null, toolCalls = listOf(toolCall), stopReason = "tool_use"),
            ChatResponse(text = finalText, toolCalls = emptyList(), stopReason = "end_turn")
        )
    )
}
