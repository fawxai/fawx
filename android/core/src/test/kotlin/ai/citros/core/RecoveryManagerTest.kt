package ai.citros.core

import kotlinx.coroutines.test.runTest
import org.junit.Test
import kotlin.test.assertContains
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

class RecoveryManagerTest {

    @Test
    fun `detectFailure returns TOOL_ERROR for explicit tool errors`() {
        val failure = detectFailure(
            toolCall = ToolCall("t1", "tap", mapOf("element_id" to 1)),
            result = ToolResult("Failed", isError = true),
            screenBefore = ScreenFingerprint(100, "com.app"),
            screenAfter = ScreenFingerprint(100, "com.app"),
            consecutiveFailures = 1
        )

        assertNotNull(failure)
        assertEquals(FailureType.TOOL_ERROR, failure.failureType)
        assertEquals(2, failure.consecutiveFailures)
    }

    @Test
    fun `detectFailure returns NO_EFFECT when UI tool leaves same fingerprint`() {
        val failure = detectFailure(
            toolCall = ToolCall("t1", "tap", mapOf("element_id" to 1)),
            result = ToolResult("Tapped", isError = false),
            screenBefore = ScreenFingerprint(42, "com.app"),
            screenAfter = ScreenFingerprint(42, "com.app"),
            consecutiveFailures = 0
        )

        assertNotNull(failure)
        assertEquals(FailureType.NO_EFFECT, failure.failureType)
    }

    @Test
    fun `detectFailure returns UNEXPECTED_STATE when package changes unexpectedly`() {
        val failure = detectFailure(
            toolCall = ToolCall("t1", "tap", mapOf("element_id" to 1)),
            result = ToolResult("Tapped", isError = false),
            screenBefore = ScreenFingerprint(1, "com.a"),
            screenAfter = ScreenFingerprint(2, "com.b"),
            consecutiveFailures = 0
        )

        assertNotNull(failure)
        assertEquals(FailureType.UNEXPECTED_STATE, failure.failureType)
    }

    @Test
    fun `detectFailure returns null for expected package switch tool`() {
        val failure = detectFailure(
            toolCall = ToolCall("t1", "open_app", mapOf("app_name" to "Gmail")),
            result = ToolResult("Opened", isError = false),
            screenBefore = ScreenFingerprint(1, "com.a"),
            screenAfter = ScreenFingerprint(2, "com.b"),
            consecutiveFailures = 0
        )

        assertNull(failure)
    }

    @Test
    fun `RecoveryManager first applicable strategy wins`() {
        val manager = RecoveryManager(
            strategies = listOf(
                object : RecoveryStrategy {
                    override val name: String = "first"
                    override fun appliesTo(failure: ActionFailure): Boolean = true
                    override fun recover(failure: ActionFailure): List<RecoveryAction> =
                        listOf(RecoveryAction("a", "press_back", emptyMap()))
                },
                object : RecoveryStrategy {
                    override val name: String = "second"
                    override fun appliesTo(failure: ActionFailure): Boolean = true
                    override fun recover(failure: ActionFailure): List<RecoveryAction> =
                        listOf(RecoveryAction("b", "press_home", emptyMap()))
                }
            )
        )

        val guidance = manager.evaluateFailure(
            ActionFailure(
                toolCall = ToolCall("t1", "tap", emptyMap()),
                result = ToolResult("ok"),
                screenBefore = null,
                screenAfter = null,
                consecutiveFailures = 1,
                foregroundApp = null,
                failureType = FailureType.NO_EFFECT
            )
        )

        assertNotNull(guidance)
        assertContains(guidance, "RECOVERY (first)")
        assertTrue(!guidance.contains("second"))
    }

    @Test
    fun `AgentExecutor appends recovery guidance into tool result`() = runTest {
        val delegate = FakeToolExecutionDelegate()
        val listener = FakeLoopProgressListener()
        val sameScreen = ScreenContent(packageName = "com.app", elements = emptyList())
        delegate.refreshAfterToolResult = sameScreen
        delegate.executeResult = ToolResult("Tapped element 1")

        val executor = AgentExecutor(delegate, listener)
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(ToolCall("t1", "tap", mapOf("element_id" to 1))),
            stopReason = "tool_use"
        )

        executor.run(response, sameScreen, { false }) {
            ChatResponse(text = "Done", toolCalls = emptyList(), stopReason = "end_turn")
        }

        val result = delegate.toolResults.first().second
        assertContains(result, "⚠️ RECOVERY")
        assertContains(result, "Suggested:")
    }

    @Test
    fun `AgentExecutor resets recovery consecutive failures after successful tool`() = runTest {
        val delegate = FakeToolExecutionDelegate()
        val listener = FakeLoopProgressListener()
        val sameScreen = ScreenContent(packageName = "com.app", elements = emptyList())
        delegate.refreshAfterToolResult = sameScreen
        delegate.onExecute = { toolCall, _ ->
            when (toolCall.name) {
                "tap" -> ToolResult("tap attempted")
                "read_screen" -> ToolResult("screen read")
                else -> ToolResult("ok")
            }
        }

        val escalationOnlyManager = RecoveryManager(
            strategies = listOf(
                object : RecoveryStrategy {
                    override val name: String = "after_two_failures"
                    override fun appliesTo(failure: ActionFailure): Boolean = failure.consecutiveFailures >= 2
                    override fun recover(failure: ActionFailure): List<RecoveryAction> =
                        listOf(RecoveryAction("Escalate", "press_home", emptyMap()))
                }
            )
        )

        val executor = AgentExecutor(delegate, listener, recoveryManager = escalationOnlyManager)
        val response = ChatResponse(
            text = null,
            toolCalls = listOf(
                ToolCall("t1", "tap", mapOf("element_id" to 1)),
                ToolCall("t2", "read_screen", emptyMap()),
                ToolCall("t3", "tap", mapOf("element_id" to 2))
            ),
            stopReason = "tool_use"
        )

        executor.run(response, sameScreen, { false }) {
            ChatResponse(text = "Done", toolCalls = emptyList(), stopReason = "end_turn")
        }

        // If success did not reset the streak, t3 would be failure #2 and include recovery guidance.
        assertTrue(delegate.toolResults.none { (_, result) -> result.contains("⚠️ RECOVERY") })
    }
}
