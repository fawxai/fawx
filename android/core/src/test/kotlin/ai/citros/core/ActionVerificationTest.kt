package ai.citros.core

import kotlinx.coroutines.test.runTest
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertTrue

class ActionVerificationTest {

    // ====== ActionVerificationCheck unit tests ======

    @Test
    fun `UI-mutating tool with screen change returns Continue`() = runTest {
        val check = ActionVerificationCheck()
        val state = LoopState(
            step = 1, maxSteps = 25, lastToolName = "tap",
            lastScreenHash = 200, isCancelled = false,
            lastToolWasUiMutating = true,
            preActionScreenHash = 100
        )
        assertEquals(CheckResult.Continue, check.check(state))
    }

    @Test
    fun `UI-mutating tool with NO screen change returns Inject warning`() = runTest {
        val check = ActionVerificationCheck()
        val state = LoopState(
            step = 1, maxSteps = 25, lastToolName = "scroll",
            lastScreenHash = 100, isCancelled = false,
            lastToolWasUiMutating = true,
            preActionScreenHash = 100
        )
        val result = check.check(state)
        assertIs<CheckResult.Inject>(result)
    }

    @Test
    fun `warning message contains tool name`() = runTest {
        val check = ActionVerificationCheck()
        val state = LoopState(
            step = 1, maxSteps = 25, lastToolName = "scroll",
            lastScreenHash = 100, isCancelled = false,
            lastToolWasUiMutating = true,
            preActionScreenHash = 100
        )
        val result = check.check(state)
        assertIs<CheckResult.Inject>(result)
        assertTrue(result.message.contains("scroll"), "Warning should contain tool name 'scroll'")
        assertTrue(result.message.contains("ACTION_UNVERIFIED"), "Warning should contain ACTION_UNVERIFIED")
    }

    @Test
    fun `non-UI tool returns Continue without verification`() = runTest {
        val check = ActionVerificationCheck()
        val state = LoopState(
            step = 1, maxSteps = 25, lastToolName = "web_search",
            lastScreenHash = 100, isCancelled = false,
            lastToolWasUiMutating = false,
            preActionScreenHash = 100
        )
        assertEquals(CheckResult.Continue, check.check(state))
    }

    @Test
    fun `null pre-action hash returns Continue`() = runTest {
        val check = ActionVerificationCheck()
        val state = LoopState(
            step = 1, maxSteps = 25, lastToolName = "tap",
            lastScreenHash = 100, isCancelled = false,
            lastToolWasUiMutating = true,
            preActionScreenHash = null
        )
        assertEquals(CheckResult.Continue, check.check(state))
    }

    @Test
    fun `null post-action hash returns Continue`() = runTest {
        val check = ActionVerificationCheck()
        val state = LoopState(
            step = 1, maxSteps = 25, lastToolName = "tap",
            lastScreenHash = null, isCancelled = false,
            lastToolWasUiMutating = true,
            preActionScreenHash = 100
        )
        assertEquals(CheckResult.Continue, check.check(state))
    }

    @Test
    fun `first call with no pre-action hash returns Continue`() = runTest {
        val check = ActionVerificationCheck()
        // preActionScreenHash defaults to null
        val state = LoopState(
            step = 1, maxSteps = 25, lastToolName = "tap",
            lastScreenHash = 100, isCancelled = false,
            lastToolWasUiMutating = true
        )
        assertEquals(CheckResult.Continue, check.check(state))
    }

    // ====== Integration: default boundary checks position ======

    @Test
    fun `ActionVerificationCheck is at correct position in default boundary checks when interruption checks disabled`() {
        val original = FeatureFlags.userInterruptionCheckEnabled
        try {
            FeatureFlags.userInterruptionCheckEnabled = false
            val defaults = AgentExecutor.defaultBoundaryChecks()
            assertEquals(5, defaults.size)
            assertIs<CancellationCheck>(defaults[0])
            assertIs<StepLimitCheck>(defaults[1])
            assertIs<StuckDetectionCheck>(defaults[2])
            assertIs<ActionVerificationCheck>(defaults[3])
            assertIs<SteerCheck>(defaults[4])
        } finally {
            FeatureFlags.userInterruptionCheckEnabled = original
        }
    }

    @Test
    fun `ActionVerificationCheck keeps UserInterruptionCheck ordering when interruption checks enabled`() {
        val original = FeatureFlags.userInterruptionCheckEnabled
        try {
            FeatureFlags.userInterruptionCheckEnabled = true
            val checks = AgentExecutor.defaultBoundaryChecksWithAccessibility(
                isAvailable = { true },
                waitForReconnect = { true },
                onReconnected = {},
                onLost = {}
            )
            assertEquals(7, checks.size)
            assertIs<CancellationCheck>(checks[0])
            assertIs<AccessibilityGateCheck>(checks[1])
            assertIs<StepLimitCheck>(checks[2])
            assertIs<StuckDetectionCheck>(checks[3])
            assertIs<ActionVerificationCheck>(checks[4])
            assertIs<UserInterruptionCheck>(checks[5])
            assertIs<SteerCheck>(checks[6])
        } finally {
            FeatureFlags.userInterruptionCheckEnabled = original
        }
    }

    // ====== Integration: end-to-end with AgentExecutor ======

    @Test
    fun `action verification injects warning in full executor loop`() = runTest {
        val delegate = FakeToolExecutionDelegate()
        val listener = FakeLoopProgressListener()
        val screen = ScreenContent(packageName = "com.test", elements = emptyList())
        delegate.refreshAfterToolResult = screen
        delegate.executeResult = ToolResult("Tapped element 1")

        val executor = AgentExecutor(delegate, listener)
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "tap", mapOf("element_id" to 1))),
            stopReason = "tool_use"
        )

        executor.run(response, screen, { false }) {
            ChatResponse(text = "Done", toolCalls = emptyList(), stopReason = "end_turn")
        }

        val result = delegate.toolResults.first().second
        assertTrue(result.contains("ACTION_UNVERIFIED"), "Expected action verification warning in tool result, got: $result")
        assertTrue(result.contains("tap"), "Warning should mention the tool name")
    }

    @Test
    fun `action verification does not warn when screen changes`() = runTest {
        val delegate = FakeToolExecutionDelegate()
        val listener = FakeLoopProgressListener()
        val screenBefore = ScreenContent(packageName = "com.before", elements = emptyList())
        val screenAfter = ScreenContent(packageName = "com.after", elements = emptyList())
        delegate.refreshAfterToolResult = screenAfter
        delegate.executeResult = ToolResult("Tapped element 1")

        val executor = AgentExecutor(delegate, listener)
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "tap", mapOf("element_id" to 1))),
            stopReason = "tool_use"
        )

        executor.run(response, screenBefore, { false }) {
            ChatResponse(text = "Done", toolCalls = emptyList(), stopReason = "end_turn")
        }

        val result = delegate.toolResults.first().second
        assertTrue(!result.contains("ACTION_UNVERIFIED"), "Should not warn when screen changed, got: $result")
    }
}
