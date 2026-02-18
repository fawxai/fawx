package ai.citros.core

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.*
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.util.concurrent.TimeUnit

/**
 * Local LLM client using OpenAI-compatible API (Ollama, llama.cpp, etc.)
 * Connects to localhost by default.
 */
open class LocalLLMClient(
    private val baseUrl: String = "http://localhost:11434",
    private val model: String = "qwen2.5:3b",
    private val systemPrompt: String = ""
) {
    private val client = OkHttpClient.Builder()
        .connectTimeout(10, TimeUnit.SECONDS)
        .readTimeout(60, TimeUnit.SECONDS)
        .writeTimeout(10, TimeUnit.SECONDS)
        .build()

    private val json = Json { ignoreUnknownKeys = true }
    
    private val conversationHistory = mutableListOf<JsonObject>()

    open suspend fun chat(message: String): Result<String> = withContext(Dispatchers.IO) {
        try {
            // Add user message to history
            conversationHistory.add(buildJsonObject {
                put("role", "user")
                put("content", message)
            })
            
            // Build messages array with system prompt + history
            val messages = buildJsonArray {
                addJsonObject {
                    put("role", "system")
                    put("content", systemPrompt)
                }
                conversationHistory.forEach { add(it) }
            }

            val requestBody = buildJsonObject {
                put("model", model)
                putJsonArray("messages") {
                    messages.forEach { add(it) }
                }
                put("stream", false)
                put("temperature", 0.1)
                put("max_tokens", 256)
            }

            val url = "${baseUrl.trimEnd('/')}/v1/chat/completions"
            
            val request = Request.Builder()
                .url(url)
                .addHeader("Content-Type", "application/json")
                .post(requestBody.toString().toRequestBody("application/json".toMediaType()))
                .build()

            val response = client.newCall(request).execute()
            val body = response.body?.string() ?: return@withContext Result.failure(
                Exception("Empty response")
            )

            if (!response.isSuccessful) {
                return@withContext Result.failure(
                    Exception("LLM error ${response.code}: ${body.take(200)}")
                )
            }

            val jsonResponse = json.parseToJsonElement(body).jsonObject
            val content = jsonResponse["choices"]?.jsonArray?.firstOrNull()
                ?.jsonObject?.get("message")?.jsonObject?.get("content")?.jsonPrimitive?.content
                ?: return@withContext Result.failure(Exception("No content in response"))

            // Add assistant response to history
            conversationHistory.add(buildJsonObject {
                put("role", "assistant")
                put("content", content)
            })
            
            // Keep history short for small context windows
            while (conversationHistory.size > 8) {
                conversationHistory.removeAt(0)
            }

            Result.success(content.trim())
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    open fun clearConversation() {
        conversationHistory.clear()
    }
}
