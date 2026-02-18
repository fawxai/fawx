package ai.citros.core

import android.util.Log
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.*
import okhttp3.FormBody
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.CertificatePinner
import okhttp3.OkHttpClient
import okhttp3.Request
import java.util.concurrent.TimeUnit

/**
 * Web search client with pluggable provider support.
 *
 * Supports three providers (tried in order):
 * 1. **DuckDuckGo Lite** (default): No API key needed. Scrapes DDG Lite HTML.
 *    Works best from mobile IPs. Free, private, zero config.
 * 2. **SearXNG** (optional): Self-hosted meta-search, no API key needed.
 *    Set searxngBaseUrl to enable.
 * 3. **Brave** (optional): Brave Search API, requires API key.
 *    Free tier: 2,000 queries/month. Set braveApiKey to enable.
 *
 * @param searxngBaseUrl Base URL for SearXNG instance (e.g., "http://localhost:8888")
 * @param braveApiKey Optional Brave Search API key for fallback
 */
class WebSearchClient(
    private val searxngBaseUrl: String? = null,
    private val braveApiKey: String? = null,
    /** Brave Search API endpoint. Override for testing. */
    private val braveEndpoint: String = BRAVE_ENDPOINT,
    /** DuckDuckGo Lite endpoint. Override for testing. Set to null to skip DDG. */
    private val ddgEndpoint: String? = DDG_LITE_URL
) {
    companion object {
        private const val TAG = "WebSearchClient"
        private const val BRAVE_ENDPOINT = "https://api.search.brave.com/res/v1/web/search"
        private const val DDG_LITE_URL = "https://lite.duckduckgo.com/lite/"
        private const val DDG_USER_AGENT = "Mozilla/5.0 (Linux; Android 16; Pixel 9) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Mobile Safari/537.36"
        private const val DEFAULT_COUNT = 3
        private const val MAX_COUNT = 5
        private const val TIMEOUT_SECONDS = 10L
    }

    /**
     * Certificate pinner for Brave Search API.
     * Pins ISRG Root X1 (expires 2035) -- stable root CA used by Let's Encrypt.
     * SearXNG and DuckDuckGo requests are not pinned.
     */
    private val braveCertPinner = CertificatePinner.Builder()
        .add("api.search.brave.com", "sha256/C5+lpZ7tcVwmwQIMcRtPbsQtWLABXhQzejna0wHFr8M=")
        .build()

    private val httpClient = OkHttpClient.Builder()
        .connectTimeout(TIMEOUT_SECONDS, TimeUnit.SECONDS)
        .readTimeout(TIMEOUT_SECONDS, TimeUnit.SECONDS)
        .certificatePinner(braveCertPinner)
        .build()

    /** Separate client without cert pinning for non-Brave providers. */
    private val plainHttpClient = OkHttpClient.Builder()
        .connectTimeout(TIMEOUT_SECONDS, TimeUnit.SECONDS)
        .readTimeout(TIMEOUT_SECONDS, TimeUnit.SECONDS)
        .build()

    private val json = Json { ignoreUnknownKeys = true }

    /**
     * Search the web and return formatted results.
     *
     * Tries DuckDuckGo first (no config needed), then SearXNG, then Brave.
     *
     * @param query Search query string
     * @param count Number of results to return (1-5, default 3)
     * @return ToolResult with formatted search results or error
     */
    suspend fun search(query: String, count: Int = DEFAULT_COUNT): ToolResult {
        if (query.isBlank()) {
            return ToolResult("Search query cannot be empty", isError = true)
        }
        val clampedCount = count.coerceIn(1, MAX_COUNT)

        // 1. Try DuckDuckGo Lite (always available, no config needed)
        if (ddgEndpoint == null) Log.d(TAG, "DuckDuckGo disabled")
        val ddgResult = if (ddgEndpoint != null) searchDuckDuckGo(query, clampedCount) else null
        if (ddgResult != null && !ddgResult.isError) return ddgResult
        if (ddgResult != null) Log.w(TAG, "DuckDuckGo search failed: ${ddgResult.text}")

        // 2. Try SearXNG if configured
        if (searxngBaseUrl != null) {
            val result = searchSearXNG(query, clampedCount)
            if (!result.isError) return result
            Log.w(TAG, "SearXNG search failed: ${result.text}")
        }

        // 3. Try Brave if API key provided
        if (braveApiKey != null) {
            return searchBrave(query, clampedCount)
        }

        // All providers failed
        return ToolResult(
            "Web search failed. DuckDuckGo returned no results and no fallback provider is configured.",
            isError = true
        )
    }

    private suspend fun searchDuckDuckGo(query: String, count: Int): ToolResult {
        return withContext(Dispatchers.IO) {
            try {
                val formBody = FormBody.Builder()
                    .add("q", query)
                    .build()

                val request = Request.Builder()
                    .url(ddgEndpoint!!)
                    .post(formBody)
                    .header("User-Agent", DDG_USER_AGENT)
                    .header("Accept", "text/html")
                    .build()

                plainHttpClient.newCall(request).execute().use { response ->
                    if (!response.isSuccessful) {
                        return@withContext ToolResult(
                            "DuckDuckGo search failed (${response.code}): ${response.message}",
                            isError = true
                        )
                    }

                    val body = response.body?.string() ?: return@withContext ToolResult(
                        "DuckDuckGo returned empty response",
                        isError = true
                    )

                    val results = parseDuckDuckGoResults(body, count)
                    if (results.isEmpty()) {
                        return@withContext ToolResult(
                            "DuckDuckGo returned no results (may be rate-limited)",
                            isError = true
                        )
                    }
                    ToolResult(formatResults(query, results))
                }
            } catch (e: Exception) {
                Log.e(TAG, "DuckDuckGo search error: ${e.message}")
                ToolResult("DuckDuckGo search failed: ${e.message?.take(100)}", isError = true)
            }
        }
    }

    private suspend fun searchSearXNG(query: String, count: Int): ToolResult {
        return withContext(Dispatchers.IO) {
            try {
                val httpUrl = (searxngBaseUrl.orEmpty().trimEnd('/') + "/search")
                    .toHttpUrl()
                    .newBuilder()
                    .addQueryParameter("q", query)
                    .addQueryParameter("format", "json")
                    .addQueryParameter("pageno", "1")
                    .build()

                val request = Request.Builder()
                    .url(httpUrl)
                    .header("Accept", "application/json")
                    .get()
                    .build()

                plainHttpClient.newCall(request).execute().use { response ->
                    if (!response.isSuccessful) {
                        return@withContext ToolResult(
                            "SearXNG search failed (${response.code}): ${response.message}",
                            isError = true
                        )
                    }

                    val body = response.body?.string() ?: return@withContext ToolResult(
                        "SearXNG returned empty response",
                        isError = true
                    )

                    val results = parseSearXNGResults(body, count)
                    ToolResult(formatResults(query, results))
                }
            } catch (e: Exception) {
                Log.e(TAG, "SearXNG search error: ${e.message}")
                ToolResult("SearXNG search failed: ${e.message?.take(100)}", isError = true)
            }
        }
    }

    private suspend fun searchBrave(query: String, count: Int): ToolResult {
        return withContext(Dispatchers.IO) {
            try {
                val httpUrl = braveEndpoint.toHttpUrl()
                    .newBuilder()
                    .addQueryParameter("q", query)
                    .addQueryParameter("count", count.toString())
                    .build()

                val request = Request.Builder()
                    .url(httpUrl)
                    .header("Accept", "application/json")
                    .header("X-Subscription-Token", braveApiKey!!)
                    .get()
                    .build()

                httpClient.newCall(request).execute().use { response ->
                    if (!response.isSuccessful) {
                        return@withContext ToolResult(
                            "Brave search failed (${response.code}): ${response.message}",
                            isError = true
                        )
                    }

                    val body = response.body?.string() ?: return@withContext ToolResult(
                        "Brave search returned empty response",
                        isError = true
                    )

                    val results = parseBraveResults(body, count)
                    ToolResult(formatResults(query, results))
                }
            } catch (e: Exception) {
                Log.e(TAG, "Brave search error: ${e.message}")
                ToolResult("Brave search failed: ${e.message?.take(100)}", isError = true)
            }
        }
    }

    /**
     * Parse DuckDuckGo Lite HTML response into search results.
     *
     * Extracts links with class `result-link` and snippets with class `result-snippet`.
     * Skips ad links (proxied through `duckduckgo.com/y.js`) and keeps snippet indices
     * aligned when ads are skipped.
     *
     * @param html Raw HTML from DDG Lite POST response
     * @param count Maximum number of results to return
     * @return Parsed search results, may be empty if DDG returned no organic results
     */
    internal fun parseDuckDuckGoResults(html: String, count: Int): List<SearchResult> {
        val results = mutableListOf<SearchResult>()

        // DDG Lite HTML structure:
        // <a rel="nofollow" href="URL" class='result-link'>TITLE</a>
        // <td class='result-snippet'>SNIPPET</td>
        val linkRegex = Regex("""<a\s+rel="nofollow"\s+href="([^"]+)"\s+class='result-link'>(.+?)</a>""", RegexOption.DOT_MATCHES_ALL)
        val snippetRegex = Regex("""<td\s+class='result-snippet'>\s*(.+?)\s*</td>""", RegexOption.DOT_MATCHES_ALL)

        val links = linkRegex.findAll(html).toList()
        val snippets = snippetRegex.findAll(html).toList()

        var snippetIdx = 0
        for (match in links) {
            val url = match.groupValues[1]
            val rawTitle = match.groupValues[2]

            // Skip ads (DDG proxied URLs) and "more info" links.
            // Ads have their own result-snippet td, so advance snippetIdx to stay aligned.
            if (url.contains("duckduckgo.com/y.js") || rawTitle.trim() == "more info") {
                if (snippetIdx < snippets.size) snippetIdx++
                continue
            }

            val title = rawTitle.replace(Regex("<[^>]+>"), "").trim()

            // Find the next non-empty snippet
            val snippet = if (snippetIdx < snippets.size) {
                snippets[snippetIdx++].groupValues[1]
                    .replace(Regex("<[^>]+>"), "")
                    .trim()
            } else ""

            results.add(SearchResult(
                title = title,
                url = url,
                description = snippet
            ))

            if (results.size >= count) break
        }

        return results
    }

    /**
     * Parse SearXNG JSON response into search results.
     *
     * @param jsonStr Raw JSON string from SearXNG `/search?format=json` endpoint
     * @param count Maximum number of results to return
     * @return Parsed search results from the `results` array
     */
    internal fun parseSearXNGResults(jsonStr: String, count: Int): List<SearchResult> {
        val root = json.parseToJsonElement(jsonStr).jsonObject
        val results = root["results"]?.jsonArray ?: return emptyList()

        return results.take(count).mapNotNull { element ->
            val obj = element.jsonObject
            val title = obj["title"]?.jsonPrimitive?.contentOrNull ?: return@mapNotNull null
            val url = obj["url"]?.jsonPrimitive?.contentOrNull ?: return@mapNotNull null
            val description = obj["content"]?.jsonPrimitive?.contentOrNull ?: ""
            SearchResult(
                title = title.trim(),
                url = url.trim(),
                description = description.trim()
            )
        }
    }

    /**
     * Parse Brave Search API JSON response into search results.
     *
     * @param jsonStr Raw JSON string from Brave `/res/v1/web/search` endpoint
     * @param count Maximum number of results to return
     * @return Parsed search results from `web.results` array
     */
    internal fun parseBraveResults(jsonStr: String, count: Int): List<SearchResult> {
        val root = json.parseToJsonElement(jsonStr).jsonObject
        val webResults = root["web"]?.jsonObject?.get("results")?.jsonArray ?: return emptyList()

        return webResults.take(count).mapNotNull { element ->
            val obj = element.jsonObject
            val title = obj["title"]?.jsonPrimitive?.contentOrNull ?: return@mapNotNull null
            val url = obj["url"]?.jsonPrimitive?.contentOrNull ?: return@mapNotNull null
            val description = obj["description"]?.jsonPrimitive?.contentOrNull ?: ""
            SearchResult(
                title = title.trim(),
                url = url.trim(),
                description = description.trim()
            )
        }
    }

    internal fun formatResults(query: String, results: List<SearchResult>): String {
        if (results.isEmpty()) {
            return "No results found for: $query"
        }
        return buildString {
            appendLine("Search results for: $query")
            appendLine()
            results.forEachIndexed { index, result ->
                appendLine("${index + 1}. ${result.title}")
                appendLine("   ${result.url}")
                if (result.description.isNotBlank()) {
                    appendLine("   ${result.description}")
                }
                if (index < results.lastIndex) appendLine()
            }
        }.trimEnd()
    }

    /**
     * A single search result.
     */
    data class SearchResult(
        val title: String,
        val url: String,
        val description: String
    )
}
