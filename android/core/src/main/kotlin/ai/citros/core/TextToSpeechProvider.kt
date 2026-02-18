package ai.citros.core

import android.content.Context

/**
 * Provider interface for text-to-speech functionality.
 *
 * Implementations wrap platform or cloud TTS engines behind a common API.
 * The Android built-in implementation wraps [android.speech.tts.TextToSpeech];
 * cloud implementations (OpenAI TTS, ElevenLabs) can be added later.
 *
 * @see AndroidTextToSpeech for the on-device implementation
 * @see VoiceManager for the coordinator that manages provider lifecycle
 */
interface TextToSpeechProvider {
    /** Unique identifier for this provider (e.g. "android", "openai", "elevenlabs"). */
    val providerId: String

    /** Human-readable name for display in settings. */
    val displayName: String

    /** Whether this provider requires network connectivity. */
    val requiresNetwork: Boolean

    /** Whether this provider is currently available and ready to speak. */
    val isAvailable: Boolean

    /**
     * One-time initialization. May be async (engine init, API key validation).
     *
     * @param context Android context, used for engine initialization
     */
    suspend fun initialize(context: Context)

    /**
     * Speak the given text. Returns when playback completes or is interrupted.
     *
     * @param text The text to speak
     * @param options TTS options (speed, pitch, queue mode)
     */
    suspend fun speak(text: String, options: TtsOptions = TtsOptions())

    /** Stop current playback immediately. */
    fun stop()

    /** Whether the engine is currently speaking. */
    val isSpeaking: Boolean

    /**
     * Release all resources. Called by [VoiceManager.release] — callers should
     * NOT call this directly; instead call [VoiceManager.release] from the
     * lifecycle owner's `onDestroy()`.
     */
    fun release()
}
