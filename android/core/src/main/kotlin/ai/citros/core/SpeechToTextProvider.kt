package ai.citros.core

import android.content.Context
import kotlinx.coroutines.flow.Flow

/**
 * Provider interface for speech-to-text functionality.
 *
 * Implementations wrap platform or cloud STT engines behind a common API.
 * The Android built-in implementation wraps [android.speech.SpeechRecognizer];
 * cloud implementations (OpenAI Whisper, Deepgram) can be added later.
 *
 * @see AndroidSpeechToText for the on-device implementation
 * @see VoiceManager for the coordinator that manages provider lifecycle
 */
interface SpeechToTextProvider {
    /** Unique identifier for this provider (e.g. "android", "openai-whisper"). */
    val providerId: String

    /** Human-readable name for display in settings (e.g. "On-Device"). */
    val displayName: String

    /** Whether this provider requires network connectivity. */
    val requiresNetwork: Boolean

    /** Whether this provider is currently available (engine ready, API key present, etc.). */
    val isAvailable: Boolean

    /**
     * One-time initialization. May be async (engine init, API key validation).
     *
     * @param context Android context, used for engine initialization
     */
    suspend fun initialize(context: Context)

    /**
     * Start listening and return a [Flow] of [SpeechEvent]s.
     *
     * The flow emits [SpeechEvent.Partial] results as the user speaks,
     * a [SpeechEvent.Final] result when recognition completes, and
     * [SpeechEvent.Error] if something goes wrong. The flow completes
     * after [SpeechEvent.Final] or [SpeechEvent.Error].
     *
     * **Threading:** Flow is collected on [kotlinx.coroutines.Dispatchers.Main]
     * (SpeechRecognizer requirement). Callers should use `flowOn()` if they
     * need to process on a different dispatcher.
     *
     * **Cancellation contract:** Cancelling the returned flow automatically calls
     * [stopListening] internally (via `callbackFlow`'s `awaitClose` block).
     * Callers do NOT need to call [stopListening] explicitly — just cancel
     * the collection (e.g., via `Job.cancel()` or scope cancellation).
     */
    fun startListening(): Flow<SpeechEvent>

    /** Explicitly stop listening (optional — flow cancellation handles this automatically). */
    fun stopListening()

    /** Cancel the current recognition session. */
    fun cancel()

    /**
     * Release all resources. Called by [VoiceManager.release] — callers should
     * NOT call this directly; instead call [VoiceManager.release] from the
     * lifecycle owner's `onDestroy()`.
     */
    fun release()
}
