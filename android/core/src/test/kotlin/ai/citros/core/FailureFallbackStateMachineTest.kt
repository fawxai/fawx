package ai.citros.core

import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class FailureFallbackStateMachineTest {
    @Test
    fun `same class follows deterministic fallback order`() {
        val sm = FailureFallbackStateMachine(mapOf(FailureClass.LOW_SIGNAL_DYNAMIC to 2))
        assertEquals(FallbackAction.ALTERNATE_SOURCE, sm.transition(FailureClass.LOW_SIGNAL_DYNAMIC).action)
        assertEquals(FallbackAction.NARROWED_QUERY, sm.transition(FailureClass.LOW_SIGNAL_DYNAMIC).action)
        assertEquals(FallbackAction.SUMMARIZE_UNCERTAINTY, sm.transition(FailureClass.LOW_SIGNAL_DYNAMIC).action)
        assertEquals(FallbackAction.EXPLICIT_BLOCKER, sm.transition(FailureClass.LOW_SIGNAL_DYNAMIC).action)
    }

    @Test
    fun `class transition resets attempt counter`() {
        val sm = FailureFallbackStateMachine(mapOf(FailureClass.UNTRUSTED to 1, FailureClass.PARTIAL to 1))
        sm.transition(FailureClass.UNTRUSTED)
        val next = sm.transition(FailureClass.PARTIAL)
        assertEquals(0, next.attempt)
        assertEquals(FallbackAction.ALTERNATE_SOURCE, next.action)
    }

    @Test
    fun `blocked class goes straight to explicit blocker`() {
        val sm = FailureFallbackStateMachine(mapOf(FailureClass.BLOCKED to 0))
        assertEquals(FallbackAction.EXPLICIT_BLOCKER, sm.transition(FailureClass.BLOCKED).action)
        assertEquals(FallbackAction.EXPLICIT_BLOCKER, sm.transition(FailureClass.BLOCKED).action)
    }

    @Test
    fun `directive contains observable retry metadata`() {
        val sm = FailureFallbackStateMachine(mapOf(FailureClass.PARTIAL to 1))
        val directive = sm.transition(FailureClass.PARTIAL).toLoopDirective()
        assertTrue(directive.contains("SYSTEM_FALLBACK"))
        assertTrue(directive.contains("class=PARTIAL"))
        assertTrue(directive.contains("attempt=0/1"))
    }

    @Test
    fun `transition logs through injectable logger`() {
        val logs = mutableListOf<String>()
        val sm = FailureFallbackStateMachine(
            retryBudgetByClass = mapOf(FailureClass.LOW_SIGNAL_DYNAMIC to 2),
            logger = FallbackStateLogger { tag, message -> logs.add("$tag $message") }
        )

        sm.transition(FailureClass.LOW_SIGNAL_DYNAMIC)

        assertEquals(1, logs.size)
        assertTrue(logs.single().contains("CitrosFallbackSM"))
        assertTrue(logs.single().contains("to=LOW_SIGNAL_DYNAMIC"))
        assertTrue(logs.single().contains("action=ALTERNATE_SOURCE"))
    }
}
