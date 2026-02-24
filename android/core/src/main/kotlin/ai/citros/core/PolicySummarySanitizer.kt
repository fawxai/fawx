package ai.citros.core

object PolicySummarySanitizer {
    fun sanitize(raw: String?): String? {
        if (raw.isNullOrBlank()) return raw
        var value = raw
        value = value.replace(Regex("\\b\\d{4}([ -]?\\d{4}){2,3}\\b"), "[REDACTED]")
        value = value.replace(Regex("\\b\\d{6,8}\\b"), "[REDACTED]")
        value = value.replace(Regex("[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\\.[A-Za-z]{2,}"), "[REDACTED]")
        value = value.replace(Regex("https?://([^/\\s]+)[^\\s]*")) { m -> "URL_HOST:${m.groupValues[1].lowercase()}" }
        return value.replace(Regex("\\s+"), " ").trim()
    }
}
