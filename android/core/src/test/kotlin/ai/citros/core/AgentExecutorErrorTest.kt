package ai.citros.core

import kotlinx.coroutines.test.runTest
import org.junit.Before
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * Tests for Phase 2 error visibility: failure counting, retry context,
 * and onToolError callback in AgentExecutor.
 */
class AgentExecutorErrorTest {

    private lateinit var mockDelegate: FakeToolExecutionDelegate
    private lateinit var mockListener: FakeLoopProgressListener

    @Before
    fun setup() {
        mockDelegate = FakeToolExecutionDelegate()
        mockListener = FakeLoopProgressListener()
    }

    // ====== Failure counter tests ======

    @Test
    fun `failure counter increments on consecutive errors for same tool`() = runTest {
        var callCount = 0
        mockDelegate.onExecute = { _, _ ->
            callCount++
            ToolResult("element not found", isError = true)
        }
        val executor = AgentExecutor(mockDelegate, mockListener, maxToolSteps = 3)
        val toolResponse = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "tap", emptyMap())),
            stopReason = "tool_use"
        )

        executor.run(toolResponse, null, { false }) { toolResponse }

        // 3 consecutive errors on "tap" → onToolError called 3 times
        assertEquals(3, mockListener.toolErrors.size)
        // First call: count=1 → EXPLORATORY (below escalation threshold)
        assertEquals(ErrorSeverity.EXPLORATORY, mockListener.toolErrors[0].third)
        // Second call: count=2 → TRANSIENT (escalateToTransientAt=2)
        assertEquals(ErrorSeverity.TRANSIENT, mockListener.toolErrors[1].third)
        // Third call: count=3 → PERSISTENT (escalateToPersistentAt=3)
        assertEquals(ErrorSeverity.PERSISTENT, mockListener.toolErrors[2].third)
    }

    @Test
    fun `failure counter resets on success`() = runTest {
        var callCount = 0
        mockDelegate.onExecute = { _, _ ->
            callCount++
            when (callCount) {
                1 -> ToolResult("element not found", isError = true)
                2 -> ToolResult("OK", isError = false) // success resets counter
                3 -> ToolResult("element not found", isError = true)
                else -> ToolResult("OK", isError = false)
            }
        }
        val executor = AgentExecutor(mockDelegate, mockListener, maxToolSteps = 4)
        val toolResponse = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "tap", emptyMap())),
            stopReason = "tool_use"
        )

        executor.run(toolResponse, null, { false }) { toolResponse }

        // Two errors total, each with count=1 (reset between them)
        assertEquals(2, mockListener.toolErrors.size)
        assertEquals(ErrorSeverity.EXPLORATORY, mockListener.toolErrors[0].third)
        assertEquals(ErrorSeverity.EXPLORATORY, mockListener.toolErrors[1].third)
    }

    @Test
    fun `failure counter resets at start of new run`() = runTest {
        var callCount = 0
        mockDelegate.onExecute = { _, _ ->
            callCount++
            ToolResult("element not found", isError = true)
        }
        val executor = AgentExecutor(mockDelegate, mockListener, maxToolSteps = 1)
        val toolResponse = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "tap", emptyMap())),
            stopReason = "tool_use"
        )

        // First run — 1 error
        executor.run(toolResponse, null, { false }) { toolResponse }
        assertEquals(1, mockListener.toolErrors.size)
        assertEquals(ErrorSeverity.EXPLORATORY, mockListener.toolErrors[0].third)

        // Second run — counter should be reset, so count=1 again (not 2)
        mockListener.toolErrors.clear()
        executor.run(toolResponse, null, { false }) { toolResponse }
        assertEquals(1, mockListener.toolErrors.size)
        assertEquals(ErrorSeverity.EXPLORATORY, mockListener.toolErrors[0].third)
    }

    @Test
    fun `pre-classified severity on ToolResult is passed through to onToolError`() = runTest {
        mockDelegate.onExecute = { _, _ ->
            ToolResult("something happened", isError = true, severity = ErrorSeverity.PERSISTENT)
        }
        val executor = AgentExecutor(mockDelegate, mockListener, maxToolSteps = 1)
        val toolResponse = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "tap", emptyMap())),
            stopReason = "tool_use"
        )

        executor.run(toolResponse, null, { false }) { toolResponse }

        assertEquals(1, mockListener.toolErrors.size)
        assertEquals("tap", mockListener.toolErrors[0].first)
        assertEquals("something happened", mockListener.toolErrors[0].second)
        assertEquals(ErrorSeverity.PERSISTENT, mockListener.toolErrors[0].third)
    }

    @Test
    fun `onToolError not called for successful tool results`() = runTest {
        mockDelegate.executeResult = ToolResult("Success", isError = false)
        val executor = AgentExecutor(mockDelegate, mockListener, maxToolSteps = 1)
        val toolResponse = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "tap", emptyMap())),
            stopReason = "tool_use"
        )

        executor.run(toolResponse, null, { false }) { toolResponse }

        assertTrue(mockListener.toolErrors.isEmpty())
    }

    @Test
    fun `different tools have independent failure counters`() = runTest {
        mockDelegate.onExecute = { _, _ ->
            ToolResult("element not found", isError = true)
        }
        val executor = AgentExecutor(mockDelegate, mockListener, maxToolSteps = 1)
        // Batch with two different tools
        val toolResponse = ChatResponse(
            text = null,
            toolCalls = listOf(
                ToolCall("t1", "tap", emptyMap()),
                ToolCall("t2", "swipe", emptyMap())
            ),
            stopReason = "tool_use"
        )

        executor.run(toolResponse, null, { false }) { toolResponse }

        // Both errors should be EXPLORATORY (count=1 each, independent counters)
        assertEquals(2, mockListener.toolErrors.size)
        assertEquals("tap", mockListener.toolErrors[0].first)
        assertEquals(ErrorSeverity.EXPLORATORY, mockListener.toolErrors[0].third)
        assertEquals("swipe", mockListener.toolErrors[1].first)
        assertEquals(ErrorSeverity.EXPLORATORY, mockListener.toolErrors[1].third)
    }

    @Test
    fun `onToolError called before onToolResult`() = runTest {
        val callOrder = mutableListOf<String>()
        val listener = object : LoopProgressListener {
            override fun onToolStarted(toolName: String, toolIndex: Int, batchSize: Int) {}
            override fun onToolResult(toolName: String, result: String, visibility: OutputVisibility, isError: Boolean) {
                callOrder.add("onToolResult")
            }
            override fun onToolError(toolName: String, errorText: String, severity: ErrorSeverity) {
                callOrder.add("onToolError")
            }
            override fun onAccessibilityLost() {}
        }
        mockDelegate.executeResult = ToolResult("element not found", isError = true)
        val executor = AgentExecutor(mockDelegate, listener, maxToolSteps = 1)
        val toolResponse = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "tap", emptyMap())),
            stopReason = "tool_use"
        )

        executor.run(toolResponse, null, { false }) { toolResponse }

        assertEquals(listOf("onToolError", "onToolResult"), callOrder)
    }

    @Test
    fun `auto-classified severity uses retryContext for escalation`() = runTest {
        // No pre-classified severity — relies on OutputClassifier with retryContext
        var callCount = 0
        mockDelegate.onExecute = { _, _ ->
            callCount++
            ToolResult("timeout connecting", isError = true) // timeout → TRANSIENT base
        }
        val executor = AgentExecutor(mockDelegate, mockListener, maxToolSteps = 3)
        val toolResponse = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "web_search", emptyMap())),
            stopReason = "tool_use"
        )

        executor.run(toolResponse, null, { false }) { toolResponse }

        assertEquals(3, mockListener.toolErrors.size)
        // OutputClassifier: "timeout" base is TRANSIENT, escalates to PERSISTENT
        // at consecutiveFailures >= RetryContext.escalateToPersistentAt (default 3)
        assertEquals(ErrorSeverity.TRANSIENT, mockListener.toolErrors[0].third)  // count=1
        assertEquals(ErrorSeverity.TRANSIENT, mockListener.toolErrors[1].third)  // count=2
        assertEquals(ErrorSeverity.PERSISTENT, mockListener.toolErrors[2].third) // count=3 → escalates
    }
}
