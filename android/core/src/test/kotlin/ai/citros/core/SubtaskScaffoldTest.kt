package ai.citros.core

import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertTrue

class SubtaskScaffoldTest {

    @Test
    fun `subtask tool schema includes required fields and defaults`() {
        val schema = PhoneTools.SUBTASK.inputSchema
        val properties = schema["properties"] as Map<*, *>

        assertTrue(properties.containsKey("goal"))
        assertTrue(properties.containsKey("success_criteria"))
        assertTrue(properties.containsKey("max_steps"))
        assertTrue(properties.containsKey("max_time_seconds"))

        val required = schema["required"] as List<*>
        assertEquals(listOf("goal", "success_criteria"), required)

        val maxSteps = properties["max_steps"] as Map<*, *>
        val maxTime = properties["max_time_seconds"] as Map<*, *>
        assertEquals(10, maxSteps["default"])
        assertEquals(10, maxTime["minimum"])
        assertEquals(300, maxTime["maximum"])
        assertEquals(60, maxTime["default"])
    }

    @Test
    fun `subtask constants match sprint one defaults`() {
        assertEquals(3, SubtaskScaffold.MAX_DEPTH)
        assertEquals(10, SubtaskScaffold.DEFAULT_MAX_STEPS)
        assertEquals(60, SubtaskScaffold.DEFAULT_MAX_TIME_SECONDS)
        assertEquals(10, SubtaskScaffold.MIN_MAX_TIME_SECONDS)
        assertEquals(300, SubtaskScaffold.MAX_MAX_TIME_SECONDS)
    }

    @Test
    fun `subtask request accepts max time in allowed bounds`() {
        val min = SubtaskRequest(goal = "g", successCriteria = "s", maxTimeSeconds = 10)
        val max = SubtaskRequest(goal = "g", successCriteria = "s", maxTimeSeconds = 300)

        assertEquals(10, min.maxTimeSeconds)
        assertEquals(300, max.maxTimeSeconds)
    }

    @Test
    fun `subtask request rejects max time below lower bound`() {
        assertFailsWith<IllegalArgumentException> {
            SubtaskRequest(goal = "g", successCriteria = "s", maxTimeSeconds = 9)
        }
    }

    @Test
    fun `subtask request rejects max time above upper bound`() {
        assertFailsWith<IllegalArgumentException> {
            SubtaskRequest(goal = "g", successCriteria = "s", maxTimeSeconds = 301)
        }
    }
}
