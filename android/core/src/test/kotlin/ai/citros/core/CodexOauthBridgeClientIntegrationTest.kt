package ai.citros.core

import kotlinx.coroutines.test.runTest
import okhttp3.mockwebserver.Dispatcher
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import okhttp3.mockwebserver.RecordedRequest
import org.junit.After
import org.junit.Before
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

class CodexOauthBridgeClientIntegrationTest {

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
    fun `full flow uses codex endpoints when available`() = runTest {
        server.dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse {
                return when (request.path) {
                    "/oauth/codex/start" -> MockResponse()
                        .setResponseCode(200)
                        .addHeader("Content-Type", "application/json")
                        .setBody(
                            """
                            {
                              "authUrl": "https://auth.openai.com/oauth2/authorize?flow=codex",
                              "loginId": "login-direct",
                              "codeVerifier": "verifier-direct"
                            }
                            """.trimIndent()
                        )
                    "/oauth/codex/exchange" -> MockResponse()
                        .setResponseCode(200)
                        .addHeader("Content-Type", "application/json")
                        .setBody("""{"accessToken":"sess-direct-token"}""")
                    else -> MockResponse().setResponseCode(404)
                }
            }
        }

        val start = client.startLogin(
            bridgeBaseUrl = bridgeBaseUrl(),
            redirectUri = "citros://oauth/callback",
            state = "state-direct"
        )
        assertTrue(start.isSuccess)
        assertEquals("login-direct", start.getOrNull()?.loginId)
        assertEquals("verifier-direct", start.getOrNull()?.codeVerifier)

        val exchange = client.exchangeCode(
            bridgeBaseUrl = bridgeBaseUrl(),
            code = "code-direct",
            state = "state-direct",
            loginId = start.getOrNull()?.loginId,
            codeVerifier = start.getOrNull()?.codeVerifier
        )
        assertTrue(exchange.isSuccess)
        assertEquals("sess-direct-token", exchange.getOrNull()?.accessToken)

        val request1 = server.takeRequest()
        val request2 = server.takeRequest()
        assertEquals("/oauth/codex/start", request1.path)
        assertEquals("/oauth/codex/exchange", request2.path)
        assertTrue(request1.body.readUtf8().contains("\"redirect_uri\":\"citros://oauth/callback\""))
        val exchangeBody = request2.body.readUtf8()
        assertTrue(exchangeBody.contains("\"code\":\"code-direct\""))
        assertTrue(exchangeBody.contains("\"state\":\"state-direct\""))
        assertTrue(exchangeBody.contains("\"login_id\":\"login-direct\""))
        assertTrue(exchangeBody.contains("\"code_verifier\":\"verifier-direct\""))
    }

    @Test
    fun `full flow falls back from codex paths to generic oauth paths`() = runTest {
        server.dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse {
                return when (request.path) {
                    "/oauth/codex/start" -> MockResponse()
                        .setResponseCode(404)
                        .setBody("""{"error":"missing"}""")
                    "/oauth/start" -> MockResponse()
                        .setResponseCode(200)
                        .addHeader("Content-Type", "application/json")
                        .setBody(
                            """
                            {
                              "result": {
                                "auth_url": "https://auth.openai.com/login?flow=generic",
                                "login_id": "login-generic",
                                "code_verifier": "verifier-generic"
                              }
                            }
                            """.trimIndent()
                        )
                    "/oauth/codex/exchange" -> MockResponse()
                        .setResponseCode(404)
                        .setBody("""{"error":"missing"}""")
                    "/oauth/exchange" -> MockResponse()
                        .setResponseCode(200)
                        .addHeader("Content-Type", "application/json")
                        .setBody(
                            """
                            {
                              "result": {
                                "access_token": "sess-generic-token"
                              }
                            }
                            """.trimIndent()
                        )
                    else -> MockResponse().setResponseCode(404)
                }
            }
        }

        val start = client.startLogin(
            bridgeBaseUrl = bridgeBaseUrl(),
            redirectUri = "citros://oauth/callback",
            state = "state-generic"
        )
        assertTrue(start.isSuccess)
        assertEquals("https://auth.openai.com/login?flow=generic", start.getOrNull()?.authUrl)
        assertEquals("login-generic", start.getOrNull()?.loginId)

        val exchange = client.exchangeCode(
            bridgeBaseUrl = bridgeBaseUrl(),
            code = "code-generic",
            state = "state-generic",
            loginId = start.getOrNull()?.loginId,
            codeVerifier = start.getOrNull()?.codeVerifier
        )
        assertTrue(exchange.isSuccess)
        assertEquals("sess-generic-token", exchange.getOrNull()?.accessToken)

        val paths = listOf(
            server.takeRequest().path,
            server.takeRequest().path,
            server.takeRequest().path,
            server.takeRequest().path
        )
        assertEquals(
            listOf(
                "/oauth/codex/start",
                "/oauth/start",
                "/oauth/codex/exchange",
                "/oauth/exchange"
            ),
            paths
        )
    }

    @Test
    fun `full flow falls back to legacy account login paths`() = runTest {
        server.dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse {
                return when (request.path) {
                    "/oauth/codex/start",
                    "/oauth/start" -> MockResponse().setResponseCode(404).setBody("""{"error":"missing"}""")
                    "/account/login/start" -> MockResponse()
                        .setResponseCode(200)
                        .addHeader("Content-Type", "application/json")
                        .setBody(
                            """
                            {
                              "data": {
                                "url": "https://auth.openai.com/login?flow=legacy",
                                "request_id": "legacy-request-1"
                              }
                            }
                            """.trimIndent()
                        )
                    "/oauth/codex/exchange",
                    "/oauth/exchange" -> MockResponse().setResponseCode(404).setBody("""{"error":"missing"}""")
                    "/account/login/exchange" -> MockResponse()
                        .setResponseCode(200)
                        .addHeader("Content-Type", "application/json")
                        .setBody("""{"data":{"token":"sess-legacy-token"}}""")
                    else -> MockResponse().setResponseCode(404)
                }
            }
        }

        val start = client.startLogin(
            bridgeBaseUrl = bridgeBaseUrl(),
            redirectUri = "citros://oauth/callback",
            state = "state-legacy"
        )
        assertTrue(start.isSuccess)
        assertEquals("https://auth.openai.com/login?flow=legacy", start.getOrNull()?.authUrl)
        assertEquals("legacy-request-1", start.getOrNull()?.loginId)

        val exchange = client.exchangeCode(
            bridgeBaseUrl = bridgeBaseUrl(),
            code = "code-legacy",
            state = "state-legacy",
            loginId = start.getOrNull()?.loginId
        )
        assertTrue(exchange.isSuccess)
        assertEquals("sess-legacy-token", exchange.getOrNull()?.accessToken)

        val paths = List(6) { server.takeRequest().path }
        assertEquals(
            listOf(
                "/oauth/codex/start",
                "/oauth/start",
                "/account/login/start",
                "/oauth/codex/exchange",
                "/oauth/exchange",
                "/account/login/exchange"
            ),
            paths
        )
    }

    @Test
    fun `full flow surfaces aggregate error when all endpoints fail`() = runTest {
        server.dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse {
                return MockResponse()
                    .setResponseCode(500)
                    .addHeader("Content-Type", "application/json")
                    .setBody("""{"error":"bridge unavailable"}""")
            }
        }

        val result = client.startLogin(
            bridgeBaseUrl = bridgeBaseUrl(),
            redirectUri = "citros://oauth/callback",
            state = "state-fail"
        )

        assertTrue(result.isFailure)
        val message = result.exceptionOrNull()?.message
        assertNotNull(message)
        assertTrue(message.contains("/oauth/codex/start"))
        assertTrue(message.contains("/oauth/start"))
        assertTrue(message.contains("/account/login/start"))
    }
}
