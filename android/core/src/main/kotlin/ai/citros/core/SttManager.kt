package ai.citros.core

import android.content.SharedPreferences
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

enum class SttState {
    INITIALIZING,
    SHERPA_READY,
    AWAITING_CONSENT,
    ANDROID_FALLBACK,
    FAILED,
}

enum class CloudSttConsent {
    UNASKED,
    ACCEPTED,
    DECLINED_TYPE,
    DECLINED_WAIT,
}

interface SttCallback {
    fun onResult(text: String)
    fun onPartialResult(text: String)
    fun onError(message: String)
}

/**
 * Coordinates STT initialization and fallback behavior.
 *
 * Privacy contract:
 * - Never silently switch to cloud STT.
 * - Android fallback is only used after explicit user consent (ACCEPTED).
 */
class SttManager(
    private val prefs: SharedPreferences,
    private val initializeSherpa: () -> Unit,
    private val isAndroidSttAvailable: () -> Boolean,
    private val startSherpaListening: (SttCallback) -> Unit,
    private val startAndroidListening: (SttCallback) -> Unit,
    private val kickOffModelDownload: () -> Unit,
    private val showCloudIndicator: () -> Unit = {},
    private val hideCloudIndicator: () -> Unit = {},
) {
    private val _state = MutableStateFlow(SttState.INITIALIZING)
    val state: StateFlow<SttState> = _state.asStateFlow()

    private val _cloudIndicatorVisible = MutableStateFlow(false)
    val cloudIndicatorVisible: StateFlow<Boolean> = _cloudIndicatorVisible.asStateFlow()

    private val lock = Any()

    var cloudConsent: CloudSttConsent
        get() = parseConsent(
            prefs.getString(KEY_CLOUD_STT_CONSENT, CloudSttConsent.UNASKED.name)
        )
        private set(value) {
            prefs.edit().putString(KEY_CLOUD_STT_CONSENT, value.name).commit()
        }

    fun initialize(): SttState = synchronized(lock) {
        _state.value = SttState.INITIALIZING
        _state.value = try {
            initializeSherpa()
            hideCloudIndicatorInternal()
            transition(_state.value, SttEvent.SHERPA_INIT_SUCCEEDED)
        } catch (_: Exception) {
            resolveFallbackState()
        }

        if (_state.value != SttState.SHERPA_READY) {
            kickOffModelDownload()
        }
        _state.value
    }

    fun handleConsentChoice(choice: CloudSttConsent): SttState = synchronized(lock) {
        require(choice != CloudSttConsent.UNASKED) {
            "UNASKED is not a valid consent choice"
        }

        cloudConsent = choice
        _state.value = when (choice) {
            CloudSttConsent.ACCEPTED -> {
                showCloudIndicatorInternal()
                transition(_state.value, SttEvent.USER_CONSENTED)
            }
            CloudSttConsent.DECLINED_TYPE,
            CloudSttConsent.DECLINED_WAIT -> {
                hideCloudIndicatorInternal()
                transition(_state.value, SttEvent.USER_DECLINED)
            }
            CloudSttConsent.UNASKED -> error("Handled by require")
        }
        _state.value
    }

    fun startListening(callback: SttCallback) {
        when (_state.value) {
            SttState.SHERPA_READY -> {
                hideCloudIndicatorInternal()
                startSherpaListening(callback)
            }
            SttState.ANDROID_FALLBACK -> {
                showCloudIndicatorInternal()
                startAndroidListening(callback)
            }
            SttState.AWAITING_CONSENT -> callback.onError("Waiting for user consent")
            SttState.FAILED -> callback.onError("No speech recognition available")
            SttState.INITIALIZING -> callback.onError("STT not initialized")
        }
    }

    fun onModelsDownloaded(): SttState = synchronized(lock) {
        if (
            _state.value != SttState.ANDROID_FALLBACK &&
            _state.value != SttState.FAILED &&
            _state.value != SttState.AWAITING_CONSENT
        ) {
            return@synchronized _state.value
        }

        return@synchronized try {
            initializeSherpa()
            hideCloudIndicatorInternal()
            _state.value = transition(_state.value, SttEvent.MODELS_DOWNLOADED_SHERPA_READY)
            _state.value
        } catch (_: Exception) {
            _state.value
        }
    }

    private fun resolveFallbackState(): SttState {
        if (!isAndroidSttAvailable()) {
            hideCloudIndicatorInternal()
            return transition(_state.value, SttEvent.NO_ANDROID_STT_AVAILABLE)
        }

        return when (cloudConsent) {
            CloudSttConsent.UNASKED -> {
                hideCloudIndicatorInternal()
                transition(_state.value, SttEvent.CONSENT_REQUIRED)
            }
            CloudSttConsent.ACCEPTED -> {
                showCloudIndicatorInternal()
                transition(_state.value, SttEvent.USER_CONSENTED)
            }
            CloudSttConsent.DECLINED_TYPE,
            CloudSttConsent.DECLINED_WAIT -> {
                hideCloudIndicatorInternal()
                transition(_state.value, SttEvent.USER_DECLINED)
            }
        }
    }

    private fun transition(from: SttState, event: SttEvent): SttState {
        return when (event) {
            SttEvent.SHERPA_INIT_SUCCEEDED,
            SttEvent.MODELS_DOWNLOADED_SHERPA_READY -> SttState.SHERPA_READY

            SttEvent.CONSENT_REQUIRED -> SttState.AWAITING_CONSENT
            SttEvent.USER_CONSENTED -> SttState.ANDROID_FALLBACK
            SttEvent.USER_DECLINED,
            SttEvent.NO_ANDROID_STT_AVAILABLE -> SttState.FAILED
        }
    }

    private fun parseConsent(raw: String?): CloudSttConsent {
        return runCatching { CloudSttConsent.valueOf(raw ?: CloudSttConsent.UNASKED.name) }
            .getOrDefault(CloudSttConsent.UNASKED)
    }

    private fun showCloudIndicatorInternal() {
        _cloudIndicatorVisible.value = true
        showCloudIndicator()
    }

    private fun hideCloudIndicatorInternal() {
        _cloudIndicatorVisible.value = false
        hideCloudIndicator()
    }

    private enum class SttEvent {
        SHERPA_INIT_SUCCEEDED,
        MODELS_DOWNLOADED_SHERPA_READY,
        CONSENT_REQUIRED,
        USER_CONSENTED,
        USER_DECLINED,
        NO_ANDROID_STT_AVAILABLE,
    }

    companion object {
        const val KEY_CLOUD_STT_CONSENT = "cloud_stt_consent"
    }
}
