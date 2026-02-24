package ai.citros.core

import org.junit.Assert.*
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * Unit tests for [AgentState] sealed class.
 *
 * Covers:
 * - State construction and data access
 * - statusLabel() for each state variant
 * - isActive() classification (active vs terminal)
 *
 * See docs/specs/sprint-0-service-architecture.md §2, test matrix S4
 */
class AgentStateTest {

    // --- statusLabel ---

    @Test
    fun `idle statusLabel returns Ready`() {
        assertEquals("Ready", AgentState.Idle.statusLabel())
    }

    @Test
    fun `thinking statusLabel returns Thinking`() {
        assertEquals("Thinking...", AgentState.Thinking("task-1").statusLabel())
    }

    @Test
    fun `executing statusLabel returns tool name`() {
        val state = AgentState.Executing("task-1", "open_app", 3, 10)
        assertEquals("open_app", state.statusLabel())
    }

    @Test
    fun `waitingForInput statusLabel returns reason`() {
        val state = AgentState.WaitingForInput("task-1", "req-1", "Confirm send?", InputType.POLICY_CONFIRMATION)
        assertEquals("Confirm send?", state.statusLabel())
    }

    @Test
    fun `complete statusLabel returns Done`() {
        assertEquals("Done", AgentState.Complete("task-1", "Message sent").statusLabel())
    }

    @Test
    fun `failed statusLabel returns Error`() {
        assertEquals("Error", AgentState.Failed("task-1", "Network error").statusLabel())
    }

    @Test
    fun `resuming statusLabel returns Resuming`() {
        assertEquals("Resuming...", AgentState.Resuming("task-1").statusLabel())
    }

    // --- isActive ---

    @Test
    fun `idle is not active`() {
        assertFalse(AgentState.Idle.isActive())
    }

    @Test
    fun `thinking is active`() {
        assertTrue(AgentState.Thinking("task-1").isActive())
    }

    @Test
    fun `executing is active`() {
        assertTrue(AgentState.Executing("task-1", "tap", 1, 5).isActive())
    }

    @Test
    fun `waitingForInput is active`() {
        assertTrue(AgentState.WaitingForInput("task-1", "req-1", "Confirm?", InputType.DISAMBIGUATION).isActive())
    }

    @Test
    fun `complete is not active`() {
        assertFalse(AgentState.Complete("task-1", "Done").isActive())
    }

    @Test
    fun `failed is not active`() {
        assertFalse(AgentState.Failed("task-1", "Error").isActive())
    }

    @Test
    fun `resuming is active`() {
        assertTrue(AgentState.Resuming("task-1").isActive())
    }

    // NH2: Transition tests removed — they were duplicates of isActive() checks.
    // Real transition tests live in AgentServiceTest where the service performs
    // actual state transitions via intent handling.

    // --- Nullable totalSteps (NB2) ---

    @Test
    fun `executing with null totalSteps`() {
        val state = AgentState.Executing("task-1", "tap", 3)
        assertNull(state.totalSteps)
        assertEquals("tap", state.statusLabel())
        assertTrue(state.isActive())
    }

    @Test
    fun `executing with explicit totalSteps`() {
        val state = AgentState.Executing("task-1", "tap", 3, 10)
        assertEquals(10, state.totalSteps)
    }

    // --- Data class equality ---

    @Test
    fun `executing states with same data are equal`() {
        val a = AgentState.Executing("task-1", "tap", 1, 5)
        val b = AgentState.Executing("task-1", "tap", 1, 5)
        assertEquals(a, b)
    }

    @Test
    fun `executing states with different step are not equal`() {
        val a = AgentState.Executing("task-1", "tap", 1, 5)
        val b = AgentState.Executing("task-1", "tap", 2, 5)
        assertNotEquals(a, b)
    }

    // --- InputType coverage ---

    @Test
    fun `all InputType values exist`() {
        val types = InputType.values()
        assertEquals(4, types.size)
        assertTrue(types.contains(InputType.POLICY_CONFIRMATION))
        assertTrue(types.contains(InputType.DISAMBIGUATION))
        assertTrue(types.contains(InputType.AUTHENTICATION))
        assertTrue(types.contains(InputType.FREE_TEXT))
    }
}
