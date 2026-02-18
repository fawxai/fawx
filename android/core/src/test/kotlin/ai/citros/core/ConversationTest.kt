package ai.citros.core

import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class ConversationTest {

    @Test
    fun `new conversation is empty`() {
        val conv = Conversation()
        assertTrue(conv.messages.isEmpty())
    }

    @Test
    fun `addUser adds user message`() {
        val conv = Conversation()
        conv.addUser("Hello")
        
        assertEquals(1, conv.messages.size)
        assertEquals("user", conv.messages[0].role)
        assertEquals("Hello", conv.messages[0].content)
    }

    @Test
    fun `addAssistant adds assistant message`() {
        val conv = Conversation()
        conv.addAssistant("Hi there")
        
        assertEquals(1, conv.messages.size)
        assertEquals("assistant", conv.messages[0].role)
        assertEquals("Hi there", conv.messages[0].content)
    }

    @Test
    fun `toApiMessages returns correct format`() {
        val conv = Conversation()
        conv.addUser("Hello")
        conv.addAssistant("Hi")
        conv.addUser("How are you?")
        
        val apiMessages = conv.toApiMessages()
        
        assertEquals(3, apiMessages.size)
        assertEquals(mapOf("role" to "user", "content" to "Hello"), apiMessages[0])
        assertEquals(mapOf("role" to "assistant", "content" to "Hi"), apiMessages[1])
        assertEquals(mapOf("role" to "user", "content" to "How are you?"), apiMessages[2])
    }

    @Test
    fun `messages have timestamps`() {
        val before = System.currentTimeMillis()
        val conv = Conversation()
        conv.addUser("Test")
        val after = System.currentTimeMillis()
        
        assertTrue(conv.messages[0].timestamp in before..after)
    }

    @Test
    fun `conversation maintains order`() {
        val conv = Conversation()
        conv.addUser("1")
        conv.addAssistant("2")
        conv.addUser("3")
        conv.addAssistant("4")
        
        assertEquals(4, conv.messages.size)
        assertEquals("1", conv.messages[0].content)
        assertEquals("2", conv.messages[1].content)
        assertEquals("3", conv.messages[2].content)
        assertEquals("4", conv.messages[3].content)
    }

    @Test
    fun `toApiMessages with maxMessages trims at turn boundary`() {
        val conv = Conversation()
        // Add 25 alternating user/assistant messages
        repeat(25) { i ->
            if (i % 2 == 0) {
                conv.addUser("User message $i")
            } else {
                conv.addAssistant("Assistant message $i")
            }
        }
        
        val apiMessages = conv.toApiMessages(maxMessages = 10)
        
        // Turn-aware: should find nearest user message boundary when trimming
        assertTrue(apiMessages.isNotEmpty())
        assertEquals("user", apiMessages[0]["role"], "First message must be user role")
        // Last message should be "User message 24" (the final message)
        assertEquals("User message 24", apiMessages.last()["content"])
    }

    @Test
    fun `toApiMessages with maxMessages keeps all if under limit`() {
        val conv = Conversation()
        conv.addUser("Message 1")
        conv.addAssistant("Message 2")
        conv.addUser("Message 3")
        
        val apiMessages = conv.toApiMessages(maxMessages = 10)
        
        assertEquals(3, apiMessages.size)
        assertEquals("Message 1", apiMessages[0]["content"])
        assertEquals("Message 2", apiMessages[1]["content"])
        assertEquals("Message 3", apiMessages[2]["content"])
    }

    @Test
    fun `toApiMessages defaults to 20 messages`() {
        val conv = Conversation()
        // Add 30 messages
        repeat(30) { i ->
            conv.addUser("Message $i")
        }
        
        val apiMessages = conv.toApiMessages()
        
        assertEquals(20, apiMessages.size)
        // Should keep messages 10-29
        assertEquals("Message 10", apiMessages[0]["content"])
        assertEquals("Message 29", apiMessages[19]["content"])
    }

    @Test
    fun `toApiMessages with maxMessages = 1 keeps only last message`() {
        val conv = Conversation()
        conv.addUser("First")
        conv.addAssistant("Second")
        conv.addUser("Third")
        
        val apiMessages = conv.toApiMessages(maxMessages = 1)
        
        assertEquals(1, apiMessages.size)
        assertEquals("Third", apiMessages[0]["content"])
    }

    @Test
    fun `toApiMessages ensures first message is always user role`() {
        val conv = Conversation()
        // Start with assistant message (unusual but possible)
        conv.addAssistant("Assistant starts")
        conv.addUser("User message")
        conv.addAssistant("Assistant replies")
        
        val apiMessages = conv.toApiMessages()
        
        // Should drop the leading assistant message
        assertEquals(2, apiMessages.size)
        assertEquals("user", apiMessages[0]["role"])
        assertEquals("User message", apiMessages[0]["content"])
    }

    @Test
    fun `toApiMessages with trimming ensures first message is user role`() {
        val conv = Conversation()
        // Create scenario where trim boundary lands on assistant message
        repeat(15) { i ->
            if (i % 2 == 0) {
                conv.addUser("User $i")
            } else {
                conv.addAssistant("Assistant $i")
            }
        }
        // Add one more assistant message so takeLast(5) starts with assistant
        conv.addAssistant("Should be dropped")
        conv.addUser("User 15")
        conv.addAssistant("Assistant 15")
        conv.addUser("User 16")
        
        val apiMessages = conv.toApiMessages(maxMessages = 5)
        
        // Turn-aware trimming should find user message boundary
        assertTrue(apiMessages.isNotEmpty())
        assertEquals("user", apiMessages[0]["role"])
    }


    // ========== isError propagation ==========

    @Test
    fun `toolResult with isError includes is_error in content blocks`() {
        val msg = Message.toolResult("tc1", "error text", isError = true)
        val blocks = msg.contentBlocks!!
        assertEquals(1, blocks.size)
        assertEquals(true, blocks[0]["is_error"])
        assertEquals("tool_result", blocks[0]["type"])
        assertEquals("tc1", blocks[0]["tool_use_id"])
        assertEquals("error text", blocks[0]["content"])
    }

    @Test
    fun `toolResult without isError does not include is_error in content blocks`() {
        val msg = Message.toolResult("tc1", "success text")
        val blocks = msg.contentBlocks!!
        assertEquals(1, blocks.size)
        assertFalse(blocks[0].containsKey("is_error"))
    }

    @Test
    fun `toolResult isError false does not include is_error in content blocks`() {
        val msg = Message.toolResult("tc1", "success text", isError = false)
        val blocks = msg.contentBlocks!!
        assertFalse(blocks[0].containsKey("is_error"))
    }

    @Test
    fun `deserialized tool result with isError reconstructs is_error in content blocks`() {
        // Simulate deserialization: _contentBlocks is @Transient so it's lost,
        // but isError is persisted. The contentBlocks getter should reconstruct correctly.
        val original = Message.toolResult("tc1", "error msg", isError = true)
        // Simulate deserialization by creating a new Message with same persisted fields
        val deserialized = Message(
            role = original.role,
            content = original.content,
            toolCallId = original.toolCallId,
            isError = original.isError
        )
        val blocks = deserialized.contentBlocks!!
        assertEquals(1, blocks.size)
        assertEquals(true, blocks[0]["is_error"])
    }

    @Test
    fun `addToolResult with isError propagates to message`() {
        val conv = Conversation()
        conv.addToolResult("tc1", "error", isError = true)
        val msg = conv.messages[0]
        assertTrue(msg.isError)
        assertEquals(true, msg.contentBlocks!![0]["is_error"])
    }


    @Test
    fun `addToolResult with isError true produces is_error in contentBlocks`() {
        val conv = Conversation()
        conv.addToolResult("tc1", "some error", isError = true)
        val msg = conv.messages.last()
        assertEquals("tool", msg.role)
        assertEquals(true, msg.isError)
        val blocks = msg.contentBlocks
        assertTrue(blocks != null, "contentBlocks should not be null")
        val block = blocks!!.first()
        assertEquals("tool_result", block["type"])
        assertEquals("tc1", block["tool_use_id"])
        assertEquals("some error", block["content"])
        assertEquals(true, block["is_error"])
    }

    @Test
    fun `addToolResult with isError false omits is_error from contentBlocks`() {
        val conv = Conversation()
        conv.addToolResult("tc2", "success result", isError = false)
        val msg = conv.messages.last()
        assertEquals(false, msg.isError)
        val blocks = msg.contentBlocks!!
        val block = blocks.first()
        assertEquals("tool_result", block["type"])
        assertTrue(!block.containsKey("is_error"), "is_error should not be present when false")
    }

    @Test
    fun `deserialized tool result message reconstructs is_error in contentBlocks`() {
        val original = Message.toolResult("tc3", "error text", isError = true)
        val deserialized = Message(
            role = original.role,
            content = original.content,
            toolCallId = original.toolCallId,
            isError = original.isError
        )
        val blocks = deserialized.contentBlocks!!
        val block = blocks.first()
        assertEquals(true, block["is_error"])
    }
}
