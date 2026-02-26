package ai.citros.chat

/**
 * Best-effort parser for user confirmation replies while the agent is waiting
 * on a policy gate (approve/deny).
 *
 * Accepts short tokens ("yes", "no") plus common natural-language variants
 * ("you have permission", "go ahead", "not now").
 */
internal object PolicyConfirmationInputParser {

    private val exactApprove = setOf(
        "yes", "y", "ok", "okay", "sure", "allow", "approve", "approved",
        "continue", "resume", "proceed", "go ahead", "do it", "open it"
    )

    // Exact-deny handles terse, unambiguous replies. Longer/mixed phrases are
    // intentionally handled by denyPatterns below.
    private val exactDeny = setOf(
        "no", "n", "nope", "nah", "not now"
    )

    // Safety-first precedence: deny signals are evaluated before approve signals.
    private val denyPatterns = listOf(
        Regex("""\b(don'?t|do not)\b"""),
        Regex("""\b(no permission|without permission)\b"""),
        Regex("""\b(cancel|stop|deny|denied|reject|rejected|block)\b"""),
        Regex(
            """\bnot\s+(sure|ok|okay|really|now|yet|approve|approved|allow|allowed|continue|proceed|resume|go ahead|do it|open it)\b"""
        ),
        Regex("""\bnever\b""")
    )

    private val approvePatterns = listOf(
        Regex("""\b(yes|yep|yeah|yup|sure|ok|okay|affirmative)\b"""),
        Regex("""\b(allow|approve|approved|continue|proceed|resume|go ahead)\b"""),
        Regex("""\b(you have permission|i (give|grant) permission|permission granted)\b"""),
        Regex("""\b(do it|open it)\b""")
    )

    private const val PATTERN_MAX_WORDS = 6

    fun parse(text: String): Boolean? {
        val normalized = normalize(text)
        if (normalized.isBlank()) return null

        if (normalized in exactDeny) return false
        if (normalized in exactApprove) return true

        if (denyPatterns.any { it.containsMatchIn(normalized) }) return false

        // Avoid false positives on long/contextual follow-ups (e.g. "Can you resume
        // from where you left off?"). Keep regex matching to short imperative replies.
        val wordCount = normalized.split(" ").size
        val isQuestionStylePrefix = normalized.startsWith("can you ") ||
            normalized.startsWith("could you ") ||
            normalized.startsWith("would you ")
        if (wordCount <= PATTERN_MAX_WORDS && !isQuestionStylePrefix) {
            if (approvePatterns.any { it.containsMatchIn(normalized) }) return true
        }

        return null
    }

    private fun normalize(text: String): String {
        return text
            .trim()
            .lowercase()
            .replace(Regex("""[^\p{L}\p{Nd}\s']+"""), " ")
            .replace(Regex("""\s+"""), " ")
            .trim()
    }
}
