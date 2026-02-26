package ai.citros.core

import kotlin.test.assertEquals
import kotlin.test.assertTrue
import org.junit.Test

class TaskStateSerializationTest {

    @Test
    fun `toSerializedToolCall and toToolCall round trip preserves id name and input`() {
        val original = ToolCall(
            id = "call-1",
            name = "tap",
            input = mapOf(
                "x" to 120,
                "y" to 340,
                "label" to "Settings",
                "enabled" to true,
                "scale" to 1.5,
                "items" to listOf("a", "b"),
                "meta" to mapOf("nested" to "value")
            )
        )

        val restored = original.toSerializedToolCall().toToolCall()

        assertEquals(original.id, restored.id)
        assertEquals(original.name, restored.name)
        assertEquals(original.input, restored.input)
    }

    @Test
    fun `toToolCall returns empty input map for malformed json`() {
        val serialized = SerializedToolCall(
            id = "call-2",
            name = "type",
            inputJson = "{not valid json"
        )

        val restored = serialized.toToolCall()

        assertEquals("call-2", restored.id)
        assertEquals("type", restored.name)
        assertTrue(restored.input.isEmpty())
    }

    @Test
    fun `toToolCall returns empty input map when input json is not an object`() {
        val serialized = SerializedToolCall(
            id = "call-3",
            name = "swipe",
            inputJson = "[1,2,3]"
        )

        val restored = serialized.toToolCall()

        assertEquals("call-3", restored.id)
        assertEquals("swipe", restored.name)
        assertTrue(restored.input.isEmpty())
    }
}
