package ai.citros.core

import java.time.ZonedDateTime
import java.time.format.DateTimeFormatter
import java.util.Locale

/**
 * Device network connectivity type.
 */
enum class NetworkType { WIFI, CELLULAR, OFFLINE }

/**
 * Snapshot of device sensor state for prompt injection.
 * Gathered per model turn to avoid stale cross-task reuse.
 *
 * All fields are nullable — null means the data was unavailable or
 * the provider chose not to report it. [toPromptLine] gracefully
 * skips null fields.
 */
data class SensorContext(
    /** Battery percentage (0..100), or null if unavailable. Provider should clamp; prompt formatting also clamps defensively. */
    val batteryPercent: Int? = null,
    /** True if device is currently charging. */
    val isCharging: Boolean? = null,
    /** Network connectivity type, or null if unknown. */
    val networkType: NetworkType? = null,
    /** Coarse location ("Denver, CO"), or null if unavailable/denied. */
    val location: String? = null,
    /** Device local time with timezone. */
    val localTime: ZonedDateTime? = null
) {
    companion object {
        private val PROMPT_TIME_FORMATTER: DateTimeFormatter =
            DateTimeFormatter.ofPattern("h:mm a z", Locale.US)
        private val CONTROL_CHARS = Regex("[\\r\\n\\t\\u0000-\\u001F\\u007F]")
        private val MULTISPACE = Regex("\\s+")
        private val SAFE_LOCATION_CHARS = Regex("[^\\p{L}\\p{N} .,'()/_-]")
    }

    /**
     * Format as a single-line prompt suffix.
     * Only includes fields that are non-null.
     * Example: "Device: battery=72% (charging) | wifi | Denver, CO | 4:15 PM MST"
     * Returns empty string if all fields are null.
     */
    fun toPromptLine(): String {
        val parts = mutableListOf<String>()

        batteryPercent?.let { pct ->
            val normalizedPct = pct.coerceIn(0, 100)
            val chargingStr = if (isCharging == true) " (charging)" else ""
            parts.add("battery=${normalizedPct}%$chargingStr")
        }

        networkType?.let { parts.add(it.name.lowercase()) }

        location
            ?.sanitizeLocation()
            ?.let { parts.add("location=\"$it\"") }

        localTime?.let { time ->
            parts.add(time.format(PROMPT_TIME_FORMATTER))
        }

        return if (parts.isEmpty()) ""
        else "Device: ${parts.joinToString(" | ")}"
    }

    private fun String.sanitizeLocation(): String? {
        val sanitized = this
            .replace(CONTROL_CHARS, " ")
            .replace("|", "/")
            .replace("\\", "/")
            .replace("\"", "'")
            .replace(MULTISPACE, " ")
            .replace(SAFE_LOCATION_CHARS, "")
            .trim()
            .take(100)
        return sanitized.takeIf { it.isNotBlank() }
    }
}
