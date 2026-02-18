package ai.citros.core

import kotlinx.coroutines.runBlocking
import okhttp3.OkHttpClient
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.After
import org.junit.Before
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * Unit tests for DeviceCodeAuthClient.
 *
 * Uses MockWebServer to simulate OpenAI's custom device auth endpoints.
 */
class DeviceCodeAuthClientTest {
    private lateinit var server: MockWebServer
    private lateinit var client: DeviceCodeAuthClient

    @Before
    fun setup() {
        server = MockWebServer()
        server.start()
        client = DeviceCodeAuthClient(
            httpClient = OkHttpClient(),
            authBaseUrl = server.url("").toString().trimEnd('/')
        )
    }

    @After
    fun teardown() {
        server.shutdown()
    }

    @Test
    fun testConstantsAreCorrect() {
        assertEquals(
            "app_EMoamEEZ73f0CkXaXp7hrann",
            DeviceCodeAuthClient.DEFAULT_CLIENT_ID
        )
        assertEquals(
            "https://auth.openai.com",
            DeviceCodeAuthClient.DEFAULT_AUTH_BASE_URL
        )
        assertEquals(
            "https://auth.openai.com/codex/device",
            DeviceCodeAuthClient.DEFAULT_VERIFICATION_URL
        )
    }

    @Test
    fun testRequestDeviceCodeParsesValidResponse() {
        server.enqueue(
            MockResponse()
                .setBody("""{"device_auth_id": "dauth_abc123", "user_code": "ABCD-1234", "interval": "5"}""")
                .setResponseCode(200)
                .setHeader("Content-Type", "application/json")
        )

        val result = runBlocking {
            client.requestDeviceCode("test-client")
        }

        assertTrue(result.isSuccess)
        val response = result.getOrThrow()
        assertEquals("dauth_abc123", response.deviceAuthId)
        assertEquals("ABCD-1234", response.userCode)
        assertEquals(5L, response.interval)
        assertEquals(DeviceCodeAuthClient.DEFAULT_VERIFICATION_URL, response.verificationUri)
    }

    @Test
    fun testRequestDeviceCodeSendsJsonBody() {
        server.enqueue(
            MockResponse()
                .setBody("""{"device_auth_id": "dauth_123", "user_code": "TEST-CODE", "interval": "5"}""")
                .setResponseCode(200)
        )

        runBlocking {
            client.requestDeviceCode("my-client-id")
        }

        val request = server.takeRequest()
        assertEquals("POST", request.method)
        assertEquals("/api/accounts/deviceauth/usercode", request.path)
        val body = request.body.readUtf8()
        assertTrue(body.contains("\"client_id\""))
        assertTrue(body.contains("my-client-id"))
    }

    @Test
    fun testRequestDeviceCodeHandles404NotEnabled() {
        server.enqueue(
            MockResponse()
                .setBody("""{"error": {"message": "Invalid"}}""")
                .setResponseCode(404)
        )

        val result = runBlocking {
            client.requestDeviceCode("test-client")
        }

        assertTrue(result.isFailure)
        val exception = result.exceptionOrNull()
        assertTrue(exception is DeviceCodeAuthClient.DeviceCodeException)
        assertTrue(
            exception!!.message!!.contains("not enabled"),
            "Should mention device code not enabled, got: ${exception.message}"
        )
    }

    @Test
    fun testRequestDeviceCodeHandlesServerError() {
        server.enqueue(
            MockResponse()
                .setBody("""{"error": "server_error"}""")
                .setResponseCode(500)
        )

        val result = runBlocking {
            client.requestDeviceCode("test-client")
        }

        assertTrue(result.isFailure)
        val exception = result.exceptionOrNull()
        assertTrue(exception is DeviceCodeAuthClient.DeviceCodeException)
        assertTrue(exception!!.message!!.contains("HTTP 500"))
    }

    @Test
    fun testRequestDeviceCodeHandlesNetworkError() {
        val tempServer = MockWebServer()
        tempServer.start()
        val tempClient = DeviceCodeAuthClient(
            httpClient = OkHttpClient(),
            authBaseUrl = tempServer.url("").toString().trimEnd('/')
        )
        tempServer.shutdown()

        val result = runBlocking {
            tempClient.requestDeviceCode("test-client")
        }

        assertTrue(result.isFailure)
    }

    @Test
    fun testRequestDeviceCodeHandlesMalformedResponse() {
        server.enqueue(
            MockResponse()
                .setBody("not json at all")
                .setResponseCode(200)
        )

        val result = runBlocking {
            client.requestDeviceCode("test-client")
        }

        assertTrue(result.isFailure)
        val exception = result.exceptionOrNull()
        assertTrue(exception is DeviceCodeAuthClient.DeviceCodeException)
        assertTrue(exception!!.message!!.contains("parse"))
    }

    @Test
    fun testRequestDeviceCodeDefaultsIntervalWhenMissing() {
        server.enqueue(
            MockResponse()
                .setBody("""{"device_auth_id": "dauth_x", "user_code": "CODE-123"}""")
                .setResponseCode(200)
        )

        val result = runBlocking {
            client.requestDeviceCode("test-client")
        }

        assertTrue(result.isSuccess)
        assertEquals(DeviceCodeAuthClient.DEFAULT_POLL_INTERVAL_SECONDS, result.getOrThrow().interval)
    }

    @Test
    fun testPollForTokenReturnsSuccessOnApproval() {
        // First poll: 403 (pending)
        server.enqueue(MockResponse().setResponseCode(403))
        // Second poll: success with auth code
        server.enqueue(
            MockResponse()
                .setBody("""{
                    "authorization_code": "authcode_xyz",
                    "code_challenge": "challenge_abc",
                    "code_verifier": "verifier_123"
                }""")
                .setResponseCode(200)
        )

        val result = runBlocking {
            client.pollForToken("dauth_123", "USER-CODE", 1L)
        }

        assertTrue(result is DeviceCodeAuthClient.PollResult.Success)
        val success = result as DeviceCodeAuthClient.PollResult.Success
        assertEquals("authcode_xyz", success.authCode)
        assertEquals("verifier_123", success.codeVerifier)
        assertEquals("challenge_abc", success.codeChallenge)
        assertEquals(2, success.diagnostics.attempts)
        assertEquals(1, success.diagnostics.pending403Count)
        assertEquals(0, success.diagnostics.pending404Count)
    }

    @Test
    fun testPollForTokenSendsCorrectRequest() {
        server.enqueue(
            MockResponse()
                .setBody("""{
                    "authorization_code": "auth_ok",
                    "code_challenge": "ch",
                    "code_verifier": "cv"
                }""")
                .setResponseCode(200)
        )

        runBlocking {
            client.pollForToken("my-device-auth-id", "MY-CODE", 1L)
        }

        val request = server.takeRequest()
        assertEquals("POST", request.method)
        assertEquals("/api/accounts/deviceauth/token", request.path)
        val body = request.body.readUtf8()
        assertTrue(body.contains("my-device-auth-id"))
        assertTrue(body.contains("MY-CODE"))
    }

    @Test
    fun testPollForTokenContinuesOn403() {
        // 403 = pending
        server.enqueue(MockResponse().setResponseCode(403))
        // Then success
        server.enqueue(
            MockResponse()
                .setBody("""{"authorization_code": "ok", "code_challenge": "c", "code_verifier": "v"}""")
                .setResponseCode(200)
        )

        val result = runBlocking {
            client.pollForToken("dauth", "code", 1L)
        }

        assertTrue(result is DeviceCodeAuthClient.PollResult.Success)
        assertEquals(2, server.requestCount)
        val success = result as DeviceCodeAuthClient.PollResult.Success
        assertEquals(1, success.diagnostics.pending403Count)
        assertEquals(0, success.diagnostics.pending404Count)
    }

    @Test
    fun testPollForTokenContinuesOn404() {
        // 404 = pending (per Codex CLI source)
        server.enqueue(MockResponse().setResponseCode(404))
        // Then success
        server.enqueue(
            MockResponse()
                .setBody("""{"authorization_code": "ok", "code_challenge": "c", "code_verifier": "v"}""")
                .setResponseCode(200)
        )

        val result = runBlocking {
            client.pollForToken("dauth", "code", 1L)
        }

        assertTrue(result is DeviceCodeAuthClient.PollResult.Success)
        assertEquals(2, server.requestCount)
        val success = result as DeviceCodeAuthClient.PollResult.Success
        assertEquals(0, success.diagnostics.pending403Count)
        assertEquals(1, success.diagnostics.pending404Count)
    }

    @Test
    fun testPollForTokenReturnsErrorOnUnexpectedStatus() {
        server.enqueue(MockResponse().setResponseCode(500).setBody("server error"))

        val result = runBlocking {
            client.pollForToken("dauth", "code", 1L)
        }

        assertTrue(result is DeviceCodeAuthClient.PollResult.Error)
        val error = result as DeviceCodeAuthClient.PollResult.Error
        assertEquals("http_500", error.error)
        assertEquals(500, error.diagnostics?.lastHttpStatus)
        assertEquals(1, error.diagnostics?.attempts)
    }

    @Test
    fun testPollForTokenHandlesMalformedSuccessResponse() {
        server.enqueue(
            MockResponse()
                .setBody("not json")
                .setResponseCode(200)
        )

        val result = runBlocking {
            client.pollForToken("dauth", "code", 1L)
        }

        assertTrue(result is DeviceCodeAuthClient.PollResult.Error)
        val error = result as DeviceCodeAuthClient.PollResult.Error
        assertEquals("parse_error", error.error)
        assertEquals(1, error.diagnostics?.attempts)
        assertEquals(200, error.diagnostics?.lastHttpStatus)
    }

    @Test
    fun testPollForTokenContinuesOnNetworkError() {
        // First: disconnect
        server.enqueue(
            MockResponse().setSocketPolicy(okhttp3.mockwebserver.SocketPolicy.DISCONNECT_AT_START)
        )
        // Second: success
        server.enqueue(
            MockResponse()
                .setBody("""{"authorization_code": "ok", "code_challenge": "c", "code_verifier": "v"}""")
                .setResponseCode(200)
        )

        val result = runBlocking {
            client.pollForToken("dauth", "code", 1L)
        }

        assertTrue(result is DeviceCodeAuthClient.PollResult.Success)
        val success = result as DeviceCodeAuthClient.PollResult.Success
        assertTrue(success.diagnostics.networkErrorCount >= 1)
    }

    @Test
    fun testPollForTokenTimeoutIncludesDiagnostics() {
        val shortTimeoutClient = DeviceCodeAuthClient(
            httpClient = OkHttpClient(),
            authBaseUrl = server.url("").toString().trimEnd('/'),
            maxWaitSeconds = 1L
        )

        val result = runBlocking {
            shortTimeoutClient.pollForToken("dauth", "code", 1L)
        }

        assertTrue(result is DeviceCodeAuthClient.PollResult.Error)
        val error = result as DeviceCodeAuthClient.PollResult.Error
        assertEquals("timeout", error.error)
        assertEquals(0, error.diagnostics?.attempts)
        assertTrue((error.diagnostics?.elapsedSeconds ?: 0) >= 1)
    }

    @Test
    fun testExchangeCodeSuccess() {
        server.enqueue(
            MockResponse()
                .setBody("""{
                    "access_token": "sk-proj-abc123",
                    "id_token": "eyJ...",
                    "refresh_token": "refresh-xyz",
                    "token_type": "Bearer"
                }""")
                .setResponseCode(200)
        )

        val result = runBlocking {
            client.exchangeCode(
                clientId = "test-client",
                authCode = "authcode_xyz",
                codeVerifier = "verifier_123"
            )
        }

        assertTrue(result.isSuccess)
        val tokens = result.getOrThrow()
        assertEquals("sk-proj-abc123", tokens.accessToken)
        assertEquals("eyJ...", tokens.idToken)
        assertEquals("refresh-xyz", tokens.refreshToken)
    }

    @Test
    fun testExchangeCodeSendsCorrectRequest() {
        server.enqueue(
            MockResponse()
                .setBody("""{"access_token": "tok", "id_token": "id", "refresh_token": "ref"}""")
                .setResponseCode(200)
        )

        runBlocking {
            client.exchangeCode(
                clientId = "my-client",
                authCode = "my-auth-code",
                codeVerifier = "my-verifier"
            )
        }

        val request = server.takeRequest()
        assertEquals("POST", request.method)
        assertEquals("/oauth/token", request.path)
        val body = request.body.readUtf8()
        assertTrue(body.contains("grant_type=authorization_code"))
        assertTrue(body.contains("client_id=my-client"))
        assertTrue(body.contains("code=my-auth-code"))
        assertTrue(body.contains("code_verifier=my-verifier"))
        assertTrue(body.contains("redirect_uri="))
    }

    @Test
    fun testExchangeCodeHandlesFailure() {
        server.enqueue(
            MockResponse()
                .setBody("""{"error": "invalid_grant"}""")
                .setResponseCode(400)
        )

        val result = runBlocking {
            client.exchangeCode(
                clientId = "test-client",
                authCode = "bad-code",
                codeVerifier = "v"
            )
        }

        assertTrue(result.isFailure)
        val exception = result.exceptionOrNull()
        assertTrue(exception is DeviceCodeAuthClient.DeviceCodeException)
        assertTrue(exception!!.message!!.contains("HTTP 400"))
    }

    @Test
    fun testExchangeCodeHandlesNullOptionalFields() {
        server.enqueue(
            MockResponse()
                .setBody("""{"access_token": "tok123"}""")
                .setResponseCode(200)
        )

        val result = runBlocking {
            client.exchangeCode(
                clientId = "test-client",
                authCode = "code",
                codeVerifier = "v"
            )
        }

        assertTrue(result.isSuccess)
        val tokens = result.getOrThrow()
        assertEquals("tok123", tokens.accessToken)
        assertEquals(null, tokens.idToken)
        assertEquals(null, tokens.refreshToken)
    }
}
