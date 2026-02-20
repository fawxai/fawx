package ai.citros.core

import android.util.Log
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.*
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.util.concurrent.TimeUnit

/**
 * Fetches API keys from the Citros key delivery endpoint.
 *
 * Keys live server-side (Vercel env vars) and are delivered to the app at
 * startup. This avoids embedding third-party API keys in the APK while
 * keeping direct connections (e.g., TinyFish SSE streaming) fast.
 *
 * The endpoint requires Bearer auth with the compiled app token. This
 * prevents unauthenticated access to server-side keys.
 *
 * @param endpoint Key delivery URL (default: production citros.ai).
 *   Note: if overridden (e.g., for testing), the [appToken] is still sent
 *   as a Bearer header. Use `appToken = null` for non-citros endpoints.
 * @param appToken Bearer token for authentication (compiled into APK)
 */
class KeyDeliveryClient(
    private val endpoint: String = DEFAULT_ENDPOINT,
    private val appToken: String? = null
) {
    companion object {
        private const val TAG = "KeyDeliveryClient"
        private const val DEFAULT_ENDPOINT = "https://citros.ai/api/keys"
        private const val TIMEOUT_SECONDS = 5L

        /** Shared HTTP client — reuses connection pool across instances. */
        private val sharedHttpClient = OkHttpClient.Builder()
            .connectTimeout(TIMEOUT_SECONDS, TimeUnit.SECONDS)
            .readTimeout(TIMEOUT_SECONDS, TimeUnit.SECONDS)
            .build()
    }

    /**
     * Fetched keys from the delivery endpoint.
     */
    data class DeliveredKeys(
        val tinyfish: String? = null,
        val appToken: String? = null
    )

    private val json = Json { ignoreUnknownKeys = true }

    /**
     * Fetch keys from the server. Returns null on any failure (network, auth, parse).
     * Failures are silent — the app falls back to user-provided keys in Settings.
     */
    suspend fun fetchKeys(): DeliveredKeys? {
        return withContext(Dispatchers.IO) {
            try {
                val requestBuilder = Request.Builder()
                    .url(endpoint)
                    .post("{}".toRequestBody("application/json".toMediaType()))
                    .header("Accept", "application/json")
                    .header("X-Citros-Client", "android/${android.os.Build.VERSION.SDK_INT}")

                if (appToken != null) {
                    requestBuilder.header("Authorization", "Bearer $appToken")
                }

                sharedHttpClient.newCall(requestBuilder.build()).execute().use { response ->
                    if (!response.isSuccessful) {
                        Log.w(TAG, "Key delivery failed (${response.code})")
                        return@withContext null
                    }

                    val body = response.body?.string() ?: return@withContext null
                    val root = json.parseToJsonElement(body).jsonObject
                    val keys = root["keys"]?.jsonObject ?: return@withContext null

                    DeliveredKeys(
                        tinyfish = keys["tinyfish"]?.jsonPrimitive?.contentOrNull,
                        appToken = keys["appToken"]?.jsonPrimitive?.contentOrNull
                    )
                }
            } catch (e: Exception) {
                Log.w(TAG, "Key delivery error: ${e.message}")
                null
            }
        }
    }
}
