package ai.citros.chat

/**
 * Resolves free-text disambiguation input to an offer_choices option.
 *
 * Matching strategy (deterministic, conservative):
 * 1) Exact normalized match
 * 2) Unique substring match (for inputs like "maps" vs "Google Maps")
 * 3) Unique token-prefix match (for inputs like "goo map")
 *
 * Returns null if no unique match is found.
 */
internal object OfferChoiceResolver {

    private val nonWordRegex = Regex("[^\\p{L}\\p{N}\\s]")
    private val whitespaceRegex = Regex("\\s+")

    private data class Candidate(
        val original: String,
        val normalized: String,
        val tokens: List<String>
    )

    fun resolveChoice(choices: List<String>, userInput: String): String? {
        val normalizedInput = normalize(userInput)
        if (normalizedInput.isBlank()) return null

        val candidates = choices.map { choice ->
            val normalizedChoice = normalize(choice)
            Candidate(
                original = choice,
                normalized = normalizedChoice,
                tokens = normalizedChoice.split(' ').filter { it.isNotBlank() }
            )
        }

        candidates.firstOrNull { it.normalized == normalizedInput }?.let { return it.original }

        if (normalizedInput.length >= 3) {
            val containsMatches = candidates.filter {
                it.normalized.contains(normalizedInput) || normalizedInput.contains(it.normalized)
            }
            if (containsMatches.size == 1) {
                return containsMatches.first().original
            }
        }

        val inputTokens = normalizedInput.split(' ').filter { it.isNotBlank() }
        if (inputTokens.isEmpty()) return null

        val tokenPrefixMatches = candidates.filter { candidate ->
            inputTokens.all { inputToken ->
                candidate.tokens.any { candidateToken -> candidateToken.startsWith(inputToken) }
            }
        }
        return if (tokenPrefixMatches.size == 1) tokenPrefixMatches.first().original else null
    }

    private fun normalize(text: String): String {
        return text
            .trim()
            .lowercase()
            .replace(nonWordRegex, " ")
            .replace(whitespaceRegex, " ")
    }
}
