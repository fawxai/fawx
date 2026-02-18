package ai.citros.core

import kotlinx.serialization.json.*
import org.junit.Assert.*
import org.junit.Test

/**
 * Tests for [TokenUsage] data class and usage parsing in provider clients.
 */
class TokenUsageTest {

    // ====== TokenUsage data class ======

    @Test
    fun `totalTokens sums input and output`() {
        val usage = TokenUsage(inputTokens = 100, outputTokens = 50)
        assertEquals(150, usage.totalTokens)
    }

    @Test
    fun `cache tokens default to zero`() {
        val usage = TokenUsage(inputTokens = 100, outputTokens = 50)
        assertEquals(0, usage.cacheReadTokens)
        assertEquals(0, usage.cacheWriteTokens)
    }

    @Test
    fun `cache tokens are stored when provided`() {
        val usage = TokenUsage(
            inputTokens = 100,
            outputTokens = 50,
            cacheReadTokens = 80,
            cacheWriteTokens = 20
        )
        assertEquals(80, usage.cacheReadTokens)
        assertEquals(20, usage.cacheWriteTokens)
    }

    // ====== ChatResponse with usage ======

    @Test
    fun `ChatResponse usage defaults to null`() {
        val response = ChatResponse(text = "hello", toolCalls = emptyList(), stopReason = "end_turn")
        assertNull(response.usage)
    }

    @Test
    fun `ChatResponse carries usage when provided`() {
        val usage = TokenUsage(inputTokens = 200, outputTokens = 100)
        val response = ChatResponse(
            text = "hello",
            toolCalls = emptyList(),
            stopReason = "end_turn",
            usage = usage
        )
        assertNotNull(response.usage)
        assertEquals(200, response.usage!!.inputTokens)
        assertEquals(100, response.usage!!.outputTokens)
    }

    // ====== Anthropic usage parsing via buildToolRequest round-trip ======
    // parseToolResponse is internal, accessible from test code in same module

    @Test
    fun `Anthropic response with usage is parsed`() {
        val client = createAnthropicClient()
        val json = buildJsonObject {
            putJsonArray("content") {
                addJsonObject {
                    put("type", "text")
                    put("text", "Hello")
                }
            }
            put("stop_reason", "end_turn")
            putJsonObject("usage") {
                put("input_tokens", 150)
                put("output_tokens", 42)
                put("cache_read_input_tokens", 120)
                put("cache_creation_input_tokens", 30)
            }
        }

        val response = client.parseToolResponse(json)
        assertNotNull(response.usage)
        assertEquals(150, response.usage!!.inputTokens)
        assertEquals(42, response.usage!!.outputTokens)
        assertEquals(120, response.usage!!.cacheReadTokens)
        assertEquals(30, response.usage!!.cacheWriteTokens)
    }

    @Test
    fun `Anthropic response without usage returns null`() {
        val client = createAnthropicClient()
        val json = buildJsonObject {
            putJsonArray("content") {
                addJsonObject {
                    put("type", "text")
                    put("text", "Hello")
                }
            }
            put("stop_reason", "end_turn")
        }

        val response = client.parseToolResponse(json)
        assertNull(response.usage)
    }

    @Test
    fun `Anthropic response with partial cache tokens defaults to zero`() {
        val client = createAnthropicClient()
        val json = buildJsonObject {
            putJsonArray("content") {
                addJsonObject {
                    put("type", "text")
                    put("text", "Hello")
                }
            }
            put("stop_reason", "end_turn")
            putJsonObject("usage") {
                put("input_tokens", 100)
                put("output_tokens", 25)
            }
        }

        val response = client.parseToolResponse(json)
        assertNotNull(response.usage)
        assertEquals(0, response.usage!!.cacheReadTokens)
        assertEquals(0, response.usage!!.cacheWriteTokens)
    }

    // ====== OpenAI usage parsing ======

    @Test
    fun `OpenAI response with usage is parsed`() {
        val client = createOpenAiClient()
        val json = buildJsonObject {
            putJsonArray("choices") {
                addJsonObject {
                    putJsonObject("message") {
                        put("role", "assistant")
                        put("content", "Hello")
                    }
                    put("finish_reason", "stop")
                }
            }
            putJsonObject("usage") {
                put("prompt_tokens", 200)
                put("completion_tokens", 80)
                put("total_tokens", 280)
            }
        }

        val response = client.parseToolResponse(json)
        assertNotNull(response.usage)
        assertEquals(200, response.usage!!.inputTokens)
        assertEquals(80, response.usage!!.outputTokens)
        assertEquals(0, response.usage!!.cacheReadTokens)
    }

    @Test
    fun `OpenAI response without usage returns null`() {
        val client = createOpenAiClient()
        val json = buildJsonObject {
            putJsonArray("choices") {
                addJsonObject {
                    putJsonObject("message") {
                        put("role", "assistant")
                        put("content", "Hello")
                    }
                    put("finish_reason", "stop")
                }
            }
        }

        val response = client.parseToolResponse(json)
        assertNull(response.usage)
    }

    // ====== Helpers ======

    private fun createAnthropicClient() = AnthropicClient(
        config = ProviderConfig(
            provider = Provider.ANTHROPIC,
            baseUrl = "https://api.anthropic.com/v1/messages",
            chatModelId = "claude-sonnet-4-20250514",
            actionModelId = "claude-sonnet-4-20250514",
            headers = mapOf(
                "x-api-key" to "sk-ant-api03-test",
                "anthropic-version" to ProviderConfig.ANTHROPIC_API_VERSION,
                "anthropic-beta" to ProviderConfig.ANTHROPIC_PROMPT_CACHING_BETA
            )
        )
    )

    private fun createOpenAiClient() = OpenAiCompatibleClientImpl(
        config = ProviderConfig(
            provider = Provider.OPENAI,
            baseUrl = "https://api.openai.com/v1/chat/completions",
            chatModelId = "gpt-4o",
            actionModelId = "gpt-4o",
            headers = mapOf("Authorization" to "Bearer test-key")
        )
    )
}
