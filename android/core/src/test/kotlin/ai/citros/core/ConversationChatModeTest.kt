package ai.citros.core

import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

/**
 * Tests for [Conversation.toApiMessages] chat-mode behavior:
 * tool message filtering, turn-aware trimming, and role safety.
 *
 * These complement [ConversationTest] which covers basic conversation operations.
 */
class ConversationChatModeTest {

    // ====== Tool message filtering ======

    @Test
    fun `tool result messages are filtered out in chat mode`() {
        val conv = Conversation()
        conv.addUser("Open Gmail")
        conv.messages.add(Message.assistantWithTools(null, listOf(
            ToolCall("t1", "open_app", mapOf("app_name" to "Gmail"))
        )))
        conv.messages.add(Message.toolResult("t1", "Opened Gmail"))
        conv.addAssistant("Done! Gmail is open.")
        conv.addUser("Thanks!")

        val apiMessages = conv.toApiMessages()

        assertTrue(apiMessages.none { it["role"] == "tool" },
            "Tool result messages should not appear in chat-mode API messages")
        assertEquals("user", apiMessages[0]["role"])
        assertEquals("Open Gmail", apiMessages[0]["content"])
    }

    @Test
    fun `assistant tool call text is preserved without Tools suffix`() {
        val conv = Conversation()
        conv.addUser("Open Gmail")
        conv.messages.add(Message.assistantWithTools(
            "I'll open Gmail for you",
            listOf(ToolCall("t1", "open_app", mapOf("app_name" to "Gmail")))
        ))
        conv.messages.add(Message.toolResult("t1", "Opened Gmail"))
        conv.addAssistant("Gmail is open.")
        conv.addUser("Thanks!")

        val apiMessages = conv.toApiMessages()

        val assistantWithText = apiMessages.find {
            it["role"] == "assistant" && it["content"]?.contains("open Gmail") == true
        }
        assertNotNull(assistantWithText, "Assistant text should be preserved")
        assertTrue(!assistantWithText["content"]!!.contains("[Tools:"),
            "Tool call metadata should be stripped from assistant text")
    }

    @Test
    fun `pure tool-call messages with no text are dropped`() {
        val conv = Conversation()
        conv.addUser("Go back")
        conv.messages.add(Message.assistantWithTools(null, listOf(
            ToolCall("t1", "press_back", emptyMap())
        )))
        conv.messages.add(Message.toolResult("t1", "Pressed back"))
        conv.addAssistant("Done!")
        conv.addUser("Thanks!")

        val apiMessages = conv.toApiMessages()

        // Pure tool-call message (no text) + tool result = both dropped
        // Remaining: "Go back", "Done!", "Thanks!"
        assertEquals(3, apiMessages.size)
        assertEquals("Go back", apiMessages[0]["content"])
        assertEquals("Done!", apiMessages[1]["content"])
        assertEquals("Thanks!", apiMessages[2]["content"])
    }

    @Test
    fun `multi-step tool task produces clean chat history`() {
        val conv = Conversation()
        conv.addUser("Send a text to Mom saying hi")
        // Step 1: open messages
        conv.messages.add(Message.assistantWithTools(null, listOf(
            ToolCall("t1", "open_app", mapOf("app_name" to "Messages"))
        )))
        conv.messages.add(Message.toolResult("t1", "Opened Messages"))
        // Step 2: tap contact
        conv.messages.add(Message.assistantWithTools(null, listOf(
            ToolCall("t2", "tap", mapOf("element_id" to 5))
        )))
        conv.messages.add(Message.toolResult("t2", "Tapped Mom"))
        // Step 3: type and send (multi-tool batch)
        conv.messages.add(Message.assistantWithTools(null, listOf(
            ToolCall("t3", "type_text", mapOf("text" to "hi")),
            ToolCall("t4", "tap", mapOf("element_id" to 10))
        )))
        conv.messages.add(Message.toolResult("t3", "Typed hi"))
        conv.messages.add(Message.toolResult("t4", "Sent"))
        // Final response
        conv.addAssistant("Done! I sent \"hi\" to Mom.")
        // New conversational message
        conv.addUser("What's the weather like?")

        val apiMessages = conv.toApiMessages()

        // All tool machinery filtered out
        assertTrue(apiMessages.none { it["role"] == "tool" })
        assertEquals("user", apiMessages[0]["role"])
        assertEquals("Send a text to Mom saying hi", apiMessages[0]["content"])
        assertEquals("What's the weather like?", apiMessages.last()["content"])
    }

    // ====== Turn-aware trimming ======

    @Test
    fun `trimming starts at user message boundary`() {
        val conv = Conversation()
        for (i in 0 until 15) {
            conv.addUser("Question $i")
            conv.addAssistant("Answer $i")
        }

        val apiMessages = conv.toApiMessages(maxMessages = 6)

        assertEquals("user", apiMessages[0]["role"])
        // Messages should alternate properly
        for (i in apiMessages.indices) {
            val expectedRole = if (i % 2 == 0) "user" else "assistant"
            assertEquals(expectedRole, apiMessages[i]["role"],
                "Message $i should be $expectedRole")
        }
    }

    @Test
    fun `trimming with tool history finds clean boundary`() {
        val conv = Conversation()
        // First conversation: tool task
        conv.addUser("Open Settings")
        conv.messages.add(Message.assistantWithTools(null, listOf(
            ToolCall("t1", "open_app", mapOf("app_name" to "Settings"))
        )))
        conv.messages.add(Message.toolResult("t1", "Opened Settings"))
        conv.addAssistant("Settings is open.")
        // Many more conversational messages
        for (i in 0 until 20) {
            conv.addUser("Chat $i")
            conv.addAssistant("Reply $i")
        }

        val apiMessages = conv.toApiMessages(maxMessages = 10)

        assertEquals("user", apiMessages[0]["role"])
        assertTrue(apiMessages.none { it["role"] == "tool" })
    }

    @Test
    fun `empty conversation returns empty list`() {
        val conv = Conversation()
        val apiMessages = conv.toApiMessages()
        assertTrue(apiMessages.isEmpty())
    }

    @Test
    fun `single user message works`() {
        val conv = Conversation()
        conv.addUser("Hello")
        val apiMessages = conv.toApiMessages()
        assertEquals(1, apiMessages.size)
        assertEquals("Hello", apiMessages[0]["content"])
    }

    @Test
    fun `conversation with only tool messages returns empty`() {
        val conv = Conversation()
        conv.messages.add(Message.assistantWithTools(null, listOf(
            ToolCall("t1", "press_back", emptyMap())
        )))
        conv.messages.add(Message.toolResult("t1", "Done"))

        val apiMessages = conv.toApiMessages()

        // Both messages get filtered (tool result dropped, pure tool-call assistant dropped)
        assertTrue(apiMessages.isEmpty())
    }

    @Test
    fun `trimming backward walk finds user when all trailing messages are assistant`() {
        val conv = Conversation()
        conv.addUser("User 1")
        conv.addAssistant("Assistant 1")
        // Add many consecutive assistant messages at the end
        repeat(15) {
            conv.addAssistant("Assistant consecutive $it")
        }

        val apiMessages = conv.toApiMessages(maxMessages = 10)

        // Forward walk finds no user message in last 10. Backward walk should
        // find "User 1" and include it + everything after.
        assertTrue(apiMessages.isNotEmpty(), "Should not return empty when user messages exist")
        assertEquals("user", apiMessages[0]["role"])
        assertEquals("User 1", apiMessages[0]["content"])
    }

    @Test
    fun `trimming backward walk skips mixed assistant sequence to find user boundary`() {
        val conv = Conversation()
        // Several user/assistant turns
        for (i in 0 until 5) {
            conv.addUser("Question $i")
            conv.addAssistant("Answer $i")
        }
        // Then many assistant messages (e.g., from a multi-part response)
        repeat(20) {
            conv.addAssistant("Part $it")
        }

        val apiMessages = conv.toApiMessages(maxMessages = 8)

        // Forward walk from rawStart won't find a user message in the assistant-heavy tail.
        // Backward walk should find the nearest user message before rawStart.
        assertTrue(apiMessages.isNotEmpty())
        assertEquals("user", apiMessages[0]["role"])
    }

}
