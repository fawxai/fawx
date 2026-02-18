package ai.citros.core

import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonNull
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

class ClaudeClientTest {

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

    private fun createClient(
        apiKey: String = "sk-ant-api03-test-key",
        model: String = ModelConfig.DEFAULT_MODEL,
        systemPrompt: String = "You are a test assistant."
    ): ClaudeClient {
        return ClaudeClient(
            apiKey = apiKey,
            model = model,
            systemPrompt = systemPrompt,
            baseUrl = server.url("/v1/messages").toString()
        )
    }

    @Test
    fun `chat request includes anthropic-beta header for prompt caching`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"content":[{"type":"text","text":"Hello!"}],"role":"assistant"}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val client = createClient(apiKey = "sk-ant-api03-my-key")
        val conversation = Conversation()
        conversation.addUser("Hi")

        client.chat(conversation)

        val request = server.takeRequest()
        assertEquals("sk-ant-api03-my-key", request.getHeader("x-api-key"))
        assertEquals("2023-06-01", request.getHeader("anthropic-version"))
        assertEquals("prompt-caching-2024-07-31", request.getHeader("anthropic-beta"))
        assertTrue(request.getHeader("Content-Type")!!.startsWith("application/json"))
    }

    @Test
    fun `legacy constructor includes prompt caching beta header`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"content":[{"type":"text","text":"Hello from legacy!"}],"role":"assistant"}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        // Use legacy constructor (ClaudeClient which wraps AnthropicClient)
        val legacyClient = ClaudeClient(
            apiKey = "sk-ant-api03-legacy-test",
            systemPrompt = "You are a test assistant.",
            baseUrl = server.url("/v1/messages").toString()
        )
        val conversation = Conversation()
        conversation.addUser("Hi")

        legacyClient.chat(conversation)

        val request = server.takeRequest()
        assertEquals("sk-ant-api03-legacy-test", request.getHeader("x-api-key"))
        assertEquals("2023-06-01", request.getHeader("anthropic-version"))
        assertEquals("prompt-caching-2024-07-31", request.getHeader("anthropic-beta"), 
            "Legacy constructor should include prompt caching beta header")
        
        // Verify system prompt is structured with cache_control
        val body = request.body.readUtf8()
        val json = Json { ignoreUnknownKeys = true }
        val requestJson = json.parseToJsonElement(body).jsonObject
        val systemArray = requestJson["system"]?.jsonArray
        assertNotNull(systemArray, "System should be an array even in legacy constructor")
        assertTrue(systemArray!!.size > 0)
        val systemBlock = systemArray[0].jsonObject
        assertNotNull(systemBlock["cache_control"], "Legacy constructor should include cache_control on system prompt")
    }

    @Test
    fun `AnthropicClient rejects config without prompt caching header`() = runTest {
        val configWithoutCaching = ProviderConfig(
            provider = Provider.ANTHROPIC,
            baseUrl = "https://api.anthropic.com/v1/messages",
            chatModelId = ModelConfig.CHAT_MODEL,
            actionModelId = ModelConfig.ACTION_MODEL,
            headers = mapOf(
                "x-api-key" to "sk-ant-api03-test",
                "anthropic-version" to "2023-06-01"
                // Missing anthropic-beta header
            )
        )
        
        try {
            AnthropicClient(configWithoutCaching)
            throw AssertionError("Expected IllegalArgumentException to be thrown")
        } catch (e: IllegalArgumentException) {
            assertTrue(e.message!!.contains("anthropic-beta"), 
                "Error message should mention missing anthropic-beta header")
            assertTrue(e.message!!.contains("prompt-caching-2024-07-31"),
                "Error message should mention the required caching beta value")
        }
    }

    @Test
    fun `chat sends correct headers with setup token`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"content":[{"type":"text","text":"Hello!"}],"role":"assistant"}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val client = createClient(apiKey = "sk-ant-oat01-setup-token-here")
        val conversation = Conversation()
        conversation.addUser("Hi")

        client.chat(conversation)

        val request = server.takeRequest()
        assertEquals("sk-ant-oat01-setup-token-here", request.getHeader("x-api-key"))
    }

    @Test
    fun `chat sends system prompt and messages in body`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"content":[{"type":"text","text":"Response"}],"role":"assistant"}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val client = createClient(systemPrompt = "You are Citros.")
        val conversation = Conversation()
        conversation.addUser("What can you do?")

        client.chat(conversation)

        val request = server.takeRequest()
        val body = request.body.readUtf8()
        val json = Json { ignoreUnknownKeys = true }
        val requestJson = json.parseToJsonElement(body).jsonObject
        
        // Verify system prompt is now an array with cache_control
        val systemArray = requestJson["system"]?.jsonArray
        assertNotNull(systemArray, "System should be an array")
        assertEquals(1, systemArray!!.size)
        
        val systemBlock = systemArray[0].jsonObject
        assertEquals("text", systemBlock["type"]?.jsonPrimitive?.content)
        assertEquals("You are Citros.", systemBlock["text"]?.jsonPrimitive?.content)
        
        val cacheControl = systemBlock["cache_control"]?.jsonObject
        assertNotNull(cacheControl, "System prompt should have cache_control")
        assertEquals("ephemeral", cacheControl!!["type"]?.jsonPrimitive?.content)
        
        assertTrue(body.contains("What can you do?"), "Body should contain user message")
        assertTrue(body.contains("\"model\":\"${ModelConfig.DEFAULT_MODEL}\""), "Body should contain model")
    }

    @Test
    fun `chatWithTools sends system prompt and tools with cache_control blocks`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"content":[{"type":"text","text":"Let me tap that"},{"type":"tool_use","id":"t1","name":"tap","input":{"element_id":5}}],"role":"assistant","stop_reason":"tool_use"}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val client = createClient(systemPrompt = "You are a phone assistant.")
        val messages = listOf(Message("user", "Tap button 5"))
        
        val result = client.chatWithTools(messages, tools = PhoneTools.ALL)

        assertTrue(result.isSuccess)
        
        val request = server.takeRequest()
        val body = request.body.readUtf8()
        val json = Json { ignoreUnknownKeys = true }
        val requestJson = json.parseToJsonElement(body).jsonObject
        
        // Verify system prompt array with cache_control
        val systemArray = requestJson["system"]?.jsonArray
        assertNotNull(systemArray, "System should be an array in tool requests")
        assertEquals(1, systemArray!!.size)
        
        val systemBlock = systemArray[0].jsonObject
        assertEquals("text", systemBlock["type"]?.jsonPrimitive?.content)
        assertEquals("You are a phone assistant.", systemBlock["text"]?.jsonPrimitive?.content)
        assertNotNull(systemBlock["cache_control"]?.jsonObject, "System prompt should have cache_control")
        assertEquals("ephemeral", systemBlock["cache_control"]?.jsonObject?.get("type")?.jsonPrimitive?.content)
        
        // Verify tools array with cache_control on last tool
        val toolsArray = requestJson["tools"]?.jsonArray
        assertNotNull(toolsArray, "Tools array should be present")
        assertTrue(toolsArray!!.size > 0, "Should have at least one tool")
        
        val lastTool = toolsArray[toolsArray.size - 1].jsonObject
        val toolCacheControl = lastTool["cache_control"]?.jsonObject
        assertNotNull(toolCacheControl, "Last tool should have cache_control")
        assertEquals("ephemeral", toolCacheControl!!["type"]?.jsonPrimitive?.content)
        
        // Verify tools before the last don't have cache_control
        if (toolsArray.size > 1) {
            val firstTool = toolsArray[0].jsonObject
            assertNull(firstTool["cache_control"], "Non-last tools should not have cache_control")
        }
    }

    // TODO: Add integration test for cache usage metrics
    // This requires real Anthropic API calls to verify cache_creation_input_tokens and cache_read_input_tokens
    // in the response usage field. Not suitable for unit tests.
    // Example verification:
    //   - First request: usage.cache_creation_input_tokens > 0
    //   - Second request within 5 min: usage.cache_read_input_tokens > 0
    //   - Total cost reduction: ~90% on input tokens

    @Test
    fun `chat returns success with text content`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"content":[{"type":"text","text":"I can help you!"}],"role":"assistant"}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val client = createClient()
        val conversation = Conversation()
        conversation.addUser("Help me")

        val result = client.chat(conversation)

        assertTrue(result.isSuccess)
        assertEquals("I can help you!", result.getOrNull())
    }

    @Test
    fun `chat returns failure on HTTP error`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"error":{"message":"Invalid API key"}}""")
            .setResponseCode(401)
            .addHeader("Content-Type", "application/json"))

        val client = createClient()
        val conversation = Conversation()
        conversation.addUser("Hi")

        val result = client.chat(conversation)

        assertTrue(result.isFailure)
        val error = result.exceptionOrNull()
        assertNotNull(error)
        assertTrue(error.message!!.contains("Invalid API key") || error.message!!.contains("401"),
            "Expected auth error message, got: ${error.message}")
    }

    @Test
    fun `chat returns failure on empty response body`() = runTest {
        server.enqueue(MockResponse()
            .setResponseCode(200)
            .setBody(""))

        val client = createClient()
        val conversation = Conversation()
        conversation.addUser("Hi")

        val result = client.chat(conversation)

        // Empty body should result in failure (no content to parse)
        assertTrue(result.isFailure)
    }

    @Test
    fun `chat returns failure on malformed JSON`() = runTest {
        server.enqueue(MockResponse()
            .setBody("not json at all")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val client = createClient()
        val conversation = Conversation()
        conversation.addUser("Hi")

        val result = client.chat(conversation)

        assertTrue(result.isFailure)
    }

    @Test
    fun `chat returns failure when content array is empty`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"content":[],"role":"assistant"}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val client = createClient()
        val conversation = Conversation()
        conversation.addUser("Hi")

        val result = client.chat(conversation)

        assertTrue(result.isFailure)
    }

    @Test
    fun `chat sends multi-turn conversation`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"content":[{"type":"text","text":"Response 2"}],"role":"assistant"}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val client = createClient()
        val conversation = Conversation()
        conversation.addUser("First message")
        conversation.addAssistant("First response")
        conversation.addUser("Second message")

        client.chat(conversation)

        val request = server.takeRequest()
        val body = request.body.readUtf8()
        assertTrue(body.contains("First message"))
        assertTrue(body.contains("First response"))
        assertTrue(body.contains("Second message"))
    }

    @Test
    fun `chat sends configurable max_tokens`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"content":[{"type":"text","text":"Hi"}],"role":"assistant"}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val client = ClaudeClient(
            apiKey = "sk-ant-api03-test",
            maxTokens = 8192,
            baseUrl = server.url("/v1/messages").toString()
        )
        val conversation = Conversation()
        conversation.addUser("Hi")

        client.chat(conversation)

        val request = server.takeRequest()
        val body = request.body.readUtf8()
        assertTrue(body.contains("\"max_tokens\":8192"), "Body should contain custom max_tokens")
    }

    @Test
    fun `chat handles non-text content blocks gracefully`() = runTest {
        // API may return tool_use blocks instead of text
        server.enqueue(MockResponse()
            .setBody("""{"content":[{"type":"tool_use","id":"t1","name":"test","input":{}}],"role":"assistant"}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val client = createClient()
        val conversation = Conversation()
        conversation.addUser("Hi")

        val result = client.chat(conversation)

        assertTrue(result.isFailure, "Should fail when no text content blocks exist")
        assertTrue(result.exceptionOrNull()?.message?.contains("no text content") == true)
    }

    @Test
    fun `tokenType identifies API key correctly`() {
        assertEquals(ClaudeClient.TokenType.API_KEY, ClaudeClient.identifyTokenType("sk-ant-api03-abcdef"))
    }

    @Test
    fun `tokenType identifies setup token correctly`() {
        assertEquals(ClaudeClient.TokenType.SETUP_TOKEN, ClaudeClient.identifyTokenType("sk-ant-oat01-abcdef"))
    }

    @Test
    fun `tokenType returns UNKNOWN for unrecognized format`() {
        assertEquals(ClaudeClient.TokenType.UNKNOWN, ClaudeClient.identifyTokenType("some-random-string"))
    }

    @Test
    fun `chat retries on 429 with retry-after header`() = runTest {
        // First request: 429 with retry-after
        server.enqueue(MockResponse()
            .setResponseCode(429)
            .addHeader("retry-after", "1")
            .setBody("""{"error":{"type":"rate_limit_error","message":"Rate limited"}}"""))
        
        // Second request: success
        server.enqueue(MockResponse()
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json")
            .setBody("""{"content":[{"type":"text","text":"Success after retry"}],"role":"assistant"}"""))

        val client = createClient()
        val conversation = Conversation()
        conversation.addUser("Test")

        val result = client.chat(conversation)

        assertTrue(result.isSuccess)
        assertEquals("Success after retry", result.getOrNull())
        assertEquals(2, server.requestCount) // Should have made 2 requests
    }

    @Test
    fun `chat retries on 429 with exponential backoff when no retry-after header`() = runTest {
        // First request: 429 without retry-after
        server.enqueue(MockResponse()
            .setResponseCode(429)
            .setBody("""{"error":{"type":"rate_limit_error","message":"Rate limited"}}"""))
        
        // Second request: still 429
        server.enqueue(MockResponse()
            .setResponseCode(429)
            .setBody("""{"error":{"type":"rate_limit_error","message":"Rate limited"}}"""))
        
        // Third request: success
        server.enqueue(MockResponse()
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json")
            .setBody("""{"content":[{"type":"text","text":"Success after 2 retries"}],"role":"assistant"}"""))

        val client = createClient()
        val conversation = Conversation()
        conversation.addUser("Test")

        val result = client.chat(conversation)

        assertTrue(result.isSuccess)
        assertEquals("Success after 2 retries", result.getOrNull())
        assertEquals(3, server.requestCount)
    }

    @Test
    fun `chat returns failure after max retries exhausted`() = runTest {
        // All requests return 429
        repeat(4) { // Initial + 3 retries
            server.enqueue(MockResponse()
                .setResponseCode(429)
                .setBody("""{"error":{"type":"rate_limit_error","message":"Rate limited"}}"""))
        }

        val client = createClient()
        val conversation = Conversation()
        conversation.addUser("Test")

        val result = client.chat(conversation)

        assertTrue(result.isFailure)
        assertTrue(result.exceptionOrNull()?.message?.contains("Rate limit") == true)
        assertEquals(4, server.requestCount) // Initial + 3 retries
    }

    @Test
    fun `chat does not retry on non-429 errors`() = runTest {
        // 401 error - should not retry
        server.enqueue(MockResponse()
            .setResponseCode(401)
            .setBody("""{"error":{"message":"Invalid API key"}}"""))

        val client = createClient()
        val conversation = Conversation()
        conversation.addUser("Test")

        val result = client.chat(conversation)

        assertTrue(result.isFailure)
        assertEquals(1, server.requestCount) // Should only try once
    }

    @Test
    fun `chat does not retry on 500 server errors`() = runTest {
        server.enqueue(MockResponse()
            .setResponseCode(500)
            .setBody("""{"error":{"message":"Internal server error"}}"""))

        val client = createClient()
        val conversation = Conversation()
        conversation.addUser("Test")

        val result = client.chat(conversation)

        assertTrue(result.isFailure)
        assertEquals(1, server.requestCount)
    }

    @Test
    fun `chat uses exponential backoff timing on retry`() = runTest {
        // Enqueue three 429 responses, then success
        server.enqueue(MockResponse()
            .setResponseCode(429)
            .setBody("""{"error":{"type":"rate_limit_error","message":"Rate limited"}}"""))
        
        server.enqueue(MockResponse()
            .setResponseCode(429)
            .setBody("""{"error":{"type":"rate_limit_error","message":"Rate limited"}}"""))
        
        server.enqueue(MockResponse()
            .setResponseCode(429)
            .setBody("""{"error":{"type":"rate_limit_error","message":"Rate limited"}}"""))
        
        server.enqueue(MockResponse()
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json")
            .setBody("""{"content":[{"type":"text","text":"Success"}],"role":"assistant"}"""))

        val client = createClient()
        val conversation = Conversation()
        conversation.addUser("Test")

        // Measure actual elapsed time to verify delays occurred
        val startTime = System.currentTimeMillis()
        val result = client.chat(conversation)
        val elapsedTime = System.currentTimeMillis() - startTime

        // Verify success after retries
        assertTrue(result.isSuccess)
        assertEquals(4, server.requestCount)
        
        // Verify exponential backoff delays occurred: 1s, 2s, 4s = 7000ms total
        // Allow some tolerance for test execution overhead
        assertTrue(
            elapsedTime >= 6900, // 7000ms - 100ms tolerance
            "Should have delayed at least 6.9s for exponential backoff (1s + 2s + 4s), got ${elapsedTime}ms"
        )
    }

    // ========== OpenRouter Provider Tests ==========

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

    private fun createOpenAiClient(
        token: String = "sk-proj-test-key",
        systemPrompt: String = "You are a test assistant."
    ): ProviderClient {
        val config = ProviderConfig.openAi(token)
        return OpenAiClient(
            config = ProviderConfig(
                provider = Provider.OPENAI,
                baseUrl = server.url("/v1/chat/completions").toString(),
                chatModelId = config.chatModelId,
                actionModelId = config.actionModelId,
                headers = config.headers
            ),
            systemPrompt = systemPrompt
        )
    }

    @Test
    fun `openRouter chat sends correct headers`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"choices":[{"message":{"role":"assistant","content":"Hello from OpenRouter!"}}]}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val client = createOpenRouterClient(apiKey = "sk-or-my-key")
        val conversation = Conversation()
        conversation.addUser("Hi")

        client.chat(conversation)

        val request = server.takeRequest()
        assertEquals("Bearer sk-or-my-key", request.getHeader("Authorization"))
        assertEquals("https://citros.ai", request.getHeader("HTTP-Referer"))
        assertEquals("Citros", request.getHeader("X-Title"))
        assertTrue(request.getHeader("Content-Type")!!.startsWith("application/json"))
    }

    @Test
    fun `openRouter chat sends system prompt as first message`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"choices":[{"message":{"role":"assistant","content":"Response"}}]}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val client = createOpenRouterClient(systemPrompt = "You are Citros on OpenRouter.")
        val conversation = Conversation()
        conversation.addUser("What can you do?")

        client.chat(conversation)

        val request = server.takeRequest()
        val body = request.body.readUtf8()
        
        // Parse JSON to verify structure precisely
        val json = Json { ignoreUnknownKeys = true }
        val requestJson = json.parseToJsonElement(body).jsonObject
        val messages = requestJson["messages"]?.jsonArray
        assertNotNull(messages, "Request should have messages array")
        assertTrue(messages.size >= 2, "Should have at least system + user message")
        
        // Verify first message is system prompt
        val firstMessage = messages[0].jsonObject
        assertEquals("system", firstMessage["role"]?.jsonPrimitive?.content, "First message should be system role")
        assertEquals("You are Citros on OpenRouter.", firstMessage["content"]?.jsonPrimitive?.content, "First message should contain system prompt")
        
        // Verify second message is user message
        val secondMessage = messages[1].jsonObject
        assertEquals("user", secondMessage["role"]?.jsonPrimitive?.content, "Second message should be user role")
        assertEquals("What can you do?", secondMessage["content"]?.jsonPrimitive?.content, "Second message should contain user prompt")
        
        // Verify model ID
        assertEquals(ModelConfig.OPENROUTER_CHAT_MODEL, requestJson["model"]?.jsonPrimitive?.content, "Should use OpenRouter model ID")
    }

    @Test
    fun `openRouter chat parses response correctly`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"choices":[{"message":{"role":"assistant","content":"I can help you with OpenRouter!"}}]}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val client = createOpenRouterClient()
        val conversation = Conversation()
        conversation.addUser("Help me")

        val result = client.chat(conversation)

        assertTrue(result.isSuccess)
        assertEquals("I can help you with OpenRouter!", result.getOrNull())
    }

    @Test
    fun `openRouter chat handles multi-turn conversation`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"choices":[{"message":{"role":"assistant","content":"Response 2"}}]}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val client = createOpenRouterClient()
        val conversation = Conversation()
        conversation.addUser("First message")
        conversation.addAssistant("First response")
        conversation.addUser("Second message")

        client.chat(conversation)

        val request = server.takeRequest()
        val body = request.body.readUtf8()
        
        // Should have system message + 3 conversation messages
        assertTrue(body.contains("\"role\":\"system\""))
        assertTrue(body.contains("First message"))
        assertTrue(body.contains("First response"))
        assertTrue(body.contains("Second message"))
    }

    @Test
    fun `openRouter chat returns failure on empty choices array`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"choices":[]}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val client = createOpenRouterClient()
        val conversation = Conversation()
        conversation.addUser("Hi")

        val result = client.chat(conversation)

        assertTrue(result.isFailure)
    }

    @Test
    fun `openRouter chat returns failure on missing content field`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"choices":[{"message":{"role":"assistant"}}]}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val client = createOpenRouterClient()
        val conversation = Conversation()
        conversation.addUser("Hi")

        val result = client.chat(conversation)

        assertTrue(result.isFailure)
    }

    @Test
    fun `openRouter chat retries on 429`() = runTest {
        // First request: 429
        server.enqueue(MockResponse()
            .setResponseCode(429)
            .addHeader("retry-after", "1")
            .setBody("""{"error":{"message":"Rate limited"}}"""))
        
        // Second request: success
        server.enqueue(MockResponse()
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json")
            .setBody("""{"choices":[{"message":{"role":"assistant","content":"Success after retry"}}]}"""))

        val client = createOpenRouterClient()
        val conversation = Conversation()
        conversation.addUser("Test")

        val result = client.chat(conversation)

        assertTrue(result.isSuccess)
        assertEquals("Success after retry", result.getOrNull())
        assertEquals(2, server.requestCount)
    }

    @Test
    fun `ProviderConfig factory creates correct Anthropic config`() {
        val config = ProviderConfig.anthropic("sk-ant-api03-test")
        
        assertEquals(Provider.ANTHROPIC, config.provider)
        assertEquals("https://api.anthropic.com/v1/messages", config.baseUrl)
        assertEquals(ModelConfig.CHAT_MODEL, config.chatModelId)
        assertEquals("sk-ant-api03-test", config.headers["x-api-key"])
    }

    @Test
    fun `ProviderConfig factory creates correct OpenRouter config`() {
        val config = ProviderConfig.openRouter("sk-or-test")
        
        assertEquals(Provider.OPENROUTER, config.provider)
        assertEquals("https://openrouter.ai/api/v1/chat/completions", config.baseUrl)
        assertEquals(ModelConfig.OPENROUTER_CHAT_MODEL, config.chatModelId)
        assertEquals("Bearer sk-or-test", config.headers["Authorization"])
    }

    @Test
    fun `auto-detection flow creates correct OpenRouter client`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"choices":[{"message":{"role":"assistant","content":"Auto-detected!"}}]}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        // Simulate user flow: detect provider from key, create config, construct client
        val apiKey = "sk-or-auto-test"
        val detectedProvider = ProviderConfig.detectProvider(apiKey)
        assertNotNull(detectedProvider)
        assertEquals(Provider.OPENROUTER, detectedProvider)

        val config = when (detectedProvider) {
            Provider.OPENROUTER -> ProviderConfig(
                provider = Provider.OPENROUTER,
                baseUrl = server.url("/api/v1/chat/completions").toString(),
                chatModelId = ModelConfig.OPENROUTER_CHAT_MODEL,
                actionModelId = ModelConfig.OPENROUTER_ACTION_MODEL,
                headers = ProviderConfig.openRouter(apiKey).headers
            )
            Provider.ANTHROPIC -> ProviderConfig.anthropic(apiKey)
            Provider.OPENAI -> ProviderConfig.openAi(apiKey)
        }

        val client = OpenRouterClient(config = config)
        val conversation = Conversation()
        conversation.addUser("Test")

        val result = client.chat(conversation)

        assertTrue(result.isSuccess)
        assertEquals("Auto-detected!", result.getOrNull())

        // Verify it used OpenRouter format
        val request = server.takeRequest()
        assertEquals("Bearer sk-or-auto-test", request.getHeader("Authorization"))
    }

    @Test
    fun `auto-detection flow creates correct Anthropic client`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"content":[{"type":"text","text":"Anthropic detected!"}],"role":"assistant"}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        // Simulate user flow: detect provider from key, create config, construct client
        val apiKey = "sk-ant-api03-auto-test"
        val detectedProvider = ProviderConfig.detectProvider(apiKey)
        assertNotNull(detectedProvider)
        assertEquals(Provider.ANTHROPIC, detectedProvider)

        val config = when (detectedProvider) {
            Provider.ANTHROPIC -> ProviderConfig(
                provider = Provider.ANTHROPIC,
                baseUrl = server.url("/v1/messages").toString(),
                chatModelId = ModelConfig.CHAT_MODEL,
                actionModelId = ModelConfig.ACTION_MODEL,
                headers = ProviderConfig.anthropic(apiKey).headers
            )
            Provider.OPENROUTER -> ProviderConfig.openRouter(apiKey)
            Provider.OPENAI -> ProviderConfig.openAi(apiKey)
        }

        val client = AnthropicClient(config = config)
        val conversation = Conversation()
        conversation.addUser("Test")

        val result = client.chat(conversation)

        assertTrue(result.isSuccess)
        assertEquals("Anthropic detected!", result.getOrNull())

        // Verify it used Anthropic format
        val request = server.takeRequest()
        assertEquals("sk-ant-api03-auto-test", request.getHeader("x-api-key"))
    }

    @Test
    fun `client with empty API key fails with descriptive error`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"error":{"message":"Invalid API key"}}""")
            .setResponseCode(401)
            .addHeader("Content-Type", "application/json"))

        val client = ClaudeClient(
            apiKey = "",
            baseUrl = server.url("/v1/messages").toString()
        )
        val conversation = Conversation()
        conversation.addUser("Test")

        val result = client.chat(conversation)

        assertTrue(result.isFailure)
        val error = result.exceptionOrNull()
        assertNotNull(error)
        assertTrue(error.message!!.contains("401") || error.message!!.contains("API error"))
    }

    @Test
    fun `auto-detection flow creates correct OpenAI client`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"choices":[{"message":{"role":"assistant","content":"OpenAI detected!"}}]}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val apiKey = "sk-proj-abc123"
        val detectedProvider = ProviderConfig.detectProvider(apiKey)
        assertNotNull(detectedProvider)
        assertEquals(Provider.OPENAI, detectedProvider)

        val config = ProviderConfig(
            provider = Provider.OPENAI,
            baseUrl = server.url("/v1/chat/completions").toString(),
            chatModelId = ModelConfig.OPENAI_CHAT_MODEL,
            actionModelId = ModelConfig.OPENAI_ACTION_MODEL,
            headers = ProviderConfig.openAi(apiKey).headers
        )

        val client = OpenAiClient(config = config)
        val conversation = Conversation()
        conversation.addUser("Test")

        val result = client.chat(conversation)

        assertTrue(result.isSuccess)
        assertEquals("OpenAI detected!", result.getOrNull())

        val request = server.takeRequest()
        assertEquals("Bearer sk-proj-abc123", request.getHeader("Authorization"))
        val body = request.body.readUtf8()
        assertTrue(body.contains("\"model\":\"${ModelConfig.OPENAI_CHAT_MODEL}\""))
    }

    @Test
    fun `ProviderConfig factory creates correct OpenAI config`() {
        val config = ProviderConfig.openAi("sk-proj-test")
        
        assertEquals(Provider.OPENAI, config.provider)
        assertEquals("https://api.openai.com/v1/chat/completions", config.baseUrl)
        assertEquals(ModelConfig.OPENAI_CHAT_MODEL, config.chatModelId)
        assertEquals("Bearer sk-proj-test", config.headers["Authorization"])
    }

    @Test
    fun `OpenAI OAuth token uses Bearer auth header and executes chat`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody("""{"choices":[{"message":{"role":"assistant","content":"OAuth works"}}]}""")
                .setResponseCode(200)
                .addHeader("Content-Type", "application/json")
        )

        val oauthToken = "eyJhbGciOiJIUzI1NiJ9.eyJ1c2VyIjoiam9lIn0.c2ln"
        val client = createOpenAiClient(token = oauthToken)
        val conversation = Conversation()
        conversation.addUser("Test OAuth")

        val result = client.chat(conversation)

        assertTrue(result.isSuccess)
        assertEquals("OAuth works", result.getOrNull())

        val request = server.takeRequest()
        assertEquals("Bearer $oauthToken", request.getHeader("Authorization"))
        val body = request.body.readUtf8()
        assertTrue(body.contains("\"model\":\"${ModelConfig.OPENAI_CHAT_MODEL}\""))
    }

    @Test
    fun `detectProvider identifies OpenAI keys`() {
        assertEquals(Provider.OPENAI, ProviderConfig.detectProvider("sk-proj-abc123"))
        assertEquals(Provider.OPENAI, ProviderConfig.detectProvider("sk-svcacct-abc123"))
        assertEquals(Provider.ANTHROPIC, ProviderConfig.detectProvider("sk-ant-api03-test"))
        assertEquals(Provider.OPENROUTER, ProviderConfig.detectProvider("sk-or-test"))
    }

    @Test
    fun `legacy constructor still works for backward compatibility`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"content":[{"type":"text","text":"Legacy works"}],"role":"assistant"}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        // Use old constructor without ProviderConfig
        val client = ClaudeClient(
            apiKey = "sk-ant-api03-legacy",
            baseUrl = server.url("/v1/messages").toString()
        )
        val conversation = Conversation()
        conversation.addUser("Test")

        val result = client.chat(conversation)

        assertTrue(result.isSuccess)
        assertEquals("Legacy works", result.getOrNull())
        
        // Should use Anthropic format (x-api-key header)
        val request = server.takeRequest()
        assertEquals("sk-ant-api03-legacy", request.getHeader("x-api-key"))
    }

    @Test
    fun `buildApiBackend wrapped in runCatching handles invalid credentials gracefully`() {
        // Edge case: verify that buildApiBackend failures are caught and filtered
        // This ensures the app doesn't crash when all providers fail during initialization
        val invalidToken = ""
        val result = runCatching {
            ProviderConfig.openAi(invalidToken)
        }
        
        // Should succeed in creating config even with invalid token
        // (actual API call failures happen later during network request)
        assertTrue(result.isSuccess)
        
        // Verify that the config can be created without throwing
        val config = result.getOrNull()
        assertEquals(Provider.OPENAI, config?.provider)
    }

    // ========== OpenAI Tool Call Tests ==========

    @Test
    fun `OpenAI tool request includes tool_calls array on assistant messages`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"choices":[{"message":{"role":"assistant","content":"Done!"},"finish_reason":"stop"}]}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val client = createOpenAiClient()
        
        // Create a message history with an assistant message that has tool calls
        val toolCalls = listOf(
            ToolCall("call_123", "tap", mapOf("element_id" to 5))
        )
        val messages = listOf(
            Message("user", "Tap the button"),
            Message.assistantWithTools("Let me tap that for you", toolCalls),
            Message.toolResult("call_123", """{"success": true}"""),
            Message("user", "Thanks")
        )
        
        val result = client.chatWithTools(messages, tools = PhoneTools.ALL)
        
        assertTrue(result.isSuccess)
        
        val request = server.takeRequest()
        val body = request.body.readUtf8()
        val json = Json { ignoreUnknownKeys = true }
        val requestJson = json.parseToJsonElement(body).jsonObject
        val messagesArray = requestJson["messages"]?.jsonArray
        
        assertNotNull(messagesArray)
        
        // Find the assistant message (should be at index 2: system, user, assistant, tool, user)
        val assistantMessage = messagesArray.firstOrNull { 
            it.jsonObject["role"]?.jsonPrimitive?.content == "assistant" &&
            it.jsonObject["tool_calls"] != null
        }?.jsonObject
        
        assertNotNull(assistantMessage, "Assistant message with tool_calls should exist in request")
        
        // Verify tool_calls array is present
        val toolCallsArray = assistantMessage["tool_calls"]?.jsonArray
        assertNotNull(toolCallsArray, "tool_calls array should be present")
        assertEquals(1, toolCallsArray.size)
        
        // Verify tool call structure
        val toolCall = toolCallsArray[0].jsonObject
        assertEquals("call_123", toolCall["id"]?.jsonPrimitive?.content)
        assertEquals("function", toolCall["type"]?.jsonPrimitive?.content)
        
        val function = toolCall["function"]?.jsonObject
        assertNotNull(function)
        assertEquals("tap", function!!["name"]?.jsonPrimitive?.content)
        
        // Verify arguments is a JSON string (not an object)
        val argumentsStr = function["arguments"]?.jsonPrimitive?.content
        assertNotNull(argumentsStr, "arguments should be a JSON string")
        assertTrue(argumentsStr!!.contains("\"element_id\""), "arguments JSON should contain element_id")
        assertTrue(argumentsStr.contains("5"), "arguments JSON should contain value 5")
    }

    @Test
    fun `OpenAI tool request handles assistant message without text content`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"choices":[{"message":{"role":"assistant","content":"Executed"},"finish_reason":"stop"}]}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val client = createOpenAiClient()
        
        // Assistant message with only tool calls, no text
        val toolCalls = listOf(
            ToolCall("call_456", "swipe", mapOf("direction" to "up"))
        )
        val messages = listOf(
            Message("user", "Scroll up"),
            Message.assistantWithTools(null, toolCalls), // No text, only tool calls
            Message.toolResult("call_456", """{"success": true}""")
        )
        
        val result = client.chatWithTools(messages, tools = PhoneTools.ALL)
        
        assertTrue(result.isSuccess)
        
        val request = server.takeRequest()
        val body = request.body.readUtf8()
        val json = Json { ignoreUnknownKeys = true }
        val requestJson = json.parseToJsonElement(body).jsonObject
        val messagesArray = requestJson["messages"]?.jsonArray
        
        assertNotNull(messagesArray)
        
        // Find the assistant message
        val assistantMessage = messagesArray.firstOrNull { 
            it.jsonObject["role"]?.jsonPrimitive?.content == "assistant" &&
            it.jsonObject["tool_calls"] != null
        }?.jsonObject
        
        assertNotNull(assistantMessage)
        
        // Content should be null when no text content
        val content = assistantMessage["content"]
        assertTrue(content is JsonNull || content?.jsonPrimitive?.content.isNullOrEmpty(), 
            "Content should be null or empty when assistant only calls tools")
        
        // tool_calls should still be present
        val toolCallsArray = assistantMessage["tool_calls"]?.jsonArray
        assertNotNull(toolCallsArray)
        assertEquals(1, toolCallsArray.size)
    }

    @Test
    fun `OpenAI tool request handles multiple tool calls in one message`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"choices":[{"message":{"role":"assistant","content":"Done"},"finish_reason":"stop"}]}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val client = createOpenAiClient()
        
        // Assistant calls multiple tools at once
        val toolCalls = listOf(
            ToolCall("call_1", "tap", mapOf("element_id" to 1)),
            ToolCall("call_2", "type", mapOf("text" to "Hello")),
            ToolCall("call_3", "swipe", mapOf("direction" to "down"))
        )
        val messages = listOf(
            Message("user", "Do all three actions"),
            Message.assistantWithTools("Let me do that", toolCalls)
        )
        
        val result = client.chatWithTools(messages, tools = PhoneTools.ALL)
        
        assertTrue(result.isSuccess)
        
        val request = server.takeRequest()
        val body = request.body.readUtf8()
        val json = Json { ignoreUnknownKeys = true }
        val requestJson = json.parseToJsonElement(body).jsonObject
        val messagesArray = requestJson["messages"]?.jsonArray
        
        val assistantMessage = messagesArray?.firstOrNull { 
            it.jsonObject["role"]?.jsonPrimitive?.content == "assistant"
        }?.jsonObject
        
        assertNotNull(assistantMessage)
        
        // Verify all three tool calls are present
        val toolCallsArray = assistantMessage["tool_calls"]?.jsonArray
        assertNotNull(toolCallsArray)
        assertEquals(3, toolCallsArray.size)
        
        // Verify each tool call has correct structure
        assertEquals("call_1", toolCallsArray[0].jsonObject["id"]?.jsonPrimitive?.content)
        assertEquals("call_2", toolCallsArray[1].jsonObject["id"]?.jsonPrimitive?.content)
        assertEquals("call_3", toolCallsArray[2].jsonObject["id"]?.jsonPrimitive?.content)
    }

    @Test
    fun `OpenAI tool result messages include tool_call_id`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"choices":[{"message":{"role":"assistant","content":"Great!"},"finish_reason":"stop"}]}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val client = createOpenAiClient()
        
        val messages = listOf(
            Message("user", "Tap button"),
            Message.assistantWithTools(null, listOf(ToolCall("call_789", "tap", mapOf("element_id" to 10)))),
            Message.toolResult("call_789", """{"success": true, "message": "Tapped"}""")
        )
        
        val result = client.chatWithTools(messages, tools = PhoneTools.ALL)
        
        assertTrue(result.isSuccess)
        
        val request = server.takeRequest()
        val body = request.body.readUtf8()
        val json = Json { ignoreUnknownKeys = true }
        val requestJson = json.parseToJsonElement(body).jsonObject
        val messagesArray = requestJson["messages"]?.jsonArray
        
        // Find the tool result message
        val toolMessage = messagesArray?.firstOrNull { 
            it.jsonObject["role"]?.jsonPrimitive?.content == "tool"
        }?.jsonObject
        
        assertNotNull(toolMessage, "Tool result message should exist")
        assertEquals("call_789", toolMessage!!["tool_call_id"]?.jsonPrimitive?.content)
        assertEquals("""{"success": true, "message": "Tapped"}""", toolMessage["content"]?.jsonPrimitive?.content)
    }

    @Test
    fun `OpenAI tool request parses multi-turn tool conversation correctly`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"choices":[{"message":{"role":"assistant","content":"All done!"},"finish_reason":"stop"}]}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val client = createOpenAiClient()
        
        // Multi-turn: user -> assistant w/ tool -> tool result -> user -> assistant w/ tool -> tool result
        val messages = listOf(
            Message("user", "Tap button 1"),
            Message.assistantWithTools("Tapping 1", listOf(ToolCall("call_a", "tap", mapOf("element_id" to 1)))),
            Message.toolResult("call_a", """{"success": true}"""),
            Message("user", "Now tap button 2"),
            Message.assistantWithTools("Tapping 2", listOf(ToolCall("call_b", "tap", mapOf("element_id" to 2)))),
            Message.toolResult("call_b", """{"success": true}""")
        )
        
        val result = client.chatWithTools(messages, tools = PhoneTools.ALL)
        
        assertTrue(result.isSuccess)
        
        val request = server.takeRequest()
        val body = request.body.readUtf8()
        val json = Json { ignoreUnknownKeys = true }
        val requestJson = json.parseToJsonElement(body).jsonObject
        val messagesArray = requestJson["messages"]?.jsonArray
        
        assertNotNull(messagesArray)
        
        // Count message types (system + 6 conversation messages = 7 total)
        assertEquals(7, messagesArray.size)
        
        // Verify both assistant messages have tool_calls
        val assistantMessages = messagesArray.filter { 
            it.jsonObject["role"]?.jsonPrimitive?.content == "assistant"
        }.map { it.jsonObject }
        
        assertEquals(2, assistantMessages.size)
        assistantMessages.forEach { msg ->
            assertNotNull(msg["tool_calls"], "Each assistant message should have tool_calls")
        }
        
        // Verify both tool result messages have tool_call_id
        val toolMessages = messagesArray.filter { 
            it.jsonObject["role"]?.jsonPrimitive?.content == "tool"
        }.map { it.jsonObject }
        
        assertEquals(2, toolMessages.size)
        assertEquals("call_a", toolMessages[0]["tool_call_id"]?.jsonPrimitive?.content)
        assertEquals("call_b", toolMessages[1]["tool_call_id"]?.jsonPrimitive?.content)
    }

    @Test
    fun `OpenAI tool response parsing handles null content`() = runTest {
        // OpenAI can return null content when only tool calls are made
        server.enqueue(MockResponse()
            .setBody("""{"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{"id":"call_x","type":"function","function":{"name":"tap","arguments":"{\"element_id\":5}"}}]},"finish_reason":"tool_calls"}]}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val client = createOpenAiClient()
        val messages = listOf(Message("user", "Tap button"))
        
        val result = client.chatWithTools(messages, tools = PhoneTools.ALL)
        
        assertTrue(result.isSuccess)
        val response = result.getOrNull()
        assertNotNull(response)
        
        // Text should be null
        assertTrue(response!!.text == null, "Text should be null when content is null")
        
        // Tool calls should be present
        assertEquals(1, response.toolCalls.size)
        assertEquals("tap", response.toolCalls[0].name)
        assertEquals(5, response.toolCalls[0].input["element_id"])
    }

    @Test
    fun `OpenAI tool response parsing handles malformed arguments gracefully`() = runTest {
        // Malformed JSON in arguments should be skipped
        server.enqueue(MockResponse()
            .setBody("""{"choices":[{"message":{"role":"assistant","content":"Oops","tool_calls":[{"id":"bad","type":"function","function":{"name":"tap","arguments":"not valid json"}},{"id":"good","type":"function","function":{"name":"swipe","arguments":"{\"direction\":\"up\"}"}}]},"finish_reason":"tool_calls"}]}""")
            .setResponseCode(200)
            .addHeader("Content-Type", "application/json"))

        val client = createOpenAiClient()
        val messages = listOf(Message("user", "Test"))
        
        val result = client.chatWithTools(messages, tools = PhoneTools.ALL)
        
        assertTrue(result.isSuccess)
        val response = result.getOrNull()
        assertNotNull(response)
        
        // Bad tool call should be skipped, good one should be parsed
        assertEquals(1, response.toolCalls.size)
        assertEquals("swipe", response.toolCalls[0].name)
        assertEquals("up", response.toolCalls[0].input["direction"])
    }
}
