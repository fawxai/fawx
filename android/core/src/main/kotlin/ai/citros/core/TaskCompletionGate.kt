package ai.citros.core

internal class TaskCompletionGate(
    private val contracts: List<CompletionContract> = defaultContracts()
) {
    private val observedArtifacts = mutableSetOf<ArtifactType>()

    fun reset() {
        observedArtifacts.clear()
    }

    fun recordExecution(
        toolName: String,
        toolInput: Map<String, Any>,
        resultText: String,
        isError: Boolean,
        isUiMutatingTool: Boolean
    ) {
        if (isError) return
        val corpus = buildCorpus(toolName, toolInput, resultText)

        if (looksLikeEmailSend(corpus, isUiMutatingTool)) observedArtifacts += ArtifactType.EMAIL_SENT
        if (looksLikeBookingConfirmation(corpus, isUiMutatingTool)) observedArtifacts += ArtifactType.BOOKING_CONFIRMED
        if (looksLikeCalendarCreation(corpus, isUiMutatingTool)) observedArtifacts += ArtifactType.CALENDAR_EVENT_CREATED
    }

    fun guardFinalText(text: String?): String? {
        if (text.isNullOrBlank()) return text
        val claimedContracts = contracts.filter { it.isClaimedBy(text) }
        if (claimedContracts.isEmpty()) return text

        val missing = linkedSetOf<ArtifactType>()
        for (contract in claimedContracts) {
            contract.requiredArtifacts.filterTo(missing) { it !in observedArtifacts }
        }
        if (missing.isEmpty()) return text

        return "NOT_COMPLETED: Missing required artifacts: ${missing.joinToString(", ") { it.label }}."
    }

    private fun buildCorpus(toolName: String, toolInput: Map<String, Any>, resultText: String): String {
        val inputText = toolInput.entries.joinToString(" ") { (k, v) -> "$k=$v" }
        return (toolName + " " + inputText + " " + resultText).lowercase()
    }

    private fun looksLikeEmailSend(corpus: String, isUiMutatingTool: Boolean): Boolean {
        val hasEmailContext = corpus.contains("email") || corpus.contains("gmail") || corpus.contains("mail")
        val hasSendSignal = corpus.contains("sent") || corpus.contains("send")
        return isUiMutatingTool && hasEmailContext && hasSendSignal
    }

    private fun looksLikeBookingConfirmation(corpus: String, isUiMutatingTool: Boolean): Boolean {
        val bookingToken = corpus.contains("book") || corpus.contains("booking") || corpus.contains("reservation")
        val confirmationToken = corpus.contains("confirm") || corpus.contains("confirmation") || corpus.contains("reserved")
        return isUiMutatingTool && bookingToken && confirmationToken
    }

    private fun looksLikeCalendarCreation(corpus: String, isUiMutatingTool: Boolean): Boolean {
        val hasCalendarContext = corpus.contains("calendar") || corpus.contains("event") || corpus.contains("schedule")
        val hasCreateSignal = corpus.contains("created") || corpus.contains("added") || corpus.contains("saved")
        return isUiMutatingTool && hasCalendarContext && hasCreateSignal
    }

    internal data class CompletionContract(
        val name: String,
        val claimPhrases: Set<String>,
        val requiredArtifacts: Set<ArtifactType>
    ) {
        fun isClaimedBy(text: String): Boolean = claimPhrases.any { text.lowercase().contains(it) }
    }

    internal enum class ArtifactType(val label: String) {
        EMAIL_SENT("email_sent"),
        BOOKING_CONFIRMED("booking_confirmed"),
        CALENDAR_EVENT_CREATED("calendar_event_created")
    }

    companion object {
        internal fun defaultContracts(): List<CompletionContract> = listOf(
            CompletionContract(
                name = "email_send",
                claimPhrases = setOf("email sent", "sent the email", "sent your email", "message sent"),
                requiredArtifacts = setOf(ArtifactType.EMAIL_SENT)
            ),
            CompletionContract(
                name = "booking_complete",
                claimPhrases = setOf("booking confirmed", "booked", "reservation confirmed"),
                requiredArtifacts = setOf(ArtifactType.BOOKING_CONFIRMED)
            ),
            CompletionContract(
                name = "calendar_scheduled",
                claimPhrases = setOf("calendar event created", "event created on calendar", "scheduled on your calendar"),
                requiredArtifacts = setOf(ArtifactType.CALENDAR_EVENT_CREATED)
            )
        )
    }
}
