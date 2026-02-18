package ai.citros.core

import android.content.Context
import android.content.SharedPreferences
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

/**
 * Central coordinator for voice I/O providers.
 *
 * Manages registered [SpeechToTextProvider] and [TextToSpeechProvider] instances,
 * handles runtime switching between providers, and persists user preferences
 * (active provider, auto-speak, auto-send) to [SharedPreferences].
 *
 * Thread safety: Provider switching is guarded by a [Mutex]. Settings are exposed
 * as [StateFlow] for atomic, thread-safe observation from UI and ViewModel.
 *
 * @param context Android context for SharedPreferences access
 * @param sttProviders Available STT providers (first is default)
 * @param ttsProviders Available TTS providers (first is default)
 * @param prefs SharedPreferences instance for persistence (default: app-private prefs)
 * @throws IllegalArgumentException if either provider list is empty
 */
class VoiceManager(
    context: Context,
    val sttProviders: List<SpeechToTextProvider>,
    val ttsProviders: List<TextToSpeechProvider>,
    private val prefs: SharedPreferences = context.applicationContext
        .getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
) {
    init {
        require(sttProviders.isNotEmpty()) {
            "At least one SpeechToTextProvider must be registered"
        }
        require(ttsProviders.isNotEmpty()) {
            "At least one TextToSpeechProvider must be registered"
        }
    }

    private val switchMutex = Mutex()

    private val _activeStt = MutableStateFlow(
        resolveProvider(sttProviders, prefs.getString(KEY_ACTIVE_STT, null))
    )
    /** Currently active STT provider. */
    val activeStt: StateFlow<SpeechToTextProvider> = _activeStt.asStateFlow()

    private val _activeTts = MutableStateFlow(
        resolveProvider(ttsProviders, prefs.getString(KEY_ACTIVE_TTS, null))
    )
    /** Currently active TTS provider. */
    val activeTts: StateFlow<TextToSpeechProvider> = _activeTts.asStateFlow()

    // Fix #4: Private mutable backing fields, public read-only StateFlow
    private val _autoSpeakResponses = MutableStateFlow(
        prefs.getBoolean(KEY_AUTO_SPEAK, false)
    )
    /**
     * Whether agent responses should be automatically spoken via TTS.
     * Default: `false`. Persisted to SharedPreferences.
     */
    val autoSpeakResponses: StateFlow<Boolean> = _autoSpeakResponses.asStateFlow()

    private val _autoSendAfterVoice = MutableStateFlow(
        prefs.getBoolean(KEY_AUTO_SEND, false)
    )
    /**
     * Whether voice input should be automatically sent after recognition completes.
     * Default: `false` (user reviews transcript first). Persisted to SharedPreferences.
     */
    val autoSendAfterVoice: StateFlow<Boolean> = _autoSendAfterVoice.asStateFlow()

    /**
     * Switch the active STT provider at runtime.
     *
     * Validates that the provider exists and is available before switching.
     * The new selection is persisted to SharedPreferences.
     *
     * @param providerId The [SpeechToTextProvider.providerId] to switch to
     * @return [Result.success] if switched, [Result.failure] with:
     *   - [IllegalArgumentException] if provider not found
     *   - [IllegalStateException] if provider is not available
     */
    suspend fun switchStt(providerId: String): Result<Unit> = switchMutex.withLock {
        val provider = sttProviders.find { it.providerId == providerId }
            ?: return@withLock Result.failure(
                IllegalArgumentException("STT provider '$providerId' not found")
            )
        if (!provider.isAvailable) {
            return@withLock Result.failure(
                IllegalStateException("STT provider '$providerId' is not available")
            )
        }
        _activeStt.value = provider
        prefs.edit().putString(KEY_ACTIVE_STT, providerId).apply()
        Result.success(Unit)
    }

    /**
     * Switch the active TTS provider at runtime.
     *
     * Validates that the provider exists and is available before switching.
     * The new selection is persisted to SharedPreferences.
     *
     * @param providerId The [TextToSpeechProvider.providerId] to switch to
     * @return [Result.success] if switched, [Result.failure] with:
     *   - [IllegalArgumentException] if provider not found
     *   - [IllegalStateException] if provider is not available
     */
    suspend fun switchTts(providerId: String): Result<Unit> = switchMutex.withLock {
        val provider = ttsProviders.find { it.providerId == providerId }
            ?: return@withLock Result.failure(
                IllegalArgumentException("TTS provider '$providerId' not found")
            )
        if (!provider.isAvailable) {
            return@withLock Result.failure(
                IllegalStateException("TTS provider '$providerId' is not available")
            )
        }
        _activeTts.value = provider
        prefs.edit().putString(KEY_ACTIVE_TTS, providerId).apply()
        Result.success(Unit)
    }

    /**
     * Persist the current auto-speak setting.
     *
     * @param enabled Whether to auto-speak agent responses
     */
    fun setAutoSpeakResponses(enabled: Boolean) {
        _autoSpeakResponses.value = enabled
        prefs.edit().putBoolean(KEY_AUTO_SPEAK, enabled).apply()
    }

    /**
     * Persist the current auto-send setting.
     *
     * @param enabled Whether to auto-send after voice recognition
     */
    fun setAutoSendAfterVoice(enabled: Boolean) {
        _autoSendAfterVoice.value = enabled
        prefs.edit().putBoolean(KEY_AUTO_SEND, enabled).apply()
    }

    /**
     * Release all registered providers. Call from the lifecycle owner's `onDestroy()`.
     */
    fun release() {
        sttProviders.forEach { it.release() }
        ttsProviders.forEach { it.release() }
    }

    private fun resolveProvider(providers: List<SpeechToTextProvider>, savedId: String?) =
        providers.find { it.providerId == savedId } ?: providers.first()

    private fun resolveProvider(providers: List<TextToSpeechProvider>, savedId: String?) =
        providers.find { it.providerId == savedId } ?: providers.first()

    companion object {
        internal const val PREFS_NAME = "citros_voice_prefs"
        internal const val KEY_ACTIVE_STT = "active_stt_provider"
        internal const val KEY_ACTIVE_TTS = "active_tts_provider"
        internal const val KEY_AUTO_SPEAK = "auto_speak_responses"
        internal const val KEY_AUTO_SEND = "auto_send_after_voice"
    }
}
