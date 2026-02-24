package ai.citros.core

import android.content.ClipboardManager
import android.content.Context
import android.graphics.Rect
import android.service.notification.NotificationListenerService
import android.service.notification.StatusBarNotification
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.async
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.After
import org.junit.Before
import org.junit.Test
import java.util.concurrent.atomic.AtomicInteger
import org.mockito.kotlin.any
import org.mockito.kotlin.doNothing
import org.mockito.kotlin.doThrow
import org.mockito.kotlin.mock
import org.mockito.kotlin.whenever
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlin.system.measureTimeMillis
import kotlin.time.Duration.Companion.milliseconds

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
        memoryProvider: MemoryProvider? = null,
        sensorProvider: SensorProvider? = null
    ): PhoneAgentApi {
        val defaultClient = ClaudeClient(
            apiKey = "sk-ant-api03-test",
            systemPrompt = PhoneAgentPrompts.SYSTEM_PROMPT,
            baseUrl = server.url("/v1/messages").toString()
        )
        val resolvedChat = chatClient ?: defaultClient
        val resolvedAction = actionClient ?: resolvedChat
        return PhoneAgentApi(
            chatClient = resolvedChat,
            actionClient = resolvedAction,
            agentFileManager = fileManager,
            memoryProvider = memoryProvider,
            sensorProvider = sensorProvider
        ).also {
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
    fun `explicit fetch intent injects web_fetch when model returns end_turn`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"content":[{"type":"text","text":"I can summarize that."}],"role":"assistant","stop_reason":"end_turn"}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val agent = createAgent()
        val response = agent.sendMessage("Fetch https://docs.openclaw.ai and summarize.", null, isActionLoop = false)

        assertEquals("tool_use", response.stopReason)
        val webFetch = response.toolCalls.firstOrNull { it.name == "web_fetch" }
        assertNotNull(webFetch)
        assertEquals("https://docs.openclaw.ai", webFetch.input["url"])
    }

    @Test
    fun `explicit fetch intent appends web_fetch when model only emits think`() = runTest {
        server.enqueue(MockResponse()
            .setBody(
                """{"content":[{"type":"tool_use","id":"think_1","name":"think","input":{"thought":"Need page contents first"}}],"role":"assistant","stop_reason":"tool_use"}"""
            )
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val agent = createAgent()
        val response = agent.sendMessage("Fetch https://docs.openclaw.ai and summarize.", null, isActionLoop = false)

        assertEquals("tool_use", response.stopReason)
        assertTrue(response.toolCalls.any { it.name == "think" })
        val webFetch = response.toolCalls.firstOrNull { it.name == "web_fetch" }
        assertNotNull(webFetch)
        assertEquals("https://docs.openclaw.ai", webFetch.input["url"])
    }

    @Test
    fun `explicit fetch intent does not inject when model already returned web_fetch`() = runTest {
        val modelResponse = ChatResponse(
            text = null,
            toolCalls = listOf(
                ToolCall(
                    id = "wf_existing",
                    name = "web_fetch",
                    input = mapOf("url" to "https://docs.openclaw.ai")
                )
            ),
            stopReason = "tool_use"
        )
        val client = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            toolResponses = ArrayDeque(listOf(modelResponse))
        )
        val agent = PhoneAgentApi(client, client).also { it.phoneControlOverride = true }

        val response = agent.sendMessage("Fetch https://docs.openclaw.ai and summarize.", null, isActionLoop = false)

        assertEquals(modelResponse, response)
        assertEquals(1, response.toolCalls.count { it.name == "web_fetch" })
        assertEquals("wf_existing", response.toolCalls.single().id)
    }

    @Test
    fun `small tier sendMessage passes model-aware tools to API`() = runTest {
        val smallTierClient = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            modelId = "claude-3-5-haiku-20241022",
            toolResponses = ArrayDeque(
                listOf(
                    ChatResponse(
                        text = "I can summarize that.",
                        toolCalls = emptyList(),
                        stopReason = "end_turn"
                    )
                )
            )
        )
        val agent = PhoneAgentApi(smallTierClient, smallTierClient).also { it.phoneControlOverride = true }

        val response = agent.sendMessage(
            "Fetch https://docs.openclaw.ai and summarize.",
            null,
            isActionLoop = false
        )

        val toolNamesPassedToApi = smallTierClient.lastTools.orEmpty().map { it.name }.toSet()
        assertFalse("web_fetch" in toolNamesPassedToApi)
        assertFalse("web_search" in toolNamesPassedToApi)
        assertTrue("tap" in toolNamesPassedToApi)
        assertTrue(response.toolCalls.isEmpty())
        assertEquals("end_turn", response.stopReason)
    }

    @Test
    fun `small tier continueAfterTools passes model-aware tools to API`() = runTest {
        val chatClient = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            modelId = "claude-sonnet-4-5-20250929",
            toolResponses = ArrayDeque(
                listOf(
                    ChatResponse(
                        text = null,
                        toolCalls = listOf(ToolCall("t1", "tap", mapOf("x" to 10, "y" to 20))),
                        stopReason = "tool_use"
                    )
                )
            )
        )
        val actionClient = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            modelId = "claude-3-5-haiku-20241022",
            toolResponses = ArrayDeque(
                listOf(
                    ChatResponse(
                        text = "Done",
                        toolCalls = emptyList(),
                        stopReason = "end_turn"
                    )
                )
            )
        )
        val agent = PhoneAgentApi(chatClient = chatClient, actionClient = actionClient).also {
            it.phoneControlOverride = true
        }

        val first = agent.sendMessage("Tap submit", null, isActionLoop = false)
        assertEquals("tool_use", first.stopReason)

        val continuation = agent.continueAfterTools()
        val toolNamesPassedToApi = actionClient.lastTools.orEmpty().map { it.name }.toSet()

        assertEquals("end_turn", continuation.stopReason)
        assertEquals(1, actionClient.chatWithToolsCalls)
        assertFalse("web_fetch" in toolNamesPassedToApi)
        assertFalse("web_search" in toolNamesPassedToApi)
        assertTrue("tap" in toolNamesPassedToApi)
    }

    @Test
    fun `assertSafetyContractDebug via PhoneAgentApi throws for invalid prompt in debug build`() {
        FeatureFlags.promptTuningV1Enabled = true
        try {
            val agent = createAgent().also { it.isDebugBuild = true }
            val method = PhoneAgentApi::class.java.getDeclaredMethod("assertSafetyContractDebug", String::class.java)
            method.isAccessible = true

            val error = assertFailsWith<java.lang.reflect.InvocationTargetException> {
                method.invoke(agent, "You are Citros. No safety clauses here.")
            }
            assertTrue(error.cause is AssertionError)
        } finally {
            FeatureFlags.resetToDefaults()
        }
    }

    @Test
    fun `assertSafetyContractDebug via PhoneAgentApi passes for valid prompt in debug build`() {
        FeatureFlags.promptTuningV1Enabled = true
        try {
            val agent = createAgent().also { it.isDebugBuild = true }
            val method = PhoneAgentApi::class.java.getDeclaredMethod("assertSafetyContractDebug", String::class.java)
            method.isAccessible = true

            val validPrompt = PhoneAgentPrompts.buildSystemPrompt(
                phoneControlAvailable = true,
                modelName = "gpt-4o"
            )
            method.invoke(agent, validPrompt)
        } finally {
            FeatureFlags.resetToDefaults()
        }
    }

    @Test
    fun `logPromptTuningMetadata tolerates missing runtime line`() {
        FeatureFlags.promptTuningV1Enabled = true
        try {
            val agent = createAgent()
            val method = PhoneAgentApi::class.java.getDeclaredMethod("logPromptTuningMetadata", String::class.java, String::class.java)
            method.isAccessible = true

            method.invoke(agent, "prompt without structured runtime line", "action")
        } finally {
            FeatureFlags.resetToDefaults()
        }
    }

    @Test
    fun `url mention without fetch intent does not inject web_fetch`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"content":[{"type":"text","text":"That site contains OpenClaw docs."}],"role":"assistant","stop_reason":"end_turn"}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val agent = createAgent()
        val response = agent.sendMessage("What is https://docs.openclaw.ai?", null, isActionLoop = false)

        assertTrue(response.toolCalls.isEmpty())
        assertEquals("end_turn", response.stopReason)
    }

    @Test
    fun `explicit fetch intent does not inject during action loop`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"content":[{"type":"text","text":"I can summarize that."}],"role":"assistant","stop_reason":"end_turn"}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val agent = createAgent()
        val response = agent.sendMessage(
            "Fetch https://docs.openclaw.ai and summarize.",
            null,
            isActionLoop = true
        )

        assertTrue(response.toolCalls.isEmpty())
        assertEquals("end_turn", response.stopReason)
    }

    @Test
    fun `explicit fetch intent does not inject when web_fetch tool unavailable`() = runTest {
        val smallTierClient = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            modelId = "claude-3-5-haiku-20241022",
            toolResponses = ArrayDeque(
                listOf(
                    ChatResponse(
                        text = "I can summarize that.",
                        toolCalls = emptyList(),
                        stopReason = "end_turn"
                    )
                )
            )
        )
        val agent = PhoneAgentApi(smallTierClient, smallTierClient).also { it.phoneControlOverride = true }

        val response = agent.sendMessage(
            "Fetch https://docs.openclaw.ai and summarize.",
            null,
            isActionLoop = false
        )

        assertTrue(response.toolCalls.isEmpty())
        assertEquals("end_turn", response.stopReason)
    }

    @Test
    fun `explicit fetch intent does not modify actionable non-think tool responses`() = runTest {
        val client = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            toolResponses = ArrayDeque(
                listOf(
                    ChatResponse(
                        text = null,
                        toolCalls = listOf(
                            ToolCall("ws1", "web_search", mapOf("query" to "openclaw docs"))
                        ),
                        stopReason = "tool_use"
                    )
                )
            )
        )
        val agent = PhoneAgentApi(client, client).also { it.phoneControlOverride = true }

        val response = agent.sendMessage(
            "Fetch https://docs.openclaw.ai and summarize.",
            null,
            isActionLoop = false
        )

        assertEquals(1, response.toolCalls.size)
        assertEquals("web_search", response.toolCalls.single().name)
        assertFalse(response.toolCalls.any { it.name == "web_fetch" })
        assertEquals("tool_use", response.stopReason)
    }

    @Test
    fun `extractExplicitWebFetchUrl trims trailing punctuation`() {
        val agent = createAgent()
        assertEquals(
            "https://docs.openclaw.ai/path",
            agent.extractExplicitWebFetchUrl("Fetch https://docs.openclaw.ai/path).")
        )
        assertEquals(
            "https://docs.openclaw.ai/path",
            agent.extractExplicitWebFetchUrl("Please summarize https://docs.openclaw.ai/path],")
        )
    }

    @Test
    fun `extractExplicitWebFetchUrl rejects no-url and no-intent inputs`() {
        val agent = createAgent()
        assertNull(agent.extractExplicitWebFetchUrl("Fetch this page please"))
        assertNull(agent.extractExplicitWebFetchUrl("https://docs.openclaw.ai"))
        assertNull(agent.extractExplicitWebFetchUrl("I read https://docs.openclaw.ai yesterday"))
    }

    @Test
    fun `extractExplicitWebFetchUrl requires explicit read object to avoid generic over-trigger`() {
        val agent = createAgent()
        assertNull(agent.extractExplicitWebFetchUrl("I like to read https://docs.openclaw.ai"))
        assertEquals(
            "https://docs.openclaw.ai",
            agent.extractExplicitWebFetchUrl("Read this URL https://docs.openclaw.ai")
        )
        assertEquals(
            "https://docs.openclaw.ai",
            agent.extractExplicitWebFetchUrl("https://docs.openclaw.ai and read it")
        )
    }

    @Test
    fun `forced web_fetch id is stable and collision-resistant hex digest`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"content":[{"type":"text","text":"Will fetch."}],"role":"assistant","stop_reason":"end_turn"}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))
        server.enqueue(MockResponse()
            .setBody("""{"content":[{"type":"text","text":"Will fetch."}],"role":"assistant","stop_reason":"end_turn"}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))
        server.enqueue(MockResponse()
            .setBody("""{"content":[{"type":"text","text":"Will fetch."}],"role":"assistant","stop_reason":"end_turn"}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val agent = createAgent()
        val first = agent.sendMessage("Fetch https://docs.openclaw.ai and summarize.", null, false)
        val second = agent.sendMessage("Fetch https://docs.openclaw.ai and summarize.", null, false)
        val third = agent.sendMessage("Fetch https://example.com and summarize.", null, false)

        val id1 = first.toolCalls.single { it.name == "web_fetch" }.id
        val id2 = second.toolCalls.single { it.name == "web_fetch" }.id
        val id3 = third.toolCalls.single { it.name == "web_fetch" }.id

        assertEquals(id1, id2)
        assertTrue(id1.matches(Regex("forced_web_fetch_[0-9a-f]{24}")))
        assertTrue(id3.matches(Regex("forced_web_fetch_[0-9a-f]{24}")))
        assertFalse(id1 == id3)
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
    fun `executeToolCall tap returns explicit privacy blocked reason`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            clickElement = { ScreenReader.ElementActionResult.PrivacyBlocked }
        )

        val result = api.executeToolCall(ToolCall("t1", "tap", mapOf("element_id" to 5)), null)

        assertTrue(result.isError)
        assertEquals("Failed: tap: blocked by privacy mode for private_app", result.text)
        assertEquals(ToolErrorCode.PRIVACY_BLOCKED, result.errorCode)
        assertFalse(result.text.contains("com.bank.app"))
    }

    @Test
    fun `executeToolCall tap reports cause-accurate failures`() = runTest {
        val cases = listOf(
            ScreenReader.ElementActionResult.ServiceUnavailable to
                "Failed: tap: accessibility service unavailable",
            ScreenReader.ElementActionResult.GestureDispatchFailed to
                "Failed: tap: gesture dispatch failed",
            ScreenReader.ElementActionResult.ElementNotFound to
                "Failed: tap: element 5 not found"
        )

        for ((actionResult, expected) in cases) {
            val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
            val api = PhoneAgentApi(
                chatClient = client,
                actionClient = client,
                clickElement = { actionResult }
            )

            val result = api.executeToolCall(ToolCall("t1", "tap", mapOf("element_id" to 5)), null)

            assertTrue(result.isError)
            assertEquals(expected, result.text)
        }
    }

    @Test
    fun `executeToolCall long_press returns explicit privacy blocked reason`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            longPressElement = { ScreenReader.ElementActionResult.PrivacyBlocked }
        )

        val result = api.executeToolCall(ToolCall("t1", "long_press", mapOf("element_id" to 2)), null)

        assertTrue(result.isError)
        assertEquals("Failed: long_press: blocked by privacy mode for private_app", result.text)
        assertEquals(ToolErrorCode.PRIVACY_BLOCKED, result.errorCode)
        assertFalse(result.text.contains("com.bank.app"))
    }

    @Test
    fun `executeToolCall long_press reports cause-accurate failures`() = runTest {
        val cases = listOf(
            ScreenReader.ElementActionResult.ServiceUnavailable to
                "Failed: long_press: accessibility service unavailable",
            ScreenReader.ElementActionResult.GestureDispatchFailed to
                "Failed: long_press: gesture dispatch failed",
            ScreenReader.ElementActionResult.ElementNotFound to
                "Failed: long_press: element 2 not found"
        )

        for ((actionResult, expected) in cases) {
            val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
            val api = PhoneAgentApi(
                chatClient = client,
                actionClient = client,
                longPressElement = { actionResult }
            )

            val result = api.executeToolCall(ToolCall("t1", "long_press", mapOf("element_id" to 2)), null)

            assertTrue(result.isError)
            assertEquals(expected, result.text)
        }
    }

    @Test
    fun `executeToolCall tap_text reports cause-accurate failures`() = runTest {
        val screen = ScreenContent(
            packageName = "com.example.app",
            elements = listOf(
                ScreenElement(
                    id = 7,
                    text = "Settings",
                    contentDescription = null,
                    className = "android.widget.TextView",
                    isClickable = true,
                    isEditable = false,
                    bounds = Rect(0, 0, 10, 10)
                )
            )
        )

        val cases = listOf(
            ScreenReader.ElementActionResult.ServiceUnavailable to
                "Failed: tap_text: accessibility service unavailable",
            ScreenReader.ElementActionResult.GestureDispatchFailed to
                "Failed: tap_text: gesture dispatch failed",
            ScreenReader.ElementActionResult.PrivacyBlocked to
                "Failed: tap_text: blocked by privacy mode for private_app",
            ScreenReader.ElementActionResult.ElementNotFound to
                "Failed: tap_text: no element matching \"Settings\""
        )

        for ((actionResult, expected) in cases) {
            val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
            val api = PhoneAgentApi(
                chatClient = client,
                actionClient = client,
                clickElement = { actionResult }
            )
            val result = api.executeToolCall(
                ToolCall("tc1", "tap_text", mapOf("text" to "Settings")),
                screen
            )
            assertEquals(expected, result.text)
            assertTrue(result.isError)
            val expectedCode = if (actionResult is ScreenReader.ElementActionResult.PrivacyBlocked) {
                ToolErrorCode.PRIVACY_BLOCKED
            } else {
                ToolErrorCode.EXECUTION_FAILED
            }
            assertEquals(expectedCode, result.errorCode)
        }
    }

    @Test
    fun `executeToolCall tap_text returns privacy blocked when screen content is privacy mode`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            clickElement = { ScreenReader.ElementActionResult.Success }
        )
        val privacyScreen = ScreenContent(
            packageName = PrivacyRedaction.APP_PLACEHOLDER,
            elements = emptyList(),
            privacyMode = true
        )

        val result = api.executeToolCall(
            ToolCall("tc1", "tap_text", mapOf("text" to "Settings")),
            privacyScreen
        )

        assertTrue(result.isError)
        assertEquals("Failed: tap_text: blocked by privacy mode for private_app", result.text)
        assertEquals(ToolErrorCode.PRIVACY_BLOCKED, result.errorCode)
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
    fun `continue after interruption pause context routes to tool mode`() = runTest {
        val toolClient = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            chatResponses = ArrayDeque(listOf("chat fallback")),
            toolResponses = ArrayDeque(
                listOf(
                    ChatResponse(
                        text = "Resuming the task",
                        toolCalls = listOf(ToolCall("tc1", "tap", mapOf("element_id" to 1))),
                        stopReason = "tool_use"
                    )
                )
            )
        )
        val agent = PhoneAgentApi(toolClient, toolClient).also { it.phoneControlOverride = true }
        agent.seedConversationHistory(
            listOf(
                Message(
                    role = Message.ROLE_USER,
                    content =
                        "[SYSTEM: User switched from com.mail to com.calendar. " +
                            "$INTERRUPTION_RESUME_MARKER]"
                ),
                Message(
                    role = Message.ROLE_ASSISTANT,
                    content = "You switched apps. Should I continue or cancel?"
                )
            )
        )

        val response = agent.sendMessage("continue", screenContent = null, isActionLoop = false)

        assertEquals(0, toolClient.chatCalls, "continue should bypass chat mode when interruption pause context exists")
        assertEquals(1, toolClient.chatWithToolsCalls, "continue should re-enter actionable mode")
        assertEquals("tool_use", response.stopReason)
        assertTrue(response.toolCalls.isNotEmpty())
    }

    @Test
    fun `resume after interruption pause context routes to tool mode`() = runTest {
        val toolClient = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            chatResponses = ArrayDeque(listOf("chat fallback")),
            toolResponses = ArrayDeque(
                listOf(
                    ChatResponse(
                        text = "Resuming now",
                        toolCalls = listOf(ToolCall("tc2", "press_back", emptyMap())),
                        stopReason = "tool_use"
                    )
                )
            )
        )
        val agent = PhoneAgentApi(toolClient, toolClient).also { it.phoneControlOverride = true }
        agent.seedConversationHistory(
            listOf(
                Message(
                    role = Message.ROLE_USER,
                    content =
                        "[SYSTEM: User switched from com.mail to com.calendar. " +
                            "$INTERRUPTION_RESUME_MARKER]"
                ),
                Message(
                    role = Message.ROLE_ASSISTANT,
                    content = "Want me to resume?"
                )
            )
        )

        val response = agent.sendMessage("resume please", screenContent = null, isActionLoop = false)

        assertEquals(0, toolClient.chatCalls)
        assertEquals(1, toolClient.chatWithToolsCalls)
        assertEquals("tool_use", response.stopReason)
    }

    @Test
    fun `continue after interruption cancel does not re-enter tool mode`() = runTest {
        val client = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            chatResponses = ArrayDeque(listOf("Canceled already — what next?")),
            toolResponses = ArrayDeque(
                listOf(
                    ChatResponse(
                        text = "Should not run tools",
                        toolCalls = listOf(ToolCall("tc_cancel", "tap", mapOf("element_id" to 9))),
                        stopReason = "tool_use"
                    )
                )
            )
        )
        val agent = PhoneAgentApi(client, client).also { it.phoneControlOverride = true }
        agent.seedConversationHistory(
            listOf(
                Message(
                    role = Message.ROLE_USER,
                    content =
                        "[SYSTEM: User switched from com.mail to com.calendar. " +
                            "$INTERRUPTION_RESUME_MARKER]"
                ),
                Message(role = Message.ROLE_ASSISTANT, content = "You switched apps. Should I continue or cancel?"),
                Message(role = Message.ROLE_USER, content = "cancel"),
                Message(role = Message.ROLE_ASSISTANT, content = "Okay, canceled.")
            )
        )

        val response = agent.sendMessage("continue", screenContent = null, isActionLoop = false)

        assertEquals("Canceled already — what next?", response.text)
        assertTrue(response.toolCalls.isEmpty())
        assertEquals(1, client.chatCalls)
        assertEquals(0, client.chatWithToolsCalls)
    }

    @Test
    fun `continue with interruption marker outside recent window stays chat mode`() = runTest {
        val client = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            chatResponses = ArrayDeque(listOf("Continue what?")),
            toolResponses = ArrayDeque(
                listOf(
                    ChatResponse(
                        text = "Should not run tools",
                        toolCalls = listOf(ToolCall("tc_old", "tap", mapOf("element_id" to 1))),
                        stopReason = "tool_use"
                    )
                )
            )
        )
        val agent = PhoneAgentApi(client, client).also { it.phoneControlOverride = true }
        val history = mutableListOf<Message>(
            Message(
                role = Message.ROLE_USER,
                content =
                    "[SYSTEM: User switched from com.mail to com.calendar. " +
                        "$INTERRUPTION_RESUME_MARKER]"
            ),
            Message(role = Message.ROLE_ASSISTANT, content = "Should I continue?")
        )
        repeat(6) { idx ->
            history += Message(role = Message.ROLE_USER, content = "user filler $idx")
            history += Message(role = Message.ROLE_ASSISTANT, content = "assistant filler $idx")
        }
        agent.seedConversationHistory(history)

        val response = agent.sendMessage("continue", screenContent = null, isActionLoop = false)

        assertEquals("Continue what?", response.text)
        assertTrue(response.toolCalls.isEmpty())
        assertEquals(1, client.chatCalls)
        assertEquals(0, client.chatWithToolsCalls)
    }

    @Test
    fun `continue without interruption context stays chat mode`() = runTest {
        val chatClient = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            chatResponses = ArrayDeque(listOf("Continue what?")),
            toolResponses = ArrayDeque(
                listOf(
                    ChatResponse(
                        text = "Should not run tools",
                        toolCalls = listOf(ToolCall("tc3", "tap", mapOf("element_id" to 9))),
                        stopReason = "tool_use"
                    )
                )
            )
        )
        val agent = PhoneAgentApi(chatClient, chatClient).also { it.phoneControlOverride = true }

        val response = agent.sendMessage("continue", screenContent = null, isActionLoop = false)

        assertEquals("Continue what?", response.text)
        assertTrue(response.toolCalls.isEmpty())
        assertEquals(1, chatClient.chatCalls)
        assertEquals(0, chatClient.chatWithToolsCalls)
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

    private class EmptyNotificationListenerService : NotificationListenerService() {
        override fun getActiveNotifications(): Array<StatusBarNotification> = emptyArray()
    }

    private class AccessDeniedNotificationListenerService : NotificationListenerService() {
        override fun getActiveNotifications(): Array<StatusBarNotification> {
            throw SecurityException("Notification access denied")
        }
    }

    private fun cancelAccessDeniedNotificationListenerService(
        notificationKey: String
    ): NotificationListenerService {
        val service: NotificationListenerService = mock()
        val activeNotification: StatusBarNotification = mock()
        whenever(activeNotification.key).thenReturn(notificationKey)
        whenever(service.activeNotifications).thenReturn(arrayOf(activeNotification))
        doThrow(SecurityException("Notification access denied")).whenever(service).cancelNotification(any())
        return service
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
        assertTrue(result.isError)
    }

    @Test
    fun `screenshot tool returns privacy blocked error when screenshot is blocked`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            isScreenReaderAttached = { true },
            takeScreenshot = { ScreenshotResult.PrivacyBlocked }
        )

        val result = api.executeToolCall(ToolCall("tc1", "screenshot", emptyMap()), null)

        assertTrue(result.isError)
        assertEquals(ToolErrorCode.PRIVACY_BLOCKED, result.errorCode)
        assertEquals(
            "Failed: screenshot: blocked by privacy mode for private_app",
            result.text
        )
        assertFalse(result.text.contains("com.bank.app"))
    }

    @Test
    fun `screenshot tool returns explicit failure reason when screenshot capture fails`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            isScreenReaderAttached = { true },
            takeScreenshot = { ScreenshotResult.Failed("Screenshot capture failed") }
        )

        val result = api.executeToolCall(ToolCall("tc1", "screenshot", emptyMap()), null)

        assertTrue(result.isError)
        assertEquals(ToolErrorCode.EXECUTION_FAILED, result.errorCode)
        assertEquals("Failed: screenshot: Screenshot capture failed", result.text)
    }

    @Test
    fun `read_screen tool payload uses compactable Screen refreshed colon format`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            isScreenReaderAttached = { true },
            getScreenContent = {
                ScreenContent(
                    packageName = "com.example.app",
                    elements = listOf(
                        ScreenElement(
                            id = 1,
                            text = "Open",
                            contentDescription = null,
                            className = "Button",
                            isClickable = true,
                            isEditable = false,
                            bounds = android.graphics.Rect(0, 0, 100, 50)
                        )
                    )
                )
            }
        )

        val result = api.executeToolCall(ToolCall("tc1", "read_screen", emptyMap()), null)
        assertTrue(result.text.startsWith("Screen refreshed:\nSCREEN:\n"), result.text)
    }

    @Test
    fun `read_screen tool payload returns privacy marker and no raw element text when blocked`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            isScreenReaderAttached = { true },
            getScreenContent = {
                ScreenContent(
                    packageName = "com.bank.app",
                    elements = listOf(
                        ScreenElement(
                            id = 1,
                            text = "SECRET BALANCE",
                            contentDescription = null,
                            className = "TextView",
                            isClickable = false,
                            isEditable = false,
                            bounds = android.graphics.Rect(0, 0, 100, 50)
                        )
                    ),
                    privacyMode = true
                )
            }
        )

        val result = api.executeToolCall(ToolCall("tc1", "read_screen", emptyMap()), null)
        assertTrue(result.isError)
        assertEquals(ToolErrorCode.PRIVACY_BLOCKED, result.errorCode)
        assertTrue(result.text.contains("Privacy mode"), result.text)
        assertFalse(result.text.contains("SECRET BALANCE"), result.text)
        assertFalse(result.text.contains("com.bank.app"), result.text)
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

        assertTrue(result.isError)
        assertEquals(ToolErrorCode.SERVICE_UNAVAILABLE, result.errorCode)
        assertTrue(result.text.contains("not available"))
    }

    @Test
    fun `set_clipboard tool returns error when clipboard not attached`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(client)
        ClipboardHelper.detach()

        val toolCall = ToolCall("tc1", "set_clipboard", mapOf("text" to "hello"))
        val result = api.executeToolCall(toolCall, null)

        assertTrue(result.isError)
        assertEquals(ToolErrorCode.SERVICE_UNAVAILABLE, result.errorCode)
        assertTrue(result.text.contains("not available"))
    }

    @Test
    fun `paste tool returns error when clipboard not attached`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(client)
        ClipboardHelper.detach()

        val toolCall = ToolCall("tc1", "paste", mapOf("text" to "hello"))
        val result = api.executeToolCall(toolCall, null)

        assertTrue(result.isError)
        assertEquals(ToolErrorCode.SERVICE_UNAVAILABLE, result.errorCode)
        assertTrue(result.text.contains("not available"))
    }

    @Test
    fun `set_clipboard tool requires non-empty text`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(client)

        val toolCall = ToolCall("tc1", "set_clipboard", mapOf("text" to ""))
        val result = api.executeToolCall(toolCall, null)

        assertTrue(result.isError)
        assertEquals(ToolErrorCode.INVALID_INPUT, result.errorCode)
        assertTrue(result.text.contains("Failed"))
    }

    @Test
    fun `paste tool requires non-empty text`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(client)

        val toolCall = ToolCall("tc1", "paste", mapOf("text" to ""))
        val result = api.executeToolCall(toolCall, null)

        assertTrue(result.isError)
        assertEquals(ToolErrorCode.INVALID_INPUT, result.errorCode)
        assertTrue(result.text.contains("Failed"))
    }

    @Test
    fun `paste tool maps writeAndPaste failure to EXECUTION_FAILED`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(client)

        val clipboardManager = mock<ClipboardManager>()
        doNothing().whenever(clipboardManager).setPrimaryClip(any<android.content.ClipData>())
        val context = mock<Context>()
        whenever(context.applicationContext).thenReturn(context)
        whenever(context.getSystemService(Context.CLIPBOARD_SERVICE)).thenReturn(clipboardManager)

        ClipboardHelper.attach(context)
        ScreenReader.detach()

        try {
            val toolCall = ToolCall("tc1", "paste", mapOf("text" to "hello"))
            val result = api.executeToolCall(toolCall, null)

            assertTrue(result.isError)
            assertEquals(ToolErrorCode.EXECUTION_FAILED, result.errorCode)
            assertEquals(
                "Failed: paste: no focused input field or clipboard write failed",
                result.text
            )
        } finally {
            ClipboardHelper.detach()
        }
    }

    @Test
    fun `set_clipboard tool maps write failure to EXECUTION_FAILED`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(client)

        val clipboardManager = mock<ClipboardManager>()
        org.mockito.kotlin.doThrow(SecurityException("Clipboard write denied"))
            .whenever(clipboardManager).setPrimaryClip(any<android.content.ClipData>())
        val context = mock<Context>()
        whenever(context.applicationContext).thenReturn(context)
        whenever(context.getSystemService(Context.CLIPBOARD_SERVICE)).thenReturn(clipboardManager)

        ClipboardHelper.detach()
        ClipboardHelper.attach(context)

        try {
            val mockFailureConfigured = runCatching {
                clipboardManager.setPrimaryClip(android.content.ClipData.newPlainText("label", "probe"))
            }.exceptionOrNull() is SecurityException

            val toolCall = ToolCall("tc1", "set_clipboard", mapOf("text" to "hello"))
            val result = api.executeToolCall(toolCall, null)

            if (mockFailureConfigured) {
                assertTrue(result.isError)
                assertEquals(ToolErrorCode.EXECUTION_FAILED, result.errorCode)
                assertEquals(
                    "Failed: set_clipboard: clipboard write denied",
                    result.text
                )
            } else {
                assertFalse(result.isError)
                assertTrue(result.text.startsWith("Copied to clipboard"))
            }
        } finally {
            ClipboardHelper.detach()
        }
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

        assertTrue(result.isError)
        assertEquals(ToolErrorCode.SERVICE_UNAVAILABLE, result.errorCode)
        assertTrue(result.text.contains("not attached"))
    }

    @Test
    fun `read_notifications maps access revocation to ACCESS_DENIED`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(client)
        NotificationHelper.attach(AccessDeniedNotificationListenerService())

        try {
            val toolCall = ToolCall("tc1", "read_notifications", emptyMap())
            val result = api.executeToolCall(toolCall, null)

            assertTrue(result.isError)
            assertEquals(ToolErrorCode.ACCESS_DENIED, result.errorCode)
            assertTrue(result.text.contains("Notification access denied"))
        } finally {
            NotificationHelper.detach()
        }
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

        assertTrue(result.isError)
        assertEquals(ToolErrorCode.SERVICE_UNAVAILABLE, result.errorCode)
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

        assertTrue(result.isError)
        assertEquals(ToolErrorCode.SERVICE_UNAVAILABLE, result.errorCode)
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

        assertTrue(result.isError)
        assertEquals(ToolErrorCode.SERVICE_UNAVAILABLE, result.errorCode)
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

        assertTrue(result.isError)
        assertEquals(ToolErrorCode.INVALID_INPUT, result.errorCode)
        assertTrue(result.text.contains("Failed"))
    }

    @Test
    fun `tap_notification validates notification key format`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(client)

        val toolCall = ToolCall("tc1", "tap_notification", mapOf("notification_key" to "invalid-key"))
        val result = api.executeToolCall(toolCall, null)

        assertTrue(result.isError)
        assertEquals(ToolErrorCode.INVALID_INPUT, result.errorCode)
        assertTrue(result.text.contains("valid notification_key format"))
    }

    @Test
    fun `dismiss_notification validates notification key format`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(client)

        val toolCall = ToolCall("tc1", "dismiss_notification", mapOf("notification_key" to "a|b"))
        val result = api.executeToolCall(toolCall, null)

        assertTrue(result.isError)
        assertEquals(ToolErrorCode.INVALID_INPUT, result.errorCode)
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

        assertTrue(result.isError)
        assertEquals(ToolErrorCode.INVALID_INPUT, result.errorCode)
        assertTrue(result.text.contains("valid notification_key format"))
    }

    @Test
    fun `notification action tools map missing target to EXECUTION_FAILED`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(client)
        NotificationHelper.attach(EmptyNotificationListenerService())

        try {
            val validKey = "0|com.example.app|123|null|1000"
            val cases = listOf(
                ToolCall("tc1", "tap_notification", mapOf("notification_key" to validKey)) to
                    "Failed: tap_notification: notification may have been dismissed or has no content intent",
                ToolCall("tc2", "dismiss_notification", mapOf("notification_key" to validKey)) to
                    "Failed: dismiss_notification: notification may be ongoing or already dismissed",
                ToolCall(
                    "tc3",
                    "reply_notification",
                    mapOf("notification_key" to validKey, "text" to "hello")
                ) to "Failed: reply_notification: notification may not support inline reply or was dismissed"
            )

            for ((toolCall, expectedText) in cases) {
                val result = api.executeToolCall(toolCall, null)
                assertTrue(result.isError, "Expected error for ${toolCall.name}")
                assertEquals(ToolErrorCode.EXECUTION_FAILED, result.errorCode)
                assertEquals(expectedText, result.text)
            }
        } finally {
            NotificationHelper.detach()
        }
    }

    @Test
    fun `notification action tools map access revocation to ACCESS_DENIED`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(client)
        NotificationHelper.attach(AccessDeniedNotificationListenerService())

        try {
            val validKey = "0|com.example.app|123|null|1000"
            val cases = listOf(
                ToolCall("tc1", "tap_notification", mapOf("notification_key" to validKey)),
                ToolCall("tc2", "dismiss_notification", mapOf("notification_key" to validKey)),
                ToolCall("tc3", "reply_notification", mapOf("notification_key" to validKey, "text" to "hello"))
            )

            for (toolCall in cases) {
                val result = api.executeToolCall(toolCall, null)
                assertTrue(result.isError, "Expected error for ${toolCall.name}")
                assertEquals(ToolErrorCode.ACCESS_DENIED, result.errorCode)
                assertTrue(result.text.contains("Notification access denied"))
            }
        } finally {
            NotificationHelper.detach()
        }
    }

    @Test
    fun `dismiss_notification maps cancelNotification access revocation to ACCESS_DENIED`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(client)
        val validKey = "0|com.example.app|123|null|1000"
        NotificationHelper.attach(cancelAccessDeniedNotificationListenerService(validKey))

        try {
            val result = api.executeToolCall(
                ToolCall("tc1", "dismiss_notification", mapOf("notification_key" to validKey)),
                null
            )

            assertTrue(result.isError)
            assertEquals(ToolErrorCode.ACCESS_DENIED, result.errorCode)
            assertTrue(result.text.contains("Notification access denied"))
        } finally {
            NotificationHelper.detach()
        }
    }

    @Test
    fun `notification key validation rejects single-segment package`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(client)

        val toolCall = ToolCall("tc1", "tap_notification", mapOf("notification_key" to "0|example|123"))
        val result = api.executeToolCall(toolCall, null)

        assertTrue(result.isError)
        assertEquals(ToolErrorCode.INVALID_INPUT, result.errorCode)
        assertTrue(result.text.contains("valid notification_key format"))
    }

    @Test
    fun `notification key validation rejects empty package segment`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(client)

        val toolCall = ToolCall("tc1", "tap_notification", mapOf("notification_key" to "0|com..app|123"))
        val result = api.executeToolCall(toolCall, null)

        assertTrue(result.isError)
        assertEquals(ToolErrorCode.INVALID_INPUT, result.errorCode)
        assertTrue(result.text.contains("valid notification_key format"))
    }

    @Test
    fun `notification key validation rejects two-part key`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(client)

        val toolCall = ToolCall("tc1", "tap_notification", mapOf("notification_key" to "0|com.example.app"))
        val result = api.executeToolCall(toolCall, null)

        assertTrue(result.isError)
        assertEquals(ToolErrorCode.INVALID_INPUT, result.errorCode)
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
        assertTrue(result.isError)
        assertEquals(ToolErrorCode.SERVICE_UNAVAILABLE, result.errorCode)
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

        assertTrue(result.isError)
        assertEquals(ToolErrorCode.INVALID_INPUT, result.errorCode)
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
    fun `executeToolCallWithVerification preserves errorCode and severity from tool result`() = runTest {
        ScreenReader.detach()
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val verifier = ActionVerifier(client, VerificationMode.ALWAYS)
        val api = PhoneAgentApi(client, client, verifier = verifier)

        val toolCall = ToolCall("tc1", "press_home", emptyMap())
        val underlying = api.executeToolCall(toolCall, null)
        val verified = api.executeToolCallWithVerification(toolCall, null)

        assertEquals(underlying.errorCode, verified.errorCode)
        assertEquals(underlying.severity, verified.severity)
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

    @Test
    fun `tracks task usage across sendMessage and continueAfterTools`() = runTest {
        val client = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            toolResponses = ArrayDeque(
                listOf(
                    ChatResponse(
                        text = null,
                        toolCalls = listOf(ToolCall("t1", "tap", mapOf("element_id" to 1))),
                        stopReason = "tool_use",
                        usage = TokenUsage(inputTokens = 100, outputTokens = 40)
                    ),
                    ChatResponse(
                        text = "Done",
                        toolCalls = emptyList(),
                        stopReason = "end_turn",
                        usage = TokenUsage(inputTokens = 60, outputTokens = 20)
                    )
                )
            )
        )
        val budgetStore = InMemoryBudgetStore(BudgetConfig(enabled = false))
        val agent = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            budgetGuard = BudgetGuard(budgetStore)
        ).also { it.phoneControlOverride = true }

        val first = agent.sendMessage("Open settings", screenContent = null, isActionLoop = false)
        assertEquals("tool_use", first.stopReason)
        assertEquals(1, first.toolCalls.size)

        agent.addToolResult("t1", "Tapped element 1", toolName = "tap", isError = false)
        val second = agent.continueAfterTools()
        assertEquals("Done", second.text)

        val summary = agent.lastTaskCostSummary
        assertNotNull(summary)
        assertEquals(220L, summary.totalTokens)
        assertEquals(160L, summary.inputTokens)
        assertEquals(60L, summary.outputTokens)
        assertEquals(2, summary.apiCalls)
        assertEquals(0.00138, summary.estimatedCostUsd, 0.0000000001)
    }

    @Test
    fun `budget is checked after each API call in tool loop`() = runTest {
        val client = ScriptedProviderClient(
            provider = Provider.OPENAI,
            modelId = "gpt-4o-mini",
            toolResponses = ArrayDeque(
                listOf(
                    ChatResponse(
                        text = null,
                        toolCalls = listOf(ToolCall("t1", "tap", mapOf("element_id" to 1))),
                        stopReason = "tool_use",
                        usage = TokenUsage(inputTokens = 1_000, outputTokens = 0)
                    ),
                    ChatResponse(
                        text = "Done",
                        toolCalls = emptyList(),
                        stopReason = "end_turn",
                        usage = TokenUsage(inputTokens = 1_000, outputTokens = 0)
                    )
                )
            )
        )
        val budgetStore = InMemoryBudgetStore(
            BudgetConfig(enabled = true, dailyLimitUsd = 0.0002, monthlyLimitUsd = 10.0)
        )
        val agent = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            budgetGuard = BudgetGuard(budgetStore)
        ).also { it.phoneControlOverride = true }

        val first = agent.sendMessage("Open settings", screenContent = null, isActionLoop = false)
        assertEquals(1, first.toolCalls.size)

        agent.addToolResult("t1", "Tapped element 1", toolName = "tap", isError = false)
        val beforeContinue = getMessages(agent).toList()
        assertFailsWith<BudgetExceededException> {
            agent.continueAfterTools()
        }
        assertEquals(beforeContinue, getMessages(agent), "continueAfterTools over-limit should not persist partial assistant turn")
    }

    @Test
    fun `chat mode records usage and enforces budget on over-limit call`() = runTest {
        val client = ScriptedProviderClient(
            provider = Provider.OPENAI,
            modelId = "gpt-4o-mini",
            chatWithUsageResponses = ArrayDeque(
                listOf(
                    ("Hello there" to TokenUsage(inputTokens = 1_000, outputTokens = 0)),
                    ("Hi again" to TokenUsage(inputTokens = 1_000, outputTokens = 0))
                )
            )
        )
        val budgetStore = InMemoryBudgetStore(
            BudgetConfig(enabled = true, dailyLimitUsd = 0.0002, monthlyLimitUsd = 10.0)
        )
        val agent = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            budgetGuard = BudgetGuard(budgetStore)
        ).also { it.phoneControlOverride = true }

        val first = agent.sendMessage("hi", screenContent = null, isActionLoop = false)
        assertEquals("Hello there", first.text)
        assertEquals(2, agent.messageCount)
        val beforeSecond = getMessages(agent).toList()

        val summary = agent.lastTaskCostSummary
        assertEquals(1, summary.apiCalls)
        assertEquals(1_000L, summary.inputTokens)
        assertEquals(0.00015, summary.estimatedCostUsd, 0.0000000001)

        assertFailsWith<BudgetExceededException> {
            agent.sendMessage("hello", screenContent = null, isActionLoop = false)
        }
        assertEquals(300L, budgetStore.getDailySpentMicrodollars())
        assertEquals(beforeSecond, getMessages(agent), "chat over-limit should not persist user/assistant partial turn")
    }

    @Test
    fun `pre-call budget gate does not emit zero-dollar allowed telemetry`() = runTest {
        val telemetryEvents = mutableListOf<BudgetTelemetryEvent>()
        val client = ScriptedProviderClient(
            provider = Provider.OPENAI,
            modelId = "gpt-4o-mini",
            chatWithUsageResponses = ArrayDeque(
                listOf(
                    ("Hello there" to TokenUsage(inputTokens = 1_000, outputTokens = 0))
                )
            )
        )
        val budgetStore = InMemoryBudgetStore(
            BudgetConfig(enabled = true, dailyLimitUsd = 10.0, monthlyLimitUsd = 10.0)
        )
        val budgetGuard = BudgetGuard(budgetStore) { event -> telemetryEvents += event }
        val agent = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            budgetGuard = budgetGuard
        ).also { it.phoneControlOverride = true }

        val response = agent.sendMessage("hi", screenContent = null, isActionLoop = false)

        assertEquals("Hello there", response.text)
        assertEquals(1, telemetryEvents.size)
        assertTrue(telemetryEvents.all { it.amountUsd > 0.0 }, "pre-call gate should not emit amountUsd=0 allowed events")
    }

    @Test
    fun `chat mode tracks usage when budget guard is null`() = runTest {
        val client = ScriptedProviderClient(
            provider = Provider.OPENAI,
            modelId = "gpt-4o-mini",
            chatWithUsageResponses = ArrayDeque(
                listOf(
                    ("Hello there" to TokenUsage(inputTokens = 1_000, outputTokens = 200))
                )
            )
        )
        val agent = PhoneAgentApi(chatClient = client, actionClient = client).also {
            it.phoneControlOverride = false
        }

        val response = agent.sendMessage("hi", screenContent = null, isActionLoop = false)
        assertEquals("Hello there", response.text)

        val summary = agent.lastTaskCostSummary
        assertEquals(1, summary.apiCalls)
        assertEquals(1_000L, summary.inputTokens)
        assertEquals(200L, summary.outputTokens)
        assertEquals(1_200L, summary.totalTokens)
        assertEquals(0.00027, summary.estimatedCostUsd, 0.0000000001)
    }

    @Test
    fun `chat mode with missing usage metadata still records fallback spend and enforces budget`() = runTest {
        val client = ScriptedProviderClient(
            provider = Provider.OPENAI,
            modelId = "gpt-4o-mini",
            chatResponses = ArrayDeque(listOf("Hello there", "Hi again"))
        )
        val budgetStore = InMemoryBudgetStore(
            BudgetConfig(enabled = true, dailyLimitUsd = 0.00008, monthlyLimitUsd = 10.0)
        )
        val agent = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            budgetGuard = BudgetGuard(budgetStore)
        ).also { it.phoneControlOverride = true }

        val first = agent.sendMessage("hi", screenContent = null, isActionLoop = false)
        assertEquals("Hello there", first.text)
        val firstSpend = budgetStore.getDailySpentMicrodollars()
        assertTrue(firstSpend > 0L)
        assertEquals(1, agent.lastTaskCostSummary.apiCalls)

        assertFailsWith<BudgetExceededException> {
            agent.sendMessage("hello", screenContent = null, isActionLoop = false)
        }
        assertTrue(budgetStore.getDailySpentMicrodollars() > firstSpend)
    }

    @Test
    fun `chat mode with budget guard preserves incremental streaming callbacks`() = runTest {
        val client = ScriptedProviderClient(
            provider = Provider.OPENAI,
            modelId = "gpt-4o-mini",
            streamingResponses = ArrayDeque(listOf(listOf("Hel", "lo", " there"))),
            chatResponses = ArrayDeque(listOf("Hello there"))
        )
        val budgetStore = InMemoryBudgetStore(
            BudgetConfig(enabled = true, dailyLimitUsd = 10.0, monthlyLimitUsd = 10.0)
        )
        val agent = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            budgetGuard = BudgetGuard(budgetStore)
        ).also { it.phoneControlOverride = true }
        val deltas = mutableListOf<String>()

        val response = agent.sendMessage(
            userMessage = "hi",
            screenContent = null,
            isActionLoop = false,
            onTextDelta = { deltas += it }
        )

        assertEquals("Hello there", response.text)
        assertEquals(listOf("Hel", "lo", " there"), deltas)
        assertEquals(1, client.chatStreamingCalls)
        assertEquals(0, client.chatWithToolsCalls)
        assertTrue(budgetStore.getDailySpentMicrodollars() > 0L)
    }

    @Test
    fun `streaming chat with missing usage metadata enforces budget with fallback spend`() = runTest {
        val client = ScriptedProviderClient(
            provider = Provider.OPENAI,
            modelId = "gpt-4o-mini",
            streamingResponses = ArrayDeque(listOf(listOf("Hel", "lo"), listOf("Hi"))),
            chatResponses = ArrayDeque(listOf("Hello", "Hi"))
        )
        val budgetStore = InMemoryBudgetStore(
            BudgetConfig(enabled = true, dailyLimitUsd = 0.00008, monthlyLimitUsd = 10.0)
        )
        val agent = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            budgetGuard = BudgetGuard(budgetStore)
        ).also { it.phoneControlOverride = true }

        val first = agent.sendMessage(
            userMessage = "hello?",
            screenContent = null,
            isActionLoop = false,
            onTextDelta = {}
        )
        assertEquals("Hello", first.text)
        val firstSpend = budgetStore.getDailySpentMicrodollars()
        assertTrue(firstSpend > 0L)

        assertFailsWith<BudgetExceededException> {
            agent.sendMessage(
                userMessage = "hello?",
                screenContent = null,
                isActionLoop = false,
                onTextDelta = {}
            )
        }
        assertTrue(budgetStore.getDailySpentMicrodollars() > firstSpend)
    }

    @Test
    fun `chat fallback estimate uses raw output length when sanitization removes artifacts`() = runTest {
        val cleanClient = ScriptedProviderClient(
            provider = Provider.OPENAI,
            modelId = "gpt-4o-mini",
            chatResponses = ArrayDeque(listOf("Short answer"))
        )
        val artifactClient = ScriptedProviderClient(
            provider = Provider.OPENAI,
            modelId = "gpt-4o-mini",
            chatResponses = ArrayDeque(listOf("<tool_use>${"x".repeat(6000)}</tool_use>Short answer"))
        )
        val cleanAgent = PhoneAgentApi(chatClient = cleanClient, actionClient = cleanClient).also {
            it.phoneControlOverride = true
        }
        val artifactAgent = PhoneAgentApi(chatClient = artifactClient, actionClient = artifactClient).also {
            it.phoneControlOverride = true
        }

        val clean = cleanAgent.sendMessage("hi", screenContent = null, isActionLoop = false)
        val artifact = artifactAgent.sendMessage("hi", screenContent = null, isActionLoop = false)

        assertEquals("Short answer", clean.text)
        assertEquals("Short answer", artifact.text)
        assertTrue(
            artifactAgent.lastTaskCostSummary.estimatedCostUsd > cleanAgent.lastTaskCostSummary.estimatedCostUsd,
            "artifact-heavy raw output should estimate higher fallback cost even when sanitized output matches"
        )
    }

    @Test
    fun `chat mode without budget guard preserves incremental streaming callbacks`() = runTest {
        val client = ScriptedProviderClient(
            provider = Provider.OPENAI,
            modelId = "gpt-4o-mini",
            streamingResponses = ArrayDeque(listOf(listOf("Hi", " there"))),
            chatResponses = ArrayDeque(listOf("Hi there"))
        )
        val agent = PhoneAgentApi(chatClient = client, actionClient = client).also {
            it.phoneControlOverride = false
        }
        val deltas = mutableListOf<String>()

        val response = agent.sendMessage(
            userMessage = "hello?",
            screenContent = null,
            isActionLoop = false,
            onTextDelta = { deltas += it }
        )

        assertEquals("Hi there", response.text)
        assertEquals(listOf("Hi", " there"), deltas)
        assertEquals(1, client.chatStreamingCalls)
        assertEquals(0, client.chatWithToolsCalls)
    }

    @Test
    fun `pre-call budget failure does not append ghost user message`() = runTest {
        val client = ScriptedProviderClient(
            provider = Provider.OPENAI,
            modelId = "gpt-4o-mini",
            toolResponses = ArrayDeque(
                listOf(
                    ChatResponse(
                        text = "should never be called",
                        toolCalls = emptyList(),
                        stopReason = "end_turn"
                    )
                )
            )
        )
        val budgetStore = InMemoryBudgetStore(
            config = BudgetConfig(enabled = true, dailyLimitUsd = 0.0002, monthlyLimitUsd = 10.0),
            dailySpentMicrodollars = 201L,
            monthlySpentMicrodollars = 201L
        )
        val agent = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            budgetGuard = BudgetGuard(budgetStore)
        ).also { it.phoneControlOverride = true }

        val error = assertFailsWith<BudgetExceededException> {
            agent.sendMessage("Open settings", screenContent = null, isActionLoop = false)
        }

        assertEquals(BudgetErrorCode.DAILY_LIMIT, error.code)
        assertEquals(0, agent.messageCount, "Pre-check failure must not mutate history")
        assertEquals(0, client.chatWithToolsCalls, "Pre-check failure must not call provider")
    }

    @Test
    fun `post-call budget exception does not persist partial tool-mode turn`() = runTest {
        val client = ScriptedProviderClient(
            provider = Provider.OPENAI,
            modelId = "gpt-4o-mini",
            toolResponses = ArrayDeque(
                listOf(
                    ChatResponse(
                        text = null,
                        toolCalls = listOf(ToolCall("t1", "tap", mapOf("element_id" to 1))),
                        stopReason = "tool_use",
                        usage = TokenUsage(inputTokens = 1_000, outputTokens = 1_000)
                    )
                )
            )
        )
        val budgetStore = InMemoryBudgetStore(
            config = BudgetConfig(enabled = true, dailyLimitUsd = 0.0002, monthlyLimitUsd = 10.0)
        )
        val agent = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            budgetGuard = BudgetGuard(budgetStore)
        ).also { it.phoneControlOverride = true }

        assertFailsWith<BudgetExceededException> {
            agent.sendMessage("Open settings", screenContent = null, isActionLoop = false)
        }

        val persisted = getMessages(agent)
        assertEquals(0, persisted.size, "Tool-mode over-limit should not persist a hidden user/assistant partial turn")
    }

    @Test
    fun `tool mode with missing usage metadata records fallback spend and enforces budget`() = runTest {
        val client = ScriptedProviderClient(
            provider = Provider.OPENAI,
            modelId = "gpt-4o-mini",
            toolResponses = ArrayDeque(
                listOf(
                    ChatResponse(
                        text = null,
                        toolCalls = listOf(ToolCall("t1", "tap", mapOf("element_id" to 1))),
                        stopReason = "tool_use"
                    ),
                    ChatResponse(
                        text = "Done",
                        toolCalls = emptyList(),
                        stopReason = "end_turn"
                    )
                )
            )
        )
        val budgetStore = InMemoryBudgetStore(
            BudgetConfig(enabled = true, dailyLimitUsd = 0.0012, monthlyLimitUsd = 10.0)
        )
        val agent = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            budgetGuard = BudgetGuard(budgetStore)
        ).also { it.phoneControlOverride = true }

        val first = agent.sendMessage("Open settings", screenContent = null, isActionLoop = false)
        assertEquals("tool_use", first.stopReason)
        val firstSpend = budgetStore.getDailySpentMicrodollars()
        assertTrue(firstSpend > 0L)

        agent.addToolResult("t1", "Tapped element 1", toolName = "tap", isError = false)
        val error = assertFailsWith<BudgetExceededException> {
            agent.continueAfterTools()
        }
        assertEquals(BudgetErrorCode.DAILY_LIMIT, error.code)
        assertTrue(budgetStore.getDailySpentMicrodollars() > firstSpend)
    }

    @Test
    fun `tool mode fallback estimate is conservative relative to known usage baseline`() = runTest {
        val knownUsageClient = ScriptedProviderClient(
            provider = Provider.OPENAI,
            modelId = "gpt-4o-mini",
            toolResponses = ArrayDeque(
                listOf(
                    ChatResponse(
                        text = null,
                        toolCalls = listOf(ToolCall("t1", "tap", mapOf("element_id" to 1))),
                        stopReason = "tool_use",
                        usage = TokenUsage(inputTokens = 500, outputTokens = 100)
                    )
                )
            )
        )
        val knownStore = InMemoryBudgetStore(BudgetConfig(enabled = false))
        val knownAgent = PhoneAgentApi(
            chatClient = knownUsageClient,
            actionClient = knownUsageClient,
            budgetGuard = BudgetGuard(knownStore)
        ).also { it.phoneControlOverride = true }

        knownAgent.sendMessage("Open settings", screenContent = null, isActionLoop = false)
        val knownSpend = knownStore.getDailySpentMicrodollars()
        assertTrue(knownSpend > 0L)

        val fallbackClient = ScriptedProviderClient(
            provider = Provider.OPENAI,
            modelId = "gpt-4o-mini",
            toolResponses = ArrayDeque(
                listOf(
                    ChatResponse(
                        text = null,
                        toolCalls = listOf(ToolCall("t1", "tap", mapOf("element_id" to 1))),
                        stopReason = "tool_use"
                    )
                )
            )
        )
        val fallbackStore = InMemoryBudgetStore(BudgetConfig(enabled = false))
        val fallbackAgent = PhoneAgentApi(
            chatClient = fallbackClient,
            actionClient = fallbackClient,
            budgetGuard = BudgetGuard(fallbackStore)
        ).also { it.phoneControlOverride = true }

        fallbackAgent.sendMessage("Open settings", screenContent = null, isActionLoop = false)
        val fallbackSpend = fallbackStore.getDailySpentMicrodollars()
        assertTrue(
            fallbackSpend >= knownSpend,
            "fallback spend ($fallbackSpend) should be >= known-usage spend ($knownSpend)"
        )
    }

    @Test
    fun `per-task cap is enforced pre-call after cap is reached`() = runTest {
        val client = ScriptedProviderClient(
            provider = Provider.OPENAI,
            modelId = "gpt-4o-mini",
            toolResponses = ArrayDeque(
                listOf(
                    ChatResponse(
                        text = null,
                        toolCalls = listOf(ToolCall("t1", "tap", mapOf("element_id" to 1))),
                        stopReason = "tool_use",
                        usage = TokenUsage(inputTokens = 1_000, outputTokens = 0)
                    ),
                    ChatResponse(
                        text = "Should not be called",
                        toolCalls = emptyList(),
                        stopReason = "end_turn",
                        usage = TokenUsage(inputTokens = 1_000, outputTokens = 0)
                    )
                )
            )
        )
        val budgetStore = InMemoryBudgetStore(
            config = BudgetConfig(
                enabled = true,
                dailyLimitUsd = 10.0,
                monthlyLimitUsd = 10.0,
                perTaskLimitUsd = 0.00015
            )
        )
        val agent = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            budgetGuard = BudgetGuard(budgetStore)
        ).also { it.phoneControlOverride = true }

        assertFailsWith<BudgetExceededException> {
            agent.sendMessage("Open settings", screenContent = null, isActionLoop = false)
        }
        assertEquals(1, client.chatWithToolsCalls)

        agent.addToolResult("t1", "Tapped element 1", toolName = "tap", isError = false)
        assertFailsWith<BudgetExceededException> {
            agent.continueAfterTools()
        }
        assertEquals(1, client.chatWithToolsCalls, "continueAfterTools must fail before provider call when capped")

        assertFailsWith<BudgetExceededException> {
            agent.sendEphemeral("[System: Summarize progress]")
        }
        assertEquals(1, client.chatWithToolsCalls, "sendEphemeral must fail before provider call when capped")
    }

    @Test
    fun `per-task cap enforces repeated sub-micro costs with carry`() = runTest {
        val client = ScriptedProviderClient(
            provider = Provider.OPENAI,
            modelId = "gpt-4o-mini",
            toolResponses = ArrayDeque(
                (1..7).map { idx ->
                    ChatResponse(
                        text = null,
                        toolCalls = listOf(ToolCall("t$idx", "tap", mapOf("element_id" to idx))),
                        stopReason = "tool_use",
                        usage = TokenUsage(inputTokens = 1, outputTokens = 0)
                    )
                }
            )
        )
        val budgetStore = InMemoryBudgetStore(
            config = BudgetConfig(
                enabled = true,
                dailyLimitUsd = 10.0,
                monthlyLimitUsd = 10.0,
                perTaskLimitUsd = 0.000001
            )
        )
        val agent = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            budgetGuard = BudgetGuard(budgetStore)
        ).also { it.phoneControlOverride = true }

        val first = agent.sendMessage("Open settings", screenContent = null, isActionLoop = false)
        assertEquals("tool_use", first.stopReason)

        repeat(5) { idx ->
            val toolId = "t${idx + 1}"
            agent.addToolResult(toolId, "Tapped element ${idx + 1}", toolName = "tap", isError = false)
            val response = agent.continueAfterTools()
            assertEquals("tool_use", response.stopReason)
        }

        agent.addToolResult("t6", "Tapped element 6", toolName = "tap", isError = false)
        assertFailsWith<BudgetExceededException> {
            agent.continueAfterTools()
        }

        assertEquals(7, client.chatWithToolsCalls)
        assertEquals(7, agent.lastTaskCostSummary.apiCalls)
        assertTrue(agent.lastTaskCostSummary.estimatedCostUsd >= 0.000001)
    }

    @Test
    fun `concurrent sendMessage calls do not bypass pre-call budget cap`() = runTest {
        val client = BlockingToolClient(
            response = ChatResponse(
                text = "Done",
                toolCalls = emptyList(),
                stopReason = "end_turn",
                usage = TokenUsage(inputTokens = 1_000, outputTokens = 0)
            )
        )
        val budgetStore = InMemoryBudgetStore(
            config = BudgetConfig(enabled = true, dailyLimitUsd = 0.00015, monthlyLimitUsd = 10.0)
        )
        val agent = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            budgetGuard = BudgetGuard(budgetStore)
        ).also { it.phoneControlOverride = true }

        val first = async {
            agent.sendMessage("Open settings", screenContent = null, isActionLoop = false)
        }
        client.awaitFirstCallStarted()

        val second = async {
            runCatching { agent.sendMessage("Open settings", screenContent = null, isActionLoop = false) }
        }
        delay(50)
        client.releaseFirstCall()

        val firstResponse = first.await()
        val secondResult = second.await()

        assertEquals("end_turn", firstResponse.stopReason)
        val secondError = secondResult.exceptionOrNull()
        assertTrue(secondError is BudgetExceededException, "second call should fail budget precheck after first spend")
        assertEquals(1, client.chatWithToolsCalls.get(), "only one provider call should execute under cap")
    }

    @Test
    fun `concurrent continueAfterTools calls do not bypass pre-call budget cap`() = runTest {
        val client = BlockingToolClient(
            response = ChatResponse(
                text = "Done",
                toolCalls = emptyList(),
                stopReason = "end_turn",
                usage = TokenUsage(inputTokens = 1_000, outputTokens = 0)
            )
        )
        val budgetStore = InMemoryBudgetStore(
            config = BudgetConfig(enabled = true, dailyLimitUsd = 0.00015, monthlyLimitUsd = 10.0)
        )
        val agent = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            budgetGuard = BudgetGuard(budgetStore)
        ).also { it.phoneControlOverride = true }

        val first = async { agent.continueAfterTools() }
        client.awaitFirstCallStarted()

        val second = async { runCatching { agent.continueAfterTools() } }
        delay(50)
        client.releaseFirstCall()

        val firstResponse = first.await()
        val secondResult = second.await()

        assertEquals("end_turn", firstResponse.stopReason)
        val secondError = secondResult.exceptionOrNull()
        assertTrue(secondError is BudgetExceededException, "second continuation should fail budget precheck")
        assertEquals(1, client.chatWithToolsCalls.get(), "only one provider continuation call should execute under cap")
    }

    @Test
    fun `clearConversation is non-blocking on caller while request is in flight and clears after release`() = runTest {
        val client = BlockingToolClient(
            response = ChatResponse(
                text = "Done",
                toolCalls = emptyList(),
                stopReason = "end_turn",
                usage = TokenUsage(inputTokens = 1_000, outputTokens = 0)
            )
        )
        val agent = PhoneAgentApi(chatClient = client, actionClient = client).also { it.phoneControlOverride = true }

        val send = async {
            agent.sendMessage("Open settings", screenContent = null, isActionLoop = false)
        }
        client.awaitFirstCallStarted()

        val clear = async { agent.clearConversation() }
        clear.await()
        assertTrue(clear.isCompleted, "clearConversation should not block caller thread")

        client.releaseFirstCall()
        val response = send.await()

        assertEquals("end_turn", response.stopReason)
        assertEquals(0, agent.messageCount, "clearConversation should apply after in-flight request and leave clean state")
        assertEquals(0, agent.currentToolStep)
        assertEquals(TaskCostSummary.EMPTY, agent.lastTaskCostSummary)
    }

    @Test
    fun `seedConversationHistory defers during in-flight request and backfills context`() = runTest {
        val client = BlockingToolClient(
            response = ChatResponse(
                text = "Done",
                toolCalls = emptyList(),
                stopReason = "end_turn",
                usage = TokenUsage(inputTokens = 1_000, outputTokens = 0)
            )
        )
        val agent = PhoneAgentApi(chatClient = client, actionClient = client).also { it.phoneControlOverride = true }
        val deferredSignals = mutableListOf<PhoneAgentApi.DeferredSeedSignal>()
        agent.onDeferredSeedSignal = { deferredSignals += it }
        val uiMessages = listOf(
            Message(role = "user", content = "prior user"),
            Message(role = "assistant", content = "prior assistant")
        )

        val send = async {
            agent.sendMessage("Open settings", screenContent = null, isActionLoop = false)
        }
        client.awaitFirstCallStarted()

        agent.seedConversationHistory(uiMessages)
        assertEquals(0, agent.messageCount, "in-flight call should not persist active turn until request releases")

        client.releaseFirstCall()
        send.await()

        val persisted = getMessages(agent)
        assertEquals("prior user", persisted[0].content)
        assertEquals("prior assistant", persisted[1].content)
        assertEquals("user", persisted[2].role)
        assertEquals("assistant", persisted[3].role)
        assertEquals(4, persisted.size)
        assertTrue(
            deferredSignals.any { it.action == "applied" && it.reason == "bootstrap_empty_history" },
            "expected applied bootstrap deferred seed signal"
        )
    }

    @Test
    fun `seedConversationHistory deferred in R31 maintenance window keeps bootstrap eligibility until maintenance applies`() = runTest {
        val client = BlockingToolClient(
            response = ChatResponse(
                text = "Done",
                toolCalls = emptyList(),
                stopReason = "end_turn",
                usage = TokenUsage(inputTokens = 1_000, outputTokens = 0)
            )
        )
        val agent = PhoneAgentApi(chatClient = client, actionClient = client).also { it.phoneControlOverride = true }
        val deferredSignals = mutableListOf<PhoneAgentApi.DeferredSeedSignal>()
        agent.onDeferredSeedSignal = { deferredSignals += it }
        val uiMessages = listOf(
            Message(role = "user", content = "seed user"),
            Message(role = "assistant", content = "seed assistant")
        )
        agent.onBeforeDeferredMaintenanceInActiveFlow = {
            agent.seedConversationHistory(uiMessages)
        }

        val send = async {
            agent.sendMessage("Open settings", screenContent = null, isActionLoop = false)
        }
        client.awaitFirstCallStarted()
        client.releaseFirstCall()
        send.await()

        val persisted = getMessages(agent)
        assertEquals(4, persisted.size)
        assertEquals("seed user", persisted[0].content)
        assertEquals("seed assistant", persisted[1].content)
        assertEquals("Open settings", persisted[2].content)
        assertEquals("Done", persisted[3].content)
        assertTrue(
            deferredSignals.any { it.action == "applied" && it.reason == "bootstrap_empty_history" },
            "expected applied bootstrap deferred seed signal for R31 maintenance window"
        )
    }

    @Test
    fun `deferred seed merge keeps first live message when seed tail role matches boundary`() = runTest {
        val client = BlockingToolClient(
            response = ChatResponse(
                text = "Done",
                toolCalls = emptyList(),
                stopReason = "end_turn",
                usage = TokenUsage(inputTokens = 1_000, outputTokens = 0)
            )
        )
        val agent = PhoneAgentApi(chatClient = client, actionClient = client).also { it.phoneControlOverride = true }
        val uiMessages = listOf(
            Message(role = "user", content = "seed user 1"),
            Message(role = "assistant", content = "seed assistant"),
            Message(role = "user", content = "seed user 2")
        )

        val send = async {
            agent.sendMessage("Open settings", screenContent = null, isActionLoop = false)
        }
        client.awaitFirstCallStarted()

        agent.seedConversationHistory(uiMessages)
        client.releaseFirstCall()
        send.await()

        val persisted = getMessages(agent)
        assertEquals(4, persisted.size)
        assertEquals("seed user 1", persisted[0].content)
        assertEquals("seed assistant", persisted[1].content)
        assertEquals("Open settings", persisted[2].content)
        assertEquals("Done", persisted[3].content)
    }

    @Test
    fun `multiple deferred seedConversationHistory calls during one request use latest snapshot`() = runTest {
        val client = BlockingToolClient(
            response = ChatResponse(
                text = "Done",
                toolCalls = emptyList(),
                stopReason = "end_turn",
                usage = TokenUsage(inputTokens = 1_000, outputTokens = 0)
            )
        )
        val agent = PhoneAgentApi(chatClient = client, actionClient = client).also { it.phoneControlOverride = true }
        val firstSeed = listOf(
            Message(role = "user", content = "stale user"),
            Message(role = "assistant", content = "stale assistant")
        )
        val latestSeed = listOf(
            Message(role = "user", content = "latest user"),
            Message(role = "assistant", content = "latest assistant")
        )

        val send = async {
            agent.sendMessage("Open settings", screenContent = null, isActionLoop = false)
        }
        client.awaitFirstCallStarted()

        agent.seedConversationHistory(firstSeed)
        agent.seedConversationHistory(latestSeed)
        client.releaseFirstCall()
        send.await()

        val persisted = getMessages(agent)
        assertEquals("latest user", persisted[0].content)
        assertEquals("latest assistant", persisted[1].content)
        assertEquals("Open settings", persisted[2].content)
        assertFalse(persisted.any { it.content.contains("stale") })
    }

    @Test
    fun `seedConversationHistory deferred during in-flight request skips stale seed when live history already exists`() = runTest {
        val client = BlockingToolClient(
            response = ChatResponse(
                text = "Done",
                toolCalls = emptyList(),
                stopReason = "end_turn",
                usage = TokenUsage(inputTokens = 1_000, outputTokens = 0)
            )
        )
        val agent = PhoneAgentApi(chatClient = client, actionClient = client).also { it.phoneControlOverride = true }
        val deferredSignals = mutableListOf<PhoneAgentApi.DeferredSeedSignal>()
        agent.onDeferredSeedSignal = { deferredSignals += it }

        agent.seedConversationHistory(
            listOf(
                Message(role = "user", content = "existing user"),
                Message(role = "assistant", content = "existing assistant")
            )
        )
        assertEquals(2, agent.messageCount)

        val send = async {
            agent.sendMessage("Open settings", screenContent = null, isActionLoop = false)
        }
        client.awaitFirstCallStarted()

        agent.seedConversationHistory(
            listOf(
                Message(role = "user", content = "stale user"),
                Message(role = "assistant", content = "stale assistant")
            )
        )

        client.releaseFirstCall()
        send.await()

        val persisted = getMessages(agent)
        assertEquals(4, persisted.size)
        assertEquals("existing user", persisted[0].content)
        assertEquals("existing assistant", persisted[1].content)
        assertEquals("Open settings", persisted[2].content)
        assertEquals("Done", persisted[3].content)
        assertFalse(persisted.any { it.content.contains("stale") })
        assertTrue(
            deferredSignals.any { it.action == "skipped" && it.reason == "non_empty_live_history_at_deferral" },
            "expected skipped deferred seed signal for non-empty live history"
        )
    }

    @Test
    fun `seedConversationHistory deferred during in-flight chat skips stale seed when live history has single user non-bootstrap`() = runTest {
        val client = BlockingUsageClient(responseText = "Done")
        val agent = PhoneAgentApi(chatClient = client, actionClient = client).also { it.phoneControlOverride = true }
        val deferredSignals = mutableListOf<PhoneAgentApi.DeferredSeedSignal>()
        agent.onDeferredSeedSignal = { deferredSignals += it }

        agent.seedConversationHistory(
            listOf(Message(role = "user", content = "existing single user"))
        )
        assertEquals(1, agent.messageCount)

        val send = async {
            agent.sendMessage("hey there", screenContent = null, isActionLoop = false)
        }
        client.awaitFirstCallStarted()

        agent.seedConversationHistory(
            listOf(
                Message(role = "user", content = "stale user"),
                Message(role = "assistant", content = "stale assistant")
            )
        )

        client.releaseFirstCall()
        send.await()

        val persisted = getMessages(agent)
        assertEquals(3, persisted.size)
        assertEquals("existing single user", persisted[0].content)
        assertEquals("hey there", persisted[1].content)
        assertEquals("Done", persisted[2].content)
        assertFalse(persisted.any { it.content.contains("stale") })
        assertTrue(
            deferredSignals.any { it.action == "skipped" && it.reason == "non_empty_live_history_at_deferral" },
            "expected skipped deferred seed signal for single-user non-bootstrap live history"
        )
    }

    @Test
    fun `onTextDelta callback reentrancy can clearConversation without deadlock`() = runTest {
        val client = ScriptedProviderClient(
            provider = Provider.OPENAI,
            modelId = "gpt-4o-mini",
            streamingResponses = ArrayDeque(
                listOf(
                    listOf("Hello", " world")
                )
            )
        )
        val agent = PhoneAgentApi(chatClient = client, actionClient = client).also { it.phoneControlOverride = true }
        val deltas = mutableListOf<String>()
        var clearInvoked = false

        val response = agent.sendMessage(
            userMessage = "hi",
            screenContent = null,
            onTextDelta = { delta ->
                deltas += delta
                if (!clearInvoked) {
                    clearInvoked = true
                    agent.clearConversation()
                }
            }
        )

        assertEquals("end_turn", response.stopReason)
        assertEquals("Hello world", response.text)
        assertEquals(listOf("Hello", " world"), deltas)
        assertEquals(0, agent.messageCount, "clearConversation requested from callback should clear safely after request")
    }

    @Test
    fun `budget telemetry preserves ordering across usage and fallback paths`() = runTest {
        val events = mutableListOf<BudgetTelemetryEvent>()
        val client = ScriptedProviderClient(
            provider = Provider.OPENAI,
            modelId = "gpt-4o-mini",
            toolResponses = ArrayDeque(
                listOf(
                    ChatResponse(
                        text = null,
                        toolCalls = listOf(ToolCall("t1", "tap", mapOf("element_id" to 1))),
                        stopReason = "tool_use",
                        usage = TokenUsage(inputTokens = 1_000, outputTokens = 0)
                    ),
                    ChatResponse(
                        text = "Done",
                        toolCalls = emptyList(),
                        stopReason = "end_turn"
                    )
                )
            )
        )
        val budgetStore = InMemoryBudgetStore(
            config = BudgetConfig(enabled = true, dailyLimitUsd = 10.0, monthlyLimitUsd = 10.0)
        )
        val agent = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            budgetGuard = BudgetGuard(budgetStore) { event -> events += event }
        ).also { it.phoneControlOverride = true }

        val first = agent.sendMessage("Open settings", screenContent = null, isActionLoop = false)
        assertEquals("tool_use", first.stopReason)
        agent.addToolResult("t1", "Tapped element 1", toolName = "tap", isError = false)
        val second = agent.continueAfterTools()
        assertEquals("end_turn", second.stopReason)

        val eventTypes = events.map { it.type }
        assertEquals(
            listOf(BudgetTelemetryEvent.Type.ALLOWED, BudgetTelemetryEvent.Type.MISSING_USAGE_FALLBACK),
            eventTypes
        )
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
        private val chatWithUsageResponses: ArrayDeque<Pair<String, TokenUsage?>> = ArrayDeque(),
        private val streamingResponses: ArrayDeque<List<String>> = ArrayDeque(),
        private val toolResponses: ArrayDeque<ChatResponse> = ArrayDeque(),
        private val visionResponses: ArrayDeque<String> = ArrayDeque(),
        override val modelId: String? = null
    ) : ProviderClient {
        var chatCalls = 0
        var chatWithUsageCalls = 0
        var chatStreamingCalls = 0
        var chatWithToolsCalls = 0
        var describeImageCalls = 0
        /** Last messages list passed to chatWithTools, for verifying conversation flow. */
        var lastMessages: List<Message>? = null
        /** Last system prompt passed to chatWithTools. */
        var lastSystemPrompt: String? = null
        /** Last tool list passed to chatWithTools. */
        var lastTools: List<Tool>? = null

        override suspend fun chat(conversation: Conversation): Result<String> {
            chatCalls++
            return Result.success(chatResponses.removeFirst())
        }

        override suspend fun chatWithUsage(conversation: Conversation): Result<Pair<String, TokenUsage?>> {
            chatWithUsageCalls++
            return if (chatWithUsageResponses.isNotEmpty()) {
                Result.success(chatWithUsageResponses.removeFirst())
            } else {
                Result.success(chat(conversation).getOrThrow() to null)
            }
        }

        override suspend fun chatStreaming(
            conversation: Conversation,
            onDelta: (String) -> Unit
        ): Result<String> {
            chatStreamingCalls++
            val chunks = if (streamingResponses.isNotEmpty()) {
                streamingResponses.removeFirst()
            } else {
                listOf(chatResponses.removeFirst())
            }
            chunks.forEach(onDelta)
            return Result.success(chunks.joinToString(""))
        }

        override suspend fun chatWithTools(
            messages: List<Message>,
            systemPrompt: String?,
            tools: List<Tool>,
            tokenLimit: Int?
        ): Result<ChatResponse> {
            chatWithToolsCalls++
            lastMessages = messages.toList()
            lastSystemPrompt = systemPrompt
            lastTools = tools.toList()
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

    private class BlockingToolClient(
        private val response: ChatResponse,
        override val modelId: String? = "gpt-4o-mini"
    ) : ProviderClient {
        override val provider: Provider = Provider.OPENAI
        val chatWithToolsCalls = AtomicInteger(0)
        private val firstCallRelease = kotlinx.coroutines.CompletableDeferred<Unit>()

        suspend fun awaitFirstCallStarted() {
            while (chatWithToolsCalls.get() == 0) {
                delay(5)
            }
        }

        fun releaseFirstCall() {
            firstCallRelease.complete(Unit)
        }

        override suspend fun chat(conversation: Conversation): Result<String> {
            return Result.success("unused")
        }

        override suspend fun chatWithTools(
            messages: List<Message>,
            systemPrompt: String?,
            tools: List<Tool>,
            tokenLimit: Int?
        ): Result<ChatResponse> {
            val callIndex = chatWithToolsCalls.incrementAndGet()
            if (callIndex == 1) {
                firstCallRelease.await()
            }
            return Result.success(response)
        }

        override suspend fun describeImage(base64Image: String, prompt: String, maxTokens: Int): Result<String> {
            return Result.failure(UnsupportedOperationException("unused"))
        }
    }

    private class BlockingUsageClient(
        private val responseText: String,
        private val usage: TokenUsage? = TokenUsage(inputTokens = 100, outputTokens = 10),
        override val modelId: String? = "gpt-4o-mini"
    ) : ProviderClient {
        override val provider: Provider = Provider.OPENAI
        val chatWithUsageCalls = AtomicInteger(0)
        private val firstCallRelease = kotlinx.coroutines.CompletableDeferred<Unit>()

        suspend fun awaitFirstCallStarted() {
            while (chatWithUsageCalls.get() == 0) {
                delay(5)
            }
        }

        fun releaseFirstCall() {
            firstCallRelease.complete(Unit)
        }

        override suspend fun chat(conversation: Conversation): Result<String> {
            return Result.success(responseText)
        }

        override suspend fun chatWithUsage(conversation: Conversation): Result<Pair<String, TokenUsage?>> {
            val callIndex = chatWithUsageCalls.incrementAndGet()
            if (callIndex == 1) {
                firstCallRelease.await()
            }
            return Result.success(responseText to usage)
        }

        override suspend fun chatWithTools(
            messages: List<Message>,
            systemPrompt: String?,
            tools: List<Tool>,
            tokenLimit: Int?
        ): Result<ChatResponse> {
            return Result.failure(UnsupportedOperationException("unused"))
        }

        override suspend fun describeImage(base64Image: String, prompt: String, maxTokens: Int): Result<String> {
            return Result.failure(UnsupportedOperationException("unused"))
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
        assertEquals(ToolErrorCode.NOT_CONFIGURED, result.errorCode)
    }

    @Test
    fun `file tool wrapper maps invalid input to INVALID_INPUT`() = runTest {
        val tempRoot = createTempDir(prefix = "phone-agent-file-invalid-input")
        try {
            val manager = AgentFileManager.fromDirectory(tempRoot)
            val agent = createAgent(fileManager = manager)
            val result = agent.executeToolCall(ToolCall("f1", "read_file", emptyMap()), null)
            assertTrue(result.isError)
            assertEquals(ToolErrorCode.INVALID_INPUT, result.errorCode)
        } finally {
            tempRoot.deleteRecursively()
        }
    }

    @Test
    fun `file tool wrapper maps security exceptions to ACCESS_DENIED`() = runTest {
        val tempRoot = createTempDir(prefix = "phone-agent-file-security")
        try {
            val manager = AgentFileManager.fromDirectory(tempRoot)
            val agent = createAgent(fileManager = manager)
            val result = agent.executeToolCall(
                ToolCall("f1", "read_file", mapOf("path" to "../secrets.txt")),
                null
            )
            assertTrue(result.isError)
            assertEquals(ToolErrorCode.ACCESS_DENIED, result.errorCode)
        } finally {
            tempRoot.deleteRecursively()
        }
    }

    @Test
    fun `file tool wrapper maps unexpected exceptions to EXECUTION_FAILED`() = runTest {
        val tempRoot = createTempDir(prefix = "phone-agent-file-execution-failed")
        try {
            val manager = AgentFileManager.fromDirectory(tempRoot)
            tempRoot.resolve("blocked").writeText("this is a file, not a directory")
            val agent = createAgent(fileManager = manager)
            val result = agent.executeToolCall(
                ToolCall("f1", "write_file", mapOf("path" to "blocked/nested.txt", "content" to "x")),
                null
            )
            assertTrue(result.isError)
            assertEquals(ToolErrorCode.EXECUTION_FAILED, result.errorCode)
        } finally {
            tempRoot.deleteRecursively()
        }
    }

    @Test
    fun `memory tool wrapper maps not configured to NOT_CONFIGURED`() = runTest {
        val agent = createAgent()
        val result = agent.executeToolCall(
            ToolCall("m1", "remember", mapOf("content" to "test")),
            null
        )
        assertTrue(result.isError, "memory tool error should have isError=true")
        assertEquals(ToolErrorCode.NOT_CONFIGURED, result.errorCode)
    }

    @Test
    fun `memory tool wrapper maps invalid input to INVALID_INPUT`() = runTest {
        val agent = createAgent(memoryProvider = InMemoryMemoryProvider())
        val result = agent.executeToolCall(
            ToolCall("m1", "remember", emptyMap()),
            null
        )
        assertTrue(result.isError)
        assertEquals(ToolErrorCode.INVALID_INPUT, result.errorCode)
    }

    @Test
    fun `memory tool wrapper maps unexpected exceptions to EXECUTION_FAILED`() = runTest {
        val explodingProvider = object : MemoryProvider {
            override suspend fun store(content: String, metadata: MemoryMetadata): String {
                throw RuntimeException("boom")
            }
            override suspend fun search(query: String, limit: Int): List<MemoryResult> = emptyList()
            override suspend fun delete(id: String) = Unit
            override suspend fun list(filter: MemoryFilter?): List<MemoryResult> = emptyList()
        }
        val agent = createAgent(memoryProvider = explodingProvider)
        val result = agent.executeToolCall(
            ToolCall("m1", "remember", mapOf("content" to "test")),
            null
        )
        assertTrue(result.isError)
        assertEquals(ToolErrorCode.EXECUTION_FAILED, result.errorCode)
    }

    @Test
    fun `unknown tool returns TOOL_NOT_FOUND with isError true`() = runTest {
        val agent = createAgent()
        val result = agent.executeToolCall(
            ToolCall("u1", "nonexistent_tool", emptyMap()),
            null
        )
        assertTrue(result.isError, "unknown tool should have isError=true")
        assertTrue(result.text.contains("unknown tool"), "should mention unknown tool")
        assertEquals(ToolErrorCode.TOOL_NOT_FOUND, result.errorCode)
    }

    @Test
    fun `ui tool failures consistently map to isError true`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val screen = ScreenContent(
            packageName = "com.example.app",
            elements = listOf(
                ScreenElement(
                    id = 5,
                    text = "Settings",
                    contentDescription = null,
                    className = "android.widget.TextView",
                    isClickable = true,
                    isEditable = false,
                    bounds = Rect(0, 0, 20, 20)
                )
            )
        )
        val api = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            clickElement = { ScreenReader.ElementActionResult.ServiceUnavailable },
            longPressElement = { ScreenReader.ElementActionResult.ElementNotFound },
            isScreenReaderAttached = { false }
        )

        val cases = listOf(
            ToolCall("u1", "tap", mapOf("element_id" to 5)) to null,
            ToolCall("u2", "tap_text", mapOf("text" to "Settings")) to screen,
            ToolCall("u3", "long_press", mapOf("element_id" to 5)) to null,
            ToolCall("u4", "screenshot", emptyMap()) to null,
            ToolCall("u5", "read_screen", emptyMap()) to null
        )

        for ((call, content) in cases) {
            val result = api.executeToolCall(call, content)
            assertTrue(result.isError, "Expected isError=true for ${call.name}, got: ${result.text}")
        }
    }

    @Test
    fun `think tool returns isError false`() = runTest {
        val agent = createAgent()
        val result = agent.executeToolCall(
            ToolCall("t1", "think", mapOf("thought" to "testing")),
            null
        )
        assertFalse(result.isError, "think tool success should have isError=false")
        assertNull(result.errorCode)
    }

    @Test
    fun `read_screen detached classifies structured service unavailable error`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            isScreenReaderAttached = { false }
        )
        val result = api.executeToolCall(ToolCall("r1", "read_screen", emptyMap()), null)
        assertTrue(result.isError)
        assertEquals(ToolErrorCode.SERVICE_UNAVAILABLE, result.errorCode)
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

    @Test
    fun `web_search missing query returns INVALID_INPUT`() = runTest {
        val agent = createAgent()
        val result = agent.executeToolCall(ToolCall("ws1", "web_search", emptyMap()), null)
        assertTrue(result.isError)
        assertEquals(ToolErrorCode.INVALID_INPUT, result.errorCode)
        assertTrue(result.text.contains("Missing required parameter: query"))
    }

    @Test
    fun `web_fetch missing url returns INVALID_INPUT`() = runTest {
        val agent = createAgent()
        val result = agent.executeToolCall(ToolCall("wf1", "web_fetch", emptyMap()), null)
        assertTrue(result.isError)
        assertEquals(ToolErrorCode.INVALID_INPUT, result.errorCode)
        assertTrue(result.text.contains("Missing required parameter: url"))
    }

    @Test
    fun `web_browse missing url returns INVALID_INPUT when configured`() = runTest {
        val defaultClient = ClaudeClient(
            apiKey = "sk-ant-api03-test",
            systemPrompt = PhoneAgentPrompts.SYSTEM_PROMPT,
            baseUrl = server.url("/v1/messages").toString()
        )
        val agent = PhoneAgentApi(
            chatClient = defaultClient,
            actionClient = defaultClient,
            tinyFishApiKey = "test-tinyfish-key",
            tinyFishEndpoint = server.url("/v1/automation/run-sse").toString()
        ).also {
            it.phoneControlOverride = true
        }

        val result = agent.executeToolCall(ToolCall("wb1", "web_browse", mapOf("goal" to "Find answer")), null)
        assertTrue(result.isError)
        assertEquals(ToolErrorCode.INVALID_INPUT, result.errorCode)
        assertTrue(result.text.contains("Missing required parameter: url"))
    }

    @Test
    fun `web_browse missing goal returns INVALID_INPUT when configured`() = runTest {
        val defaultClient = ClaudeClient(
            apiKey = "sk-ant-api03-test",
            systemPrompt = PhoneAgentPrompts.SYSTEM_PROMPT,
            baseUrl = server.url("/v1/messages").toString()
        )
        val agent = PhoneAgentApi(
            chatClient = defaultClient,
            actionClient = defaultClient,
            tinyFishApiKey = "test-tinyfish-key",
            tinyFishEndpoint = server.url("/v1/automation/run-sse").toString()
        ).also {
            it.phoneControlOverride = true
        }

        val result = agent.executeToolCall(ToolCall("wb2", "web_browse", mapOf("url" to "https://example.com")), null)
        assertTrue(result.isError)
        assertEquals(ToolErrorCode.INVALID_INPUT, result.errorCode)
        assertTrue(result.text.contains("Missing required parameter: goal"))
    }

    @Test
    fun `request_tools validates missing empty and invalid categories`() = runTest {
        val agent = createAgent()

        val missing = agent.executeToolCall(ToolCall("rt1", "request_tools", emptyMap()), null)
        assertTrue(missing.isError)
        assertEquals(ToolErrorCode.INVALID_INPUT, missing.errorCode)
        assertTrue(missing.text.contains("Missing required parameter: categories"))

        val empty = agent.executeToolCall(ToolCall("rt2", "request_tools", mapOf("categories" to emptyList<String>())), null)
        assertTrue(empty.isError)
        assertEquals(ToolErrorCode.INVALID_INPUT, empty.errorCode)
        assertTrue(empty.text.contains("at least one category"))

        val invalid = agent.executeToolCall(
            ToolCall(
                "rt3",
                "request_tools",
                mapOf("categories" to listOf("research", "core", "", "invalid_category", 17))
            ),
            null
        )
        assertTrue(invalid.isError)
        assertEquals(ToolErrorCode.INVALID_INPUT, invalid.errorCode)
        assertTrue(invalid.text.contains("Invalid categories"))
        assertTrue(invalid.text.contains("core"))
        assertTrue(invalid.text.contains("invalid_category"))
    }

    @Test
    fun `request_tools returns tool list for valid categories`() = runTest {
        val agent = createAgent()
        val result = agent.executeToolCall(
            ToolCall("rt4", "request_tools", mapOf("categories" to listOf("navigation", "observation"))),
            null
        )

        assertFalse(result.isError)
        assertNull(result.errorCode)
        assertTrue(result.text.contains("Requested categories: navigation, observation"))
        assertTrue(result.text.contains("open_app"))
        assertTrue(result.text.contains("read_screen"))
    }

    @Test
    fun `tool error mapping is consistent across core ui web and meta tools`() = runTest {
        val agent = createAgent()
        val cases = listOf(
            ToolCall("map1", "nonexistent_tool", emptyMap()) to ToolErrorCode.TOOL_NOT_FOUND,
            ToolCall("map2", "web_search", emptyMap()) to ToolErrorCode.INVALID_INPUT,
            ToolCall("map3", "web_fetch", emptyMap()) to ToolErrorCode.INVALID_INPUT,
            ToolCall("map4", "web_browse", mapOf("url" to "https://example.com", "goal" to "Find")) to ToolErrorCode.NOT_CONFIGURED,
            ToolCall("map5", "request_tools", emptyMap()) to ToolErrorCode.INVALID_INPUT,
            ToolCall("map6", "read_screen", emptyMap()) to ToolErrorCode.SERVICE_UNAVAILABLE
        )

        for ((toolCall, expectedCode) in cases) {
            val result = agent.executeToolCall(toolCall, null)
            assertTrue(result.isError, "Expected isError=true for ${toolCall.name}, got: ${result.text}")
            assertEquals(expectedCode, result.errorCode, "Unexpected error code for ${toolCall.name}")
        }
    }

    @Test
    fun `tool error mapping is consistent for clipboard and notification tools`() = runTest {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val api = PhoneAgentApi(client)
        ClipboardHelper.detach()
        NotificationHelper.detach()

        val cases = listOf(
            ToolCall("map_clip_1", "copy", emptyMap()) to ToolErrorCode.SERVICE_UNAVAILABLE,
            ToolCall("map_clip_2", "set_clipboard", mapOf("text" to "")) to ToolErrorCode.INVALID_INPUT,
            ToolCall("map_clip_3", "set_clipboard", mapOf("text" to "hello")) to ToolErrorCode.SERVICE_UNAVAILABLE,
            ToolCall("map_clip_4", "paste", mapOf("text" to "")) to ToolErrorCode.INVALID_INPUT,
            ToolCall("map_clip_5", "paste", mapOf("text" to "hello")) to ToolErrorCode.SERVICE_UNAVAILABLE,
            ToolCall("map_notif_1", "read_notifications", emptyMap()) to ToolErrorCode.SERVICE_UNAVAILABLE,
            ToolCall("map_notif_2", "tap_notification", mapOf("notification_key" to "invalid")) to ToolErrorCode.INVALID_INPUT,
            ToolCall("map_notif_3", "tap_notification", mapOf("notification_key" to "0|com.example.app|123|null|1000")) to ToolErrorCode.SERVICE_UNAVAILABLE,
            ToolCall("map_notif_4", "dismiss_notification", mapOf("notification_key" to "invalid")) to ToolErrorCode.INVALID_INPUT,
            ToolCall("map_notif_5", "dismiss_notification", mapOf("notification_key" to "0|com.example.app|123|null|1000")) to ToolErrorCode.SERVICE_UNAVAILABLE,
            ToolCall("map_notif_6", "reply_notification", mapOf("notification_key" to "0|com.example.app|123|null|1000", "text" to "")) to ToolErrorCode.INVALID_INPUT,
            ToolCall("map_notif_7", "reply_notification", mapOf("notification_key" to "invalid", "text" to "hello")) to ToolErrorCode.INVALID_INPUT,
            ToolCall("map_notif_8", "reply_notification", mapOf("notification_key" to "0|com.example.app|123|null|1000", "text" to "hello")) to ToolErrorCode.SERVICE_UNAVAILABLE
        )

        for ((toolCall, expectedCode) in cases) {
            val result = api.executeToolCall(toolCall, null)
            assertTrue(result.isError, "Expected isError=true for ${toolCall.name}, got: ${result.text}")
            assertEquals(expectedCode, result.errorCode, "Unexpected error code for ${toolCall.name}")
        }
    }

    @Test
    fun `tool results with errorCode always set isError true`() = runTest {
        val agent = createAgent()
        val cases = listOf(
            ToolCall("inv1", "nonexistent_tool", emptyMap()),
            ToolCall("inv2", "web_search", emptyMap()),
            ToolCall("inv3", "web_fetch", emptyMap()),
            ToolCall("inv4", "request_tools", emptyMap()),
            ToolCall("inv5", "web_browse", mapOf("url" to "https://example.com", "goal" to "Find"))
        )

        for (toolCall in cases) {
            val result = agent.executeToolCall(toolCall, null)
            assertNotNull(result.errorCode, "Expected errorCode for ${toolCall.name}")
            assertTrue(result.isError, "Expected isError=true when errorCode is set for ${toolCall.name}")
        }
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
        class RecordingTinyFishClient : TinyFishBrowserClient {
            var capturedUrl: String? = null
            var capturedGoal: String? = null
            var capturedStealth: Boolean? = null

            override suspend fun browse(
                url: String,
                goal: String,
                stealth: Boolean,
                proxyCountry: String?,
                onProgress: ((String) -> Unit)?
            ): ToolResult {
                capturedUrl = url
                capturedGoal = goal
                capturedStealth = stealth
                return ToolResult("mock tinyfish success")
            }
        }

        val defaultClient = ClaudeClient(
            apiKey = "sk-ant-api03-test",
            systemPrompt = PhoneAgentPrompts.SYSTEM_PROMPT,
            baseUrl = server.url("/v1/messages").toString()
        )
        val fakeTinyFishClient = RecordingTinyFishClient()
        val agent = PhoneAgentApi(
            chatClient = defaultClient,
            actionClient = defaultClient,
            tinyFishApiKey = "test-tinyfish-key",
            tinyFishClientFactory = { _, _ -> fakeTinyFishClient }
        ).also {
            it.phoneControlOverride = true
        }

        val result = agent.executeToolCall(
            ToolCall("t1", "web_browse", mapOf("url" to "https://example.com", "goal" to "Find the answer")),
            null
        )

        assertFalse(result.isError, "Expected success but got error: " + result.text)
        assertEquals("mock tinyfish success", result.text)
        assertEquals("https://example.com", fakeTinyFishClient.capturedUrl)
        assertEquals("Find the answer", fakeTinyFishClient.capturedGoal)
        assertEquals(false, fakeTinyFishClient.capturedStealth)
    }
    // --- Conversation history seeding tests (#612) ---

    @Test
    fun `seedConversationHistory populates empty messages from UI`() = runTest {
        val agent = createAgent()
        assertEquals(0, agent.messageCount)

        val uiMessages = listOf(
            Message(role = "user", content = "what's the weather?"),
            Message(role = "assistant", content = "It's 38F and partly cloudy in Denver."),
            Message(role = "user", content = "should I bring a jacket?")
        )

        agent.seedConversationHistory(uiMessages)
        assertEquals(3, agent.messageCount)
    }

    @Test
    fun `seedConversationHistory is no-op when messages already exist`() = runTest {
        val agent = createAgent()

        agent.seedConversationHistory(listOf(
            Message(role = "user", content = "first turn"),
            Message(role = "assistant", content = "first response")
        ))
        assertEquals(2, agent.messageCount)

        // Second seed should be a no-op
        agent.seedConversationHistory(listOf(
            Message(role = "user", content = "should NOT appear"),
            Message(role = "assistant", content = "neither should this")
        ))
        assertEquals(2, agent.messageCount)  // unchanged
    }

    @Test
    fun `seedConversationHistory skips tool and blank messages`() = runTest {
        val agent = createAgent()
        val uiMessages = listOf(
            Message(role = "user", content = "do something"),
            Message(role = "tool", content = "tool result here"),
            Message(role = "assistant", content = "Done!"),
            Message(role = "assistant", content = "  ")  // blank, should be skipped
        )

        agent.seedConversationHistory(uiMessages)
        assertEquals(2, agent.messageCount)  // user + "Done!", tool and blank skipped
    }

    @Test
    fun `seedConversationHistory strips tool metadata from assistant messages`() = runTest {
        val agent = createAgent()
        val uiMessages = listOf(
            Message(role = "user", content = "open settings"),
            Message(role = "assistant", content = "Opened Settings [Tools: open_app, tap]")
        )

        agent.seedConversationHistory(uiMessages)
        assertEquals(2, agent.messageCount)
    }

    @Test
    fun `seedConversationHistory with empty UI messages is no-op`() = runTest {
        val agent = createAgent()
        agent.seedConversationHistory(emptyList())
        assertEquals(0, agent.messageCount)
    }

    @Test
    fun `seedConversationHistory deduplicates consecutive same-role messages`() = runTest {
        val agent = createAgent()
        // After stripping tool messages, two user messages end up adjacent
        val uiMessages = listOf(
            Message(role = "user", content = "do something"),
            Message(role = "tool", content = "tool result"),  // stripped
            Message(role = "user", content = "also do this"),  // steer, now adjacent to first user
            Message(role = "assistant", content = "Done!")
        )

        agent.seedConversationHistory(uiMessages)
        // user + user -> only first kept (consecutive dedup), then assistant = 2
        assertEquals(2, agent.messageCount)
    }

    // --- PR #619: seed conversation history content assertions ---

    @Suppress("UNCHECKED_CAST")
    private fun getMessages(agent: PhoneAgentApi): List<Message> {
        val field = PhoneAgentApi::class.java.getDeclaredField("messages")
        field.isAccessible = true
        return (field.get(agent) as List<Message>)
    }

    @Test
    fun `seedConversationHistory preserves message content and order`() = runTest {
        val agent = createAgent()
        val uiMessages = listOf(
            Message(role = "user", content = "what's the weather?"),
            Message(role = "assistant", content = "It's 38F and partly cloudy in Denver."),
            Message(role = "user", content = "should I bring a jacket?")
        )

        agent.seedConversationHistory(uiMessages)

        val seeded = getMessages(agent)
        assertEquals(3, seeded.size)
        assertEquals("user", seeded[0].role)
        assertEquals("what's the weather?", seeded[0].content)
        assertEquals("assistant", seeded[1].role)
        assertEquals("It's 38F and partly cloudy in Denver.", seeded[1].content)
        assertEquals("user", seeded[2].role)
        assertEquals("should I bring a jacket?", seeded[2].content)
    }

    @Test
    fun `seedConversationHistory with only tool messages results in empty seed`() = runTest {
        val agent = createAgent()
        val uiMessages = listOf(
            Message(role = "tool", content = "tool result 1"),
            Message(role = "tool", content = "tool result 2"),
            Message(role = "tool", content = "tool result 3")
        )

        agent.seedConversationHistory(uiMessages)
        assertEquals(0, agent.messageCount)
    }

    // ========== Thread Safety Tests (#644) ==========

    @Test
    fun `messages list supports concurrent iteration and modification without ConcurrentModificationException`() {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val agent = PhoneAgentApi(client, client)

        // Seed some initial messages
        val uiMessages = (1..50).map { i ->
            if (i % 2 == 1) Message(role = "user", content = "msg $i")
            else Message(role = "assistant", content = "reply $i")
        }
        agent.seedConversationHistory(uiMessages)

        // Use real threads via Dispatchers.Default for actual thread interleaving
        kotlinx.coroutines.runBlocking(kotlinx.coroutines.Dispatchers.Default) {
            val jobs = mutableListOf<kotlinx.coroutines.Job>()
            repeat(50) {
                jobs += launch {
                    // Read: access messageCount
                    agent.messageCount
                }
                jobs += launch {
                    // Write: add tool results
                    agent.addToolResult("tool_$it", "result_$it", "tap")
                }
            }
            jobs.forEach { it.join() }
        }
        // No ConcurrentModificationException = pass
    }

    @Test
    fun `seedConversationHistory and clearConversation interleaving is safe`() {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val agent = PhoneAgentApi(client, client)

        val uiMessages = listOf(
            Message(role = "user", content = "hello"),
            Message(role = "assistant", content = "hi there")
        )

        // Use real threads for actual concurrency
        kotlinx.coroutines.runBlocking(kotlinx.coroutines.Dispatchers.Default) {
            val jobs = (1..50).map { i ->
                launch {
                    if (i % 2 == 0) {
                        agent.seedConversationHistory(uiMessages)
                    } else {
                        agent.clearConversation()
                    }
                }
            }
            jobs.forEach { it.join() }
        }
        // No exception = pass. Final state is non-deterministic but safe.
    }

    @Test
    fun `addToolResult and addSteerMessage concurrent access is safe`() {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val agent = PhoneAgentApi(client, client)

        kotlinx.coroutines.runBlocking(kotlinx.coroutines.Dispatchers.Default) {
            val jobs = (1..20).map { i ->
                launch {
                    if (i % 2 == 0) {
                        agent.addToolResult("tool_$i", "result_$i", "tap")
                    } else {
                        agent.addSteerMessage("steer $i")
                    }
                }
            }
            jobs.forEach { it.join() }
        }

        // All 20 messages should be added (CopyOnWriteArrayList is safe for concurrent adds)
        assertEquals(20, agent.messageCount)
    }

    @Test
    fun `concurrent seedConversationHistory calls do not duplicate messages`() {
        val client = ScriptedProviderClient(Provider.ANTHROPIC, ArrayDeque(), ArrayDeque())
        val agent = PhoneAgentApi(client, client)

        val uiMessages = listOf(
            Message(role = "user", content = "hello"),
            Message(role = "assistant", content = "hi there")
        )

        // Launch many concurrent seed attempts — only one should succeed
        kotlinx.coroutines.runBlocking(kotlinx.coroutines.Dispatchers.Default) {
            val latch = java.util.concurrent.CountDownLatch(1)
            val jobs = (1..20).map {
                launch {
                    latch.await() // all threads start together
                    agent.seedConversationHistory(uiMessages)
                }
            }
            latch.countDown()
            jobs.forEach { it.join() }
        }

        // Synchronized ensures only one seed succeeds — exactly 2 messages
        assertEquals(2, agent.messageCount)
    }

    // ========== sendEphemeral system prompt regression (#606) ==========

    @Test
    fun `sendEphemeral passes non-null system prompt`() = runTest {
        val client = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            toolResponses = ArrayDeque(listOf(
                ChatResponse(text = "Summary of progress", toolCalls = emptyList(), stopReason = "end_turn")
            ))
        )
        val agent = PhoneAgentApi(chatClient = client, actionClient = client).also {
            it.phoneControlOverride = true
        }

        val result = agent.sendEphemeral("[System: Summarize progress]")

        assertEquals("Summary of progress", result)
        assertNotNull(client.lastSystemPrompt, "sendEphemeral must pass a non-null system prompt (#606)")
        assertTrue(client.lastSystemPrompt!!.isNotBlank(), "system prompt must not be blank")
    }


    @Test
    fun `sendEphemeral records usage and enforces budget`() = runTest {
        val client = ScriptedProviderClient(
            provider = Provider.OPENAI,
            modelId = "gpt-4o-mini",
            toolResponses = ArrayDeque(
                listOf(
                    ChatResponse(
                        text = "Summary one",
                        toolCalls = emptyList(),
                        stopReason = "end_turn",
                        usage = TokenUsage(inputTokens = 1_000, outputTokens = 0)
                    ),
                    ChatResponse(
                        text = "Summary two",
                        toolCalls = emptyList(),
                        stopReason = "end_turn",
                        usage = TokenUsage(inputTokens = 1_000, outputTokens = 0)
                    )
                )
            )
        )
        val budgetStore = InMemoryBudgetStore(
            BudgetConfig(enabled = true, dailyLimitUsd = 0.0002, monthlyLimitUsd = 10.0)
        )
        val agent = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            budgetGuard = BudgetGuard(budgetStore)
        ).also { it.phoneControlOverride = true }

        val first = agent.sendEphemeral("[System: Summarize progress]")
        assertEquals("Summary one", first)
        assertEquals(1, agent.messageCount)
        val beforeSecond = getMessages(agent).toList()

        assertFailsWith<BudgetExceededException> {
            agent.sendEphemeral("[System: Summarize progress again]")
        }
        assertEquals(300L, budgetStore.getDailySpentMicrodollars())
        assertEquals(beforeSecond, getMessages(agent), "sendEphemeral over-limit should not persist assistant reply")
    }


    // --- Sensor/timeout/concurrency CI-targeted tests ---
    // Keep test names containing one of: sensor, Sensor, timeout, concurrent,
    // so :core:phoneAgentApiSensorCiTest includes this section via Gradle filter patterns.

    @Test
    fun `sendMessage handles SensorProvider snapshot exceptions gracefully`() = runTest {
        val client = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            toolResponses = ArrayDeque(listOf(
                ChatResponse(text = "ok", toolCalls = emptyList(), stopReason = "end_turn")
            ))
        )
        val throwingProvider = object : SensorProvider {
            override suspend fun snapshot(): SensorContext {
                throw IllegalStateException("boom")
            }
        }
        val agent = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            sensorProvider = throwingProvider
        ).also { it.phoneControlOverride = true }

        val response = agent.sendMessage("Open Settings", screenContent = null, isActionLoop = false)

        assertEquals("ok", response.text)
        assertNotNull(client.lastSystemPrompt)
        assertFalse(client.lastSystemPrompt!!.contains("Device Awareness"))
    }

    @Test
    fun `sendMessage rethrows SensorProvider CancellationException`() = runTest {
        val client = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            toolResponses = ArrayDeque(listOf(
                ChatResponse(text = "ok", toolCalls = emptyList(), stopReason = "end_turn")
            ))
        )
        val cancellingProvider = object : SensorProvider {
            override suspend fun snapshot(): SensorContext {
                throw kotlinx.coroutines.CancellationException("cancelled")
            }
        }
        val agent = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            sensorProvider = cancellingProvider
        ).also { it.phoneControlOverride = true }

        assertFailsWith<kotlinx.coroutines.CancellationException> {
            agent.sendMessage("Open Settings", screenContent = null, isActionLoop = false)
        }
    }

    @Test
    fun `sendMessage non-action loop injects sensor context into system prompt integration`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"content":[{"type":"text","text":"Done"}],"role":"assistant","stop_reason":"end_turn"}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val sensorProvider = object : SensorProvider {
            override suspend fun snapshot(): SensorContext =
                SensorContext(batteryPercent = 44, networkType = NetworkType.WIFI)
        }
        val agent = createAgent(sensorProvider = sensorProvider)

        val response = agent.sendMessage("Open Settings", screenContent = null, isActionLoop = false)

        assertEquals("Done", response.text)
        val request = server.takeRequest()
        val body = request.body.readUtf8()
        assertTrue(body.contains("Device: battery=44% | wifi"))
    }

    @Test
    fun `sendMessage action loop captures sensor snapshot once per task`() = runTest {
        val client = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            toolResponses = ArrayDeque(listOf(
                ChatResponse(text = "ok", toolCalls = emptyList(), stopReason = "end_turn")
            ))
        )
        var snapshotCalls = 0
        val countingProvider = object : SensorProvider {
            override suspend fun snapshot(): SensorContext {
                snapshotCalls++
                return SensorContext(batteryPercent = 44, networkType = NetworkType.WIFI)
            }
        }
        val agent = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            sensorProvider = countingProvider
        ).also { it.phoneControlOverride = true }

        val response = agent.sendMessage("continue", screenContent = null, isActionLoop = true)

        assertEquals("ok", response.text)
        assertEquals(1, snapshotCalls)
    }

    @Test
    fun `sendMessage action loop without prior task start injects fresh sensor context into system prompt`() = runTest {
        val client = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            toolResponses = ArrayDeque(listOf(
                ChatResponse(text = "ok", toolCalls = emptyList(), stopReason = "end_turn")
            ))
        )
        val sensorProvider = object : SensorProvider {
            override suspend fun snapshot(): SensorContext =
                SensorContext(batteryPercent = 44, networkType = NetworkType.WIFI)
        }
        val agent = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            sensorProvider = sensorProvider
        ).also { it.phoneControlOverride = true }

        val response = agent.sendMessage("continue", screenContent = null, isActionLoop = true)

        assertEquals("ok", response.text)
        assertNotNull(client.lastSystemPrompt)
        assertTrue(client.lastSystemPrompt!!.contains("Device: battery=44% | wifi"))
    }

    @Test
    fun `sensor snapshot is reused for continuation prompts within a task`() = runTest {
        val chatClient = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            toolResponses = ArrayDeque(listOf(
                ChatResponse(
                    text = null,
                    toolCalls = listOf(ToolCall("tc1", "tap", mapOf("element_id" to 7))),
                    stopReason = "tool_use"
                )
            ))
        )
        val actionClient = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            toolResponses = ArrayDeque(listOf(
                ChatResponse(text = "Done", toolCalls = emptyList(), stopReason = "end_turn")
            ))
        )
        var snapshotCalls = 0
        val batterySeries = ArrayDeque(listOf(44, 66))
        val sensorProvider = object : SensorProvider {
            override suspend fun snapshot(): SensorContext {
                snapshotCalls++
                return SensorContext(
                    batteryPercent = batterySeries.first(),
                    networkType = NetworkType.WIFI,
                    localTime = java.time.ZonedDateTime.now(java.time.ZoneOffset.UTC).minusSeconds(45)
                )
            }
        }
        val agent = PhoneAgentApi(
            chatClient = chatClient,
            actionClient = actionClient,
            sensorProvider = sensorProvider
        ).also { it.phoneControlOverride = true }

        val initial = agent.sendMessage("Tap it", screenContent = null, isActionLoop = false)
        assertEquals(1, snapshotCalls)
        assertEquals(1, chatClient.chatWithToolsCalls)
        assertNotNull(chatClient.lastSystemPrompt)
        assertTrue(chatClient.lastSystemPrompt!!.contains("Device: battery=44% | wifi"))
        assertTrue(initial.toolCalls.isNotEmpty())

        agent.addToolResult("tc1", "Tapped element 7")
        val continuation = agent.continueAfterTools()

        assertEquals("Done", continuation.text)
        assertEquals(1, snapshotCalls)
        assertEquals(1, actionClient.chatWithToolsCalls)
        assertNotNull(actionClient.lastSystemPrompt)
        assertTrue(actionClient.lastSystemPrompt!!.contains("Device: battery=44% | wifi"))
        assertFalse(actionClient.lastSystemPrompt!!.contains("## Device Awareness"))
    }

    @Test
    fun `clearConversation clears prior sensor context before subsequent action loop prompt`() = runTest {
        val client = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            toolResponses = ArrayDeque(
                listOf(
                    ChatResponse(text = "first", toolCalls = emptyList(), stopReason = "end_turn"),
                    ChatResponse(text = "second", toolCalls = emptyList(), stopReason = "end_turn")
                )
            )
        )
        val sensorProvider = object : SensorProvider {
            private val values = ArrayDeque(listOf(44, 88))
            override suspend fun snapshot(): SensorContext =
                SensorContext(batteryPercent = values.removeFirst(), networkType = NetworkType.WIFI)
        }
        val agent = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            sensorProvider = sensorProvider
        ).also { it.phoneControlOverride = true }

        agent.sendMessage("Open settings", screenContent = null, isActionLoop = false)
        assertNotNull(client.lastSystemPrompt)
        assertTrue(client.lastSystemPrompt!!.contains("Device: battery=44% | wifi"))

        agent.clearConversation()
        agent.sendMessage("continue", screenContent = null, isActionLoop = true)

        assertNotNull(client.lastSystemPrompt)
        assertTrue(client.lastSystemPrompt!!.contains("Device: battery=88% | wifi"))
        assertFalse(client.lastSystemPrompt!!.contains("Device: battery=44% | wifi"))
    }

    @Test
    fun `concurrent action-loop sends reuse one sensor snapshot for the current task`() = runTest {
        val promptLog = java.util.Collections.synchronizedList(mutableListOf<String>())
        val client = object : ProviderClient {
            override val provider: Provider = Provider.ANTHROPIC
            override val modelId: String? = null

            override suspend fun chat(conversation: Conversation): Result<String> = Result.success("chat")

            override suspend fun chatWithTools(
                messages: List<Message>,
                systemPrompt: String?,
                tools: List<Tool>,
                tokenLimit: Int?
            ): Result<ChatResponse> {
                if (systemPrompt != null) promptLog.add(systemPrompt)
                return Result.success(ChatResponse(text = "ok", toolCalls = emptyList(), stopReason = "end_turn"))
            }

            override suspend fun describeImage(base64Image: String, prompt: String, maxTokens: Int): Result<String> {
                return Result.success("desc")
            }
        }

        val snapshotCalls = java.util.concurrent.atomic.AtomicInteger(0)
        val sensorProvider = object : SensorProvider {
            override suspend fun snapshot(): SensorContext {
                val next = snapshotCalls.incrementAndGet()
                // Deterministically exposes check-then-set races when cache access is unsynchronized.
                delay(10)
                return SensorContext(batteryPercent = next, networkType = NetworkType.WIFI)
            }
        }
        val agent = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            sensorProvider = sensorProvider
        ).also { it.phoneControlOverride = true }

        kotlinx.coroutines.coroutineScope {
            repeat(6) { idx ->
                launch {
                    agent.sendMessage("continue-$idx", screenContent = null, isActionLoop = true)
                }
            }
        }

        assertEquals(1, snapshotCalls.get())
        assertEquals(6, promptLog.size)
        promptLog.forEach { prompt ->
            assertTrue(prompt.contains("Device: battery=1% | wifi"))
        }
    }

    @Test
    fun `sensor snapshot exceptions are counted as failures and return no metadata`() = runTest {
        val client = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            toolResponses = ArrayDeque(
                listOf(
                    ChatResponse(text = "ok", toolCalls = emptyList(), stopReason = "end_turn")
                )
            )
        )
        val sensorProvider = object : SensorProvider {
            override suspend fun snapshot(): SensorContext {
                error("boom")
            }
        }
        val agent = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            sensorProvider = sensorProvider
        ).also { it.phoneControlOverride = true }

        agent.sendMessage("Open settings", screenContent = null, isActionLoop = false)

        assertEquals(1, agent.sensorSnapshotFailureTotal)
        assertEquals(0, agent.sensorSnapshotTimeoutTotal)
        assertNull(agent.cachedTaskSensorSnapshot)
        assertNotNull(client.lastSystemPrompt)
        assertFalse(client.lastSystemPrompt!!.contains("Device:"))
    }

    @Test
    fun `sensor snapshot timeout increments timeout counter and keeps prompt metadata empty`() = runTest {
        val client = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            toolResponses = ArrayDeque(
                listOf(
                    ChatResponse(text = "ok", toolCalls = emptyList(), stopReason = "end_turn")
                )
            )
        )
        val sensorProvider = object : SensorProvider {
            override suspend fun snapshot(): SensorContext {
                kotlinx.coroutines.withTimeout(1) {
                    delay(10)
                }
                return SensorContext(batteryPercent = 77, networkType = NetworkType.WIFI)
            }
        }
        val agent = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            sensorProvider = sensorProvider
        ).also { it.phoneControlOverride = true }

        agent.sendMessage("Open settings", screenContent = null, isActionLoop = false)

        assertEquals(0, agent.sensorSnapshotFailureTotal)
        assertEquals(1, agent.sensorSnapshotTimeoutTotal)
        assertNull(agent.cachedTaskSensorSnapshot)
        assertNotNull(client.lastSystemPrompt)
        assertFalse(client.lastSystemPrompt!!.contains("Device:"))
    }

    @Test
    fun `sensor snapshot task-start budget is enforced by timeout constant`() = runTest {
        val client = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            toolResponses = ArrayDeque(
                listOf(ChatResponse(text = "ok", toolCalls = emptyList(), stopReason = "end_turn"))
            )
        )
        val sensorProvider = object : SensorProvider {
            override suspend fun snapshot(): SensorContext {
                delay(PhoneAgentApi.SENSOR_SNAPSHOT_TIMEOUT_MS + 100)
                return SensorContext(batteryPercent = 50, networkType = NetworkType.WIFI)
            }
        }
        val agent = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            sensorProvider = sensorProvider
        ).also { it.phoneControlOverride = true }

        val elapsedMs = measureTimeMillis {
            agent.sendMessage("Open settings", screenContent = null, isActionLoop = false)
        }

        assertTrue(
            elapsedMs < (PhoneAgentApi.SENSOR_SNAPSHOT_TIMEOUT_MS + 300),
            "Task start should not wait for slow sensor capture (elapsed=${elapsedMs}ms)"
        )
        assertEquals(1, agent.sensorSnapshotTimeoutTotal)
        assertEquals(0, agent.sensorSnapshotFailureTotal)
        assertEquals(1, agent.sensorSnapshotTotal)
        assertTrue(agent.sensorSnapshotLatencyTotalMs > 0)
        assertTrue(agent.sensorSnapshotLatencyMaxMs > 0)
        assertNotNull(client.lastSystemPrompt)
        assertFalse(client.lastSystemPrompt!!.contains("Device:"))
    }

    @Test
    fun `sensor snapshot preserves non-location fields when capture is slower than old 15ms budget`() = runTest {
        val client = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            toolResponses = ArrayDeque(
                listOf(
                    ChatResponse(text = "ok", toolCalls = emptyList(), stopReason = "end_turn")
                )
            )
        )
        val sensorProvider = object : SensorProvider {
            override suspend fun snapshot(): SensorContext {
                // Simulate a slow location branch while still returning partial sensor fields.
                delay(20)
                return SensorContext(
                    batteryPercent = 77,
                    networkType = NetworkType.WIFI,
                    localTime = java.time.ZonedDateTime.now(java.time.ZoneOffset.UTC),
                    location = null
                )
            }
        }
        val agent = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            sensorProvider = sensorProvider
        ).also { it.phoneControlOverride = true }

        agent.sendMessage("Open settings", screenContent = null, isActionLoop = false)

        assertEquals(0, agent.sensorSnapshotFailureTotal)
        assertEquals(0, agent.sensorSnapshotTimeoutTotal)
        assertNotNull(agent.cachedTaskSensorSnapshot)
        assertEquals(77, agent.cachedTaskSensorSnapshot!!.batteryPercent)
        assertEquals(NetworkType.WIFI, agent.cachedTaskSensorSnapshot!!.networkType)
        assertNull(agent.cachedTaskSensorSnapshot!!.location)
        assertNotNull(client.lastSystemPrompt)
        assertTrue(client.lastSystemPrompt!!.contains("Device: battery=77% | wifi"))
        assertFalse(client.lastSystemPrompt!!.contains("location="))
    }

    @Test
    fun `clearConversation does not block when sensor snapshot is in flight`() = runTest(timeout = 5_000.milliseconds) {
        val client = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            toolResponses = ArrayDeque(
                listOf(ChatResponse(text = "ok", toolCalls = emptyList(), stopReason = "end_turn"))
            )
        )
        val started = CompletableDeferred<Unit>()
        val unblock = CompletableDeferred<Unit>()
        val sensorProvider = object : SensorProvider {
            override suspend fun snapshot(): SensorContext {
                started.complete(Unit)
                unblock.await()
                return SensorContext(batteryPercent = 77, networkType = NetworkType.WIFI)
            }
        }
        val agent = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            sensorProvider = sensorProvider
        ).also { it.phoneControlOverride = true }

        val sendJob = launch {
            agent.sendMessage("Open settings", screenContent = null, isActionLoop = false)
        }
        started.await()

        val clearElapsedMs = measureTimeMillis { agent.clearConversation() }
        assertTrue(clearElapsedMs < 50, "clearConversation should return immediately (elapsed=${clearElapsedMs}ms)")

        unblock.complete(Unit)
        sendJob.join()

        assertNull(agent.cachedTaskSensorSnapshot, "In-flight snapshot from prior epoch must not be cached")
    }

    @Test
    fun `sensor snapshot failure on task start is cached and not retried on continuation turns`() = runTest {
        val chatClient = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            toolResponses = ArrayDeque(
                listOf(
                    ChatResponse(
                        text = null,
                        toolCalls = listOf(ToolCall("tc1", "tap", mapOf("element_id" to 5))),
                        stopReason = "tool_use"
                    )
                )
            )
        )
        val actionClient = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            toolResponses = ArrayDeque(
                listOf(ChatResponse(text = "done", toolCalls = emptyList(), stopReason = "end_turn"))
            )
        )
        var snapshotCalls = 0
        val sensorProvider = object : SensorProvider {
            override suspend fun snapshot(): SensorContext {
                snapshotCalls++
                error("boom")
            }
        }
        val agent = PhoneAgentApi(
            chatClient = chatClient,
            actionClient = actionClient,
            sensorProvider = sensorProvider
        ).also { it.phoneControlOverride = true }

        val first = agent.sendMessage("Tap it", screenContent = null, isActionLoop = false)
        assertTrue(first.toolCalls.isNotEmpty())
        agent.addToolResult("tc1", "Tapped element 5")
        val second = agent.continueAfterTools()

        assertEquals("done", second.text)
        assertEquals(1, snapshotCalls)
        assertEquals(1, agent.sensorSnapshotFailureTotal)
        assertEquals(0, agent.sensorSnapshotTimeoutTotal)
    }

    @Test
    fun `sensor snapshot timeout on task start is cached and not retried on continuation turns`() = runTest {
        val chatClient = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            toolResponses = ArrayDeque(
                listOf(
                    ChatResponse(
                        text = null,
                        toolCalls = listOf(ToolCall("tc1", "tap", mapOf("element_id" to 9))),
                        stopReason = "tool_use"
                    )
                )
            )
        )
        val actionClient = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            toolResponses = ArrayDeque(
                listOf(ChatResponse(text = "done", toolCalls = emptyList(), stopReason = "end_turn"))
            )
        )
        var snapshotCalls = 0
        val sensorProvider = object : SensorProvider {
            override suspend fun snapshot(): SensorContext {
                snapshotCalls++
                kotlinx.coroutines.delay(PhoneAgentApi.SENSOR_SNAPSHOT_TIMEOUT_MS + 20)
                return SensorContext(batteryPercent = 77, networkType = NetworkType.WIFI)
            }
        }
        val agent = PhoneAgentApi(
            chatClient = chatClient,
            actionClient = actionClient,
            sensorProvider = sensorProvider
        ).also { it.phoneControlOverride = true }

        val first = agent.sendMessage("Tap it", screenContent = null, isActionLoop = false)
        assertTrue(first.toolCalls.isNotEmpty())
        agent.addToolResult("tc1", "Tapped element 9")
        val second = agent.continueAfterTools()

        assertEquals("done", second.text)
        assertEquals(1, snapshotCalls)
        assertEquals(0, agent.sensorSnapshotFailureTotal)
        assertEquals(1, agent.sensorSnapshotTimeoutTotal)
    }

    @Test
    fun `sensor context toggle flow updates prompt payload behavior end to end`() = runTest {
        val promptLog = mutableListOf<String>()
        val client = object : ProviderClient {
            override val provider: Provider = Provider.ANTHROPIC
            override val modelId: String? = null

            override suspend fun chat(conversation: Conversation): Result<String> = Result.success("chat")

            override suspend fun chatWithTools(
                messages: List<Message>,
                systemPrompt: String?,
                tools: List<Tool>,
                tokenLimit: Int?
            ): Result<ChatResponse> {
                if (systemPrompt != null) promptLog.add(systemPrompt)
                return Result.success(ChatResponse(text = "ok", toolCalls = emptyList(), stopReason = "end_turn"))
            }

            override suspend fun describeImage(base64Image: String, prompt: String, maxTokens: Int): Result<String> {
                return Result.success("desc")
            }
        }

        var sensorContextEnabled = true
        val sensorProvider = object : SensorProvider {
            override suspend fun snapshot(): SensorContext {
                return if (sensorContextEnabled) {
                    SensorContext(batteryPercent = 55, networkType = NetworkType.WIFI)
                } else {
                    SensorContext()
                }
            }
        }
        val agent = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            sensorProvider = sensorProvider
        ).also { it.phoneControlOverride = true }

        agent.sendMessage("Open Settings", screenContent = null, isActionLoop = false)
        assertTrue(promptLog.last().contains("Device: battery=55% | wifi"))

        sensorContextEnabled = false
        agent.clearConversation()
        agent.sendMessage("Open Settings again", screenContent = null, isActionLoop = false)
        assertFalse(promptLog.last().contains("Device:"))
    }

    @Test
    fun `sendEphemeral injects sensor context into system prompt`() = runTest {
        val client = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            toolResponses = ArrayDeque(listOf(
                ChatResponse(text = "Summary", toolCalls = emptyList(), stopReason = "end_turn")
            ))
        )
        val sensorProvider = object : SensorProvider {
            override suspend fun snapshot(): SensorContext =
                SensorContext(batteryPercent = 44, networkType = NetworkType.WIFI)
        }
        val agent = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            sensorProvider = sensorProvider
        ).also { it.phoneControlOverride = true }

        val result = agent.sendEphemeral("[System: Summarize progress]")

        assertEquals("Summary", result)
        assertNotNull(client.lastSystemPrompt)
        assertTrue(client.lastSystemPrompt!!.contains("Device: battery=44% | wifi"))
    }

    @Test
    fun `sendEphemeral swallows SensorProvider exception and still succeeds`() = runTest {
        val client = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            toolResponses = ArrayDeque(listOf(
                ChatResponse(text = "Summary", toolCalls = emptyList(), stopReason = "end_turn")
            ))
        )
        val throwingProvider = object : SensorProvider {
            override suspend fun snapshot(): SensorContext {
                throw IllegalStateException("boom")
            }
        }
        val agent = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            sensorProvider = throwingProvider
        ).also { it.phoneControlOverride = true }

        val result = agent.sendEphemeral("[System: Summarize progress]")

        assertEquals("Summary", result)
        assertNotNull(client.lastSystemPrompt)
        assertFalse(client.lastSystemPrompt!!.contains("Device Awareness"))
    }

    @Test
    fun `sendEphemeral rethrows SensorProvider CancellationException`() = runTest {
        val client = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            toolResponses = ArrayDeque(listOf(
                ChatResponse(text = "Summary", toolCalls = emptyList(), stopReason = "end_turn")
            ))
        )
        val cancellingProvider = object : SensorProvider {
            override suspend fun snapshot(): SensorContext {
                throw kotlinx.coroutines.CancellationException("cancelled")
            }
        }
        val agent = PhoneAgentApi(
            chatClient = client,
            actionClient = client,
            sensorProvider = cancellingProvider
        ).also { it.phoneControlOverride = true }

        assertFailsWith<kotlinx.coroutines.CancellationException> {
            agent.sendEphemeral("[System: Summarize progress]")
        }    }

    // ── Learn tool tests ──

    @Test
    fun `learn tool records pattern to knowledge file`() = runTest {
        val tempDir = java.io.File.createTempFile("agent-test", "").also { it.delete(); it.mkdirs() }
        try {
            val fileManager = AgentFileManager.fromDirectory(tempDir)
            val agent = createAgent(fileManager = fileManager)

            val result = agent.executeToolCall(
                ToolCall("l1", "learn", mapOf(
                    "app_package" to "com.test.app",
                    "pattern" to "Tap by text works better than element ID",
                    "category" to "navigation"
                )),
                null
            )

            assertFalse(result.isError, "learn should succeed: ${result.text}")
            assertTrue(result.text.contains("Learned pattern"))
            assertTrue(result.text.contains("com.test.app"))

            // Verify persisted
            val knowledge = fileManager.readKnowledge("com.test.app")
            assertNotNull(knowledge, "Knowledge file should exist")
            assertTrue(knowledge!!.contains("Tap by text works better"))
        } finally {
            tempDir.deleteRecursively()
        }
    }

    @Test
    fun `learn tool fails without file manager`() = runTest {
        val agent = createAgent(fileManager = null)
        val result = agent.executeToolCall(
            ToolCall("l1", "learn", mapOf(
                "app_package" to "com.test.app",
                "pattern" to "Test pattern"
            )),
            null
        )
        assertTrue(result.isError, "learn should fail without file manager")
    }

    @Test
    fun `learn tool fails with empty app_package`() = runTest {
        val tempDir = java.io.File.createTempFile("agent-test", "").also { it.delete(); it.mkdirs() }
        try {
            val fileManager = AgentFileManager.fromDirectory(tempDir)
            val agent = createAgent(fileManager = fileManager)

            val result = agent.executeToolCall(
                ToolCall("l1", "learn", mapOf(
                    "app_package" to "",
                    "pattern" to "Test pattern"
                )),
                null
            )
            assertTrue(result.isError, "learn should fail with empty app_package")
        } finally {
            tempDir.deleteRecursively()
        }
    }

    @Test
    fun `learn tool fails with malformed app_package`() = runTest {
        val tempDir = java.io.File.createTempFile("agent-test", "").also { it.delete(); it.mkdirs() }
        try {
            val fileManager = AgentFileManager.fromDirectory(tempDir)
            val agent = createAgent(fileManager = fileManager)

            val result = agent.executeToolCall(
                ToolCall("l1", "learn", mapOf(
                    "app_package" to "../",
                    "pattern" to "Bad package should be rejected"
                )),
                null
            )

            assertTrue(result.isError, "learn should fail with malformed app_package")
            assertTrue(result.text.contains("valid Android package name"), "Expected validation message, got: ${result.text}")
            assertTrue(fileManager.listKnowledgePackages().isEmpty(), "No knowledge file should be created")
        } finally {
            tempDir.deleteRecursively()
        }
    }

    @Test
    fun `learn tool fails with invalid category`() = runTest {
        val tempDir = java.io.File.createTempFile("agent-test", "").also { it.delete(); it.mkdirs() }
        try {
            val fileManager = AgentFileManager.fromDirectory(tempDir)
            val agent = createAgent(fileManager = fileManager)

            val result = agent.executeToolCall(
                ToolCall("l1", "learn", mapOf(
                    "app_package" to "com.test.app",
                    "pattern" to "Test",
                    "category" to "bogus"
                )),
                null
            )
            assertTrue(result.isError, "learn should fail with invalid category")
        } finally {
            tempDir.deleteRecursively()
        }
    }

    @Test
    fun `learn tool normalizes category case and whitespace`() = runTest {
        val tempDir = java.io.File.createTempFile("agent-test", "").also { it.delete(); it.mkdirs() }
        try {
            val fileManager = AgentFileManager.fromDirectory(tempDir)
            val agent = createAgent(fileManager = fileManager)

            val result = agent.executeToolCall(
                ToolCall("l1", "learn", mapOf(
                    "app_package" to "com.test.app",
                    "pattern" to "Mixed-case category test",
                    "category" to "  Navigation  "
                )),
                null
            )

            assertFalse(result.isError, "learn should accept normalized category: ${result.text}")
            val knowledge = fileManager.readKnowledge("com.test.app")!!
            assertTrue(knowledge.contains("## Navigation"), "Category header should be normalized")
            assertTrue(result.text.contains("[navigation]"), "Response should use normalized category")
        } finally {
            tempDir.deleteRecursively()
        }
    }

    @Test
    fun `learn tool defaults category to navigation`() = runTest {
        val tempDir = java.io.File.createTempFile("agent-test", "").also { it.delete(); it.mkdirs() }
        try {
            val fileManager = AgentFileManager.fromDirectory(tempDir)
            val agent = createAgent(fileManager = fileManager)

            val result = agent.executeToolCall(
                ToolCall("l1", "learn", mapOf(
                    "app_package" to "com.test.app",
                    "pattern" to "Default category test"
                )),
                null
            )

            assertFalse(result.isError)
            val knowledge = fileManager.readKnowledge("com.test.app")!!
            assertTrue(knowledge.contains("## Navigation"), "Should default to navigation category")
        } finally {
            tempDir.deleteRecursively()
        }
    }
}
