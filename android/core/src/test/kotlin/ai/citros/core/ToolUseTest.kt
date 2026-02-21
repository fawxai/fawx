package ai.citros.core

import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonArray
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
import kotlin.test.assertTrue

class ToolUseTest {

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

    private fun createAnthropicClient(
        apiKey: String = "sk-ant-api03-test-key",
        systemPrompt: String = "You are a test assistant."
    ): ClaudeClient {
        return ClaudeClient(
            apiKey = apiKey,
            systemPrompt = systemPrompt,
            baseUrl = server.url("/v1/messages").toString()
        )
    }

    private fun createOpenRouterClient(
        apiKey: String = "sk-or-test-key",
        systemPrompt: String = "You are a test assistant."
    ): ProviderClient {
        val config = ProviderConfig.openRouter(apiKey)
        return OpenRouterClient(
            config = ProviderConfig(
                provider = Provider.OPENROUTER,
                baseUrl = server.url("/api/v1/chat/completions").toString(),
                chatModelId = config.chatModelId,
                actionModelId = config.actionModelId,
                headers = config.headers
            ),
            systemPrompt = systemPrompt
        )
    }

    // ========== Anthropic Tool Use Tests ==========

    @Test
    fun `chatWithTools sends Anthropic tool use request format correctly`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"content":[{"type":"text","text":"I'll tap that"}],"role":"assistant","stop_reason":"end_turn"}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val client = createAnthropicClient()
        val messages = listOf(
            Message(role = "user", content = "Tap element 5")
        )

        client.chatWithTools(messages, tools = listOf(PhoneTools.TAP))

        val request = server.takeRequest()
        val body = request.body.readUtf8()
        val json = Json { ignoreUnknownKeys = true }
        val requestJson = json.parseToJsonElement(body).jsonObject

        // Verify tools array is included
        val tools = requestJson["tools"]?.jsonArray
        assertNotNull(tools, "Request should include tools array")
        assertTrue(tools.isNotEmpty(), "Tools array should not be empty")

        // Verify tool structure
        val firstTool = tools[0].jsonObject
        assertEquals("tap", firstTool["name"]?.jsonPrimitive?.content)
        assertNotNull(firstTool["description"])
        assertNotNull(firstTool["input_schema"])

        // Verify input_schema structure
        val inputSchema = firstTool["input_schema"]?.jsonObject
        assertEquals("object", inputSchema?.get("type")?.jsonPrimitive?.content)
    }

    @Test
    fun `chatWithTools parses Anthropic tool use response correctly`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"content":[{"type":"tool_use","id":"toolu_123","name":"tap","input":{"element_id":5}}],"role":"assistant","stop_reason":"tool_use"}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val client = createAnthropicClient()
        val messages = listOf(
            Message(role = "user", content = "Tap element 5")
        )

        val result = client.chatWithTools(messages)

        assertTrue(result.isSuccess)
        val response = result.getOrNull()
        assertNotNull(response)

        // Verify tool call extracted
        assertEquals(1, response.toolCalls.size)
        val toolCall = response.toolCalls[0]
        assertEquals("toolu_123", toolCall.id)
        assertEquals("tap", toolCall.name)
        assertEquals(5, toolCall.input["element_id"])
        assertEquals("tool_use", response.stopReason)
    }

    @Test
    fun `chatWithTools parses Anthropic text-only response correctly`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"content":[{"type":"text","text":"Task complete!"}],"role":"assistant","stop_reason":"end_turn"}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val client = createAnthropicClient()
        val messages = listOf(
            Message(role = "user", content = "Are you done?")
        )

        val result = client.chatWithTools(messages)

        assertTrue(result.isSuccess)
        val response = result.getOrNull()
        assertNotNull(response)

        // Verify text response with no tool calls
        assertEquals("Task complete!", response.text)
        assertTrue(response.toolCalls.isEmpty())
        assertEquals("end_turn", response.stopReason)
    }

    @Test
    fun `chatWithTools parses Anthropic mixed text and tool use response`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"content":[{"type":"text","text":"I'll tap that element"},{"type":"tool_use","id":"toolu_456","name":"tap","input":{"element_id":3}}],"role":"assistant","stop_reason":"tool_use"}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val client = createAnthropicClient()
        val messages = listOf(
            Message(role = "user", content = "Tap element 3")
        )

        val result = client.chatWithTools(messages)

        assertTrue(result.isSuccess)
        val response = result.getOrNull()
        assertNotNull(response)

        // Verify both text and tool call extracted
        assertEquals("I'll tap that element", response.text)
        assertEquals(1, response.toolCalls.size)
        val toolCall = response.toolCalls[0]
        assertEquals("toolu_456", toolCall.id)
        assertEquals("tap", toolCall.name)
        assertEquals(3, toolCall.input["element_id"])
    }

    @Test
    fun `chatWithTools parses Anthropic multiple tool calls in single response`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"content":[{"type":"tool_use","id":"t1","name":"tap","input":{"element_id":1}},{"type":"tool_use","id":"t2","name":"type_text","input":{"text":"hello"}}],"role":"assistant","stop_reason":"tool_use"}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val client = createAnthropicClient()
        val messages = listOf(
            Message(role = "user", content = "Do multiple actions")
        )

        val result = client.chatWithTools(messages)

        assertTrue(result.isSuccess)
        val response = result.getOrNull()
        assertNotNull(response)

        // Verify multiple tool calls parsed
        assertEquals(2, response.toolCalls.size)
        
        val call1 = response.toolCalls[0]
        assertEquals("t1", call1.id)
        assertEquals("tap", call1.name)
        assertEquals(1, call1.input["element_id"])
        
        val call2 = response.toolCalls[1]
        assertEquals("t2", call2.id)
        assertEquals("type_text", call2.name)
        assertEquals("hello", call2.input["text"])
    }

    // ========== OpenAI/OpenRouter Tool Use Tests ==========

    @Test
    fun `chatWithTools sends OpenAI tool use request format correctly`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"choices":[{"message":{"role":"assistant","content":"I'll tap that"},"finish_reason":"stop"}]}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val client = createOpenRouterClient()
        val messages = listOf(
            Message(role = "user", content = "Tap element 5")
        )

        client.chatWithTools(messages, tools = listOf(PhoneTools.TAP))

        val request = server.takeRequest()
        val body = request.body.readUtf8()
        val json = Json { ignoreUnknownKeys = true }
        val requestJson = json.parseToJsonElement(body).jsonObject

        // Verify tools array with function wrapper
        val tools = requestJson["tools"]?.jsonArray
        assertNotNull(tools, "Request should include tools array")
        assertTrue(tools.isNotEmpty(), "Tools array should not be empty")

        // Verify OpenAI tool structure
        val firstTool = tools[0].jsonObject
        assertEquals("function", firstTool["type"]?.jsonPrimitive?.content)
        
        val functionObj = firstTool["function"]?.jsonObject
        assertNotNull(functionObj)
        assertEquals("tap", functionObj["name"]?.jsonPrimitive?.content)
        assertNotNull(functionObj["description"])
        assertNotNull(functionObj["parameters"])

        // Verify parameters structure (OpenAI uses "parameters" not "input_schema")
        val parameters = functionObj["parameters"]?.jsonObject
        assertEquals("object", parameters?.get("type")?.jsonPrimitive?.content)
    }

    @Test
    fun `chatWithTools parses OpenAI tool use response correctly`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{"id":"call_123","type":"function","function":{"name":"tap","arguments":"{\"element_id\":5}"}}]},"finish_reason":"tool_calls"}]}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val client = createOpenRouterClient()
        val messages = listOf(
            Message(role = "user", content = "Tap element 5")
        )

        val result = client.chatWithTools(messages)

        assertTrue(result.isSuccess)
        val response = result.getOrNull()
        assertNotNull(response)

        // Verify tool call parsed from OpenAI format
        assertEquals(1, response.toolCalls.size)
        val toolCall = response.toolCalls[0]
        assertEquals("call_123", toolCall.id)
        assertEquals("tap", toolCall.name)
        assertEquals(5, toolCall.input["element_id"])
        assertEquals("tool_calls", response.stopReason)
    }

    @Test
    fun `chatWithTools parses OpenAI text-only response correctly`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"choices":[{"message":{"role":"assistant","content":"Task complete!"},"finish_reason":"stop"}]}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val client = createOpenRouterClient()
        val messages = listOf(
            Message(role = "user", content = "Are you done?")
        )

        val result = client.chatWithTools(messages)

        assertTrue(result.isSuccess)
        val response = result.getOrNull()
        assertNotNull(response)

        // Verify text response with no tool calls
        assertEquals("Task complete!", response.text)
        assertTrue(response.toolCalls.isEmpty())
        assertEquals("stop", response.stopReason)
    }

    // ========== Tool Result Message Tests ==========

    @Test
    fun `Message toolResult factory creates correct message for Anthropic format`() {
        val toolResult = Message.toolResult("toolu_123", "Element tapped successfully")

        assertEquals("tool", toolResult.role)
        assertEquals("Element tapped successfully", toolResult.content)
        assertEquals("toolu_123", toolResult.toolCallId)
        assertNotNull(toolResult.contentBlocks)

        // Verify content blocks structure for Anthropic
        val block = toolResult.contentBlocks!![0]
        assertEquals("tool_result", block["type"])
        assertEquals("toolu_123", block["tool_use_id"])
        assertEquals("Element tapped successfully", block["content"])
    }
    
    @Test
    fun `Message contentBlocks reconstructs correctly after serialization`() {
        // Create a tool result message
        val original = Message.toolResult("call_abc", "Result content")
        
        // Simulate serialization/deserialization by creating a new Message
        // with only the serialized fields (no _contentBlocks)
        val deserialized = Message(
            role = "tool",
            content = "Result content",
            toolCallId = "call_abc"
            // _contentBlocks is null after deserialization (marked @Transient)
        )
        
        // Verify contentBlocks is reconstructed from toolCallId and content
        assertNotNull(deserialized.contentBlocks, "contentBlocks should be reconstructed")
        val block = deserialized.contentBlocks!![0]
        assertEquals("tool_result", block["type"])
        assertEquals("call_abc", block["tool_use_id"])
        assertEquals("Result content", block["content"])
    }
    
    @Test
    fun `regular messages do not have contentBlocks`() {
        val userMessage = Message(role = "user", content = "Hello")
        val assistantMessage = Message(role = "assistant", content = "Hi there")
        
        assertNull(userMessage.contentBlocks)
        assertNull(assistantMessage.contentBlocks)
    }

    @Test
    fun `chatWithTools sends Anthropic tool result in correct format`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"content":[{"type":"text","text":"Done"}],"role":"assistant","stop_reason":"end_turn"}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val client = createAnthropicClient()
        val testMessages = listOf(
            Message(role = "user", content = "Tap element 5"),
            Message(role = "assistant", content = ""),  // Assistant with tool call (simplified)
            Message.toolResult("toolu_123", "Element tapped")
        )

        client.chatWithTools(testMessages)

        val request = server.takeRequest()
        val body = request.body.readUtf8()
        
        // Verify tool result is sent as user message with content blocks
        assertTrue(body.contains("\"type\":\"tool_result\""))
        assertTrue(body.contains("\"tool_use_id\":\"toolu_123\""))
        assertTrue(body.contains("Element tapped"))
    }

    @Test
    fun `chatWithTools sends OpenAI tool result in correct format`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"choices":[{"message":{"role":"assistant","content":"Done"},"finish_reason":"stop"}]}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val client = createOpenRouterClient()
        val testMessages = listOf(
            Message(role = "user", content = "Tap element 5"),
            Message(role = "assistant", content = ""),  // Assistant with tool call (simplified)
            Message.toolResult("call_123", "Element tapped")
        )

        client.chatWithTools(testMessages)

        val request = server.takeRequest()
        val body = request.body.readUtf8()
        val json = Json { ignoreUnknownKeys = true }
        val requestJson = json.parseToJsonElement(body).jsonObject
        val messagesArray = requestJson["messages"]?.jsonArray
        assertNotNull(messagesArray)

        // Find the tool result message
        val toolMessage = messagesArray.firstOrNull { 
            it.jsonObject["role"]?.jsonPrimitive?.content == "tool" 
        }?.jsonObject
        
        assertNotNull(toolMessage, "Should have tool role message")
        assertEquals("Element tapped", toolMessage["content"]?.jsonPrimitive?.content)
        assertEquals("call_123", toolMessage["tool_call_id"]?.jsonPrimitive?.content)
    }

    // ========== Multi-Turn Tool Use Flow Test ==========

    @Test
    fun `multi-turn tool use flow works correctly`() = runTest {
        // Simulate a complete multi-turn flow:
        // 1. User: "Tap element 5"
        // 2. Assistant: [tool_use: tap(5)]
        // 3. Tool Result: "Success"
        // 4. Assistant: [tool_use: read_screen()]
        // 5. Tool Result: "Screen content..."
        // 6. Assistant: "Done!" (text response = task complete)
        
        // First turn: tap tool call
        server.enqueue(MockResponse()
            .setBody("""{"content":[{"type":"tool_use","id":"t1","name":"tap","input":{"element_id":5}}],"role":"assistant","stop_reason":"tool_use"}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))
        
        // Second turn: read_screen tool call
        server.enqueue(MockResponse()
            .setBody("""{"content":[{"type":"tool_use","id":"t2","name":"read_screen","input":{}}],"role":"assistant","stop_reason":"tool_use"}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))
        
        // Third turn: completion text
        server.enqueue(MockResponse()
            .setBody("""{"content":[{"type":"text","text":"Done! Element 5 was tapped."}],"role":"assistant","stop_reason":"end_turn"}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val client = createAnthropicClient()
        val messages = mutableListOf(
            Message(role = "user", content = "Tap element 5")
        )

        // Turn 1: Initial request, get tap tool call
        val result1 = client.chatWithTools(messages)
        assertTrue(result1.isSuccess)
        val response1 = result1.getOrNull()!!
        assertEquals(1, response1.toolCalls.size)
        assertEquals("tap", response1.toolCalls[0].name)
        assertEquals("tool_use", response1.stopReason)
        
        // Add tool result and continue
        messages.add(Message.toolResult("t1", "Success: Element 5 tapped"))

        // Turn 2: Get read_screen tool call
        val result2 = client.chatWithTools(messages)
        assertTrue(result2.isSuccess)
        val response2 = result2.getOrNull()!!
        assertEquals(1, response2.toolCalls.size)
        assertEquals("read_screen", response2.toolCalls[0].name)
        assertEquals("tool_use", response2.stopReason)
        
        // Add tool result and continue
        messages.add(Message.toolResult("t2", "Screen shows: [1] Button 'Submit' [2] TextField 'Email'"))

        // Turn 3: Get completion text
        val result3 = client.chatWithTools(messages)
        assertTrue(result3.isSuccess)
        val response3 = result3.getOrNull()!!
        assertEquals("Done! Element 5 was tapped.", response3.text)
        assertTrue(response3.toolCalls.isEmpty())
        assertEquals("end_turn", response3.stopReason)
        
        // Verify 3 requests were made
        assertEquals(3, server.requestCount)
    }

    // ========== PhoneTools Definition Tests ==========

    @Test
    fun `PhoneTools ALL contains all expected tools`() {
        val toolNames = PhoneTools.ALL.map { it.name }
        
        val expectedTools = setOf(
            "tap", "tap_text", "type_text", "swipe", "press_back", "press_home",
            "open_app", "open_notifications", "read_screen", "scroll",
            "screenshot",
            "copy", "set_clipboard", "paste",
            "read_notifications", "tap_notification", "dismiss_notification", "reply_notification",
            "read_file", "write_file", "list_files",
            "remember", "recall", "list_memories", "learn",
            "think", "wait", "long_press", "request_tools"
        )
        assertEquals(expectedTools, toolNames.toSet(), "PhoneTools.ALL should contain exactly the expected tools")
    }

    @Test
    fun `PhoneTools think has correct schema`() {
        val think = PhoneTools.THINK
        assertEquals("think", think.name)
        assertTrue(think.description.contains("plan") || think.description.contains("reason"), "Description should mention reasoning/planning")
        
        @Suppress("UNCHECKED_CAST")
        val required = think.inputSchema["required"] as? List<String>
        assertTrue(required?.contains("thought") == true)
    }

    @Test
    fun `PhoneTools wait has correct schema with bounds`() {
        val wait = PhoneTools.WAIT
        assertEquals("wait", wait.name)
        
        @Suppress("UNCHECKED_CAST")
        val properties = wait.inputSchema["properties"] as? Map<String, Any>
        assertNotNull(properties)
        
        @Suppress("UNCHECKED_CAST")
        val secondsProp = properties["seconds"] as? Map<String, Any>
        assertNotNull(secondsProp)
        assertEquals("integer", secondsProp["type"])
    }

    @Test
    fun `PhoneTools long_press has correct schema`() {
        val longPress = PhoneTools.LONG_PRESS
        assertEquals("long_press", longPress.name)
        assertTrue(longPress.description.contains("context menu") || longPress.description.contains("Long-press"))
        
        @Suppress("UNCHECKED_CAST")
        val required = longPress.inputSchema["required"] as? List<String>
        assertTrue(required?.contains("element_id") == true)
    }

    @Test
    fun `PhoneTools tap has correct schema`() {
        val tap = PhoneTools.TAP
        
        assertEquals("tap", tap.name)
        assertTrue(tap.description.contains("element"))
        
        val schema = tap.inputSchema
        assertEquals("object", schema["type"])
        
        @Suppress("UNCHECKED_CAST")
        val properties = schema["properties"] as? Map<String, Any>
        assertNotNull(properties)
        assertTrue(properties.containsKey("element_id"))
        
        @Suppress("UNCHECKED_CAST")
        val required = schema["required"] as? List<String>
        assertNotNull(required)
        assertTrue(required.contains("element_id"))
    }

    @Test
    fun `PhoneTools tap_text has correct schema`() {
        val tapText = PhoneTools.TAP_TEXT
        
        assertEquals("tap_text", tapText.name)
        assertTrue(tapText.description.contains("text"))
        
        @Suppress("UNCHECKED_CAST")
        val properties = tapText.inputSchema["properties"] as? Map<String, Any>
        assertNotNull(properties)
        assertTrue(properties.containsKey("text"))
        
        @Suppress("UNCHECKED_CAST")
        val required = tapText.inputSchema["required"] as? List<String>
        assertTrue(required?.contains("text") == true)
    }

    @Test
    fun `PhoneTools type_text has correct schema and description warns about no submit`() {
        val typeText = PhoneTools.TYPE_TEXT
        
        assertEquals("type_text", typeText.name)
        assertTrue(typeText.description.contains("Does NOT submit"), "Description should warn about no auto-submit")
        
        @Suppress("UNCHECKED_CAST")
        val properties = typeText.inputSchema["properties"] as? Map<String, Any>
        assertTrue(properties?.containsKey("text") == true)
    }

    @Test
    fun `PhoneTools swipe has direction enum`() {
        val swipe = PhoneTools.SWIPE
        
        assertEquals("swipe", swipe.name)
        
        @Suppress("UNCHECKED_CAST")
        val properties = swipe.inputSchema["properties"] as? Map<String, Any>
        assertNotNull(properties)
        
        @Suppress("UNCHECKED_CAST")
        val directionProp = properties["direction"] as? Map<String, Any>
        assertNotNull(directionProp)
        
        @Suppress("UNCHECKED_CAST")
        val enumValues = directionProp["enum"] as? List<String>
        assertNotNull(enumValues)
        assertEquals(4, enumValues.size)
        assertTrue(enumValues.containsAll(listOf("up", "down", "left", "right")))
    }

    @Test
    fun `PhoneTools press_back has no parameters`() {
        val pressBack = PhoneTools.PRESS_BACK
        
        assertEquals("press_back", pressBack.name)
        
        @Suppress("UNCHECKED_CAST")
        val properties = pressBack.inputSchema["properties"] as? Map<String, Any>
        assertTrue(properties?.isEmpty() == true)
        
        @Suppress("UNCHECKED_CAST")
        val required = pressBack.inputSchema["required"] as? List<String>
        assertTrue(required?.isEmpty() == true)
    }

    @Test
    fun `PhoneTools open_app requires app_name parameter`() {
        val openApp = PhoneTools.OPEN_APP
        
        assertEquals("open_app", openApp.name)
        
        @Suppress("UNCHECKED_CAST")
        val properties = openApp.inputSchema["properties"] as? Map<String, Any>
        assertTrue(properties?.containsKey("app_name") == true)
        
        @Suppress("UNCHECKED_CAST")
        val required = openApp.inputSchema["required"] as? List<String>
        assertTrue(required?.contains("app_name") == true)
    }

    @Test
    fun `PhoneTools scroll has up and down enum only`() {
        val scroll = PhoneTools.SCROLL
        
        assertEquals("scroll", scroll.name)
        
        @Suppress("UNCHECKED_CAST")
        val properties = scroll.inputSchema["properties"] as? Map<String, Any>
        assertNotNull(properties)
        
        @Suppress("UNCHECKED_CAST")
        val directionProp = properties["direction"] as? Map<String, Any>
        assertNotNull(directionProp)
        
        @Suppress("UNCHECKED_CAST")
        val enumValues = directionProp["enum"] as? List<String>
        assertNotNull(enumValues)
        assertEquals(2, enumValues.size)
        assertTrue(enumValues.containsAll(listOf("up", "down")))
    }

    @Test
    fun `PhoneTools file tools have expected schemas`() {
        assertEquals("read_file", PhoneTools.READ_FILE.name)
        assertEquals("write_file", PhoneTools.WRITE_FILE.name)
        assertEquals("list_files", PhoneTools.LIST_FILES.name)

        @Suppress("UNCHECKED_CAST")
        val readRequired = PhoneTools.READ_FILE.inputSchema["required"] as? List<String>
        assertTrue(readRequired?.contains("path") == true)

        @Suppress("UNCHECKED_CAST")
        val writeRequired = PhoneTools.WRITE_FILE.inputSchema["required"] as? List<String>
        assertTrue(writeRequired?.containsAll(listOf("path", "content")) == true)

        assertTrue(PhoneTools.WRITE_FILE.description.contains("SECURITY.md is read-only"))
    }

    @Test
    fun `PhoneTools memory tools have expected schemas`() {
        assertEquals("remember", PhoneTools.REMEMBER.name)
        assertEquals("recall", PhoneTools.RECALL.name)
        assertEquals("list_memories", PhoneTools.LIST_MEMORIES.name)

        @Suppress("UNCHECKED_CAST")
        val rememberRequired = PhoneTools.REMEMBER.inputSchema["required"] as? List<String>
        assertTrue(rememberRequired?.contains("content") == true)

        @Suppress("UNCHECKED_CAST")
        val recallRequired = PhoneTools.RECALL.inputSchema["required"] as? List<String>
        assertTrue(recallRequired?.contains("query") == true)

        @Suppress("UNCHECKED_CAST")
        val listRequired = PhoneTools.LIST_MEMORIES.inputSchema["required"] as? List<String>
        assertTrue(listRequired?.isEmpty() == true)
    }
}
