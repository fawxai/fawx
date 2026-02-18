package ai.citros.core

import org.junit.Assert.*
import org.junit.Test

/** Tests for [SpeechEvent] sealed class behavior. */
class SpeechEventTest {

    @Test
    fun `Partial event contains text`() {
        val event = SpeechEvent.Partial("hello")
        assertEquals("hello", event.text)
    }

    @Test
    fun `Final event contains text`() {
        val event = SpeechEvent.Final("hello world")
        assertEquals("hello world", event.text)
    }

    @Test
    fun `Error event contains SpeechError`() {
        val error = SpeechError.Timeout()
        val event = SpeechEvent.Error(error)
        assertSame(error, event.error)
    }

    @Test
    fun `sealed class exhaustive when`() {
        val events = listOf(
            SpeechEvent.Partial("hi"),
            SpeechEvent.Final("hi there"),
            SpeechEvent.Error(SpeechError.NetworkError())
        )
        events.forEach { event ->
            val result = when (event) {
                is SpeechEvent.Partial -> "partial"
                is SpeechEvent.Final -> "final"
                is SpeechEvent.Error -> "error"
            }
            assertNotNull(result)
        }
    }

    @Test
    fun `data class equality works`() {
        assertEquals(SpeechEvent.Partial("test"), SpeechEvent.Partial("test"))
        assertNotEquals(SpeechEvent.Partial("a"), SpeechEvent.Partial("b"))
        assertEquals(SpeechEvent.Final("done"), SpeechEvent.Final("done"))
    }

    @Test
    fun `data class copy works`() {
        val original = SpeechEvent.Partial("hello")
        val copy = original.copy(text = "world")
        assertEquals("world", copy.text)
    }
}
