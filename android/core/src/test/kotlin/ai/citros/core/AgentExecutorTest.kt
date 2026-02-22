package ai.citros.core

import kotlinx.coroutines.test.runTest
import org.junit.Before
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertFailsWith
import kotlin.test.assertIs
import kotlin.test.assertTrue

class AgentExecutorTest {

    companion object {
        private const val DEFAULT_MAX_STEPS = 25
    }

    private lateinit var mockDelegate: FakeToolExecutionDelegate
    private lateinit var mockListener: FakeLoopProgressListener

    @Before
    fun setup() {
        mockDelegate = FakeToolExecutionDelegate()
        mockListener = FakeLoopProgressListener()
    }

    // ====== Core loop behavior ======

    @Test
    fun `no tools returns immediately with no_tools`() = runTest {
        val executor = AgentExecutor(mockDelegate, mockListener)
        val response = ChatResponse(text = "Hello!", toolCalls = emptyList(), stopReason = "end_turn")

        val result = executor.run(response, null, { false }) {
            ChatResponse(text = "unused", toolCalls = emptyList(), stopReason = "end_turn")
        }

        assertIs<LoopResult.Completed>(result)
        assertEquals("no_tools", result.exitReason)
        assertEquals(0, result.steps)
        assertEquals("Hello!", result.text)
    }

    @Test
    fun `loop completes on end_turn with final text`() = runTest {
        val executor = AgentExecutor(mockDelegate, mockListener)
        val initialResponse = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "press_back", emptyMap())),
            stopReason = "tool_use"
        )

        val result = executor.run(initialResponse, null, { false }) {
            ChatResponse(text = "Done!", toolCalls = emptyList(), stopReason = "end_turn")
        }

        assertIs<LoopResult.Completed>(result)
        assertEquals("Done!", result.text)
        assertEquals(1, result.steps)
        assertEquals("end_turn", result.exitReason)
    }

    @Test
    fun `loop stops at max steps via StepLimitCheck`() = runTest {
        val executor = AgentExecutor(mockDelegate, mockListener, maxToolSteps = 3)
        val toolResponse = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "press_back", emptyMap())),
            stopReason = "tool_use"
        )

        val result = executor.run(toolResponse, null, { false }) { toolResponse }

        assertIs<LoopResult.Completed>(result)
        assertEquals("max_steps", result.exitReason)
        assertEquals(3, result.steps)
    }

    @Test
    fun `loop stops on cancellation via CancellationCheck`() = runTest {
        var stepCount = 0
        val executor = AgentExecutor(mockDelegate, mockListener)
        val toolResponse = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "press_back", emptyMap())),
            stopReason = "tool_use"
        )

        val result = executor.run(toolResponse, null, { stepCount > 0 }) {
            stepCount++
            toolResponse
        }

        assertIs<LoopResult.Completed>(result)
        assertEquals("cancelled", result.exitReason)
    }

    @Test
    fun `accessibility loss returns accessibility_lost reason`() = runTest {
        mockDelegate.screenReaderAvailable = false
        mockDelegate.accessibilityWaitResult = false

        val executor = AgentExecutor(
            mockDelegate, mockListener,
            boundaryChecks = AgentExecutor.defaultBoundaryChecksWithAccessibility(
                isAvailable = { mockDelegate.isScreenReaderAvailable() },
                waitForReconnect = { timeout -> mockDelegate.waitForAccessibility(timeout) },
                onReconnected = { mockDelegate.refreshScreen() },
                onLost = { mockListener.onAccessibilityLost() },
                baseTimeoutMs = mockDelegate.accessibilityWaitMs(), maxRetries = 1
            )
        )
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "tap", mapOf("element_id" to 1))),
            stopReason = "tool_use"
        )

        val result = executor.run(response, null, { false }) {
            ChatResponse(text = "unused", toolCalls = emptyList(), stopReason = "end_turn")
        }

        assertIs<LoopResult.Completed>(result)
        assertEquals("accessibility_lost", result.exitReason)
        assertTrue(mockListener.accessibilityLostCalled)
    }

    @Test
    fun `tool results are reported to listener`() = runTest {
        mockDelegate.executeResult = ToolResult("Opened Gmail")
        val executor = AgentExecutor(mockDelegate, mockListener)
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "open_app", mapOf("app_name" to "Gmail"))),
            stopReason = "tool_use"
        )

        executor.run(response, null, { false }) {
            ChatResponse(text = "Done", toolCalls = emptyList(), stopReason = "end_turn")
        }

        assertEquals(1, mockListener.toolResults.size)
        assertEquals("open_app", mockListener.toolResults[0].first)
        assertEquals("Opened Gmail", mockListener.toolResults[0].second)
    }

    @Test
    fun `ui mutating tools trigger refreshScreenAfterTool`() = runTest {
        val screen = ScreenContent(packageName = "com.gmail", elements = emptyList())
        mockDelegate.refreshAfterToolResult = screen
        mockDelegate.executeResult = ToolResult("Tapped element 5")

        val executor = AgentExecutor(mockDelegate, mockListener)
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "tap", mapOf("element_id" to 5))),
            stopReason = "tool_use"
        )

        executor.run(response, null, { false }) {
            ChatResponse(text = "Done", toolCalls = emptyList(), stopReason = "end_turn")
        }

        assertTrue(mockDelegate.refreshAfterToolCalled)
    }

    @Test
    fun `non-mutating tools do not trigger screen refresh`() = runTest {
        mockDelegate.executeResult = ToolResult("Thought: hmm")
        mockDelegate.uiMutatingTools = emptySet()

        val executor = AgentExecutor(mockDelegate, mockListener)
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "think", mapOf("thought" to "hmm"))),
            stopReason = "tool_use"
        )

        executor.run(response, null, { false }) {
            ChatResponse(text = "Done", toolCalls = emptyList(), stopReason = "end_turn")
        }

        assertEquals(false, mockDelegate.refreshAfterToolCalled)
    }

    @Test
    fun `continuation error is handled gracefully`() = runTest {
        val executor = AgentExecutor(mockDelegate, mockListener)
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "press_back", emptyMap())),
            stopReason = "tool_use"
        )

        val result = executor.run(response, null, { false }) {
            throw RuntimeException("API connection lost")
        }

        assertIs<LoopResult.Completed>(result)
        assertEquals(1, result.steps)
    }

    @Test
    fun `step counter reported via onStepStarted`() = runTest {
        val executor = AgentExecutor(mockDelegate, mockListener)
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "press_back", emptyMap())),
            stopReason = "tool_use"
        )

        executor.run(response, null, { false }) {
            ChatResponse(text = "Done", toolCalls = emptyList(), stopReason = "end_turn")
        }

        assertEquals(1, mockDelegate.lastStepStarted)
    }

    @Test
    fun `tool execution error is caught and passed as result`() = runTest {
        mockDelegate.onExecute = { _, _ -> throw RuntimeException("Crash!") }

        val executor = AgentExecutor(mockDelegate, mockListener)
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "tap", mapOf("element_id" to 1))),
            stopReason = "tool_use"
        )

        val result = executor.run(response, null, { false }) {
            ChatResponse(text = "Done", toolCalls = emptyList(), stopReason = "end_turn")
        }

        assertIs<LoopResult.Completed>(result)
        assertEquals("end_turn", result.exitReason)
        assertTrue(mockDelegate.toolResults.any { it.second.contains("Error: Crash!") })
    }

    @Test
    fun `private read_screen result progresses to blind action without repeated read retries`() = runTest {
        mockDelegate.onExecute = { toolCall, _ ->
            when (toolCall.name) {
                "read_screen" -> ToolResult("Screen refreshed:\nSCREEN: [Privacy mode — screen content hidden for private_app. Ask the user for guidance if needed.]", isError = true)
                "press_back" -> ToolResult("Pressed back")
                else -> ToolResult("Unexpected tool ${toolCall.name}", isError = true)
            }
        }

        val executor = AgentExecutor(mockDelegate, mockListener)
        val initialResponse = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "read_screen", emptyMap())),
            stopReason = "tool_use"
        )
        var continueCalls = 0

        val result = executor.run(initialResponse, null, { false }) {
            continueCalls++
            if (continueCalls == 1) {
                ChatResponse(
                    text = null,
                    toolCalls = listOf(ToolCall("t2", "press_back", emptyMap())),
                    stopReason = "tool_use"
                )
            } else {
                ChatResponse(text = "Recovered with blind action", toolCalls = emptyList(), stopReason = "end_turn")
            }
        }

        assertIs<LoopResult.Completed>(result)
        assertEquals("end_turn", result.exitReason)
        assertEquals(listOf("read_screen", "press_back"), mockListener.toolResults.map { it.first })
        assertEquals(1, mockListener.toolResults.count { it.first == "read_screen" })
        assertTrue(mockDelegate.toolResults.first().second.contains("private_app"))
        assertFalse(mockDelegate.toolResults.first().second.contains("com.bank.app"))
    }

    @Test
    fun `private screenshot block progresses to blind action without repeated screenshot retries`() = runTest {
        mockDelegate.onExecute = { toolCall, _ ->
            when (toolCall.name) {
                "screenshot" -> ToolResult("Failed: screenshot: blocked by privacy mode for private_app", isError = true)
                "press_home" -> ToolResult("Pressed home")
                else -> ToolResult("Unexpected tool ${toolCall.name}", isError = true)
            }
        }

        val executor = AgentExecutor(mockDelegate, mockListener)
        val initialResponse = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "screenshot", emptyMap())),
            stopReason = "tool_use"
        )
        var continueCalls = 0

        val result = executor.run(initialResponse, null, { false }) {
            continueCalls++
            if (continueCalls == 1) {
                ChatResponse(
                    text = null,
                    toolCalls = listOf(ToolCall("t2", "press_home", emptyMap())),
                    stopReason = "tool_use"
                )
            } else {
                ChatResponse(text = "Used blind fallback", toolCalls = emptyList(), stopReason = "end_turn")
            }
        }

        assertIs<LoopResult.Completed>(result)
        assertEquals("end_turn", result.exitReason)
        assertEquals(listOf("screenshot", "press_home"), mockListener.toolResults.map { it.first })
        assertEquals(1, mockListener.toolResults.count { it.first == "screenshot" })
        assertTrue(mockDelegate.toolResults.first().second.contains("private_app"))
    }

    @Test
    fun `multiple tool calls in one response are all executed`() = runTest {
        val executor = AgentExecutor(mockDelegate, mockListener)
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(
                ToolCall("t1", "press_back", emptyMap()),
                ToolCall("t2", "press_home", emptyMap())
            ),
            stopReason = "tool_use"
        )

        executor.run(response, null, { false }) {
            ChatResponse(text = "Done", toolCalls = emptyList(), stopReason = "end_turn")
        }

        assertEquals(2, mockDelegate.toolResults.size)
    }

    @Test
    fun `Stop check mid-batch skips remaining tool calls`() = runTest {
        var checkCount = 0
        val stopAfterTwo = object : BoundaryCheck {
            override suspend fun check(state: LoopState): CheckResult {
                checkCount++
                return if (checkCount >= 2) CheckResult.Stop("mid_batch") else CheckResult.Continue
            }
        }
        val executor = AgentExecutor(
            mockDelegate, mockListener,
            boundaryChecks = listOf(stopAfterTwo),
            maxToolSteps = DEFAULT_MAX_STEPS
        )
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(
                ToolCall("t1", "press_back", emptyMap()),
                ToolCall("t2", "press_home", emptyMap()),
                ToolCall("t3", "tap", mapOf("element_id" to 1))  // Should not execute
            ),
            stopReason = "tool_use"
        )

        val result = executor.run(response, null, { false }) {
            ChatResponse(text = "unused", toolCalls = emptyList(), stopReason = "end_turn")
        }

        assertIs<LoopResult.Completed>(result)
        assertEquals("mid_batch", result.exitReason)
        assertEquals(2, mockDelegate.toolResults.size, "Only t1 and t2 should have results")
    }

    // ====== Stuck detection integration (via StuckDetectionCheck) ======

    @Test
    fun `stuck detection fires mid-loop and injects warning into tool result`() = runTest {
        val sameScreen = ScreenContent(packageName = "com.stuck.app", elements = emptyList())
        mockDelegate.refreshAfterToolResult = sameScreen
        mockDelegate.executeResult = ToolResult("Tapped element 1")
        var stepCount = 0

        val executor = AgentExecutor(mockDelegate, mockListener)
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "tap", mapOf("element_id" to 1))),
            stopReason = "tool_use"
        )

        executor.run(response, sameScreen, { false }) {
            stepCount++
            if (stepCount >= 4) {
                ChatResponse(text = "Giving up", toolCalls = emptyList(), stopReason = "end_turn")
            } else {
                ChatResponse(
                    text = null,
                    toolCalls = listOf(ToolCall("t${stepCount + 1}", "tap", mapOf("element_id" to 1))),
                    stopReason = "tool_use"
                )
            }
        }

        val stuckResults = mockDelegate.toolResults.filter { it.second.contains("STUCK") }
        assertTrue(stuckResults.isNotEmpty(), "Expected stuck warning to be injected into at least one tool result")
    }

    // ====== Accessibility reattachment (via AccessibilityGateCheck) ======

    @Test
    fun `screen is refreshed after accessibility reattaches`() = runTest {
        mockDelegate.screenReaderAvailable = false
        mockDelegate.accessibilityWaitResult = true
        val refreshedScreen = ScreenContent(packageName = "com.reattached", elements = emptyList())
        mockDelegate.refreshScreenResult = refreshedScreen
        mockDelegate.refreshAfterToolResult = refreshedScreen

        val executor = AgentExecutor(
            mockDelegate, mockListener,
            boundaryChecks = AgentExecutor.defaultBoundaryChecksWithAccessibility(
                isAvailable = { mockDelegate.isScreenReaderAvailable() },
                waitForReconnect = { timeout -> mockDelegate.waitForAccessibility(timeout) },
                onReconnected = { mockDelegate.refreshScreen() },
                onLost = { mockListener.onAccessibilityLost() },
                baseTimeoutMs = mockDelegate.accessibilityWaitMs(), maxRetries = 1
            )
        )
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "tap", mapOf("element_id" to 1))),
            stopReason = "tool_use"
        )

        executor.run(response, null, { false }) {
            ChatResponse(text = "Done", toolCalls = emptyList(), stopReason = "end_turn")
        }

        assertTrue(mockDelegate.refreshScreenCalled, "Expected refreshScreen() after accessibility reattachment")
    }

    @Test
    fun `accessibility gate check stops loop when service is lost`() = runTest {
        mockDelegate.screenReaderAvailable = false
        mockDelegate.accessibilityWaitResult = false

        val executor = AgentExecutor(
            mockDelegate, mockListener,
            boundaryChecks = AgentExecutor.defaultBoundaryChecksWithAccessibility(
                isAvailable = { mockDelegate.isScreenReaderAvailable() },
                waitForReconnect = { timeout -> mockDelegate.waitForAccessibility(timeout) },
                onReconnected = { mockDelegate.refreshScreen() },
                onLost = { mockListener.onAccessibilityLost() },
                baseTimeoutMs = mockDelegate.accessibilityWaitMs(), maxRetries = 1
            )
        )
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(
                ToolCall("t1", "press_back", emptyMap()),
                ToolCall("t2", "press_home", emptyMap())
            ),
            stopReason = "tool_use"
        )

        val result = executor.run(response, null, { false }) {
            ChatResponse(text = "unused", toolCalls = emptyList(), stopReason = "end_turn")
        }

        assertIs<LoopResult.Completed>(result)
        assertEquals("accessibility_lost", result.exitReason)
        assertTrue(mockListener.accessibilityLostCalled)
        // First tool executes (fails naturally), boundary check catches it, loop stops
        assertEquals(1, mockDelegate.toolResults.size, "Only first tool should have a result before accessibility gate stops loop")
    }

    @Test
    fun `accessibility gate allows continuation when service is available`() = runTest {
        // Default: screenReaderAvailable = true
        val executor = AgentExecutor(
            mockDelegate, mockListener,
            boundaryChecks = AgentExecutor.defaultBoundaryChecksWithAccessibility(
                isAvailable = { mockDelegate.isScreenReaderAvailable() },
                waitForReconnect = { timeout -> mockDelegate.waitForAccessibility(timeout) },
                onReconnected = { mockDelegate.refreshScreen() },
                onLost = { mockListener.onAccessibilityLost() },
                baseTimeoutMs = mockDelegate.accessibilityWaitMs(), maxRetries = 1
            )
        )
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "press_back", emptyMap())),
            stopReason = "tool_use"
        )

        val result = executor.run(response, null, { false }) {
            ChatResponse(text = "Done", toolCalls = emptyList(), stopReason = "end_turn")
        }

        assertIs<LoopResult.Completed>(result)
        assertEquals("end_turn", result.exitReason)
        assertEquals("Done", result.text)
    }

    // ====== Boundary check customization ======

    @Test
    fun `custom boundary check can stop the loop`() = runTest {
        val customCheck = object : BoundaryCheck {
            override suspend fun check(state: LoopState): CheckResult {
                return if (state.step >= 2) CheckResult.Stop("custom_limit") else CheckResult.Continue
            }
        }
        val executor = AgentExecutor(
            mockDelegate, mockListener,
            boundaryChecks = listOf(CancellationCheck(), customCheck),
            maxToolSteps = 100
        )
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "press_back", emptyMap())),
            stopReason = "tool_use"
        )

        val result = executor.run(response, null, { false }) { response }

        assertIs<LoopResult.Completed>(result)
        assertEquals("custom_limit", result.exitReason)
        assertEquals(2, result.steps)
    }

    @Test
    fun `custom boundary check can inject into tool results`() = runTest {
        val customCheck = object : BoundaryCheck {
            override suspend fun check(state: LoopState): CheckResult {
                return CheckResult.Inject("\n\n[custom note]")
            }
        }
        val executor = AgentExecutor(
            mockDelegate, mockListener,
            boundaryChecks = listOf(CancellationCheck(), StepLimitCheck(), customCheck),
            maxToolSteps = 5
        )
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "press_back", emptyMap())),
            stopReason = "tool_use"
        )

        executor.run(response, null, { false }) {
            ChatResponse(text = "Done", toolCalls = emptyList(), stopReason = "end_turn")
        }

        assertTrue(
            mockDelegate.toolResults.any { it.second.contains("[custom note]") },
            "Expected custom injection in tool results"
        )
    }

    @Test
    fun `Stop takes priority over Inject in same evaluation`() = runTest {
        val injectCheck = object : BoundaryCheck {
            override suspend fun check(state: LoopState): CheckResult = CheckResult.Inject("[note]")
        }
        val stopCheck = object : BoundaryCheck {
            override suspend fun check(state: LoopState): CheckResult = CheckResult.Stop("forced")
        }
        // Stop check is first — should short-circuit before inject
        val executor = AgentExecutor(
            mockDelegate, mockListener,
            boundaryChecks = listOf(stopCheck, injectCheck),
            maxToolSteps = 100
        )
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "press_back", emptyMap())),
            stopReason = "tool_use"
        )

        val result = executor.run(response, null, { false }) { response }

        assertIs<LoopResult.Completed>(result)
        assertEquals("forced", result.exitReason)
        assertEquals(1, result.steps)
        // Tool result should NOT contain injection (Stop short-circuits)
        assertTrue(mockDelegate.toolResults.none { it.second.contains("[note]") })
    }

    @Test
    fun `multiple Inject results are concatenated`() = runTest {
        val checkA = object : BoundaryCheck {
            override suspend fun check(state: LoopState): CheckResult = CheckResult.Inject("[a]")
        }
        val checkB = object : BoundaryCheck {
            override suspend fun check(state: LoopState): CheckResult = CheckResult.Inject("[b]")
        }
        val executor = AgentExecutor(
            mockDelegate, mockListener,
            boundaryChecks = listOf(CancellationCheck(), checkA, checkB, StepLimitCheck()),
            maxToolSteps = 5
        )
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "press_back", emptyMap())),
            stopReason = "tool_use"
        )

        executor.run(response, null, { false }) {
            ChatResponse(text = "Done", toolCalls = emptyList(), stopReason = "end_turn")
        }

        val result = mockDelegate.toolResults.first().second
        assertTrue(result.contains("[a]"), "Expected first injection")
        assertTrue(result.contains("[b]"), "Expected second injection")
    }

    @Test
    fun `empty boundary checks list means no limits`() = runTest {
        var stepCount = 0
        val executor = AgentExecutor(
            mockDelegate, mockListener,
            boundaryChecks = emptyList(),
            maxToolSteps = 100
        )
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "press_back", emptyMap())),
            stopReason = "tool_use"
        )

        val result = executor.run(response, null, { false }) {
            stepCount++
            if (stepCount >= 5) {
                ChatResponse(text = "Done", toolCalls = emptyList(), stopReason = "end_turn")
            } else {
                response
            }
        }

        // Loop should run all 5 steps without boundary checks stopping it
        assertIs<LoopResult.Completed>(result)
        assertEquals("end_turn", result.exitReason)
        assertEquals(5, result.steps)
    }

    @Test
    fun `default boundary checks are in priority order`() {
        val defaults = AgentExecutor.defaultBoundaryChecks()

        assertEquals(6, defaults.size)
        assertIs<CancellationCheck>(defaults[0], "Cancellation should be first (highest priority)")
        assertIs<StepLimitCheck>(defaults[1], "Step limit should be second")
        assertIs<StuckDetectionCheck>(defaults[2], "Stuck detection should be third")
        assertIs<ActionVerificationCheck>(defaults[3], "Action verification should be fourth")
        assertIs<UserInterruptionCheck>(defaults[4], "User interruption should be fifth")
        assertIs<SteerCheck>(defaults[5], "Steer should be last (user intent after all gates)")
    }

    @Test
    fun `defaultBoundaryChecksWithAccessibility includes AccessibilityGateCheck`() {
        val checks = AgentExecutor.defaultBoundaryChecksWithAccessibility(
            isAvailable = { true },
            waitForReconnect = { true },
            onReconnected = {},
            onLost = {}
        )

        assertEquals(7, checks.size)
        assertIs<CancellationCheck>(checks[0], "Cancellation should be first")
        assertIs<AccessibilityGateCheck>(checks[1], "Accessibility gate should be second")
        assertIs<StepLimitCheck>(checks[2], "Step limit should be third")
        assertIs<StuckDetectionCheck>(checks[3], "Stuck detection should be fourth")
        assertIs<ActionVerificationCheck>(checks[4], "Action verification should be fifth")
        assertIs<UserInterruptionCheck>(checks[5], "User interruption should be sixth")
        assertIs<SteerCheck>(checks[6], "Steer should be last")
    }


    // ====== Steer behavior ======

    @Test
    fun `pre-batch steer delivers messages without executing tools`() = runTest {
        val executor = AgentExecutor(
            mockDelegate, mockListener,
            boundaryChecks = listOf(CancellationCheck(), SteerCheck()),
            steerMessageSource = {
                // Return steer on first call, empty after
                if (mockDelegate.steerMessages.isEmpty()) listOf("wrong app")
                else emptyList()
            }
        )
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "tap", mapOf("element_id" to 1))),
            stopReason = "tool_use"
        )

        val result = executor.run(response, null, { false }) {
            ChatResponse(text = "Switching to Calendar", toolCalls = emptyList(), stopReason = "end_turn")
        }

        assertIs<LoopResult.Completed>(result)
        assertEquals("end_turn", result.exitReason)
        assertEquals(0, result.steps, "Pre-batch steer should not increment step counter")
        assertEquals(listOf("wrong app"), mockDelegate.steerMessages)
        assertEquals(0, mockDelegate.toolResults.size, "No tools should execute on pre-batch steer")
    }

    @Test
    fun `post-tool steer skips remaining tool calls in batch`() = runTest {
        var steerCallCount = 0
        val executor = AgentExecutor(
            mockDelegate, mockListener,
            boundaryChecks = listOf(CancellationCheck(), SteerCheck()),
            steerMessageSource = {
                steerCallCount++
                // Return steer on 2nd drain (post-tool boundary of first tool call)
                // 1st drain = pre-batch (empty), 2nd drain = after t1 executes
                if (steerCallCount == 2) listOf("use Calendar instead")
                else emptyList()
            }
        )
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(
                ToolCall("t1", "press_back", emptyMap()),
                ToolCall("t2", "press_home", emptyMap()),
                ToolCall("t3", "tap", mapOf("element_id" to 1))
            ),
            stopReason = "tool_use"
        )

        val result = executor.run(response, null, { false }) {
            ChatResponse(text = "Opening Calendar", toolCalls = emptyList(), stopReason = "end_turn")
        }

        assertIs<LoopResult.Completed>(result)
        assertEquals(1, result.steps)
        // t1 gets its real result, t2 and t3 get explicit skip results
        assertEquals(3, mockDelegate.toolResults.size, "All 3 tool calls should have results (1 real + 2 skipped)")
        assertEquals("t1", mockDelegate.toolResults[0].first)
        assertEquals("t2", mockDelegate.toolResults[1].first)
        assertEquals("Skipped: user sent a new message.", mockDelegate.toolResults[1].second)
        assertEquals("t3", mockDelegate.toolResults[2].first)
        assertEquals("Skipped: user sent a new message.", mockDelegate.toolResults[2].second)
        assertEquals(listOf("use Calendar instead"), mockDelegate.steerMessages)
    }

    @Test
    fun `steer messages are delivered as user messages via addSteerMessage`() = runTest {
        var steerCallCount = 0
        val executor = AgentExecutor(
            mockDelegate, mockListener,
            boundaryChecks = listOf(CancellationCheck(), SteerCheck()),
            steerMessageSource = {
                steerCallCount++
                if (steerCallCount == 2) listOf("msg1", "msg2")
                else emptyList()
            }
        )
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "press_back", emptyMap())),
            stopReason = "tool_use"
        )

        executor.run(response, null, { false }) {
            ChatResponse(text = "Done", toolCalls = emptyList(), stopReason = "end_turn")
        }

        assertEquals(listOf("msg1", "msg2"), mockDelegate.steerMessages)
    }

    @Test
    fun `steer continues loop instead of exiting`() = runTest {
        var steerCallCount = 0
        var continuationCount = 0
        val executor = AgentExecutor(
            mockDelegate, mockListener,
            boundaryChecks = listOf(CancellationCheck(), SteerCheck()),
            steerMessageSource = {
                steerCallCount++
                if (steerCallCount == 2) listOf("redirect")
                else emptyList()
            }
        )
        val toolResponse = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "press_back", emptyMap())),
            stopReason = "tool_use"
        )

        val result = executor.run(toolResponse, null, { false }) {
            continuationCount++
            if (continuationCount >= 2) {
                ChatResponse(text = "Done after steer", toolCalls = emptyList(), stopReason = "end_turn")
            } else {
                // After steer, model gets a new turn with more tools
                ChatResponse(
                    text = null,
                    toolCalls = listOf(ToolCall("t2", "press_home", emptyMap())),
                    stopReason = "tool_use"
                )
            }
        }

        assertIs<LoopResult.Completed>(result)
        assertEquals("end_turn", result.exitReason)
        assertEquals("Done after steer", result.text)
        assertTrue(continuationCount >= 2, "Loop should have continued after steer")
    }

    @Test
    fun `cancellation with pending steer delivers messages then stops`() = runTest {
        // Pre-batch steer fires, messages are delivered, then cancellation check
        // stops the loop. Messages are preserved in history (user sent them).
        val executor = AgentExecutor(
            mockDelegate, mockListener,
            boundaryChecks = listOf(CancellationCheck(), SteerCheck()),
            steerMessageSource = {
                // Always return a steer message
                listOf("steer message")
            }
        )
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "press_back", emptyMap())),
            stopReason = "tool_use"
        )

        // isCancelled = true — but steer messages are delivered first (pre-batch)
        val result = executor.run(response, null, { true }) {
            ChatResponse(text = "unused", toolCalls = emptyList(), stopReason = "end_turn")
        }

        assertIs<LoopResult.Completed>(result)
        assertEquals("cancelled", result.exitReason)
        // Steer messages ARE delivered — users expect sent messages to appear in history
        assertEquals(listOf("steer message"), mockDelegate.steerMessages,
            "Steer messages should be delivered to history even when cancel follows immediately")
        // But no tools should have executed
        assertTrue(mockDelegate.toolResults.isEmpty(), "No tools should execute after steer + cancel")
    }

    @Test
    fun `Stop takes priority over Steer at post-tool boundary`() = runTest {
        // When both Stop (from CancellationCheck) and Steer fire at the SAME
        // post-tool boundary check, Stop wins and steer is not applied.
        var steerCallCount = 0
        var cancelled = false
        val executor = AgentExecutor(
            mockDelegate, mockListener,
            boundaryChecks = listOf(CancellationCheck(), SteerCheck()),
            steerMessageSource = {
                steerCallCount++
                // No steer at pre-batch (count 1), steer at post-tool (count 2)
                if (steerCallCount == 2) {
                    cancelled = true  // Cancel arrives at same time as steer
                    listOf("too late")
                } else emptyList()
            }
        )
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "press_back", emptyMap())),
            stopReason = "tool_use"
        )

        val result = executor.run(response, null, { cancelled }) {
            ChatResponse(text = "unused", toolCalls = emptyList(), stopReason = "end_turn")
        }

        assertIs<LoopResult.Completed>(result)
        assertEquals("cancelled", result.exitReason)
        // At post-tool boundary: CancellationCheck returns Stop, which short-circuits
        // before SteerCheck runs. Steer messages from source are in LoopState but
        // evaluateBoundaryChecks never reaches SteerCheck.
        assertTrue(mockDelegate.steerMessages.isEmpty(),
            "Post-tool steer should not be delivered when Stop short-circuits")
    }

    @Test
    fun `Steer takes priority over Inject even when Inject fires first`() = runTest {
        var injectCheckRan = false
        val injectCheck = object : BoundaryCheck {
            override suspend fun check(state: LoopState): CheckResult {
                injectCheckRan = true
                return CheckResult.Inject("[warning]")
            }
        }
        var steerCallCount = 0
        val executor = AgentExecutor(
            mockDelegate, mockListener,
            boundaryChecks = listOf(injectCheck, SteerCheck()),
            steerMessageSource = {
                steerCallCount++
                if (steerCallCount == 2) listOf("user redirect")
                else emptyList()
            }
        )
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "press_back", emptyMap())),
            stopReason = "tool_use"
        )

        executor.run(response, null, { false }) {
            ChatResponse(text = "Done", toolCalls = emptyList(), stopReason = "end_turn")
        }

        // Both checks should have run, but Steer wins
        assertTrue(injectCheckRan, "Inject check should have been evaluated")
        val t1Result = mockDelegate.toolResults.first { it.first == "t1" }.second
        assertTrue(!t1Result.contains("[warning]"), "Inject should not apply when Steer takes priority")
        assertEquals(listOf("user redirect"), mockDelegate.steerMessages)
    }

    @Test
    fun `pre-batch steer handles API error gracefully`() = runTest {
        val executor = AgentExecutor(
            mockDelegate, mockListener,
            boundaryChecks = listOf(CancellationCheck(), SteerCheck()),
            steerMessageSource = {
                if (mockDelegate.steerMessages.isEmpty()) listOf("steer msg")
                else emptyList()
            }
        )
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "tap", mapOf("element_id" to 1))),
            stopReason = "tool_use"
        )

        val result = executor.run(response, null, { false }) {
            throw RuntimeException("API connection lost")
        }

        assertIs<LoopResult.Completed>(result)
        // Should have handled the error — steer message was delivered
        assertEquals(listOf("steer msg"), mockDelegate.steerMessages)
    }

    @Test
    fun `empty steer source means no steer behavior`() = runTest {
        val executor = AgentExecutor(
            mockDelegate, mockListener,
            steerMessageSource = { emptyList() }
        )
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "press_back", emptyMap())),
            stopReason = "tool_use"
        )

        val result = executor.run(response, null, { false }) {
            ChatResponse(text = "Done", toolCalls = emptyList(), stopReason = "end_turn")
        }

        assertIs<LoopResult.Completed>(result)
        assertEquals("end_turn", result.exitReason)
        assertTrue(mockDelegate.steerMessages.isEmpty())
    }


    @Test
    fun `pre-batch steer delivers messages even when cancelled immediately after`() = runTest {
        var cancelled = false
        val executor = AgentExecutor(
            mockDelegate, mockListener,
            boundaryChecks = listOf(CancellationCheck(), SteerCheck()),
            steerMessageSource = {
                if (mockDelegate.steerMessages.isEmpty()) {
                    // Simulate user sending steer then immediately hitting cancel
                    cancelled = true
                    listOf("urgent redirect")
                } else emptyList()
            }
        )
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "tap", mapOf("element_id" to 1))),
            stopReason = "tool_use"
        )

        val result = executor.run(response, null, { cancelled }) {
            ChatResponse(text = "unused", toolCalls = emptyList(), stopReason = "end_turn")
        }

        assertIs<LoopResult.Completed>(result)
        assertEquals("cancelled", result.exitReason)
        // Critical: steer message MUST be delivered even though cancel followed immediately
        assertEquals(listOf("urgent redirect"), mockDelegate.steerMessages,
            "Steer messages should be delivered to history even when cancelled immediately after")
        assertEquals(0, result.steps, "No tools should execute")
    }

    @Test
    fun `pre-batch steer API error returns explicit exit reason`() = runTest {
        val executor = AgentExecutor(
            mockDelegate, mockListener,
            boundaryChecks = listOf(CancellationCheck(), SteerCheck()),
            steerMessageSource = {
                if (mockDelegate.steerMessages.isEmpty()) listOf("redirect")
                else emptyList()
            }
        )
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "tap", mapOf("element_id" to 1))),
            stopReason = "tool_use"
        )

        val result = executor.run(response, null, { false }) {
            throw RuntimeException("Connection reset")
        }

        assertIs<LoopResult.Completed>(result)
        assertEquals("api_error_after_steer", result.exitReason,
            "Should return explicit exit reason, not continue with error response")
        assertEquals(listOf("redirect"), mockDelegate.steerMessages,
            "Steer messages should still be delivered despite API error")
    }

    @Test
    fun `multiple steer messages drain atomically in integration`() = runTest {
        var steerCallCount = 0
        val executor = AgentExecutor(
            mockDelegate, mockListener,
            boundaryChecks = listOf(CancellationCheck(), SteerCheck()),
            steerMessageSource = {
                steerCallCount++
                if (steerCallCount == 2) listOf("first", "second", "third")
                else emptyList()
            }
        )
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "press_back", emptyMap())),
            stopReason = "tool_use"
        )

        executor.run(response, null, { false }) {
            ChatResponse(text = "Done", toolCalls = emptyList(), stopReason = "end_turn")
        }

        assertEquals(listOf("first", "second", "third"), mockDelegate.steerMessages,
            "All messages from a single drain should be delivered")
    }

    @Test
    fun `steer mid-batch generates skip results for all remaining tool calls`() = runTest {
        // Regression test for #482: When steer fires after tool N in a batch of M,
        // tools N+1..M must get explicit "Skipped" results to maintain the API contract
        // (every tool_use must have a corresponding tool_result).
        var steerCallCount = 0
        val executor = AgentExecutor(
            mockDelegate, mockListener,
            boundaryChecks = listOf(CancellationCheck(), SteerCheck()),
            steerMessageSource = {
                steerCallCount++
                // Steer fires after 2nd tool call (3rd drain: pre-batch=1, post-t1=2, post-t2=3)
                if (steerCallCount == 3) listOf("stop, do something else")
                else emptyList()
            }
        )
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(
                ToolCall("t1", "press_back", emptyMap()),
                ToolCall("t2", "tap", mapOf("element_id" to 1)),
                ToolCall("t3", "type_text", mapOf("text" to "hello")),
                ToolCall("t4", "press_home", emptyMap()),
                ToolCall("t5", "open_app", mapOf("app_name" to "Gmail"))
            ),
            stopReason = "tool_use"
        )

        val result = executor.run(response, null, { false }) {
            ChatResponse(text = "OK, doing something else", toolCalls = emptyList(), stopReason = "end_turn")
        }

        assertIs<LoopResult.Completed>(result)
        assertEquals("end_turn", result.exitReason)

        // t1 and t2 executed normally, t3-t5 got skip results
        assertEquals(5, mockDelegate.toolResults.size,
            "All 5 tool calls must have results (2 real + 3 skipped)")
        // t1, t2 = real results
        assertEquals("t1", mockDelegate.toolResults[0].first)
        assertEquals("t2", mockDelegate.toolResults[1].first)
        // t3, t4, t5 = skip results
        for (i in 2..4) {
            assertEquals("Skipped: user sent a new message.", mockDelegate.toolResults[i].second,
                "Tool \${mockDelegate.toolResults[i].first} should have skip result")
        }
        assertEquals(listOf("stop, do something else"), mockDelegate.steerMessages)
    }

    @Test
    fun `steer on last tool in batch produces no skip results`() = runTest {
        // When steer fires after the LAST tool in a batch, there are no remaining
        // tools to skip — skip result generation should handle this edge case.
        var steerCallCount = 0
        val executor = AgentExecutor(
            mockDelegate, mockListener,
            boundaryChecks = listOf(CancellationCheck(), SteerCheck()),
            steerMessageSource = {
                steerCallCount++
                // Steer fires after last (3rd) tool call
                if (steerCallCount == 4) listOf("change direction")
                else emptyList()
            }
        )
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(
                ToolCall("t1", "press_back", emptyMap()),
                ToolCall("t2", "press_home", emptyMap()),
                ToolCall("t3", "tap", mapOf("element_id" to 1))
            ),
            stopReason = "tool_use"
        )

        val result = executor.run(response, null, { false }) {
            ChatResponse(text = "Changed", toolCalls = emptyList(), stopReason = "end_turn")
        }

        assertIs<LoopResult.Completed>(result)
        // All 3 tools executed normally — no skips needed
        assertEquals(3, mockDelegate.toolResults.size)
        assertTrue(mockDelegate.toolResults.none { it.second.contains("Skipped") },
            "No skip results should exist when steer fires on the last tool")
        assertEquals(listOf("change direction"), mockDelegate.steerMessages)
    }

    // ====== transformContext hook ======

    @Test
    fun `transformContext hook is called before continueAfterTools`() = runTest {
        val callOrder = mutableListOf<String>()
        val executor = AgentExecutor(
            mockDelegate, mockListener,
            transformContext = { callOrder.add("transform") }
        )
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "press_back", emptyMap())),
            stopReason = "tool_use"
        )

        executor.run(response, null, { false }) {
            callOrder.add("continue")
            ChatResponse(text = "Done", toolCalls = emptyList(), stopReason = "end_turn")
        }

        // transform must come before continue
        assertEquals(listOf("transform", "continue"), callOrder)
    }

    @Test
    fun `transformContext hook is called on every iteration`() = runTest {
        var transformCount = 0
        var continueCount = 0
        val executor = AgentExecutor(
            mockDelegate, mockListener,
            maxToolSteps = 3,
            transformContext = { transformCount++ }
        )
        val toolResponse = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "press_back", emptyMap())),
            stopReason = "tool_use"
        )

        executor.run(toolResponse, null, { false }) {
            continueCount++
            toolResponse // Keep looping until max steps
        }

        // Should be called once per continueAfterTools invocation
        assertEquals(continueCount, transformCount)
    }

    @Test
    fun `transformContext hook is called before continueAfterTools after steer`() = runTest {
        val callOrder = mutableListOf<String>()
        var steerCallCount = 0
        var continueCallCount = 0
        val executor = AgentExecutor(
            mockDelegate, mockListener,
            boundaryChecks = AgentExecutor.defaultBoundaryChecks(),
            steerMessageSource = {
                steerCallCount++
                // Pre-batch steer on first call only
                if (steerCallCount == 1) listOf("change course")
                else emptyList()
            },
            transformContext = { callOrder.add("transform") }
        )
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "press_back", emptyMap())),
            stopReason = "tool_use"
        )

        executor.run(response, null, { false }) {
            callOrder.add("continue")
            continueCallCount++
            if (continueCallCount == 1) {
                // After steer, model returns more tool calls so loop continues
                ChatResponse(
                    text = null,
                    toolCalls = listOf(ToolCall("t2", "tap", mapOf("element_id" to 1))),
                    stopReason = "tool_use"
                )
            } else {
                ChatResponse(text = "Done", toolCalls = emptyList(), stopReason = "end_turn")
            }
        }

        // Call 1: steer path -> transform + continue
        // Call 2: normal loop -> transform + continue
        // Every transform must precede its continue
        assertTrue(callOrder.size >= 4, "Should have at least 2 transform+continue pairs, got ${callOrder.size}: $callOrder")
        for (i in callOrder.indices step 2) {
            if (i + 1 < callOrder.size) {
                assertEquals("transform", callOrder[i], "transform should precede continue at index $i")
                assertEquals("continue", callOrder[i + 1], "continue should follow transform at index $i")
            }
        }
    }

    @Test
    fun `null transformContext is no-op`() = runTest {
        // Default null transformContext should not affect behavior
        val executor = AgentExecutor(mockDelegate, mockListener)
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "press_back", emptyMap())),
            stopReason = "tool_use"
        )

        val result = executor.run(response, null, { false }) {
            ChatResponse(text = "Done", toolCalls = emptyList(), stopReason = "end_turn")
        }

        assertIs<LoopResult.Completed>(result)
        assertEquals("Done", result.text)
        assertEquals("end_turn", result.exitReason)
    }

    @Test
    fun `transformContext exception propagates to caller`() = runTest {
        val executor = AgentExecutor(
            mockDelegate, mockListener,
            transformContext = { throw RuntimeException("Context transform failed") }
        )
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "press_back", emptyMap())),
            stopReason = "tool_use"
        )

        assertFailsWith<RuntimeException>("Context transform failed") {
            executor.run(response, null, { false }) {
                ChatResponse(text = "Done", toolCalls = emptyList(), stopReason = "end_turn")
            }
        }
    }

    // ====== Mocks ======


    @Test
    fun `executeToolCall exception produces isError true in listener`() = runTest {
        mockDelegate.onExecuteThrow = RuntimeException("test crash")
        val executor = AgentExecutor(mockDelegate, mockListener)
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "tap", mapOf("element_id" to 1))),
            stopReason = "tool_use"
        )

        executor.run(response, null, { false }) {
            ChatResponse(text = "Done", toolCalls = emptyList(), stopReason = "end_turn")
        }

        // Verify the listener received isError=true
        assertTrue(mockListener.toolResultsWithError.isNotEmpty(), "should have at least one result")
        val entry = mockListener.toolResultsWithError.first()
        val isError = entry[3] as Boolean
        val result = entry[1] as String
        assertTrue(isError, "exception should produce isError=true")
        assertTrue(result.contains("test crash"), "error message should contain exception text")
    }

    @Test
    fun `onToolStarted fires before onToolResult for each tool`() = runTest {
        val executor = AgentExecutor(mockDelegate, mockListener)
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(
                ToolCall("t1", "open_app", mapOf("app_name" to "Gmail")),
                ToolCall("t2", "tap", mapOf("element_id" to 5))
            ),
            stopReason = "tool_use"
        )

        executor.run(response, null, { false }) {
            ChatResponse(text = "Done", toolCalls = emptyList(), stopReason = "end_turn")
        }

        // onToolStarted should fire twice — once per tool in the batch
        assertEquals(2, mockListener.toolStarted.size, "should have 2 onToolStarted calls")
        assertEquals("open_app", mockListener.toolStarted[0].first)
        assertEquals(0, mockListener.toolStarted[0].second) // toolIndex
        assertEquals(2, mockListener.toolStarted[0].third)  // batchSize
        assertEquals("tap", mockListener.toolStarted[1].first)
        assertEquals(1, mockListener.toolStarted[1].second)
        assertEquals(2, mockListener.toolStarted[1].third)

        // onToolResult should also fire twice
        assertEquals(2, mockListener.toolResults.size)
    }

    @Test
    fun `onToolStarted fires before executeToolCall`() = runTest {
        val callOrder = mutableListOf<String>()

        // Track order: onToolStarted vs executeToolCall
        val orderTrackingListener = object : LoopProgressListener {
            override fun onToolStarted(toolName: String, toolIndex: Int, batchSize: Int) {
                callOrder.add("started:$toolName")
            }
            override fun onToolResult(toolName: String, result: String, visibility: OutputVisibility, isError: Boolean) {
                callOrder.add("result:$toolName")
            }
            override fun onAccessibilityLost() {}
        }

        val orderTrackingDelegate = object : ToolExecutionDelegate {
            override suspend fun executeToolCall(toolCall: ToolCall, screenContent: ScreenContent?): ToolResult {
                callOrder.add("execute:${toolCall.name}")
                return ToolResult("ok")
            }
            override suspend fun refreshScreenAfterTool(toolName: String, actionResult: String): ScreenContent? = null
            override suspend fun settleDelay(toolName: String, actionResult: String) {}
            override fun isUiMutatingTool(toolName: String) = false
            override fun formatToolResult(actionResult: String, screenContent: ScreenContent?): String = actionResult
            override fun addSteerMessage(text: String) {}
            override fun addToolResult(toolCallId: String, result: String, toolName: String?, isError: Boolean) {}
            override fun onStepStarted(step: Int, maxSteps: Int) {}
            override fun outputVerbosity() = OutputVerbosity.NORMAL
            override suspend fun refreshScreen(): ScreenContent? = null
            override fun isScreenReaderAvailable() = true
            override suspend fun waitForAccessibility(timeoutMs: Long) = true
            override fun accessibilityWaitMs() = 100L
        }

        val executor = AgentExecutor(orderTrackingDelegate, orderTrackingListener)
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "open_app", mapOf("app_name" to "Gmail"))),
            stopReason = "tool_use"
        )

        executor.run(response, null, { false }) {
            ChatResponse(text = "Done", toolCalls = emptyList(), stopReason = "end_turn")
        }

        assertEquals(listOf("started:open_app", "execute:open_app", "result:open_app"), callOrder)
    }

}
