package ai.citros.core

import android.util.Log
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.isActive
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.*
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.io.BufferedReader
import java.util.concurrent.TimeUnit

/**
 * Client for TinyFish Web Agent API.
 *
 * Executes browser automation tasks via TinyFish's cloud-hosted web agent.
 * Sends a URL + natural language goal, receives structured results via SSE streaming.
 *
 * Use cases: price comparison, data extraction, form filling, multi-site research —
 * tasks that require navigating real websites with dynamic content, authentication walls,
 * or bot protection.
 *
 * @param apiKey TinyFish API key
 * @param endpoint API endpoint (override for testing)
 */
interface TinyFishBrowserClient {
    suspend fun browse(
        url: String,
        goal: String,
        stealth: Boolean = false,
        proxyCountry: String? = null,
        onProgress: ((String) -> Unit)? = null
    ): ToolResult
}

class TinyFishClient(
    private val apiKey: String,
    private val endpoint: String = DEFAULT_ENDPOINT
) : TinyFishBrowserClient {
    companion object {
        private const val TAG = "TinyFishClient"
        internal const val DEFAULT_ENDPOINT = "https://agent.tinyfish.ai/v1/automation/run-sse"
        private const val CONNECT_TIMEOUT_SECONDS = 15L
        private const val READ_TIMEOUT_SECONDS = 180L
        internal const val MAX_RESULT_CHARS = 10000
        private const val BROWSER_PROFILE_LITE = "lite"
        private const val BROWSER_PROFILE_STEALTH = "stealth"
        private val prettyJson = Json { prettyPrint = true }
    }

    private val httpClient = OkHttpClient.Builder()
        .connectTimeout(CONNECT_TIMEOUT_SECONDS, TimeUnit.SECONDS)
        .readTimeout(READ_TIMEOUT_SECONDS, TimeUnit.SECONDS)
        .build()

    private val json = Json { ignoreUnknownKeys = true }

    /**
     * Execute a browser automation task.
     *
     * Blocks until the TinyFish agent completes the task (typically 10-60 seconds).
     * Progress events are reported via [onProgress] callback for UI updates.
     *
     * @param url Target website URL
     * @param goal Natural language description of the task
     * @param stealth Use anti-detection browser for bot-protected sites
     * @param proxyCountry Optional proxy country code (US, GB, CA, DE, FR, JP, AU)
     * @param onProgress Optional callback for progress events (step descriptions)
     * @return ToolResult with structured result or error
     */
    override suspend fun browse(
        url: String,
        goal: String,
        stealth: Boolean,
        proxyCountry: String?,
        onProgress: ((String) -> Unit)?
    ): ToolResult {
        if (url.isBlank()) return ToolResult("URL cannot be empty", isError = true)
        if (goal.isBlank()) return ToolResult("Goal cannot be empty", isError = true)
        if (!url.startsWith("http://") && !url.startsWith("https://")) {
            return ToolResult("Invalid URL: must start with http:// or https://", isError = true)
        }

        return withContext(Dispatchers.IO) {
            try {
                val requestBody = buildJsonObject {
                    put("url", url)
                    put("goal", goal)
                    put("browser_profile", if (stealth) BROWSER_PROFILE_STEALTH else BROWSER_PROFILE_LITE)
                    putJsonObject("proxy_config") {
                        put("enabled", proxyCountry != null)
                        if (proxyCountry != null) put("country_code", proxyCountry)
                    }
                }.toString()

                val request = Request.Builder()
                    .url(endpoint)
                    .header("X-API-Key", apiKey)
                    .header("Accept", "text/event-stream")
                    .post(requestBody.toRequestBody("application/json".toMediaType()))
                    .build()

                httpClient.newCall(request).execute().use { response ->
                    if (!response.isSuccessful) {
                        val errorBody = response.body?.string()?.take(500) ?: response.message
                        return@withContext ToolResult(
                            "TinyFish request failed (${response.code}): $errorBody",
                            isError = true
                        )
                    }

                    val body = response.body ?: return@withContext ToolResult(
                        "TinyFish returned empty response",
                        isError = true
                    )

                    parseSSEStream(body.byteStream().bufferedReader(), onProgress)
                }
            } catch (e: java.net.SocketTimeoutException) {
                Log.e(TAG, "TinyFish timeout: ${e.message}")
                ToolResult(
                    "TinyFish automation timed out. The task may be too complex — try breaking it into smaller steps.",
                    isError = true
                )
            } catch (e: Exception) {
                Log.e(TAG, "TinyFish browse error: ${e.message}")
                ToolResult("TinyFish browse failed: ${e.message?.take(100)}", isError = true)
            }
        }
    }

    /**
     * Parse SSE event stream and extract the final result.
     *
     * TinyFish SSE events:
     * - STARTED: automation begun (logged)
     * - PROGRESS: intermediate step description (reported via callback)
     * - STREAMING_URL: live browser stream URL (logged)
     * - COMPLETE: final result with status and resultJson
     * - HEARTBEAT: keepalive (ignored)
     */
    internal suspend fun parseSSEStream(
        reader: BufferedReader,
        onProgress: ((String) -> Unit)? = null
    ): ToolResult {
        var result: ToolResult? = null
        val progressSteps = mutableListOf<String>()

        try {
            reader.useLines { lines ->
                for (line in lines) {
                    // Check coroutine cancellation to release IO thread on user cancel
                    if (!currentCoroutineContext().isActive) break
                    if (!line.startsWith("data: ")) continue
                    val data = line.removePrefix("data: ").trim()
                    if (data.isEmpty()) continue

                    try {
                        val event = json.parseToJsonElement(data).jsonObject
                        val type = event["type"]?.jsonPrimitive?.contentOrNull ?: continue

                        when (type) {
                            "STARTED" -> {
                                val runId = event["runId"]?.jsonPrimitive?.contentOrNull
                                Log.d(TAG, "Automation started: runId=$runId")
                            }
                            "PROGRESS" -> {
                                val purpose = event["purpose"]?.jsonPrimitive?.contentOrNull ?: ""
                                if (purpose.isNotBlank()) {
                                    progressSteps.add(purpose)
                                    onProgress?.invoke(purpose)
                                    Log.d(TAG, "Progress: $purpose")
                                }
                            }
                            "STREAMING_URL" -> {
                                val streamUrl = event["streamingUrl"]?.jsonPrimitive?.contentOrNull
                                Log.d(TAG, "Streaming URL: $streamUrl")
                            }
                            "COMPLETE" -> {
                                result = parseCompleteEvent(event, progressSteps)
                            }
                            "HEARTBEAT" -> { /* keepalive, ignore */ }
                        }
                    } catch (e: Exception) {
                        Log.w(TAG, "Failed to parse SSE event: $data — ${e.message}")
                    }
                }
            }
        } catch (e: Exception) {
            Log.e(TAG, "SSE stream error: ${e.message}")
            return ToolResult("TinyFish stream error: ${e.message?.take(100)}", isError = true)
        }

        return result ?: ToolResult("TinyFish stream ended without completing", isError = true)
    }

    /**
     * Parse a COMPLETE event into a formatted ToolResult.
     */
    private fun parseCompleteEvent(
        event: JsonObject,
        progressSteps: List<String>
    ): ToolResult {
        val status = event["status"]?.jsonPrimitive?.contentOrNull ?: "UNKNOWN"

        return when (status) {
            "COMPLETED" -> {
                val resultJson = event["resultJson"]
                val formatted = formatResult(resultJson, progressSteps)
                ToolResult(formatted.take(MAX_RESULT_CHARS))
            }
            "FAILED" -> {
                val error = event["error"]?.jsonPrimitive?.contentOrNull ?: "Unknown error"
                ToolResult("Web automation failed: $error", isError = true)
            }
            "CANCELLED" -> {
                ToolResult("Web automation was cancelled", isError = true)
            }
            else -> {
                ToolResult("Web automation ended with status: $status", isError = true)
            }
        }
    }

    /**
     * Format the result JSON into readable text for the agent.
     *
     * Produces structured text the LLM can reason about.
     * Falls back to pretty-printed JSON for unstructured results.
     */
    internal fun formatResult(resultJson: JsonElement?, progressSteps: List<String>): String {
        return buildString {
            appendLine("Web automation completed.")

            if (progressSteps.isNotEmpty()) {
                appendLine()
                appendLine("Steps taken:")
                for (step in progressSteps) {
                    appendLine("  - $step")
                }
            }

            if (resultJson != null && resultJson !is JsonNull) {
                appendLine()
                appendLine("Result:")
                appendLine(prettyJson.encodeToString(JsonElement.serializer(), resultJson))
            }
        }.trimEnd()
    }
}
