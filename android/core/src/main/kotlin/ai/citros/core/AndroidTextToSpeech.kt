package ai.citros.core

import android.content.Context
import android.speech.tts.TextToSpeech
import android.speech.tts.UtteranceProgressListener
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import kotlinx.coroutines.suspendCancellableCoroutine
import java.util.Locale
import java.util.UUID
import java.util.concurrent.ConcurrentLinkedQueue
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

/**
 * Android on-device [TextToSpeechProvider] implementation.
 *
 * Wraps [android.speech.tts.TextToSpeech] with proper async initialization
 * handling. Requests arriving before initialization completes are queued
 * and played once the engine is ready.
 *
 * **Initialization:** Call [initialize] before [speak]. Uses
 * [suspendCancellableCoroutine] to await the `onInit` callback.
 *
 * **Speak:** Uses [suspendCancellableCoroutine] with [UtteranceProgressListener]
 * to suspend until playback completes.
 */
class AndroidTextToSpeech : TextToSpeechProvider {

    override val providerId: String = "android"
    override val displayName: String = "On-Device"
    override val requiresNetwork: Boolean = false

    private var tts: TextToSpeech? = null
    private var ready: Boolean = false
    private val pendingQueue = ConcurrentLinkedQueue<PendingUtterance>()
    private val scope = CoroutineScope(Dispatchers.Main)

    /** Active continuation for the currently playing utterance. */
    @Volatile
    private var activeContinuation: kotlinx.coroutines.CancellableContinuation<Unit>? = null

    override val isAvailable: Boolean
        get() = ready

    /** Whether the engine is currently speaking. */
    override val isSpeaking: Boolean
        get() = tts?.isSpeaking == true

    override suspend fun initialize(context: Context) {
        if (ready) return
        val appContext = context.applicationContext
        val engine = suspendCancellableCoroutine { cont ->
            var localEngine: TextToSpeech? = null
            localEngine = TextToSpeech(appContext) { status ->
                if (status == TextToSpeech.SUCCESS) {
                    tts = localEngine
                    localEngine?.language = Locale.getDefault()
                    ready = true
                    // Drain any queued utterances
                    drainQueue()
                    cont.resume(localEngine!!)
                } else {
                    cont.resumeWithException(
                        IllegalStateException("TTS initialization failed with status $status")
                    )
                }
            }
            cont.invokeOnCancellation {
                localEngine?.shutdown()
            }
        }
        // engine is now the initialized TextToSpeech instance
    }

    override suspend fun speak(text: String, options: TtsOptions) {
        val engine = tts
        if (engine == null || !ready) {
            // Queue for when init completes
            suspendCancellableCoroutine { cont ->
                pendingQueue.add(PendingUtterance(text, options, cont))
                cont.invokeOnCancellation {
                    pendingQueue.removeAll { it.continuation === cont }
                }
            }
            return
        }

        speakInternal(engine, text, options)
    }

    private suspend fun speakInternal(
        engine: TextToSpeech,
        text: String,
        options: TtsOptions
    ) {
        val utteranceId = UUID.randomUUID().toString()

        engine.setSpeechRate(options.speed)
        engine.setPitch(options.pitch)

        val queueMode = when (options.queueMode) {
            QueueMode.FLUSH -> TextToSpeech.QUEUE_FLUSH
            QueueMode.ADD -> TextToSpeech.QUEUE_ADD
        }

        suspendCancellableCoroutine { cont ->
            // Cancel any previous active continuation when using FLUSH mode
            if (queueMode == TextToSpeech.QUEUE_FLUSH) {
                activeContinuation?.let { prev ->
                    if (prev.isActive) {
                        prev.resumeWithException(
                            java.util.concurrent.CancellationException("Superseded by new utterance")
                        )
                    }
                }
            }
            activeContinuation = cont

            engine.setOnUtteranceProgressListener(object : UtteranceProgressListener() {
                override fun onStart(id: String?) {}

                override fun onDone(id: String?) {
                    if (id == utteranceId && cont.isActive) {
                        activeContinuation = null
                        cont.resume(Unit)
                    }
                }

                @Deprecated("Deprecated in Java")
                override fun onError(id: String?) {
                    if (id == utteranceId && cont.isActive) {
                        activeContinuation = null
                        cont.resumeWithException(
                            IllegalStateException("TTS playback error for utterance $id")
                        )
                    }
                }

                override fun onError(id: String?, errorCode: Int) {
                    if (id == utteranceId && cont.isActive) {
                        activeContinuation = null
                        cont.resumeWithException(
                            IllegalStateException("TTS playback error (code $errorCode)")
                        )
                    }
                }
            })

            val params = android.os.Bundle()
            engine.speak(text, queueMode, params, utteranceId)

            cont.invokeOnCancellation {
                activeContinuation = null
                engine.stop()
            }
        }
    }

    override fun stop() {
        tts?.stop()
    }

    override fun release() {
        scope.cancel()
        ready = false
        pendingQueue.clear()
        activeContinuation = null
        tts?.stop()
        tts?.shutdown()
        tts = null
    }

    private fun drainQueue() {
        val engine = tts ?: return
        val items = buildList {
            while (pendingQueue.isNotEmpty()) {
                pendingQueue.poll()?.let { add(it) }
            }
        }
        if (items.isEmpty()) return
        scope.launch {
            for (pending in items) {
                try {
                    speakInternal(engine, pending.text, pending.options)
                    if (pending.continuation.isActive) pending.continuation.resume(Unit)
                } catch (e: Exception) {
                    if (pending.continuation.isActive) pending.continuation.resumeWithException(e)
                }
            }
        }
    }

    private data class PendingUtterance(
        val text: String,
        val options: TtsOptions,
        val continuation: kotlinx.coroutines.CancellableContinuation<Unit>
    )
}
