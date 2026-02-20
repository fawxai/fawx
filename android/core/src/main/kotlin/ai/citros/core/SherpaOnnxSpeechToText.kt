package ai.citros.core

import android.Manifest
import android.annotation.SuppressLint
import android.content.Context
import android.content.pm.PackageManager
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import com.k2fsa.sherpa.onnx.OfflineModelConfig
import com.k2fsa.sherpa.onnx.OfflineRecognizer
import com.k2fsa.sherpa.onnx.OfflineRecognizerConfig
import com.k2fsa.sherpa.onnx.OfflineTransducerModelConfig
import com.k2fsa.sherpa.onnx.SileroVadModelConfig
import com.k2fsa.sherpa.onnx.Vad
import com.k2fsa.sherpa.onnx.VadModelConfig
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.callbackFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File
import java.util.concurrent.atomic.AtomicBoolean

/**
 * On-device speech-to-text provider using sherpa-onnx.
 *
 * Uses a "simulated streaming" architecture:
 * 1. Microphone audio captured via [AudioRecord] (16kHz mono PCM 16-bit)
 * 2. Audio fed to silero-VAD for voice activity detection
 * 3. When speech ends, accumulated audio segment sent to [OfflineRecognizer]
 * 4. Transcription result emitted as [SpeechEvent.Final]
 *
 * Each non-empty transcription result is emitted as a [SpeechEvent.Final].
 * The session continues listening until stopped via [stopListening], cancelled,
 * or timed out. Callers should accumulate Finals to build the complete
 * transcription. Empty transcription results (e.g. from background noise that
 * passes VAD) are silently discarded and the session continues listening.
 *
 * Model files must be pre-downloaded to [modelDir] before use. Required files:
 * - `silero_vad.onnx` (VAD model)
 * - `encoder.int8.onnx`, `decoder.int8.onnx`, `joiner.int8.onnx` (transducer model)
 * - `tokens.txt` (token vocabulary)
 *
 * **Threading:** The audio capture loop runs on [Dispatchers.IO]. When a speech
 * segment is detected, inference runs on [Dispatchers.Default] (CPU-bound) via
 * [withContext]. This blocks the capture loop during inference — a deliberate
 * trade-off for simplicity. Audio is buffered by the OS during inference, and
 * VAD processes it on the next iteration. For real-time streaming with concurrent
 * capture + inference, a producer-consumer architecture would be needed.
 *
 * **Lifecycle:** Callers must cancel the [startListening] flow before calling
 * [release]. The flow's `awaitClose` block signals the capture loop to stop.
 * Calling [release] while the flow is active may free native memory that the
 * capture loop is still using, which could cause a native crash.
 *
 * @param modelDir absolute path to directory containing model files
 * @param numThreads number of CPU threads for recognizer inference (default 2)
 * @param sttProvider inference provider for the recognizer — must be "cpu" or "nnapi" (default "cpu")
 * @param modelType sherpa-onnx model type identifier (default "nemo_transducer")
 * @param timeoutMs maximum recording duration in milliseconds (default 30000)
 * @throws IllegalArgumentException if [sttProvider] is not "cpu" or "nnapi"
 * @throws IllegalArgumentException if [numThreads] is less than 1
 */
class SherpaOnnxSpeechToText(
    internal val modelDir: String,
    internal val numThreads: Int = 2,
    internal val sttProvider: String = "cpu",
    internal val modelType: String = DEFAULT_MODEL_TYPE,
    internal val timeoutMs: Long = DEFAULT_TIMEOUT_MS,
) : SpeechToTextProvider {

    init {
        require(sttProvider in VALID_PROVIDERS) {
            "Invalid provider '$sttProvider'. Must be one of: ${VALID_PROVIDERS.joinToString()}"
        }
        require(numThreads >= 1) {
            "numThreads must be >= 1, got $numThreads"
        }
    }

    override val providerId: String = PROVIDER_ID
    override val displayName: String = DISPLAY_NAME
    override val requiresNetwork: Boolean = false

    override val isAvailable: Boolean
        get() {
            val dir = File(modelDir)
            return dir.isDirectory && REQUIRED_MODEL_FILES.all { File(dir, it).exists() }
        }

    private var recognizer: OfflineRecognizer? = null
    private var vad: Vad? = null
    private var appContext: Context? = null
    private val isListening = AtomicBoolean(false)

    override suspend fun initialize(context: Context) {
        appContext = context.applicationContext
        if (!isAvailable) {
            throw IllegalStateException("Model files not found at $modelDir")
        }

        // VAD uses single thread on CPU — Silero VAD is lightweight (~1ms per frame)
        // and doesn't benefit from multi-threading or NNAPI acceleration.
        val vadConfig = VadModelConfig(
            sileroVadModelConfig = SileroVadModelConfig(
                model = "$modelDir/silero_vad.onnx",
                threshold = VAD_THRESHOLD,
                minSilenceDuration = VAD_MIN_SILENCE_DURATION,
                minSpeechDuration = VAD_MIN_SPEECH_DURATION,
                windowSize = VAD_WINDOW_SIZE,
            ),
            sampleRate = SAMPLE_RATE,
            numThreads = 1,
            provider = "cpu",
        )

        val recognizerConfig = OfflineRecognizerConfig(
            modelConfig = OfflineModelConfig(
                transducer = OfflineTransducerModelConfig(
                    encoder = "$modelDir/encoder.int8.onnx",
                    decoder = "$modelDir/decoder.int8.onnx",
                    joiner = "$modelDir/joiner.int8.onnx",
                ),
                tokens = "$modelDir/tokens.txt",
                numThreads = numThreads,
                provider = sttProvider,
                modelType = modelType,
            ),
        )

        vad = Vad(config = vadConfig)
        recognizer = OfflineRecognizer(config = recognizerConfig)
    }

    override fun startListening(): Flow<SpeechEvent> = callbackFlow {
        val context = appContext
        if (context == null) {
            trySend(SpeechEvent.Error(SpeechError.Unavailable("Provider not initialized. Call initialize() first.")))
            close()
            return@callbackFlow
        }
        if (context.checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
            trySend(SpeechEvent.Error(SpeechError.PermissionDenied("Microphone permission denied")))
            close()
            return@callbackFlow
        }

        val currentVad = vad
        val currentRecognizer = recognizer
        if (currentVad == null || currentRecognizer == null) {
            trySend(SpeechEvent.Error(SpeechError.Unavailable("Provider not initialized. Call initialize() first.")))
            close()
            return@callbackFlow
        }

        isListening.set(true)

        val captureJob = launch(Dispatchers.IO) {
            var audioRecord: AudioRecord? = null
            try {
                val bufferSize = AudioRecord.getMinBufferSize(
                    SAMPLE_RATE,
                    AudioFormat.CHANNEL_IN_MONO,
                    AudioFormat.ENCODING_PCM_16BIT,
                )
                if (bufferSize == AudioRecord.ERROR || bufferSize == AudioRecord.ERROR_BAD_VALUE) {
                    trySend(SpeechEvent.Error(SpeechError.EngineError("Failed to determine audio buffer size")))
                    close()
                    return@launch
                }

                audioRecord = createAudioRecord(bufferSize)

                if (audioRecord.state != AudioRecord.STATE_INITIALIZED) {
                    trySend(SpeechEvent.Error(SpeechError.EngineError("Failed to initialize AudioRecord")))
                    close()
                    return@launch
                }

                audioRecord.startRecording()
                val startTime = System.currentTimeMillis()
                val shortBuffer = ShortArray(VAD_WINDOW_SIZE)
                // speechDetected tracks whether we've emitted a Partial. Only emitted
                // once per session since the session ends after the first Final.
                var speechDetected = false

                while (isActive && isListening.get()) {
                    // Check timeout
                    if (System.currentTimeMillis() - startTime > timeoutMs) {
                        trySend(SpeechEvent.Error(SpeechError.Timeout("Recording timed out after ${timeoutMs}ms")))
                        break
                    }

                    val readCount = audioRecord.read(shortBuffer, 0, shortBuffer.size)
                    if (readCount <= 0) continue

                    // Convert short PCM to float [-1.0, 1.0]
                    val floatBuffer = FloatArray(readCount) { shortBuffer[it] / 32768.0f }

                    currentVad.acceptWaveform(floatBuffer)

                    if (currentVad.isSpeechDetected() && !speechDetected) {
                        speechDetected = true
                        trySend(SpeechEvent.Partial("Listening..."))
                    }

                    // Drain all completed speech segments from VAD queue
                    while (!currentVad.empty()) {
                        val segment = currentVad.front()
                        currentVad.pop()

                        // Run offline recognition on the speech segment.
                        // This blocks the IO capture loop during inference (see class KDoc).
                        val text = withContext(Dispatchers.Default) {
                            val stream = currentRecognizer.createStream()
                            stream.acceptWaveform(segment.samples, SAMPLE_RATE)
                            currentRecognizer.decode(stream)
                            val result = currentRecognizer.getResult(stream)
                            stream.release()
                            result.text.trim()
                        }

                        if (text.isNotEmpty()) {
                            trySend(SpeechEvent.Final(text))
                        }
                        // Reset so next speech detection triggers a fresh Partial.
                        // Empty results (noise, brief sounds) are silently discarded.
                        speechDetected = false
                    }

                }
            } catch (e: SecurityException) {
                trySend(SpeechEvent.Error(SpeechError.PermissionDenied("Microphone permission denied: ${e.message}")))
            } catch (e: Exception) {
                trySend(SpeechEvent.Error(SpeechError.EngineError("Recording error: ${e.message}")))
            } finally {
                try {
                    audioRecord?.stop()
                    audioRecord?.release()
                } catch (_: Exception) { /* best effort cleanup */ }
                close()
            }
        }

        awaitClose {
            isListening.set(false)
            captureJob.cancel()
        }
    }

    override fun stopListening() {
        isListening.set(false)
    }

    override fun cancel() {
        isListening.set(false)
    }

    /**
     * Release all native resources.
     *
     * **Important:** Callers must ensure the [startListening] flow has been cancelled
     * (or completed) before calling this method. Calling [release] while the capture
     * loop is active may free native memory still in use, causing a native crash.
     * The recommended pattern is:
     * ```
     * listeningJob.cancel()  // cancels the flow, triggers awaitClose
     * provider.release()     // safe to call after flow completes
     * ```
     */
    override fun release() {
        isListening.set(false)
        recognizer?.release()
        recognizer = null
        vad?.release()
        vad = null
        appContext = null
    }

    @SuppressLint("MissingPermission")
    private fun createAudioRecord(bufferSize: Int): AudioRecord {
        return AudioRecord(
            MediaRecorder.AudioSource.MIC,
            SAMPLE_RATE,
            AudioFormat.CHANNEL_IN_MONO,
            AudioFormat.ENCODING_PCM_16BIT,
            bufferSize,
        )
    }

    internal companion object {
        const val PROVIDER_ID = "sherpa-onnx"
        const val DISPLAY_NAME = "On-Device (Sherpa)"
        const val SAMPLE_RATE = 16000
        const val DEFAULT_TIMEOUT_MS = 30_000L
        const val DEFAULT_MODEL_TYPE = "nemo_transducer"
        const val VAD_THRESHOLD = 0.5f
        const val VAD_MIN_SILENCE_DURATION = 1.5f
        const val VAD_MIN_SPEECH_DURATION = 0.25f
        const val VAD_WINDOW_SIZE = 512

        val VALID_PROVIDERS = setOf("cpu", "nnapi")

        val REQUIRED_MODEL_FILES = listOf(
            "silero_vad.onnx",
            "encoder.int8.onnx",
            "decoder.int8.onnx",
            "joiner.int8.onnx",
            "tokens.txt",
        )
    }
}
