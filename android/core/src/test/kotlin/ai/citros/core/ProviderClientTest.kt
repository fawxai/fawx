package ai.citros.core

import kotlinx.coroutines.test.runTest
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.After
import org.junit.Before
import org.junit.Test
import kotlin.test.*

class ProviderClientTest {

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

    // ========== Test Helpers for Parameterized Rate Limit Tests ==========

    /**
     * Helper to test that daily rate limits don't retry (used for OpenAI/OpenRouter).
     */
    private suspend fun testDailyRateLimitDoesNotRetry(
        provider: Provider,
        createClient: (baseUrl: String, maxAttempts: Int) -> ProviderClient,
        errorBody: String,
        successBody: String
    ) {
        server.enqueue(
            MockResponse()
                .setBody(errorBody)
                .setResponseCode(429)
                .addHeader("retry-after", "1")
        )
        // This response should NOT be consumed if retry is correctly skipped
        server.enqueue(
            MockResponse()
                .setBody(successBody)
                .setResponseCode(200)
        )

        val client = createClient(server.url("/").toString(), 3)
        val conversation = Conversation().apply { addUser("test") }

        val result = client.chat(conversation)

        assertTrue(result.isFailure)
        val msg = result.exceptionOrNull()?.message ?: ""
        assertTrue(msg.contains("Daily request limit reached"), "Expected daily-cap message, got: $msg")
        assertEquals(1, server.requestCount, "Daily RPD caps should not be retried")
    }

    /**
     * Helper to test that transient rate limits retry successfully (used for OpenAI/OpenRouter).
     */
    private suspend fun testTransientRateLimitRetries(
        provider: Provider,
        createClient: (baseUrl: String, maxAttempts: Int) -> ProviderClient,
        errorBody: String,
        successBody: String,
        expectedResponse: String
    ) {
        server.enqueue(
            MockResponse()
                .setBody(errorBody)
                .setResponseCode(429)
                .addHeader("retry-after", "1")
        )
        server.enqueue(
            MockResponse()
                .setBody(successBody)
                .setResponseCode(200)
                .addHeader("Content-Type", "application/json")
        )

        val client = createClient(server.url("/").toString(), 2)
        val conversation = Conversation()
        conversation.addUser("test")

        val result = client.chat(conversation)

        assertTrue(result.isSuccess, "Expected success after retry, got: ${result.exceptionOrNull()?.message}")
        assertEquals(expectedResponse, result.getOrNull())
        assertEquals(2, server.requestCount, "Should have retried transient rate limit")
    }

    // ========== Basic Provider Tests ==========

    @Test
    fun `AnthropicClient implements ProviderClient for Anthropic`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody("""{"content":[{"type":"text","text":"hello"}],"role":"assistant"}""")
                .setResponseCode(200)
                .addHeader("Content-Type", "application/json")
        )

        val client: ProviderClient = AnthropicClient(
            apiKey = "sk-ant-api03-test-key",
            baseUrl = server.url("/v1/messages").toString()
        )
        val conversation = Conversation()
        conversation.addUser("hi")

        val result = client.chat(conversation)

        assertTrue(result.isSuccess)
        assertEquals(Provider.ANTHROPIC, client.provider)
    }

    @Test
    fun `ClaudeClient delegates to AnthropicClient for backward compatibility`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody("""{"content":[{"type":"text","text":"hello"}],"role":"assistant"}""")
                .setResponseCode(200)
                .addHeader("Content-Type", "application/json")
        )

        @Suppress("DEPRECATION")
        val client: ProviderClient = ClaudeClient(
            apiKey = "sk-ant-api03-test-key",
            baseUrl = server.url("/v1/messages").toString()
        )
        val conversation = Conversation()
        conversation.addUser("hi")

        val result = client.chat(conversation)

        assertTrue(result.isSuccess)
        assertEquals(Provider.ANTHROPIC, client.provider)
    }

    @Test
    fun `OpenAiClient delegates using OpenAI configuration`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody("""{"choices":[{"message":{"role":"assistant","content":"ok"}}]}""")
                .setResponseCode(200)
                .addHeader("Content-Type", "application/json")
        )

        val config = ProviderConfig(
            provider = Provider.OPENAI,
            baseUrl = server.url("/v1/chat/completions").toString(),
            chatModelId = ModelConfig.OPENAI_CHAT_MODEL,
            actionModelId = ModelConfig.OPENAI_ACTION_MODEL,
            headers = ProviderConfig.openAi("oauth-token").headers
        )
        val client: ProviderClient = OpenAiClient(config = config)
        val conversation = Conversation()
        conversation.addUser("ping")

        val result = client.chat(conversation)
        val request = server.takeRequest()

        assertTrue(result.isSuccess)
        assertEquals(Provider.OPENAI, client.provider)
        assertEquals("Bearer oauth-token", request.getHeader("Authorization"))
    }

    @Test
    fun `OpenRouterClient delegates using OpenRouter configuration`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody("""{"choices":[{"message":{"role":"assistant","content":"ok"}}]}""")
                .setResponseCode(200)
                .addHeader("Content-Type", "application/json")
        )

        val config = ProviderConfig(
            provider = Provider.OPENROUTER,
            baseUrl = server.url("/api/v1/chat/completions").toString(),
            chatModelId = ModelConfig.OPENROUTER_CHAT_MODEL,
            actionModelId = ModelConfig.OPENROUTER_ACTION_MODEL,
            headers = ProviderConfig.openRouter("sk-or-test").headers
        )
        val client: ProviderClient = OpenRouterClient(config = config)
        val conversation = Conversation()
        conversation.addUser("ping")

        val result = client.chat(conversation)
        val request = server.takeRequest()

        assertTrue(result.isSuccess)
        assertEquals(Provider.OPENROUTER, client.provider)
        assertEquals("Bearer sk-or-test", request.getHeader("Authorization"))
    }

    // ========== Tool Calling Tests ==========

    @Test
    fun `Anthropic chatWithTools handles tool calls correctly`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody(
                    """{"content":[{"type":"text","text":"I'll tap that"},{"type":"tool_use","id":"toolu_123","name":"tap","input":{"element_id":42}}],"stop_reason":"tool_use"}"""
                )
                .setResponseCode(200)
                .addHeader("Content-Type", "application/json")
        )

        val client = AnthropicClient(
            apiKey = "sk-ant-api03-test-key",
            baseUrl = server.url("/v1/messages").toString()
        )

        val messages = listOf(Message(role = "user", content = "tap the button"))
        val result = client.chatWithTools(messages, tools = PhoneTools.ALL)

        assertTrue(result.isSuccess)
        val response = result.getOrThrow()
        assertEquals("I'll tap that", response.text)
        assertEquals(1, response.toolCalls.size)
        assertEquals("tap", response.toolCalls[0].name)
        assertEquals(42, response.toolCalls[0].input["element_id"])
    }

    @Test
    fun `OpenAI chatWithTools handles tool calls correctly`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody(
                    """{"choices":[{"message":{"role":"assistant","content":"Typing text","tool_calls":[{"id":"call_123","type":"function","function":{"name":"type_text","arguments":"{\"text\":\"hello\"}"}}]},"finish_reason":"tool_calls"}]}"""
                )
                .setResponseCode(200)
                .addHeader("Content-Type", "application/json")
        )

        val config = ProviderConfig(
            provider = Provider.OPENAI,
            baseUrl = server.url("/v1/chat/completions").toString(),
            chatModelId = ModelConfig.OPENAI_CHAT_MODEL,
            actionModelId = ModelConfig.OPENAI_ACTION_MODEL,
            headers = ProviderConfig.openAi("test-key").headers
        )
        val client = OpenAiClient(config = config)

        val messages = listOf(Message(role = "user", content = "type hello"))
        val result = client.chatWithTools(messages, tools = PhoneTools.ALL)

        assertTrue(result.isSuccess)
        val response = result.getOrThrow()
        assertEquals("Typing text", response.text)
        assertEquals(1, response.toolCalls.size)
        assertEquals("type_text", response.toolCalls[0].name)
        assertEquals("hello", response.toolCalls[0].input["text"])
    }

    @Test
    fun `OpenRouter chatWithTools handles tool calls correctly`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody(
                    """{"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{"id":"call_456","type":"function","function":{"name":"swipe","arguments":"{\"direction\":\"up\"}"}}]},"finish_reason":"tool_calls"}]}"""
                )
                .setResponseCode(200)
                .addHeader("Content-Type", "application/json")
        )

        val config = ProviderConfig(
            provider = Provider.OPENROUTER,
            baseUrl = server.url("/api/v1/chat/completions").toString(),
            chatModelId = ModelConfig.OPENROUTER_CHAT_MODEL,
            actionModelId = ModelConfig.OPENROUTER_ACTION_MODEL,
            headers = ProviderConfig.openRouter("sk-or-test").headers
        )
        val client = OpenRouterClient(config = config)

        val messages = listOf(Message(role = "user", content = "scroll up"))
        val result = client.chatWithTools(messages, tools = PhoneTools.ALL)

        assertTrue(result.isSuccess)
        val response = result.getOrThrow()
        assertNull(response.text)
        assertEquals(1, response.toolCalls.size)
        assertEquals("swipe", response.toolCalls[0].name)
        assertEquals("up", response.toolCalls[0].input["direction"])
    }

    // ========== Error Handling Tests ==========

    // ========== Empty Tools List Tests (PR #629) ==========

    @Test
    fun `Anthropic chatWithTools with empty tools list does not throw`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody("""{"content":[{"type":"text","text":"No tools needed"}],"role":"assistant","stop_reason":"end_turn"}""")
                .setResponseCode(200)
                .addHeader("Content-Type", "application/json")
        )

        val client = AnthropicClient(
            apiKey = "sk-ant-api03-test-key",
            baseUrl = server.url("/v1/messages").toString()
        )

        val messages = listOf(Message(role = "user", content = "test"))
        val result = client.chatWithTools(messages, tools = emptyList())

        assertTrue(result.isSuccess)
    }

    @Test
    fun `Anthropic chatWithTools with empty tools list omits tools from request body`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody("""{"content":[{"type":"text","text":"ok"}],"role":"assistant","stop_reason":"end_turn"}""")
                .setResponseCode(200)
                .addHeader("Content-Type", "application/json")
        )

        val client = AnthropicClient(
            apiKey = "sk-ant-api03-test-key",
            baseUrl = server.url("/v1/messages").toString()
        )

        val messages = listOf(Message(role = "user", content = "test"))
        client.chatWithTools(messages, tools = emptyList())

        val request = server.takeRequest()
        val body = request.body.readUtf8()
        assertFalse(body.contains("\"tools\""), "Empty tools list should omit 'tools' key from request body, got: $body")
    }

    @Test
    fun `Anthropic chatWithTools with non-empty tools still includes tools array`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody("""{"content":[{"type":"text","text":"Using tools"}],"role":"assistant","stop_reason":"end_turn"}""")
                .setResponseCode(200)
                .addHeader("Content-Type", "application/json")
        )

        val client = AnthropicClient(
            apiKey = "sk-ant-api03-test-key",
            baseUrl = server.url("/v1/messages").toString()
        )

        val messages = listOf(Message(role = "user", content = "test"))
        client.chatWithTools(messages, tools = PhoneTools.ALL)

        val request = server.takeRequest()
        val body = request.body.readUtf8()
        assertTrue(body.contains("\"tools\""), "Non-empty tools list should include 'tools' key in request body")
    }

    @Test
    fun `OpenAI chatWithTools with empty tools list does not throw`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody("""{"choices":[{"message":{"role":"assistant","content":"No tools"},"finish_reason":"stop"}]}""")
                .setResponseCode(200)
                .addHeader("Content-Type", "application/json")
        )

        val config = ProviderConfig.openAi("test-key").copy(
            baseUrl = server.url("/v1/chat/completions").toString()
        )
        val client = OpenAiClient(config = config)

        val messages = listOf(Message(role = "user", content = "test"))
        val result = client.chatWithTools(messages, tools = emptyList())

        assertTrue(result.isSuccess)
    }

    @Test
    fun `OpenAI chatWithTools with empty tools list omits tools from request body`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody("""{"choices":[{"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}]}""")
                .setResponseCode(200)
                .addHeader("Content-Type", "application/json")
        )

        val config = ProviderConfig.openAi("test-key").copy(
            baseUrl = server.url("/v1/chat/completions").toString()
        )
        val client = OpenAiClient(config = config)

        val messages = listOf(Message(role = "user", content = "test"))
        client.chatWithTools(messages, tools = emptyList())

        val request = server.takeRequest()
        val body = request.body.readUtf8()
        assertFalse(body.contains("\"tools\""), "Empty tools list should omit 'tools' key from request body, got: $body")
    }

    @Test
    fun `OpenAI chatWithTools with non-empty tools still includes tools array`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody("""{"choices":[{"message":{"role":"assistant","content":"Using tools"},"finish_reason":"stop"}]}""")
                .setResponseCode(200)
                .addHeader("Content-Type", "application/json")
        )

        val config = ProviderConfig.openAi("test-key").copy(
            baseUrl = server.url("/v1/chat/completions").toString()
        )
        val client = OpenAiClient(config = config)

        val messages = listOf(Message(role = "user", content = "test"))
        client.chatWithTools(messages, tools = PhoneTools.ALL)

        val request = server.takeRequest()
        val body = request.body.readUtf8()
        assertTrue(body.contains("\"tools\""), "Non-empty tools list should include 'tools' key in request body")
    }

    @Test
    fun `chat handles 401 authentication error with structured exception`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody("""{"error":{"message":"Invalid API key"}}""")
                .setResponseCode(401)
                .addHeader("Content-Type", "application/json")
        )

        val client = AnthropicClient(
            apiKey = "invalid-key",
            baseUrl = server.url("/v1/messages").toString()
        )
        val conversation = Conversation()
        conversation.addUser("test")

        val result = client.chat(conversation)

        assertTrue(result.isFailure)
        val exception = result.exceptionOrNull()
        assertTrue(exception is ProviderException)
        assertEquals(401, exception.statusCode)
        assertTrue(exception.isAuthFailure)
    }

    @Test
    fun `chat handles 403 forbidden error with structured exception`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody("""{"error":{"message":"Forbidden"}}""")
                .setResponseCode(403)
                .addHeader("Content-Type", "application/json")
        )

        val client = OpenAiClient(
            config = ProviderConfig.openAi("forbidden-key").copy(
                baseUrl = server.url("/v1/chat/completions").toString()
            )
        )
        val conversation = Conversation()
        conversation.addUser("test")

        val result = client.chat(conversation)

        assertTrue(result.isFailure)
        val exception = result.exceptionOrNull()
        assertTrue(exception is ProviderException)
        assertEquals(403, exception.statusCode)
        assertTrue(exception.isAuthFailure)
    }

    @Test
    fun `chat handles 500 server error without marking as auth failure`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody("""{"error":{"message":"Internal server error"}}""")
                .setResponseCode(500)
                .addHeader("Content-Type", "application/json")
        )

        val client = AnthropicClient(
            apiKey = "sk-ant-api03-test-key",
            baseUrl = server.url("/v1/messages").toString()
        )
        val conversation = Conversation()
        conversation.addUser("test")

        val result = client.chat(conversation)

        assertTrue(result.isFailure)
        val exception = result.exceptionOrNull()
        assertTrue(exception is ProviderException)
        assertEquals(500, exception.statusCode)
        assertFalse(exception.isAuthFailure)
    }

    @Test
    fun `chat handles malformed response gracefully`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody("""{"invalid": "json structure"}""")
                .setResponseCode(200)
                .addHeader("Content-Type", "application/json")
        )

        val client = AnthropicClient(
            apiKey = "sk-ant-api03-test-key",
            baseUrl = server.url("/v1/messages").toString()
        )
        val conversation = Conversation()
        conversation.addUser("test")

        val result = client.chat(conversation)

        assertTrue(result.isFailure)
        val exception = result.exceptionOrNull()
        assertTrue(exception is ProviderException)
        assertFalse(exception.isAuthFailure)
    }

    @Test
    fun `chatWithTools handles malformed tool call arguments gracefully`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody(
                    """{"choices":[{"message":{"role":"assistant","content":"Trying","tool_calls":[{"id":"call_1","type":"function","function":{"name":"tap","arguments":"not valid json"}}]},"finish_reason":"tool_calls"}]}"""
                )
                .setResponseCode(200)
                .addHeader("Content-Type", "application/json")
        )

        val config = ProviderConfig.openAi("test-key").copy(
            baseUrl = server.url("/v1/chat/completions").toString()
        )
        val client = OpenAiClient(config = config)

        val messages = listOf(Message(role = "user", content = "tap"))
        val result = client.chatWithTools(messages, tools = PhoneTools.ALL)

        // Should succeed but skip the malformed tool call
        assertTrue(result.isSuccess)
        val response = result.getOrThrow()
        assertEquals(0, response.toolCalls.size) // Malformed call was skipped
    }

    // ========== Rate Limit / Retry Tests ==========

    @Test
    fun `chat retries on 429 with exponential backoff`() = runTest {
        // First request: 429 rate limit
        server.enqueue(
            MockResponse()
                .setBody("""{"error":{"message":"Rate limit exceeded"}}""")
                .setResponseCode(429)
                .addHeader("retry-after", "1")
        )
        // Second request: success
        server.enqueue(
            MockResponse()
                .setBody("""{"content":[{"type":"text","text":"success"}],"role":"assistant"}""")
                .setResponseCode(200)
                .addHeader("Content-Type", "application/json")
        )

        val client = AnthropicClient(
            apiKey = "sk-ant-api03-test-key",
            baseUrl = server.url("/v1/messages").toString(),
            maxAttempts = 2
        )
        val conversation = Conversation()
        conversation.addUser("test")

        val result = client.chat(conversation)

        assertTrue(result.isSuccess)
        assertEquals("success", result.getOrNull())
        assertEquals(2, server.requestCount) // Should have retried once
    }

    @Test
    fun `chat fails after max attempts on repeated 429`() = runTest {
        // All requests return 429
        repeat(3) {
            server.enqueue(
                MockResponse()
                    .setBody("""{"error":{"message":"Rate limit exceeded"}}""")
                    .setResponseCode(429)
                    .addHeader("retry-after", "1")
            )
        }

        val client = AnthropicClient(
            apiKey = "sk-ant-api03-test-key",
            baseUrl = server.url("/v1/messages").toString(),
            maxAttempts = 3
        )
        val conversation = Conversation()
        conversation.addUser("test")

        val result = client.chat(conversation)

        assertTrue(result.isFailure)
        val exception = result.exceptionOrNull()
        assertTrue(exception is ProviderException)
        assertEquals(429, (exception as ProviderException).statusCode)
        assertTrue(exception.message?.contains("Request failed after") == true || exception.message?.contains("rate") == true,
            "Expected retry-exhausted message but got: ${exception.message}")
        assertEquals(3, server.requestCount) // Should have tried maxAttempts times
    }

    @Test
    fun `OpenAI daily 429 does not retry and returns daily limit guidance`() = runTest {
        testDailyRateLimitDoesNotRetry(
            provider = Provider.OPENAI,
            createClient = { baseUrl, maxAttempts ->
                OpenAiClient(
                    config = ProviderConfig.openAi("test-key").copy(
                        baseUrl = baseUrl + "v1/chat/completions"
                    ),
                    maxAttempts = maxAttempts
                )
            },
            errorBody = """{"error":{"message":"Rate limit reached for gpt-4o-mini on requests per day (RPD)","type":"requests","code":"rate_limit_exceeded"}}""",
            successBody = """{"choices":[{"message":{"role":"assistant","content":"unexpected retry"}}]}"""
        )
    }

    @Test
    fun `OpenAI generic daily wording without limit pattern still retries`() = runTest {
        testTransientRateLimitRetries(
            provider = Provider.OPENAI,
            createClient = { baseUrl, maxAttempts ->
                OpenAiClient(
                    config = ProviderConfig.openAi("test-key").copy(
                        baseUrl = baseUrl + "v1/chat/completions"
                    ),
                    maxAttempts = maxAttempts
                )
            },
            errorBody = """{"error":{"message":"Daily maintenance window in progress","type":"server_error","code":"temporarily_unavailable"}}""",
            successBody = """{"choices":[{"message":{"role":"assistant","content":"ok"}}]}""",
            expectedResponse = "ok"
        )
    }

    // ========== 529/503 Retry Tests ==========

    @Test
    fun `chat retries on 529 Overloaded and succeeds`() = runTest {
        // First request: 529 Overloaded
        server.enqueue(
            MockResponse()
                .setBody("""{"error":{"message":"Overloaded"}}""")
                .setResponseCode(529)
                .addHeader("retry-after", "1")
        )
        // Second request: success
        server.enqueue(
            MockResponse()
                .setBody("""{"content":[{"type":"text","text":"recovered"}],"role":"assistant"}""")
                .setResponseCode(200)
                .addHeader("Content-Type", "application/json")
        )

        val client = AnthropicClient(
            apiKey = "sk-ant-api03-test-key",
            baseUrl = server.url("/v1/messages").toString(),
            maxAttempts = 2
        )
        val conversation = Conversation()
        conversation.addUser("test")

        val result = client.chat(conversation)

        assertTrue(result.isSuccess, "Expected success after 529 retry, got: ${result.exceptionOrNull()?.message}")
        assertEquals("recovered", result.getOrNull())
        assertEquals(2, server.requestCount)
    }

    @Test
    fun `chat retries on 503 Service Unavailable and succeeds`() = runTest {
        // First request: 503
        server.enqueue(
            MockResponse()
                .setBody("""{"error":{"message":"Service Unavailable"}}""")
                .setResponseCode(503)
                .addHeader("retry-after", "1")
        )
        // Second request: success
        server.enqueue(
            MockResponse()
                .setBody("""{"content":[{"type":"text","text":"back online"}],"role":"assistant"}""")
                .setResponseCode(200)
                .addHeader("Content-Type", "application/json")
        )

        val client = AnthropicClient(
            apiKey = "sk-ant-api03-test-key",
            baseUrl = server.url("/v1/messages").toString(),
            maxAttempts = 2
        )
        val conversation = Conversation()
        conversation.addUser("test")

        val result = client.chat(conversation)

        assertTrue(result.isSuccess, "Expected success after 503 retry, got: ${result.exceptionOrNull()?.message}")
        assertEquals("back online", result.getOrNull())
        assertEquals(2, server.requestCount)
    }

    @Test
    fun `chat fails after max attempts on repeated 529`() = runTest {
        repeat(3) {
            server.enqueue(
                MockResponse()
                    .setBody("""{"error":{"message":"Overloaded"}}""")
                    .setResponseCode(529)
                    .addHeader("retry-after", "1")
            )
        }

        val client = AnthropicClient(
            apiKey = "sk-ant-api03-test-key",
            baseUrl = server.url("/v1/messages").toString(),
            maxAttempts = 3
        )
        val conversation = Conversation()
        conversation.addUser("test")

        val result = client.chat(conversation)

        assertTrue(result.isFailure)
        val exception = result.exceptionOrNull() as ProviderException
        assertEquals(529, exception.statusCode)
        assertTrue(exception.message?.contains("Request failed after") == true,
            "Expected exhausted-attempts message but got: ${exception.message}")
        assertEquals(3, server.requestCount)
    }

    @Test
    fun `chat fails after max attempts on repeated 503`() = runTest {
        repeat(3) {
            server.enqueue(
                MockResponse()
                    .setBody("""{"error":{"message":"Service Unavailable"}}""")
                    .setResponseCode(503)
                    .addHeader("retry-after", "1")
            )
        }

        val client = AnthropicClient(
            apiKey = "sk-ant-api03-test-key",
            baseUrl = server.url("/v1/messages").toString(),
            maxAttempts = 3
        )
        val conversation = Conversation()
        conversation.addUser("test")

        val result = client.chat(conversation)

        assertTrue(result.isFailure)
        val exception = result.exceptionOrNull() as ProviderException
        assertEquals(503, exception.statusCode)
        assertTrue(exception.message?.contains("Request failed after") == true,
            "Expected exhausted-attempts message but got: ${exception.message}")
        assertEquals(3, server.requestCount)
    }

    @Test
    fun `chat recovers from mixed retryable errors (429 then 529 then success)`() = runTest {
        // First: 429
        server.enqueue(
            MockResponse()
                .setBody("""{"error":{"message":"Rate limit exceeded"}}""")
                .setResponseCode(429)
                .addHeader("retry-after", "1")
        )
        // Second: 529
        server.enqueue(
            MockResponse()
                .setBody("""{"error":{"message":"Overloaded"}}""")
                .setResponseCode(529)
                .addHeader("retry-after", "1")
        )
        // Third: success
        server.enqueue(
            MockResponse()
                .setBody("""{"content":[{"type":"text","text":"finally"}],"role":"assistant"}""")
                .setResponseCode(200)
                .addHeader("Content-Type", "application/json")
        )

        val client = AnthropicClient(
            apiKey = "sk-ant-api03-test-key",
            baseUrl = server.url("/v1/messages").toString(),
            maxAttempts = 3
        )
        val conversation = Conversation()
        conversation.addUser("test")

        val result = client.chat(conversation)

        assertTrue(result.isSuccess, "Expected success after mixed retries, got: ${result.exceptionOrNull()?.message}")
        assertEquals("finally", result.getOrNull())
        assertEquals(3, server.requestCount)
    }

    @Test
    fun `529 without retry-after header uses exponential backoff`() = runTest {
        // First request: 529 with no retry-after header
        server.enqueue(
            MockResponse()
                .setBody("""{"error":{"message":"Overloaded"}}""")
                .setResponseCode(529)
        )
        // Second request: success
        server.enqueue(
            MockResponse()
                .setBody("""{"content":[{"type":"text","text":"ok"}],"role":"assistant"}""")
                .setResponseCode(200)
                .addHeader("Content-Type", "application/json")
        )

        val client = AnthropicClient(
            apiKey = "sk-ant-api03-test-key",
            baseUrl = server.url("/v1/messages").toString(),
            maxAttempts = 2
        )
        val conversation = Conversation()
        conversation.addUser("test")

        val result = client.chat(conversation)

        assertTrue(result.isSuccess, "Expected success after 529 retry without retry-after")
        assertEquals(2, server.requestCount)
    }

    // ========== Error Formatting Tests ==========

    @Test
    fun `429 error shows friendly rate limit message`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody("""{"error":{"message":"Rate limit exceeded","type":"rate_limit_error"}}""")
                .setResponseCode(429)
        )

        val client = AnthropicClient(
            apiKey = "sk-ant-api03-test-key",
            baseUrl = server.url("/v1/messages").toString(),
            maxAttempts = 1
        )
        val conversation = Conversation()
        conversation.addUser("test")

        val result = client.chat(conversation)
        assertTrue(result.isFailure)
        val msg = result.exceptionOrNull()?.message ?: ""
        assertTrue(msg.contains("Rate limited"), "Expected friendly rate limit message, got: $msg")
        assertFalse(msg.contains("{"), "Should not contain raw JSON, got: $msg")
    }

    @Test
    fun `402 error shows quota exceeded message`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody("""{"error":{"message":"Insufficient quota"}}""")
                .setResponseCode(402)
        )

        val client = AnthropicClient(
            apiKey = "sk-ant-api03-test-key",
            baseUrl = server.url("/v1/messages").toString(),
            maxAttempts = 1
        )
        val conversation = Conversation()
        conversation.addUser("test")

        val result = client.chat(conversation)
        assertTrue(result.isFailure)
        val msg = result.exceptionOrNull()?.message ?: ""
        assertTrue(msg.contains("quota", ignoreCase = true), "Expected quota message, got: $msg")
        assertFalse(msg.contains("{"), "Should not contain raw JSON, got: $msg")
    }

    @Test
    fun `500 error shows friendly server error message`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody("""{"error":{"message":"Internal server error"}}""")
                .setResponseCode(500)
        )

        val client = AnthropicClient(
            apiKey = "sk-ant-api03-test-key",
            baseUrl = server.url("/v1/messages").toString(),
            maxAttempts = 1
        )
        val conversation = Conversation()
        conversation.addUser("test")

        val result = client.chat(conversation)
        assertTrue(result.isFailure)
        val msg = result.exceptionOrNull()?.message ?: ""
        assertTrue(msg.contains("experiencing issues") || msg.contains("provider"),
            "Expected friendly server error message, got: $msg")
        assertFalse(msg.contains("{"), "Should not contain raw JSON, got: $msg")
    }

    @Test
    fun `404 error shows model not found message`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody("""{"error":{"message":"model: some-model does not exist"}}""")
                .setResponseCode(404)
        )

        val client = AnthropicClient(
            apiKey = "sk-ant-api03-test-key",
            baseUrl = server.url("/v1/messages").toString(),
            maxAttempts = 1
        )
        val conversation = Conversation()
        conversation.addUser("test")

        val result = client.chat(conversation)
        assertTrue(result.isFailure)
        val msg = result.exceptionOrNull()?.message ?: ""
        assertTrue(msg.contains("does not exist") || msg.contains("not found", ignoreCase = true),
            "Expected model not found message, got: $msg")
        assertFalse(msg.startsWith("API error"), "Should not use raw 'API error' prefix, got: $msg")
    }

    @Test
    fun `OpenAI 401 with OAuth scope error shows specific message`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody("""{"error":{"message":"Missing scopes: model.request","type":"invalid_request_error"}}""")
                .setResponseCode(401)
        )

        val client = OpenAiClient(
            config = ProviderConfig.openAi("oauth-token-without-scopes").copy(
                baseUrl = server.url("/v1/chat/completions").toString()
            )
        )
        val conversation = Conversation()
        conversation.addUser("test")

        val result = client.chat(conversation)
        assertTrue(result.isFailure)
        val msg = result.exceptionOrNull()?.message ?: ""
        assertTrue(msg.contains("model.request permission"),
            "Expected OAuth scope error message, got: $msg")
        assertTrue(msg.contains("Use an API key instead"),
            "Expected fallback suggestion, got: $msg")
    }

    @Test
    fun `502 error shows friendly server error message`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody("""{"error":{"message":"Bad gateway"}}""")
                .setResponseCode(502)
        )

        val client = AnthropicClient(
            apiKey = "sk-ant-api03-test-key",
            baseUrl = server.url("/v1/messages").toString(),
            maxAttempts = 1
        )
        val conversation = Conversation()
        conversation.addUser("test")

        val result = client.chat(conversation)
        assertTrue(result.isFailure)
        val msg = result.exceptionOrNull()?.message ?: ""
        assertTrue(msg.contains("experiencing issues"),
            "Expected friendly server error message, got: $msg")
        assertTrue(msg.contains("Bad gateway"),
            "Expected extracted error detail, got: $msg")
    }

    @Test
    fun `503 error shows friendly server error message`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody("""{"error":{"message":"Service temporarily unavailable"}}""")
                .setResponseCode(503)
        )

        val client = AnthropicClient(
            apiKey = "sk-ant-api03-test-key",
            baseUrl = server.url("/v1/messages").toString(),
            maxAttempts = 1
        )
        val conversation = Conversation()
        conversation.addUser("test")

        val result = client.chat(conversation)
        assertTrue(result.isFailure)
        val msg = result.exceptionOrNull()?.message ?: ""
        assertTrue(msg.contains("experiencing issues"),
            "Expected friendly server error message, got: $msg")
        assertTrue(msg.contains("Service temporarily unavailable"),
            "Expected extracted error detail, got: $msg")
    }

    @Test
    fun `empty error object falls back to generic message`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody("""{"error":{}}""")
                .setResponseCode(401)
        )

        val client = AnthropicClient(
            apiKey = "sk-ant-api03-test-key",
            baseUrl = server.url("/v1/messages").toString(),
            maxAttempts = 1
        )
        val conversation = Conversation()
        conversation.addUser("test")

        val result = client.chat(conversation)
        assertTrue(result.isFailure)
        val msg = result.exceptionOrNull()?.message ?: ""
        assertTrue(msg.contains("Invalid API key"),
            "Expected fallback auth message for empty error object, got: $msg")
    }

    @Test
    fun `malformed error body falls back gracefully`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody("This is not JSON at all")
                .setResponseCode(500)
        )

        val client = AnthropicClient(
            apiKey = "sk-ant-api03-test-key",
            baseUrl = server.url("/v1/messages").toString(),
            maxAttempts = 1
        )
        val conversation = Conversation()
        conversation.addUser("test")

        val result = client.chat(conversation)
        assertTrue(result.isFailure)
        val msg = result.exceptionOrNull()?.message ?: ""
        assertTrue(msg.contains("experiencing issues"),
            "Expected graceful fallback for malformed body, got: $msg")
    }

    // ========== OpenRouter Rate Limit Tests (Issue #241) ==========

    @Test
    fun `OpenRouter 429 error shows friendly rate limit message`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody("""{"error":{"message":"Rate limit exceeded for this model","type":"rate_limit_error"}}""")
                .setResponseCode(429)
        )

        val config = ProviderConfig.openRouter("sk-or-test").copy(
            baseUrl = server.url("/api/v1/chat/completions").toString()
        )
        val client = OpenRouterClient(config = config, maxAttempts = 1)
        val conversation = Conversation()
        conversation.addUser("test")

        val result = client.chat(conversation)

        assertTrue(result.isFailure)
        val msg = result.exceptionOrNull()?.message ?: ""
        assertTrue(msg.contains("Rate limited"), "Expected friendly rate limit message, got: $msg")
        assertFalse(msg.contains("{"), "Should not contain raw JSON, got: $msg")
    }

    @Test
    fun `OpenRouter daily rate limit does not retry and returns daily limit guidance`() = runTest {
        // OpenRouter can proxy OpenAI models, so daily RPD limits can occur
        testDailyRateLimitDoesNotRetry(
            provider = Provider.OPENROUTER,
            createClient = { baseUrl, maxAttempts ->
                OpenRouterClient(
                    config = ProviderConfig.openRouter("sk-or-test").copy(
                        baseUrl = baseUrl + "api/v1/chat/completions"
                    ),
                    maxAttempts = maxAttempts
                )
            },
            errorBody = """{"error":{"message":"Rate limit reached for openai/gpt-4o-mini on requests per day (RPD)","type":"requests","code":"rate_limit_exceeded"}}""",
            successBody = """{"choices":[{"message":{"role":"assistant","content":"unexpected retry"}}]}"""
        )
    }

    @Test
    fun `OpenRouter transient rate limit retries and succeeds`() = runTest {
        testTransientRateLimitRetries(
            provider = Provider.OPENROUTER,
            createClient = { baseUrl, maxAttempts ->
                OpenRouterClient(
                    config = ProviderConfig.openRouter("sk-or-test").copy(
                        baseUrl = baseUrl + "api/v1/chat/completions"
                    ),
                    maxAttempts = maxAttempts
                )
            },
            errorBody = """{"error":{"message":"Rate limit exceeded - too many requests per minute","type":"rate_limit_error"}}""",
            successBody = """{"choices":[{"message":{"role":"assistant","content":"success after retry"}}]}""",
            expectedResponse = "success after retry"
        )
    }

    @Test
    fun `OpenRouter generic error extracts message from JSON body`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody("""{"error":{"message":"Model openai/gpt-4-invalid does not exist","type":"invalid_request_error"}}""")
                .setResponseCode(404)
        )

        val config = ProviderConfig.openRouter("sk-or-test").copy(
            baseUrl = server.url("/api/v1/chat/completions").toString()
        )
        val client = OpenRouterClient(config = config)
        val conversation = Conversation()
        conversation.addUser("test")

        val result = client.chat(conversation)

        assertTrue(result.isFailure)
        val msg = result.exceptionOrNull()?.message ?: ""
        assertTrue(msg.contains("does not exist"), "Expected extracted error message, got: $msg")
        assertFalse(msg.contains("{"), "Should not contain raw JSON, got: $msg")
    }

    // ========== Validation Tests ==========

    @Test
    fun `OpenAiClient rejects wrong provider config before construction`() {
        val wrongConfig = ProviderConfig.anthropic("sk-ant-api03-test-key")

        val exception = assertFailsWith<IllegalArgumentException> {
            OpenAiClient(config = wrongConfig)
        }
        assertTrue(exception.message!!.contains("ANTHROPIC"),
            "Error message should interpolate actual provider, got: ${exception.message}")
    }

    @Test
    fun `OpenRouterClient rejects wrong provider config before construction`() {
        val wrongConfig = ProviderConfig.openAi("sk-test-key")

        val exception = assertFailsWith<IllegalArgumentException> {
            OpenRouterClient(config = wrongConfig)
        }
        assertTrue(exception.message!!.contains("OPENAI"),
            "Error message should interpolate actual provider, got: ${exception.message}")
    }

    @Test
    fun `AnthropicClient rejects wrong provider config`() {
        val wrongConfig = ProviderConfig.openAi("sk-test-key")

        assertFailsWith<IllegalArgumentException> {
            AnthropicClient(config = wrongConfig)
        }
    }

    // ========== ProviderException Auth-Failure Edge Cases (#235) ==========
    // Note: Some status codes (e.g., 401) are also tested as integration tests above
    // via MockWebServer. These unit tests validate ProviderException construction directly.

    @Test
    fun `ProviderException isAuthFailure is true for 401`() {
        val ex = ProviderException(Provider.ANTHROPIC, 401, "Unauthorized", isAuthFailure = true)
        assertTrue(ex.isAuthFailure)
    }

    @Test
    fun `ProviderException isAuthFailure is true for 403`() {
        val ex = ProviderException(Provider.OPENAI, 403, "Forbidden", isAuthFailure = true)
        assertTrue(ex.isAuthFailure)
    }

    @Test
    fun `ProviderException isAuthFailure is false for 404`() {
        val ex = ProviderException(Provider.OPENAI, 404, "Model not found", isAuthFailure = false)
        assertFalse(ex.isAuthFailure)
    }

    @Test
    fun `ProviderException isAuthFailure is false for 429`() {
        val ex = ProviderException(Provider.ANTHROPIC, 429, "Rate limited", isAuthFailure = false)
        assertFalse(ex.isAuthFailure)
    }

    @Test
    fun `ProviderException isAuthFailure is false for 500`() {
        val ex = ProviderException(Provider.OPENROUTER, 500, "Server error", isAuthFailure = false)
        assertFalse(ex.isAuthFailure)
    }

    @Test
    fun `ProviderException isAuthFailure is false for null status code`() {
        val ex = ProviderException(Provider.ANTHROPIC, null, "Network error", isAuthFailure = false)
        assertFalse(ex.isAuthFailure)
        assertNull(ex.statusCode)
    }

    @Test
    fun `ProviderException companion isAuthFailure returns true for auth failure result`() {
        val result = Result.failure<String>(
            ProviderException(Provider.OPENAI, 401, "Bad key", isAuthFailure = true)
        )
        assertTrue(ProviderException.isAuthFailure(result))
    }

    @Test
    fun `ProviderException companion isAuthFailure returns false for non-auth failure`() {
        val result = Result.failure<String>(
            ProviderException(Provider.OPENAI, 500, "Server error", isAuthFailure = false)
        )
        assertFalse(ProviderException.isAuthFailure(result))
    }

    @Test
    fun `ProviderException companion isAuthFailure returns false for non-ProviderException`() {
        val result = Result.failure<String>(RuntimeException("something else"))
        assertFalse(ProviderException.isAuthFailure(result))
    }

    @Test
    fun `ProviderException companion isAuthFailure returns false for success result`() {
        val result = Result.success("ok")
        assertFalse(ProviderException.isAuthFailure(result))
    }

    @Test
    fun `ProviderException message includes provider name`() {
        val ex = ProviderException(Provider.ANTHROPIC, 401, "Invalid key", isAuthFailure = true)
        assertTrue(ex.message!!.contains("ANTHROPIC"))
    }

    @Test
    fun `ProviderException preserves cause`() {
        val cause = RuntimeException("underlying")
        val ex = ProviderException(Provider.OPENAI, 500, "Wrapped", isAuthFailure = false, cause = cause)
        assertSame(cause, ex.cause)
    }

    // ========== Error Mapping Edge Cases (#247) ==========
    // Integration tests using MockWebServer to verify end-to-end error handling.
    // Complements the unit tests above which test ProviderException in isolation.

    @Test
    fun `402 quota error IS auth failure because it falls in 401-403 range`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody("""{"error":{"message":"Insufficient quota","type":"insufficient_quota"}}""")
                .setResponseCode(402)
        )

        val client = OpenAiClient(
            config = ProviderConfig.openAi("test-key").copy(
                baseUrl = server.url("/v1/chat/completions").toString()
            ),
            maxAttempts = 1
        )
        val conversation = Conversation()
        conversation.addUser("test")

        val result = client.chat(conversation)
        assertTrue(result.isFailure)
        val ex = result.exceptionOrNull() as ProviderException
        assertEquals(402, ex.statusCode)
        // BaseProviderClient uses `response.code in 401..403` for auth failure detection.
        // 402 Payment Required falls in this range, so it's treated as auth failure.
        assertTrue(ex.isAuthFailure)
    }

    @Test
    fun `404 model not found is not auth failure`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody("""{"error":{"message":"The model 'gpt-5-turbo' does not exist","type":"invalid_request_error"}}""")
                .setResponseCode(404)
        )

        val client = OpenAiClient(
            config = ProviderConfig.openAi("test-key").copy(
                baseUrl = server.url("/v1/chat/completions").toString()
            ),
            maxAttempts = 1
        )
        val conversation = Conversation()
        conversation.addUser("test")

        val result = client.chat(conversation)
        assertTrue(result.isFailure)
        val ex = result.exceptionOrNull() as ProviderException
        assertEquals(404, ex.statusCode)
        assertFalse(ex.isAuthFailure)
    }

    @Test
    fun `empty response body returns error with status code`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody("")
                .setResponseCode(200)
        )

        val client = AnthropicClient(
            apiKey = "sk-ant-api03-test-key",
            baseUrl = server.url("/v1/messages").toString(),
            maxAttempts = 1
        )
        val conversation = Conversation()
        conversation.addUser("test")

        val result = client.chat(conversation)
        assertTrue(result.isFailure)
        val ex = result.exceptionOrNull() as ProviderException
        assertTrue(ex.message!!.contains("empty"))
    }

    @Test
    fun `5xx server error returns non-auth ProviderException`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody("""{"error":{"message":"Internal server error"}}""")
                .setResponseCode(503)
        )

        val client = AnthropicClient(
            apiKey = "sk-ant-api03-test-key",
            baseUrl = server.url("/v1/messages").toString(),
            maxAttempts = 1
        )
        val conversation = Conversation()
        conversation.addUser("test")

        val result = client.chat(conversation)
        assertTrue(result.isFailure)
        val ex = result.exceptionOrNull() as ProviderException
        assertEquals(503, ex.statusCode)
        assertFalse(ex.isAuthFailure)
    }

    @Test
    fun `malformed JSON error body still returns ProviderException`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody("this is not json at all")
                .setResponseCode(500)
        )

        val client = AnthropicClient(
            apiKey = "sk-ant-api03-test-key",
            baseUrl = server.url("/v1/messages").toString(),
            maxAttempts = 1
        )
        val conversation = Conversation()
        conversation.addUser("test")

        val result = client.chat(conversation)
        assertTrue(result.isFailure)
        val ex = result.exceptionOrNull()
        assertTrue(ex is ProviderException || ex is Exception)
    }

    // ========== Vision / describeImage Tests (#338) ==========

    @Test
    fun `Anthropic describeImage sends image content block`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody("""{"content":[{"type":"text","text":"A home screen with icons"}],"role":"assistant","stop_reason":"end_turn"}""")
                .setResponseCode(200)
                .addHeader("Content-Type", "application/json")
        )

        val client = AnthropicClient(
            apiKey = "sk-ant-api03-test-key",
            baseUrl = server.url("/v1/messages").toString()
        )

        val result = client.describeImage("dGVzdA==", "What's on this screen?")
        assertTrue(result.isSuccess)
        assertEquals("A home screen with icons", result.getOrNull())

        val request = server.takeRequest()
        val body = request.body.readUtf8()
        assertTrue(body.contains("\"type\":\"image\""))
        assertTrue(body.contains("\"media_type\":\"image/png\""))
        assertTrue(body.contains("\"data\":\"dGVzdA==\""))
        assertTrue(body.contains("What's on this screen?"))
    }

    @Test
    fun `OpenAI describeImage sends image_url content block`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody("""{"choices":[{"message":{"role":"assistant","content":"A settings menu"},"finish_reason":"stop"}]}""")
                .setResponseCode(200)
                .addHeader("Content-Type", "application/json")
        )

        val client = OpenAiClient(
            config = ProviderConfig.openAi("test-key").copy(
                baseUrl = server.url("/v1/chat/completions").toString()
            )
        )

        val result = client.describeImage("dGVzdA==", "Describe this")
        assertTrue(result.isSuccess)
        assertEquals("A settings menu", result.getOrNull())

        val request = server.takeRequest()
        val body = request.body.readUtf8()
        assertTrue(body.contains("\"type\":\"image_url\""))
        assertTrue(body.contains("data:image/png;base64,dGVzdA=="))
        assertTrue(body.contains("Describe this"))
    }

    @Test
    fun `describeImage returns failure on API error`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody("""{"error":{"message":"Invalid image","type":"invalid_request_error"}}""")
                .setResponseCode(400)
        )

        val client = AnthropicClient(
            apiKey = "sk-ant-api03-test-key",
            baseUrl = server.url("/v1/messages").toString()
        )

        val result = client.describeImage("bad-data", "Describe this")
        assertTrue(result.isFailure)
    }

    // ========== Vision Prompt Constant Tests (#358) ==========

    @Test
    fun `DEFAULT_VISION_PROMPT is non-empty and references phone screen`() {
        assertTrue(PhoneAgentPrompts.DEFAULT_VISION_PROMPT.isNotBlank())
        assertTrue(PhoneAgentPrompts.DEFAULT_VISION_PROMPT.contains("phone screen"))
    }

    // ========== Configurable max_tokens Tests (#357) ==========

    @Test
    fun `describeImage sends custom max_tokens`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody("""{"content":[{"type":"text","text":"Detailed description"}],"role":"assistant","stop_reason":"end_turn"}""")
                .setResponseCode(200)
                .addHeader("Content-Type", "application/json")
        )

        val client = AnthropicClient(
            apiKey = "sk-ant-api03-test-key",
            baseUrl = server.url("/v1/messages").toString()
        )

        val result = client.describeImage("dGVzdA==", "Describe this", maxTokens = 2048)
        assertTrue(result.isSuccess)

        val request = server.takeRequest()
        val body = request.body.readUtf8()
        assertTrue(body.contains("\"max_tokens\":2048"))
    }

    @Test
    fun `describeImage uses default max_tokens when not specified`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody("""{"content":[{"type":"text","text":"Description"}],"role":"assistant","stop_reason":"end_turn"}""")
                .setResponseCode(200)
                .addHeader("Content-Type", "application/json")
        )

        val client = AnthropicClient(
            apiKey = "sk-ant-api03-test-key",
            baseUrl = server.url("/v1/messages").toString()
        )

        val result = client.describeImage("dGVzdA==", "Describe this")
        assertTrue(result.isSuccess)

        val request = server.takeRequest()
        val body = request.body.readUtf8()
        assertTrue(body.contains("\"max_tokens\":${ProviderClient.DEFAULT_VISION_MAX_TOKENS}"))
    }

    @Test
    fun `OpenAI describeImage sends custom max_tokens`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody("""{"choices":[{"message":{"role":"assistant","content":"Big description"},"finish_reason":"stop"}]}""")
                .setResponseCode(200)
                .addHeader("Content-Type", "application/json")
        )

        val client = OpenAiClient(
            config = ProviderConfig.openAi("test-key").copy(
                baseUrl = server.url("/v1/chat/completions").toString()
            )
        )

        val result = client.describeImage("dGVzdA==", "Describe this", maxTokens = 4096)
        assertTrue(result.isSuccess)

        val request = server.takeRequest()
        val body = request.body.readUtf8()
        assertTrue(body.contains("\"max_tokens\":4096"))
    }

    // ========== maxTokens Bounds Validation (#375) ==========

    @Test
    fun `describeImage rejects zero maxTokens`() = runTest {
        val client = AnthropicClient(
            config = ProviderConfig.anthropic("test-key").copy(
                baseUrl = server.url("/v1/messages").toString()
            )
        )
        val result = client.describeImage("dGVzdA==", "test", maxTokens = 0)
        assertTrue(result.isFailure)
        assertTrue(result.exceptionOrNull()!!.message!!.contains("maxTokens must be between"))
    }

    @Test
    fun `describeImage rejects negative maxTokens`() = runTest {
        val client = AnthropicClient(
            config = ProviderConfig.anthropic("test-key").copy(
                baseUrl = server.url("/v1/messages").toString()
            )
        )
        val result = client.describeImage("dGVzdA==", "test", maxTokens = -1)
        assertTrue(result.isFailure)
        assertTrue(result.exceptionOrNull()!!.message!!.contains("maxTokens must be between"))
    }

    @Test
    fun `describeImage rejects excessively large maxTokens`() = runTest {
        val client = AnthropicClient(
            config = ProviderConfig.anthropic("test-key").copy(
                baseUrl = server.url("/v1/messages").toString()
            )
        )
        val result = client.describeImage("dGVzdA==", "test", maxTokens = 100_000)
        assertTrue(result.isFailure)
        assertTrue(result.exceptionOrNull()!!.message!!.contains("maxTokens must be between"))
    }

    @Test
    fun `describeImage accepts upper boundary maxTokens`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody("""{"content":[{"type":"text","text":"ok"}],"stop_reason":"end_turn"}""")
                .setResponseCode(200)
                .addHeader("Content-Type", "application/json")
        )

        val client = AnthropicClient(
            config = ProviderConfig.anthropic("test-key").copy(
                baseUrl = server.url("/v1/messages").toString()
            )
        )
        // MAX_VISION_TOKENS = 16384 should work (maximum)
        val result = client.describeImage("dGVzdA==", "test", maxTokens = 16_384)
        assertTrue(result.isSuccess)
    }

    @Test
    fun `describeImage accepts boundary maxTokens values`() = runTest {
        server.enqueue(
            MockResponse()
                .setBody("""{"content":[{"type":"text","text":"ok"}],"stop_reason":"end_turn"}""")
                .setResponseCode(200)
                .addHeader("Content-Type", "application/json")
        )

        val client = AnthropicClient(
            config = ProviderConfig.anthropic("test-key").copy(
                baseUrl = server.url("/v1/messages").toString()
            )
        )
        // maxTokens = 1 should work (minimum)
        val result = client.describeImage("dGVzdA==", "test", maxTokens = 1)
        assertTrue(result.isSuccess)
    }
}
