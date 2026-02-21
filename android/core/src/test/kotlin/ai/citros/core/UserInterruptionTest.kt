package ai.citros.core

import kotlinx.coroutines.test.runTest
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertNull
import kotlin.test.assertTrue

class UserInterruptionTest {

    companion object {
        private const val DEFAULT_MAX_STEPS = 25
        private const val SAMPLE_HASH = 123
    }

    private fun defaultState(interruption: InterruptionEvent? = null) = LoopState(
        step = 1,
        maxSteps = DEFAULT_MAX_STEPS,
        lastToolName = "tap",
        lastScreenHash = SAMPLE_HASH,
        isCancelled = false,
        pendingInterruption = interruption
    )

    // ====== UserInterruptionCheck ======

    @Test
    fun `UserInterruptionCheck returns Continue when no interruption`() = runTest {
        val check = UserInterruptionCheck()
        val result = check.check(defaultState())
        assertEquals(CheckResult.Continue, result)
    }

    @Test
    fun `UserInterruptionCheck returns Steer on app switch with correct message`() = runTest {
        val check = UserInterruptionCheck()
        val state = defaultState(InterruptionEvent.AppSwitch("com.gmail", "com.calendar"))
        val result = check.check(state)
        assertIs<CheckResult.Steer>(result)
        assertEquals(1, result.userMessages.size)
        assertTrue(result.userMessages[0].contains("[SYSTEM:"))
        assertTrue(result.userMessages[0].contains("switched"))
    }

    @Test
    fun `UserInterruptionCheck returns Steer on user touch with correct message`() = runTest {
        val check = UserInterruptionCheck()
        val state = defaultState(InterruptionEvent.UserTouch(100, 200))
        val result = check.check(state)
        assertIs<CheckResult.Steer>(result)
        assertTrue(result.userMessages[0].contains("touched the screen"))
    }

    @Test
    fun `UserInterruptionCheck returns Steer on external interrupt with correct message`() = runTest {
        val check = UserInterruptionCheck()
        val state = defaultState(InterruptionEvent.ExternalInterrupt("Incoming phone call"))
        val result = check.check(state)
        assertIs<CheckResult.Steer>(result)
        assertTrue(result.userMessages[0].contains("Incoming phone call"))
    }

    @Test
    fun `UserInterruptionCheck message contains previous and new app names`() = runTest {
        val check = UserInterruptionCheck()
        val state = defaultState(InterruptionEvent.AppSwitch("com.gmail", "com.calendar"))
        val result = check.check(state)
        assertIs<CheckResult.Steer>(result)
        assertTrue(result.userMessages[0].contains("com.gmail"))
        assertTrue(result.userMessages[0].contains("com.calendar"))
    }

    @Test
    fun `UserInterruptionCheck touch message includes coordinates`() = runTest {
        val check = UserInterruptionCheck()
        val state = defaultState(InterruptionEvent.UserTouch(350, 750))
        val result = check.check(state)
        assertIs<CheckResult.Steer>(result)
        assertTrue(result.userMessages[0].contains("(350, 750)"))
        assertTrue(result.userMessages[0].contains("touched the screen"))
    }

    // ====== Edge case tests ======

    @Test
    fun `AppSwitch with empty string app names`() = runTest {
        val check = UserInterruptionCheck()
        val state = defaultState(InterruptionEvent.AppSwitch("", ""))
        val result = check.check(state)
        assertIs<CheckResult.Steer>(result)
        assertTrue(result.userMessages[0].contains("switched"))
    }

    @Test
    fun `ExternalInterrupt with empty description uses fallback`() = runTest {
        val check = UserInterruptionCheck()
        val state = defaultState(InterruptionEvent.ExternalInterrupt(""))
        val result = check.check(state)
        assertIs<CheckResult.Steer>(result)
        assertTrue(result.userMessages[0].contains("External interruption occurred"))
    }

    @Test
    fun `UserTouch with negative coordinates`() = runTest {
        val check = UserInterruptionCheck()
        val state = defaultState(InterruptionEvent.UserTouch(-1, -1))
        val result = check.check(state)
        assertIs<CheckResult.Steer>(result)
        assertTrue(result.userMessages[0].contains("(-1, -1)"))
    }

    @Test
    fun `UserTouch with zero coordinates`() = runTest {
        val check = UserInterruptionCheck()
        val state = defaultState(InterruptionEvent.UserTouch(0, 0))
        val result = check.check(state)
        assertIs<CheckResult.Steer>(result)
        assertTrue(result.userMessages[0].contains("(0, 0)"))
    }

    @Test
    fun `Rapid successive interruptions - source returns events on consecutive calls`() = runTest {
        val events = mutableListOf(
            InterruptionEvent.UserTouch(10, 20),
            InterruptionEvent.AppSwitch("a", "b")
        )
        val interruptionSource: () -> InterruptionEvent? = {
            if (events.isNotEmpty()) events.removeAt(0) else null
        }

        val check = UserInterruptionCheck()

        // First call with first event
        val state1 = defaultState(interruptionSource())
        val result1 = check.check(state1)
        assertIs<CheckResult.Steer>(result1)
        assertTrue(result1.userMessages[0].contains("touched the screen"))

        // Second call with second event
        val state2 = defaultState(interruptionSource())
        val result2 = check.check(state2)
        assertIs<CheckResult.Steer>(result2)
        assertTrue(result2.userMessages[0].contains("switched"))

        // Third call: no more events
        val state3 = defaultState(interruptionSource())
        val result3 = check.check(state3)
        assertEquals(CheckResult.Continue, result3)
    }

    // ====== LoopState backward compatibility ======

    @Test
    fun `LoopState pendingInterruption defaults to null for backward compatibility`() {
        val state = LoopState(
            step = 1,
            maxSteps = DEFAULT_MAX_STEPS,
            lastToolName = "tap",
            lastScreenHash = SAMPLE_HASH,
            isCancelled = false
        )
        assertNull(state.pendingInterruption)
    }

    @Test
    fun `LoopState no-arg pendingInterruption is null by default`() {
        // Verify the default parameter works when not explicitly passed
        val state = LoopState(
            step = 5,
            maxSteps = 10,
            lastToolName = "scroll",
            lastScreenHash = 999,
            isCancelled = false,
            pendingSteerMessages = listOf("hello"),
            lastToolWasUiMutating = true,
            preActionScreenHash = 888
            // pendingInterruption intentionally omitted
        )
        assertNull(state.pendingInterruption)
    }

    // ====== Integration: AgentExecutor with UserInterruptionCheck ======

    @Test
    fun `AgentExecutor pauses loop on app switch interruption`() = runTest {
        val delegate = FakeToolExecutionDelegate()
        val listener = FakeLoopProgressListener()
        var eventFired = false
        val interruptionSource: () -> InterruptionEvent? = {
            if (!eventFired) {
                eventFired = true
                InterruptionEvent.AppSwitch("com.gmail", "com.calendar")
            } else null
        }

        val executor = AgentExecutor(
            delegate = delegate,
            progressListener = listener,
            boundaryChecks = listOf(
                CancellationCheck(),
                StepLimitCheck(),
                StuckDetectionCheck.withDefaults(),
                ActionVerificationCheck(),
                UserInterruptionCheck(),
                SteerCheck()
            ),
            interruptionSource = interruptionSource
        )
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "tap", mapOf("element_id" to 1))),
            stopReason = "tool_use"
        )

        val result = executor.run(response, null, { false }) {
            ChatResponse(text = "OK, pausing", toolCalls = emptyList(), stopReason = "end_turn")
        }

        assertIs<LoopResult.Completed>(result)
        assertEquals("end_turn", result.exitReason)
        // Steer message should have been delivered
        assertTrue(delegate.steerMessages.any { it.contains("switched from com.gmail to com.calendar") })
    }
}
