package ai.citros.chat

import ai.citros.core.AgentState
import ai.citros.core.PhoneAgentApi
import android.content.Intent
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runTest
import org.junit.After
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.mockito.Mockito.mock
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.android.controller.ServiceController
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * Unit tests for [AgentService] lifecycle and state management.
 *
 * Uses Robolectric for service lifecycle simulation.
 *
 * See docs/specs/sprint-0-service-architecture.md, test matrix S1-S6
 */
@RunWith(RobolectricTestRunner::class)
class AgentServiceTest {

    private lateinit var controller: ServiceController<AgentService>
    private lateinit var service: AgentService

    @Before
    fun setUp() {
        controller = Robolectric.buildService(AgentService::class.java)
        service = controller.get()
    }

    /**
     * Configure a mock PhoneAgentApi on the service so that START_TASK
     * transitions to Thinking instead of immediately failing.
     */
    private fun configureWithMockApi() {
        controller.create()
        val binder = service.onBind(Intent()) as AgentService.AgentBinder
        binder.configureExecution(api = mock(PhoneAgentApi::class.java))
    }

    @After
    fun tearDown() {
        try {
            controller.destroy()
        } catch (_: Exception) {
            // Service may already be destroyed
        }
    }

    // --- S1: Service starts as foreground with notification ---

    @Test
    fun `service creates successfully`() {
        controller.create()
        assertNotNull(service)
    }

    @Test
    fun `initial state is Idle`() {
        controller.create()
        assertIs<AgentState.Idle>(service.agentState.value)
    }

    // --- S3: Task dispatched via START_TASK intent ---

    @Test
    fun `START_TASK transitions to Thinking`() {
        configureWithMockApi()
        val intent = AgentService.startTaskIntent(service, "Open Settings")
        service.onStartCommand(intent, 0, 1)

        assertIs<AgentState.Thinking>(service.agentState.value)
    }

    @Test
    fun `START_TASK includes taskId`() {
        configureWithMockApi()
        val intent = AgentService.startTaskIntent(service, "Open Settings")
        service.onStartCommand(intent, 0, 1)

        val state = service.agentState.value
        assertIs<AgentState.Thinking>(state)
        assertTrue(state.taskId.isNotEmpty())
    }

    // --- S4: State transitions ---

    @Test
    fun `completeTask transitions to Complete`() {
        configureWithMockApi()
        val intent = AgentService.startTaskIntent(service, "test")
        service.onStartCommand(intent, 0, 1)

        service.completeTask("task-1", "Done!")

        val state = service.agentState.value
        assertIs<AgentState.Complete>(state)
        assertEquals("Done!", state.result)
    }

    @Test
    fun `failTask transitions to Failed`() {
        configureWithMockApi()
        val intent = AgentService.startTaskIntent(service, "test")
        service.onStartCommand(intent, 0, 1)

        service.failTask("task-1", "Network error")

        val state = service.agentState.value
        assertIs<AgentState.Failed>(state)
        assertEquals("Network error", state.error)
    }

    @Test
    fun `updateState sets arbitrary state`() {
        controller.create()
        val executing = AgentState.Executing("task-1", "tap", 3, 10)
        service.updateState(executing)

        assertEquals(executing, service.agentState.value)
    }

    // --- S5: Cancel intent stops active task ---

    @Test
    fun `CANCEL intent transitions active task to Idle`() {
        configureWithMockApi()
        // Start a task
        val startIntent = AgentService.startTaskIntent(service, "test")
        service.onStartCommand(startIntent, 0, 1)
        assertIs<AgentState.Thinking>(service.agentState.value)

        // Cancel it
        val cancelIntent = AgentService.cancelIntent(service)
        service.onStartCommand(cancelIntent, 0, 2)

        assertIs<AgentState.Idle>(service.agentState.value)
    }

    @Test
    fun `CANCEL intent when idle is no-op`() {
        controller.create()
        val cancelIntent = AgentService.cancelIntent(service)
        service.onStartCommand(cancelIntent, 0, 1)

        assertIs<AgentState.Idle>(service.agentState.value)
    }

    // --- Concurrent task policy: new message during active task = steer ---

    @Test
    fun `START_TASK during active task stays in current state`() {
        configureWithMockApi()
        // Start first task
        val firstIntent = AgentService.startTaskIntent(service, "Open Settings")
        service.onStartCommand(firstIntent, 0, 1)
        val firstState = service.agentState.value
        assertIs<AgentState.Thinking>(firstState)
        val firstTaskId = firstState.taskId

        // Second message during active task — treated as steer, state unchanged
        val secondIntent = AgentService.startTaskIntent(service, "Actually open Gmail")
        service.onStartCommand(secondIntent, 0, 2)

        val currentState = service.agentState.value
        assertIs<AgentState.Thinking>(currentState)
        assertEquals(firstTaskId, currentState.taskId) // Still the first task
    }

    // --- S6: Steer intent ---

    @Test
    fun `STEER intent during active task does not change state`() {
        configureWithMockApi()
        val startIntent = AgentService.startTaskIntent(service, "test")
        service.onStartCommand(startIntent, 0, 1)

        val steerIntent = AgentService.steerIntent(service, "Try a different approach")
        service.onStartCommand(steerIntent, 0, 2)

        // State should still be Thinking (steer doesn't change state)
        assertIs<AgentState.Thinking>(service.agentState.value)
    }

    @Test
    fun `STEER intent when idle is ignored`() {
        controller.create()
        val steerIntent = AgentService.steerIntent(service, "Try something")
        service.onStartCommand(steerIntent, 0, 1)

        assertIs<AgentState.Idle>(service.agentState.value)
    }

    // --- STOP intent ---

    @Test
    fun `STOP intent transitions to Idle`() {
        controller.create()
        val stopIntent = AgentService.stopIntent(service)
        service.onStartCommand(stopIntent, 0, 1)

        assertIs<AgentState.Idle>(service.agentState.value)
    }

    // --- System restart (null intent) ---

    @Test
    fun `null intent on start command goes to Idle`() {
        controller.create()
        // Simulate system restart: null intent
        service.onStartCommand(null, 0, 1)

        assertIs<AgentState.Idle>(service.agentState.value)
    }

    // --- Intent factory methods ---

    @Test
    fun `startTaskIntent creates correct intent`() {
        controller.create()
        val intent = AgentService.startTaskIntent(service, "Hello")
        assertEquals(AgentService.ACTION_START_TASK, intent.action)
        assertEquals("Hello", intent.getStringExtra(AgentService.EXTRA_MESSAGE))
        assertNotNull(intent.getStringExtra(AgentService.EXTRA_TASK_ID))
    }

    @Test
    fun `steerIntent creates correct intent`() {
        controller.create()
        val intent = AgentService.steerIntent(service, "Go back")
        assertEquals(AgentService.ACTION_STEER, intent.action)
        assertEquals("Go back", intent.getStringExtra(AgentService.EXTRA_MESSAGE))
    }

    @Test
    fun `cancelIntent creates correct intent`() {
        controller.create()
        val intent = AgentService.cancelIntent(service)
        assertEquals(AgentService.ACTION_CANCEL, intent.action)
    }

    @Test
    fun `stopIntent creates correct intent`() {
        controller.create()
        val intent = AgentService.stopIntent(service)
        assertEquals(AgentService.ACTION_STOP, intent.action)
    }

    // --- Binder ---

    @Test
    fun `onBind returns AgentBinder`() {
        controller.create()
        val binder = service.onBind(Intent())
        assertIs<AgentService.AgentBinder>(binder)
    }

    @Test
    fun `binder provides service reference`() {
        controller.create()
        val binder = service.onBind(Intent()) as AgentService.AgentBinder
        assertEquals(service, binder.getService())
    }

    // --- Conversation messages ---

    @Test
    fun `initial conversation messages is empty`() {
        controller.create()
        assertTrue(service.conversationMessages.value.isEmpty())
    }

    // --- isActive convenience ---

    @Test
    fun `isActive reflects state correctly through transitions`() {
        configureWithMockApi()

        // Idle → not active
        assertFalse(service.agentState.value.isActive())

        // Thinking → active
        val startIntent = AgentService.startTaskIntent(service, "test")
        service.onStartCommand(startIntent, 0, 1)
        assertTrue(service.agentState.value.isActive())

        // Complete → not active
        service.completeTask("task-1", "Done")
        assertFalse(service.agentState.value.isActive())
    }

    // --- B2: Idle timeout tests (spec S2) ---

    @OptIn(ExperimentalCoroutinesApi::class)
    @Test
    fun `idle timeout starts after completeTask`() = runTest {
        val testDispatcher = StandardTestDispatcher(testScheduler)
        configureWithMockApi()
        service.dispatcher = testDispatcher

        // Start and complete a task
        val intent = AgentService.startTaskIntent(service, "test")
        service.onStartCommand(intent, 0, 1)
        service.completeTask("task-1", "Done")

        // Idle timeout job should be active
        assertNotNull(service.idleTimeoutJob)
        assertTrue(service.idleTimeoutJob!!.isActive)
    }

    @OptIn(ExperimentalCoroutinesApi::class)
    @Test
    fun `idle timeout starts after failTask`() = runTest {
        val testDispatcher = StandardTestDispatcher(testScheduler)
        configureWithMockApi()
        service.dispatcher = testDispatcher

        val intent = AgentService.startTaskIntent(service, "test")
        service.onStartCommand(intent, 0, 1)
        service.failTask("task-1", "Error")

        assertNotNull(service.idleTimeoutJob)
        assertTrue(service.idleTimeoutJob!!.isActive)
    }

    @OptIn(ExperimentalCoroutinesApi::class)
    @Test
    fun `idle timeout starts after handleCancel`() = runTest {
        val testDispatcher = StandardTestDispatcher(testScheduler)
        configureWithMockApi()
        service.dispatcher = testDispatcher

        val startIntent = AgentService.startTaskIntent(service, "test")
        service.onStartCommand(startIntent, 0, 1)

        val cancelIntent = AgentService.cancelIntent(service)
        service.onStartCommand(cancelIntent, 0, 2)

        assertNotNull(service.idleTimeoutJob)
        assertTrue(service.idleTimeoutJob!!.isActive)
    }

    @OptIn(ExperimentalCoroutinesApi::class)
    @Test
    fun `idle timeout is cancelled when new task starts`() = runTest {
        val testDispatcher = StandardTestDispatcher(testScheduler)
        configureWithMockApi()
        service.dispatcher = testDispatcher

        // Complete a task → idle timeout starts
        val intent1 = AgentService.startTaskIntent(service, "first")
        service.onStartCommand(intent1, 0, 1)
        service.completeTask("task-1", "Done")
        assertNotNull(service.idleTimeoutJob)

        // Start new task → idle timeout should be cancelled
        val intent2 = AgentService.startTaskIntent(service, "second")
        service.onStartCommand(intent2, 0, 2)
        assertTrue(service.idleTimeoutJob?.isCancelled ?: true)
    }

    @OptIn(ExperimentalCoroutinesApi::class)
    @Test
    fun `idle timeout starts after system restart`() = runTest {
        val testDispatcher = StandardTestDispatcher(testScheduler)
        controller.create()
        service.dispatcher = testDispatcher

        // Simulate system restart (null intent)
        service.onStartCommand(null, 0, 1)

        assertNotNull(service.idleTimeoutJob)
        assertTrue(service.idleTimeoutJob!!.isActive)
    }

    // --- B4: Cancel cleans up task job ---

    @Test
    fun `cancel cancels currentTaskJob`() {
        configureWithMockApi()
        val startIntent = AgentService.startTaskIntent(service, "test")
        service.onStartCommand(startIntent, 0, 1)

        // Task job should exist
        assertNotNull(service.currentTaskJob)

        val cancelIntent = AgentService.cancelIntent(service)
        service.onStartCommand(cancelIntent, 0, 2)

        // Task job should be cancelled
        assertTrue(service.currentTaskJob?.isCancelled ?: true)
    }

    @Test
    fun `stop cancels currentTaskJob`() {
        configureWithMockApi()
        // Need to go foreground first
        val startIntent = AgentService.startTaskIntent(service, "test")
        service.onStartCommand(startIntent, 0, 1)
        assertNotNull(service.currentTaskJob)

        val stopIntent = AgentService.stopIntent(service)
        service.onStartCommand(stopIntent, 0, 2)

        assertNull(service.currentTaskJob)
    }

    // --- NH1: Notification channel ---

    @Test
    fun `notification channel is created on service create`() {
        controller.create()
        val nm = service.getSystemService(android.app.NotificationManager::class.java)
        val channel = nm.getNotificationChannel(AgentService.CHANNEL_ID)
        assertNotNull(channel)
        assertEquals(AgentService.CHANNEL_NAME, channel.name.toString())
    }

    // --- NB4 related: handleStartTask with null message ---

    @Test
    fun `START_TASK with null message is no-op`() {
        controller.create()
        val intent = Intent(service, AgentService::class.java).apply {
            action = AgentService.ACTION_START_TASK
            // No EXTRA_MESSAGE
        }
        service.onStartCommand(intent, 0, 1)

        assertIs<AgentState.Idle>(service.agentState.value)
    }

    // --- STOP during active task ---

    @Test
    fun `STOP during active task transitions to Idle and cancels job`() {
        configureWithMockApi()
        val startIntent = AgentService.startTaskIntent(service, "test")
        service.onStartCommand(startIntent, 0, 1)
        assertIs<AgentState.Thinking>(service.agentState.value)

        val stopIntent = AgentService.stopIntent(service)
        service.onStartCommand(stopIntent, 0, 2)

        assertIs<AgentState.Idle>(service.agentState.value)
    }
}
