package ai.citros.core

import android.util.Log
import fi.iki.elonen.NanoHTTPD
import fi.iki.elonen.NanoHTTPD.Response
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.put
import okhttp3.FormBody
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull
import java.net.ServerSocket
import java.security.MessageDigest
import java.util.Base64
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.TimeUnit

/**
 * In-process OAuth bridge server for Android.
 *
 * This allows a seamless workaround flow where the app starts a local bridge
 * automatically and uses it via localhost without requiring external bridge setup.
 */
class EmbeddedCodexOauthBridgeServer(
    private val config: Config,
    private val httpClient: OkHttpClient = sharedClient
) {
    data class Config(
        val authorizeUrl: String = DEFAULT_AUTHORIZE_URL,
        val tokenUrl: String = DEFAULT_TOKEN_URL,
        val clientId: String,
        val clientSecret: String? = null,
        val scope: String = DEFAULT_SCOPE
    )

    data class RunningServer(
        val port: Int,
        val baseUrl: String
    )

    private data class LoginSession(
        val state: String,
        val redirectUri: String,
        val codeVerifier: String,
        val createdAtSecs: Long
    )

    private val json = Json {
        ignoreUnknownKeys = true
        explicitNulls = false
    }
    private val sessions = ConcurrentHashMap<String, LoginSession>()

    @Volatile
    private var server: BridgeServer? = null

    suspend fun start(): Result<RunningServer> = withContext(Dispatchers.IO) {
        runCatching {
            if (config.clientId.isBlank()) {
                throw IllegalArgumentException("Missing OAuth client ID")
            }
            if (config.authorizeUrl.isBlank() || config.tokenUrl.isBlank()) {
                throw IllegalArgumentException("Missing OAuth authorize/token URL")
            }

            synchronized(this@EmbeddedCodexOauthBridgeServer) {
                server?.let {
                    val port = it.listeningPort
                    return@synchronized RunningServer(port, "http://127.0.0.1:$port")
                }

                val port = pickFreePort()
                val created = BridgeServer(port)
                created.start(NanoHTTPD.SOCKET_READ_TIMEOUT, false)
                server = created
                RunningServer(port, "http://127.0.0.1:$port")
            }
        }
    }

    fun stop() {
        synchronized(this) {
            server?.stop()
            server = null
            sessions.clear()
        }
    }

    fun isRunning(): Boolean = server != null

    private inner class BridgeServer(port: Int) : NanoHTTPD("127.0.0.1", port) {
        override fun serve(session: IHTTPSession): Response {
            return try {
                when {
                    session.method == Method.GET && session.uri == "/health" -> {
                        jsonResponse(Response.Status.OK, buildJsonObject {
                            put("ok", true)
                            put("service", "embedded-codex-oauth-bridge")
                        })
                    }
                    session.method != Method.POST -> errorResponse(
                        Response.Status.NOT_FOUND,
                        "not_found",
                        "Endpoint not found"
                    )
                    session.uri in START_PATHS -> handleStart(session)
                    session.uri in EXCHANGE_PATHS -> handleExchange(session)
                    else -> errorResponse(Response.Status.NOT_FOUND, "not_found", "Endpoint not found")
                }
            } catch (e: Exception) {
                Log.e("EmbeddedOAuthBridge", "Request failed: ${session.uri}", e)
                errorResponse(
                    Response.Status.INTERNAL_ERROR,
                    "server_error",
                    e.message ?: "Unknown server error"
                )
            }
        }
    }

    private fun handleStart(session: NanoHTTPD.IHTTPSession): NanoHTTPD.Response {
        evictExpiredSessions()
        
        val payload = parseBodyAsJson(session)
            ?: return errorResponse(Response.Status.BAD_REQUEST, "invalid_request", "Invalid JSON body")

        val redirectUri = extractString(payload, listOf("redirect_uri", "redirectUri"))
            ?: return errorResponse(Response.Status.BAD_REQUEST, "invalid_request", "Missing redirect_uri")
        val state = extractString(payload, listOf("state"))
            ?: return errorResponse(Response.Status.BAD_REQUEST, "invalid_request", "Missing state")

        if (!isAllowedRedirectUri(redirectUri)) {
            return errorResponse(
                Response.Status.BAD_REQUEST,
                "invalid_request",
                "Invalid redirect_uri. Use citros://oauth/callback or localhost."
            )
        }

        val loginId = UUID.randomUUID().toString()
        val codeVerifier = generatePkceVerifier()
        val codeChallenge = codeChallengeS256(codeVerifier)

        val authUrl = config.authorizeUrl
            .toHttpUrlOrNull()
            ?.newBuilder()
            ?.addQueryParameter("response_type", "code")
            ?.addQueryParameter("client_id", config.clientId)
            ?.addQueryParameter("redirect_uri", redirectUri)
            ?.addQueryParameter("scope", config.scope)
            ?.addQueryParameter("state", state)
            ?.addQueryParameter("code_challenge", codeChallenge)
            ?.addQueryParameter("code_challenge_method", "S256")
            ?.build()
            ?.toString()
            ?: return errorResponse(
                Response.Status.INTERNAL_ERROR,
                "server_error",
                "Invalid authorize URL configuration"
            )

        sessions[loginId] = LoginSession(
            state = state,
            redirectUri = redirectUri,
            codeVerifier = codeVerifier,
            createdAtSecs = nowSecs()
        )

        return jsonResponse(Response.Status.OK, buildJsonObject {
            put("authUrl", authUrl)
            put("auth_url", authUrl)
            put("loginId", loginId)
            put("login_id", loginId)
        })
    }

    private fun handleExchange(session: NanoHTTPD.IHTTPSession): NanoHTTPD.Response {
        val payload = parseBodyAsJson(session)
            ?: return errorResponse(Response.Status.BAD_REQUEST, "invalid_request", "Invalid JSON body")

        val code = extractString(payload, listOf("code"))
            ?: return errorResponse(Response.Status.BAD_REQUEST, "invalid_request", "Missing code")

        val loginId = extractString(payload, listOf("login_id", "loginId"))
            ?: return errorResponse(
                Response.Status.BAD_REQUEST,
                "invalid_request",
                "Missing login_id. The exchange endpoint requires a login_id from the /start response."
            )

        val incomingState = extractString(payload, listOf("state"))
            ?: return errorResponse(Response.Status.BAD_REQUEST, "invalid_request", "Missing state parameter")

        val sessionData = sessions[loginId]
            ?: return errorResponse(
                Response.Status.BAD_REQUEST,
                "invalid_request",
                "Unknown or expired login_id. Please restart the sign-in flow."
            )

        // Validate state matches session
        if (incomingState != sessionData.state) {
            return errorResponse(Response.Status.BAD_REQUEST, "invalid_request", "State mismatch for login session")
        }

        sessions.remove(loginId)
        val resolvedVerifier = sessionData.codeVerifier
        val resolvedRedirectUri = sessionData.redirectUri

        val tokenResult = runCatching {
            val formBuilder = FormBody.Builder()
                .add("grant_type", "authorization_code")
                .add("code", code)
                .add("client_id", config.clientId)
                .add("redirect_uri", resolvedRedirectUri)
                .add("code_verifier", resolvedVerifier)

            config.clientSecret?.takeIf { it.isNotBlank() }?.let {
                formBuilder.add("client_secret", it)
            }

            val request = Request.Builder()
                .url(config.tokenUrl)
                .post(formBuilder.build())
                .build()

            httpClient.newCall(request).execute().use { response ->
                val body = response.body?.string().orEmpty()
                if (!response.isSuccessful) {
                    throw IllegalStateException(
                        "Token endpoint returned ${response.code}: ${safeErrorString(body)}"
                    )
                }

                val jsonBody = json.parseToJsonElement(body).jsonObject
                val token = extractString(
                    jsonBody,
                    listOf("access_token", "accessToken", "token")
                ) ?: throw IllegalStateException("Token endpoint response missing access token")

                val tokenType = extractString(jsonBody, listOf("token_type", "tokenType"))
                val expiresIn = extractLong(jsonBody, listOf("expires_in", "expiresIn"))
                val refreshToken = extractString(jsonBody, listOf("refresh_token", "refreshToken"))

                buildJsonObject {
                    put("accessToken", token)
                    put("access_token", token)
                    tokenType?.let {
                        put("tokenType", it)
                        put("token_type", it)
                    }
                    expiresIn?.let {
                        put("expiresIn", it)
                        put("expires_in", it)
                    }
                    refreshToken?.let {
                        put("refreshToken", it)
                        put("refresh_token", it)
                    }
                }
            }
        }

        return tokenResult.fold(
            onSuccess = { payloadJson ->
                jsonResponse(Response.Status.OK, payloadJson)
            },
            onFailure = { error ->
                errorResponse(
                    Response.Status.INTERNAL_ERROR,
                    "upstream_error",
                    error.message ?: "Token exchange failed"
                )
            }
        )
    }

    private fun parseBodyAsJson(session: NanoHTTPD.IHTTPSession): JsonObject? {
        return runCatching {
            val files = HashMap<String, String>()
            session.parseBody(files)
            val body = files["postData"].orEmpty()
            if (body.isBlank()) return null
            json.parseToJsonElement(body).jsonObject
        }.getOrNull()
    }

    private fun jsonResponse(
        status: NanoHTTPD.Response.IStatus,
        payload: JsonObject
    ): NanoHTTPD.Response {
        return NanoHTTPD.newFixedLengthResponse(
            status,
            "application/json",
            payload.toString()
        ).apply {
            addHeader("Cache-Control", "no-store")
        }
    }

    private fun errorResponse(
        status: NanoHTTPD.Response.IStatus,
        error: String,
        message: String
    ): NanoHTTPD.Response {
        return jsonResponse(status, buildJsonObject {
            put("error", error)
            put("error_description", message.take(MAX_ERROR_CHARS))
        })
    }

    private fun extractString(payload: JsonObject, keys: List<String>): String? {
        for (key in keys) {
            val value = (payload[key] as? JsonPrimitive)?.contentOrNull?.trim()
            if (!value.isNullOrBlank()) {
                return value
            }
        }
        return null
    }

    private fun extractLong(payload: JsonObject, keys: List<String>): Long? {
        for (key in keys) {
            val primitive = payload[key] as? JsonPrimitive ?: continue
            primitive.contentOrNull?.toLongOrNull()?.let { return it }
        }
        return null
    }

    private fun safeErrorString(body: String): String {
        if (body.isBlank()) return "empty response"
        val parsed = runCatching { json.parseToJsonElement(body).jsonObject }.getOrNull()
        val providerError = parsed?.let {
            extractString(it, listOf("error_description", "error", "message"))
        }
        return (providerError ?: body).take(MAX_ERROR_CHARS)
    }

    private fun isAllowedRedirectUri(uri: String): Boolean {
        if (uri == "citros://oauth/callback") return true
        // Allow localhost for bridge compatibility (any port, /callback path only)
        val parsed = uri.toHttpUrlOrNull() ?: return false
        return (parsed.host == "127.0.0.1" || parsed.host == "localhost") &&
            parsed.scheme == "http" &&
            parsed.encodedPath == "/callback"
    }

    private fun pickFreePort(): Int {
        ServerSocket(0).use { socket ->
            return socket.localPort
        }
    }

    private fun nowSecs(): Long = System.currentTimeMillis() / 1000

    private fun evictExpiredSessions() {
        val now = nowSecs()
        sessions.entries.removeIf { (_, session) ->
            now - session.createdAtSecs > SESSION_TTL_SECONDS
        }
    }

    private fun generatePkceVerifier(): String {
        val bytes = ByteArray(PKCE_LENGTH)
        random.nextBytes(bytes)
        return buildString(PKCE_LENGTH) {
            for (raw in bytes) {
                val idx = (raw.toInt() and 0xff) % PKCE_CHARSET.length
                append(PKCE_CHARSET[idx])
            }
        }
    }

    private fun codeChallengeS256(codeVerifier: String): String {
        val digest = MessageDigest.getInstance("SHA-256").digest(codeVerifier.toByteArray())
        return Base64.getUrlEncoder().withoutPadding().encodeToString(digest)
    }

    companion object {
        const val DEFAULT_AUTHORIZE_URL = "https://auth.openai.com/authorize"
        const val DEFAULT_TOKEN_URL = "https://auth.openai.com/oauth/token"
        const val DEFAULT_SCOPE = "openid profile email offline_access"

        private const val MAX_ERROR_CHARS = 500
        private const val SESSION_TTL_SECONDS = 15 * 60L
        private val random = java.security.SecureRandom()
        private const val PKCE_LENGTH = 64
        private const val PKCE_CHARSET =
            "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~"

        private val START_PATHS = setOf(
            "/oauth/codex/start",
            "/oauth/start",
            "/account/login/start"
        )

        private val EXCHANGE_PATHS = setOf(
            "/oauth/codex/exchange",
            "/oauth/exchange",
            "/account/login/exchange"
        )

        private val sharedClient = OkHttpClient.Builder()
            .connectTimeout(15, TimeUnit.SECONDS)
            .readTimeout(30, TimeUnit.SECONDS)
            .writeTimeout(15, TimeUnit.SECONDS)
            .build()
    }
}
