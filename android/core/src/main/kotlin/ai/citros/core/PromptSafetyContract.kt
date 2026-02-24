package ai.citros.core

/**
 * Canonical safety text contract for prompt tuning (H2.4 spec Section 4.4).
 *
 * Safety invariance is enforced through canonical safety clauses and deterministic
 * normalization rules. These clauses MUST be present in all FULL/MINIMAL prompts.
 */
object PromptSafetyContract {

    /** SAFE-001: Irreversible action confirmation requirement. */
    const val SAFE_001 = "Never perform irreversible or high-stakes user actions without explicit confirmation."

    /** SAFE-002: Ambiguous/stale tool output handling. */
    const val SAFE_002 = "If tool output is ambiguous, stale, or missing required identifiers, request clarification before acting."

    /** SAFE-003: Task completion verification. */
    const val SAFE_003 = "Do not claim task completion unless the required UI state or tool result confirms completion."

    /** SAFE-004: Accessibility detached limitation reporting. */
    const val SAFE_004 = "When accessibility control is detached, report the limitation and avoid action instructions that require detached capabilities."

    /** All canonical safety clauses in order. */
    val ALL_CLAUSES: List<Pair<String, String>> = listOf(
        "SAFE-001" to SAFE_001,
        "SAFE-002" to SAFE_002,
        "SAFE-003" to SAFE_003,
        "SAFE-004" to SAFE_004
    )

    /**
     * Normalizes safety text to a canonical form for semantic-equivalence checks.
     *
     * Allowed shortening rules (Section 4.4):
     * 1. Replace repeated whitespace with a single space.
     * 2. Remove parenthetical clarifiers that do not alter modal verbs.
     * 3. Convert punctuation style (`;` vs `.`) without removing obligations.
     */
    fun normalize(text: String): String {
        var result = text
        // Rule 1: collapse whitespace
        result = result.replace(Regex("""\s+"""), " ")
        // Rule 2: remove parenthetical clarifiers not containing modal verbs.
        // Regex assumes non-nested parentheses, which matches our authored safety text style.
        val modalVerbs = setOf("must", "must not", "never", "do not", "shall", "shall not")
        result = result.replace(Regex("""\([^)]*\)""")) { match ->
            val content = match.value.lowercase()
            if (modalVerbs.any { content.contains(it) }) match.value else ""
        }
        // Rule 3: normalize semicolons to periods
        result = result.replace(';', '.')
        // Clean up double spaces from removals
        result = result.replace(Regex("""\s+"""), " ").trim()
        return result
    }

    /**
     * Assert that emitted prompt text contains all canonical safety clauses
     * with semantic equivalence after normalization.
     *
     * @return list of missing clause IDs (empty if all present)
     */
    fun findMissingClauses(promptText: String): List<String> {
        val normalizedPrompt = normalize(promptText)
        return ALL_CLAUSES.filter { (_, clause) ->
            !normalizedPrompt.contains(normalize(clause))
        }.map { it.first }
    }

    /**
     * Verify all canonical safety clauses are present.
     * @throws IllegalStateException if any clause is missing
     */
    fun assertAllPresent(promptText: String) {
        val missing = findMissingClauses(promptText)
        check(missing.isEmpty()) {
            "Missing canonical safety clauses: ${missing.joinToString(", ")}"
        }
    }
}
