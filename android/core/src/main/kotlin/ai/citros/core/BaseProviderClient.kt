package ai.citros.core

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.*
import android.util.Log
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.util.concurrent.TimeUnit

/**
 * Base provider client with common HTTP logic and retry handling.
 *
 * Subclasses implement provider-specific request building and response parsing.
 *
 * ## Retry Policy
 * Retries are performed for transient errors with exponential backoff:
 * - **429 (Rate Limit):** Temporary capacity restrictions that resolve with backoff.
 *   Daily hard caps (detected by error message patterns) are NOT retried.
 * - **529 (Overloaded):** Anthropic-specific transient error indicating server overload.
 * - **503 (Service Unavailable):** Common transient server error across providers.
 *
 * Other errors fail immediately without retry because:
 * - **Auth errors (401/403):** Retrying won't fix invalid credentials.
 * - **Server errors (500/502):** May indicate persistent issues; fail fast for failover.
 * - **Network errors:** May be persistent; immediate failure allows faster failover.
 *
 * @param maxAttempts Maximum retry attempts for transient errors (default: 4)
 */
abstract class BaseProviderClient(
    protected val config: ProviderConfig,
    protected val systemPrompt: String,
    protected val maxTokens: Int,
    protected val maxAttempts: Int
) : ProviderClient {

    override val provider: Provider = config.provider
    override val modelId: String = config.chatModelId

    protected val json = Json {
        ignoreUnknownKeys = true
        encodeDefaults = true
    }

    companion object {
        private const val TAG = "CitrosAPI"

        /**
         * Maximum allowed max_tokens for vision requests.
         * Conservative cross-provider limit: Anthropic supports up to 8192 for most
         * vision models, OpenAI GPT-4o supports 16384. Using 16384 as the upper bound
         * covers all current providers. Provider-specific limits may be added later
         * if models diverge significantly.
         */
        const val MAX_VISION_TOKENS = 16_384

        /** Shared OkHttpClient instance for connection pooling and resource efficiency. */
        internal val sharedClient = OkHttpClient.Builder()
            .connectTimeout(30, TimeUnit.SECONDS)
            .readTimeout(60, TimeUnit.SECONDS)
            .writeTimeout(30, TimeUnit.SECONDS)
            .build()

        const val DEFAULT_SYSTEM_PROMPT = """You are Citros, a friendly AI assistant living on the user's phone. You're helpful, concise, and have a warm personality. Keep responses brief and conversational - you're chatting, not writing essays.

You can help with questions, tasks, and conversations. Be direct and useful."""
    }

    /**
     * Format API error responses into user-friendly messages.
     *
     * Parses provider JSON error bodies to extract meaningful messages instead of
     * dumping raw JSON to the UI. Falls back to a generic message if parsing fails.
     *
     * @param statusCode HTTP status code
     * @param body Raw response body (usually JSON)
     * @return Human-readable error message
     */
    private data class ParsedApiError(
        val message: String?,
        val type: String?,
        val code: String?
    )

    private fun parseApiError(body: String): ParsedApiError {
        return try {
            val jsonBody = json.parseToJsonElement(body).jsonObject
            val errorObj = jsonBody["error"]?.jsonObject
            ParsedApiError(
                message = errorObj?.get("message")?.jsonPrimitive?.content,
                type = errorObj?.get("type")?.jsonPrimitive?.content,
                code = errorObj?.get("code")?.jsonPrimitive?.content
            )
        } catch (_: Exception) {
            ParsedApiError(message = null, type = null, code = null)
        }
    }

    private fun isDailyRateLimit(error: ParsedApiError): Boolean {
        // OpenRouter proxies OpenAI models (e.g., openai/gpt-4o-mini) and returns OpenAI-compatible error formats
        // Anthropic uses per-minute rate limits (RPM/ITPM/OTPM), not daily RPD caps — no daily limit check needed
        if (provider != Provider.OPENAI && provider != Provider.OPENROUTER) return false
        val text = listOfNotNull(error.message, error.code, error.type).joinToString(" ").lowercase()
        val dailyPatterns = listOf("requests per day", "daily limit", "daily quota", "rpd")
        return dailyPatterns.any { pattern -> text.contains(pattern) }
    }

    private fun shouldRetryRateLimit(error: ParsedApiError): Boolean = !isDailyRateLimit(error)

    /**
     * Whether this HTTP status code is a retryable server error.
     * - 529: Anthropic "Overloaded" — explicitly transient
     * - 503: Service Unavailable — common transient error
     */
    private fun isRetryableServerError(code: Int): Boolean =
        code == 529 || code == 503

    internal fun formatApiErrorMessage(statusCode: Int, body: String): String {
        val error = parseApiError(body)
        val errorMessage = error.message

        // OpenAI OAuth scope error (specific case)
        if (
            provider == Provider.OPENAI &&
            statusCode == 401 &&
            errorMessage?.contains("Missing scopes: model.request", ignoreCase = true) == true
        ) {
            return "OpenAI token is missing the model.request permission. " +
                "Use an API key instead, or an OAuth token that includes model.request."
        }

        val providerName = provider.name.lowercase().replaceFirstChar { it.uppercase() }

        return when (statusCode) {
            429 -> {
                when {
                    isDailyRateLimit(error) -> {
                        "Daily request limit reached for this model. Switch models or try again tomorrow."
                    }
                    else -> {
                        val detail = errorMessage ?: "Too many requests"
                        "Rate limited: $detail. Please wait a moment and try again."
                    }
                }
            }
            402 -> "API quota exceeded. Check your billing on the $providerName website."
            401 -> errorMessage ?: "Invalid API key. Check your credentials in Settings."
            403 -> errorMessage ?: "Access denied. Your API key may not have permission for this model."
            404 -> errorMessage ?: "Model not found. It may have been deprecated or the ID is incorrect."
            in 500..599 -> {
                val detail = errorMessage ?: "server error"
                "$providerName is experiencing issues ($detail). Try again in a moment."
            }
            else -> errorMessage ?: "Error $statusCode from $providerName."
        }
    }

    /**
     * Common HTTP logic for sending requests with retry handling.
     *
     * **Retry behavior:** Retries on 429 (rate limit), 529 (overloaded), and 503
     * (service unavailable) up to [maxAttempts] with exponential backoff.
     * All other errors (auth failures, persistent server errors, network issues)
     * fail immediately to allow fast failover. See class documentation for rationale.
     *
     * @param requestBody JSON request body to send
     * @param parseResponse Function to parse successful JSON response into ChatResponse
     * @return Result containing ChatResponse on success, or ProviderException on failure
     */
    protected suspend fun executeRequest(
        requestBody: JsonObject,
        parseResponse: (JsonObject) -> ChatResponse
    ): Result<ChatResponse> = withContext(Dispatchers.IO) {
        val requestStartMs = System.currentTimeMillis()
        Log.d(TAG, "request: provider=$provider, url=${config.baseUrl}, bodySize=${requestBody.toString().length}")
        var attempt = 1
        var lastRetryStatusCode: Int? = null

        while (attempt <= maxAttempts) {
            try {
                val requestBuilder = Request.Builder()
                    .url(config.baseUrl)
                    .addHeader("Content-Type", "application/json")
                    .post(requestBody.toString().toRequestBody("application/json".toMediaType()))

                config.headers.forEach { (key, value) ->
                    requestBuilder.addHeader(key, value)
                }

                val request = requestBuilder.build()
                val response = sharedClient.newCall(request).execute()
                val body = response.body?.string()

                if (body.isNullOrBlank()) {
                    return@withContext Result.failure(
                        ProviderException(
                            provider = provider,
                            statusCode = response.code,
                            message = "API returned empty response body",
                            isAuthFailure = response.code in 401..403
                        )
                    )
                }

                // Handle retryable errors: 429 (rate limit), 529 (overloaded), 503 (service unavailable)
                if (attempt < maxAttempts) {
                    if (response.code == 429) {
                        val parsedError = parseApiError(body)
                        if (shouldRetryRateLimit(parsedError)) {
                            lastRetryStatusCode = 429
                            val retryAfter = response.header("retry-after")?.toLongOrNull()
                                ?: (1L shl attempt) // Exponential backoff: 1s, 2s, 4s
                            attempt++
                            delay(retryAfter * 1000)
                            continue
                        }
                    } else if (isRetryableServerError(response.code)) {
                        lastRetryStatusCode = response.code
                        val retryAfter = response.header("retry-after")?.toLongOrNull()
                            ?: (1L shl attempt) // Exponential backoff: 1s, 2s, 4s
                        Log.w(TAG, "Retryable server error ${response.code}, attempt $attempt/$maxAttempts, retry in ${retryAfter}s")
                        attempt++
                        delay(retryAfter * 1000)
                        continue
                    }
                }

                if (!response.isSuccessful) {
                    return@withContext Result.failure(
                        ProviderException(
                            provider = provider,
                            statusCode = response.code,
                            message = formatApiErrorMessage(response.code, body),
                            isAuthFailure = response.code in 401..403
                        )
                    )
                }

                val jsonResponse = json.parseToJsonElement(body).jsonObject
                val chatResponse = parseResponse(jsonResponse)

                val elapsedMs = System.currentTimeMillis() - requestStartMs
                Log.d(TAG, "response: provider=$provider, ${elapsedMs}ms, stop=${chatResponse.stopReason}, tools=${chatResponse.toolCalls.size}, tokens=${chatResponse.usage?.let { "in=${it.inputTokens},out=${it.outputTokens}" + if (it.cacheReadTokens > 0 || it.cacheWriteTokens > 0) ",cache_r=${it.cacheReadTokens},cache_w=${it.cacheWriteTokens}" else "" } ?: "n/a"}, text=${chatResponse.text?.take(60)}")
                return@withContext Result.success(chatResponse)
            } catch (e: ProviderException) {
                val elapsedMs = System.currentTimeMillis() - requestStartMs
                Log.e(TAG, "request failed: provider=$provider, ${elapsedMs}ms, status=${e.statusCode}, ${e.message}")
                return@withContext Result.failure(e)
            } catch (e: Exception) {
                val elapsedMs = System.currentTimeMillis() - requestStartMs
                Log.e(TAG, "request error: provider=$provider, ${elapsedMs}ms, ${e.javaClass.simpleName}: ${e.message}")
                // For non-HTTP exceptions (network errors, timeouts), fail immediately without retry
                return@withContext Result.failure(
                    ProviderException(
                        provider = provider,
                        statusCode = null,
                        message = e.message ?: "Unknown error",
                        isAuthFailure = false,
                        cause = e
                    )
                )
            }
        }

        // Exhausted all retry attempts (429/529/503)
        return@withContext Result.failure(
            ProviderException(
                provider = provider,
                statusCode = lastRetryStatusCode,
                message = "Request failed after $maxAttempts attempts. The ${config.provider.name.lowercase().replaceFirstChar { it.uppercase() }} API may be overloaded or rate-limited. Try again shortly.",
                isAuthFailure = false
            )
        )
    }

    /**
     * Execute a streaming HTTP request using Server-Sent Events (SSE).
     *
     * Sends the request with `"stream": true` added to the body, then reads
     * the SSE response line-by-line, parsing text deltas and forwarding them
     * to [onDelta]. Returns the fully assembled text on success.
     *
     * No retry logic for streaming requests — failures are immediate.
     * Rate limit (429) retry is only supported in the non-streaming [executeRequest].
     *
     * @param requestBody JSON request body (stream:true is added automatically)
     * @param parseSSEDelta Parse a `data:` line and return a text delta, or null if not a text event
     * @param isDone Return true if the `data:` line signals stream completion
     * @param onDelta Called with each text fragment for real-time UI updates
     * @return Complete assembled text, or failure
     */
    protected suspend fun executeStreamingRequest(
        requestBody: JsonObject,
        parseSSEDelta: (String) -> String?,
        isDone: (String) -> Boolean,
        onDelta: (String) -> Unit
    ): Result<String> = withContext(Dispatchers.IO) {
        val requestStartMs = System.currentTimeMillis()

        // Add stream: true to request body
        val streamingBody = JsonObject(
            requestBody.toMap().toMutableMap().apply {
                put("stream", JsonPrimitive(true))
            }
        )

        Log.d(TAG, "stream request: provider=$provider, url=${config.baseUrl}")

        try {
            val requestBuilder = Request.Builder()
                .url(config.baseUrl)
                .addHeader("Content-Type", "application/json")
                .addHeader("Accept", "text/event-stream")
                .post(streamingBody.toString().toRequestBody("application/json".toMediaType()))

            config.headers.forEach { (key, value) ->
                requestBuilder.addHeader(key, value)
            }

            val request = requestBuilder.build()

            // Use a separate client with extended read timeout for SSE connections
            val streamingClient = sharedClient.newBuilder()
                .readTimeout(5, TimeUnit.MINUTES)
                .build()

            val response = streamingClient.newCall(request).execute()

            // Use response.use{} to ensure the body is always closed,
            // preventing socket leaks on the 5-min SSE timeout.
            return@withContext response.use { resp ->
                if (!resp.isSuccessful) {
                    val body = resp.body?.string() ?: ""
                    val elapsedMs = System.currentTimeMillis() - requestStartMs
                    Log.e(TAG, "stream request failed: provider=$provider, ${elapsedMs}ms, status=${resp.code}")
                    return@use Result.failure(
                        ProviderException(
                            provider = provider,
                            statusCode = resp.code,
                            message = formatApiErrorMessage(resp.code, body),
                            isAuthFailure = resp.code in 401..403
                        )
                    )
                }

                val reader = resp.body?.charStream()?.let { java.io.BufferedReader(it) }
                if (reader == null) {
                    return@use Result.failure(
                        ProviderException(
                            provider = provider,
                            statusCode = resp.code,
                            message = "Streaming response has no body",
                            isAuthFailure = false
                        )
                    )
                }

                val fullText = StringBuilder()

                reader.use { r ->
                    r.lineSequence().forEach { line ->
                        val trimmed = line.trim()
                        if (trimmed.startsWith("data:")) {
                            // SSE spec: "data:" followed by optional space then payload
                            val data = trimmed.removePrefix("data:").trimStart()
                            if (data.isEmpty()) return@forEach

                            if (isDone(data)) return@forEach

                            val delta = parseSSEDelta(data)
                            if (delta != null && delta.isNotEmpty()) {
                                fullText.append(delta)
                                onDelta(delta)
                            }
                        }
                        // Skip non-data lines (event:, id:, retry:, comments, blank lines)
                    }
                }

                val elapsedMs = System.currentTimeMillis() - requestStartMs
                val result = fullText.toString()
                Log.d(TAG, "stream response: provider=$provider, ${elapsedMs}ms, chars=${result.length}, text=${result.take(60)}")
                Result.success(result)
            }
        } catch (e: ProviderException) {
            val elapsedMs = System.currentTimeMillis() - requestStartMs
            Log.e(TAG, "stream failed: provider=$provider, ${elapsedMs}ms, status=${e.statusCode}, ${e.message}")
            return@withContext Result.failure(e)
        } catch (e: Exception) {
            val elapsedMs = System.currentTimeMillis() - requestStartMs
            Log.e(TAG, "stream error: provider=$provider, ${elapsedMs}ms, ${e.javaClass.simpleName}: ${e.message}")
            return@withContext Result.failure(
                ProviderException(
                    provider = provider,
                    statusCode = null,
                    message = e.message ?: "Stream error",
                    isAuthFailure = false,
                    cause = e
                )
            )
        }
    }

    /**
     * Parse token usage from an API response JSON object.
     *
     * Handles both Anthropic and OpenAI usage formats:
     * - Anthropic: `input_tokens`, `output_tokens`, `cache_read_input_tokens`, `cache_creation_input_tokens`
     * - OpenAI/OpenRouter: `prompt_tokens`, `completion_tokens`
     *
     * @return [TokenUsage] if usage data is present, null otherwise
     */
    protected fun parseUsage(jsonResponse: JsonObject): TokenUsage? {
        val usage = jsonResponse["usage"]?.jsonObject ?: return null
        // Anthropic format
        val inputTokens = usage["input_tokens"]?.jsonPrimitive?.intOrNull
        if (inputTokens != null) {
            return TokenUsage(
                inputTokens = inputTokens,
                outputTokens = usage["output_tokens"]?.jsonPrimitive?.intOrNull ?: 0,
                cacheReadTokens = usage["cache_read_input_tokens"]?.jsonPrimitive?.intOrNull ?: 0,
                cacheWriteTokens = usage["cache_creation_input_tokens"]?.jsonPrimitive?.intOrNull ?: 0
            )
        }
        // OpenAI/OpenRouter format
        val promptTokens = usage["prompt_tokens"]?.jsonPrimitive?.intOrNull
        if (promptTokens != null) {
            return TokenUsage(
                inputTokens = promptTokens,
                outputTokens = usage["completion_tokens"]?.jsonPrimitive?.intOrNull ?: 0
            )
        }
        return null
    }

    /**
     * Convert Any to JsonElement using JsonUtils.
     * Delegates to JsonUtils.anyToJsonElement() for consistent serialization.
     */
    protected fun anyToJsonElement(value: Any?): JsonElement = JsonUtils.anyToJsonElement(value)

    /**
     * Convert a JsonObject to a Map, preserving JSON types.
     *
     * - JSON strings stay as Kotlin Strings
     * - JSON numbers stay as their natural Kotlin type (Int, Long, or Double)
     * - JSON booleans stay as Kotlin Boolean
     * - JSON null values are filtered out (keys with null values are omitted)
     * - Nested objects/arrays are converted to their Kotlin equivalents
     */
    protected fun parseJsonObjectToMap(jsonObject: JsonObject): Map<String, Any> {
        val result = mutableMapOf<String, Any>()
        jsonObject.forEach { (key, value) ->
            parseJsonElement(value)?.let { result[key] = it }
        }
        return result
    }

    /**
     * Parse a JsonElement to its natural Kotlin type.
     */
    protected fun parseJsonElement(element: JsonElement): Any? {
        return when (element) {
            is JsonNull -> null
            is JsonPrimitive -> {
                when {
                    element.isString -> element.content
                    element.content == "true" -> true
                    element.content == "false" -> false
                    else -> {
                        // For numbers, prefer Int if it fits, otherwise Long, otherwise Double
                        element.content.toIntOrNull()
                            ?: element.content.toLongOrNull()
                            ?: element.content.toDoubleOrNull()
                            ?: element.content
                    }
                }
            }
            is JsonArray -> element.map { parseJsonElement(it) }
            is JsonObject -> parseJsonObjectToMap(element)
        }
    }

    /**
     * Default describeImage implementation using Anthropic/OpenAI vision format.
     * Subclasses can override for provider-specific image handling.
     */
    override suspend fun describeImage(base64Image: String, prompt: String, maxTokens: Int): Result<String> {
        if (maxTokens !in 1..MAX_VISION_TOKENS) {
            return Result.failure(
                IllegalArgumentException("maxTokens must be between 1 and $MAX_VISION_TOKENS, got $maxTokens")
            )
        }
        val requestBody = buildVisionRequest(base64Image, prompt, maxTokens)
        return executeRequest(requestBody) { jsonResponse ->
            val text = parseChatResponse(jsonResponse)
            ChatResponse(text = text, toolCalls = emptyList(), stopReason = "end_turn", usage = parseUsage(jsonResponse))
        }.map { it.text ?: "No description returned" }
    }

    /**
     * Build a vision request with image content. Default implementation uses Anthropic format.
     * Subclasses should override for provider-specific formats.
     */
    protected open fun buildVisionRequest(base64Image: String, prompt: String, maxTokens: Int = ProviderClient.DEFAULT_VISION_MAX_TOKENS): JsonObject {
        return buildJsonObject {
            put("model", config.chatModelId)
            put("max_tokens", maxTokens)
            putJsonArray("messages") {
                addJsonObject {
                    put("role", "user")
                    putJsonArray("content") {
                        addJsonObject {
                            put("type", "image")
                            putJsonObject("source") {
                                put("type", "base64")
                                put("media_type", "image/png")
                                put("data", base64Image)
                            }
                        }
                        addJsonObject {
                            put("type", "text")
                            put("text", prompt)
                        }
                    }
                }
            }
        }
    }

    /**
     * Shared text-only chat execution for providers whose chat endpoints return
     * plain assistant text plus optional usage.
     */
    protected suspend fun executeTextChat(conversation: Conversation): Result<ChatResponse> {
        return executeRequest(
            requestBody = buildChatRequest(conversation),
            parseResponse = { jsonResponse ->
                val text = parseChatResponse(jsonResponse)
                    ?: throw ProviderException(
                        provider = provider,
                        statusCode = null,
                        message = "API returned no text content in response",
                        isAuthFailure = false
                    )
                ChatResponse(text, emptyList(), null, parseUsage(jsonResponse))
            }
        )
    }

    // Abstract methods that subclasses must implement
    protected abstract fun buildChatRequest(conversation: Conversation): JsonObject
    internal abstract fun buildToolRequest(
        messages: List<Message>,
        systemPrompt: String,
        tools: List<Tool>,
        maxTokens: Int
    ): JsonObject
    protected abstract fun parseChatResponse(jsonResponse: JsonObject): String?
    internal abstract fun parseToolResponse(jsonResponse: JsonObject): ChatResponse
}
