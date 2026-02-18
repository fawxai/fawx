package ai.citros.core

import android.content.Context
import android.content.Intent
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.speech.RecognitionListener
import android.speech.RecognizerIntent
import android.speech.SpeechRecognizer
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.callbackFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * Android on-device [SpeechToTextProvider] implementation.
 *
 * Wraps [android.speech.SpeechRecognizer] in a [Flow]-based API. The recognizer
 * is created and managed on the main thread (Android requirement). A hard timeout
 * (default 30s) prevents indefinite hangs on devices with broken implementations.
 *
 * **State machine:** `IDLE → LISTENING → PROCESSING → done`
 *
 * **Cancellation:** Cancelling the flow returned by [startListening] automatically
 * stops and destroys the recognizer via the `awaitClose` block.
 *
 * @param timeoutMs Hard timeout in milliseconds for recognition sessions. Default 30s.
 */
class AndroidSpeechToText(
    private val timeoutMs: Long = DEFAULT_TIMEOUT_MS
) : SpeechToTextProvider {

    override val providerId: String = "android"
    override val displayName: String = "On-Device"
    override val requiresNetwork: Boolean = false

    private var context: Context? = null
    private var recognizer: SpeechRecognizer? = null
    private var state: State = State.IDLE
    private val mainHandler = Handler(Looper.getMainLooper())

    override val isAvailable: Boolean
        get() {
            val ctx = context ?: return false
            return SpeechRecognizer.isRecognitionAvailable(ctx)
        }

    override suspend fun initialize(context: Context) {
        this.context = context.applicationContext
    }

    override fun startListening(): Flow<SpeechEvent> = callbackFlow {
        val ctx = context
            ?: throw IllegalStateException("AndroidSpeechToText not initialized. Call initialize() first.")

        if (state != State.IDLE) {
            trySend(SpeechEvent.Error(SpeechError.EngineError("Recognizer busy")))
            close()
            return@callbackFlow
        }

        if (!SpeechRecognizer.isRecognitionAvailable(ctx)) {
            trySend(SpeechEvent.Error(SpeechError.Unavailable()))
            close()
            return@callbackFlow
        }

        val localRecognizer: SpeechRecognizer
        // Fix #5: All state/field assignments on Main thread to avoid race conditions
        withContext(Dispatchers.Main) {
            localRecognizer = SpeechRecognizer.createSpeechRecognizer(ctx)
            recognizer = localRecognizer
            state = State.LISTENING
        }

        val timeoutRunnable = Runnable {
            if (state != State.IDLE) {
                state = State.IDLE
                trySend(SpeechEvent.Error(SpeechError.Timeout()))
                localRecognizer.stopListening()
                close()
            }
        }

        val listener = object : RecognitionListener {
            override fun onReadyForSpeech(params: Bundle?) {}
            override fun onBeginningOfSpeech() {}
            override fun onRmsChanged(rmsdB: Float) {}
            override fun onBufferReceived(buffer: ByteArray?) {}
            override fun onEndOfSpeech() {
                state = State.PROCESSING
            }

            override fun onPartialResults(partialResults: Bundle?) {
                val matches = partialResults
                    ?.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION)
                val text = matches?.firstOrNull()
                if (!text.isNullOrBlank()) {
                    trySend(SpeechEvent.Partial(text))
                }
            }

            override fun onResults(results: Bundle?) {
                mainHandler.removeCallbacks(timeoutRunnable)
                state = State.IDLE
                val matches = results
                    ?.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION)
                val text = matches?.firstOrNull().orEmpty()
                // Fix #3: Don't emit Final("") on empty results
                if (text.isBlank()) {
                    trySend(SpeechEvent.Error(SpeechError.Timeout("No speech detected")))
                } else {
                    trySend(SpeechEvent.Final(text))
                }
                close()
            }

            override fun onError(error: Int) {
                mainHandler.removeCallbacks(timeoutRunnable)
                state = State.IDLE
                trySend(SpeechEvent.Error(mapError(error)))
                close()
            }

            override fun onEvent(eventType: Int, params: Bundle?) {}
        }

        val intent = Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH).apply {
            putExtra(
                RecognizerIntent.EXTRA_LANGUAGE_MODEL,
                RecognizerIntent.LANGUAGE_MODEL_FREE_FORM
            )
            putExtra(RecognizerIntent.EXTRA_PARTIAL_RESULTS, true)
        }

        // All recognizer calls must be on the main thread
        mainHandler.post {
            localRecognizer.setRecognitionListener(listener)
            localRecognizer.startListening(intent)
            mainHandler.postDelayed(timeoutRunnable, timeoutMs)
        }

        awaitClose {
            mainHandler.removeCallbacks(timeoutRunnable)
            mainHandler.post {
                state = State.IDLE
                localRecognizer.stopListening()
                localRecognizer.destroy()
            }
            recognizer = null
        }
    }

    override fun stopListening() {
        mainHandler.post {
            recognizer?.stopListening()
        }
    }

    override fun cancel() {
        mainHandler.post {
            recognizer?.cancel()
            state = State.IDLE
        }
    }

    override fun release() {
        // Fix #2: Capture reference before nulling to avoid leaking SpeechRecognizer
        val toDestroy = recognizer
        recognizer = null
        state = State.IDLE
        context = null
        mainHandler.post {
            toDestroy?.destroy()
        }
    }

    private enum class State { IDLE, LISTENING, PROCESSING }

    companion object {
        /** Default hard timeout for recognition sessions (30 seconds). */
        const val DEFAULT_TIMEOUT_MS = 30_000L

        /**
         * Maps Android [SpeechRecognizer] error codes to [SpeechError] subtypes.
         *
         * @param errorCode Error code from [RecognitionListener.onError]
         * @return Corresponding [SpeechError]
         */
        internal fun mapError(errorCode: Int): SpeechError = when (errorCode) {
            SpeechRecognizer.ERROR_INSUFFICIENT_PERMISSIONS ->
                SpeechError.PermissionDenied()
            SpeechRecognizer.ERROR_NETWORK,
            SpeechRecognizer.ERROR_NETWORK_TIMEOUT ->
                SpeechError.NetworkError()
            SpeechRecognizer.ERROR_NO_MATCH,
            SpeechRecognizer.ERROR_SPEECH_TIMEOUT ->
                SpeechError.Timeout("No speech detected")
            SpeechRecognizer.ERROR_RECOGNIZER_BUSY,
            SpeechRecognizer.ERROR_CLIENT ->
                SpeechError.EngineError("Recognizer error (code $errorCode)")
            SpeechRecognizer.ERROR_AUDIO ->
                SpeechError.EngineError("Audio recording error")
            SpeechRecognizer.ERROR_SERVER ->
                SpeechError.EngineError("Server error")
            else -> SpeechError.Unavailable("Unknown error (code $errorCode)")
        }
    }
}
