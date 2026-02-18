package ai.citros.core

import kotlinx.serialization.json.*
import org.junit.Assert.*
import org.junit.Test

/**
 * Tests that [AnthropicClient.buildToolRequest] correctly merges consecutive
 * tool result messages into a single user message.
 *
 * Anthropic API requires:
 * 1. Alternating user/assistant roles (no consecutive same-role messages)
 * 2. All tool_results for a batch in ONE user message
 *
 * Bug #519: When the model returns multiple tool calls in one response,
 * each tool result was serialized as a separate role="user" message,
 * causing Anthropic to reject the payload.
 */
class AnthropicToolResultMergeTest {

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
        ))),
        Tool("screenshot", "Take a screenshot", mapOf("type" to "object", "properties" to emptyMap<String, Any>()))
    )

    private fun extractMessages(request: JsonObject): JsonArray {
        return request["messages"]!!.jsonArray
    }

    /**
     * Single tool call produces correct user/assistant alternation.
     */
    @Test
    fun `single tool result produces one user message`() {
        val messages = listOf(
            Message(role = "user", content = "tap the button"),
            Message.assistantWithTools("I'll tap it", listOf(
                ToolCall("tool1", "tap", mapOf("element_id" to 5))
            )),
            Message.toolResult("tool1", "Tapped element 5")
        )

        val request = client.buildToolRequest(messages, "system", testTools, 4096)
        val apiMessages = extractMessages(request)

        assertEquals(3, apiMessages.size)
        assertEquals("user", apiMessages[0].jsonObject["role"]!!.jsonPrimitive.content)
        assertEquals("assistant", apiMessages[1].jsonObject["role"]!!.jsonPrimitive.content)
        assertEquals("user", apiMessages[2].jsonObject["role"]!!.jsonPrimitive.content)

        // Verify tool_result content block
        val toolResultContent = apiMessages[2].jsonObject["content"]!!.jsonArray
        assertEquals(1, toolResultContent.size)
        assertEquals("tool_result", toolResultContent[0].jsonObject["type"]!!.jsonPrimitive.content)
        assertEquals("tool1", toolResultContent[0].jsonObject["tool_use_id"]!!.jsonPrimitive.content)
    }

    /**
     * Multiple tool calls in one batch: tool results MUST be merged into
     * a single user message. This is the bug #519 regression test.
     */
    @Test
    fun `multiple tool results are merged into single user message`() {
        val messages = listOf(
            Message(role = "user", content = "check my calendar"),
            Message.assistantWithTools("I'll tap and screenshot", listOf(
                ToolCall("tool1", "tap", mapOf("element_id" to 4)),
                ToolCall("tool2", "screenshot", emptyMap())
            )),
            Message.toolResult("tool1", "Tapped element 4"),
            Message.toolResult("tool2", "Screenshot captured but vision failed: timeout")
        )

        val request = client.buildToolRequest(messages, "system", testTools, 4096)
        val apiMessages = extractMessages(request)

        // Should be 3 messages: user, assistant, user (merged tool results)
        // NOT 4 messages with two consecutive user messages
        assertEquals(3, apiMessages.size)
        assertEquals("user", apiMessages[0].jsonObject["role"]!!.jsonPrimitive.content)
        assertEquals("assistant", apiMessages[1].jsonObject["role"]!!.jsonPrimitive.content)
        assertEquals("user", apiMessages[2].jsonObject["role"]!!.jsonPrimitive.content)

        // The merged user message should have BOTH tool_result blocks
        val toolResultContent = apiMessages[2].jsonObject["content"]!!.jsonArray
        assertEquals(2, toolResultContent.size)

        assertEquals("tool_result", toolResultContent[0].jsonObject["type"]!!.jsonPrimitive.content)
        assertEquals("tool1", toolResultContent[0].jsonObject["tool_use_id"]!!.jsonPrimitive.content)

        assertEquals("tool_result", toolResultContent[1].jsonObject["type"]!!.jsonPrimitive.content)
        assertEquals("tool2", toolResultContent[1].jsonObject["tool_use_id"]!!.jsonPrimitive.content)
    }

    /**
     * Three tool calls in one batch: all three results merged.
     */
    @Test
    fun `three tool results are merged into single user message`() {
        val messages = listOf(
            Message(role = "user", content = "do three things"),
            Message.assistantWithTools("On it", listOf(
                ToolCall("t1", "tap", mapOf("element_id" to 1)),
                ToolCall("t2", "tap", mapOf("element_id" to 2)),
                ToolCall("t3", "screenshot", emptyMap())
            )),
            Message.toolResult("t1", "Tapped 1"),
            Message.toolResult("t2", "Tapped 2"),
            Message.toolResult("t3", "Screenshot OK")
        )

        val request = client.buildToolRequest(messages, "system", testTools, 4096)
        val apiMessages = extractMessages(request)

        assertEquals(3, apiMessages.size)
        val toolResultContent = apiMessages[2].jsonObject["content"]!!.jsonArray
        assertEquals(3, toolResultContent.size)
        assertEquals("t1", toolResultContent[0].jsonObject["tool_use_id"]!!.jsonPrimitive.content)
        assertEquals("t2", toolResultContent[1].jsonObject["tool_use_id"]!!.jsonPrimitive.content)
        assertEquals("t3", toolResultContent[2].jsonObject["tool_use_id"]!!.jsonPrimitive.content)
    }

    /**
     * Multiple rounds of tool calls: each round's results are merged separately.
     */
    @Test
    fun `multi-round tool calls maintain correct alternation`() {
        val messages = listOf(
            // Round 1
            Message(role = "user", content = "open calendar"),
            Message.assistantWithTools("Opening", listOf(
                ToolCall("t1", "tap", mapOf("element_id" to 1)),
                ToolCall("t2", "tap", mapOf("element_id" to 2))
            )),
            Message.toolResult("t1", "Tapped 1"),
            Message.toolResult("t2", "Tapped 2"),
            // Round 2
            Message.assistantWithTools("Now screenshot", listOf(
                ToolCall("t3", "screenshot", emptyMap())
            )),
            Message.toolResult("t3", "Screenshot OK")
        )

        val request = client.buildToolRequest(messages, "system", testTools, 4096)
        val apiMessages = extractMessages(request)

        // user, assistant, user(merged t1+t2), assistant, user(t3)
        assertEquals(5, apiMessages.size)
        assertEquals("user", apiMessages[0].jsonObject["role"]!!.jsonPrimitive.content)
        assertEquals("assistant", apiMessages[1].jsonObject["role"]!!.jsonPrimitive.content)
        assertEquals("user", apiMessages[2].jsonObject["role"]!!.jsonPrimitive.content)
        assertEquals("assistant", apiMessages[3].jsonObject["role"]!!.jsonPrimitive.content)
        assertEquals("user", apiMessages[4].jsonObject["role"]!!.jsonPrimitive.content)

        // Round 1: 2 tool results merged
        val round1Content = apiMessages[2].jsonObject["content"]!!.jsonArray
        assertEquals(2, round1Content.size)

        // Round 2: 1 tool result
        val round2Content = apiMessages[4].jsonObject["content"]!!.jsonArray
        assertEquals(1, round2Content.size)
    }

    /**
     * Tool result with isError=true preserves the is_error field in merged output.
     */
    @Test
    fun `merged tool results preserve isError flag`() {
        val messages = listOf(
            Message(role = "user", content = "test"),
            Message.assistantWithTools("Testing", listOf(
                ToolCall("t1", "tap", mapOf("element_id" to 1)),
                ToolCall("t2", "screenshot", emptyMap())
            )),
            Message.toolResult("t1", "Tapped element 1", isError = false),
            Message.toolResult("t2", "Vision failed: timeout", isError = true)
        )

        val request = client.buildToolRequest(messages, "system", testTools, 4096)
        val apiMessages = extractMessages(request)
        val toolResultContent = apiMessages[2].jsonObject["content"]!!.jsonArray

        assertEquals(2, toolResultContent.size)

        // First result: no is_error
        assertFalse(toolResultContent[0].jsonObject.containsKey("is_error"))

        // Second result: is_error = true
        assertTrue(toolResultContent[1].jsonObject["is_error"]!!.jsonPrimitive.boolean)
    }

    /**
     * Verify no consecutive same-role messages in any scenario.
     */
    @Test
    fun `no consecutive same-role messages in output`() {
        val messages = listOf(
            Message(role = "user", content = "test"),
            Message.assistantWithTools("Round 1", listOf(
                ToolCall("t1", "tap", mapOf("element_id" to 1)),
                ToolCall("t2", "tap", mapOf("element_id" to 2)),
                ToolCall("t3", "screenshot", emptyMap())
            )),
            Message.toolResult("t1", "OK"),
            Message.toolResult("t2", "OK"),
            Message.toolResult("t3", "Failed", isError = true),
            Message.assistantWithTools("Round 2", listOf(
                ToolCall("t4", "tap", mapOf("element_id" to 3))
            )),
            Message.toolResult("t4", "OK")
        )

        val request = client.buildToolRequest(messages, "system", testTools, 4096)
        val apiMessages = extractMessages(request)

        // Check no consecutive same-role messages
        for (i in 1 until apiMessages.size) {
            val prevRole = apiMessages[i - 1].jsonObject["role"]!!.jsonPrimitive.content
            val currRole = apiMessages[i].jsonObject["role"]!!.jsonPrimitive.content
            assertNotEquals(
                "Consecutive same-role messages at index ${i-1} and $i (role=$currRole)",
                prevRole, currRole
            )
        }
    }

    /**
     * Interleaved user text message between tool rounds: each round's results
     * are merged independently, and the user text message stays separate.
     *
     * In practice, an assistant response always separates tool results from
     * the next user message (the agent processes results before the user can
     * send another message or steer).
     */
    @Test
    fun `interleaved user message between tool rounds keeps correct structure`() {
        val messages = listOf(
            // Round 1: user request → assistant calls 2 tools
            Message(role = "user", content = "open calendar"),
            Message.assistantWithTools("Opening", listOf(
                ToolCall("t1", "tap", mapOf("element_id" to 1)),
                ToolCall("t2", "screenshot", emptyMap())
            )),
            Message.toolResult("t1", "Tapped 1"),
            Message.toolResult("t2", "Screenshot OK"),
            // Assistant processes tool results and responds
            Message(role = "assistant", content = "Calendar is open. What would you like to do?"),
            // User sends a new message
            Message(role = "user", content = "actually check email instead"),
            // Round 2: assistant calls 2 more tools
            Message.assistantWithTools("Switching to email", listOf(
                ToolCall("t3", "tap", mapOf("element_id" to 5)),
                ToolCall("t4", "tap", mapOf("element_id" to 6))
            )),
            Message.toolResult("t3", "Tapped 5"),
            Message.toolResult("t4", "Tapped 6")
        )

        val request = client.buildToolRequest(messages, "system", testTools, 4096)
        val apiMessages = extractMessages(request)

        // user, assistant(tools), user(merged t1+t2), assistant(text), user(text), assistant(tools), user(merged t3+t4)
        assertEquals(7, apiMessages.size)
        assertEquals("user", apiMessages[0].jsonObject["role"]!!.jsonPrimitive.content)
        assertEquals("assistant", apiMessages[1].jsonObject["role"]!!.jsonPrimitive.content)
        assertEquals("user", apiMessages[2].jsonObject["role"]!!.jsonPrimitive.content)      // merged t1+t2
        assertEquals("assistant", apiMessages[3].jsonObject["role"]!!.jsonPrimitive.content)  // text response
        assertEquals("user", apiMessages[4].jsonObject["role"]!!.jsonPrimitive.content)      // new user msg
        assertEquals("assistant", apiMessages[5].jsonObject["role"]!!.jsonPrimitive.content)  // tools round 2
        assertEquals("user", apiMessages[6].jsonObject["role"]!!.jsonPrimitive.content)      // merged t3+t4

        // Round 1: 2 tool results merged
        val round1Content = apiMessages[2].jsonObject["content"]!!.jsonArray
        assertEquals(2, round1Content.size)

        // User text message is plain text
        val userText = apiMessages[4].jsonObject["content"]!!.jsonPrimitive.content
        assertEquals("actually check email instead", userText)

        // Round 2: 2 tool results merged
        val round2Content = apiMessages[6].jsonObject["content"]!!.jsonArray
        assertEquals(2, round2Content.size)

        // Verify no consecutive same-role messages
        for (i in 1 until apiMessages.size) {
            val prevRole = apiMessages[i - 1].jsonObject["role"]!!.jsonPrimitive.content
            val currRole = apiMessages[i].jsonObject["role"]!!.jsonPrimitive.content
            assertNotEquals(
                "Consecutive same-role messages at index \${i-1} and \$i",
                prevRole, currRole
            )
        }
    }

    /**
     * Tool result message with null contentBlocks (defensive edge case).
     * If a Message has role="tool" but toolCallId is null (malformed),
     * contentBlocks returns null and it falls through to the text branch.
     */
    @Test
    fun `tool message without contentBlocks falls through to text branch`() {
        // Construct a malformed tool message: role="tool" but no toolCallId
        // so contentBlocks getter returns null
        val messages = listOf(
            Message(role = "user", content = "test"),
            Message.assistantWithTools("Acting", listOf(
                ToolCall("t1", "tap", mapOf("element_id" to 1))
            )),
            // Normal tool result
            Message.toolResult("t1", "Tapped 1"),
            // Malformed: role="tool" but no toolCallId → contentBlocks=null
            Message(role = "tool", content = "orphaned result")
        )

        val request = client.buildToolRequest(messages, "system", testTools, 4096)
        val apiMessages = extractMessages(request)

        // The malformed message should fall through to text branch
        // and be serialized as a plain {role: "tool", content: "orphaned result"}
        // which Anthropic will reject — but that's the caller's bug, not ours.
        // The important thing is we don't crash.
        assertTrue(apiMessages.size >= 3)
    }
}
