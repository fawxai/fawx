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

    @Test
    fun `sanitizeMessages converts incomplete assistant tool_use batch to plain assistant text`() {
        val messages = listOf(
            Message(role = Message.ROLE_USER, content = "Open settings"),
            Message.assistantWithTools(
                text = "Tapping settings",
                toolCalls = listOf(ToolCall("t1", "tap", mapOf("element_id" to 1)))
            ),
            Message(role = Message.ROLE_USER, content = "Actually stop")
        )

        val result = client.sanitizeMessages(messages)

        assertEquals(Message.ROLE_ASSISTANT, result[1].role)
        assertTrue(result[1].content.contains("Tapping settings"))
        assertFalse(result[1].content.contains("[Tools:"))
        assertNull(result[1].contentBlocks)
    }

    @Test
    fun `buildToolRequest removes unmatched tool_use ids from interrupted batches`() {
        val messages = listOf(
            Message(role = Message.ROLE_USER, content = "Open settings"),
            Message.assistantWithTools(
                text = "Executing tools",
                toolCalls = listOf(
                    ToolCall("t1", "tap", mapOf("element_id" to 1)),
                    ToolCall("t2", "tap", mapOf("element_id" to 2))
                )
            ),
            // Partial completion only: t1 has a result, t2 never did.
            Message.toolResult("t1", "Tapped first item", toolName = "tap"),
            Message(role = Message.ROLE_USER, content = "continue")
        )

        val request = client.buildToolRequest(messages, "system", testTools, 4096)
        val apiMessages = extractMessages(request)

        assertNoOrphanedToolResults(apiMessages)
        assertNoUnmatchedToolUses(apiMessages)

        val assistantToolBatch = apiMessages[1].jsonObject["content"]!!.jsonArray
        val toolUseIds = assistantToolBatch
            .filter { it.jsonObject["type"]!!.jsonPrimitive.content == "tool_use" }
            .map { it.jsonObject["id"]!!.jsonPrimitive.content }
        assertEquals(listOf("t1"), toolUseIds)
    }

    @Test
    fun `sanitizeMessages preserves matched tool_result content in partial completion`() {
        val messages = listOf(
            Message(role = Message.ROLE_USER, content = "Open settings"),
            Message.assistantWithTools(
                text = "Executing tools",
                toolCalls = listOf(
                    ToolCall("t1", "tap", mapOf("element_id" to 1)),
                    ToolCall("t2", "tap", mapOf("element_id" to 2))
                )
            ),
            Message.toolResult("t1", "Tapped first item", toolName = "tap"),
            Message(role = Message.ROLE_USER, content = "continue")
        )

        val sanitized = client.sanitizeMessages(messages)
        val preservedToolResult = sanitized.firstOrNull { it.role == Message.ROLE_TOOL && it.toolCallId == "t1" }

        assertNotNull("matched tool_result should be preserved through sanitize path", preservedToolResult)
        assertEquals("Tapped first item", preservedToolResult!!.content)

        val sanitizedAssistant = sanitized[1]
        assertEquals(Message.ROLE_ASSISTANT, sanitizedAssistant.role)
        assertNotNull("partial completion keeps matched tool_use blocks", sanitizedAssistant.contentBlocks)
        val toolUseIds = sanitizedAssistant.contentBlocks!!
            .filter { it["type"] == "tool_use" }
            .mapNotNull { it["id"] as? String }
        assertEquals(listOf("t1"), toolUseIds)
    }

    @Test
    fun `sanitizeMessages fallback extraction keeps assistant text when no tools marker`() {
        val staleToolJson = """
            [
              {"id":"t1","name":"tap","input":{"element_id":1}},
              {"id":"t2","name":"tap","input":{"element_id":2}}
            ]
        """.trimIndent()
        val assistantWithStaleToolJson = Message(
            role = Message.ROLE_ASSISTANT,
            content = "Retrying from interrupted state",
            toolCallsJson = staleToolJson
        )

        val messages = listOf(
            Message(role = Message.ROLE_USER, content = "open settings"),
            assistantWithStaleToolJson,
            Message.toolResult("t1", "Tapped first item", toolName = "tap"),
            Message(role = Message.ROLE_USER, content = "continue")
        )

        val sanitized = client.sanitizeMessages(messages)
        val sanitizedAssistant = sanitized[1]

        assertEquals("Retrying from interrupted state", sanitizedAssistant.content.substringBefore(Message.TOOL_CALLS_MARKER))
        assertNotNull("matched tool_use should be reconstructed from fallback text path", sanitizedAssistant.contentBlocks)

        val toolUseIds = sanitizedAssistant.contentBlocks!!
            .filter { it["type"] == "tool_use" }
            .mapNotNull { it["id"] as? String }
        assertEquals(listOf("t1"), toolUseIds)
    }

    @Test
    fun `sanitizeMessages degraded assistant cannot resurrect stale tool metadata`() {
        val staleToolJson = "[{\"id\":\"t1\",\"name\":\"tap\",\"input\":{\"element_id\":1}}]"
        val messages = listOf(
            Message(role = Message.ROLE_USER, content = "open settings"),
            Message(role = Message.ROLE_ASSISTANT, content = "Retrying", toolCallsJson = staleToolJson),
            Message(role = Message.ROLE_USER, content = "stop")
        )

        val sanitized = client.sanitizeMessages(messages)
        val degradedAssistant = sanitized[1]

        assertNull("degraded assistant must not keep stale toolCallsJson", degradedAssistant.toolCallsJson)
        assertNull("degraded assistant must not keep stale contentBlocks", degradedAssistant.contentBlocks)
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
        assertNoUnmatchedToolUses(apiMessages)
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
        assertNoUnmatchedToolUses(apiMessages)
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

    /**
     * Verifies every assistant tool_use batch is fully satisfied by immediately
     * following role="user" messages that contain tool_result blocks.
     *
     * Role expectation: once a non-tool-result user message or any non-user role
     * is encountered, the scan stops for that batch (adjacent batch semantics).
     */
    private fun assertNoUnmatchedToolUses(apiMessages: JsonArray) {
        for (i in 0 until apiMessages.size) {
            val assistantMsg = apiMessages[i].jsonObject
            val role = assistantMsg["role"]!!.jsonPrimitive.content
            val content = assistantMsg["content"]
            if (role != Message.ROLE_ASSISTANT || content !is JsonArray) continue

            val expected = content.mapNotNull { block ->
                val obj = block.jsonObject
                if (obj["type"]?.jsonPrimitive?.content == "tool_use") {
                    obj["id"]?.jsonPrimitive?.content
                } else {
                    null
                }
            }.toSet()

            if (expected.isEmpty()) continue

            val observed = mutableSetOf<String>()
            var j = i + 1
            while (j < apiMessages.size) {
                val nextMsg = apiMessages[j].jsonObject
                val nextRole = nextMsg["role"]!!.jsonPrimitive.content
                if (nextRole != Message.ROLE_USER) break

                val nextContent = nextMsg["content"]
                if (nextContent !is JsonArray) {
                    j++
                    continue
                }

                var sawToolResult = false
                for (block in nextContent) {
                    val obj = block.jsonObject
                    if (obj["type"]?.jsonPrimitive?.content == "tool_result") {
                        sawToolResult = true
                        obj["tool_use_id"]?.jsonPrimitive?.content?.let { observed.add(it) }
                    }
                }

                if (sawToolResult) {
                    j++
                } else {
                    break
                }
            }

            assertTrue(
                "Unmatched tool_use ids at API msg[$i]: expected=$expected observed=$observed",
                observed.containsAll(expected)
            )
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
