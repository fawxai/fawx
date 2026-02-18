package ai.citros.core

import kotlinx.serialization.json.*
import kotlinx.coroutines.runBlocking
import org.junit.Assert.*
import org.junit.Test
import java.io.BufferedReader
import java.io.StringReader

class TinyFishClientTest {

    private val client = TinyFishClient(apiKey = "test-key")

    // ── SSE Parsing ─────────────────────────────────────────────────────

    @Test
    fun `parseSSEStream extracts COMPLETE result`() {
        val sse = """
            data: {"type":"STARTED","runId":"run_1","timestamp":"2026-01-01T00:00:00Z"}
            data: {"type":"PROGRESS","runId":"run_1","purpose":"Navigating to page","timestamp":"2026-01-01T00:00:01Z"}
            data: {"type":"PROGRESS","runId":"run_1","purpose":"Clicking search button","timestamp":"2026-01-01T00:00:02Z"}
            data: {"type":"COMPLETE","runId":"run_1","status":"COMPLETED","resultJson":{"price":"$299"},"timestamp":"2026-01-01T00:00:05Z"}
        """.trimIndent()

        val result = runBlocking { client.parseSSEStream(BufferedReader(StringReader(sse))) }

        assertFalse(result.isError)
        assertTrue(result.text.contains("Web automation completed."))
        assertTrue(result.text.contains("Navigating to page"))
        assertTrue(result.text.contains("Clicking search button"))
        assertTrue(result.text.contains("\$299"))
    }

    @Test
    fun `parseSSEStream handles FAILED status`() {
        val sse = """
            data: {"type":"STARTED","runId":"run_2","timestamp":"2026-01-01T00:00:00Z"}
            data: {"type":"COMPLETE","runId":"run_2","status":"FAILED","error":"Page not found","timestamp":"2026-01-01T00:00:03Z"}
        """.trimIndent()

        val result = runBlocking { client.parseSSEStream(BufferedReader(StringReader(sse))) }

        assertTrue(result.isError)
        assertTrue(result.text.contains("Page not found"))
    }

    @Test
    fun `parseSSEStream handles CANCELLED status`() {
        val sse = """
            data: {"type":"STARTED","runId":"run_3","timestamp":"2026-01-01T00:00:00Z"}
            data: {"type":"COMPLETE","runId":"run_3","status":"CANCELLED","timestamp":"2026-01-01T00:00:03Z"}
        """.trimIndent()

        val result = runBlocking { client.parseSSEStream(BufferedReader(StringReader(sse))) }

        assertTrue(result.isError)
        assertTrue(result.text.contains("cancelled"))
    }

    @Test
    fun `parseSSEStream returns error when no COMPLETE event`() {
        val sse = """
            data: {"type":"STARTED","runId":"run_4","timestamp":"2026-01-01T00:00:00Z"}
            data: {"type":"HEARTBEAT","timestamp":"2026-01-01T00:00:05Z"}
        """.trimIndent()

        val result = runBlocking { client.parseSSEStream(BufferedReader(StringReader(sse))) }

        assertTrue(result.isError)
        assertTrue(result.text.contains("without completing"))
    }

    @Test
    fun `parseSSEStream ignores non-data lines`() {
        val sse = """
            : comment line
            event: message
            data: {"type":"STARTED","runId":"run_5","timestamp":"2026-01-01T00:00:00Z"}
            
            data: {"type":"COMPLETE","runId":"run_5","status":"COMPLETED","resultJson":{"ok":true},"timestamp":"2026-01-01T00:00:03Z"}
        """.trimIndent()

        val result = runBlocking { client.parseSSEStream(BufferedReader(StringReader(sse))) }

        assertFalse(result.isError)
        assertTrue(result.text.contains("Web automation completed."))
    }

    @Test
    fun `parseSSEStream reports progress via callback`() {
        val sse = """
            data: {"type":"STARTED","runId":"run_6","timestamp":"2026-01-01T00:00:00Z"}
            data: {"type":"PROGRESS","runId":"run_6","purpose":"Step 1","timestamp":"2026-01-01T00:00:01Z"}
            data: {"type":"PROGRESS","runId":"run_6","purpose":"Step 2","timestamp":"2026-01-01T00:00:02Z"}
            data: {"type":"COMPLETE","runId":"run_6","status":"COMPLETED","resultJson":null,"timestamp":"2026-01-01T00:00:03Z"}
        """.trimIndent()

        val progressLog = mutableListOf<String>()
        val result = runBlocking {
            client.parseSSEStream(
                BufferedReader(StringReader(sse)),
                onProgress = { progressLog.add(it) }
            )
        }

        assertFalse(result.isError)
        assertEquals(listOf("Step 1", "Step 2"), progressLog)
    }

    @Test
    fun `parseSSEStream skips malformed events gracefully`() {
        val sse = """
            data: {"type":"STARTED","runId":"run_7","timestamp":"2026-01-01T00:00:00Z"}
            data: not valid json
            data: {"type":"COMPLETE","runId":"run_7","status":"COMPLETED","resultJson":{"data":"ok"},"timestamp":"2026-01-01T00:00:03Z"}
        """.trimIndent()

        val result = runBlocking { client.parseSSEStream(BufferedReader(StringReader(sse))) }

        assertFalse(result.isError)
        assertTrue(result.text.contains("Web automation completed."))
    }

    // ── Result Formatting ───────────────────────────────────────────────

    @Test
    fun `formatResult with JSON object`() {
        val resultJson = buildJsonObject {
            put("price", "\$299")
            put("inStock", true)
        }

        val formatted = client.formatResult(resultJson, listOf("Navigated to page", "Found product"))

        assertTrue(formatted.contains("Web automation completed."))
        assertTrue(formatted.contains("Navigated to page"))
        assertTrue(formatted.contains("Found product"))
        assertTrue(formatted.contains("\$299"))
        assertTrue(formatted.contains("Result:"))
    }

    @Test
    fun `formatResult with null resultJson`() {
        val formatted = client.formatResult(null, listOf("Did something"))

        assertTrue(formatted.contains("Web automation completed."))
        assertTrue(formatted.contains("Did something"))
        assertFalse(formatted.contains("Result:"))
    }

    @Test
    fun `formatResult with empty progress`() {
        val resultJson = buildJsonObject { put("status", "ok") }
        val formatted = client.formatResult(resultJson, emptyList())

        assertTrue(formatted.contains("Web automation completed."))
        assertFalse(formatted.contains("Steps taken:"))
        assertTrue(formatted.contains("Result:"))
    }

    @Test
    fun `formatResult truncates to MAX_RESULT_CHARS`() {
        val largeJson = buildJsonObject {
            put("data", "x".repeat(20000))
        }

        val sse = """
            data: {"type":"STARTED","runId":"run_8","timestamp":"2026-01-01T00:00:00Z"}
            data: {"type":"COMPLETE","runId":"run_8","status":"COMPLETED","resultJson":{"data":"${
                "x".repeat(20000)
            }"},"timestamp":"2026-01-01T00:00:03Z"}
        """.trimIndent()

        val result = runBlocking { client.parseSSEStream(BufferedReader(StringReader(sse))) }

        assertFalse(result.isError)
        assertTrue(result.text.length <= TinyFishClient.MAX_RESULT_CHARS)
    }


    // ── Input Validation (browse suspend function) ───────────────────

    @Test
    fun `browse with blank URL returns error`() = runBlocking {
        val result = client.browse(url = "", goal = "Find something")
        assertTrue(result.isError)
        assertTrue(result.text.contains("URL cannot be empty"))
    }

    @Test
    fun `browse with blank goal returns error`() = runBlocking {
        val result = client.browse(url = "https://example.com", goal = "")
        assertTrue(result.isError)
        assertTrue(result.text.contains("Goal cannot be empty"))
    }

    @Test
    fun `browse with non-HTTP URL returns error`() = runBlocking {
        val result = client.browse(url = "ftp://example.com", goal = "Find something")
        assertTrue(result.isError)
        assertTrue(result.text.contains("Invalid URL"))
    }

    @Test
    fun `parseSSEStream handles STREAMING_URL event`() {
        val sse = """
            data: {"type":"STARTED","runId":"run_9","timestamp":"2026-01-01T00:00:00Z"}
            data: {"type":"STREAMING_URL","runId":"run_9","streamingUrl":"https://stream.tinyfish.ai/abc","timestamp":"2026-01-01T00:00:01Z"}
            data: {"type":"COMPLETE","runId":"run_9","status":"COMPLETED","resultJson":{"ok":true},"timestamp":"2026-01-01T00:00:05Z"}
        """.trimIndent()

        val result = runBlocking { client.parseSSEStream(BufferedReader(StringReader(sse))) }

        assertFalse(result.isError)
        assertTrue(result.text.contains("Web automation completed."))
    }

    @Test
    fun `parseSSEStream handles complex nested resultJson`() {
        val jsonStr = """data: {"type":"COMPLETE","runId":"run_10","status":"COMPLETED","resultJson":{"products":[{"name":"AirPods Pro","price":"249"},{"name":"AirPods Max","price":"549"}]},"timestamp":"2026-01-01T00:00:05Z"}"""

        val result = runBlocking { client.parseSSEStream(BufferedReader(StringReader(jsonStr))) }

        assertFalse(result.isError)
        assertTrue(result.text.contains("AirPods Pro"))
        assertTrue(result.text.contains("AirPods Max"))
        assertTrue(result.text.contains("249"))
        assertTrue(result.text.contains("549"))
    }
}
