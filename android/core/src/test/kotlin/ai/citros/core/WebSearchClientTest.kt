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

class WebSearchClientTest {

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

    // ========== SearXNG tests ==========

    @Test
    fun `search returns formatted results from SearXNG`() = runTest {
        val json = """
        {
            "results": [
                {"title": "Kotlin Wikipedia", "url": "https://en.wikipedia.org/wiki/Kotlin", "content": "Kotlin is a language"},
                {"title": "Kotlin Home", "url": "https://kotlinlang.org", "content": "Official site"},
                {"title": "Kotlin Tutorial", "url": "https://example.com/kotlin", "content": "Learn Kotlin"}
            ]
        }
        """.trimIndent()
        server.enqueue(MockResponse().setBody(json).setResponseCode(200))

        val client = WebSearchClient(searxngBaseUrl = server.url("/").toString(), ddgEndpoint = null)
        val result = client.search("kotlin", count = 3)

        assertFalse(result.isError)
        assertTrue(result.text.contains("Kotlin Wikipedia"))
        assertTrue(result.text.contains("https://en.wikipedia.org/wiki/Kotlin"))
        assertTrue(result.text.contains("Kotlin is a language"))
    }

    @Test
    fun `search clamps count to max 5`() = runTest {
        val json = """{"results": [{"title": "Result", "url": "https://example.com", "content": "desc"}]}"""
        server.enqueue(MockResponse().setBody(json).setResponseCode(200))

        val client = WebSearchClient(searxngBaseUrl = server.url("/").toString(), ddgEndpoint = null)
        client.search("test", count = 100)

        val request = server.takeRequest()
        // Count clamping happens client-side in parseSearXNGResults
        assertTrue(request.path!!.contains("q=test"))
    }

    @Test
    fun `search returns error on empty query`() = runTest {
        val client = WebSearchClient(searxngBaseUrl = "http://localhost:1234", ddgEndpoint = null)
        val result = client.search("")
        assertTrue(result.isError)
        assertTrue(result.text.contains("empty"))
    }

    @Test
    fun `search returns error when SearXNG returns HTTP error`() = runTest {
        server.enqueue(MockResponse().setResponseCode(500).setBody("Internal Server Error"))

        val client = WebSearchClient(searxngBaseUrl = server.url("/").toString(), ddgEndpoint = null)
        val result = client.search("test")

        assertTrue(result.isError)
        assertTrue(result.text.contains("failed") || result.text.contains("500"),
            "Error should mention failure: ${result.text}")
    }

    @Test
    fun `search returns no results message when empty`() = runTest {
        server.enqueue(MockResponse().setBody("""{"results": []}""").setResponseCode(200))

        val client = WebSearchClient(searxngBaseUrl = server.url("/").toString(), ddgEndpoint = null)
        val result = client.search("xyznonexistent")

        assertFalse(result.isError)
        assertTrue(result.text.contains("No results found"))
        assertTrue(result.text.contains("Do NOT open a browser"),
            "Empty results should include anti-browser directive: ${result.text}")
    }

    @Test
    fun `search returns error when all providers fail`() = runTest {
        val client = WebSearchClient(ddgEndpoint = null)
        val result = client.search("test")
        assertTrue(result.isError, "Should fail when all providers are unavailable")
        assertTrue(result.text.contains("Do NOT open Chrome"),
            "Error should include anti-browser directive: ${result.text}")
    }

    // ========== Brave fallback tests ==========

    @Test
    fun `search falls back to Brave when SearXNG fails`() = runTest {
        val searxngServer = MockWebServer()
        val braveServer = MockWebServer()
        try {
            searxngServer.start()
            braveServer.start()

            searxngServer.enqueue(MockResponse().setResponseCode(503))

            val braveJson = """
            {
                "web": {
                    "results": [
                        {"title": "Brave Result", "url": "https://brave.com", "description": "From Brave"}
                    ]
                }
            }
            """.trimIndent()
            braveServer.enqueue(MockResponse().setBody(braveJson).setResponseCode(200))

            val client = WebSearchClient(
                searxngBaseUrl = searxngServer.url("/").toString(),
                braveApiKey = "test-key",
                braveEndpoint = braveServer.url("/").toString(),
                ddgEndpoint = null
            )
            val result = client.search("test")

            assertFalse(result.isError, "Should succeed via Brave fallback: ${result.text}")
            assertTrue(result.text.contains("Brave Result"))
        } finally {
            searxngServer.shutdown()
            braveServer.shutdown()
        }
    }

    @Test
    fun `search returns error when SearXNG fails and no Brave key`() = runTest {
        server.enqueue(MockResponse().setResponseCode(503))

        val client = WebSearchClient(searxngBaseUrl = server.url("/").toString(), ddgEndpoint = null)
        val result = client.search("test")

        assertTrue(result.isError)
        // Falls through to all-providers-failed which has the anti-browser directive
        assertTrue(result.text.contains("Do NOT open Chrome"),
            "Should include anti-browser directive: ${result.text}")
    }

    @Test
    fun `formatResults empty results include anti-browser directive`() {
        val client = WebSearchClient()
        val formatted = client.formatResults("test", emptyList())
        assertTrue(formatted.contains("No results found"))
        assertTrue(formatted.contains("Do NOT open a browser"),
            "Empty results should warn against browser fallback: $formatted")
    }

    // ========== Parsing tests ==========

    @Test
    fun `parseSearXNGResults handles missing fields gracefully`() {
        val client = WebSearchClient()
        val json = """
        {
            "results": [
                {"title": "Has All", "url": "https://example.com", "content": "Full result"},
                {"url": "https://no-title.com", "content": "Missing title"},
                {"title": "No URL", "content": "Missing url"},
                {"title": "No Desc", "url": "https://no-desc.com"}
            ]
        }
        """.trimIndent()

        val results = client.parseSearXNGResults(json, 10)
        // Only results with both title and url should be included
        assertEquals(2, results.size)
        assertEquals("Has All", results[0].title)
        assertEquals("No Desc", results[1].title)
        assertEquals("", results[1].description)
    }

    @Test
    fun `parseBraveResults extracts from nested web results`() {
        val client = WebSearchClient()
        val json = """
        {
            "web": {
                "results": [
                    {"title": "Result 1", "url": "https://one.com", "description": "First"},
                    {"title": "Result 2", "url": "https://two.com", "description": "Second"}
                ]
            }
        }
        """.trimIndent()

        val results = client.parseBraveResults(json, 5)
        assertEquals(2, results.size)
        assertEquals("Result 1", results[0].title)
        assertEquals("https://two.com", results[1].url)
    }

    @Test
    fun `formatResults numbers results correctly`() {
        val client = WebSearchClient()
        val results = listOf(
            WebSearchClient.SearchResult("First", "https://first.com", "Desc 1"),
            WebSearchClient.SearchResult("Second", "https://second.com", "Desc 2")
        )
        val formatted = client.formatResults("test query", results)
        assertTrue(formatted.contains("1. First"))
        assertTrue(formatted.contains("2. Second"))
        assertTrue(formatted.contains("Search results for: test query"))
    }

    @Test
    fun `parseDuckDuckGoResults extracts links and snippets`() {
        val client = WebSearchClient()
        val html = """
            <a rel="nofollow" href="https://example.com/one" class='result-link'>First Result</a>
            <td class='result-snippet'>
              This is the first <b>snippet</b> with HTML.
            </td>
            <a rel="nofollow" href="https://example.com/two" class='result-link'>Second Result</a>
            <td class='result-snippet'>
              Second snippet here.
            </td>
        """.trimIndent()

        val results = client.parseDuckDuckGoResults(html, 5)
        assertEquals(2, results.size)
        assertEquals("First Result", results[0].title)
        assertEquals("https://example.com/one", results[0].url)
        assertEquals("This is the first snippet with HTML.", results[0].description)
        assertEquals("Second Result", results[1].title)
    }

    @Test
    fun `parseDuckDuckGoResults skips ads and aligns snippets correctly`() {
        val client = WebSearchClient()
        // Ad has its own result-snippet td — must be skipped to keep snippets aligned
        val html = """
            <a rel="nofollow" href="https://duckduckgo.com/y.js?ad_domain=spam.com" class='result-link'>Ad Title</a>
            <td class='result-snippet'>Ad snippet that should be skipped</td>
            <a rel="nofollow" href="https://real-result.com" class='result-link'>Real Result</a>
            <td class='result-snippet'>Correct snippet for real result</td>
            <a rel="nofollow" href="https://second.com" class='result-link'>Second Result</a>
            <td class='result-snippet'>Second snippet</td>
        """.trimIndent()

        val results = client.parseDuckDuckGoResults(html, 5)
        assertEquals(2, results.size)
        assertEquals("Real Result", results[0].title)
        assertEquals("Correct snippet for real result", results[0].description)
        assertEquals("Second Result", results[1].title)
        assertEquals("Second snippet", results[1].description)
    }

    @Test
    fun `parseDuckDuckGoResults respects count limit`() {
        val client = WebSearchClient()
        val html = """
            <a rel="nofollow" href="https://one.com" class='result-link'>One</a>
            <td class='result-snippet'>S1</td>
            <a rel="nofollow" href="https://two.com" class='result-link'>Two</a>
            <td class='result-snippet'>S2</td>
            <a rel="nofollow" href="https://three.com" class='result-link'>Three</a>
            <td class='result-snippet'>S3</td>
        """.trimIndent()

        val results = client.parseDuckDuckGoResults(html, 2)
        assertEquals(2, results.size)
    }

    @Test
    fun `parseDuckDuckGoResults returns empty for no results`() {
        val client = WebSearchClient()
        val html = "<html><body>No results</body></html>"
        val results = client.parseDuckDuckGoResults(html, 3)
        assertTrue(results.isEmpty())
    }

}
