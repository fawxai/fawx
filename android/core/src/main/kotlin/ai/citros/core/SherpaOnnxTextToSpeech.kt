package ai.citros.core

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack
import com.k2fsa.sherpa.onnx.GeneratedAudio
import com.k2fsa.sherpa.onnx.OfflineTts
import com.k2fsa.sherpa.onnx.OfflineTtsConfig
import com.k2fsa.sherpa.onnx.OfflineTtsModelConfig
import com.k2fsa.sherpa.onnx.OfflineTtsVitsModelConfig
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import java.io.File
import java.util.concurrent.atomic.AtomicBoolean

/**
 * On-device text-to-speech using sherpa-onnx with a Piper VITS model.
 *
 * Generates raw PCM audio via [OfflineTts] and plays it through [AudioTrack].
 * Generation runs on [Dispatchers.Default] (CPU-bound), playback on
 * [Dispatchers.IO].
 *
 * **Limitations:**
 * - Pitch control is not supported by Piper VITS models; [TtsOptions.pitch]
 *   is ignored.
 * - Generation may take 1–3 seconds per sentence on mobile devices.
 *
 * @param modelDir Path to the directory containing the TTS model files
 * @param numThreads Number of CPU threads for inference (default: 2)
 * @see AndroidTextToSpeech for the platform TTS alternative
 */
class SherpaOnnxTextToSpeech(
    private val modelDir: String,
    private val numThreads: Int = 2,
) : TextToSpeechProvider {

    override val providerId: String = "sherpa-onnx"
    override val displayName: String = "On-Device (Sherpa)"
    override val requiresNetwork: Boolean = false

    @Volatile private var tts: OfflineTts? = null
    @Volatile private var audioTrack: AudioTrack? = null
    private val speaking = AtomicBoolean(false)
    private val initMutex = Mutex()
    private val playbackMutex = Mutex()

    override val isAvailable: Boolean
        get() = tts != null

    override val isSpeaking: Boolean
        get() = speaking.get()

    init {
        require(numThreads >= 1) { "numThreads must be >= 1, got $numThreads" }
    }

    override suspend fun initialize(context: Context) {
        if (tts != null) return
        initMutex.withLock {
            if (tts != null) return  // Double-check after acquiring lock
            val config = OfflineTtsConfig(
                model = OfflineTtsModelConfig(
                    vits = OfflineTtsVitsModelConfig(
                        model = "$modelDir/en_US-lessac-high.onnx",
                        tokens = "$modelDir/tokens.txt",
                        dataDir = "$modelDir/${ModelManager.ESPEAK_NG_DATA_DIR}",
                        lengthScale = 1.0f,
                    ),
                    numThreads = numThreads,
                    provider = "cpu",
                ),
            )
            tts = withContext(Dispatchers.Default) {
                OfflineTts(config = config)
            }
        }
    }

    override suspend fun speak(text: String, options: TtsOptions) {
        val engine = tts ?: throw IllegalStateException("TTS not initialized")

        if (options.queueMode == QueueMode.FLUSH) {
            stop()
        }

        // For ADD mode, wait for current playback to finish
        playbackMutex.withLock {
            // Generate audio (CPU-bound)
            val audio = withContext(Dispatchers.Default) {
                currentCoroutineContext().ensureActive()
                engine.generate(text = text, sid = 0, speed = options.speed)
            }

            // Play via AudioTrack (IO-bound)
            withContext(Dispatchers.IO) {
                playAudio(audio)
            }
        }
    }

    override fun stop() {
        speaking.set(false)
        audioTrack?.let { track ->
            try {
                track.pause()
                track.flush()
                track.stop()
            } catch (_: IllegalStateException) {
                // Already stopped
            }
        }
        releaseAudioTrack()
    }

    override fun release() {
        stop()
        tts?.release()
        tts = null
    }

    private fun playAudio(audio: GeneratedAudio) {
        val samples = audio.samples
        if (samples.isEmpty()) return

        val sampleRate = audio.sampleRate

        // Convert float samples [-1.0, 1.0] to 16-bit PCM
        val pcmData = ShortArray(samples.size) { i ->
            (samples[i].coerceIn(-1.0f, 1.0f) * Short.MAX_VALUE).toInt().toShort()
        }

        val bufferSize = AudioTrack.getMinBufferSize(
            sampleRate,
            AudioFormat.CHANNEL_OUT_MONO,
            AudioFormat.ENCODING_PCM_16BIT
        )

        val track = AudioTrack.Builder()
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_ASSISTANT)
                    .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
                    .build()
            )
            .setAudioFormat(
                AudioFormat.Builder()
                    .setSampleRate(sampleRate)
                    .setChannelMask(AudioFormat.CHANNEL_OUT_MONO)
                    .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                    .build()
            )
            .setBufferSizeInBytes(maxOf(bufferSize, pcmData.size * 2))
            .setTransferMode(AudioTrack.MODE_STATIC)
            .build()

        audioTrack = track
        speaking.set(true)

        try {
            track.write(pcmData, 0, pcmData.size)
            track.play()

            // Wait for playback to complete using playback head position
            while (speaking.get() && track.playbackHeadPosition < pcmData.size) {
                Thread.sleep(20)
            }
        } finally {
            speaking.set(false)
            releaseAudioTrack()
        }
    }

    private fun releaseAudioTrack() {
        audioTrack?.let { track ->
            try {
                track.release()
            } catch (_: Exception) {
                // Ignore release errors
            }
        }
        audioTrack = null
    }
}
