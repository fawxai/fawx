package ai.citros.core

import kotlinx.serialization.json.JsonObject
import kotlin.test.Test
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class ProviderExceptionTest {

    @Test
    fun `isAuthFailure returns true for auth-related Result failures`() {
        val result = Result.failure<String>(
            ProviderException(
                provider = Provider.OPENAI,
                statusCode = 401,
                message = "Invalid API key",
                isAuthFailure = true
            )
        )

        assertTrue(ProviderException.isAuthFailure(result))
    }

    @Test
    fun `isAuthFailure returns false for non-auth Result failures`() {
        val result = Result.failure<String>(
            ProviderException(
                provider = Provider.ANTHROPIC,
                statusCode = 500,
                message = "Internal server error",
                isAuthFailure = false
            )
        )

        assertFalse(ProviderException.isAuthFailure(result))
    }

    @Test
    fun `isAuthFailure returns false for success Result`() {
        val result = Result.success("test")

        assertFalse(ProviderException.isAuthFailure(result))
    }

    @Test
    fun `base provider parseJsonObjectToMap filters out null values`() {
        val client = object : BaseProviderClient(
            config = ProviderConfig.anthropic("sk-ant-api03-test-key"),
            systemPrompt = "test",
            maxTokens = 10,
            maxAttempts = 1
        ) {
            override fun buildChatRequest(conversation: Conversation): JsonObject = JsonObject(emptyMap())
            override fun buildToolRequest(
                messages: List<Message>,
                systemPrompt: String,
                tools: List<Tool>,
                maxTokens: Int
            ): JsonObject = JsonObject(emptyMap())
            override fun parseChatResponse(jsonResponse: JsonObject): String? = null
            override fun parseToolResponse(jsonResponse: JsonObject): ChatResponse = ChatResponse(null, emptyList(), null)
            override suspend fun chat(conversation: Conversation): Result<String> = Result.success("")
            override suspend fun chatWithTools(
                messages: List<Message>,
                systemPrompt: String?,
                tools: List<Tool>,
                tokenLimit: Int?
            ): Result<ChatResponse> = Result.success(ChatResponse(null, emptyList(), null))

            fun parse(jsonObject: JsonObject): Map<String, Any> = parseJsonObjectToMap(jsonObject)
        }

        val json = JsonObject(
            mapOf(
                "a" to kotlinx.serialization.json.JsonPrimitive(1),
                "b" to kotlinx.serialization.json.JsonNull,
                "c" to kotlinx.serialization.json.JsonPrimitive("x")
            )
        )

        val result = client.parse(json)

        assertTrue(result.containsKey("a"))
        assertTrue(result.containsKey("c"))
        assertFalse(result.containsKey("b"))
    }
}
