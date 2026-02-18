package ai.citros.core

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import org.junit.After
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test
import kotlinx.coroutines.runBlocking

class EmbeddedCodexOauthBridgeServerTest {
    private lateinit var bridge: EmbeddedCodexOauthBridgeServer
    private val client = OkHttpClient()
    private val json = Json { ignoreUnknownKeys = true }
    private var baseUrl = ""

    @Before
    fun setup() = runBlocking {
        bridge = EmbeddedCodexOauthBridgeServer(
            EmbeddedCodexOauthBridgeServer.Config(
                clientId = "test-client-id",
                authorizeUrl = "https://auth.example.com/authorize",
                tokenUrl = "https://auth.example.com/token"
            )
        )
        val result = bridge.start()
        assertTrue(result.isSuccess)
        baseUrl = result.getOrThrow().baseUrl
    }

    @After
    fun teardown() {
        bridge.stop()
    }

    private fun post(path: String, body: String): okhttp3.Response {
        val request = Request.Builder()
            .url("$baseUrl$path")
            .post(body.toRequestBody("application/json".toMediaType()))
            .build()
        return client.newCall(request).execute()
    }

    private fun get(path: String): okhttp3.Response {
        val request = Request.Builder().url("$baseUrl$path").build()
        return client.newCall(request).execute()
    }

    @Test
    fun healthEndpointReturnsOk() {
        val response = get("/health")
        assertEquals(200, response.code)
        val body = json.parseToJsonElement(response.body!!.string()).jsonObject
        assertEquals("true", body["ok"]?.jsonPrimitive?.content)
    }

    @Test
    fun startReturnsAuthUrlAndLoginId() {
        val response = post("/oauth/codex/start", """{"redirect_uri":"citros://oauth/callback","state":"test-state"}""")
        assertEquals(200, response.code)
        val body = json.parseToJsonElement(response.body!!.string()).jsonObject
        assertNotNull(body["authUrl"]?.jsonPrimitive?.content)
        assertNotNull(body["loginId"]?.jsonPrimitive?.content)
        assertTrue(body["authUrl"]?.jsonPrimitive?.content?.contains("code_challenge") == true)
        // Should NOT contain code_verifier (PKCE leak fix)
        assertNull(body["codeVerifier"])
        assertNull(body["code_verifier"])
    }

    @Test
    fun startRejectsMissingRedirectUri() {
        val response = post("/oauth/codex/start", """{"state":"test-state"}""")
        assertEquals(400, response.code)
    }

    @Test
    fun startRejectsMissingState() {
        val response = post("/oauth/codex/start", """{"redirect_uri":"citros://oauth/callback"}""")
        assertEquals(400, response.code)
    }

    @Test
    fun startRejectsInvalidRedirectUri() {
        val response = post("/oauth/codex/start", """{"redirect_uri":"https://evil.com/callback","state":"s"}""")
        assertEquals(400, response.code)
    }

    @Test
    fun exchangeRejectsUnknownLoginId() {
        val response = post("/oauth/codex/exchange", """{"code":"auth-code","login_id":"nonexistent","state":"s"}""")
        assertEquals(400, response.code)
        val body = json.parseToJsonElement(response.body!!.string()).jsonObject
        assertTrue(body["error_description"]?.jsonPrimitive?.content?.contains("Unknown") == true)
    }

    @Test
    fun exchangeRejectsMissingState() {
        // First start a session
        val startResponse = post("/oauth/codex/start", """{"redirect_uri":"citros://oauth/callback","state":"my-state"}""")
        val startBody = json.parseToJsonElement(startResponse.body!!.string()).jsonObject
        val loginId = startBody["loginId"]?.jsonPrimitive?.content!!

        // Try exchange without state
        val response = post("/oauth/codex/exchange", """{"code":"auth-code","login_id":"$loginId"}""")
        assertEquals(400, response.code)
    }

    @Test
    fun exchangeRejectsStateMismatch() {
        val startResponse = post("/oauth/codex/start", """{"redirect_uri":"citros://oauth/callback","state":"correct-state"}""")
        val startBody = json.parseToJsonElement(startResponse.body!!.string()).jsonObject
        val loginId = startBody["loginId"]?.jsonPrimitive?.content!!

        val response = post("/oauth/codex/exchange", """{"code":"auth-code","login_id":"$loginId","state":"wrong-state"}""")
        assertEquals(400, response.code)
    }

    @Test
    fun startResponseDoesNotLeakCodeVerifier() {
        val response = post("/oauth/codex/start", """{"redirect_uri":"citros://oauth/callback","state":"s"}""")
        val bodyStr = response.body!!.string()
        assertFalse(bodyStr.contains("codeVerifier"))
        assertFalse(bodyStr.contains("code_verifier"))
    }

    @Test
    fun serverStopsCleanly() = runBlocking {
        assertTrue(bridge.isRunning())
        bridge.stop()
        assertFalse(bridge.isRunning())
    }

    @Test
    fun redirectUriValidationBlocksArbitraryPaths() {
        val response = post("/oauth/codex/start", """{"redirect_uri":"http://localhost:1234/evil","state":"s"}""")
        assertEquals(400, response.code)
    }

    @Test
    fun redirectUriValidationAllowsLocalhostCallback() {
        val response = post("/oauth/codex/start", """{"redirect_uri":"http://localhost:4318/callback","state":"s"}""")
        assertEquals(200, response.code)
    }

    @Test
    fun exchangeRejectsMissingLoginId() {
        val response = post("/oauth/codex/exchange", """{"code":"auth-code","code_verifier":"verifier","redirect_uri":"citros://oauth/callback","state":"s"}""")
        assertEquals(400, response.code)
        val body = json.parseToJsonElement(response.body!!.string()).jsonObject
        assertTrue(body["error_description"]?.jsonPrimitive?.content?.contains("login_id") == true)
    }
}
