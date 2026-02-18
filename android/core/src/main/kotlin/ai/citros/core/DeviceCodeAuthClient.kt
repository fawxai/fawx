package ai.citros.core

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.SerializationException
import kotlinx.serialization.json.Json
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.io.IOException

/**
 * Device code authentication client for OpenAI/Codex.
 *
 * Uses OpenAI's custom device auth protocol (not standard RFC 8628).
 * The flow:
 * 1. Request a user code from /api/accounts/deviceauth/usercode
 * 2. User visits auth.openai.com/codex/device and enters the code
 * 3. App polls /api/accounts/deviceauth/token until authorized
 * 4. On success, exchange the authorization code for OAuth tokens
 *
 * ## Thread Safety
 * This class is thread-safe for concurrent method calls. The shared OkHttpClient
 * is thread-safe, and all state is passed via parameters. However, calling
 * `pollForToken()` multiple times concurrently for the same device code is
 * not recommended and may cause issues with the OAuth server.
 *
 * Example usage:
 * ```kotlin
 * val client = DeviceCodeAuthClient()
 * val deviceCode = client.requestDeviceCode(
 *     DeviceCodeAuthClient.DEFAULT_CLIENT_ID
 * ).getOrThrow()
 *
 * println("Visit: ${deviceCode.verificationUri}")
 * println("Enter code: ${deviceCode.userCode}")
 *
 * when (val result = client.pollForToken(
 *     deviceCode.deviceAuthId,
 *     deviceCode.userCode,
 *     deviceCode.interval
 * )) {
 *     is PollResult.Success -> {
 *         // Exchange authorization code for tokens
 *         val tokens = client.exchangeCode(
 *             DeviceCodeAuthClient.DEFAULT_CLIENT_ID,
 *             result.authCode,
 *             result.codeVerifier,
 *             result.codeChallenge
 *         ).getOrThrow()
 *         println("Token: ${tokens.accessToken}")
 *     }
 *     is PollResult.Error -> println("Error: ${result.error}")
 * }
 * ```
 */
class DeviceCodeAuthClient(
    private val httpClient: OkHttpClient = createDefaultHttpClient(),
    private val authBaseUrl: String = DEFAULT_AUTH_BASE_URL,
    private val maxWaitSeconds: Long = MAX_WAIT_SECONDS
) {
    companion object {
        /**
         * Default OpenAI OAuth client ID for Codex.
         *
         * **Security Note:** This is a PUBLIC OAuth client ID used by the Codex CLI.
         * Device code flow is designed for public clients that cannot securely store
         * client secrets. This client ID is safe to embed in source code.
         */
        const val DEFAULT_CLIENT_ID = "app_EMoamEEZ73f0CkXaXp7hrann"

        /**
         * Default OpenAI auth base URL.
         */
        const val DEFAULT_AUTH_BASE_URL = "https://auth.openai.com"

        /**
         * Default verification URL where users enter their code.
         */
        const val DEFAULT_VERIFICATION_URL = "https://auth.openai.com/codex/device"

        /**
         * Default polling interval in seconds.
         */
        const val DEFAULT_POLL_INTERVAL_SECONDS = 5L

        /**
         * Maximum wait time for device auth (15 minutes, matching Codex CLI).
         */
        const val MAX_WAIT_SECONDS = 900L

        /**
         * Default HTTP timeout (10 seconds).
         */
        private const val DEFAULT_TIMEOUT_MS = 10_000L
        private const val RESPONSE_PREVIEW_LIMIT = 160

        private val JSON_MEDIA_TYPE = "application/json".toMediaType()

        /**
         * Create an OkHttpClient with sensible default timeouts.
         */
        private fun createDefaultHttpClient(): OkHttpClient {
            return OkHttpClient.Builder()
                .connectTimeout(DEFAULT_TIMEOUT_MS, java.util.concurrent.TimeUnit.MILLISECONDS)
                .readTimeout(DEFAULT_TIMEOUT_MS, java.util.concurrent.TimeUnit.MILLISECONDS)
                .writeTimeout(DEFAULT_TIMEOUT_MS, java.util.concurrent.TimeUnit.MILLISECONDS)
                .build()
        }
    }

    /**
     * Device code response from the authorization server.
     *
     * @property deviceAuthId Server-assigned device auth session ID (used for polling)
     * @property userCode Short user code to enter at verification URI
     * @property verificationUri URL where user enters the code
     * @property interval Minimum polling interval in seconds
     */
    data class DeviceCodeResponse(
        val deviceAuthId: String,
        val userCode: String,
        val verificationUri: String,
        val interval: Long
    )

    /**
     * OAuth token response after exchanging the authorization code.
     *
     * @property accessToken Access token for API calls
     * @property idToken OpenID Connect ID token
     * @property refreshToken Refresh token for getting new access tokens
     */
    data class TokenResponse(
        val accessToken: String,
        val idToken: String?,
        val refreshToken: String?
    )

    /**
     * Polling diagnostics used for on-device troubleshooting.
     *
     * @property attempts Total poll attempts made
     * @property pending403Count Number of 403 pending responses received
     * @property pending404Count Number of 404 pending responses received
     * @property networkErrorCount Number of transient network failures during polling
     * @property elapsedSeconds Total elapsed polling time in seconds
     * @property lastHttpStatus Most recent HTTP status seen (if any)
     * @property lastResponsePreview Truncated body/message from the latest relevant response
     */
    data class PollDiagnostics(
        val attempts: Int,
        val pending403Count: Int,
        val pending404Count: Int,
        val networkErrorCount: Int,
        val elapsedSeconds: Long,
        val lastHttpStatus: Int?,
        val lastResponsePreview: String?
    )

    /**
     * Result of polling for authorization.
     */
    sealed class PollResult {
        /**
         * Authorization granted — contains auth code and PKCE values for token exchange.
         */
        data class Success(
            val authCode: String,
            val codeVerifier: String,
            val codeChallenge: String,
            val diagnostics: PollDiagnostics
        ) : PollResult()

        /**
         * Error occurred during polling.
         *
         * @property error Error code (e.g., "timeout", "not_enabled")
         * @property description Human-readable error description
         */
        data class Error(
            val error: String,
            val description: String?,
            val diagnostics: PollDiagnostics? = null
        ) : PollResult()
    }

    /**
     * Exception thrown when device code operations fail.
     */
    class DeviceCodeException(message: String, cause: Throwable? = null) : Exception(message, cause)

    // --- API request/response models ---

    @Serializable
    private data class UserCodeRequest(
        @SerialName("client_id") val clientId: String
    )

    @Serializable
    private data class UserCodeApiResponse(
        @SerialName("device_auth_id") val deviceAuthId: String,
        @SerialName("user_code") val userCode: String,
        @SerialName("interval") val interval: String = "5"
    )

    @Serializable
    private data class TokenPollRequest(
        @SerialName("device_auth_id") val deviceAuthId: String,
        @SerialName("user_code") val userCode: String
    )

    @Serializable
    private data class CodeSuccessApiResponse(
        @SerialName("authorization_code") val authorizationCode: String,
        @SerialName("code_challenge") val codeChallenge: String,
        @SerialName("code_verifier") val codeVerifier: String
    )

    @Serializable
    private data class TokenExchangeApiResponse(
        @SerialName("access_token") val accessToken: String,
        @SerialName("id_token") val idToken: String? = null,
        @SerialName("refresh_token") val refreshToken: String? = null,
        @SerialName("token_type") val tokenType: String? = null
    )

    private val json = Json {
        ignoreUnknownKeys = true
        isLenient = true
    }

    /**
     * Request a device code from OpenAI's auth server.
     *
     * @param clientId OAuth client ID (defaults to Codex CLI's public client ID)
     * @return Result containing DeviceCodeResponse on success
     */
    suspend fun requestDeviceCode(
        clientId: String = DEFAULT_CLIENT_ID
    ): Result<DeviceCodeResponse> = withContext(Dispatchers.IO) {
        runCatching {
            val url = "${authBaseUrl}/api/accounts/deviceauth/usercode"
            val body = json.encodeToString(UserCodeRequest.serializer(), UserCodeRequest(clientId))

            val request = Request.Builder()
                .url(url)
                .post(body.toRequestBody(JSON_MEDIA_TYPE))
                .header("Content-Type", "application/json")
                .build()

            httpClient.newCall(request).execute().use { response ->
                val responseBody = response.body?.string()
                    ?: throw DeviceCodeException("Empty response body")

                if (response.code == 404) {
                    throw DeviceCodeException(
                        "Device code login is not enabled. Your workspace admin needs to enable " +
                        "device code authentication, or use an API key instead."
                    )
                }

                if (!response.isSuccessful) {
                    throw DeviceCodeException(
                        "Device code request failed with HTTP ${response.code}: $responseBody"
                    )
                }

                try {
                    val apiResponse = json.decodeFromString<UserCodeApiResponse>(responseBody)
                    val interval = apiResponse.interval.trim().toLongOrNull() ?: DEFAULT_POLL_INTERVAL_SECONDS
                    DeviceCodeResponse(
                        deviceAuthId = apiResponse.deviceAuthId,
                        userCode = apiResponse.userCode,
                        verificationUri = DEFAULT_VERIFICATION_URL,
                        interval = interval
                    )
                } catch (e: SerializationException) {
                    throw DeviceCodeException(
                        "Failed to parse device code response: ${e.message}",
                        e
                    )
                }
            }
        }
    }

    /**
     * Poll for authorization until the user approves, or timeout (15 minutes).
     *
     * OpenAI's protocol returns 403/404 while pending, and 200 with an
     * authorization code + PKCE values on success.
     *
     * @param deviceAuthId Device auth session ID from requestDeviceCode
     * @param userCode User code from requestDeviceCode
     * @param interval Polling interval in seconds
     * @return PollResult with auth code on success, or error
     */
    suspend fun pollForToken(
        deviceAuthId: String,
        userCode: String,
        interval: Long = DEFAULT_POLL_INTERVAL_SECONDS
    ): PollResult = withContext(Dispatchers.IO) {
        val url = "${authBaseUrl}/api/accounts/deviceauth/token"
        val maxWaitMs = maxWaitSeconds * 1000
        val startTime = System.currentTimeMillis()
        var attempts = 0
        var pending403Count = 0
        var pending404Count = 0
        var networkErrorCount = 0
        var lastHttpStatus: Int? = null
        var lastResponsePreview: String? = null

        fun diagnosticsSnapshot(): PollDiagnostics {
            val elapsedSeconds = ((System.currentTimeMillis() - startTime) / 1000).coerceAtLeast(0)
            return PollDiagnostics(
                attempts = attempts,
                pending403Count = pending403Count,
                pending404Count = pending404Count,
                networkErrorCount = networkErrorCount,
                elapsedSeconds = elapsedSeconds,
                lastHttpStatus = lastHttpStatus,
                lastResponsePreview = lastResponsePreview
            )
        }

        while (true) {
            // Wait before polling
            delay(interval * 1000)

            // Check timeout
            if (System.currentTimeMillis() - startTime >= maxWaitMs) {
                return@withContext PollResult.Error(
                    error = "timeout",
                    description = "Device code expired after ${maxWaitSeconds / 60} minutes",
                    diagnostics = diagnosticsSnapshot()
                )
            }

            attempts += 1
            val body = json.encodeToString(
                TokenPollRequest.serializer(),
                TokenPollRequest(deviceAuthId, userCode)
            )

            val request = Request.Builder()
                .url(url)
                .post(body.toRequestBody(JSON_MEDIA_TYPE))
                .header("Content-Type", "application/json")
                .build()

            try {
                httpClient.newCall(request).execute().use { response ->
                    val responseBody = response.body?.string() ?: ""
                    lastHttpStatus = response.code

                    // Only capture response preview for errors/pending — never for success.
                    // Success responses contain PKCE secrets (code_verifier, authorization_code)
                    // that must not be persisted to SharedPreferences or displayed in UI.
                    if (!response.isSuccessful) {
                        lastResponsePreview = responseBody.take(RESPONSE_PREVIEW_LIMIT).ifBlank { null }
                    }

                    if (response.isSuccessful) {
                        // User approved — we get an authorization code + PKCE values
                        try {
                            val codeResp = json.decodeFromString<CodeSuccessApiResponse>(responseBody)
                            return@withContext PollResult.Success(
                                authCode = codeResp.authorizationCode,
                                codeVerifier = codeResp.codeVerifier,
                                codeChallenge = codeResp.codeChallenge,
                                diagnostics = diagnosticsSnapshot()
                            )
                        } catch (e: Exception) {
                            return@withContext PollResult.Error(
                                error = "parse_error",
                                description = "Failed to parse auth response: ${e.message}",
                                diagnostics = diagnosticsSnapshot()
                            )
                        }
                    }

                    // 403 or 404 = still pending (per Codex CLI source)
                    if (response.code == 403 || response.code == 404) {
                        if (response.code == 403) {
                            pending403Count += 1
                        } else {
                            pending404Count += 1
                        }
                        // Continue polling
                    } else {
                        // Unexpected error
                        return@withContext PollResult.Error(
                            error = "http_${response.code}",
                            description = "Device auth failed with HTTP ${response.code}: $responseBody",
                            diagnostics = diagnosticsSnapshot()
                        )
                    }
                }
            } catch (e: IOException) {
                // Network error — transient, continue polling
                networkErrorCount += 1
                lastHttpStatus = null
                lastResponsePreview = e.message?.take(RESPONSE_PREVIEW_LIMIT)
            }
        }

        @Suppress("UNREACHABLE_CODE")
        error("Unreachable: polling loop always returns via return@withContext")
    }

    /**
     * Exchange the authorization code for OAuth tokens.
     *
     * After polling succeeds, use the returned auth code and PKCE values
     * to get actual access/refresh tokens from OpenAI's token endpoint.
     *
     * @param clientId OAuth client ID
     * @param authCode Authorization code from PollResult.Success
     * @param codeVerifier PKCE code verifier from PollResult.Success
     * @param codeChallenge PKCE code challenge from PollResult.Success (unused in exchange, kept for completeness)
     * @return Result containing TokenResponse
     */
    suspend fun exchangeCode(
        clientId: String = DEFAULT_CLIENT_ID,
        authCode: String,
        codeVerifier: String,
        codeChallenge: String = ""
    ): Result<TokenResponse> = withContext(Dispatchers.IO) {
        runCatching {
            val url = "${authBaseUrl}/oauth/token"
            val redirectUri = "${authBaseUrl}/deviceauth/callback"

            // Match exact parameter order from Codex CLI (server.rs)
            val formBody = okhttp3.FormBody.Builder()
                .add("grant_type", "authorization_code")
                .add("code", authCode)
                .add("redirect_uri", redirectUri)
                .add("client_id", clientId)
                .add("code_verifier", codeVerifier)
                .build()

            val request = Request.Builder()
                .url(url)
                .post(formBody)
                .build()

            httpClient.newCall(request).execute().use { response ->
                val responseBody = response.body?.string()
                    ?: throw DeviceCodeException("Empty response body")

                if (!response.isSuccessful) {
                    throw DeviceCodeException(
                        "Token exchange failed with HTTP ${response.code}: $responseBody"
                    )
                }

                try {
                    val tokenResp = json.decodeFromString<TokenExchangeApiResponse>(responseBody)
                    TokenResponse(
                        accessToken = tokenResp.accessToken,
                        idToken = tokenResp.idToken,
                        refreshToken = tokenResp.refreshToken
                    )
                } catch (e: SerializationException) {
                    throw DeviceCodeException(
                        "Failed to parse token response: ${e.message}",
                        e
                    )
                }
            }
        }
    }
}
