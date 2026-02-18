package ai.citros.core

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.JsonObjectBuilder
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.put
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.util.concurrent.TimeUnit

/**
 * Lightweight client for a Codex OAuth bridge service used by mobile auth flows.
 *
 * Bridge contract (tolerant):
 * - Start login: returns an auth URL (`authUrl`, `auth_url`, `url`, etc.)
 * - Exchange code: returns an OAuth token (`accessToken`, `access_token`, `token`, etc.)
 *
 * Response wrappers such as `{ "result": { ... } }` and `{ "data": { ... } }` are supported.
 */
class CodexOauthBridgeClient(
    private val client: OkHttpClient = sharedClient
) {
    private val json = Json {
        ignoreUnknownKeys = true
        explicitNulls = false
    }

    data class StartLoginResult(
        val authUrl: String,
        val loginId: String? = null,
        val codeVerifier: String? = null
    )

    data class ExchangeCodeResult(
        val accessToken: String
    )

    /**
     * Initiates an OAuth login flow via the bridge service.
     *
     * @param bridgeBaseUrl Base URL of the bridge service (e.g., "http://127.0.0.1:4318")
     * @param redirectUri Deep link URI for OAuth callback (e.g., "citros://oauth/callback")
     * @param state CSRF protection token (must be validated on callback)
     * @return Result containing auth URL and optional session identifiers (loginId, codeVerifier)
     */
    suspend fun startLogin(
        bridgeBaseUrl: String,
        redirectUri: String,
        state: String
    ): Result<StartLoginResult> = withContext(Dispatchers.IO) {
        val payload = buildJsonObject {
            put("redirect_uri", redirectUri)
            put("redirectUri", redirectUri)
            put("state", state)
        }

        postWithFallback(
            baseUrl = bridgeBaseUrl,
            paths = START_PATHS,
            payload = payload
        ).mapCatching { body ->
            val authUrl = extractString(body, AUTH_URL_KEYS)
                ?: throw IllegalStateException("OAuth bridge response is missing auth URL")
            StartLoginResult(
                authUrl = authUrl,
                loginId = extractString(body, LOGIN_ID_KEYS),
                codeVerifier = extractString(body, CODE_VERIFIER_KEYS)
            )
        }
    }

    /**
     * Exchanges an OAuth authorization code for an access token.
     *
     * @param bridgeBaseUrl Base URL of the bridge service
     * @param code Authorization code from OAuth callback
     * @param state OAuth state parameter (optional, for validation)
     * @param loginId Session identifier from startLogin (optional)
     * @param codeVerifier PKCE code verifier from startLogin (optional)
     * @return Result containing the access token
     */
    suspend fun exchangeCode(
        bridgeBaseUrl: String,
        code: String,
        state: String?,
        loginId: String? = null,
        codeVerifier: String? = null,
        redirectUri: String? = null
    ): Result<ExchangeCodeResult> = withContext(Dispatchers.IO) {
        val payload = buildJsonObject {
            put("code", code)
            state?.takeIf { it.isNotBlank() }?.let { put("state", it) }
            putIfNotBlank("login_id", "loginId", loginId)
            putIfNotBlank("code_verifier", "codeVerifier", codeVerifier)
            putIfNotBlank("redirect_uri", "redirectUri", redirectUri)
        }

        postWithFallback(
            baseUrl = bridgeBaseUrl,
            paths = EXCHANGE_PATHS,
            payload = payload
        ).mapCatching { body ->
            val token = extractString(body, TOKEN_KEYS)
                ?: throw IllegalStateException("OAuth bridge response is missing access token")
            ExchangeCodeResult(accessToken = token)
        }
    }

    private fun postWithFallback(
        baseUrl: String,
        paths: List<String>,
        payload: JsonObject
    ): Result<JsonObject> {
        val normalizedBase = normalizeBaseUrl(baseUrl)
        val errors = mutableListOf<String>()

        for (path in paths) {
            val url = normalizedBase + path
            val result = runCatching {
                val request = Request.Builder()
                    .url(url)
                    .addHeader("Content-Type", "application/json")
                    .post(payload.toString().toRequestBody(JSON_MEDIA_TYPE))
                    .build()

                client.newCall(request).execute().use { response ->
                    val body = response.body?.string().orEmpty()
                    if (!response.isSuccessful) {
                        throw IllegalStateException(
                            "status=${response.code} body=${body.take(MAX_ERROR_BODY_CHARS)}"
                        )
                    }
                    if (body.isBlank()) {
                        throw IllegalStateException("status=${response.code} body=empty")
                    }
                    json.parseToJsonElement(body).jsonObject
                }
            }

            result.getOrNull()?.let { return Result.success(it) }
            errors += "$path -> ${result.exceptionOrNull()?.message ?: "unknown error"}"
        }

        return Result.failure(
            IllegalStateException(
                "OAuth bridge request failed for ${paths.joinToString()}: ${errors.joinToString(" | ")}"
            )
        )
    }

    private fun normalizeBaseUrl(baseUrl: String): String {
        val trimmed = baseUrl.trim()
        require(trimmed.isNotEmpty()) { "Bridge URL cannot be empty" }
        return trimmed.trimEnd('/')
    }

    private fun extractString(payload: JsonObject, keys: List<String>): String? {
        val candidates = sequenceOf(
            payload,
            payload["result"]?.asJsonObjectOrNull(),
            payload["data"]?.asJsonObjectOrNull(),
            payload["payload"]?.asJsonObjectOrNull(),
            payload["session"]?.asJsonObjectOrNull()
        ).filterNotNull()

        for (candidate in candidates) {
            for (key in keys) {
                val value = candidate[key]?.asJsonStringOrNull()
                if (!value.isNullOrBlank()) {
                    return value
                }
            }
        }

        return null
    }

    private fun JsonElement.asJsonObjectOrNull(): JsonObject? = this as? JsonObject

    private fun JsonElement.asJsonStringOrNull(): String? {
        val primitive = this as? JsonPrimitive ?: return null
        return primitive.contentOrNull
    }

    /**
     * Puts a value into the JSON object builder only if it's not null and not blank.
     * Supports both snake_case and camelCase keys for API compatibility.
     */
    private fun JsonObjectBuilder.putIfNotBlank(snakeKey: String, camelKey: String, value: String?) {
        value?.takeIf { it.isNotBlank() }?.let {
            put(snakeKey, it)
            put(camelKey, it)
        }
    }

    companion object {
        private val JSON_MEDIA_TYPE = "application/json".toMediaType()
        private const val MAX_ERROR_BODY_CHARS = 300

        const val DEFAULT_BRIDGE_BASE_URL = "http://127.0.0.1:4318"

        private val START_PATHS = listOf(
            "/oauth/codex/start",
            "/oauth/start",
            "/account/login/start"
        )

        private val EXCHANGE_PATHS = listOf(
            "/oauth/codex/exchange",
            "/oauth/exchange",
            "/account/login/exchange"
        )

        private val AUTH_URL_KEYS = listOf(
            "authUrl",
            "auth_url",
            "url",
            "loginUrl",
            "login_url"
        )

        private val TOKEN_KEYS = listOf(
            "accessToken",
            "access_token",
            "token",
            "oauthToken",
            "oauth_token"
        )

        private val LOGIN_ID_KEYS = listOf(
            "loginId",
            "login_id",
            "requestId",
            "request_id",
            "id"
        )

        private val CODE_VERIFIER_KEYS = listOf(
            "codeVerifier",
            "code_verifier"
        )

        private val sharedClient = OkHttpClient.Builder()
            .connectTimeout(15, TimeUnit.SECONDS)
            .readTimeout(20, TimeUnit.SECONDS)
            .writeTimeout(15, TimeUnit.SECONDS)
            .build()
    }
}
