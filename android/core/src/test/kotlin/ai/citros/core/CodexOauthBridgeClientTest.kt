package ai.citros.core

import kotlinx.coroutines.test.runTest
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.After
import org.junit.Before
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

class CodexOauthBridgeClientTest {

    private lateinit var server: MockWebServer
    private lateinit var client: CodexOauthBridgeClient

    @Before
    fun setUp() {
        server = MockWebServer()
        server.start()
        client = CodexOauthBridgeClient()
    }

    @After
    fun tearDown() {
        server.shutdown()
    }

    private fun bridgeBaseUrl(): String = server.url("/").toString().trimEnd('/')

    @Test
    fun `startLogin parses direct authUrl response`() = runTest {
        server.enqueue(
            MockResponse()
                .setResponseCode(200)
                .addHeader("Content-Type", "application/json")
                .setBody(
                    """
                    {
                      "authUrl": "https://auth.openai.com/oauth2/authorize",
                      "loginId": "login-123"
                    }
                    """.trimIndent()
                )
        )

        val result = client.startLogin(
            bridgeBaseUrl = bridgeBaseUrl(),
            redirectUri = "citros://oauth/callback",
            state = "state-1"
        )

        assertTrue(result.isSuccess)
        assertEquals("https://auth.openai.com/oauth2/authorize", result.getOrNull()?.authUrl)
        assertEquals("login-123", result.getOrNull()?.loginId)

        val request = server.takeRequest()
        assertEquals("/oauth/codex/start", request.path)
        val body = request.body.readUtf8()
        assertTrue(body.contains("\"redirect_uri\":\"citros://oauth/callback\""))
        assertTrue(body.contains("\"state\":\"state-1\""))
    }

    @Test
    fun `startLogin falls back to alternate endpoint and nested payload`() = runTest {
        server.enqueue(
            MockResponse()
                .setResponseCode(404)
                .setBody("""{"error":"not found"}""")
        )
        server.enqueue(
            MockResponse()
                .setResponseCode(200)
                .addHeader("Content-Type", "application/json")
                .setBody(
                    """
                    {
                      "result": {
                        "auth_url": "https://auth.openai.com/login?flow=codex"
                      }
                    }
                    """.trimIndent()
                )
        )

        val result = client.startLogin(
            bridgeBaseUrl = bridgeBaseUrl(),
            redirectUri = "citros://oauth/callback",
            state = "state-2"
        )

        assertTrue(result.isSuccess)
        assertEquals("https://auth.openai.com/login?flow=codex", result.getOrNull()?.authUrl)

        val first = server.takeRequest()
        val second = server.takeRequest()
        assertEquals("/oauth/codex/start", first.path)
        assertEquals("/oauth/start", second.path)
    }

    @Test
    fun `exchangeCode parses access token from nested data payload`() = runTest {
        server.enqueue(
            MockResponse()
                .setResponseCode(200)
                .addHeader("Content-Type", "application/json")
                .setBody(
                    """
                    {
                      "data": {
                        "access_token": "sess-codex-token-xyz"
                      }
                    }
                    """.trimIndent()
                )
        )

        val result = client.exchangeCode(
            bridgeBaseUrl = bridgeBaseUrl(),
            code = "code-1",
            state = "state-3",
            loginId = "login-abc",
            codeVerifier = "pkce-verifier"
        )

        assertTrue(result.isSuccess)
        assertEquals("sess-codex-token-xyz", result.getOrNull()?.accessToken)

        val request = server.takeRequest()
        assertEquals("/oauth/codex/exchange", request.path)
        val body = request.body.readUtf8()
        assertTrue(body.contains("\"code\":\"code-1\""))
        assertTrue(body.contains("\"state\":\"state-3\""))
        assertTrue(body.contains("\"login_id\":\"login-abc\""))
        assertTrue(body.contains("\"code_verifier\":\"pkce-verifier\""))
    }

    @Test
    fun `exchangeCode returns failure when response has no token`() = runTest {
        server.enqueue(
            MockResponse()
                .setResponseCode(200)
                .addHeader("Content-Type", "application/json")
                .setBody("""{"ok":true}""")
        )

        val result = client.exchangeCode(
            bridgeBaseUrl = bridgeBaseUrl(),
            code = "code-2",
            state = "state-4"
        )

        assertTrue(result.isFailure)
        val message = result.exceptionOrNull()?.message
        assertNotNull(message)
        assertTrue(message.contains("missing access token"))
    }

    // Edge case tests from code review

    @Test
    fun `startLogin fails when authUrl is blank string`() = runTest {
        server.enqueue(
            MockResponse()
                .setResponseCode(200)
                .addHeader("Content-Type", "application/json")
                .setBody("""{"authUrl":""}""")
        )

        val result = client.startLogin(
            bridgeBaseUrl = bridgeBaseUrl(),
            redirectUri = "citros://oauth/callback",
            state = "state-blank"
        )

        assertTrue(result.isFailure)
        val message = result.exceptionOrNull()?.message
        assertNotNull(message)
        assertTrue(message.contains("missing auth URL"))
    }

    @Test
    fun `startLogin fails with malformed JSON response`() = runTest {
        server.enqueue(
            MockResponse()
                .setResponseCode(200)
                .addHeader("Content-Type", "application/json")
                .setBody("""{"authUrl":"https://example.com", invalid json}""")
        )

        val result = client.startLogin(
            bridgeBaseUrl = bridgeBaseUrl(),
            redirectUri = "citros://oauth/callback",
            state = "state-malformed"
        )

        assertTrue(result.isFailure)
    }

    @Test
    fun `exchangeCode respects token key priority order`() = runTest {
        // Test that 'accessToken' takes precedence over 'token'
        server.enqueue(
            MockResponse()
                .setResponseCode(200)
                .addHeader("Content-Type", "application/json")
                .setBody(
                    """
                    {
                      "token": "fallback-token",
                      "accessToken": "preferred-token"
                    }
                    """.trimIndent()
                )
        )

        val result = client.exchangeCode(
            bridgeBaseUrl = bridgeBaseUrl(),
            code = "code-priority",
            state = "state-priority"
        )

        assertTrue(result.isSuccess)
        // Should pick accessToken first in priority
        assertEquals("preferred-token", result.getOrNull()?.accessToken)
    }

    @Test
    fun `exchangeCode handles blank token value`() = runTest {
        server.enqueue(
            MockResponse()
                .setResponseCode(200)
                .addHeader("Content-Type", "application/json")
                .setBody("""{"accessToken":"   "}""")
        )

        val result = client.exchangeCode(
            bridgeBaseUrl = bridgeBaseUrl(),
            code = "code-blank-token",
            state = "state-blank-token"
        )

        assertTrue(result.isFailure)
        val message = result.exceptionOrNull()?.message
        assertNotNull(message)
        assertTrue(message.contains("missing access token"))
    }

    @Test
    fun `exchangeCode works with optional parameters omitted`() = runTest {
        server.enqueue(
            MockResponse()
                .setResponseCode(200)
                .addHeader("Content-Type", "application/json")
                .setBody("""{"accessToken":"sess-minimal"}""")
        )

        val result = client.exchangeCode(
            bridgeBaseUrl = bridgeBaseUrl(),
            code = "code-minimal",
            state = null,
            loginId = null,
            codeVerifier = null
        )

        assertTrue(result.isSuccess)
        assertEquals("sess-minimal", result.getOrNull()?.accessToken)

        val request = server.takeRequest()
        val body = request.body.readUtf8()
        assertTrue(body.contains("\"code\":\"code-minimal\""))
        // Optional parameters should not be in the payload when null
        assertTrue(!body.contains("login_id") || !body.contains("loginId"))
        assertTrue(!body.contains("code_verifier") || !body.contains("codeVerifier"))
    }
}
