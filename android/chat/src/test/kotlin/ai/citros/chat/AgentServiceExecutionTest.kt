package ai.citros.chat

import ai.citros.core.AgentState
import ai.citros.core.ChatResponse
import ai.citros.core.PhoneAgentApi
import ai.citros.core.ToolCall
import ai.citros.core.ToolResult
import android.content.Intent
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.After
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.mockito.ArgumentMatchers.anyString
import org.mockito.Mockito.doAnswer
import org.mockito.Mockito.doThrow
import org.mockito.Mockito.mock
import org.mockito.Mockito.times
import org.mockito.Mockito.verify
import org.mockito.Mockito.`when`
import java.util.concurrent.atomic.AtomicBoolean
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.android.controller.ServiceController
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertNull
import kotlin.test.assertTrue

@RunWith(RobolectricTestRunner::class)
class AgentServiceExecutionTest {

    private lateinit var controller: ServiceController<AgentService>
    private lateinit var service: AgentService

    @Before
    fun setUp() {
        InterruptionDetector.stopMonitoring()
        controller = Robolectric.buildService(AgentService::class.java)
        service = controller.get()
        controller.create()
    }

    @After
    fun tearDown() {
        InterruptionDetector.stopMonitoring()
        try {
            controller.destroy()
        } catch (_: Exception) {}
    }

    @Test
    fun `START_TASK without PhoneAgentApi fails gracefully`() {
        val intent = AgentService.startTaskIntent(service, "Open Settings")
        service.onStartCommand(intent, 0, 1)

        val state = service.agentState.value
        assertIs<AgentState.Failed>(state)
        assert(state.error.contains("not configured"))
    }

    @Test
    fun `binder configureExecution sets PhoneAgentApi`() {
        val binder = service.onBind(Intent()) as AgentService.AgentBinder
        val mockApi = mock(PhoneAgentApi::class.java)

        binder.configureExecution(api = mockApi)

        val intent = AgentService.startTaskIntent(service, "Open Settings")
        service.onStartCommand(intent, 0, 1)

        val state = service.agentState.value
        assertIs<AgentState.Thinking>(state)
    }

    @Test
    fun `binder clearCallbacks nulls steer and progress`() {
        val binder = service.onBind(Intent()) as AgentService.AgentBinder
        val mockApi = mock(PhoneAgentApi::class.java)
        val mockCallback = mock(ServiceProgressCallback::class.java)

        binder.configureExecution(
            api = mockApi,
            steerSource = { emptyList() },
            progress = mockCallback
        )

        binder.clearCallbacks()

        val steerField = AgentService::class.java.getDeclaredField("steerMessageSource").apply { isAccessible = true }
        val progressField = AgentService::class.java.getDeclaredField("progressCallback").apply { isAccessible = true }

        assertNull(steerField.get(service))
        assertNull(progressField.get(service))
    }

    @OptIn(ExperimentalCoroutinesApi::class)
    @Test
    fun `cancel during execution sets serviceCancelFlag`() = runTest {
        val testDispatcher = StandardTestDispatcher(testScheduler)
        service.dispatcher = testDispatcher

        val binder = service.onBind(Intent()) as AgentService.AgentBinder
        val mockApi = mock(PhoneAgentApi::class.java)
        binder.configureExecution(api = mockApi)

        val startIntent = AgentService.startTaskIntent(service, "test")
        service.onStartCommand(startIntent, 0, 1)

        val cancelIntent = AgentService.cancelIntent(service)
        service.onStartCommand(cancelIntent, 0, 2)

        assertIs<AgentState.Idle>(service.agentState.value)
    }

    @OptIn(ExperimentalCoroutinesApi::class)
    @Test
    fun `execution happy path stores conversation and completes`() = runTest {
        val testDispatcher = StandardTestDispatcher(testScheduler)
        service.dispatcher = testDispatcher

        val binder = service.onBind(Intent()) as AgentService.AgentBinder
        val mockApi = mock(PhoneAgentApi::class.java)
        `when`(mockApi.sendMessage("hello", null, false)).thenReturn(
            ChatResponse(text = "Hi there", toolCalls = emptyList(), stopReason = "end_turn")
        )
        binder.configureExecution(api = mockApi)

        service.onStartCommand(AgentService.startTaskIntent(service, "hello"), 0, 1)
        advanceUntilIdle()

        val state = service.agentState.value
        assertIs<AgentState.Complete>(state)
        assertEquals("Hi there", state.result)
        assertTrue(service.conversationMessages.value.any { it.role == "user" && it.content == "hello" })
        assertTrue(service.conversationMessages.value.any { it.role == "assistant" && it.content == "Hi there" })
        verify(mockApi).seedConversationHistory(service.conversationMessages.value.dropLast(1))
    }

    @OptIn(ExperimentalCoroutinesApi::class)
    @Test
    fun `tool loop path executes via AgentExecutor and completes`() = runTest {
        val testDispatcher = StandardTestDispatcher(testScheduler)
        service.dispatcher = testDispatcher

        val toolCall = ToolCall(id = "tool-1", name = "open_app", input = mapOf("app_name" to "Gmail"))
        val binder = service.onBind(Intent()) as AgentService.AgentBinder
        val mockApi = mock(PhoneAgentApi::class.java)
        val callback = mock(ServiceProgressCallback::class.java)

        `when`(mockApi.sendMessage("open gmail", null, false)).thenReturn(
            ChatResponse(text = null, toolCalls = listOf(toolCall), stopReason = "tool_use")
        )
        `when`(mockApi.executeToolCall(toolCall, null)).thenReturn(ToolResult(text = "Opened Gmail", isError = false))
        `when`(mockApi.formatToolResult("Opened Gmail", null)).thenReturn("Opened Gmail")
        `when`(mockApi.continueAfterTools()).thenReturn(
            ChatResponse(text = "Done", toolCalls = emptyList(), stopReason = "end_turn")
        )

        binder.configureExecution(api = mockApi, progress = callback)

        service.onStartCommand(AgentService.startTaskIntent(service, "open gmail"), 0, 1)
        advanceUntilIdle()

        val state = service.agentState.value
        assertIs<AgentState.Complete>(state)
        assertEquals("Done", state.result)

        verify(mockApi).executeToolCall(toolCall, null)
        verify(mockApi).addToolResult("tool-1", "Opened Gmail", "open_app", false)
        verify(mockApi).continueAfterTools()
        verify(callback).onExecutionComplete(1)
        verify(callback).onToolStatus("open_app")
        verify(callback).onToolStatus(null)

        val messages = service.conversationMessages.value
        assertTrue(messages.any { it.role == "assistant" && it.content.contains("🤖 Opened Gmail") })
        assertTrue(messages.any { it.role == "assistant" && it.content == "Done" })
    }

    @OptIn(ExperimentalCoroutinesApi::class)
    @Test
    fun `tool loop starts interruption monitoring and stops it after success`() = runTest {
        val testDispatcher = StandardTestDispatcher(testScheduler)
        service.dispatcher = testDispatcher

        val toolCall = ToolCall(id = "tool-1", name = "open_app", input = mapOf("app_name" to "Gmail"))
        val binder = service.onBind(Intent()) as AgentService.AgentBinder
        val mockApi = mock(PhoneAgentApi::class.java)
        val callback = mock(ServiceProgressCallback::class.java)
        val monitoringObservedDuringExecution = AtomicBoolean(false)

        `when`(mockApi.sendMessage("open gmail", null, false)).thenReturn(
            ChatResponse(text = null, toolCalls = listOf(toolCall), stopReason = "tool_use")
        )
        `when`(mockApi.executeToolCall(toolCall, null)).thenReturn(ToolResult(text = "Opened Gmail", isError = false))
        `when`(mockApi.formatToolResult("Opened Gmail", null)).thenReturn("Opened Gmail")
        `when`(mockApi.continueAfterTools()).thenReturn(
            ChatResponse(text = "Done", toolCalls = emptyList(), stopReason = "end_turn")
        )
        doAnswer {
            monitoringObservedDuringExecution.set(isInterruptionMonitoringActive())
            null
        }.`when`(callback).onToolStatus("open_app")

        binder.configureExecution(api = mockApi, progress = callback)

        service.onStartCommand(AgentService.startTaskIntent(service, "open gmail"), 0, 1)
        advanceUntilIdle()

        assertTrue(monitoringObservedDuringExecution.get())
        assertFalse(isInterruptionMonitoringActive())
        assertNull(currentExpectedInterruptionPackage())
    }

    @OptIn(ExperimentalCoroutinesApi::class)
    @Test
    fun `tool loop error still stops interruption monitoring in finally`() = runTest {
        val testDispatcher = StandardTestDispatcher(testScheduler)
        service.dispatcher = testDispatcher

        val toolCall = ToolCall(id = "tool-1", name = "open_app", input = mapOf("app_name" to "Gmail"))
        val binder = service.onBind(Intent()) as AgentService.AgentBinder
        val mockApi = mock(PhoneAgentApi::class.java)
        val callback = mock(ServiceProgressCallback::class.java)
        val monitoringObservedDuringExecution = AtomicBoolean(false)

        `when`(mockApi.sendMessage("open gmail", null, false)).thenReturn(
            ChatResponse(text = null, toolCalls = listOf(toolCall), stopReason = "tool_use")
        )
        `when`(mockApi.executeToolCall(toolCall, null)).thenReturn(ToolResult(text = "Opened Gmail", isError = false))
        `when`(mockApi.formatToolResult("Opened Gmail", null)).thenReturn("Opened Gmail")
        `when`(mockApi.continueAfterTools()).thenThrow(RuntimeException("loop boom"))
        doAnswer {
            monitoringObservedDuringExecution.set(isInterruptionMonitoringActive())
            null
        }.`when`(callback).onToolStatus("open_app")

        binder.configureExecution(api = mockApi, progress = callback)

        service.onStartCommand(AgentService.startTaskIntent(service, "open gmail"), 0, 1)
        advanceUntilIdle()

        assertIs<AgentState.Failed>(service.agentState.value)
        assertTrue(monitoringObservedDuringExecution.get())
        assertFalse(isInterruptionMonitoringActive())
        assertNull(currentExpectedInterruptionPackage())
    }

    @OptIn(ExperimentalCoroutinesApi::class)
    @Test
    fun `tool loop progress callback failure is isolated`() = runTest {
        val testDispatcher = StandardTestDispatcher(testScheduler)
        service.dispatcher = testDispatcher

        val toolCall = ToolCall(id = "tool-1", name = "open_app", input = mapOf("app_name" to "Gmail"))
        val binder = service.onBind(Intent()) as AgentService.AgentBinder
        val mockApi = mock(PhoneAgentApi::class.java)
        val callback = mock(ServiceProgressCallback::class.java)

        `when`(mockApi.sendMessage("open gmail", null, false)).thenReturn(
            ChatResponse(text = null, toolCalls = listOf(toolCall), stopReason = "tool_use")
        )
        `when`(mockApi.executeToolCall(toolCall, null)).thenReturn(ToolResult(text = "Opened Gmail", isError = false))
        `when`(mockApi.formatToolResult("Opened Gmail", null)).thenReturn("Opened Gmail")
        `when`(mockApi.continueAfterTools()).thenReturn(
            ChatResponse(text = "Done", toolCalls = emptyList(), stopReason = "end_turn")
        )
        doThrow(RuntimeException("callback boom")).`when`(callback).onToolStatus("open_app")

        binder.configureExecution(api = mockApi, progress = callback)

        service.onStartCommand(AgentService.startTaskIntent(service, "open gmail"), 0, 1)
        advanceUntilIdle()

        val state = service.agentState.value
        assertIs<AgentState.Complete>(state)
        assertEquals("Done", state.result)
        verify(mockApi).continueAfterTools()
    }

    @OptIn(ExperimentalCoroutinesApi::class)
    @Test
    fun `sendMessageWithFallback retries once after first failure`() = runTest {
        val testDispatcher = StandardTestDispatcher(testScheduler)
        service.dispatcher = testDispatcher

        val binder = service.onBind(Intent()) as AgentService.AgentBinder
        val mockApi = mock(PhoneAgentApi::class.java)
        `when`(mockApi.sendMessage("hello", null, false))
            .thenThrow(RuntimeException("temporary"))
            .thenReturn(ChatResponse(text = "Recovered", toolCalls = emptyList(), stopReason = "end_turn"))
        binder.configureExecution(api = mockApi)

        service.onStartCommand(AgentService.startTaskIntent(service, "hello"), 0, 1)
        advanceUntilIdle()

        val state = service.agentState.value
        assertIs<AgentState.Complete>(state)
        assertEquals("Recovered", state.result)
        verify(mockApi, times(2)).sendMessage("hello", null, false)
    }

    @OptIn(ExperimentalCoroutinesApi::class)
    @Test
    fun `safeProgressCallback isolates callback exceptions`() = runTest {
        val testDispatcher = StandardTestDispatcher(testScheduler)
        service.dispatcher = testDispatcher

        val binder = service.onBind(Intent()) as AgentService.AgentBinder
        val mockApi = mock(PhoneAgentApi::class.java)
        val callback = mock(ServiceProgressCallback::class.java)
        `when`(mockApi.sendMessage("hello", null, false)).thenReturn(
            ChatResponse(text = "Hi there", toolCalls = emptyList(), stopReason = "end_turn")
        )
        doThrow(RuntimeException("callback boom")).`when`(callback).onAssistantMessage(anyString())
        binder.configureExecution(api = mockApi, progress = callback)

        service.onStartCommand(AgentService.startTaskIntent(service, "hello"), 0, 1)
        advanceUntilIdle()

        val state = service.agentState.value
        assertIs<AgentState.Complete>(state)
        assertEquals("Hi there", state.result)
    }

    @Test
    fun `explanationPromptForExit maps known exit reasons`() {
        val method = AgentService::class.java.getDeclaredMethod("explanationPromptForExit", String::class.java)
        method.isAccessible = true

        val maxSteps = method.invoke(service, "max_steps") as String?
        val accessibilityLost = method.invoke(service, "accessibility_lost") as String?
        val unknown = method.invoke(service, "other") as String?

        assertTrue(maxSteps?.contains("step limit") == true)
        assertTrue(accessibilityLost?.contains("lost connection") == true)
        assertNull(unknown)
    }

    private fun isInterruptionMonitoringActive(): Boolean {
        val field = InterruptionDetector::class.java.getDeclaredField("monitoring")
        field.isAccessible = true
        val ref = field.get(InterruptionDetector) as java.util.concurrent.atomic.AtomicBoolean
        return ref.get()
    }

    private fun currentExpectedInterruptionPackage(): String? {
        val field = InterruptionDetector::class.java.getDeclaredField("expectedPackage")
        field.isAccessible = true
        return field.get(InterruptionDetector) as String?
    }
}
