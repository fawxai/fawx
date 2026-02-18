package ai.citros.core

import kotlinx.coroutines.test.runTest
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import java.io.File

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28])
class SherpaOnnxTextToSpeechTest {

    @Test
    fun `providerId returns sherpa-onnx`() {
        val tts = SherpaOnnxTextToSpeech("/fake/model/dir")
        assertEquals("sherpa-onnx", tts.providerId)
    }

    @Test
    fun `displayName returns expected value`() {
        val tts = SherpaOnnxTextToSpeech("/fake/model/dir")
        assertEquals("On-Device (Sherpa)", tts.displayName)
    }

    @Test
    fun `requiresNetwork returns false`() {
        val tts = SherpaOnnxTextToSpeech("/fake/model/dir")
        assertFalse(tts.requiresNetwork)
    }

    @Test
    fun `isAvailable returns false before initialization`() {
        val tts = SherpaOnnxTextToSpeech("/fake/model/dir")
        assertFalse(tts.isAvailable)
    }

    @Test
    fun `isSpeaking returns false before initialization`() {
        val tts = SherpaOnnxTextToSpeech("/fake/model/dir")
        assertFalse(tts.isSpeaking)
    }

    @Test
    fun `stop does not throw before initialization`() {
        val tts = SherpaOnnxTextToSpeech("/fake/model/dir")
        // Should not throw
        tts.stop()
    }

    @Test
    fun `release does not throw before initialization`() {
        val tts = SherpaOnnxTextToSpeech("/fake/model/dir")
        // Should not throw
        tts.release()
    }

    @Test
    fun `constructor accepts custom numThreads`() {
        val tts = SherpaOnnxTextToSpeech("/fake/model/dir", numThreads = 4)
        assertEquals("sherpa-onnx", tts.providerId)
    }

    @Test(expected = IllegalStateException::class)
    fun `speak throws IllegalStateException before initialization`() = runTest {
        val tts = SherpaOnnxTextToSpeech("/fake/model/dir")
        tts.speak("hello")
    }

    @Test(expected = IllegalArgumentException::class)
    fun `constructor rejects zero numThreads`() {
        SherpaOnnxTextToSpeech("/fake/model/dir", numThreads = 0)
    }
}
