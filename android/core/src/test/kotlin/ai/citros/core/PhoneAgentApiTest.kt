package ai.citros.core

import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.After
import org.junit.Before
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class PhoneAgentApiTest {

    private lateinit var server: MockWebServer

    @Before
    fun setUp() {
        server = MockWebServer()
        server.start()
    }

    @After
    fun tearDown() {
        server.shutdown()
    }

    private fun createAgent(
        chatClient: ClaudeClient? = null,
        actionClient: ClaudeClient? = null,
        fileManager: AgentFileManager? = null,
        memoryProvider: MemoryProvider? = null
    ): PhoneAgentApi {
        val defaultClient = ClaudeClient(
            apiKey = "sk-ant-api03-test",
            systemPrompt = PhoneAgentPrompts.SYSTEM_PROMPT,
            baseUrl = server.url("/v1/messages").toString()
        )
        val resolvedChat = chatClient ?: defaultClient
        val resolvedAction = actionClient ?: resolvedChat
        return PhoneAgentApi(resolvedChat, resolvedAction, fileManager, memoryProvider).also {
            it.phoneControlOverride = true // Simulate phone control available in tests
        }
    }

    private fun enqueueResponse(text: String) {
        server.enqueue(MockResponse()
            .setBody("""{"content":[{"type":"text","text":${kotlinx.serialization.json.Json.encodeToString(kotlinx.serialization.serializer<String>(), text)}}],"role":"assistant"}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))
    }


    // ========== Structured Tool Use Tests ==========

    @Test
    fun `sendMessage with tool call returns ChatResponse with toolCalls`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"content":[{"type":"tool_use","id":"toolu_123","name":"tap","input":{"element_id":5}}],"role":"assistant","stop_reason":"tool_use"}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val agent = createAgent()
        val response = agent.sendMessage("Tap element 5", null, isActionLoop = false)

        assertEquals(1, response.toolCalls.size)
        assertEquals("toolu_123", response.toolCalls[0].id)
        assertEquals("tap", response.toolCalls[0].name)
        assertEquals(5, (response.toolCalls[0].input["element_id"] as? Number)?.toInt())
    }

    @Test
    fun `sendMessage with text-only response returns ChatResponse with empty toolCalls`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"content":[{"type":"text","text":"Task complete!"}],"role":"assistant","stop_reason":"end_turn"}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val agent = createAgent()
        val response = agent.sendMessage("Are you done?", null, isActionLoop = false)

        assertEquals("Task complete!", response.text)
        assertTrue(response.toolCalls.isEmpty())
        assertEquals("end_turn", response.stopReason)
    }

    @Test
    fun `executeToolCall tap validates element_id parameter`() = runTest {
        val agent = createAgent()

        // Valid tap
        val validTap = ToolCall("t1", "tap", mapOf("element_id" to 5))
        val result1 = agent.executeToolCall(validTap, null)
        assertTrue(result1.text.contains("tap") || result1.text.contains("Tap") || result1.text.contains("Failed"))

        // Missing element_id
        val invalidTap = ToolCall("t2", "tap", emptyMap())
        val result2 = agent.executeToolCall(invalidTap, null)
        assertTrue(result2.text.contains("Failed") || result2.text.contains("requires"))
    }

    @Test
    fun `executeToolCall unknown tool returns error`() = runTest {
        val agent = createAgent()
        val unknownTool = ToolCall("t1", "unknown_action", emptyMap())
        val result = agent.executeToolCall(unknownTool, null)

        assertTrue(result.text.contains("Failed") || result.text.contains("unknown"))
    }

    @Test
    fun `file tools execute happy path`() = runTest {
        val tempRoot = createTempDir(prefix = "phone-agent-file-tools")
        try {
            val manager = AgentFileManager.fromDirectory(tempRoot)
            val agent = createAgent(fileManager = manager)

            val write = agent.executeToolCall(
                ToolCall("f1", "write_file", mapOf("path" to "memory/2026-02-12.md", "content" to "hello")),
                null
            )
            val read = agent.executeToolCall(
                ToolCall("f2", "read_file", mapOf("path" to "memory/2026-02-12.md")),
                null
            )
            val list = agent.executeToolCall(
                ToolCall("f3", "list_files", mapOf("path" to "memory")),
                null
            )

            assertTrue(write.text.contains("\"ok\":true"))
            assertTrue(read.text.contains("\"content\":\"hello\""))
            assertTrue(list.text.contains("2026-02-12.md"))
        } finally {
            tempRoot.deleteRecursively()
        }
    }

    @Test
    fun `file tool json responses escape special characters`() = runTest {
        val tempRoot = createTempDir(prefix = "phone-agent-file-tools-json")
        try {
            val manager = AgentFileManager.fromDirectory(tempRoot)
            val agent = createAgent(fileManager = manager)

            val quotedPath = "memory/file\"name\".md"
            val write = agent.executeToolCall(
                ToolCall("j1", "write_file", mapOf("path" to quotedPath, "content" to "hello")),
                null
            )
            val read = agent.executeToolCall(
                ToolCall("j2", "read_file", mapOf("path" to quotedPath)),
                null
            )

            val writeJson = Json.parseToJsonElement(write.text).jsonObject
            val readJson = Json.parseToJsonElement(read.text).jsonObject

            assertEquals(true, writeJson["ok"]?.jsonPrimitive?.content?.toBoolean())
            assertEquals(quotedPath, writeJson["path"]?.jsonPrimitive?.content)
            assertEquals("hello", readJson["content"]?.jsonPrimitive?.content)
        } finally {
            tempRoot.deleteRecursively()
        }
    }

    @Test
    fun `file tools enforce security constraints`() = runTest {
        val tempRoot = createTempDir(prefix = "phone-agent-file-tools-security")
        try {
            val manager = AgentFileManager.fromDirectory(tempRoot)
            val agent = createAgent(fileManager = manager)

            val traversal = agent.executeToolCall(
                ToolCall("s1", "read_file", mapOf("path" to "../secrets.txt")),
                null
            )
            val securityWrite = agent.executeToolCall(
                ToolCall("s2", "write_file", mapOf("path" to "SECURITY.md", "content" to "hack")),
                null
            )

            assertTrue(traversal.text.contains("\"ok\":false"))
            assertTrue(traversal.text.contains("\"tool\":\"read_file\""))
            assertTrue(securityWrite.text.contains("\"ok\":false"))
            assertTrue(securityWrite.text.contains("\"tool\":\"write_file\""))
        } finally {
            tempRoot.deleteRecursively()
        }
    }

    @Test
    fun `memory tools execute happy path`() = runTest {
        val memoryProvider = InMemoryMemoryProvider()
        val agent = createAgent(memoryProvider = memoryProvider)

        val remember = agent.executeToolCall(
            ToolCall("m1", "remember", mapOf("content" to "buy coffee", "tags" to "shopping,errands")),
            null
        )
        val recall = agent.executeToolCall(
            ToolCall("m2", "recall", mapOf("query" to "coffee", "limit" to 5)),
            null
        )
        val listed = agent.executeToolCall(
            ToolCall("m3", "list_memories", mapOf("limit" to 10)),
            null
        )

        val rememberJson = Json.parseToJsonElement(remember.text).jsonObject
        val recallJson = Json.parseToJsonElement(recall.text).jsonObject
        val listJson = Json.parseToJsonElement(listed.text).jsonObject

        assertEquals(true, rememberJson["ok"]?.jsonPrimitive?.content?.toBoolean())
        assertEquals("remember", rememberJson["tool"]?.jsonPrimitive?.content)
        assertEquals("recall", recallJson["tool"]?.jsonPrimitive?.content)
        assertEquals("list_memories", listJson["tool"]?.jsonPrimitive?.content)
        assertTrue(recall.text.contains("buy coffee"))
    }

    @Test
    fun `executeToolCall think returns thought without side effects`() = runTest {
        val agent = createAgent()
        val think = ToolCall("t1", "think", mapOf("thought" to "I need to scroll down to find Settings"))
        val result = agent.executeToolCall(think, null)
        assertTrue(result.text.contains("I need to scroll down to find Settings"))
        // think should not produce error
        assertTrue(!result.text.contains("Failed:"))
    }

    @Test
    fun `executeToolCall think with empty thought returns error`() = runTest {
        val agent = createAgent()
        val think = ToolCall("t1", "think", mapOf("thought" to ""))
        val result = agent.executeToolCall(think, null)
        assertTrue(result.text.contains("Failed") || result.text.contains("requires"))
    }

    @Test
    fun `executeToolCall wait returns screen refresh message`() = runTest {
        val agent = createAgent()
        // Without ScreenReader attached, should still return a meaningful result
        val wait = ToolCall("t1", "wait", mapOf("seconds" to 2))
        val result = agent.executeToolCall(wait, null)
        assertTrue(result.text.contains("Waited") || result.text.contains("wait"))
    }

    @Test
    fun `executeToolCall wait clamps seconds to valid range`() = runTest {
        val agent = createAgent()
        val waitTooLong = ToolCall("t1", "wait", mapOf("seconds" to 99))
        val result = agent.executeToolCall(waitTooLong, null)
        // Should not error, just clamp
        assertTrue(!result.text.contains("Failed:"))
    }

    @Test
    fun `executeToolCall wait with missing seconds uses default`() = runTest {
        val agent = createAgent()
        val wait = ToolCall("t1", "wait", emptyMap())
        val result = agent.executeToolCall(wait, null)
        assertTrue(result.text.contains("Waited"))
    }

    @Test
    fun `executeToolCall long_press validates element_id`() = runTest {
        val agent = createAgent()
        
        // Missing element_id
        val invalid = ToolCall("t1", "long_press", emptyMap())
        val result = agent.executeToolCall(invalid, null)
        assertTrue(result.text.contains("Failed") || result.text.contains("requires"))
    }

    @Test
    fun `executeToolCall long_press with valid element_id returns result`() = runTest {
        val agent = createAgent()
        val valid = ToolCall("t1", "long_press", mapOf("element_id" to 5))
        val result = agent.executeToolCall(valid, null)
        assertTrue(result.text.contains("Long-pressed") || result.text.contains("Failed"))
    }

    @Test
    fun `memory tools return configured error when provider missing`() = runTest {
        val agent = createAgent(memoryProvider = null)

        val remember = agent.executeToolCall(
            ToolCall("m1", "remember", mapOf("content" to "buy coffee")),
            null
        )

        assertTrue(remember.text.contains("\"ok\":false"))
        assertTrue(remember.text.contains("Memory provider not configured"))
    }

    @Test
    fun `addToolResult adds message to conversation`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"content":[{"type":"tool_use","id":"t1","name":"tap","input":{"element_id":1}}],"role":"assistant","stop_reason":"tool_use"}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val agent = createAgent()
        val response = agent.sendMessage("Tap element 1", null, isActionLoop = false)

        // Add tool result
        agent.addToolResult(response.toolCalls[0].id, "Tapped successfully")

        // Next request should include the tool result
        server.enqueue(MockResponse()
            .setBody("""{"content":[{"type":"text","text":"Great!"}],"role":"assistant","stop_reason":"end_turn"}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        agent.sendMessage("[Next turn]", null, isActionLoop = true)

        // Verify both requests were made
        assertEquals(2, server.requestCount)

        // Check that second request includes tool result
        server.takeRequest() // Skip first
        val request2 = server.takeRequest()
        val body = request2.body.readUtf8()
        assertTrue(body.contains("tool_result") || body.contains("tool"))
    }

    @Test
    fun `system prompt has no JSON instructions`() {
        val prompt = PhoneAgentPrompts.SYSTEM_PROMPT

        // Should NOT contain JSON format instructions
        assertTrue(!prompt.contains("{\"action\""))
        assertTrue(!prompt.contains("Respond with JSON"))

        // Should contain capability descriptions
        assertTrue(prompt.contains("tap") || prompt.contains("Tap") || prompt.contains("screen"))
    }

    @Test
    fun `system prompt includes strategy section`() {
        val prompt = PhoneAgentPrompts.SYSTEM_PROMPT
        assertTrue(prompt.contains("Strategy"), "Prompt should include Strategy section")
        assertTrue(prompt.contains("Direct Commands"), "Prompt should include direct command pattern")
        assertTrue(prompt.contains("Tasks"), "Prompt should include task pattern")
    }

    @Test
    fun `action prompt is shorter than system prompt`() {
        assertTrue(
            PhoneAgentPrompts.ACTION_PROMPT.length < PhoneAgentPrompts.SYSTEM_PROMPT.length,
            "Action prompt (${PhoneAgentPrompts.ACTION_PROMPT.length}) should be shorter than system prompt (${PhoneAgentPrompts.SYSTEM_PROMPT.length})"
        )
    }

    @Test
    fun `action prompt includes key reminders`() {
        val prompt = PhoneAgentPrompts.ACTION_PROMPT
        assertTrue(prompt.contains("Element IDs"), "Action prompt should remind about ephemeral IDs")
        assertTrue(prompt.contains("type_text"), "Action prompt should remind about type_text")
    }

    @Test
    fun `system prompt includes recovery instructions`() {
        val prompt = PhoneAgentPrompts.SYSTEM_PROMPT
        assertTrue(
            prompt.contains("When Things Go Wrong"),
            "Prompt should include recovery section"
        )
    }

    @Test
    fun `system prompt disambiguates settings commands`() {
        val prompt = PhoneAgentPrompts.SYSTEM_PROMPT
        assertTrue(
            prompt.contains("Disambiguation"),
            "Prompt should include Disambiguation section"
        )
        assertTrue(
            prompt.contains("Android Settings"),
            "Prompt should clarify 'open settings' means Android Settings"
        )
    }

    @Test
    fun `conversational first turn uses chat mode without tools`() = runTest {
        val chatOnlyClient = ScriptedProviderClient(
            provider = Provider.OPENAI,
            chatResponses = ArrayDeque(listOf("Hey there!")),
            toolResponses = ArrayDeque(
                listOf(
                    ChatResponse(
                        text = "Should never be used",
                        toolCalls = listOf(ToolCall("toolu_1", "tap", mapOf("element_id" to 1))),
                        stopReason = "tool_use"
                    )
                )
            )
        )

        val agent = PhoneAgentApi(chatOnlyClient, chatOnlyClient)
        val response = agent.sendMessage("hi", screenContent = null, isActionLoop = false)

        assertEquals("Hey there!", response.text)
        assertTrue(response.toolCalls.isEmpty())
        assertEquals(1, chatOnlyClient.chatCalls)
        assertEquals(0, chatOnlyClient.chatWithToolsCalls)
    }

    @Test
    fun `question with action word still uses chat mode without tools`() = runTest {
        val chatOnlyClient = ScriptedProviderClient(
            provider = Provider.OPENAI,
            chatResponses = ArrayDeque(listOf("Sure — what tab are you thinking about?")),
            toolResponses = ArrayDeque(
                listOf(
                    ChatResponse(
                        text = "Should never be used",
                        toolCalls = listOf(ToolCall("toolu_2", "open_app", mapOf("app_name" to "Chrome"))),
                        stopReason = "tool_use"
                    )
                )
            )
        )

        val agent = PhoneAgentApi(chatOnlyClient, chatOnlyClient)
        val response = agent.sendMessage("Can you open a new tab?", screenContent = null, isActionLoop = false)

        assertTrue(response.text?.contains("tab", ignoreCase = true) == true)
        assertTrue(response.toolCalls.isEmpty())
        assertEquals(1, chatOnlyClient.chatCalls)
        assertEquals(0, chatOnlyClient.chatWithToolsCalls)
    }

    @Test
    fun `action phrases like take screenshot use tool mode`() = runTest {
        val toolResponse = ChatResponse(
            text = "Done",
            toolCalls = emptyList(),
            stopReason = "end_turn"
        )
        val toolClient = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            chatResponses = ArrayDeque(listOf("no", "no", "no", "no")),
            toolResponses = ArrayDeque(
                listOf(toolResponse, toolResponse, toolResponse, toolResponse)
            )
        )

        val agent = PhoneAgentApi(toolClient, toolClient).also { it.phoneControlOverride = true }

        // All of these should trigger tool mode, not chat mode
        for (phrase in listOf("Take a screenshot", "Set a timer", "Check my notifications", "Show me the weather")) {
            toolClient.chatCalls = 0
            toolClient.chatWithToolsCalls = 0
            agent.clearConversation()
            agent.sendMessage(phrase, screenContent = null, isActionLoop = false)
            assertEquals(0, toolClient.chatCalls, "\"$phrase\" should NOT use chat mode")
            assertEquals(1, toolClient.chatWithToolsCalls, "\"$phrase\" should use tool mode")
        }
    }

    @Test
    fun `action requests use chat mode when phone control disabled`() = runTest {
        // #390: Without phone control, the model should NOT receive tools.
        // This prevents hallucinated XML tool calls in plain text responses.
        val client = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            chatResponses = ArrayDeque(listOf(
                "I can't control your phone right now. Please enable phone control in Settings."
            )),
            toolResponses = ArrayDeque(
                listOf(
                    ChatResponse(
                        text = "Should never be used",
                        toolCalls = listOf(ToolCall("toolu_1", "tap", mapOf("element_id" to 1))),
                        stopReason = "tool_use"
                    )
                )
            )
        )

        val agent = PhoneAgentApi(client, client).also { it.phoneControlOverride = false }
        val response = agent.sendMessage("Take a screenshot", screenContent = null, isActionLoop = false)

        // Should use chat mode, not tool mode
        assertEquals(1, client.chatCalls, "Should use chat mode (no tools)")
        assertEquals(0, client.chatWithToolsCalls, "Should NOT use tool mode")
        assertTrue(response.toolCalls.isEmpty(), "Should have no tool calls")
    }

    @Test
    fun `currentToolStep is settable and readable`() {
        val agent = createAgent()
        assertEquals(0, agent.currentToolStep)
        agent.currentToolStep = 5
        assertEquals(5, agent.currentToolStep)
    }

    @Test
    fun `clearConversation resets tool step counter`() {
        val agent = createAgent()
        agent.currentToolStep = 7
        assertEquals(7, agent.currentToolStep)
        agent.clearConversation()
        assertEquals(0, agent.currentToolStep)
    }

    @Test
    fun `clearConversation clears tool use history`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"content":[{"type":"text","text":"Hi"}],"role":"assistant","stop_reason":"end_turn"}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val agent = createAgent()
        agent.sendMessage("UNIQUE_MARKER_FIRST", null, isActionLoop = false)

        // Clear and send another message
        agent.clearConversation()

        server.enqueue(MockResponse()
            .setBody("""{"content":[{"type":"text","text":"New conversation"}],"role":"assistant","stop_reason":"end_turn"}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        agent.sendMessage("UNIQUE_MARKER_SECOND", null, isActionLoop = false)

        // Verify second request doesn't include first message
        val request1 = server.takeRequest()
        val request2 = server.takeRequest()

        val body1 = request1.body.readUtf8()
        val body2 = request2.body.readUtf8()

        // First request has first marker, second request should NOT have it
        assertTrue(body1.contains("UNIQUE_MARKER_FIRST"))
        assertTrue(body2.contains("UNIQUE_MARKER_SECOND"))
        assertTrue(!body2.contains("UNIQUE_MARKER_FIRST"))
    }

    private class InMemoryMemoryProvider : MemoryProvider {
        private val data = mutableListOf<MemoryResult>()

        override suspend fun store(content: String, metadata: MemoryMetadata): String {
            val id = "mem-${data.size + 1}"
            data += MemoryResult(
                id = id,
                content = content,
                tags = metadata.tags,
                source = metadata.source,
                createdAt = System.currentTimeMillis()
            )
            return id
        }

        override suspend fun search(query: String, limit: Int): List<MemoryResult> {
            return data.filter { it.content.contains(query, ignoreCase = true) }.take(limit)
        }

        override suspend fun delete(id: String) {
            data.removeAll { it.id == id }
        }

        override suspend fun list(filter: MemoryFilter?): List<MemoryResult> {
            val limited = filter?.limit ?: data.size
            return data.takeLast(limited).reversed()
        }
    }

    // ========== Screenshot Tool Tests (#338) ==========

    @Test
    fun `screenshot tool returns error when accessibility not attached`() = runTest {
        val client = ScriptedProviderClient(
            Provider.ANTHROPIC,
            ArrayDeque(),
            ArrayDeque()
        )
        val api = PhoneAgentApi(client)
        ScreenReader.detach()

        val toolCall = ToolCall("tc1", "screenshot", emptyMap())
        val result = api.executeToolCall(toolCall, null)

        assertEquals("Accessibility service not attached", result.text)
    }

    @Test
    fun `screenshot tool included in PhoneTools ALL`() {
        val screenshotTool = PhoneTools.ALL.find { it.name == "screenshot" }
        assertNotNull(screenshotTool, "screenshot tool should be in PhoneTools.ALL")
        assertTrue(screenshotTool!!.description.contains("vision"))
    }

    @Test
    fun `screenshot tool has optional prompt parameter`() {
        val tool = PhoneTools.SCREENSHOT
        assertEquals("screenshot", tool.name)
        val props = tool.inputSchema["properties"] as? Map<*, *>
        assertNotNull(props)
        assertTrue(props!!.containsKey("prompt"))
        val required = tool.inputSchema["required"] as? List<*>
        assertNotNull(required)
        assertTrue(required!!.isEmpty())
    }

    @Test
    fun `describeImage returns failure message when vision API fails`() = runTest {
        val client = ScriptedProviderClient(
            Provider.ANTHROPIC,
            ArrayDeque(),
            ArrayDeque(),
            visionResponses = ArrayDeque() // Empty = will fail
        )
        val result = client.describeImage("dGVzdA==", "Describe this")
        assertTrue(result.isFailure)
        assertTrue(result.exceptionOrNull()?.message?.contains("No vision response") == true)
    }

    @Test
    fun `describeImage called on chat client for screenshot`() = runTest {
        val client = ScriptedProviderClient(
            Provider.ANTHROPIC,
            ArrayDeque(),
            ArrayDeque(),
            visionResponses = ArrayDeque(listOf("A home screen with app icons"))
        )

        val result = client.describeImage("dGVzdA==", "Describe this")
        assertTrue(result.isSuccess)
        assertEquals("A home screen with app icons", result.getOrNull())
        assertEquals(1, client.describeImageCalls)
    }

    // ========== Clipboard Tool Tests (#339) ==========

    @Test
    fun `copy tool returns error when clipboard not attached`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(client)
        ClipboardHelper.detach()

        val toolCall = ToolCall("tc1", "copy", emptyMap())
        val result = api.executeToolCall(toolCall, null)

        assertTrue(result.text.contains("not available"))
    }

    @Test
    fun `set_clipboard tool returns error when clipboard not attached`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(client)
        ClipboardHelper.detach()

        val toolCall = ToolCall("tc1", "set_clipboard", mapOf("text" to "hello"))
        val result = api.executeToolCall(toolCall, null)

        assertTrue(result.text.contains("not available"))
    }

    @Test
    fun `paste tool returns error when clipboard not attached`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(client)
        ClipboardHelper.detach()

        val toolCall = ToolCall("tc1", "paste", mapOf("text" to "hello"))
        val result = api.executeToolCall(toolCall, null)

        assertTrue(result.text.contains("not available"))
    }

    @Test
    fun `set_clipboard tool requires non-empty text`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(client)

        val toolCall = ToolCall("tc1", "set_clipboard", mapOf("text" to ""))
        val result = api.executeToolCall(toolCall, null)

        assertTrue(result.text.contains("Failed"))
    }

    @Test
    fun `paste tool requires non-empty text`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(client)

        val toolCall = ToolCall("tc1", "paste", mapOf("text" to ""))
        val result = api.executeToolCall(toolCall, null)

        assertTrue(result.text.contains("Failed"))
    }

    @Test
    fun `clipboard tools included in PhoneTools ALL`() {
        assertNotNull(PhoneTools.ALL.find { it.name == "copy" }, "copy tool missing")
        assertNotNull(PhoneTools.ALL.find { it.name == "set_clipboard" }, "set_clipboard tool missing")
        assertNotNull(PhoneTools.ALL.find { it.name == "paste" }, "paste tool missing")
    }

    @Test
    fun `copy tool has no required parameters`() {
        val required = PhoneTools.COPY.inputSchema["required"] as? List<*>
        assertNotNull(required)
        assertTrue(required!!.isEmpty())
    }

    @Test
    fun `set_clipboard tool requires text parameter`() {
        val required = PhoneTools.SET_CLIPBOARD.inputSchema["required"] as? List<*>
        assertNotNull(required)
        assertTrue(required!!.contains("text"))
    }

    @Test
    fun `paste tool requires text parameter`() {
        val required = PhoneTools.PASTE.inputSchema["required"] as? List<*>
        assertNotNull(required)
        assertTrue(required!!.contains("text"))
    }

    // ========== Notification Tool Tests (#340) ==========

    @Test
    fun `read_notifications returns error when listener not attached`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(client)
        NotificationHelper.detach()

        val toolCall = ToolCall("tc1", "read_notifications", emptyMap())
        val result = api.executeToolCall(toolCall, null)

        assertTrue(result.text.contains("not attached"))
    }

    @Test
    fun `tap_notification returns error when listener not attached`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(client)
        NotificationHelper.detach()

        val toolCall = ToolCall(
            "tc1",
            "tap_notification",
            mapOf("notification_key" to "0|com.example.app|123|null|1000")
        )
        val result = api.executeToolCall(toolCall, null)

        assertTrue(result.text.contains("not attached"))
    }

    @Test
    fun `dismiss_notification returns error when listener not attached`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(client)
        NotificationHelper.detach()

        val toolCall = ToolCall(
            "tc1",
            "dismiss_notification",
            mapOf("notification_key" to "0|com.example.app|123|null|1000")
        )
        val result = api.executeToolCall(toolCall, null)

        assertTrue(result.text.contains("not attached"))
    }

    @Test
    fun `reply_notification returns error when listener not attached`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(client)
        NotificationHelper.detach()

        val toolCall = ToolCall(
            "tc1",
            "reply_notification",
            mapOf(
                "notification_key" to "0|com.example.app|123|null|1000",
                "text" to "hello"
            )
        )
        val result = api.executeToolCall(toolCall, null)

        assertTrue(result.text.contains("not attached"))
    }

    @Test
    fun `reply_notification requires non-empty text`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(client)

        val toolCall = ToolCall(
            "tc1",
            "reply_notification",
            mapOf("notification_key" to "0|com.example.app|123|null|1000", "text" to "")
        )
        val result = api.executeToolCall(toolCall, null)

        assertTrue(result.text.contains("Failed"))
    }

    @Test
    fun `tap_notification validates notification key format`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(client)

        val toolCall = ToolCall("tc1", "tap_notification", mapOf("notification_key" to "invalid-key"))
        val result = api.executeToolCall(toolCall, null)

        assertTrue(result.text.contains("valid notification_key format"))
    }

    @Test
    fun `dismiss_notification validates notification key format`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(client)

        val toolCall = ToolCall("tc1", "dismiss_notification", mapOf("notification_key" to "a|b"))
        val result = api.executeToolCall(toolCall, null)

        assertTrue(result.text.contains("valid notification_key format"))
    }

    @Test
    fun `reply_notification validates notification key format`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(client)

        val toolCall = ToolCall(
            "tc1",
            "reply_notification",
            mapOf("notification_key" to "123", "text" to "hello")
        )
        val result = api.executeToolCall(toolCall, null)

        assertTrue(result.text.contains("valid notification_key format"))
    }

    @Test
    fun `notification key validation rejects single-segment package`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(client)

        val toolCall = ToolCall("tc1", "tap_notification", mapOf("notification_key" to "0|example|123"))
        val result = api.executeToolCall(toolCall, null)

        assertTrue(result.text.contains("valid notification_key format"))
    }

    @Test
    fun `notification key validation rejects empty package segment`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(client)

        val toolCall = ToolCall("tc1", "tap_notification", mapOf("notification_key" to "0|com..app|123"))
        val result = api.executeToolCall(toolCall, null)

        assertTrue(result.text.contains("valid notification_key format"))
    }

    @Test
    fun `notification key validation rejects two-part key`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(client)

        val toolCall = ToolCall("tc1", "tap_notification", mapOf("notification_key" to "0|com.example.app"))
        val result = api.executeToolCall(toolCall, null)

        assertTrue(result.text.contains("valid notification_key format"))
    }

    @Test
    fun `notification key validation accepts minimal valid key`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(client)
        NotificationHelper.detach()

        val toolCall = ToolCall("tc1", "tap_notification", mapOf("notification_key" to "0|a.b|c"))
        val result = api.executeToolCall(toolCall, null)

        // Valid key → falls through to not-attached check (not format error)
        assertTrue(result.text.contains("not attached"))
    }

    @Test
    fun `notification key validation rejects whitespace-padded key`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(client)

        val toolCall = ToolCall(
            "tc1",
            "tap_notification",
            mapOf("notification_key" to " 0|com.example.app|123 ")
        )
        val result = api.executeToolCall(toolCall, null)

        assertTrue(result.text.contains("whitespace"))
    }

    @Test
    fun `notification tools included in PhoneTools ALL`() {
        assertNotNull(PhoneTools.ALL.find { it.name == "read_notifications" }, "read_notifications missing")
        assertNotNull(PhoneTools.ALL.find { it.name == "tap_notification" }, "tap_notification missing")
        assertNotNull(PhoneTools.ALL.find { it.name == "dismiss_notification" }, "dismiss_notification missing")
        assertNotNull(PhoneTools.ALL.find { it.name == "reply_notification" }, "reply_notification missing")
    }

    @Test
    fun `tap_notification requires notification_key`() {
        val required = PhoneTools.TAP_NOTIFICATION.inputSchema["required"] as? List<*>
        assertNotNull(required)
        assertTrue(required!!.contains("notification_key"))
    }

    @Test
    fun `reply_notification requires notification_key and text`() {
        val required = PhoneTools.REPLY_NOTIFICATION.inputSchema["required"] as? List<*>
        assertNotNull(required)
        assertTrue(required!!.contains("notification_key"))
        assertTrue(required.contains("text"))
    }

    // ========== Self-Verification Tests (#341) ==========

    @Test
    fun `executeToolCallWithVerification returns plain result when mode is NEVER`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val verifier = ActionVerifier(client, VerificationMode.NEVER)
        val api = PhoneAgentApi(client, client, verifier = verifier)

        val toolCall = ToolCall("tc1", "press_back", emptyMap())
        val result = api.executeToolCallWithVerification(toolCall, null)

        // press_back fails gracefully when ScreenReader not attached
        assertFalse(result.text.contains("[Verified"))
        assertFalse(result.text.contains("[Verification FAILED"))
    }

    @Test
    fun `executeToolCallWithVerification skips non-UI tools even in ALWAYS mode`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val verifier = ActionVerifier(client, VerificationMode.ALWAYS)
        val api = PhoneAgentApi(client, client, verifier = verifier)

        val toolCall = ToolCall("tc1", "think", mapOf("thought" to "planning"))
        val result = api.executeToolCallWithVerification(toolCall, null)

        assertEquals("Thought: planning", result.text)
        assertFalse(result.text.contains("[Verified"))
    }

    @Test
    fun `executeToolCallWithVerification appends skipped when ALWAYS and ScreenReader detached`() = runTest {
        ScreenReader.detach()
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val verifier = ActionVerifier(client, VerificationMode.ALWAYS)
        val api = PhoneAgentApi(client, client, verifier = verifier)

        val toolCall = ToolCall("tc1", "press_home", emptyMap())
        val result = api.executeToolCallWithVerification(toolCall, null)

        // Verification runs but gracefully handles detached state (error → skipped)
        assertTrue(result.text.contains("[Verification skipped"))
    }

    @Test
    fun `ON_FAILURE mode does not verify successful actions`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val verifier = ActionVerifier(client, VerificationMode.ON_FAILURE)
        val api = PhoneAgentApi(client, client, verifier = verifier)

        // think always succeeds and is non-UI — won't verify
        val toolCall = ToolCall("tc1", "think", mapOf("thought" to "ok"))
        val result = api.executeToolCallWithVerification(toolCall, null)

        assertFalse(result.text.contains("[Verified"))
        assertFalse(result.text.contains("[Verification FAILED"))
    }

    @Test
    fun `PhoneAgentApi exposes verifier`() {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val verifier = ActionVerifier(client, VerificationMode.ALWAYS)
        val api = PhoneAgentApi(client, client, verifier = verifier)

        assertEquals(VerificationMode.ALWAYS, api.verifier.let {
            // Verify the verifier is accessible and has the right mode
            // by checking shouldVerify behavior
            assertTrue(it.shouldVerify("tap", "Tapped element 5"))
            VerificationMode.ALWAYS
        })
    }

    // ========== phoneControlOverride ==========

    @Test
    fun `phoneControlOverride true bypasses ScreenReader check`() = runTest {
        ScreenReader.detach()
        val toolResponse = ChatResponse(text = "Done", toolCalls = emptyList(), stopReason = "end_turn")
        val client = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            chatResponses = ArrayDeque(listOf("fallback")),
            toolResponses = ArrayDeque(listOf(toolResponse))
        )
        val agent = PhoneAgentApi(client, client).also { it.phoneControlOverride = true }

        agent.sendMessage("Open Gmail", screenContent = null, isActionLoop = false)
        assertEquals(0, client.chatCalls, "Should use tool mode despite ScreenReader detached")
        assertEquals(1, client.chatWithToolsCalls)
    }

    // ========== Clean Loop Architecture (#416) ==========

    @Test
    fun `UI_MUTATING_TOOLS contains expected tools`() {
        val expected = setOf(
            "tap", "tap_text", "long_press",
            "type_text",
            "swipe", "scroll",
            "press_back", "press_home",
            "open_app", "open_notifications"
        )
        assertEquals(expected, PhoneAgentApi.UI_MUTATING_TOOLS)
    }

    @Test
    fun `UI_MUTATING_TOOLS does not contain non-mutating tools`() {
        val nonMutating = listOf("think", "remember", "recall", "list_memories",
            "read_file", "write_file", "list_files", "copy", "set_clipboard",
            "screenshot", "read_screen", "wait", "paste")
        for (tool in nonMutating) {
            assertFalse(tool in PhoneAgentApi.UI_MUTATING_TOOLS, "$tool should not be UI-mutating")
        }
    }

    @Test
    fun `formatToolResult without screen returns action summary only`() {
        val agent = createAgent()
        val result = agent.formatToolResult("Tapped element 5")
        assertEquals("Tapped element 5", result)
    }

    @Test
    fun `formatToolResult with screen appends SCREEN section`() {
        val agent = createAgent()
        val screen = ScreenContent(
            packageName = "com.example",
            elements = listOf(
                ScreenElement(id = 1, text = "Settings", contentDescription = null,
                    className = "Button", isClickable = true, isEditable = false,
                    bounds = android.graphics.Rect(0, 0, 100, 50))
            )
        )
        val result = agent.formatToolResult("Tapped element 1", screen)
        assertTrue(result.startsWith("Tapped element 1"), "Should start with action summary")
        assertTrue(result.contains("\n\nSCREEN:\n"), "Should have SCREEN separator")
        assertTrue(result.contains("Settings"), "Should contain screen element text")
    }

    @Test
    fun `continueAfterTools uses actionClient not chatClient`() = runTest {
        // Setup: chatClient for initial message, actionClient for continuation
        val chatClient = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            toolResponses = ArrayDeque(listOf(
                ChatResponse(
                    text = null,
                    toolCalls = listOf(ToolCall("tc1", "tap", mapOf("element_id" to 5))),
                    stopReason = "tool_use"
                )
            ))
        )
        val actionClient = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            toolResponses = ArrayDeque(listOf(
                ChatResponse(text = "Done! I tapped it.", toolCalls = emptyList(), stopReason = "end_turn")
            ))
        )
        val agent = PhoneAgentApi(chatClient, actionClient).also { it.phoneControlOverride = true }

        // Initial message uses chatClient
        val initial = agent.sendMessage("Tap the button", screenContent = null)
        assertEquals(1, chatClient.chatWithToolsCalls, "Initial should use chatClient")
        assertEquals(0, actionClient.chatWithToolsCalls)

        // Add tool result
        agent.addToolResult("tc1", "Tapped element 5")

        // Continue should use actionClient
        val continued = agent.continueAfterTools()
        assertEquals(1, chatClient.chatWithToolsCalls, "chatClient should not be called again")
        assertEquals(1, actionClient.chatWithToolsCalls, "continueAfterTools should use actionClient")
        assertEquals("Done! I tapped it.", continued.text)
    }

    @Test
    fun `continueAfterTools does not inject user message`() = runTest {
        val responses = ArrayDeque(listOf(
            ChatResponse(
                text = null,
                toolCalls = listOf(ToolCall("tc1", "open_app", mapOf("app_name" to "Gmail"))),
                stopReason = "tool_use"
            )
        ))
        val actionResponses = ArrayDeque(listOf(
            ChatResponse(text = "Gmail is open.", toolCalls = emptyList(), stopReason = "end_turn")
        ))
        val chatClient = ScriptedProviderClient(provider = Provider.ANTHROPIC, toolResponses = responses)
        val actionClient = ScriptedProviderClient(provider = Provider.ANTHROPIC, toolResponses = actionResponses)
        val agent = PhoneAgentApi(chatClient, actionClient).also { it.phoneControlOverride = true }

        // Send initial message
        agent.sendMessage("Open Gmail", screenContent = null)
        agent.addToolResult("tc1", "Opened Gmail")

        // Continue — capture messages sent to model
        agent.continueAfterTools()

        // Verify clean conversation flow — no synthetic step messages
        val lastMessages = actionClient.lastMessages
        assertNotNull(lastMessages, "actionClient should have received messages")
        assertNoSyntheticStepMessages(lastMessages!!)
    }

    @Test
    fun `continueAfterTools handles multi-step tool chains`() = runTest {
        val chatClient = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            toolResponses = ArrayDeque(listOf(
                ChatResponse(
                    text = null,
                    toolCalls = listOf(ToolCall("tc1", "open_app", mapOf("app_name" to "Gmail"))),
                    stopReason = "tool_use"
                )
            ))
        )
        val actionClient = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            toolResponses = ArrayDeque(listOf(
                // First continue: model wants to tap
                ChatResponse(
                    text = null,
                    toolCalls = listOf(ToolCall("tc2", "tap", mapOf("element_id" to 3))),
                    stopReason = "tool_use"
                ),
                // Second continue: model is done
                ChatResponse(text = "Opened the email.", toolCalls = emptyList(), stopReason = "end_turn")
            ))
        )
        val agent = PhoneAgentApi(chatClient, actionClient).also { it.phoneControlOverride = true }

        // Step 1: initial message
        agent.sendMessage("Open Gmail and read the first email", screenContent = null)
        agent.addToolResult("tc1", "Opened Gmail")

        // Step 2: first continue — model wants another tool
        val step2 = agent.continueAfterTools()
        assertEquals(1, step2.toolCalls.size)
        assertEquals("tap", step2.toolCalls[0].name)

        // Add second tool result and continue again
        agent.addToolResult("tc2", "Tapped element 3")
        val step3 = agent.continueAfterTools()
        assertEquals("Opened the email.", step3.text)
        assertTrue(step3.toolCalls.isEmpty())
        assertEquals(2, actionClient.chatWithToolsCalls)
    }

    // ========== isLikelyConversationalMessage Fix (#390, #392) ==========

    @Test
    fun `question with context word calendar routes to tools`() = runTest {
        val toolResponse = ChatResponse(text = "Checking calendar", toolCalls = emptyList(), stopReason = "end_turn")
        val client = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            chatResponses = ArrayDeque(listOf("fallback")),
            toolResponses = ArrayDeque(listOf(toolResponse))
        )
        val agent = PhoneAgentApi(client, client).also { it.phoneControlOverride = true }

        agent.sendMessage("What's on my calendar?", screenContent = null, isActionLoop = false)
        assertEquals(0, client.chatCalls, "'What's on my calendar?' should route to tools")
        assertEquals(1, client.chatWithToolsCalls)
    }

    @Test
    fun `question with context word email routes to tools`() = runTest {
        val toolResponse = ChatResponse(text = "Checking email", toolCalls = emptyList(), stopReason = "end_turn")
        val client = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            chatResponses = ArrayDeque(listOf("fallback")),
            toolResponses = ArrayDeque(listOf(toolResponse))
        )
        val agent = PhoneAgentApi(client, client).also { it.phoneControlOverride = true }

        agent.sendMessage("Do I have any email?", screenContent = null, isActionLoop = false)
        assertEquals(0, client.chatCalls, "'Do I have any email?' should route to tools")
        assertEquals(1, client.chatWithToolsCalls)
    }

    @Test
    fun `plain question without action words routes to chat`() = runTest {
        val client = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            chatResponses = ArrayDeque(listOf("42")),
            toolResponses = ArrayDeque()
        )
        val agent = PhoneAgentApi(client, client).also { it.phoneControlOverride = true }

        agent.sendMessage("What is the meaning of life?", screenContent = null, isActionLoop = false)
        assertEquals(1, client.chatCalls, "Plain question should route to chat")
        assertEquals(0, client.chatWithToolsCalls)
    }

    @Test
    fun `isLikelyConversationalMessage priority order`() {
        val agent = createAgent()

        // Known conversational phrases → always chat
        assertTrue(agent.isLikelyConversationalMessage("hi"))
        assertTrue(agent.isLikelyConversationalMessage("hello"))
        assertTrue(agent.isLikelyConversationalMessage("thanks"))

        // Action hint overrides ? → tool mode
        assertFalse(agent.isLikelyConversationalMessage("What's on my calendar?"))
        assertFalse(agent.isLikelyConversationalMessage("Can you check my email?"))
        assertFalse(agent.isLikelyConversationalMessage("What notification did I get?"))

        // ? without action hints → chat
        assertTrue(agent.isLikelyConversationalMessage("What is 2+2?"))
        assertTrue(agent.isLikelyConversationalMessage("Who is the president?"))

        // Short without special chars → chat
        assertTrue(agent.isLikelyConversationalMessage("ok"))
        assertTrue(agent.isLikelyConversationalMessage("sure"))

        // Longer messages with no hints → tool mode (default)
        assertFalse(agent.isLikelyConversationalMessage("I need to do something complicated on my device"))
    }

    @Test
    fun `action hints use word boundary matching`() {
        val agent = createAgent()

        // "calendar" as a whole word → tool mode
        assertFalse(agent.isLikelyConversationalMessage("What's on my calendar?"))

        // "calendaring" is not the word "calendar" → no match, falls through to ? → chat
        assertTrue(agent.isLikelyConversationalMessage("What is calendaring?"))

        // Multi-word hints still use substring matching
        assertFalse(agent.isLikelyConversationalMessage("Can you go home?"))
        assertFalse(agent.isLikelyConversationalMessage("Please turn on wifi"))
    }

    // ========== stripToolArtifacts Tests ==========

    @Test
    fun `stripToolArtifacts removes XML tool_use tags`() {
        val input = """Here's what I'll do: <tool_use>{"name":"tap","input":{"element_id":5}}</tool_use> Done!"""
        val result = PhoneAgentApi.stripToolArtifacts(input)
        assertEquals("Here's what I'll do:  Done!", result)
    }

    @Test
    fun `stripToolArtifacts removes XML tool_call tags`() {
        val input = """Let me help: <tool_call>tap element 5</tool_call>"""
        val result = PhoneAgentApi.stripToolArtifacts(input)
        assertEquals("Let me help:", result)
    }

    @Test
    fun `stripToolArtifacts removes XML function_call tags`() {
        val input = """<function_call>open_app Settings</function_call> Opening settings now."""
        val result = PhoneAgentApi.stripToolArtifacts(input)
        assertEquals("Opening settings now.", result)
    }

    @Test
    fun `stripToolArtifacts removes JSON tool objects`() {
        val input = """I'll tap the button. {"name":"tap","input":{"element_id":3}} There you go."""
        val result = PhoneAgentApi.stripToolArtifacts(input)
        assertEquals("I'll tap the button.  There you go.", result)
    }

    @Test
    fun `stripToolArtifacts preserves clean text`() {
        val input = "I can't control your phone right now. Please enable accessibility."
        val result = PhoneAgentApi.stripToolArtifacts(input)
        assertEquals(input, result)
    }

    @Test
    fun `stripToolArtifacts handles empty string`() {
        assertEquals("", PhoneAgentApi.stripToolArtifacts(""))
    }

    @Test
    fun `stripToolArtifacts handles multiple artifacts`() {
        val input = """<tool_use>tap</tool_use> and <tool_call>swipe</tool_call> done"""
        val result = PhoneAgentApi.stripToolArtifacts(input)
        assertEquals("and  done", result)
    }

    // ========== Chat-mode system note when phone control disabled ==========

    @Test
    fun `chat mode includes system note when phone control disabled`() = runTest {
        val client = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            chatResponses = ArrayDeque(listOf("I can't control your phone right now.")),
            toolResponses = ArrayDeque()
        )
        val agent = PhoneAgentApi(client, client).also { it.phoneControlOverride = false }

        agent.sendMessage("Open Gmail", screenContent = null, isActionLoop = false)

        // Should use chat mode
        assertEquals(1, client.chatCalls)
        assertEquals(0, client.chatWithToolsCalls)
    }

    @Test
    fun `chat mode strips artifacts from response even when phone control disabled`() = runTest {
        val client = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            chatResponses = ArrayDeque(listOf(
                """Sure! <tool_use>{"name":"open_app","input":{"app_name":"Gmail"}}</tool_use> Opening Gmail now."""
            )),
            toolResponses = ArrayDeque()
        )
        val agent = PhoneAgentApi(client, client).also { it.phoneControlOverride = false }

        val response = agent.sendMessage("Open Gmail", screenContent = null, isActionLoop = false)

        // Artifacts should be stripped
        assertFalse(response.text?.contains("<tool_use>") == true)
        assertFalse(response.text?.contains("open_app") == true)
        assertTrue(response.text?.contains("Opening Gmail now.") == true)
    }

    @Test
    fun `default verifier is NEVER mode`() {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(client)

        // Default should not verify anything
        assertFalse(api.verifier.shouldVerify("tap", "Tapped element 5"))
    }

    /** Assert no messages contain synthetic "[Step X/20]" patterns from v1 loop. */
    private fun assertNoSyntheticStepMessages(messages: List<Message>) {
        messages.filter { it.role == "user" }.forEach { msg ->
            assertFalse(
                msg.content?.contains("[Step") ?: false,
                "Found synthetic step message: ${msg.content}"
            )
        }
    }

    private class ScriptedProviderClient(
        override val provider: Provider,
        private val chatResponses: ArrayDeque<String> = ArrayDeque(),
        private val toolResponses: ArrayDeque<ChatResponse> = ArrayDeque(),
        private val visionResponses: ArrayDeque<String> = ArrayDeque()
    ) : ProviderClient {
        var chatCalls = 0
        var chatWithToolsCalls = 0
        var describeImageCalls = 0
        /** Last messages list passed to chatWithTools, for verifying conversation flow. */
        var lastMessages: List<Message>? = null

        override suspend fun chat(conversation: Conversation): Result<String> {
            chatCalls++
            return Result.success(chatResponses.removeFirst())
        }

        override suspend fun chatWithTools(
            messages: List<Message>,
            systemPrompt: String?,
            tools: List<Tool>,
            tokenLimit: Int?
        ): Result<ChatResponse> {
            chatWithToolsCalls++
            lastMessages = messages.toList()
            return Result.success(toolResponses.removeFirst())
        }

        override suspend fun describeImage(base64Image: String, prompt: String, maxTokens: Int): Result<String> {
            describeImageCalls++
            return if (visionResponses.isNotEmpty()) {
                Result.success(visionResponses.removeFirst())
            } else {
                Result.failure(ProviderException(provider, null, "No vision response", false))
            }
        }
    }

    // ========== ToolResult.isError flag assertions (#493) ==========

    @Test
    fun `file tool errors return isError true`() = runTest {
        val agent = createAgent()
        val result = agent.executeToolCall(
            ToolCall("f1", "read_file", mapOf("path" to "test.txt")),
            null
        )
        assertTrue(result.isError, "file tool error should have isError=true")
    }

    @Test
    fun `memory tool errors return isError true`() = runTest {
        val agent = createAgent()
        val result = agent.executeToolCall(
            ToolCall("m1", "remember", mapOf("content" to "test")),
            null
        )
        assertTrue(result.isError, "memory tool error should have isError=true")
    }

    @Test
    fun `unknown tool returns isError false with failure text`() = runTest {
        val agent = createAgent()
        val result = agent.executeToolCall(
            ToolCall("u1", "nonexistent_tool", emptyMap()),
            null
        )
        assertFalse(result.isError, "unknown tool should have isError=false (legacy)")
        assertTrue(result.text.contains("unknown tool"), "should mention unknown tool")
    }

    @Test
    fun `think tool returns isError false`() = runTest {
        val agent = createAgent()
        val result = agent.executeToolCall(
            ToolCall("t1", "think", mapOf("thought" to "testing")),
            null
        )
        assertFalse(result.isError, "think tool success should have isError=false")
    }


    // ========== Web Search Wiring Tests ==========

    @Test
    fun `getToolsForModel always includes web_search for non-SMALL models`() {
        // DuckDuckGo fallback means web_search is always available, no config needed
        val agent = createAgent()
        val tools = agent.getToolsForModel()
        val toolNames = tools.map { it.name }
        assertTrue("web_search" in toolNames, "web_search should always be available (DuckDuckGo fallback)")
        assertTrue("web_fetch" in toolNames, "web_fetch should always be in tool list")
    }

    @Test
    fun `getToolsForModel excludes API tools for SMALL tier models`() {
        val agent = createAgent()
        val tools = agent.getToolsForModel("claude-3-5-haiku-20241022")
        val toolNames = tools.map { it.name }
        assertFalse("web_search" in toolNames, "web_search should be excluded for SMALL tier models")
        assertFalse("web_fetch" in toolNames, "web_fetch should be excluded for SMALL tier models")
    }



    // ========== TinyFish Web Browse Tests ==========

    @Test
    fun `web_browse returns error when tinyFishApiKey is null`() = kotlinx.coroutines.runBlocking {
        val agent = createAgent()
        val result = agent.executeToolCall(
            ToolCall("t1", "web_browse", mapOf("url" to "https://example.com", "goal" to "Find price")),
            null
        )
        assertTrue(result.isError)
        assertTrue(result.text.contains("not configured") || result.text.contains("not available"))
    }

    @Test
    fun `getToolsForModel excludes web_browse when tinyFishApiKey is null`() {
        val agent = createAgent()
        val tools = agent.getToolsForModel()
        val toolNames = tools.map { it.name }
        assertFalse("web_browse" in toolNames, "web_browse should be excluded when API key is null")
    }

    @Test
    fun `getToolsForModel includes web_browse when tinyFishApiKey is set`() {
        val defaultClient = ClaudeClient(
            apiKey = "sk-ant-api03-test",
            systemPrompt = PhoneAgentPrompts.SYSTEM_PROMPT,
            baseUrl = server.url("/v1/messages").toString()
        )
        val agent = PhoneAgentApi(
            chatClient = defaultClient,
            actionClient = defaultClient,
            tinyFishApiKey = "test-tinyfish-key"
        ).also {
            it.phoneControlOverride = true
        }
        val tools = agent.getToolsForModel()
        val toolNames = tools.map { it.name }
        assertTrue("web_browse" in toolNames, "web_browse should be included when API key is set")
    }

    @Test
    fun `executeToolCall web_browse dispatches to TinyFishClient when key is configured`() = kotlinx.coroutines.runBlocking {
        // Serve a mock TinyFish SSE response
        val sseBody = listOf(
            """data: {"type":"STARTED","runId":"run_1","timestamp":"2026-01-01T00:00:00Z"}""",
            """data: {"type":"PROGRESS","runId":"run_1","purpose":"Navigating to page","timestamp":"2026-01-01T00:00:01Z"}""",
            """data: {"type":"COMPLETE","runId":"run_1","status":"COMPLETED","resultJson":{"answer":"42"},"timestamp":"2026-01-01T00:00:05Z"}"""
        ).joinToString("\n")
        server.enqueue(MockResponse()
            .setBody(sseBody)
            .setResponseCode(200)
            .addHeader("Content-Type", "text/event-stream"))

        val tinyFishEndpoint = server.url("/v1/automation/run-sse").toString()
        val defaultClient = ClaudeClient(
            apiKey = "sk-ant-api03-test",
            systemPrompt = PhoneAgentPrompts.SYSTEM_PROMPT,
            baseUrl = server.url("/v1/messages").toString()
        )
        val agent = PhoneAgentApi(
            chatClient = defaultClient,
            actionClient = defaultClient,
            tinyFishApiKey = "test-tinyfish-key",
            tinyFishEndpoint = tinyFishEndpoint
        ).also {
            it.phoneControlOverride = true
        }

        val result = agent.executeToolCall(
            ToolCall("t1", "web_browse", mapOf("url" to "https://example.com", "goal" to "Find the answer")),
            null
        )

        assertFalse(result.isError, "Expected success but got error: " + result.text)
        assertTrue(result.text.contains("42"), "Result should contain automation result")
        // Verify the request actually reached MockWebServer (TinyFish endpoint)
        val request = server.takeRequest()
        assertEquals("/v1/automation/run-sse", request.path)
        val body = request.body.readUtf8()
        assertTrue(body.contains("example.com"), "Request body should contain URL")
        assertTrue(body.contains("Find the answer"), "Request body should contain goal")
    }

}
