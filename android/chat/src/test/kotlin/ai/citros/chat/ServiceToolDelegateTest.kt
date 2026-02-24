package ai.citros.chat

import ai.citros.core.OutputVerbosity
import ai.citros.core.PhoneAgentApi
import ai.citros.core.ScreenContent
import ai.citros.core.ToolCall
import ai.citros.core.ToolResult
import kotlinx.coroutines.test.currentTime
import kotlinx.coroutines.test.runTest
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.mockito.Mockito.mock
import org.mockito.Mockito.`when`
import org.mockito.Mockito.verify
import org.robolectric.RobolectricTestRunner
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/**
 * Unit tests for [ServiceToolDelegate].
 *
 * Tests that the delegate correctly wraps PhoneAgentApi and ScreenReader
 * without any Activity/ViewModel dependency.
 */
@RunWith(RobolectricTestRunner::class)
class ServiceToolDelegateTest {

    private lateinit var mockApi: PhoneAgentApi
    private lateinit var delegate: ServiceToolDelegate

    @Before
    fun setUp() {
        InterruptionDetector.stopMonitoring()
        mockApi = mock(PhoneAgentApi::class.java)
        delegate = ServiceToolDelegate(mockApi, OutputVerbosity.NORMAL)
    }

    @Test
    fun `executeToolCall delegates to PhoneAgentApi`() = runTest {
        val toolCall = ToolCall(id = "tc-1", name = "open_app", input = mapOf("app_name" to "Settings"))
        val expectedResult = ToolResult("Opened Settings", isError = false)
        `when`(mockApi.executeToolCall(toolCall, null)).thenReturn(expectedResult)

        val result = delegate.executeToolCall(toolCall, null)

        assertEquals("Opened Settings", result.text)
        assertFalse(result.isError)
        verify(mockApi).executeToolCall(toolCall, null)
    }

    @Test
    fun `executeToolCall returns error on exception`() = runTest {
        val toolCall = ToolCall(id = "tc-1", name = "tap", input = mapOf("x" to 100, "y" to 200))
        `when`(mockApi.executeToolCall(toolCall, null)).thenThrow(RuntimeException("Connection lost"))

        val result = delegate.executeToolCall(toolCall, null)

        assertTrue(result.isError)
        assertTrue(result.text.contains("Connection lost"))
    }

    @Test
    fun `executeToolCall marks and clears interruption guard for ui mutating tools`() = runTest {
        val toolCall = ToolCall(id = "tc-2", name = "tap", input = mapOf("element_id" to 1))
        `when`(mockApi.executeToolCall(toolCall, null)).thenAnswer {
            assertTrue(isAgentActionInProgress())
            ToolResult("ok", isError = false)
        }

        delegate.executeToolCall(toolCall, null)

        assertFalse(isAgentActionInProgress())
    }

    @Test
    fun `executeToolCall clears interruption guard when ui mutating tool throws`() = runTest {
        val toolCall = ToolCall(id = "tc-3", name = "tap", input = mapOf("element_id" to 1))
        `when`(mockApi.executeToolCall(toolCall, null)).thenAnswer {
            assertTrue(isAgentActionInProgress())
            throw RuntimeException("boom")
        }

        val result = delegate.executeToolCall(toolCall, null)

        assertTrue(result.isError)
        assertFalse(isAgentActionInProgress())
    }

    @Test
    fun `executeToolCall does not mark interruption guard for non mutating tools`() = runTest {
        val toolCall = ToolCall(id = "tc-4", name = "read_screen", input = emptyMap<String, Any>())
        `when`(mockApi.executeToolCall(toolCall, null)).thenAnswer {
            assertFalse(isAgentActionInProgress())
            ToolResult("screen", isError = false)
        }

        delegate.executeToolCall(toolCall, null)

        assertFalse(isAgentActionInProgress())
    }

    @Test
    fun `isUiMutatingTool checks PhoneAgentApi constants`() {
        assertTrue(delegate.isUiMutatingTool("tap"))
        assertTrue(delegate.isUiMutatingTool("type_text"))
        assertTrue(delegate.isUiMutatingTool("swipe"))
        assertFalse(delegate.isUiMutatingTool("read_screen"))
        assertFalse(delegate.isUiMutatingTool("think"))
    }

    @Test
    fun `addToolResult delegates to PhoneAgentApi`() {
        delegate.addToolResult("tc-1", "result text", "tap", false)
        verify(mockApi).addToolResult("tc-1", "result text", "tap", false)
    }

    @Test
    fun `addSteerMessage delegates to PhoneAgentApi`() {
        delegate.addSteerMessage("try something else")
        verify(mockApi).addSteerMessage("try something else")
    }

    @Test
    fun `outputVerbosity returns configured value`() {
        assertEquals(OutputVerbosity.NORMAL, delegate.outputVerbosity())
    }

    @Test
    fun `formatToolResult delegates to PhoneAgentApi`() {
        `when`(mockApi.formatToolResult("tapped Send", null)).thenReturn("Action: tapped Send")
        val result = delegate.formatToolResult("tapped Send", null)
        assertEquals("Action: tapped Send", result)
    }

    @Test
    fun `onStepStarted sets currentToolStep on api`() {
        delegate.onStepStarted(3, 10)
        verify(mockApi).currentToolStep = 3
    }

    @Test
    fun `accessibilityWaitMs returns 5000`() {
        assertEquals(5000L, delegate.accessibilityWaitMs())
    }

    @Test
    fun `settleDelay skips delay for smart poll and wait semantics`() = runTest {
        val start = currentTime
        delegate.settleDelay("open_app", "Opened")
        delegate.settleDelay("press_home", "Done")
        delegate.settleDelay("press_back", "Done")
        delegate.settleDelay("wait", "Waited")
        delegate.settleDelay("think", "Thought")
        assertEquals(start, currentTime)
    }

    @Test
    fun `settleDelay uses tap and default delays aligned with ChatViewModel`() = runTest {
        val start = currentTime
        delegate.settleDelay("tap", "Tapped")
        assertEquals(start + ServiceToolDelegate.DELAY_AFTER_TAP_MS, currentTime)

        delegate.settleDelay("type_text", "Typed")
        assertEquals(
            start + ServiceToolDelegate.DELAY_AFTER_TAP_MS + ServiceToolDelegate.DELAY_DEFAULT_MS,
            currentTime
        )
    }

    private fun isAgentActionInProgress(): Boolean {
        val field = InterruptionDetector::class.java.getDeclaredField("agentActionInProgress")
        field.isAccessible = true
        val atomic = field.get(InterruptionDetector) as java.util.concurrent.atomic.AtomicBoolean
        return atomic.get()
    }
}
