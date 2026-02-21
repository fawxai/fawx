package ai.citros.chat

import org.junit.After
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test
import ai.citros.core.InterruptionEvent
import java.lang.reflect.Field

/**
 * Tests for InterruptionDetector's public lifecycle API.
 *
 * Note: onAccessibilityEvent() requires Android AccessibilityEvent which
 * is not available in plain JUnit. Those paths are covered by:
 * - InterruptionClassifier tests (pure logic in :core)
 * - Integration testing on-device
 *
 * These tests verify drain/monitoring lifecycle, state reset, and
 * agent action flagging via reflection into the singleton's atomics.
 */
class InterruptionDetectorTest {

    @Before
    fun setUp() {
        // Ensure clean state — detach resets everything
        InterruptionDetector.detach()
    }

    @After
    fun tearDown() {
        InterruptionDetector.detach()
    }

    @Test
    fun `drain returns null when not monitoring`() {
        assertNull(InterruptionDetector.drain())
    }

    @Test
    fun `drain returns null when no event queued`() {
        InterruptionDetector.startMonitoring("com.example.app")
        assertNull(InterruptionDetector.drain())
    }

    @Test
    fun `drain returns event and clears it`() {
        InterruptionDetector.startMonitoring("com.example.app")
        // Inject a pending event via reflection
        setPendingEvent(InterruptionEvent.AppSwitch("com.example.app", "com.other.app"))
        val event = InterruptionDetector.drain()
        assertNotNull(event)
        assertTrue(event is InterruptionEvent.AppSwitch)
        // Second drain should return null (cleared)
        assertNull(InterruptionDetector.drain())
    }

    @Test
    fun `startMonitoring resets state`() {
        InterruptionDetector.startMonitoring("com.example.app")
        setPendingEvent(InterruptionEvent.ExternalInterrupt("test"))
        // startMonitoring should clear pending event
        InterruptionDetector.startMonitoring("com.other.app")
        assertNull(InterruptionDetector.drain())
    }

    @Test
    fun `stopMonitoring clears pending event`() {
        InterruptionDetector.startMonitoring("com.example.app")
        setPendingEvent(InterruptionEvent.AppSwitch("a", "b"))
        InterruptionDetector.stopMonitoring()
        assertNull(InterruptionDetector.drain())
    }

    @Test
    fun `markAgentAction and clearAgentAction toggle flag`() {
        InterruptionDetector.startMonitoring()
        InterruptionDetector.markAgentAction()
        assertTrue(getAgentActionInProgress())
        InterruptionDetector.clearAgentAction()
        assertFalse(getAgentActionInProgress())
    }

    @Test
    fun `setExpectedPackage updates expected package`() {
        InterruptionDetector.startMonitoring("com.original")
        InterruptionDetector.setExpectedPackage("com.new.app")
        assertEquals("com.new.app", getExpectedPackage())
    }

    @Test
    fun `stopMonitoring clears expected package`() {
        InterruptionDetector.startMonitoring("com.example.app")
        InterruptionDetector.stopMonitoring()
        assertNull(getExpectedPackage())
    }

    @Test
    fun `stopMonitoring clears agent action flag`() {
        InterruptionDetector.startMonitoring()
        InterruptionDetector.markAgentAction()
        InterruptionDetector.stopMonitoring()
        assertFalse(getAgentActionInProgress())
    }

    // ===== Reflection helpers for testing singleton internal state =====

    private fun setPendingEvent(event: InterruptionEvent?) {
        val field = InterruptionDetector::class.java.getDeclaredField("pendingEvent")
        field.isAccessible = true
        @Suppress("UNCHECKED_CAST")
        val ref = field.get(InterruptionDetector) as java.util.concurrent.atomic.AtomicReference<InterruptionEvent?>
        ref.set(event)
    }

    private fun getAgentActionInProgress(): Boolean {
        val field = InterruptionDetector::class.java.getDeclaredField("agentActionInProgress")
        field.isAccessible = true
        val ref = field.get(InterruptionDetector) as java.util.concurrent.atomic.AtomicBoolean
        return ref.get()
    }

    private fun getExpectedPackage(): String? {
        val field = InterruptionDetector::class.java.getDeclaredField("expectedPackage")
        field.isAccessible = true
        return field.get(InterruptionDetector) as String?
    }
}
