package ai.citros.core

import android.graphics.Rect
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(manifest = Config.NONE)
class ContextCompactorTest {

    private var idCounter = 0

    private fun toolResult(content: String, toolName: String? = null, id: String = "id_${idCounter++}") =
        Message.toolResult(id, content, toolName = toolName, isError = false)

    private fun screenDump(action: String, app: String = "com.example") =
        "$action\n\nSCREEN:\nApp: $app\n[0] \"Button\" [click]\n[1] \"Label\""

    private fun conversation(vararg toolResults: Message): List<Message> {
        val msgs = mutableListOf<Message>()
        msgs.add(Message(role = "user", content = "Do something"))
        for (tr in toolResults) {
            msgs.add(Message.assistantWithTools(null, listOf(ToolCall(tr.toolCallId ?: "id", tr.toolName ?: "tap", emptyMap()))))
            msgs.add(tr)
        }
        return msgs
    }

    // -- No-op cases --

    @Test
    fun `no trimming when disabled`() {
        val compactor = ContextCompactor(TrimmingPolicy.DISABLED)
        val msgs = conversation(
            toolResult(screenDump("Tapped [0]"), "tap"),
            toolResult(screenDump("Tapped [1]"), "tap"),
            toolResult(screenDump("Tapped [2]"), "tap"),
            toolResult(screenDump("Tapped [3]"), "tap"),
            toolResult(screenDump("Tapped [4]"), "tap")
        )
        val result = compactor.compact(msgs)
        assertEquals(msgs, result)
    }

    @Test
    fun `no trimming when below minMessages threshold`() {
        val compactor = ContextCompactor(TrimmingPolicy(minMessagesBeforeTrim = 100))
        val msgs = conversation(
            toolResult(screenDump("Tapped [0]"), "tap"),
            toolResult(screenDump("Tapped [1]"), "tap")
        )
        val result = compactor.compact(msgs)
        assertEquals(msgs, result)
    }

    @Test
    fun `no trimming when below token estimate threshold`() {
        val compactor = ContextCompactor(TrimmingPolicy(
            minMessagesBeforeTrim = 1,
            maxTokenEstimate = Int.MAX_VALUE  // Never triggers
        ))
        val msgs = conversation(
            toolResult(screenDump("Tapped [0]"), "tap"),
            toolResult(screenDump("Tapped [1]"), "tap")
        )
        val result = compactor.compact(msgs)
        assertEquals(msgs, result)
    }

    // -- Mechanical trimming --

    @Test
    fun `mechanical results trimmed beyond keepFull threshold`() {
        val compactor = ContextCompactor(TrimmingPolicy(
            minMessagesBeforeTrim = 1,
            maxTokenEstimate = 0,  // Always triggers
            keepFullByCategory = mapOf(ToolCategory.MECHANICAL to 2),
            trimMode = TrimMode.ACTION_SUMMARY
        ))
        val msgs = conversation(
            toolResult(screenDump("Tapped [0]"), "tap"),
            toolResult(screenDump("Tapped [1]"), "tap"),
            toolResult(screenDump("Tapped [2]"), "tap"),
            toolResult(screenDump("Tapped [3]"), "tap")
        )
        val result = compactor.compact(msgs)

        // First 2 taps trimmed (oldest), last 2 kept full
        val toolResults = result.filter { it.role == "tool" }
        assertEquals(4, toolResults.size)

        // Oldest two should be trimmed
        assertTrue(toolResults[0].content.contains(ContextCompactor.TRIM_MARKER))
        assertTrue(toolResults[1].content.contains(ContextCompactor.TRIM_MARKER))
        assertFalse(toolResults[0].content.contains("SCREEN:"))
        assertFalse(toolResults[1].content.contains("SCREEN:"))

        // Latest two should be full
        assertFalse(toolResults[2].content.contains(ContextCompactor.TRIM_MARKER))
        assertFalse(toolResults[3].content.contains(ContextCompactor.TRIM_MARKER))
        assertTrue(toolResults[2].content.contains("SCREEN:"))
        assertTrue(toolResults[3].content.contains("SCREEN:"))
    }

    @Test
    fun `ACTION_SUMMARY keeps only first line`() {
        val compactor = ContextCompactor(TrimmingPolicy(
            minMessagesBeforeTrim = 1,
            maxTokenEstimate = 0,
            keepFullByCategory = mapOf(ToolCategory.MECHANICAL to 0),
            trimMode = TrimMode.ACTION_SUMMARY
        ))
        val msgs = conversation(
            toolResult(screenDump("Tapped element [5]"), "tap")
        )
        val result = compactor.compact(msgs)
        val toolMsg = result.first { it.role == "tool" }

        assertEquals("Tapped element [5]\n${ContextCompactor.TRIM_MARKER}", toolMsg.content)
    }

    @Test
    fun `STRIP_SCREEN_ONLY removes only SCREEN section`() {
        val compactor = ContextCompactor(TrimmingPolicy(
            minMessagesBeforeTrim = 1,
            maxTokenEstimate = 0,
            keepFullByCategory = mapOf(ToolCategory.MECHANICAL to 0),
            trimMode = TrimMode.STRIP_SCREEN_ONLY
        ))
        val msgs = conversation(
            toolResult(screenDump("Tapped element [5]"), "tap")
        )
        val result = compactor.compact(msgs)
        val toolMsg = result.first { it.role == "tool" }

        assertTrue(toolMsg.content.startsWith("Tapped element [5]"))
        assertFalse(toolMsg.content.contains("SCREEN:"))
        assertTrue(toolMsg.content.contains(ContextCompactor.TRIM_MARKER))
    }

    // -- Research never trimmed --

    @Test
    fun `research results never trimmed regardless of count`() {
        val compactor = ContextCompactor(TrimmingPolicy(
            minMessagesBeforeTrim = 1,
            maxTokenEstimate = 0,
            trimMode = TrimMode.ACTION_SUMMARY
        ))
        val msgs = conversation(
            toolResult("Results for \"weather\": 1. Sunny 75F", "web_search"),
            toolResult("Results for \"news\": 1. Headlines today", "web_search"),
            toolResult("Results for \"sports\": 1. Game scores", "web_search"),
            toolResult("Results for \"stocks\": 1. Market up", "web_search"),
            toolResult("Results for \"tech\": 1. New phone", "web_search")
        )
        val result = compactor.compact(msgs)
        val toolResults = result.filter { it.role == "tool" }

        // ALL research results should be untouched
        for (tr in toolResults) {
            assertFalse("Research result should not be trimmed", tr.content.contains(ContextCompactor.TRIM_MARKER))
        }
    }

    // -- Category independence --

    @Test
    fun `categories trimmed independently`() {
        val compactor = ContextCompactor(TrimmingPolicy(
            minMessagesBeforeTrim = 1,
            maxTokenEstimate = 0,
            keepFullByCategory = mapOf(
                ToolCategory.MECHANICAL to 1,
                ToolCategory.PROMINENT to 1,
                ToolCategory.RESEARCH to Int.MAX_VALUE
            ),
            trimMode = TrimMode.ACTION_SUMMARY
        ))
        val msgs = conversation(
            toolResult(screenDump("Tapped [0]"), "tap"),         // MECHANICAL - will be trimmed
            toolResult(screenDump("Opened Gmail"), "open_app"),  // PROMINENT - will be trimmed
            toolResult("Results for \"test\": data", "web_search"), // RESEARCH - never trimmed
            toolResult(screenDump("Tapped [1]"), "tap"),         // MECHANICAL - kept (last 1)
            toolResult(screenDump("Opened Maps"), "open_app")    // PROMINENT - kept (last 1)
        )
        val result = compactor.compact(msgs)
        val toolResults = result.filter { it.role == "tool" }

        // First tap: trimmed
        assertTrue(toolResults[0].content.contains(ContextCompactor.TRIM_MARKER))
        // First open_app: trimmed
        assertTrue(toolResults[1].content.contains(ContextCompactor.TRIM_MARKER))
        // web_search: never trimmed
        assertFalse(toolResults[2].content.contains(ContextCompactor.TRIM_MARKER))
        // Last tap: kept
        assertFalse(toolResults[3].content.contains(ContextCompactor.TRIM_MARKER))
        // Last open_app: kept
        assertFalse(toolResults[4].content.contains(ContextCompactor.TRIM_MARKER))
    }

    // -- Idempotency --

    @Test
    fun `already trimmed messages are skipped`() {
        val compactor = ContextCompactor(TrimmingPolicy(
            minMessagesBeforeTrim = 1,
            maxTokenEstimate = 0,
            keepFullByCategory = mapOf(ToolCategory.MECHANICAL to 0),
            trimMode = TrimMode.ACTION_SUMMARY
        ))
        val msgs = conversation(
            toolResult("Tapped [0]\n${ContextCompactor.TRIM_MARKER}", "tap"), // Already trimmed
            toolResult(screenDump("Tapped [1]"), "tap")
        )
        val result = compactor.compact(msgs)
        val firstTool = result.first { it.role == "tool" }

        // Already-trimmed message should not be double-trimmed
        assertEquals("Tapped [0]\n${ContextCompactor.TRIM_MARKER}", firstTool.content)
    }

    @Test
    fun `compact is idempotent and marker appears exactly once`() {
        val compactor = ContextCompactor(TrimmingPolicy(
            minMessagesBeforeTrim = 1,
            maxTokenEstimate = 0,
            keepFullByCategory = mapOf(ToolCategory.MECHANICAL to 1),
            trimMode = TrimMode.ACTION_SUMMARY
        ))
        val msgs = conversation(
            toolResult(screenDump("Tapped [0]"), "tap"),
            toolResult(screenDump("Tapped [1]"), "tap"),
            toolResult(screenDump("Tapped [2]"), "tap")
        )
        val first = compactor.compact(msgs)
        val second = compactor.compact(first)

        // Running compact twice should produce same result
        assertEquals(first.map { it.content }, second.map { it.content })

        // Verify marker appears exactly once in each trimmed message
        for (msg in first.filter { it.content.contains(ContextCompactor.TRIM_MARKER) }) {
            val markerCount = msg.content.split(ContextCompactor.TRIM_MARKER).size - 1
            assertEquals("TRIM_MARKER should appear exactly once", 1, markerCount)
        }
    }

    // -- Reasoning trimming --

    @Test
    fun `reasoning results keep only last 1 by default`() {
        val compactor = ContextCompactor(TrimmingPolicy(
            minMessagesBeforeTrim = 1,
            maxTokenEstimate = 0,
            trimMode = TrimMode.ACTION_SUMMARY
        ))
        val msgs = conversation(
            toolResult("Thought: I should tap the inbox button", "think"),
            toolResult("Thought: Now I need to find the email", "think"),
            toolResult("Thought: Found it, tapping now", "think")
        )
        val result = compactor.compact(msgs)
        val toolResults = result.filter { it.role == "tool" }

        // First two trimmed, last one kept
        assertTrue(toolResults[0].content.contains(ContextCompactor.TRIM_MARKER))
        assertTrue(toolResults[1].content.contains(ContextCompactor.TRIM_MARKER))
        assertFalse(toolResults[2].content.contains(ContextCompactor.TRIM_MARKER))
    }

    // -- Content-based fallback --

    @Test
    fun `resolveCategory uses toolName when available`() {
        val compactor = ContextCompactor()
        val msg = toolResult("Some content", toolName = "web_search")
        assertEquals(ToolCategory.RESEARCH, compactor.resolveCategory(msg))
    }

    @Test
    fun `resolveCategory falls back to content heuristic`() {
        val compactor = ContextCompactor()
        val tap = toolResult("Tapped element [5]")
        val search = toolResult("Results for \"weather\": Sunny")
        val open = toolResult("Opened Gmail")
        val think = toolResult("Thought: I should try another approach")

        assertEquals(ToolCategory.MECHANICAL, compactor.resolveCategory(tap))
        assertEquals(ToolCategory.RESEARCH, compactor.resolveCategory(search))
        assertEquals(ToolCategory.PROMINENT, compactor.resolveCategory(open))
        assertEquals(ToolCategory.REASONING, compactor.resolveCategory(think))
    }

    // -- Message structure preserved --

    @Test
    fun `trimmed messages preserve role, toolCallId, and isError`() {
        val compactor = ContextCompactor(TrimmingPolicy(
            minMessagesBeforeTrim = 1,
            maxTokenEstimate = 0,
            keepFullByCategory = mapOf(ToolCategory.MECHANICAL to 0),
            trimMode = TrimMode.ACTION_SUMMARY
        ))
        val original = Message.toolResult("call_123", screenDump("Tapped [0]"), toolName = "tap", isError = true)
        val msgs = listOf(
            Message(role = "user", content = "test"),
            Message.assistantWithTools(null, listOf(ToolCall("call_123", "tap", emptyMap()))),
            original
        )
        val result = compactor.compact(msgs)
        val trimmed = result.last()

        assertEquals("tool", trimmed.role)
        assertEquals("call_123", trimmed.toolCallId)
        assertTrue(trimmed.isError)
        assertEquals("tap", trimmed.toolName)
    }

    // -- User and assistant messages never touched --

    @Test
    fun `user and assistant messages are never modified`() {
        val compactor = ContextCompactor(TrimmingPolicy(
            minMessagesBeforeTrim = 1,
            maxTokenEstimate = 0,
            trimMode = TrimMode.ACTION_SUMMARY
        ))
        val msgs = listOf(
            Message(role = "user", content = "Open Gmail and read my email"),
            Message(role = "assistant", content = "I'll open Gmail for you."),
            toolResult(screenDump("Opened Gmail"), "open_app"),
            Message(role = "assistant", content = "I can see your inbox with 3 emails.")
        )
        val result = compactor.compact(msgs)

        assertEquals("Open Gmail and read my email", result[0].content)
        assertEquals("I'll open Gmail for you.", result[1].content)
        assertEquals("I can see your inbox with 3 emails.", result[3].content)
    }

    // -- Legacy static method --

    @Test
    fun `legacy static compact method works`() {
        // Build a conversation big enough to trigger the default 60K token threshold
        val bigScreen = "Tapped [0]" + "\n\nSCREEN:\n" + "x".repeat(200_000)
        val msgs = listOf(
            Message(role = "user", content = "test"),
            Message.assistantWithTools(null, listOf(ToolCall("id1", "tap", emptyMap()))),
            toolResult(bigScreen, "tap"),
            Message.assistantWithTools(null, listOf(ToolCall("id2", "tap", emptyMap()))),
            toolResult(bigScreen, "tap"),
            Message.assistantWithTools(null, listOf(ToolCall("id3", "tap", emptyMap()))),
            toolResult(screenDump("Tapped [2]"), "tap")
        )
        // Static method should still work
        val result = ContextCompactor.compact(msgs)
        assertNotNull(result)
        assertTrue(result.isNotEmpty())
    }

    // -- defaultKeepFull for OTHER category --

    @Test
    fun `OTHER category uses defaultKeepFull`() {
        val compactor = ContextCompactor(TrimmingPolicy(
            minMessagesBeforeTrim = 1,
            maxTokenEstimate = 0,
            defaultKeepFull = 1,
            trimMode = TrimMode.ACTION_SUMMARY
        ))
        // "wait" is in OTHER category
        val msgs = conversation(
            toolResult("Waited 2 seconds\n\nSCREEN:\nApp: com.test\n[0] \"X\"", "wait"),
            toolResult("Waited 3 seconds\n\nSCREEN:\nApp: com.test\n[0] \"Y\"", "wait")
        )
        val result = compactor.compact(msgs)
        val toolResults = result.filter { it.role == "tool" }

        // First wait trimmed, second kept
        assertTrue(toolResults[0].content.contains(ContextCompactor.TRIM_MARKER))
        assertFalse(toolResults[1].content.contains(ContextCompactor.TRIM_MARKER))
    }

    // -- compactWithMetrics --

    @Test
    fun `compactWithMetrics returns null metrics when no compaction needed`() {
        val compactor = ContextCompactor(TrimmingPolicy(
            minMessagesBeforeTrim = 1,
            maxTokenEstimate = Int.MAX_VALUE
        ))
        val msgs = conversation(
            toolResult(screenDump("Tapped [0]"), "tap")
        )
        val (result, metrics) = compactor.compactWithMetrics(msgs)
        assertEquals(msgs, result)
        assertNull(metrics)
    }

    @Test
    fun `compactWithMetrics returns accurate metrics after compaction`() {
        val compactor = ContextCompactor(TrimmingPolicy(
            minMessagesBeforeTrim = 1,
            maxTokenEstimate = 0,
            keepFullByCategory = mapOf(ToolCategory.MECHANICAL to 1),
            trimMode = TrimMode.ACTION_SUMMARY
        ))
        val msgs = conversation(
            toolResult(screenDump("Tapped [0]"), "tap"),
            toolResult(screenDump("Tapped [1]"), "tap"),
            toolResult(screenDump("Tapped [2]"), "tap")
        )
        val (result, metrics) = compactor.compactWithMetrics(msgs)
        assertNotNull(metrics)
        assertEquals("context_compactor", metrics!!.stage)
        assertEquals(msgs.size, metrics.inputMessages)
        assertEquals(result.size, metrics.outputMessages)
        assertEquals(2, metrics.messagesCompacted)
        assertTrue(metrics.didCompact)
    }

    @Test
    fun `metrics tokensSaved matches actual char reduction`() {
        val compactor = ContextCompactor(TrimmingPolicy(
            minMessagesBeforeTrim = 1,
            maxTokenEstimate = 0,
            keepFullByCategory = mapOf(ToolCategory.MECHANICAL to 0),
            trimMode = TrimMode.ACTION_SUMMARY
        ))
        val msgs = conversation(
            toolResult(screenDump("Tapped [0]"), "tap")
        )
        val (result, metrics) = compactor.compactWithMetrics(msgs)
        assertNotNull(metrics)
        val inputChars = msgs.sumOf { it.content.length }
        val outputChars = result.sumOf { it.content.length }
        assertEquals((inputChars - outputChars) / 3, metrics!!.tokensSaved)
    }

    // -- Int.MAX_VALUE keepFull edge case --

    @Test
    fun `keepFull of Int MAX_VALUE never trims regardless of count`() {
        val compactor = ContextCompactor(TrimmingPolicy(
            minMessagesBeforeTrim = 1,
            maxTokenEstimate = 0,
            keepFullByCategory = mapOf(ToolCategory.OTHER to Int.MAX_VALUE)
        ))
        // Create many OTHER-category tool results
        val msgs = conversation(
            *Array(50) { i -> toolResult("Result $i\n\nSCREEN:\nApp: com.test\n[0] \"X\"", "wait") }
        )
        val result = compactor.compact(msgs)
        val toolResults = result.filter { it.role == "tool" }

        // None should be trimmed
        assertTrue(
            "No messages should be trimmed when keepFull is Int.MAX_VALUE",
            toolResults.none { it.content.contains(ContextCompactor.TRIM_MARKER) }
        )
    }
}
