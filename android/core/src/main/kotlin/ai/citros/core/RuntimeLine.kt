package ai.citros.core

import java.time.Instant
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter

/**
 * Runtime line builder for prompt telemetry (H2.4 spec Section 4.5).
 *
 * Schema: runtime|ts=<RFC3339>|model=...|tier=...|mode=...|accessibility=...|tool_policy=...|prompt_chars=...|prompt_tokens_est=...|trimmed=...|trimmed_sections=...
 *
 * Key order is fixed and pipe-delimited. No user content, tool arguments,
 * contact names, or message text may appear in the runtime line.
 */
object RuntimeLine {

    // Intentionally second-level precision only (no fractional seconds) for compact deterministic telemetry.
    private val RFC3339_UTC = DateTimeFormatter.ofPattern("yyyy-MM-dd'T'HH:mm:ss'Z'")

    /**
     * Build a runtime line per the spec schema.
     *
     * @param modelName provider model ID
     * @param tier resolved model tier
     * @param mode prompt mode
     * @param accessibility "attached" or "detached"
     * @param toolPolicy tool policy identifier (e.g. "full", "small_restricted")
     * @param promptChars character count of final prompt
     * @param promptTokensEst estimated token count (ceil(chars/4))
     * @param trimmed whether any sections were trimmed
     * @param trimmedSections list of trimmed section IDs (canonical, sorted)
     * @param timestamp override for testing; defaults to now
     */
    fun build(
        modelName: String?,
        tier: ModelTier,
        mode: PromptMode,
        accessibility: String,
        toolPolicy: String,
        promptChars: Int,
        promptTokensEst: Int,
        trimmed: Boolean,
        trimmedSections: List<String> = emptyList(),
        timestamp: Instant = Instant.now()
    ): String {
        val ts = timestamp.atOffset(ZoneOffset.UTC).format(RFC3339_UTC)
        val model = modelName?.takeIf { it.isNotBlank() } ?: "unknown"
        val sections = if (trimmedSections.isEmpty()) "none"
        else trimmedSections.sorted().joinToString(",")

        return "runtime|ts=$ts|model=$model|tier=$tier|mode=$mode|accessibility=$accessibility|tool_policy=$toolPolicy|prompt_chars=$promptChars|prompt_tokens_est=$promptTokensEst|trimmed=$trimmed|trimmed_sections=$sections"
    }

    /** Regex for validating runtime line format. */
    val SCHEMA_REGEX = Regex(
        """^runtime\|ts=\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z\|model=[^|]+\|tier=(FLAGSHIP|STANDARD|SMALL)\|mode=(FULL|MINIMAL|NONE)\|accessibility=(attached|detached)\|tool_policy=[^|]+\|prompt_chars=\d+\|prompt_tokens_est=\d+\|trimmed=(true|false)\|trimmed_sections=([a-z_,]+|none)$"""
    )

    data class RuntimeLineData(
        val ts: String,
        val model: String,
        val tier: String,
        val mode: String,
        val accessibility: String,
        val toolPolicy: String,
        val promptChars: String,
        val promptTokensEst: String,
        val trimmed: String,
        val trimmedSections: String
    ) {
        fun asMap(): LinkedHashMap<String, String> = linkedMapOf(
            "ts" to ts,
            "model" to model,
            "tier" to tier,
            "mode" to mode,
            "accessibility" to accessibility,
            "tool_policy" to toolPolicy,
            "prompt_chars" to promptChars,
            "prompt_tokens_est" to promptTokensEst,
            "trimmed" to trimmed,
            "trimmed_sections" to trimmedSections
        )
    }

    /**
     * Parse a runtime line into strongly typed runtime telemetry fields.
     * @return parsed data, or null if format is invalid
     */
    fun parse(line: String): RuntimeLineData? {
        if (!line.startsWith("runtime|")) return null
        val parts = line.removePrefix("runtime|").split("|")
        val map = linkedMapOf<String, String>()
        for (part in parts) {
            val eq = part.indexOf('=')
            if (eq < 0) return null
            map[part.substring(0, eq)] = part.substring(eq + 1)
        }
        return RuntimeLineData(
            ts = map["ts"] ?: return null,
            model = map["model"] ?: return null,
            tier = map["tier"] ?: return null,
            mode = map["mode"] ?: return null,
            accessibility = map["accessibility"] ?: return null,
            toolPolicy = map["tool_policy"] ?: return null,
            promptChars = map["prompt_chars"] ?: return null,
            promptTokensEst = map["prompt_tokens_est"] ?: return null,
            trimmed = map["trimmed"] ?: return null,
            trimmedSections = map["trimmed_sections"] ?: return null
        )
    }
}
