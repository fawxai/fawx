package ai.citros.core

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.Rule
import org.junit.rules.TemporaryFolder
import java.io.File

/**
 * Unit tests for [SherpaOnnxSpeechToText].
 *
 * These tests verify configuration, state management, and error paths.
 * Actual audio recording and sherpa-onnx inference require Android hardware
 * and are not testable in JVM unit tests.
 */
class SherpaOnnxSpeechToTextTest {

    @get:Rule
    val tempFolder = TemporaryFolder()

    @Test
    fun `providerId returns sherpa-onnx`() {
        val provider = SherpaOnnxSpeechToText(modelDir = "/nonexistent")
        assertEquals("sherpa-onnx", provider.providerId)
    }

    @Test
    fun `displayName returns On-Device (Sherpa)`() {
        val provider = SherpaOnnxSpeechToText(modelDir = "/nonexistent")
        assertEquals("On-Device (Sherpa)", provider.displayName)
    }

    @Test
    fun `requiresNetwork returns false`() {
        val provider = SherpaOnnxSpeechToText(modelDir = "/nonexistent")
        assertFalse(provider.requiresNetwork)
    }

    @Test
    fun `isAvailable returns false when model dir does not exist`() {
        val provider = SherpaOnnxSpeechToText(modelDir = "/nonexistent/path")
        assertFalse(provider.isAvailable)
    }

    @Test
    fun `isAvailable returns false when model files are incomplete`() {
        val dir = tempFolder.newFolder("partial-model")
        // Only create some of the required files
        File(dir, "silero_vad.onnx").createNewFile()
        File(dir, "tokens.txt").createNewFile()
        // Missing: encoder, decoder, joiner

        val provider = SherpaOnnxSpeechToText(modelDir = dir.absolutePath)
        assertFalse(provider.isAvailable)
    }

    @Test
    fun `isAvailable returns false when modelDir is a file not a directory`() {
        val file = tempFolder.newFile("not-a-directory")
        val provider = SherpaOnnxSpeechToText(modelDir = file.absolutePath)
        assertFalse(provider.isAvailable)
    }

    @Test
    fun `isAvailable returns true when all model files exist`() {
        val dir = tempFolder.newFolder("complete-model")
        SherpaOnnxSpeechToText.REQUIRED_MODEL_FILES.forEach { filename ->
            File(dir, filename).createNewFile()
        }

        val provider = SherpaOnnxSpeechToText(modelDir = dir.absolutePath)
        assertTrue(provider.isAvailable)
    }

    @Test
    fun `constructor stores parameters correctly`() {
        val provider = SherpaOnnxSpeechToText(
            modelDir = "/some/path",
            numThreads = 4,
            sttProvider = "nnapi",
            modelType = "zipformer",
            timeoutMs = 60_000L,
        )

        assertEquals("/some/path", provider.modelDir)
        assertEquals(4, provider.numThreads)
        assertEquals("nnapi", provider.sttProvider)
        assertEquals("zipformer", provider.modelType)
        assertEquals(60_000L, provider.timeoutMs)
    }

    @Test
    fun `default parameters are correct`() {
        val provider = SherpaOnnxSpeechToText(modelDir = "/path")

        assertEquals(2, provider.numThreads)
        assertEquals("cpu", provider.sttProvider)
        assertEquals("nemo_transducer", provider.modelType)
        assertEquals(30_000L, provider.timeoutMs)
    }

    @Test(expected = IllegalArgumentException::class)
    fun `invalid sttProvider throws IllegalArgumentException`() {
        SherpaOnnxSpeechToText(modelDir = "/path", sttProvider = "gpu")
    }

    @Test(expected = IllegalArgumentException::class)
    fun `empty sttProvider throws IllegalArgumentException`() {
        SherpaOnnxSpeechToText(modelDir = "/path", sttProvider = "")
    }

    @Test(expected = IllegalArgumentException::class)
    fun `numThreads zero throws IllegalArgumentException`() {
        SherpaOnnxSpeechToText(modelDir = "/path", numThreads = 0)
    }

    @Test(expected = IllegalArgumentException::class)
    fun `numThreads negative throws IllegalArgumentException`() {
        SherpaOnnxSpeechToText(modelDir = "/path", numThreads = -1)
    }

    @Test
    fun `release is safe to call multiple times`() {
        val provider = SherpaOnnxSpeechToText(modelDir = "/nonexistent")
        // Should not throw
        provider.release()
        provider.release()
    }

    @Test
    fun `stopListening is safe to call before startListening`() {
        val provider = SherpaOnnxSpeechToText(modelDir = "/nonexistent")
        // Should not throw
        provider.stopListening()
    }

    @Test
    fun `cancel is safe to call before startListening`() {
        val provider = SherpaOnnxSpeechToText(modelDir = "/nonexistent")
        // Should not throw
        provider.cancel()
    }

    @Test
    fun `REQUIRED_MODEL_FILES contains all expected files`() {
        val expected = listOf(
            "silero_vad.onnx",
            "encoder.int8.onnx",
            "decoder.int8.onnx",
            "joiner.int8.onnx",
            "tokens.txt",
        )
        assertEquals(expected, SherpaOnnxSpeechToText.REQUIRED_MODEL_FILES)
    }

    @Test
    fun `companion constants have expected values`() {
        assertEquals(16000, SherpaOnnxSpeechToText.SAMPLE_RATE)
        assertEquals(30_000L, SherpaOnnxSpeechToText.DEFAULT_TIMEOUT_MS)
        assertEquals(512, SherpaOnnxSpeechToText.VAD_WINDOW_SIZE)
    }

    @Test
    fun `VAD silence duration is at least 1 second for natural speech pauses`() {
        // Regression test for #637: 0.5s silence duration caused premature
        // recording cutoff during natural speech pauses.
        assertTrue(
            "VAD_MIN_SILENCE_DURATION must be >= 1.0s to tolerate natural speech pauses (#637)",
            SherpaOnnxSpeechToText.VAD_MIN_SILENCE_DURATION >= 1.0f
        )
        assertEquals("nemo_transducer", SherpaOnnxSpeechToText.DEFAULT_MODEL_TYPE)
    }

    @Test
    fun `stopListening is idempotent when called multiple times`() {
        val provider = SherpaOnnxSpeechToText(modelDir = "/nonexistent")
        // Multiple calls should not throw — important because beginListening
        // calls stopListening() on the previous provider before starting new session (#637).
        provider.stopListening()
        provider.stopListening()
        provider.stopListening()
    }

    @Test
    fun `cancel is idempotent when called multiple times`() {
        val provider = SherpaOnnxSpeechToText(modelDir = "/nonexistent")
        provider.cancel()
        provider.cancel()
        provider.cancel()
    }

    @Test
    fun `stopListening followed by release is safe sequence`() {
        // Verifies the re-tap cleanup sequence: stopListening() to signal
        // the capture loop, then release() to free native resources.
        val provider = SherpaOnnxSpeechToText(modelDir = "/nonexistent")
        provider.stopListening()
        provider.release()
    }

    @Test
    fun `VALID_PROVIDERS contains cpu and nnapi`() {
        assertEquals(setOf("cpu", "nnapi"), SherpaOnnxSpeechToText.VALID_PROVIDERS)
    }

    @Test
    fun `sttProvider cpu is accepted`() {
        // Should not throw
        SherpaOnnxSpeechToText(modelDir = "/path", sttProvider = "cpu")
    }

    @Test
    fun `sttProvider nnapi is accepted`() {
        // Should not throw
        SherpaOnnxSpeechToText(modelDir = "/path", sttProvider = "nnapi")
    }
}
