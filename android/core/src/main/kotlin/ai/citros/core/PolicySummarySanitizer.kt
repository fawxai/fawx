package ai.citros.core

object PolicySummarySanitizer {
    private val cardPattern = Regex("\\b\\d{4}([ -]?\\d{4}){2,3}\\b")
    private val numericOtpPattern = Regex("\\b\\d{6,8}\\b")
    private val dashOtpPattern = Regex("\\b\\d{3,4}-\\d{3,4}\\b")
    private val alphaNumericOtpPattern = Regex("\\b(?=[A-Za-z0-9]{6,8}\\b)(?=[A-Za-z0-9]*\\d)[A-Za-z0-9]{6,8}\\b")
    private val phrasePrefixOtpPattern = Regex("(?i)(your (?:code|verification code|otp) is:?\\s*)(\\S+)")
    private val phraseSuffixOtpPattern = Regex("(?i)(\\S+)(\\s+is your (?:code|verification code|otp))")
    private val emailPattern = Regex("[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\\.[A-Za-z]{2,}")
    private val urlPattern = Regex("https?://([^/\\s]+)[^\\s]*")

    fun sanitize(raw: String?): String? {
        if (raw.isNullOrBlank()) return raw
        var value = raw
        value = value.replace(cardPattern, "[REDACTED]")
        value = value.replace(dashOtpPattern, "[REDACTED]")
        value = value.replace(numericOtpPattern, "[REDACTED]")
        value = value.replace(alphaNumericOtpPattern, "[REDACTED]")
        value = value.replace(phrasePrefixOtpPattern) { match -> "${match.groupValues[1]}[REDACTED]" }
        value = value.replace(phraseSuffixOtpPattern) { match -> "[REDACTED]${match.groupValues[2]}" }
        value = value.replace(emailPattern, "[REDACTED]")
        value = value.replace(urlPattern) { m -> "URL_HOST:${m.groupValues[1].lowercase()}" }
        return value.replace(Regex("\\s+"), " ").trim()
    }
}
