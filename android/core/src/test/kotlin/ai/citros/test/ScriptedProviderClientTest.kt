package ai.citros.test

import ai.citros.core.ChatResponse
import ai.citros.core.Conversation
import ai.citros.core.Message
import ai.citros.core.Provider
import ai.citros.core.Tool
import ai.citros.core.ToolCall
import kotlinx.coroutines.test.runTest
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class ScriptedProviderClientTest {

    @Test
    fun `scripted provider returns chat and tool responses`() = runTest {
        val tool = Tool(name = "tap", description = "tap", inputSchema = mapOf("type" to "object"))
        val expectedToolResponse = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall(id = "tool-1", name = "tap", input = mapOf("element_id" to 12))),
            stopReason = "tool_use"
        )
        val client = ScriptedProviderClient(
            provider = Provider.OPENAI,
            chatResponses = ArrayDeque<String>(listOf("hello")),
            toolResponses = ArrayDeque<ChatResponse>(listOf(expectedToolResponse))
        )

        val chat = client.chat(Conversation(messages = mutableListOf(Message(role = "user", content = "hi")))).getOrThrow()
        val toolResponse = client.chatWithTools(
            messages = listOf(Message(role = "user", content = "tap it")),
            systemPrompt = "system",
            tools = listOf(tool),
            tokenLimit = 256
        ).getOrThrow()

        assertEquals("hello", chat)
        assertEquals("tool_use", toolResponse.stopReason)
        assertEquals(1, client.chatCalls)
        assertEquals(1, client.chatWithToolsCalls)
        assertEquals("system", client.lastSystemPrompt)
        assertTrue(client.lastTools?.any { it.name == "tap" } == true)
    }
}
