package ai.citros.core

import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

class ContextManagerTest {

    private val manager = ContextManager()

    // ========== compact() tests ==========

    @Test
    fun `compact returns original messages when below threshold`() {
        val messages = listOf(
            Message(role = "user", content = "Open Settings"),
            Message(role = "assistant", content = "I'll open Settings for you")
        )
        val result = manager.compact(messages, currentStep = 3)
        assertEquals(messages, result)
    }

    @Test
    fun `compact returns original messages when list is small`() {
        val messages = listOf(
            Message(role = "user", content = "Open Settings"),
            Message(role = "assistant", content = "Opening..."),
            Message(role = "user", content = "CURRENT SCREEN:\nApp: Settings\n[1] WiFi [click]"),
            Message(role = "assistant", content = "I see Settings")
        )
        // Even at step 10, if messages are few enough, no compaction
        val result = manager.compact(messages, currentStep = 10)
        assertEquals(messages, result)
    }

    @Test
    fun `compact keeps first message intact`() {
        val messages = buildRealisticConversation(10)
        val result = manager.compact(messages, currentStep = 8)
        assertEquals(messages[0].content, result[0].content)
        assertEquals("user", result[0].role)
    }

    @Test
    fun `compact keeps recent messages in full`() {
        val messages = buildRealisticConversation(10)
        val result = manager.compact(messages, currentStep = 8)
        
        // Last RECENT_MESSAGES messages should be unchanged
        val recentCount = ContextManager.RECENT_MESSAGES
        val originalRecent = messages.takeLast(recentCount)
        val resultRecent = result.takeLast(recentCount)
        assertEquals(originalRecent.size, resultRecent.size)
        for (i in originalRecent.indices) {
            assertEquals(originalRecent[i].content, resultRecent[i].content)
        }
    }

    @Test
    fun `compact summarizes old screen content`() {
        val messages = buildRealisticConversation(10)
        val result = manager.compact(messages, currentStep = 8)
        
        // Old messages should be compacted (shorter than originals)
        val recentCount = ContextManager.RECENT_MESSAGES
        val oldCompacted = result.drop(1).dropLast(recentCount)
        assertTrue(oldCompacted.isNotEmpty(), "Should have compacted messages")
        
        for (msg in oldCompacted) {
            if (msg.role == "user" && msg.content?.contains("PREVIOUS SCREEN") == true) {
                assertTrue(msg.content!!.length <= ContextManager.COMPACTED_SCREEN_MAX_CHARS)
            }
        }
    }

    @Test
    fun `compact reduces total message size`() {
        val messages = buildRealisticConversation(12)
        val originalSize = messages.sumOf { (it.content ?: "").length }
        
        val result = manager.compact(messages, currentStep = 10)
        val compactedSize = result.sumOf { (it.content ?: "").length }
        
        assertTrue(compactedSize < originalSize, "Compacted size ($compactedSize) should be less than original ($originalSize)")
    }

    @Test
    fun `compact handles extreme step count`() {
        val messages = buildRealisticConversation(50)
        val result = manager.compact(messages, currentStep = 100)
        
        // Should not crash, should compact
        assertTrue(result.isNotEmpty())
        assertTrue(result.size <= messages.size)
        // First message preserved
        assertEquals(messages[0].content, result[0].content)
        // Recent window preserved
        val recentCount = ContextManager.RECENT_MESSAGES
        assertEquals(messages.takeLast(recentCount), result.takeLast(recentCount))
    }

    @Test
    fun `compact handles multi-tool result sequences correctly`() {
        // Simulate a response with 3 tool calls → 3 tool results → user
        val messages = mutableListOf<Message>()
        messages.add(Message(role = "user", content = "CURRENT SCREEN:\nApp: launcher\n[1] Chrome [click]\n\nDo three things"))
        messages.add(Message(role = "assistant", content = "I'll do all three"))
        messages.add(Message.toolResult("t1", "Tapped element 1"))
        messages.add(Message.toolResult("t2", "Typed hello"))
        messages.add(Message.toolResult("t3", "Swiped up"))
        messages.add(Message(role = "user", content = "CURRENT SCREEN:\nApp: chrome\n[1] Search [edit]\n\n[Executed 3 tool(s)]"))
        messages.add(Message(role = "assistant", content = "Done with all three"))
        // Add more to trigger compaction
        for (i in 1..5) {
            messages.add(Message(role = "user", content = "CURRENT SCREEN:\nApp: app$i\n[$i] Button [click]\n\nNext"))
            messages.add(Message(role = "assistant", content = "Step $i done"))
        }

        val result = manager.compact(messages, currentStep = 8)
        assertTrue(result.isNotEmpty())
        // Should not crash with irregular message sequences
        assertEquals(messages[0].content, result[0].content)
    }

    // ========== compactWithMetrics() tests ==========

    @Test
    fun `compactWithMetrics returns null metrics below threshold`() {
        val messages = listOf(
            Message(role = "user", content = "Open Settings"),
            Message(role = "assistant", content = "Opening...")
        )
        val (result, metrics) = manager.compactWithMetrics(messages, currentStep = 3)
        assertEquals(messages, result)
        assertNull(metrics)
    }

    @Test
    fun `compactWithMetrics returns accurate metrics after compaction`() {
        val messages = buildRealisticConversation(12)
        val (result, metrics) = manager.compactWithMetrics(messages, currentStep = 10)

        assertNotNull(metrics)
        assertEquals("context_manager", metrics!!.stage)
        assertEquals(messages.size, metrics.inputMessages)
        assertEquals(result.size, metrics.outputMessages)
        assertTrue(metrics.messagesCompacted > 0)
        assertTrue(metrics.didCompact)
        assertTrue(metrics.tokensSaved > 0)
    }

    // ========== compactScreenContent() tests ==========

    @Test
    fun `compactScreenContent extracts app name and element counts`() {
        val content = """CURRENT SCREEN:
App: com.google.android.settings
[1] "WiFi" [click]
[2] "Bluetooth" [click]
[3] "Search settings" [edit]
[4] "Display" [click]

Tap WiFi please"""

        val result = manager.compactScreenContent(content)
        assertTrue(result.contains("PREVIOUS SCREEN"))
        assertTrue(result.contains("settings"))
        assertTrue(result.contains("4 elements"))
        assertTrue(result.contains("3 clickable"))
        assertTrue(result.contains("1 editable"))
    }

    @Test
    fun `compactScreenContent preserves user message after screen dump`() {
        val content = """CURRENT SCREEN:
App: com.google.android.settings
[1] "WiFi" [click]

Tap WiFi please"""

        val result = manager.compactScreenContent(content)
        assertTrue(result.contains("Tap WiFi"))
    }

    @Test
    fun `compactScreenContent handles missing screen section`() {
        val content = "Just a normal message"
        val result = manager.compactScreenContent(content)
        assertEquals(content, result)
    }

    // ========== compactToolResult() tests ==========

    @Test
    fun `compactToolResult summarizes screen refresh in bracket format`() {
        val content = """Screen refreshed:
App: com.google.android.settings
[1] "WiFi" [click]
[2] "Bluetooth" [click]
[3] "Search" [edit]"""

        val result = manager.compactToolResult(content)
        assertTrue(result.startsWith("[Screen:"))
        assertTrue(result.endsWith("]"))
        assertTrue(result.contains("3 elements"))
    }

    @Test
    fun `compactToolResult formats thought in bracket format`() {
        val content = "Thought: I need to scroll down to find the WiFi toggle"
        val result = manager.compactToolResult(content)
        assertTrue(result.startsWith("[Thought:"))
        assertTrue(result.endsWith("]"))
    }

    @Test
    fun `compactToolResult truncates long thought`() {
        val longThought = "Thought: " + "a".repeat(200)
        val result = manager.compactToolResult(longThought)
        assertTrue(result.length <= ContextManager.COMPACTED_TOOL_RESULT_MAX_CHARS)
    }

    @Test
    fun `compactToolResult keeps short results unchanged`() {
        val short = "Tapped element 5"
        val result = manager.compactToolResult(short)
        assertEquals(short, result)
    }

    @Test
    fun `compactToolResult formats long results in bracket format`() {
        val long = "x".repeat(200)
        val result = manager.compactToolResult(long)
        assertTrue(result.length <= ContextManager.COMPACTED_TOOL_RESULT_MAX_CHARS)
        assertTrue(result.startsWith("[Action:"))
        assertTrue(result.endsWith("]"))
    }

    @Test
    fun `compactToolResult formats wait result in bracket format`() {
        val content = """Waited 2s. Screen:
App: com.google.chrome
[1] "Search" [edit]"""
        val result = manager.compactToolResult(content)
        assertTrue(result.startsWith("["))
        assertTrue(result.endsWith("]"))
        assertTrue(result.contains("Waited"))
        assertTrue(result.contains("chrome"))
    }

    // ========== truncateAtWordBoundary() tests ==========

    @Test
    fun `truncateAtWordBoundary returns short text unchanged`() {
        val result = manager.truncateAtWordBoundary("hello world", 50)
        assertEquals("hello world", result)
    }

    @Test
    fun `truncateAtWordBoundary cuts at word boundary`() {
        val result = manager.truncateAtWordBoundary("the quick brown fox jumps over the lazy dog", 20)
        assertTrue(result.endsWith("..."))
        // Should not cut mid-word
        assertFalse(result.contains("quic...") || result.contains("brow..."))
    }

    @Test
    fun `truncateAtWordBoundary respects max length`() {
        val long = "word ".repeat(100)
        val result = manager.truncateAtWordBoundary(long, 50)
        assertTrue(result.length <= 50)
        assertTrue(result.endsWith("..."))
    }

    @Test
    fun `truncateAtWordBoundary handles single long word`() {
        val result = manager.truncateAtWordBoundary("a".repeat(200), 50)
        assertTrue(result.length <= 50)
        assertTrue(result.endsWith("..."))
    }

    // ========== Helper ==========

    /**
     * Build a realistic conversation matching the actual action loop pattern:
     * [user+screen] → [assistant+tools] → [tool_result] → [user+screen] → ...
     */
    private fun buildRealisticConversation(steps: Int): MutableList<Message> {
        val messages = mutableListOf<Message>()
        
        // Initial user message with screen
        messages.add(Message(role = "user", content = """CURRENT SCREEN:
App: com.google.android.launcher
[1] "Search" [click]
[2] "Chrome" [click]
[3] "Settings" [click]
[4] "Camera" [click]

Open Settings and turn on WiFi"""))

        for (i in 1..steps) {
            // Assistant with tool call
            messages.add(Message(role = "assistant", content = "I'll tap element $i"))
            
            // Tool result
            messages.add(Message.toolResult("tool_$i", "Tapped element $i"))
            
            // Next user message with screen context (action loop message)
            messages.add(Message(role = "user", content = """CURRENT SCREEN:
App: com.google.android.settings
[${i*3}] "Item $i" [click]
[${i*3+1}] "Item ${i+1}" [click]
[${i*3+2}] "Search" [edit]

[Step $i/20 — executed 1 tool(s)]"""))
        }
        
        return messages
    }
}
