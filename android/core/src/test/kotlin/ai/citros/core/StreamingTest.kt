package ai.citros.core

import kotlinx.serialization.json.*
import org.junit.Assert.*
import org.junit.Test

/**
 * Tests for SSE streaming delta parsing across providers.
 *
 * These tests validate that each provider correctly extracts text deltas
 * from their SSE event format, handles edge cases (empty deltas, malformed
 * JSON, non-text events), and detects stream completion signals.
 */
class StreamingTest {

    // ========== Anthropic SSE Parsing ==========

    private val anthropicClient = AnthropicClient(
        apiKey = "sk-ant-api03-test-key",
        model = "claude-sonnet-4-5-20250929"
    )

    @Test
    fun `anthropic parseSSEDelta extracts text from content_block_delta`() {
        val data = """{
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "text_delta", "text": "Hello"}
        }"""
        assertEquals("Hello", anthropicClient.parseAnthropicSSEDelta(data))
    }

    @Test
    fun `anthropic parseSSEDelta handles multi-token text`() {
        val data = """{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" world, how are you?"}}"""
        assertEquals(" world, how are you?", anthropicClient.parseAnthropicSSEDelta(data))
    }

    @Test
    fun `anthropic parseSSEDelta returns null for message_start`() {
        val data = """{"type":"message_start","message":{"id":"msg_abc","type":"message"}}"""
        assertNull(anthropicClient.parseAnthropicSSEDelta(data))
    }

    @Test
    fun `anthropic parseSSEDelta returns null for content_block_start`() {
        val data = """{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"""
        assertNull(anthropicClient.parseAnthropicSSEDelta(data))
    }

    @Test
    fun `anthropic parseSSEDelta returns null for content_block_stop`() {
        val data = """{"type":"content_block_stop","index":0}"""
        assertNull(anthropicClient.parseAnthropicSSEDelta(data))
    }

    @Test
    fun `anthropic parseSSEDelta returns null for message_delta`() {
        val data = """{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":15}}"""
        assertNull(anthropicClient.parseAnthropicSSEDelta(data))
    }

    @Test
    fun `anthropic parseSSEDelta returns null for input_json_delta (tool use)`() {
        val data = """{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"name\""}}"""
        assertNull(anthropicClient.parseAnthropicSSEDelta(data))
    }

    @Test
    fun `anthropic parseSSEDelta returns null for malformed JSON`() {
        assertNull(anthropicClient.parseAnthropicSSEDelta("not json"))
        assertNull(anthropicClient.parseAnthropicSSEDelta(""))
        assertNull(anthropicClient.parseAnthropicSSEDelta("{"))
    }

    @Test
    fun `anthropic parseSSEDelta handles special characters in text`() {
        val data = """{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"line1\nline2\t\"quoted\""}}"""
        assertEquals("line1\nline2\t\"quoted\"", anthropicClient.parseAnthropicSSEDelta(data))
    }

    @Test
    fun `anthropic isStreamDone detects message_stop`() {
        assertTrue(anthropicClient.isAnthropicStreamDone("""{"type":"message_stop"}"""))
    }

    @Test
    fun `anthropic isStreamDone returns false for other events`() {
        assertFalse(anthropicClient.isAnthropicStreamDone("""{"type":"content_block_delta"}"""))
        assertFalse(anthropicClient.isAnthropicStreamDone("""{"type":"message_start"}"""))
        assertFalse(anthropicClient.isAnthropicStreamDone("""{"type":"message_delta"}"""))
    }

    @Test
    fun `anthropic isStreamDone returns false for malformed JSON`() {
        assertFalse(anthropicClient.isAnthropicStreamDone("not json"))
        assertFalse(anthropicClient.isAnthropicStreamDone(""))
    }

    // ========== OpenAI-Compatible SSE Parsing ==========

    private val openAiClient = OpenAiClient(
        config = ProviderConfig(
            provider = Provider.OPENAI,
            baseUrl = "https://api.openai.com/v1/chat/completions",
            chatModelId = "gpt-4o",
            actionModelId = "gpt-4o-mini",
            headers = mapOf("Authorization" to "Bearer sk-test")
        )
    )

    // Access the internal impl for testing SSE parsing
    private val openAiImpl = OpenAiCompatibleClientImpl(
        config = ProviderConfig(
            provider = Provider.OPENAI,
            baseUrl = "https://api.openai.com/v1/chat/completions",
            chatModelId = "gpt-4o",
            actionModelId = "gpt-4o-mini",
            headers = mapOf("Authorization" to "Bearer sk-test")
        )
    )

    @Test
    fun `openai parseSSEDelta extracts content from delta`() {
        val data = """{"id":"chatcmpl-abc","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}"""
        assertEquals("Hello", openAiImpl.parseOpenAiSSEDelta(data))
    }

    @Test
    fun `openai parseSSEDelta handles multi-token content`() {
        val data = """{"id":"chatcmpl-abc","choices":[{"index":0,"delta":{"content":" world!"},"finish_reason":null}]}"""
        assertEquals(" world!", openAiImpl.parseOpenAiSSEDelta(data))
    }

    @Test
    fun `openai parseSSEDelta returns null for role-only delta`() {
        val data = """{"id":"chatcmpl-abc","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}"""
        assertNull(openAiImpl.parseOpenAiSSEDelta(data))
    }

    @Test
    fun `openai parseSSEDelta returns null for empty delta`() {
        val data = """{"id":"chatcmpl-abc","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"""
        assertNull(openAiImpl.parseOpenAiSSEDelta(data))
    }

    @Test
    fun `openai parseSSEDelta returns null for null content`() {
        val data = """{"id":"chatcmpl-abc","choices":[{"index":0,"delta":{"content":null},"finish_reason":null}]}"""
        assertNull(openAiImpl.parseOpenAiSSEDelta(data))
    }

    @Test
    fun `openai parseSSEDelta returns null for malformed JSON`() {
        assertNull(openAiImpl.parseOpenAiSSEDelta("not json"))
        assertNull(openAiImpl.parseOpenAiSSEDelta(""))
        assertNull(openAiImpl.parseOpenAiSSEDelta("{"))
    }

    @Test
    fun `openai parseSSEDelta handles special characters`() {
        val data = """{"choices":[{"index":0,"delta":{"content":"line1\nline2\t\"hi\""}}]}"""
        assertEquals("line1\nline2\t\"hi\"", openAiImpl.parseOpenAiSSEDelta(data))
    }

    @Test
    fun `openai isStreamDone detects DONE signal`() {
        assertTrue(openAiImpl.isOpenAiStreamDone("[DONE]"))
    }

    @Test
    fun `openai isStreamDone returns false for data`() {
        assertFalse(openAiImpl.isOpenAiStreamDone("""{"choices":[{"delta":{"content":"hi"}}]}"""))
        assertFalse(openAiImpl.isOpenAiStreamDone(""))
        assertFalse(openAiImpl.isOpenAiStreamDone("DONE"))
    }

    // ========== OpenRouter SSE Parsing (same format as OpenAI) ==========

    private val openRouterImpl = OpenAiCompatibleClientImpl(
        config = ProviderConfig(
            provider = Provider.OPENROUTER,
            baseUrl = "https://openrouter.ai/api/v1/chat/completions",
            chatModelId = "anthropic/claude-sonnet-4-5",
            actionModelId = "anthropic/claude-sonnet-4-5",
            headers = mapOf("Authorization" to "Bearer sk-or-test")
        )
    )

    @Test
    fun `openrouter uses same SSE format as openai`() {
        val data = """{"choices":[{"index":0,"delta":{"content":"test"},"finish_reason":null}]}"""
        assertEquals("test", openRouterImpl.parseOpenAiSSEDelta(data))
        assertTrue(openRouterImpl.isOpenAiStreamDone("[DONE]"))
    }

    // ========== Default chatStreaming Fallback ==========

    @Test
    fun `default chatStreaming emits full text as single delta`() {
        // Create a mock provider that returns a fixed response via chat()
        val mockClient = object : ProviderClient {
            override val provider = Provider.ANTHROPIC
            override suspend fun chat(conversation: Conversation) = Result.success("Full response text")
            override suspend fun chatWithTools(
                messages: List<Message>,
                systemPrompt: String?,
                tools: List<Tool>,
                tokenLimit: Int?
            ) = Result.success(ChatResponse("text", emptyList(), "end_turn"))
            override suspend fun describeImage(base64Image: String, prompt: String, maxTokens: Int) =
                Result.success("description")
        }

        val deltas = mutableListOf<String>()
        val result = kotlinx.coroutines.runBlocking {
            mockClient.chatStreaming(Conversation()) { delta -> deltas.add(delta) }
        }

        assertTrue(result.isSuccess)
        assertEquals("Full response text", result.getOrNull())
        // Default fallback emits the entire text as a single delta
        assertEquals(listOf("Full response text"), deltas)
    }

    @Test
    fun `default chatStreaming propagates failure without calling onDelta`() {
        val mockClient = object : ProviderClient {
            override val provider = Provider.ANTHROPIC
            override suspend fun chat(conversation: Conversation) =
                Result.failure<String>(ProviderException(Provider.ANTHROPIC, 500, "Server error", false))
            override suspend fun chatWithTools(
                messages: List<Message>,
                systemPrompt: String?,
                tools: List<Tool>,
                tokenLimit: Int?
            ) = Result.success(ChatResponse(null, emptyList(), null))
            override suspend fun describeImage(base64Image: String, prompt: String, maxTokens: Int) =
                Result.success("description")
        }

        val deltas = mutableListOf<String>()
        val result = kotlinx.coroutines.runBlocking {
            mockClient.chatStreaming(Conversation()) { delta -> deltas.add(delta) }
        }

        assertTrue(result.isFailure)
        // onDelta should NOT be called on failure
        assertTrue(deltas.isEmpty())
    }

    // ========== Simulated SSE Stream Assembly ==========

    @Test
    fun `anthropic SSE stream assembles full text from multiple deltas`() {
        val sseLines = listOf(
            """{"type":"message_start","message":{"id":"msg_1"}}""",
            """{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}""",
            """{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}""",
            """{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" world"}}""",
            """{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"!"}}""",
            """{"type":"content_block_stop","index":0}""",
            """{"type":"message_delta","delta":{"stop_reason":"end_turn"}}""",
            """{"type":"message_stop"}"""
        )

        val fullText = StringBuilder()
        val deltas = mutableListOf<String>()

        for (line in sseLines) {
            if (anthropicClient.isAnthropicStreamDone(line)) break
            val delta = anthropicClient.parseAnthropicSSEDelta(line)
            if (delta != null) {
                fullText.append(delta)
                deltas.add(delta)
            }
        }

        assertEquals("Hello world!", fullText.toString())
        assertEquals(listOf("Hello", " world", "!"), deltas)
    }

    @Test
    fun `openai SSE stream assembles full text from multiple deltas`() {
        val sseLines = listOf(
            """{"choices":[{"delta":{"role":"assistant"},"finish_reason":null}]}""",
            """{"choices":[{"delta":{"content":"Hello"},"finish_reason":null}]}""",
            """{"choices":[{"delta":{"content":" world"},"finish_reason":null}]}""",
            """{"choices":[{"delta":{"content":"!"},"finish_reason":null}]}""",
            """{"choices":[{"delta":{},"finish_reason":"stop"}]}""",
            "[DONE]"
        )

        val fullText = StringBuilder()
        val deltas = mutableListOf<String>()

        for (line in sseLines) {
            if (openAiImpl.isOpenAiStreamDone(line)) break
            val delta = openAiImpl.parseOpenAiSSEDelta(line)
            if (delta != null) {
                fullText.append(delta)
                deltas.add(delta)
            }
        }

        assertEquals("Hello world!", fullText.toString())
        assertEquals(listOf("Hello", " world", "!"), deltas)
    }
}
