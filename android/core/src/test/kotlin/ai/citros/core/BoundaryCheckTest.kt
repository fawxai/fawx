package ai.citros.core

import kotlinx.coroutines.test.runTest
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertTrue
import kotlin.test.assertFalse

class BoundaryCheckTest {

    companion object {
        private const val DEFAULT_MAX_STEPS = 25
        private const val SAMPLE_HASH = 123
    }

    // ====== CancellationCheck ======

    @Test
    fun `CancellationCheck returns Continue when not cancelled`() = runTest {
        val check = CancellationCheck()
        val state = LoopState(
            step = 1,
            maxSteps = DEFAULT_MAX_STEPS,
            lastToolName = "tap",
            lastScreenHash = SAMPLE_HASH,
            isCancelled = false
        )
        assertEquals(CheckResult.Continue, check.check(state))
    }

    @Test
    fun `CancellationCheck returns Stop when cancelled`() = runTest {
        val check = CancellationCheck()
        val state = LoopState(
            step = 1,
            maxSteps = DEFAULT_MAX_STEPS,
            lastToolName = "tap",
            lastScreenHash = SAMPLE_HASH,
            isCancelled = true
        )
        val result = check.check(state)
        assertIs<CheckResult.Stop>(result)
        assertEquals("cancelled", result.reason)
    }

    // ====== StepLimitCheck ======

    @Test
    fun `StepLimitCheck returns Continue when under limit`() = runTest {
        val check = StepLimitCheck()
        val state = LoopState(
            step = 5,
            maxSteps = DEFAULT_MAX_STEPS,
            lastToolName = "tap",
            lastScreenHash = SAMPLE_HASH,
            isCancelled = false
        )
        assertEquals(CheckResult.Continue, check.check(state))
    }

    @Test
    fun `StepLimitCheck returns Continue one step before limit`() = runTest {
        val check = StepLimitCheck()
        val state = LoopState(
            step = DEFAULT_MAX_STEPS - 1,
            maxSteps = DEFAULT_MAX_STEPS,
            lastToolName = "tap",
            lastScreenHash = SAMPLE_HASH,
            isCancelled = false
        )
        assertEquals(CheckResult.Continue, check.check(state))
    }

    @Test
    fun `StepLimitCheck returns Stop at exact limit`() = runTest {
        val check = StepLimitCheck()
        val state = LoopState(
            step = DEFAULT_MAX_STEPS,
            maxSteps = DEFAULT_MAX_STEPS,
            lastToolName = "tap",
            lastScreenHash = SAMPLE_HASH,
            isCancelled = false
        )
        val result = check.check(state)
        assertIs<CheckResult.Stop>(result)
        assertEquals("max_steps", result.reason)
    }

    @Test
    fun `StepLimitCheck returns Stop above limit`() = runTest {
        val check = StepLimitCheck()
        val state = LoopState(
            step = DEFAULT_MAX_STEPS + 5,
            maxSteps = DEFAULT_MAX_STEPS,
            lastToolName = "tap",
            lastScreenHash = SAMPLE_HASH,
            isCancelled = false
        )
        val result = check.check(state)
        assertIs<CheckResult.Stop>(result)
        assertEquals("max_steps", result.reason)
    }

    // ====== StuckDetectionCheck ======

    @Test
    fun `StuckDetectionCheck returns Continue when screen changes`() = runTest {
        val check = StuckDetectionCheck.withDefaults()

        assertEquals(
            CheckResult.Continue,
            check.check(LoopState(1, DEFAULT_MAX_STEPS, "tap", 100, false))
        )
        assertEquals(
            CheckResult.Continue,
            check.check(LoopState(2, DEFAULT_MAX_STEPS, "tap", 200, false))
        )
        assertEquals(
            CheckResult.Continue,
            check.check(LoopState(3, DEFAULT_MAX_STEPS, "tap", 300, false))
        )
    }

    @Test
    fun `StuckDetectionCheck injects warning after threshold identical screens`() = runTest {
        val check = StuckDetectionCheck(StuckDetector(screenThreshold = 3))
        val hash = 42

        // First two: Continue
        assertEquals(
            CheckResult.Continue,
            check.check(LoopState(1, DEFAULT_MAX_STEPS, "tap", hash, false))
        )
        assertEquals(
            CheckResult.Continue,
            check.check(LoopState(2, DEFAULT_MAX_STEPS, "tap", hash, false))
        )

        // Third identical: Inject
        val result = check.check(LoopState(3, DEFAULT_MAX_STEPS, "tap", hash, false))
        assertIs<CheckResult.Inject>(result)
        assertTrue(result.message.contains("STUCK"))
    }

    @Test
    fun `StuckDetectionCheck injects warning for consecutive waits on stuck screen`() = runTest {
        val check = StuckDetectionCheck(
            StuckDetector(screenThreshold = 5, waitThreshold = 2)
        )
        val hash = 42

        // Build up identical screen hashes (need 2+ for wait detection)
        check.check(LoopState(1, DEFAULT_MAX_STEPS, "tap", hash, false))
        check.check(LoopState(2, DEFAULT_MAX_STEPS, "wait", hash, false))

        // Second consecutive wait with stuck screen
        val result = check.check(LoopState(3, DEFAULT_MAX_STEPS, "wait", hash, false))
        assertIs<CheckResult.Inject>(result)
        assertTrue(result.message.contains("Waiting more won't help"))
    }

    @Test
    fun `StuckDetectionCheck returns Continue with null screen hash`() = runTest {
        val check = StuckDetectionCheck.withDefaults()

        // Null hashes should never trigger stuck detection
        repeat(5) { i ->
            assertEquals(
                CheckResult.Continue,
                check.check(LoopState(i + 1, DEFAULT_MAX_STEPS, "tap", null, false))
            )
        }
    }

    @Test
    fun `StuckDetectionCheck resets wait counter on non-wait tool`() = runTest {
        val check = StuckDetectionCheck(
            StuckDetector(screenThreshold = 10, waitThreshold = 2)
        )
        val hash = 42

        // One wait
        check.check(LoopState(1, DEFAULT_MAX_STEPS, "wait", hash, false))
        // Non-wait resets the consecutive wait counter
        check.check(LoopState(2, DEFAULT_MAX_STEPS, "tap", hash, false))
        // First wait again (counter back to 1, threshold is 2)
        val result = check.check(LoopState(3, DEFAULT_MAX_STEPS, "wait", hash, false))

        // Should NOT fire — only 1 consecutive wait, screen threshold 10 not reached
        assertEquals(CheckResult.Continue, result)
    }

    @Test
    fun `StuckDetectionCheck screen stuck warning takes precedence over wait warning`() = runTest {
        val check = StuckDetectionCheck(
            StuckDetector(screenThreshold = 3, waitThreshold = 2)
        )
        val hash = 42

        // Build up to both thresholds simultaneously
        check.check(LoopState(1, DEFAULT_MAX_STEPS, "wait", hash, false))
        check.check(LoopState(2, DEFAULT_MAX_STEPS, "wait", hash, false))

        // Third call: screen threshold (3) AND wait threshold (2+) both met
        val result = check.check(LoopState(3, DEFAULT_MAX_STEPS, "wait", hash, false))
        assertIs<CheckResult.Inject>(result)
        // Screen stuck warning should appear (takes precedence)
        assertTrue(result.message.contains("STUCK"))
    }

    // ====== CheckResult equality ======

    @Test
    fun `CheckResult Continue is singleton`() {
        assertEquals(CheckResult.Continue, CheckResult.Continue)
    }

    @Test
    fun `CheckResult Stop equality by reason`() {
        assertEquals(
            CheckResult.Stop("cancelled"),
            CheckResult.Stop("cancelled")
        )
    }

    @Test
    fun `CheckResult Inject equality by message`() {
        assertEquals(
            CheckResult.Inject("warning"),
            CheckResult.Inject("warning")
        )
    }

    // ====== SteerCheck ======

    @Test
    fun `SteerCheck returns Continue when no pending messages`() = runTest {
        val check = SteerCheck()
        val state = LoopState(
            step = 1,
            maxSteps = DEFAULT_MAX_STEPS,
            lastToolName = "tap",
            lastScreenHash = SAMPLE_HASH,
            isCancelled = false,
            pendingSteerMessages = emptyList()
        )
        assertEquals(CheckResult.Continue, check.check(state))
    }

    @Test
    fun `SteerCheck returns Steer with pending messages`() = runTest {
        val check = SteerCheck()
        val state = LoopState(
            step = 1,
            maxSteps = DEFAULT_MAX_STEPS,
            lastToolName = "tap",
            lastScreenHash = SAMPLE_HASH,
            isCancelled = false,
            pendingSteerMessages = listOf("no, open Calendar instead")
        )
        val result = check.check(state)
        assertIs<CheckResult.Steer>(result)
        assertEquals(listOf("no, open Calendar instead"), result.userMessages)
    }

    @Test
    fun `SteerCheck returns all pending messages`() = runTest {
        val check = SteerCheck()
        val state = LoopState(
            step = 1,
            maxSteps = DEFAULT_MAX_STEPS,
            lastToolName = "tap",
            lastScreenHash = SAMPLE_HASH,
            isCancelled = false,
            pendingSteerMessages = listOf("wrong app", "use Calendar")
        )
        val result = check.check(state)
        assertIs<CheckResult.Steer>(result)
        assertEquals(listOf("wrong app", "use Calendar"), result.userMessages)
    }

    // ====== LoopState default pendingSteerMessages ======

    @Test
    fun `LoopState defaults pendingSteerMessages to empty list`() {
        val state = LoopState(
            step = 1,
            maxSteps = DEFAULT_MAX_STEPS,
            lastToolName = "tap",
            lastScreenHash = SAMPLE_HASH,
            isCancelled = false
        )
        assertEquals(emptyList(), state.pendingSteerMessages)
    }

    // ====== CheckResult.Steer equality ======

    @Test
    fun `CheckResult Steer equality by messages`() {
        assertEquals(
            CheckResult.Steer(listOf("hello")),
            CheckResult.Steer(listOf("hello"))
        )
    }

    // ====== AccessibilityGateCheck ======

    @Test
    fun `AccessibilityGateCheck returns Continue when accessibility is available`() = runTest {
        var waitCalled = false
        val check = AccessibilityGateCheck(
            isAvailable = { true },
            waitForReconnect = { waitCalled = true; false },
            onReconnected = {},
            onLost = {}
        )
        val state = LoopState(1, DEFAULT_MAX_STEPS, "tap", SAMPLE_HASH, false)

        val result = check.check(state)

        assertEquals(CheckResult.Continue, result)
        assertFalse(waitCalled, "Should not wait when already available")
    }

    @Test
    fun `AccessibilityGateCheck does not wait when already available`() = runTest {
        var waitCalled = false
        val check = AccessibilityGateCheck(
            isAvailable = { true },
            waitForReconnect = { waitCalled = true; true },
            onReconnected = {},
            onLost = {}
        )
        val state = LoopState(1, DEFAULT_MAX_STEPS, "tap", SAMPLE_HASH, false)

        check.check(state)

        assertFalse(waitCalled, "waitForReconnect should not be called when service is available")
    }

    @Test
    fun `AccessibilityGateCheck waits and returns Continue when accessibility reconnects`() = runTest {
        var waitTimeoutReceived: Long? = null
        val check = AccessibilityGateCheck(
            isAvailable = { false },
            waitForReconnect = { timeout ->
                waitTimeoutReceived = timeout
                true  // reconnection succeeds
            },
            onReconnected = {},
            onLost = {},
            baseTimeoutMs = 3000L,
            maxRetries = 1
        )
        val state = LoopState(1, DEFAULT_MAX_STEPS, "tap", SAMPLE_HASH, false)

        val result = check.check(state)

        assertEquals(CheckResult.Continue, result)
        assertEquals(3000L, waitTimeoutReceived, "Should pass configured timeout to waitForReconnect")
    }

    @Test
    fun `AccessibilityGateCheck returns Stop when accessibility does not reconnect`() = runTest {
        val check = AccessibilityGateCheck(
            isAvailable = { false },
            waitForReconnect = { false },  // reconnection fails on all attempts
            onReconnected = {},
            onLost = {},
            maxRetries = 1
        )
        val state = LoopState(1, DEFAULT_MAX_STEPS, "tap", SAMPLE_HASH, false)

        val result = check.check(state)

        assertIs<CheckResult.Stop>(result)
        assertEquals("accessibility_lost", result.reason)
    }

    @Test
    fun `AccessibilityGateCheck calls onReconnected after successful reconnection`() = runTest {
        var reconnectedCalled = false
        val check = AccessibilityGateCheck(
            isAvailable = { false },
            waitForReconnect = { true },
            onReconnected = { reconnectedCalled = true },
            onLost = {},
            maxRetries = 1
        )
        val state = LoopState(1, DEFAULT_MAX_STEPS, "tap", SAMPLE_HASH, false)

        check.check(state)

        assertTrue(reconnectedCalled, "onReconnected should be called after successful reconnection")
    }

    @Test
    fun `AccessibilityGateCheck calls onLost when reconnection fails`() = runTest {
        var lostCalled = false
        val check = AccessibilityGateCheck(
            isAvailable = { false },
            waitForReconnect = { false },
            onReconnected = {},
            onLost = { lostCalled = true },
            maxRetries = 1
        )
        val state = LoopState(1, DEFAULT_MAX_STEPS, "tap", SAMPLE_HASH, false)

        check.check(state)

        assertTrue(lostCalled, "onLost should be called when reconnection fails")
    }

    @Test
    fun `AccessibilityGateCheck does not call onLost on successful reconnection`() = runTest {
        var lostCalled = false
        val check = AccessibilityGateCheck(
            isAvailable = { false },
            waitForReconnect = { true },
            onReconnected = {},
            onLost = { lostCalled = true },
            maxRetries = 1
        )
        val state = LoopState(1, DEFAULT_MAX_STEPS, "tap", SAMPLE_HASH, false)

        check.check(state)

        assertFalse(lostCalled, "onLost should not be called on successful reconnection")
    }

    @Test
    fun `AccessibilityGateCheck does not call onReconnected on failed reconnection`() = runTest {
        var reconnectedCalled = false
        val check = AccessibilityGateCheck(
            isAvailable = { false },
            waitForReconnect = { false },
            onReconnected = { reconnectedCalled = true },
            onLost = {},
            maxRetries = 1
        )
        val state = LoopState(1, DEFAULT_MAX_STEPS, "tap", SAMPLE_HASH, false)

        check.check(state)

        assertFalse(reconnectedCalled, "onReconnected should not be called on failed reconnection")
    }

    @Test
    fun `AccessibilityGateCheck uses default base timeout of 2000ms`() = runTest {
        var receivedTimeout: Long? = null
        val check = AccessibilityGateCheck(
            isAvailable = { false },
            waitForReconnect = { timeout ->
                receivedTimeout = timeout
                true  // succeed on first attempt
            },
            onReconnected = {},
            onLost = {}
            // No baseTimeoutMs — should use DEFAULT_BASE_TIMEOUT_MS (2000L)
        )
        val state = LoopState(1, DEFAULT_MAX_STEPS, "tap", SAMPLE_HASH, false)

        check.check(state)

        assertEquals(2000L, receivedTimeout, "Default base timeout should be 2000ms")
    }


    @Test
    fun `AccessibilityGateCheck retries with exponential backoff`() = runTest {
        val receivedTimeouts = mutableListOf<Long>()
        val check = AccessibilityGateCheck(
            isAvailable = { false },
            waitForReconnect = { timeout ->
                receivedTimeouts.add(timeout)
                false  // all attempts fail
            },
            onReconnected = {},
            onLost = {},
            baseTimeoutMs = 1000L,
            maxRetries = 3
        )
        val state = LoopState(1, DEFAULT_MAX_STEPS, "tap", SAMPLE_HASH, false)

        val result = check.check(state)

        assertIs<CheckResult.Stop>(result)
        assertEquals(3, receivedTimeouts.size, "Should have attempted 3 retries")
        assertEquals(1000L, receivedTimeouts[0], "First attempt: 1000ms")
        assertEquals(2000L, receivedTimeouts[1], "Second attempt: 2000ms")
        assertEquals(4000L, receivedTimeouts[2], "Third attempt: 4000ms")
    }

    @Test
    fun `AccessibilityGateCheck succeeds on second retry attempt`() = runTest {
        var attemptCount = 0
        val check = AccessibilityGateCheck(
            isAvailable = { false },
            waitForReconnect = {
                attemptCount++
                attemptCount >= 2  // fail first, succeed second
            },
            onReconnected = {},
            onLost = {},
            baseTimeoutMs = 1000L,
            maxRetries = 3
        )
        val state = LoopState(1, DEFAULT_MAX_STEPS, "tap", SAMPLE_HASH, false)

        val result = check.check(state)

        assertEquals(CheckResult.Continue, result)
        assertEquals(2, attemptCount, "Should have succeeded on second attempt")
    }
}
