package ai.citros.core

/**
 * Error types for speech recognition failures.
 *
 * Each subtype maps to a specific category of failure that callers can
 * handle distinctly (e.g., showing a permission rationale vs. a retry button).
 */
sealed class SpeechError {
    /** Microphone permission was denied by the user. */
    data class PermissionDenied(val message: String = "Microphone permission denied") : SpeechError()

    /** Network error during cloud-based recognition. */
    data class NetworkError(val message: String = "Network error") : SpeechError()

    /** Recognition engine internal error. */
    data class EngineError(val message: String = "Recognition engine error") : SpeechError()

    /** Recognition timed out without producing a result. */
    data class Timeout(val message: String = "Recognition timed out") : SpeechError()

    /** Speech recognition is not available on this device. */
    data class Unavailable(val message: String = "Speech recognition unavailable") : SpeechError()
}
