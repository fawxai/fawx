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
}
