package ai.citros.core

import kotlinx.coroutines.test.runTest
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.After
import org.junit.Before
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

class KeyDeliveryClientTest {

    private lateinit var server: MockWebServer

    @Before
    fun setup() {
        server = MockWebServer()
        server.start()
    }

    @After
    fun tearDown() {
        server.shutdown()
    }

    @Test
    fun `fetchKeys returns tinyfish key on success`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"keys": {"tinyfish": "tf-test-key-123", "appToken": "app-token-456"}}""")
            .setResponseCode(200))

        val client = KeyDeliveryClient(endpoint = server.url("/api/keys").toString())
        val keys = client.fetchKeys()

        assertNotNull(keys)
        assertEquals("tf-test-key-123", keys.tinyfish)
        assertEquals("app-token-456", keys.appToken)
    }

    @Test
    fun `fetchKeys returns null on HTTP error`() = runTest {
        server.enqueue(MockResponse().setResponseCode(500))

        val client = KeyDeliveryClient(endpoint = server.url("/api/keys").toString())
        val keys = client.fetchKeys()

        assertNull(keys)
    }

    @Test
    fun `fetchKeys returns null on 401 unauthorized`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"error": "Unauthorized"}""")
            .setResponseCode(401))

        val client = KeyDeliveryClient(
            endpoint = server.url("/api/keys").toString(),
            appToken = "wrong-token"
        )
        val keys = client.fetchKeys()

        assertNull(keys, "Should return null on 401")
    }

    @Test
    fun `fetchKeys sends POST with correct headers`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"keys": {}}""")
            .setResponseCode(200))

        val client = KeyDeliveryClient(endpoint = server.url("/api/keys").toString())
        client.fetchKeys()

        val request = server.takeRequest()
        assertEquals("POST", request.method)
        assertTrue(request.getHeader("Content-Type")?.contains("application/json") == true,
            "Should send Content-Type: application/json, got: ${request.getHeader("Content-Type")}")
        assertEquals("application/json", request.getHeader("Accept"))
        assertTrue(request.getHeader("X-Citros-Client")?.startsWith("android/") == true,
            "Should send X-Citros-Client header, got: ${request.getHeader("X-Citros-Client")}")
    }

    @Test
    fun `fetchKeys handles missing tinyfish key gracefully`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"keys": {}}""")
            .setResponseCode(200))

        val client = KeyDeliveryClient(endpoint = server.url("/api/keys").toString())
        val keys = client.fetchKeys()

        assertNotNull(keys)
        assertNull(keys.tinyfish)
    }

    @Test
    fun `fetchKeys handles malformed JSON gracefully`() = runTest {
        server.enqueue(MockResponse()
            .setBody("not json")
            .setResponseCode(200))

        val client = KeyDeliveryClient(endpoint = server.url("/api/keys").toString())
        val keys = client.fetchKeys()

        assertNull(keys)
    }

    @Test
    fun `fetchKeys handles network error gracefully`() = runTest {
        // Server is shut down — connection will fail
        server.shutdown()

        val client = KeyDeliveryClient(endpoint = "http://localhost:1/api/keys")
        val keys = client.fetchKeys()

        assertNull(keys)
    }

    @Test
    fun `fetchKeys sends Bearer auth header when appToken is provided`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"keys": {"tinyfish": "tf-key"}}""")
            .setResponseCode(200))

        val client = KeyDeliveryClient(
            endpoint = server.url("/api/keys").toString(),
            appToken = "test-app-token-xyz"
        )
        client.fetchKeys()

        val request = server.takeRequest()
        assertEquals("Bearer test-app-token-xyz", request.getHeader("Authorization"))
    }

    @Test
    fun `fetchKeys omits Bearer auth header when appToken is null`() = runTest {
        server.enqueue(MockResponse()
            .setBody("""{"keys": {}}""")
            .setResponseCode(200))

        val client = KeyDeliveryClient(
            endpoint = server.url("/api/keys").toString(),
            appToken = null
        )
        client.fetchKeys()

        val request = server.takeRequest()
        assertNull(request.getHeader("Authorization"))
    }

    @Test
    fun `fetchKeys no longer returns appToken in response`() = runTest {
        // Even if server returns appToken (legacy), client should still parse fine
        // but callers should use BuildConfig.CITROS_APP_TOKEN instead.
        server.enqueue(MockResponse()
            .setBody("""{"keys": {"tinyfish": "tf-key", "appToken": "should-ignore"}}""")
            .setResponseCode(200))

        val client = KeyDeliveryClient(
            endpoint = server.url("/api/keys").toString(),
            appToken = "compiled-token"
        )
        val keys = client.fetchKeys()

        assertNotNull(keys)
        assertEquals("tf-key", keys.tinyfish)
        // appToken field still parsed (backward compat) but callers shouldn't use it
        assertEquals("should-ignore", keys.appToken)
    }
}
