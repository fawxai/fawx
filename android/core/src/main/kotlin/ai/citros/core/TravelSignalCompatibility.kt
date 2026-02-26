package ai.citros.core

/**
 * Legacy travel-specific compatibility shim.
 *
 * This keeps tactical flight-page dynamic-shell heuristics isolated from the
 * generic classifier so we can retire it incrementally after parity is proven.
 */
object TravelSignalCompatibility : ToolSignalCompatibility {
    // Initial dynamic-travel URL coverage for sites known to frequently render flight
    // results with JavaScript-heavy shells.
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
    // Common 3-letter abbreviations that frequently appear in scraped page chrome/content,
    // but are not airport codes; filtering these reduces false route-signal positives.
    private val NON_ROUTE_IATA_TOKENS = setOf(
        "USD", "FAQ", "API", "APP", "WWW", "HTTP", "HTTPS",
        "HTML", "JSON", "XML", "UTC", "EST", "PST", "CST", "MST", "GMT"
    )

    override fun classify(observation: ToolSignalObservation): ToolSignalClass? {
        if (observation.toolName !in setOf("web_fetch", "web_search")) return null
        val normalized = observation.text.lowercase()

        if (normalized.contains("dynamic travel")) {
            return ToolSignalClass.LOW_SIGNAL_DYNAMIC
        }

        val url = observation.toolInput["url"] as? String
        if (!url.isNullOrBlank() && isLikelyDynamicTravelUrl(url) && isLikelyDynamicTravelShell(url, observation.text)) {
            return ToolSignalClass.LOW_SIGNAL_DYNAMIC
        }

        return null
    }

    fun isLikelyDynamicTravelUrl(url: String): Boolean {
        val lower = url.lowercase()
        if (DYNAMIC_TRAVEL_URL_MARKERS.any { marker -> lower.contains(marker) }) {
            return true
        }

        val knownDomain = DYNAMIC_TRAVEL_DOMAINS_REQUIRING_FLIGHT_HINT.any { domain -> lower.contains(domain) }
        return knownDomain && lower.contains("flight")
    }

    fun isLikelyDynamicTravelShell(url: String, extractedText: String): Boolean {
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
            .filterNot { token -> token in NON_ROUTE_IATA_TOKENS }
            .toSet()
        return iataTokens.size >= 2
    }
}
