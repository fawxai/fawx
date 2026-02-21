package ai.citros.core

import kotlinx.serialization.json.*
import org.junit.Assert.*
import org.junit.Test

/**
 * Regression tests for #665: orphaned tool_result after long tool loops.
 *
 * Validates that:
 * 1. Message.withContent() nulls _contentBlocks (forces reconstruction)
 * 2. Compaction doesn't create orphaned tool_results
 * 3. Safety net in buildToolRequest catches any remaining orphans
 * 4. Long tool loop + user follow-up produces valid API messages
 */
class OrphanedToolResultTest {

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
        Tool("tap", "Tap", mapOf("type" to "object", "properties" to mapOf(
            "element_id" to mapOf("type" to "integer")
        )))
    )

    private fun extractMessages(request: JsonObject): JsonArray =
        request["messages"]!!.jsonArray

    // ==================== Message.withContent() ====================

    @Test
    fun `withContent nulls contentBlocks for tool messages`() {
        val original = Message.toolResult("toolu_1", "Tapped element 5\n\nSCREEN:\nApp: Settings")
        assertNotNull("original should have contentBlocks", original.contentBlocks)

        val compacted = original.withContent("Tapped element 5")
        // contentBlocks should reconstruct from new content
        val blocks = compacted.contentBlocks!!
        assertEquals(1, blocks.size)
        assertEquals("tool_result", blocks[0]["type"])
        assertEquals("toolu_1", blocks[0]["tool_use_id"])
        assertEquals("Tapped element 5", blocks[0]["content"]) // new content, not original
    }

    @Test
    fun `withContent preserves toolCallId and toolName`() {
        val original = Message.toolResult("toolu_1", "long content", toolName = "tap", isError = true)
        val compacted = original.withContent("short")
        assertEquals("toolu_1", compacted.toolCallId)
        assertEquals("tap", compacted.toolName)
        assertTrue(compacted.isError)
        assertEquals("short", compacted.content)
    }

    @Test
    fun `copy preserves stale contentBlocks but withContent does not`() {
        val original = Message.toolResult("toolu_1", "Original long content with SCREEN data")
        val originalBlocks = original.contentBlocks!!

        // copy() preserves _contentBlocks (the bug)
        val copied = original.copy(content = "Trimmed")
        val copiedBlocks = copied.contentBlocks!!
        assertEquals("Original long content with SCREEN data", copiedBlocks[0]["content"])

        // withContent() forces reconstruction from new content
        val withNew = original.withContent("Trimmed")
        val newBlocks = withNew.contentBlocks!!
        assertEquals("Trimmed", newBlocks[0]["content"])
    }

    // ==================== sanitizeMessages() ====================

    @Test
    fun `sanitizeMessages passes valid message sequence unchanged`() {
        val messages = listOf(
            Message(role = "user", content = "hello"),
            Message.assistantWithTools("tapping", listOf(ToolCall("t1", "tap", mapOf("element_id" to 1)))),
            Message.toolResult("t1", "Tapped")
        )
        val result = client.sanitizeMessages(messages)
        assertEquals(3, result.size)
        assertEquals("tool", result[2].role)
    }

    @Test
    fun `sanitizeMessages converts leading tool message to text`() {
        val messages = listOf(
            Message.toolResult("orphan", "Some result"),
            Message(role = "user", content = "hello")
        )
        val result = client.sanitizeMessages(messages)
        assertEquals(1, result.size)
        // Leading tool converted to user text and merged with following user message
        assertEquals("user", result[0].role)
        assertTrue(result[0].content.contains("tool result"))
    }

    @Test
    fun `sanitizeMessages converts orphaned tool_result to text`() {
        val messages = listOf(
            Message(role = "user", content = "hello"),
            Message.assistantWithTools("tapping", listOf(ToolCall("t1", "tap", mapOf("element_id" to 1)))),
            Message.toolResult("t1", "Tapped"),
            // This tool_result references t1 but the preceding assistant is gone after compaction
            Message(role = "assistant", content = "Done"),
            Message.toolResult("orphan_id", "Orphaned result")
        )
        val result = client.sanitizeMessages(messages)
        // The orphaned tool_result should be converted to user text
        val orphanConverted = result.find { it.content.contains("Orphaned result") }
        assertNotNull("orphaned result should be preserved as text", orphanConverted)
        assertEquals("user", orphanConverted!!.role)
    }

    @Test
    fun `sanitizeMessages truncates converted orphaned tool_result content to 200 chars`() {
        val longContent = "x".repeat(260)
        val messages = listOf(
            Message(role = "user", content = "hello"),
            Message(role = "assistant", content = "Done"),
            Message.toolResult("orphan_id", longContent)
        )

        val result = client.sanitizeMessages(messages)
        val converted = result.last()
        assertEquals("user", converted.role)
        assertTrue(converted.content.startsWith("[tool result] "))
        val preserved = converted.content.removePrefix("[tool result] ")
        assertEquals(200, preserved.length)
        assertEquals("x".repeat(200), preserved)
    }

    @Test
    fun `sanitizeMessages converts tool message with null toolCallId to text`() {
        val messages = listOf(
            Message(role = "user", content = "hello"),
            Message.assistantWithTools("tapping", listOf(ToolCall("t1", "tap", mapOf("element_id" to 1)))),
            Message(role = Message.ROLE_TOOL, content = "Result missing tool id")
        )

        val result = client.sanitizeMessages(messages)
        val converted = result.last()
        assertEquals(Message.ROLE_USER, converted.role)
        assertTrue(converted.content.contains("Result missing tool id"))
        assertNull(converted.contentBlocks)
    }

    // ==================== Long tool loop + follow-up ====================

    @Test
    fun `long tool loop with follow-up produces valid API messages`() {
        val messages = buildLongToolLoop(30)
        // Add user follow-up (simulating what happens after end_turn)
        val withFollowUp = messages + Message(role = "user", content = "Now do something else")

        val request = client.buildToolRequest(withFollowUp, "system", testTools, 4096)
        val apiMessages = extractMessages(request)

        // Validate: no consecutive same-role, every tool_result has matching tool_use
        assertNoOrphanedToolResults(apiMessages)
        assertAlternatingRoles(apiMessages)
        assertEquals("user", apiMessages[0].jsonObject["role"]!!.jsonPrimitive.content)
    }

    @Test
    fun `compacted long loop preserves tool pairing`() {
        val messages = buildLongToolLoop(30)
        val compactor = ContextCompactor()
        val manager = ContextManager()

        val stage1 = compactor.compact(messages)
        val stage2 = manager.compact(stage1, currentStep = 30)

        // After compaction, build API request
        val request = client.buildToolRequest(stage2, "system", testTools, 4096)
        val apiMessages = extractMessages(request)

        assertNoOrphanedToolResults(apiMessages)
        assertAlternatingRoles(apiMessages)
    }

    @Test
    fun `compaction with withContent produces correct contentBlocks`() {
        val messages = buildLongToolLoop(10)
        val manager = ContextManager()
        val compacted = manager.compact(messages, currentStep = 10)

        // Check that compacted tool messages have contentBlocks reflecting new content
        for (msg in compacted) {
            if (msg.role == "tool" && msg.contentBlocks != null) {
                val blocks = msg.contentBlocks!!
                for (block in blocks) {
                    if (block["type"] == "tool_result") {
                        // The content in the block should match the message's content
                        assertEquals(
                            "tool_result content should match message content",
                            msg.content,
                            block["content"]
                        )
                    }
                }
            }
        }
    }

    @Test
    fun `sanitizeMessages handles empty list`() {
        assertEquals(emptyList<Message>(), client.sanitizeMessages(emptyList()))
    }

    @Test
    fun `sanitizeMessages handles all-tool messages`() {
        val messages = listOf(
            Message.toolResult("t1", "Result 1"),
            Message.toolResult("t2", "Result 2")
        )
        val result = client.sanitizeMessages(messages)
        // All converted to user text
        assertTrue(result.all { it.role == "user" })
        assertTrue(result.none { it.contentBlocks != null })
    }

    // ==================== Helpers ====================

    /**
     * Build a realistic long tool loop: user → (assistant+tools → tool_result) × N → assistant(end_turn)
     */
    private fun buildLongToolLoop(steps: Int): List<Message> {
        val messages = mutableListOf<Message>()
        messages.add(Message(role = "user", content = "CURRENT SCREEN:\nApp: launcher\n[1] Settings [click]\n\nOpen settings and configure WiFi"))

        for (i in 1..steps) {
            messages.add(Message.assistantWithTools(
                "Step $i",
                listOf(ToolCall("toolu_$i", "tap", mapOf("element_id" to i)))
            ))
            messages.add(Message.toolResult(
                "toolu_$i",
                "Tapped element $i\n\nSCREEN:\nApp: settings\n[$i] Item $i [click]\n[${i+1}] Next [click]",
                toolName = "tap"
            ))
        }

        // End turn
        messages.add(Message(role = "assistant", content = "Done! WiFi is configured."))
        return messages
    }

    private fun assertNoOrphanedToolResults(apiMessages: JsonArray) {
        var lastAssistantToolIds = emptySet<String>()
        for (i in 0 until apiMessages.size) {
            val msg = apiMessages[i].jsonObject
            val role = msg["role"]!!.jsonPrimitive.content
            val content = msg["content"]

            if (role == "assistant" && content is JsonArray) {
                lastAssistantToolIds = content.mapNotNull { block ->
                    val obj = block.jsonObject
                    if (obj["type"]?.jsonPrimitive?.content == "tool_use") {
                        obj["id"]?.jsonPrimitive?.content
                    } else null
                }.toSet()
            } else if (role == "user" && content is JsonArray) {
                // Check tool_result blocks
                for (block in content) {
                    val obj = block.jsonObject
                    if (obj["type"]?.jsonPrimitive?.content == "tool_result") {
                        val toolUseId = obj["tool_use_id"]!!.jsonPrimitive.content
                        assertTrue(
                            "Orphaned tool_result at API msg[$i]: tool_use_id=$toolUseId not in preceding assistant's tool_use ids=$lastAssistantToolIds",
                            toolUseId in lastAssistantToolIds
                        )
                    }
                }
            } else {
                if (role != "assistant") lastAssistantToolIds = emptySet()
            }
        }
    }

    private fun assertAlternatingRoles(apiMessages: JsonArray) {
        for (i in 1 until apiMessages.size) {
            val prev = apiMessages[i - 1].jsonObject["role"]!!.jsonPrimitive.content
            val curr = apiMessages[i].jsonObject["role"]!!.jsonPrimitive.content
            assertNotEquals(
                "Consecutive same-role at API msg[${i-1}] and [$i]: role=$curr",
                prev, curr
            )
        }
    }
}
