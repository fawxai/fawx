package ai.citros.core

import kotlinx.coroutines.flow.Flow

/**
 * Accumulates speech segments from a [SpeechToTextProvider] into a complete
 * transcription, preserving any existing text in the input field.
 *
 * Extracted from `MessageInput.beginListening` (#637) so that both production
 * code and tests exercise the same algorithm.
 *
 * Usage:
 * ```
 * val accumulator = VoiceAccumulator(prefix = existingText)
 * stt.startListening().collect { event ->
 *     val display = accumulator.onEvent(event)
 *     if (display != null) text = display
 * }
 * // After flow completes:
 * val result = accumulator.finish(autoSend = autoSendEnabled)
 * ```
 *
 * @param prefix existing text in the input field before voice starts
 */
class VoiceAccumulator(private val prefix: String) {

    private var accumulated = ""
    private var _hasError = false

    /** True if an error event was received during accumulation. */
    val hasError: Boolean get() = _hasError

    /** The accumulated voice text (without prefix). */
    val accumulatedText: String get() = accumulated

    /**
     * Process a [SpeechEvent] and return the display text for the input field,
     * or `null` if the event doesn't update the display (e.g. errors).
     *
     * Error events set [hasError] but return `null` — callers handle error
     * display (Toast, etc.) themselves.
     */
    fun onEvent(event: SpeechEvent): String? = when (event) {
        is SpeechEvent.Partial -> {
            val base = if (prefix.isNotBlank()) "$prefix " else ""
            if (accumulated.isEmpty()) {
                "${base}Listening..."
            } else {
                "$base$accumulated..."
            }
        }
        is SpeechEvent.Final -> {
            accumulated = (accumulated + " " + event.text).trim()
            val base = if (prefix.isNotBlank()) "$prefix " else ""
            base + accumulated
        }
        is SpeechEvent.Error -> {
            _hasError = true
            null
        }
    }

    /**
     * Called after the flow completes (timeout, manual stop, or cancellation).
     *
     * @param autoSend whether auto-send-after-voice is enabled
     * @return the result of the voice session
     */
    fun finish(autoSend: Boolean): Result {
        return if (accumulated.isNotBlank() && autoSend) {
            val finalText = (if (prefix.isNotBlank()) "$prefix " else "") + accumulated
            Result(displayText = "", autoSendText = finalText)
        } else {
            // Leave accumulated text in the field (or prefix if nothing was captured)
            val display = if (accumulated.isNotBlank()) {
                (if (prefix.isNotBlank()) "$prefix " else "") + accumulated
            } else {
                prefix
            }
            Result(displayText = display, autoSendText = null)
        }
    }

    /**
     * Result of a completed voice accumulation session.
     *
     * @param displayText what to show in the text field after the session
     * @param autoSendText the text to send automatically, or `null` if auto-send didn't fire
     */
    data class Result(
        val displayText: String,
        val autoSendText: String?,
    )
}
