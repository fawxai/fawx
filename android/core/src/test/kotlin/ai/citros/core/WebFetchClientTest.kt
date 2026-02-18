package ai.citros.core

import kotlinx.coroutines.test.runTest
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.After
import org.junit.Before
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class WebFetchClientTest {

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
    fun `fetch extracts text from HTML page`() = runTest {
        val html = """
        <html>
        <head><title>Test Page</title></head>
        <body>
            <nav>Navigation</nav>
            <main>
                <h1>Hello World</h1>
                <p>This is the main content of the page.</p>
            </main>
            <footer>Footer content</footer>
        </body>
        </html>
        """.trimIndent()
        server.enqueue(MockResponse()
            .setBody(html)
            .setHeader("Content-Type", "text/html")
            .setResponseCode(200))

        val client = WebFetchClient()
        val result = client.fetch(server.url("/test").toString())

        assertFalse(result.isError)
        assertTrue(result.text.contains("Test Page"))
        assertTrue(result.text.contains("Hello World"))
        assertTrue(result.text.contains("main content"))
        // Nav and footer should be stripped
        assertFalse(result.text.contains("Navigation"))
        assertFalse(result.text.contains("Footer content"))
    }

    @Test
    fun `fetch returns plain text for non-HTML content`() = runTest {
        val json = """{"key": "value", "items": [1, 2, 3]}"""
        server.enqueue(MockResponse()
            .setBody(json)
            .setHeader("Content-Type", "application/json")
            .setResponseCode(200))

        val client = WebFetchClient()
        val result = client.fetch(server.url("/api").toString())

        assertFalse(result.isError)
        assertTrue(result.text.contains(""""key": "value""""))
    }

    @Test
    fun `fetch truncates large content`() = runTest {
        val longContent = "x".repeat(10000)
        server.enqueue(MockResponse()
            .setBody(longContent)
            .setHeader("Content-Type", "text/plain")
            .setResponseCode(200))

        val client = WebFetchClient()
        val result = client.fetch(server.url("/large").toString(), maxChars = 500)

        assertFalse(result.isError)
        assertTrue(result.text.contains("truncated to 500"))
        // Total length should be URL line + truncation notice + content
        assertTrue(result.text.length < 700)
    }

    @Test
    fun `fetch rejects invalid URL`() = runTest {
        val client = WebFetchClient()

        val result1 = client.fetch("not-a-url")
        assertTrue(result1.isError)
        assertTrue(result1.text.contains("Invalid URL"))

        val result2 = client.fetch("")
        assertTrue(result2.isError)
        assertTrue(result2.text.contains("empty"))
    }

    @Test
    fun `fetch returns error on HTTP error`() = runTest {
        server.enqueue(MockResponse().setResponseCode(404))

        val client = WebFetchClient()
        val result = client.fetch(server.url("/missing").toString())

        assertTrue(result.isError)
        assertTrue(result.text.contains("404"))
    }

    @Test
    fun `fetch clamps maxChars to valid range`() = runTest {
        val content = "Hello World"
        server.enqueue(MockResponse()
            .setBody(content)
            .setHeader("Content-Type", "text/plain")
            .setResponseCode(200))

        val client = WebFetchClient()
        // maxChars below minimum should be clamped to 100
        val result = client.fetch(server.url("/").toString(), maxChars = 1)

        assertFalse(result.isError)
        assertTrue(result.text.contains("Hello World"))
    }

    @Test
    fun `fetch includes URL in output`() = runTest {
        server.enqueue(MockResponse()
            .setBody("Content")
            .setHeader("Content-Type", "text/plain")
            .setResponseCode(200))

        val client = WebFetchClient()
        val url = server.url("/page").toString()
        val result = client.fetch(url)

        assertFalse(result.isError)
        assertTrue(result.text.contains("URL: $url"))
    }

    // ========== extractReadableText tests ==========

    @Test
    fun `extractReadableText strips scripts and styles`() {
        val client = WebFetchClient()
        val html = """
        <html>
        <head>
            <style>body { color: red; }</style>
            <script>alert('xss')</script>
        </head>
        <body>
            <p>Visible content</p>
            <script>more script</script>
        </body>
        </html>
        """.trimIndent()

        val text = client.extractReadableText(html)
        assertTrue(text.contains("Visible content"))
        assertFalse(text.contains("alert"))
        assertFalse(text.contains("color: red"))
    }

    @Test
    fun `extractReadableText prefers main or article content`() {
        val client = WebFetchClient()
        val html = """
        <html>
        <body>
            <aside>Sidebar stuff</aside>
            <main>
                <p>Main content here</p>
            </main>
        </body>
        </html>
        """.trimIndent()

        val text = client.extractReadableText(html)
        assertTrue(text.contains("Main content here"))
        assertFalse(text.contains("Sidebar stuff"))
    }

    @Test
    fun `extractReadableText strips hidden elements`() {
        val client = WebFetchClient()
        val html = """
        <html>
        <body>
            <p>Visible content</p>
            <p hidden>Hidden with attribute</p>
            <p aria-hidden="true">Aria hidden</p>
            <p style="display:none">CSS hidden</p>
        </body>
        </html>
        """.trimIndent()

        val text = client.extractReadableText(html)
        assertTrue(text.contains("Visible content"))
        assertFalse(text.contains("Hidden with attribute"))
        assertFalse(text.contains("Aria hidden"))
        assertFalse(text.contains("CSS hidden"))
    }

    @Test
    fun `extractReadableText returns empty for blank page`() {
        val client = WebFetchClient()
        val html = """
        <html>
        <head><title></title></head>
        <body>
            <script>only scripts here</script>
        </body>
        </html>
        """.trimIndent()

        val text = client.extractReadableText(html)
        assertTrue(text.isBlank())
    }
}
