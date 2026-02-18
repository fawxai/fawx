package ai.citros.core

/**
 * Options for text-to-speech playback.
 *
 * @property speed Speech rate multiplier (0.5–2.0). Default is normal speed.
 * @property pitch Voice pitch multiplier (0.5–2.0). Default is normal pitch.
 * @property queueMode How to handle speech that arrives while already speaking.
 */
data class TtsOptions(
    val speed: Float = 1.0f,
    val pitch: Float = 1.0f,
    val queueMode: QueueMode = QueueMode.FLUSH
)

/**
 * Queue mode for TTS playback.
 *
 * Controls what happens when [TextToSpeechProvider.speak] is called while
 * the engine is already speaking.
 */
enum class QueueMode {
    /** Cancel in-progress speech and start the new utterance immediately. */
    FLUSH,
    /** Queue the new utterance behind the current one. */
    ADD
}
