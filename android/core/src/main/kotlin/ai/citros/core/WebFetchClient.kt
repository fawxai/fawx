package ai.citros.core

import android.util.Log
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.OkHttpClient
import okhttp3.Request
import org.jsoup.Jsoup
import android.os.Build
import java.util.concurrent.TimeUnit

/**
 * Fetches and extracts readable text content from web pages.
 *
 * Uses OkHttp for HTTP requests and Jsoup for HTML → text extraction.
 * Strips navigation, scripts, styles, and other non-content elements
 * to produce clean text suitable for LLM context.
 *
 * No API key required — this is a direct HTTP fetch.
 */
class WebFetchClient {
    companion object {
        private const val TAG = "WebFetchClient"
        private const val DEFAULT_MAX_CHARS = 5000
        private const val MAX_CHARS_CAP = 50000
        private const val TIMEOUT_SECONDS = 15L
        internal const val CONTENT_TYPE_HTML = "text/html"
        internal const val CONTENT_TYPE_XHTML = "application/xhtml"
        internal const val ACCEPT_HEADER = "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"

        // Initial dynamic-travel URL coverage for sites known to frequently render flight
        // results with JavaScript-heavy shells.
        // TODO(citros): Expand this list as fetch telemetry surfaces additional domains.
        internal val DYNAMIC_TRAVEL_URL_MARKERS = listOf(
            "/travel/flights",
            "google.com/flights",
            "kayak.com/flight",
            "flightconnections.com",
            "expedia.com/flights",
            "booking.com/flights"
        )

        // Some providers have variable path structures; require a flight hint to reduce
        // false positives from non-flight pages on the same domain.
        internal val DYNAMIC_TRAVEL_DOMAINS_REQUIRING_FLIGHT_HINT = listOf(
            "skyscanner.",
            "southwest.com",
            "united.com",
            "delta.com"
        )

        private val PRICE_SIGNAL_REGEX =
            Regex("""\$\s?\d|usd\s?\d|\d\s?usd""", RegexOption.IGNORE_CASE)
        private val CITY_ROUTE_SIGNAL_REGEX = Regex(
            """\bfrom\s+[A-Za-z][A-Za-z.'-]{2,}\s+to\s+[A-Za-z][A-Za-z.'-]{2,}\b""",
            RegexOption.IGNORE_CASE
        )
        private val IATA_ROUTE_SIGNAL_REGEX =
            Regex("""\b[A-Z]{3}\b\s*(?:→|->|to)\s*\b[A-Z]{3}\b""")
        private val TRAVEL_CONTEXT_SIGNAL_REGEX = Regex(
            """\b(flight|flights|fare|fares|airline|airport|depart|arrival)\b""",
            RegexOption.IGNORE_CASE
        )
        private val IATA_TOKEN_REGEX = Regex("""\b[A-Z]{3}\b""")
        private val NON_ROUTE_IATA_TOKENS = setOf(
            "USD", "FAQ", "API", "APP", "WWW", "HTTP", "HTTPS",
            "HTML", "JSON", "XML", "UTC", "EST", "PST", "CST", "MST", "GMT"
        )

        /** User-Agent with Android OS version for accurate site rendering decisions. */
        internal val USER_AGENT = "Citros/1.0 (Android ${Build.VERSION.RELEASE}; ${Build.MODEL}; AI Assistant)"
    }

    private val httpClient = OkHttpClient.Builder()
        .connectTimeout(TIMEOUT_SECONDS, TimeUnit.SECONDS)
        .readTimeout(TIMEOUT_SECONDS, TimeUnit.SECONDS)
        .followRedirects(true)
        .followSslRedirects(true)
        .build()

    /**
     * Fetch a URL and extract readable text content.
     *
     * @param url HTTP or HTTPS URL to fetch
     * @param maxChars Maximum characters to return (100-50000, default 5000)
     * @return ToolResult with extracted text or error
     */
    suspend fun fetch(url: String, maxChars: Int = DEFAULT_MAX_CHARS): ToolResult {
        // Validate URL
        if (url.isBlank()) {
            return ToolResult("URL cannot be empty", isError = true)
        }
        if (!url.startsWith("http://") && !url.startsWith("https://")) {
            return ToolResult("Invalid URL: must start with http:// or https://", isError = true)
        }

        val clampedMaxChars = maxChars.coerceIn(100, MAX_CHARS_CAP)

        return withContext(Dispatchers.IO) {
            try {
                val request = Request.Builder()
                    .url(url)
                    .header("User-Agent", USER_AGENT)
                    .header("Accept", ACCEPT_HEADER)
                    .get()
                    .build()

                val response = httpClient.newCall(request).execute()
                if (!response.isSuccessful) {
                    if (isLikelyDynamicTravelUrl(url)) {
                        return@withContext ToolResult(
                            "Fetch blocked (${response.code}) on a dynamic travel site. " +
                                "Use web_search results/snippets from multiple sources and provide a best-effort answer with uncertainty; " +
                                "do NOT ask the user to manually open apps.",
                            isError = true
                        )
                    }
                    return@withContext ToolResult(
                        "Fetch failed (${response.code}): ${response.message}",
                        isError = true
                    )
                }

                val contentType = response.header("Content-Type")?.lowercase() ?: ""
                val body = response.body?.string() ?: return@withContext ToolResult(
                    "Fetch returned empty response",
                    isError = true
                )

                val text = if (contentType.contains(CONTENT_TYPE_HTML) || contentType.contains(CONTENT_TYPE_XHTML)) {
                    extractReadableText(body)
                } else {
                    // Non-HTML content (JSON, plain text, etc.) — return as-is
                    body
                }

                if (text.isBlank()) {
                    if (isLikelyDynamicTravelUrl(url)) {
                        // Intentionally non-error: the request succeeded, but the page is
                        // likely a dynamic shell. Treat this as guidance so orchestration can
                        // continue with search snippets instead of short-circuiting as a hard failure.
                        return@withContext ToolResult(
                            "Fetched page shell for a dynamic travel site, but no readable fare rows were exposed. " +
                                "Continue with web_search snippets and provide best-effort options with uncertainty; do NOT ask the user to manually open apps."
                        )
                    }
                    return@withContext ToolResult(
                        "Page at $url returned no readable content",
                        isError = true
                    )
                }

                if (isLikelyDynamicTravelShell(url, text)) {
                    // Intentionally non-error: we have partial signal from a shell page and
                    // should keep the flow moving with best-effort guidance.
                    return@withContext ToolResult(
                        "Fetched a dynamic travel shell page with limited static fare data. " +
                            "Use web_search snippets/alternative sources for concrete prices and clearly label uncertainty."
                    )
                }

                val truncated = text.take(clampedMaxChars)
                val wasTruncated = text.length > clampedMaxChars

                ToolResult(buildString {
                    appendLine("URL: $url")
                    if (wasTruncated) {
                        appendLine("(truncated to $clampedMaxChars of ${text.length} chars)")
                    }
                    appendLine()
                    append(truncated)
                }.trimEnd())
            } catch (e: Exception) {
                Log.e(TAG, "Fetch error for $url: ${e.message}")
                ToolResult("Fetch failed: ${e.message?.take(100)}", isError = true)
            }
        }
    }

    internal fun isLikelyDynamicTravelUrl(url: String): Boolean {
        val lower = url.lowercase()
        if (DYNAMIC_TRAVEL_URL_MARKERS.any { lower.contains(it) }) {
            return true
        }

        val knownDomain = DYNAMIC_TRAVEL_DOMAINS_REQUIRING_FLIGHT_HINT.any { lower.contains(it) }
        return knownDomain && lower.contains("flight")
    }

    internal fun isLikelyDynamicTravelShell(url: String, extractedText: String): Boolean {
        if (!isLikelyDynamicTravelUrl(url)) return false
        val text = extractedText.trim()
        if (text.isEmpty()) return true

        val hasPriceSignal = PRICE_SIGNAL_REGEX.containsMatchIn(text)
        val hasRouteSignal = hasGenericRouteSignal(text)
        return !hasPriceSignal && !hasRouteSignal
    }

    private fun hasGenericRouteSignal(text: String): Boolean {
        if (CITY_ROUTE_SIGNAL_REGEX.containsMatchIn(text) || IATA_ROUTE_SIGNAL_REGEX.containsMatchIn(text)) {
            return true
        }
        if (!TRAVEL_CONTEXT_SIGNAL_REGEX.containsMatchIn(text)) {
            return false
        }

        val iataTokens = IATA_TOKEN_REGEX.findAll(text)
            .map { it.value.uppercase() }
            .filterNot { it in NON_ROUTE_IATA_TOKENS }
            .toSet()
        return iataTokens.size >= 2
    }

    /**
     * Extract readable text from HTML using Jsoup.
     *
     * Removes non-content elements (scripts, styles, navigation, footers)
     * and returns clean text suitable for LLM context.
     */
    internal fun extractReadableText(html: String): String {
        val doc = Jsoup.parse(html)

        // Remove non-content elements
        doc.select("script, style, nav, footer, header, aside, iframe, noscript, svg, form").remove()
        // Remove hidden elements
        doc.select("[style*=display:none], [style*=display: none], [hidden], [aria-hidden=true]").remove()

        // Get title
        val title = doc.title()?.takeIf { it.isNotBlank() }

        // Get main content — prefer <main>, <article>, or <body>
        val mainContent = doc.selectFirst("main")
            ?: doc.selectFirst("article")
            ?: doc.selectFirst("[role=main]")
            ?: doc.body()

        val bodyText = mainContent?.text()?.trim() ?: ""

        return buildString {
            if (title != null) {
                appendLine(title)
                appendLine()
            }
            append(bodyText)
        }.trim()
    }
}
