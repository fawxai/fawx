package ai.citros.core

import android.util.Log
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.*
import okhttp3.FormBody
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.CertificatePinner
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.util.concurrent.TimeUnit

/**
 * Web search client with pluggable provider support.
 *
 * Supports four providers (tried in order):
 * 1. **Citros Search** (default): Proxied Brave Search via citros.ai edge function.
 *    No API key needed on device — key lives server-side. Zero config.
 * 2. **DuckDuckGo Lite** (fallback): Scrapes DDG Lite HTML. No API key needed.
 *    Works best from mobile IPs. May be rate-limited.
 * 3. **SearXNG** (optional): Self-hosted meta-search, no API key needed.
 *    Set searxngBaseUrl to enable.
 * 4. **Brave** (optional): Direct Brave Search API, requires user's own API key.
 *    Free tier: 2,000 queries/month. Set braveApiKey to enable.
 *
 * @param citrosSearchEndpoint Citros search proxy URL (null to skip)
 * @param citrosAppToken Bearer token for Citros API auth (null to skip auth)
 * @param searxngBaseUrl Base URL for SearXNG instance (e.g., "http://localhost:8888")
 * @param braveApiKey Optional Brave Search API key for direct Brave access
 * @param domainGuardrailMode Whether to emit tactical anti-browser directives in fallback text
 */
class WebSearchClient(
    private val citrosSearchEndpoint: String? = CITROS_SEARCH_ENDPOINT,
    private val citrosAppToken: String? = null,
    private val searxngBaseUrl: String? = null,
    private val braveApiKey: String? = null,
    private val domainGuardrailMode: PhoneAgentPrompts.DomainGuardrailMode =
        PhoneAgentPrompts.DomainGuardrailMode.GENERIC,
    /** Brave Search API endpoint. Override for testing. */
    private val braveEndpoint: String = BRAVE_ENDPOINT,
    /** DuckDuckGo Lite endpoint. Override for testing. Set to null to skip DDG. */
    private val ddgEndpoint: String? = DDG_LITE_URL
) {
    companion object {
        private const val TAG = "WebSearchClient"
        private const val CITROS_SEARCH_ENDPOINT = "https://citros.ai/api/search"
        private const val BRAVE_ENDPOINT = "https://api.search.brave.com/res/v1/web/search"
        private const val DDG_LITE_URL = "https://lite.duckduckgo.com/lite/"
        private const val DDG_USER_AGENT = "Mozilla/5.0 (Linux; Android 16; Pixel 9) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Mobile Safari/537.36"
        private const val DEFAULT_COUNT = 3
        private const val MAX_COUNT = 5
        private const val TIMEOUT_SECONDS = 10L
    }

    /**
     * Certificate pinner for Let's Encrypt services (Brave + Citros).
     * Pins ISRG Root X1 (expires 2035) -- stable root CA used by Let's Encrypt.
     * Both citros.ai (Vercel) and api.search.brave.com use LE certificates.
     */
    private val letsEncryptCertPinner = CertificatePinner.Builder()
        .add("api.search.brave.com", "sha256/C5+lpZ7tcVwmwQIMcRtPbsQtWLABXhQzejna0wHFr8M=")
        .add("citros.ai", "sha256/C5+lpZ7tcVwmwQIMcRtPbsQtWLABXhQzejna0wHFr8M=")
        .build()

    /** HTTP client with cert pinning for Brave and Citros endpoints. */
    private val httpClient = OkHttpClient.Builder()
        .connectTimeout(TIMEOUT_SECONDS, TimeUnit.SECONDS)
        .readTimeout(TIMEOUT_SECONDS, TimeUnit.SECONDS)
        .certificatePinner(letsEncryptCertPinner)
        .build()

    /** Separate client without cert pinning for third-party providers (SearXNG, DDG). */
    private val plainHttpClient = OkHttpClient.Builder()
        .connectTimeout(TIMEOUT_SECONDS, TimeUnit.SECONDS)
        .readTimeout(TIMEOUT_SECONDS, TimeUnit.SECONDS)
        .build()

    private val json = Json { ignoreUnknownKeys = true }

    private fun browserFallbackDirective(): String {
        return if (domainGuardrailMode == PhoneAgentPrompts.DomainGuardrailMode.COMPATIBILITY) {
            " Do NOT open Chrome or any browser app to search manually."
        } else {
            ""
        }
    }

    /**
     * Search the web and return formatted results.
     *
     * Provider chain: Citros proxy → DuckDuckGo → SearXNG → Brave (direct).
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

        // 1. Try Citros search proxy (zero config, most reliable)
        if (citrosSearchEndpoint != null) {
            val result = searchCitros(query, clampedCount)
            if (!result.isError) return result
            Log.w(TAG, "Citros search failed: ${result.text}")
        }

        // 2. Try DuckDuckGo Lite (no config needed, may be rate-limited)
        if (ddgEndpoint == null) Log.d(TAG, "DuckDuckGo disabled")
        val ddgResult = if (ddgEndpoint != null) searchDuckDuckGo(query, clampedCount) else null
        if (ddgResult != null && !ddgResult.isError) return ddgResult
        if (ddgResult != null) Log.w(TAG, "DuckDuckGo search failed: ${ddgResult.text}")

        // 3. Try SearXNG if configured
        if (searxngBaseUrl != null) {
            val result = searchSearXNG(query, clampedCount)
            if (!result.isError) return result
            Log.w(TAG, "SearXNG search failed: ${result.text}")
        }

        // 4. Try Brave if user provided their own API key
        if (braveApiKey != null) {
            return searchBrave(query, clampedCount)
        }

        // All providers failed.
        return ToolResult(
            "Web search temporarily unavailable. Tell the user the search could not be completed and suggest they try again later." +
                browserFallbackDirective(),
            isError = true
        )
    }

    /**
     * Search via Citros proxy (Brave Search behind a Vercel edge function).
     * POSTs JSON `{query, count}`, receives Brave API JSON response.
     * No API key needed on device — key is server-side.
     */
    private suspend fun searchCitros(query: String, count: Int): ToolResult {
        return withContext(Dispatchers.IO) {
            try {
                val jsonBody = buildJsonObject {
                    put("query", query)
                    put("count", count)
                }.toString()

                val requestBuilder = Request.Builder()
                    .url(citrosSearchEndpoint!!)
                    .post(jsonBody.toRequestBody("application/json; charset=utf-8".toMediaType()))
                    .header("Accept", "application/json")

                if (citrosAppToken != null) {
                    requestBuilder.header("Authorization", "Bearer $citrosAppToken")
                }

                val request = requestBuilder.build()

                httpClient.newCall(request).execute().use { response ->
                    if (!response.isSuccessful) {
                        return@withContext ToolResult(
                            "Citros search failed (${response.code})",
                            isError = true
                        )
                    }

                    val body = response.body?.string() ?: return@withContext ToolResult(
                        "Citros search returned empty response",
                        isError = true
                    )

                    // Citros proxy returns Brave API JSON format
                    val results = parseBraveResults(body, count)
                    ToolResult(formatResults(query, results))
                }
            } catch (e: Exception) {
                Log.e(TAG, "Citros search error: ${e.message}")
                ToolResult("Citros search failed: ${e.message?.take(100)}", isError = true)
            }
        }
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
                        Log.w(TAG, "DuckDuckGo returned no organic results (body=${body.length} chars, may be rate-limited)")
                        return@withContext ToolResult(
                            "DuckDuckGo returned no results (may be rate-limited)." + browserFallbackDirective(),
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
                            "Brave search failed (${response.code}): ${response.message}." + browserFallbackDirective(),
                            isError = true
                        )
                    }

                    val body = response.body?.string() ?: return@withContext ToolResult(
                        "Brave search returned empty response." + browserFallbackDirective(),
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
            return "No results found for: $query. Tell the user no results were found." + browserFallbackDirective()
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
