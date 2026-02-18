package ai.citros.core

import org.junit.Assert.*
import org.junit.Test

/** Tests for [TtsOptions] and [QueueMode]. */
class TtsOptionsTest {

    @Test
    fun `default options`() {
        val options = TtsOptions()
        assertEquals(1.0f, options.speed, 0.001f)
        assertEquals(1.0f, options.pitch, 0.001f)
        assertEquals(QueueMode.FLUSH, options.queueMode)
    }

    @Test
    fun `custom options`() {
        val options = TtsOptions(speed = 1.5f, pitch = 0.8f, queueMode = QueueMode.ADD)
        assertEquals(1.5f, options.speed, 0.001f)
        assertEquals(0.8f, options.pitch, 0.001f)
        assertEquals(QueueMode.ADD, options.queueMode)
    }

    @Test
    fun `data class copy works`() {
        val original = TtsOptions(speed = 2.0f)
        val copy = original.copy(pitch = 0.5f)
        assertEquals(2.0f, copy.speed, 0.001f)
        assertEquals(0.5f, copy.pitch, 0.001f)
        assertEquals(QueueMode.FLUSH, copy.queueMode)
    }

    @Test
    fun `data class equality`() {
        assertEquals(TtsOptions(), TtsOptions())
        assertNotEquals(
            TtsOptions(speed = 1.0f),
            TtsOptions(speed = 2.0f)
        )
    }

    @Test
    fun `QueueMode values`() {
        val values = QueueMode.values()
        assertEquals(2, values.size)
        assertEquals(QueueMode.FLUSH, values[0])
        assertEquals(QueueMode.ADD, values[1])
    }

    @Test
    fun `QueueMode valueOf`() {
        assertEquals(QueueMode.FLUSH, QueueMode.valueOf("FLUSH"))
        assertEquals(QueueMode.ADD, QueueMode.valueOf("ADD"))
    }
}
