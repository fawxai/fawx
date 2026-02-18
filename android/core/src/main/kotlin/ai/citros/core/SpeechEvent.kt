package ai.citros.core

/**
 * Events emitted by [SpeechToTextProvider.startListening].
 *
 * A typical flow emits zero or more [Partial] events followed by exactly one
 * [Final] or [Error] event, after which the flow completes.
 */
sealed class SpeechEvent {
    /** Intermediate recognition result while the user is still speaking. */
    data class Partial(val text: String) : SpeechEvent()

    /** Final recognition result. The flow completes after this event. */
    data class Final(val text: String) : SpeechEvent()

    /** Recognition error. The flow completes after this event. */
    data class Error(val error: SpeechError) : SpeechEvent()
}
