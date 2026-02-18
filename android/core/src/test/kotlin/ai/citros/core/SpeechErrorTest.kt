package ai.citros.core

import org.junit.Assert.*
import org.junit.Test

/** Tests for [SpeechError] sealed class behavior. */
class SpeechErrorTest {

    @Test
    fun `PermissionDenied has default message`() {
        val error = SpeechError.PermissionDenied()
        assertEquals("Microphone permission denied", error.message)
    }

    @Test
    fun `PermissionDenied accepts custom message`() {
        val error = SpeechError.PermissionDenied("Custom message")
        assertEquals("Custom message", error.message)
    }

    @Test
    fun `NetworkError has default message`() {
        val error = SpeechError.NetworkError()
        assertEquals("Network error", error.message)
    }

    @Test
    fun `EngineError has default message`() {
        val error = SpeechError.EngineError()
        assertEquals("Recognition engine error", error.message)
    }

    @Test
    fun `Timeout has default message`() {
        val error = SpeechError.Timeout()
        assertEquals("Recognition timed out", error.message)
    }

    @Test
    fun `Unavailable has default message`() {
        val error = SpeechError.Unavailable()
        assertEquals("Speech recognition unavailable", error.message)
    }

    @Test
    fun `sealed class exhaustive when`() {
        val errors: List<SpeechError> = listOf(
            SpeechError.PermissionDenied(),
            SpeechError.NetworkError(),
            SpeechError.EngineError(),
            SpeechError.Timeout(),
            SpeechError.Unavailable()
        )
        errors.forEach { error ->
            val result = when (error) {
                is SpeechError.PermissionDenied -> "permission"
                is SpeechError.NetworkError -> "network"
                is SpeechError.EngineError -> "engine"
                is SpeechError.Timeout -> "timeout"
                is SpeechError.Unavailable -> "unavailable"
            }
            assertNotNull(result)
        }
    }

    @Test
    fun `data class equality works`() {
        assertEquals(SpeechError.Timeout(), SpeechError.Timeout())
        assertEquals(
            SpeechError.PermissionDenied("msg"),
            SpeechError.PermissionDenied("msg")
        )
        assertNotEquals(
            SpeechError.PermissionDenied("a"),
            SpeechError.PermissionDenied("b")
        )
    }

    @Test
    fun `Android error code mapping`() {
        // ERROR_INSUFFICIENT_PERMISSIONS = 9
        assertTrue(
            AndroidSpeechToText.mapError(9) is SpeechError.PermissionDenied
        )
        // ERROR_NETWORK = 2
        assertTrue(
            AndroidSpeechToText.mapError(2) is SpeechError.NetworkError
        )
        // ERROR_NETWORK_TIMEOUT = 1
        assertTrue(
            AndroidSpeechToText.mapError(1) is SpeechError.NetworkError
        )
        // ERROR_NO_MATCH = 7
        assertTrue(
            AndroidSpeechToText.mapError(7) is SpeechError.Timeout
        )
        // ERROR_SPEECH_TIMEOUT = 6
        assertTrue(
            AndroidSpeechToText.mapError(6) is SpeechError.Timeout
        )
        // ERROR_RECOGNIZER_BUSY = 8
        assertTrue(
            AndroidSpeechToText.mapError(8) is SpeechError.EngineError
        )
        // ERROR_AUDIO = 3
        assertTrue(
            AndroidSpeechToText.mapError(3) is SpeechError.EngineError
        )
        // ERROR_SERVER = 4
        assertTrue(
            AndroidSpeechToText.mapError(4) is SpeechError.EngineError
        )
        // ERROR_CLIENT = 5
        assertTrue(
            AndroidSpeechToText.mapError(5) is SpeechError.EngineError
        )
        // Unknown error code
        assertTrue(
            AndroidSpeechToText.mapError(999) is SpeechError.Unavailable
        )
    }
}
