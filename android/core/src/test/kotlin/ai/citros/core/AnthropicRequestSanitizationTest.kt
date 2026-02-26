package ai.citros.core

import kotlinx.serialization.json.*
import org.junit.Assert.*
import org.junit.Test

/**
 * Regression tests for Anthropic-specific assistant content sanitization.
 *
 * Anthropic rejects assistant text that ends in trailing whitespace and also
 * rejects empty assistant content payloads.
 */
class AnthropicRequestSanitizationTest {

    private val client = AnthropicClient(
        config = ProviderConfig(
            provider = Provider.ANTHROPIC,
            baseUrl = "https://api.anthropic.com/v1/messages",
            chatModelId = "claude-sonnet-4-20250514",
            actionModelId = "claude-sonnet-4-20250514",
            headers = mapOf(
                "x-api-key" to "sk-ant-api03-test",
                "anthropic-version" to ProviderConfig.ANTHROPIC_API_VERSION,
                "anthropic-beta" to ProviderConfig.ANTHROPIC_PROMPT_CACHING_BETA
            )
        )
    )

    private val testTools = listOf(
        Tool("tap", "Tap an element", mapOf("type" to "object", "properties" to mapOf(
            "element_id" to mapOf("type" to "integer")
        )))
    )

    private fun extractMessages(request: JsonObject): JsonArray {
        return request["messages"]!!.jsonArray
    }

    private fun buildChatRequestForTest(conversation: Conversation): JsonObject {
        val method = AnthropicClient::class.java.getDeclaredMethod(
            "buildChatRequest",
            Conversation::class.java
        )
        method.isAccessible = true
        return method.invoke(client, conversation) as JsonObject
    }

    @Test
    fun `buildToolRequest trims trailing whitespace from assistant text messages`() {
        val messages = listOf(
            Message(role = "user", content = "hi"),
            Message(role = "assistant", content = "Done with trailing spaces   ")
        )

        val request = client.buildToolRequest(messages, "system", testTools, 4096)
        val apiMessages = extractMessages(request)

        val assistantContent = apiMessages[1].jsonObject["content"]!!.jsonPrimitive.content
        assertEquals("Done with trailing spaces", assistantContent)
    }

    @Test
    fun `buildToolRequest preserves leading assistant whitespace while trimming trailing whitespace`() {
        val messages = listOf(
            Message(role = "user", content = "format this"),
            Message(role = "assistant", content = "    indented response    ")
        )

        val request = client.buildToolRequest(messages, "system", testTools, 4096)
        val apiMessages = extractMessages(request)

        val assistantContent = apiMessages[1].jsonObject["content"]!!.jsonPrimitive.content
        assertEquals("    indented response", assistantContent)
    }

    @Test
    fun `buildToolRequest trims trailing whitespace from assistant text blocks`() {
        val messages = listOf(
            Message(role = "user", content = "tap"),
            Message.assistantWithTools("Let me do that...\n", listOf(
                ToolCall("t1", "tap", mapOf("element_id" to 7))
            )),
            Message.toolResult("t1", "Tapped")
        )

        val request = client.buildToolRequest(messages, "system", testTools, 4096)
        val apiMessages = extractMessages(request)

        val assistantBlocks = apiMessages[1].jsonObject["content"]!!.jsonArray
        val textBlock = assistantBlocks.first { it.jsonObject["type"]!!.jsonPrimitive.content == "text" }.jsonObject
        assertEquals("Let me do that...", textBlock["text"]!!.jsonPrimitive.content)
    }

    @Test
    fun `buildChatRequest trims trailing whitespace from assistant text messages`() {
        val conversation = Conversation(
            mutableListOf(
                Message(role = "user", content = "hello"),
                Message(role = "assistant", content = "Sure thing   ")
            )
        )

        val request = buildChatRequestForTest(conversation)
        val apiMessages = extractMessages(request)

        assertEquals(2, apiMessages.size)
        val assistantContent = apiMessages[1].jsonObject["content"]!!.jsonPrimitive.content
        assertEquals("Sure thing", assistantContent)
    }

    @Test
    fun `buildToolRequest drops whitespace-only assistant text messages`() {
        val messages = listOf(
            Message(role = "user", content = "first"),
            Message(role = "assistant", content = "   \n  \t"),
            Message(role = "user", content = "second")
        )

        val request = client.buildToolRequest(messages, "system", testTools, 4096)
        val apiMessages = extractMessages(request)

        assertEquals(2, apiMessages.size)
        assertEquals("user", apiMessages[0].jsonObject["role"]!!.jsonPrimitive.content)
        assertEquals("user", apiMessages[1].jsonObject["role"]!!.jsonPrimitive.content)
        assertTrue(apiMessages.none { msg ->
            msg.jsonObject["role"]!!.jsonPrimitive.content == "assistant"
        })
    }

    @Test
    fun `buildToolRequest drops assistant text blocks that are empty after trim but keeps tool use`() {
        val messages = listOf(
            Message(role = "user", content = "tap"),
            Message.assistantWithTools("   \n  ", listOf(
                ToolCall("t1", "tap", mapOf("element_id" to 7))
            )),
            Message.toolResult("t1", "Tapped")
        )

        val request = client.buildToolRequest(messages, "system", testTools, 4096)
        val apiMessages = extractMessages(request)

        val assistantBlocks = apiMessages[1].jsonObject["content"]!!.jsonArray
        assertEquals(1, assistantBlocks.size)
        assertEquals("tool_use", assistantBlocks[0].jsonObject["type"]!!.jsonPrimitive.content)
        assertTrue(assistantBlocks.none { it.jsonObject["type"]!!.jsonPrimitive.content == "text" })
    }

    @Test
    fun `buildToolRequest drops assistant content-block message when all blocks become empty`() {
        val messages = listOf(
            Message(role = "user", content = "first"),
            Message.assistantWithTools("  \n  ", emptyList()),
            Message(role = "user", content = "second")
        )

        val request = client.buildToolRequest(messages, "system", testTools, 4096)
        val apiMessages = extractMessages(request)

        assertEquals(2, apiMessages.size)
        assertEquals("user", apiMessages[0].jsonObject["role"]!!.jsonPrimitive.content)
        assertEquals("user", apiMessages[1].jsonObject["role"]!!.jsonPrimitive.content)
    }

    @Test
    fun `buildChatRequest drops whitespace-only assistant text messages`() {
        val conversation = Conversation(
            mutableListOf(
                Message(role = "user", content = "first"),
                Message(role = "assistant", content = "   \n   \t"),
                Message(role = "user", content = "second")
            )
        )

        val request = buildChatRequestForTest(conversation)
        val apiMessages = extractMessages(request)

        assertEquals(2, apiMessages.size)
        assertEquals("user", apiMessages[0].jsonObject["role"]!!.jsonPrimitive.content)
        assertEquals("first", apiMessages[0].jsonObject["content"]!!.jsonPrimitive.content)
        assertEquals("user", apiMessages[1].jsonObject["role"]!!.jsonPrimitive.content)
        assertEquals("second", apiMessages[1].jsonObject["content"]!!.jsonPrimitive.content)
    }
}
