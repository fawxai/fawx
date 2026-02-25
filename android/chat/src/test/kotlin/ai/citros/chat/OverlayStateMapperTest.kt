package ai.citros.chat

import ai.citros.core.*
import org.junit.Assert.*
import org.junit.Test

/**
 * Tests for OverlayStateMapper - converting ChatViewModel messages to OverlayState.
 */
class OverlayStateMapperTest {

    @Test
    fun `mapToOverlayState with empty messages returns IDLE state`() {
        val state = OverlayStateMapper.mapToOverlayState(
            messages = emptyList(),
            isLoading = false
        )

        assertEquals(OverlayRunState.IDLE, state.runState)
        assertEquals(0, state.totalSteps)
        assertEquals(0, state.currentStepIndex)
        assertTrue(state.steps.isEmpty())
        assertTrue(state.lines.isEmpty())
    }

    @Test
    fun `mapToOverlayState carries runtime action pills`() {
        val pills = listOf(
            ActionPill(
                id = "p1",
                label = "Yes",
                style = PillStyle.PRIMARY,
                action = PillAction.Approve("req-1")
            )
        )
        val state = OverlayStateMapper.mapToOverlayState(
            messages = listOf(Message(role = "user", content = "Confirm?")),
            isLoading = true,
            actionPills = pills
        )

        assertEquals(1, state.actionPills.size)
        assertEquals("Yes", state.actionPills.first().label)
    }

    @Test
    fun `mapToOverlayState with single user message returns USER line and IDLE state`() {
        val messages = listOf(
            Message(role = "user", content = "Turn on Wi-Fi")
        )

        val state = OverlayStateMapper.mapToOverlayState(
            messages = messages,
            isLoading = false
        )

        assertEquals(1, state.lines.size)
        assertEquals(OverlayLineType.USER, state.lines[0].type)
        assertEquals("Turn on Wi-Fi", state.lines[0].text)
        assertEquals(OverlayRunState.IDLE, state.runState)
    }

    @Test
    fun `mapToOverlayState with tool result messages creates SYSTEM lines`() {
        val messages = listOf(
            Message(role = "user", content = "Open settings"),
            Message(role = "assistant", content = "🤖 Opening Settings app..."),
            Message(role = "assistant", content = "🤖 Tapped Wi-Fi toggle")
        )

        val state = OverlayStateMapper.mapToOverlayState(
            messages = messages,
            isLoading = false
        )

        assertEquals(3, state.lines.size)
        assertEquals(OverlayLineType.USER, state.lines[0].type)
        assertEquals("Open settings", state.lines[0].text)
        assertEquals(OverlayLineType.SYSTEM, state.lines[1].type)
        assertEquals("Opening Settings app...", state.lines[1].text)
        assertEquals(OverlayLineType.SYSTEM, state.lines[2].type)
        assertEquals("Tapped Wi-Fi toggle", state.lines[2].text)
    }

    @Test
    fun `mapToOverlayState strips robot emoji prefix from tool results`() {
        val messages = listOf(
            Message(role = "user", content = "Test"),
            Message(role = "assistant", content = "🤖 Action executed successfully")
        )

        val state = OverlayStateMapper.mapToOverlayState(
            messages = messages,
            isLoading = false
        )

        assertEquals(2, state.lines.size)
        assertEquals("Action executed successfully", state.lines[1].text)
        assertFalse(state.lines[1].text.startsWith("🤖"))
    }

    @Test
    fun `mapToOverlayState with isLoading true sets EXECUTING state`() {
        val messages = listOf(
            Message(role = "user", content = "Turn on Wi-Fi"),
            Message(role = "assistant", content = "🤖 Opening Settings...")
        )

        val state = OverlayStateMapper.mapToOverlayState(
            messages = messages,
            isLoading = true
        )

        assertEquals(OverlayRunState.EXECUTING, state.runState)
    }

    @Test
    fun `mapToOverlayState with completed task sets COMPLETED state`() {
        val messages = listOf(
            Message(role = "user", content = "Turn on Wi-Fi"),
            Message(role = "assistant", content = "🤖 Opened Settings"),
            Message(role = "assistant", content = "🤖 Tapped Wi-Fi"),
            Message(role = "assistant", content = "Wi-Fi is now on")
        )

        val state = OverlayStateMapper.mapToOverlayState(
            messages = messages,
            isLoading = false
        )

        assertEquals(OverlayRunState.COMPLETED, state.runState)
    }

    @Test
    fun `mapToOverlayState with error message sets FAILED state`() {
        val messages = listOf(
            Message(role = "user", content = "Turn on Wi-Fi"),
            Message(role = "assistant", content = "💥 Crashed: Network error")
        )

        val state = OverlayStateMapper.mapToOverlayState(
            messages = messages,
            isLoading = false
        )

        assertEquals(OverlayRunState.FAILED, state.runState)
    }

    @Test
    fun `mapToOverlayState creates steps from tool results`() {
        val messages = listOf(
            Message(role = "user", content = "Turn on Wi-Fi"),
            Message(role = "assistant", content = "🤖 Opening Settings"),
            Message(role = "assistant", content = "🤖 Scrolling to Wi-Fi"),
            Message(role = "assistant", content = "🤖 Tapping toggle")
        )

        val state = OverlayStateMapper.mapToOverlayState(
            messages = messages,
            isLoading = false
        )

        assertEquals(3, state.totalSteps)
        assertEquals(3, state.steps.size)
        assertEquals("Opening Settings", state.steps[0].label)
        assertEquals("Scrolling to Wi-Fi", state.steps[1].label)
        assertEquals("Tapping toggle", state.steps[2].label)
        
        // Check step numbers
        assertEquals(1, state.steps[0].step)
        assertEquals(3, state.steps[0].total)
        assertEquals(2, state.steps[1].step)
        assertEquals(3, state.steps[1].total)
    }

    @Test
    fun `mapToOverlayState with isLoading sets currentStepIndex to last step`() {
        val messages = listOf(
            Message(role = "user", content = "Turn on Wi-Fi"),
            Message(role = "assistant", content = "🤖 Opening Settings"),
            Message(role = "assistant", content = "🤖 Scrolling to Wi-Fi")
        )

        val state = OverlayStateMapper.mapToOverlayState(
            messages = messages,
            isLoading = true
        )

        assertEquals(2, state.totalSteps)
        assertEquals(1, state.currentStepIndex) // 0-indexed, so last step is index 1
    }

    @Test
    fun `mapToOverlayState with completed task sets currentStepIndex to totalSteps`() {
        val messages = listOf(
            Message(role = "user", content = "Turn on Wi-Fi"),
            Message(role = "assistant", content = "🤖 Opening Settings"),
            Message(role = "assistant", content = "🤖 Tapped toggle"),
            Message(role = "assistant", content = "Done!")
        )

        val state = OverlayStateMapper.mapToOverlayState(
            messages = messages,
            isLoading = false
        )

        assertEquals(2, state.totalSteps)
        assertEquals(2, state.currentStepIndex) // All steps completed
    }

    @Test
    fun `mapToOverlayState only includes messages from last user turn`() {
        val messages = listOf(
            Message(role = "user", content = "Open Gmail"),
            Message(role = "assistant", content = "🤖 Opened Gmail"),
            Message(role = "assistant", content = "Gmail is open"),
            Message(role = "user", content = "Turn on Wi-Fi"),
            Message(role = "assistant", content = "🤖 Opening Settings"),
            Message(role = "assistant", content = "🤖 Tapped toggle")
        )

        val state = OverlayStateMapper.mapToOverlayState(
            messages = messages,
            isLoading = false
        )

        // Should only include the second conversation turn
        assertEquals(3, state.lines.size)
        assertEquals("Turn on Wi-Fi", state.lines[0].text)
        assertEquals("Opening Settings", state.lines[1].text)
        assertEquals("Tapped toggle", state.lines[2].text)
        
        assertEquals(2, state.totalSteps)
    }

    @Test
    fun `mapToOverlayState handles mixed assistant messages correctly`() {
        val messages = listOf(
            Message(role = "user", content = "Turn on Wi-Fi"),
            Message(role = "assistant", content = "🤖 Opening Settings"),
            Message(role = "assistant", content = "Let me find the toggle"),
            Message(role = "assistant", content = "🤖 Tapped toggle"),
            Message(role = "assistant", content = "All done!")
        )

        val state = OverlayStateMapper.mapToOverlayState(
            messages = messages,
            isLoading = false
        )

        // Tool results should be SYSTEM, regular responses should not be included
        // or should be treated differently
        assertEquals(5, state.lines.size)
        assertEquals(OverlayLineType.USER, state.lines[0].type)
        assertEquals(OverlayLineType.SYSTEM, state.lines[1].type)
        // "Let me find the toggle" is not a tool result, so it should be SYSTEM or skipped
        // For now, let's assume non-tool-result assistant messages are also added
        assertEquals(2, state.totalSteps) // Only tool results count as steps
    }

    @Test
    fun `mapToOverlayState with no user message returns IDLE`() {
        val messages = listOf(
            Message(role = "assistant", content = "Hello!")
        )

        val state = OverlayStateMapper.mapToOverlayState(
            messages = messages,
            isLoading = false
        )

        assertEquals(OverlayRunState.IDLE, state.runState)
        assertTrue(state.lines.isEmpty())
        assertEquals(0, state.totalSteps)
    }

    @Test
    fun `mapToOverlayState with tool result as last message and not loading returns STOPPED`() {
        val messages = listOf(
            Message(role = "user", content = "Test"),
            Message(role = "assistant", content = "🤖 Tool executed")
        )
        
        val state = OverlayStateMapper.mapToOverlayState(
            messages = messages,
            isLoading = false
        )
        
        assertEquals(OverlayRunState.STOPPED, state.runState)
    }

    @Test
    fun `mapToOverlayState with Error prefix message sets FAILED state`() {
        val messages = listOf(
            Message(role = "user", content = "Test"),
            Message(role = "assistant", content = "Error: Something went wrong")
        )
        
        val state = OverlayStateMapper.mapToOverlayState(
            messages = messages,
            isLoading = false
        )
        
        assertEquals(OverlayRunState.FAILED, state.runState)
    }

    @Test
    fun `mapToOverlayState after clear conversation returns IDLE not STOPPED`() {
        // Simulates the state after clearConversation() - empty messages, not loading
        val state = OverlayStateMapper.mapToOverlayState(
            messages = emptyList(),
            isLoading = false
        )

        // Should be IDLE (neutral) not STOPPED (red error text) - fixes #438
        assertEquals(OverlayRunState.IDLE, state.runState)
        assertEquals(OverlayState.EMPTY, state)
    }

    @Test
    fun `mapToOverlayState with user message but no tool results has zero steps and IDLE state`() {
        val messages = listOf(
            Message(role = "user", content = "Test"),
            Message(role = "assistant", content = "I'll help with that")
        )
        
        val state = OverlayStateMapper.mapToOverlayState(
            messages = messages,
            isLoading = false
        )
        
        assertEquals(0, state.totalSteps)
        assertTrue(state.steps.isEmpty())
        assertEquals(OverlayRunState.IDLE, state.runState)
    }
}
