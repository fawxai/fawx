package ai.citros.chat

import ai.citros.core.OverlayLine
import ai.citros.core.OverlayLineType
import ai.citros.core.OverlayRunState
import ai.citros.core.OverlayState
import ai.citros.core.OverlayStep
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.joinAll
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.runTest
import org.junit.After
import org.junit.Before
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

@OptIn(ExperimentalCoroutinesApi::class)
class OverlayControllerTest {

    @Before
    fun setUp() {
        OverlayController.reset()
    }

    @After
    fun tearDown() {
        OverlayController.reset()
    }

    @Test
    fun `initial state is inactive with FULL_APP mode`() {
        assertFalse(OverlayController.isOverlayActive.value)
        assertEquals(OverlaySurfaceMode.FULL_APP, OverlayController.surfaceMode.value)
        assertEquals(OverlayState.EMPTY, OverlayController.overlayState.value)
        assertNull(OverlayController.queuedMessage.value)
        assertEquals(0, OverlayController.unreadCount.value)
    }

    @Test
    fun `activateOverlay sets active and switches to SEARCH_BAR`() {
        OverlayController.activateOverlay()

        assertTrue(OverlayController.isOverlayActive.value)
        assertEquals(OverlaySurfaceMode.SEARCH_BAR, OverlayController.surfaceMode.value)
    }

    @Test
    fun `activateOverlay preserves non-FULL_APP mode`() {
        OverlayController.updateSurfaceMode(OverlaySurfaceMode.SEARCH_BAR)
        OverlayController.activateOverlay()

        assertTrue(OverlayController.isOverlayActive.value)
        assertEquals(OverlaySurfaceMode.SEARCH_BAR, OverlayController.surfaceMode.value)
    }

    @Test
    fun `deactivateOverlay resets to FULL_APP and inactive`() {
        OverlayController.activateOverlay()
        OverlayController.deactivateOverlay()

        assertFalse(OverlayController.isOverlayActive.value)
        assertEquals(OverlaySurfaceMode.FULL_APP, OverlayController.surfaceMode.value)
    }

    @Test
    fun `updateSurfaceMode to FULL_APP deactivates overlay`() {
        OverlayController.activateOverlay()
        assertTrue(OverlayController.isOverlayActive.value)

        OverlayController.updateSurfaceMode(OverlaySurfaceMode.FULL_APP)

        assertFalse(OverlayController.isOverlayActive.value)
        assertEquals(OverlaySurfaceMode.FULL_APP, OverlayController.surfaceMode.value)
    }

    @Test
    fun `updateSurfaceMode to SEARCH_BAR keeps overlay active`() {
        OverlayController.activateOverlay()
        OverlayController.updateSurfaceMode(OverlaySurfaceMode.SEARCH_BAR)

        assertTrue(OverlayController.isOverlayActive.value)
        assertEquals(OverlaySurfaceMode.SEARCH_BAR, OverlayController.surfaceMode.value)
    }

    @Test
    fun `updateSurfaceMode to PANEL keeps overlay active`() {
        OverlayController.activateOverlay()
        OverlayController.updateSurfaceMode(OverlaySurfaceMode.SEARCH_BAR)
        OverlayController.updateSurfaceMode(OverlaySurfaceMode.PANEL)

        assertTrue(OverlayController.isOverlayActive.value)
        assertEquals(OverlaySurfaceMode.PANEL, OverlayController.surfaceMode.value)
    }

    @Test
    fun `updateOverlayState propagates state`() {
        val state = OverlayState(
            runState = OverlayRunState.EXECUTING,
            steps = listOf(OverlayStep(step = 1, total = 3, label = "Opening app")),
            lines = listOf(OverlayLine(id = 1, type = OverlayLineType.USER, text = "Do something")),
            currentStepIndex = 0,
            totalSteps = 3
        )

        OverlayController.updateOverlayState(state)

        assertEquals(state, OverlayController.overlayState.value)
        assertEquals(OverlayRunState.EXECUTING, OverlayController.overlayState.value.runState)
    }

    @Test
    fun `updateQueuedMessage stores non-blank text`() {
        OverlayController.updateQueuedMessage("check bluetooth too")
        assertEquals("check bluetooth too", OverlayController.queuedMessage.value)
    }

    @Test
    fun `updateQueuedMessage clears blank text`() {
        OverlayController.updateQueuedMessage("something")
        OverlayController.updateQueuedMessage("   ")
        assertNull(OverlayController.queuedMessage.value)
    }

    @Test
    fun `updateQueuedMessage clears null text`() {
        OverlayController.updateQueuedMessage("something")
        OverlayController.updateQueuedMessage(null)
        assertNull(OverlayController.queuedMessage.value)
    }

    @Test
    fun `updateUnreadCount updates count`() {
        OverlayController.updateUnreadCount(5)
        assertEquals(5, OverlayController.unreadCount.value)
    }

    @Test
    fun `updateUnreadCount coerces negative to zero`() {
        OverlayController.updateUnreadCount(-3)
        assertEquals(0, OverlayController.unreadCount.value)
    }

    @Test
    fun `resetUnreadCount sets count to zero`() {
        OverlayController.updateUnreadCount(5)
        OverlayController.resetUnreadCount()
        assertEquals(0, OverlayController.unreadCount.value)
    }

    // --- Queued message tests (#445) ---

    @Test
    fun `queued message survives overlay state updates`() {
        OverlayController.updateQueuedMessage("user typed this")

        OverlayController.updateOverlayState(
            OverlayState(
                runState = OverlayRunState.EXECUTING,
                steps = listOf(OverlayStep(step = 2, total = 5, label = "Tapping")),
                lines = emptyList(),
                currentStepIndex = 0,
                totalSteps = 5
            )
        )

        assertEquals("user typed this", OverlayController.queuedMessage.value)
    }

    // --- Action dispatch tests (#463) ---
    //
    // SharedFlow + runTest gotcha (#466): dispatch() uses tryEmit() which buffers
    // values synchronously. Collectors must use UnconfinedTestDispatcher so they
    // eagerly process buffered emissions. StandardTestDispatcher (the default)
    // requires explicit advanceUntilIdle() and still races with SharedFlow's
    // internal notification mechanism. backgroundScope prevents the collector
    // from blocking runTest completion.

    @Test
    fun `dispatch emits QueueMessage action`() = runTest {
        val received = mutableListOf<OverlayAction>()
        backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) {
            OverlayController.actions.collect { received.add(it) }
        }

        OverlayController.dispatch(OverlayAction.QueueMessage("hello"))

        assertEquals(1, received.size)
        assertEquals(OverlayAction.QueueMessage("hello"), received[0])
    }

    @Test
    fun `dispatch emits SetSurfaceMode action`() = runTest {
        val received = mutableListOf<OverlayAction>()
        backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) {
            OverlayController.actions.collect { received.add(it) }
        }

        OverlayController.dispatch(OverlayAction.SetSurfaceMode(OverlaySurfaceMode.SEARCH_BAR))

        assertEquals(1, received.size)
        assertEquals(OverlayAction.SetSurfaceMode(OverlaySurfaceMode.SEARCH_BAR), received[0])
    }

    @Test
    fun `dispatch emits Deactivate action`() = runTest {
        val received = mutableListOf<OverlayAction>()
        backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) {
            OverlayController.actions.collect { received.add(it) }
        }

        OverlayController.dispatch(OverlayAction.Deactivate)

        assertEquals(1, received.size)
        assertEquals(OverlayAction.Deactivate, received[0])
    }

    @Test
    fun `dispatch emits StopExecution action`() = runTest {
        val received = mutableListOf<OverlayAction>()
        backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) {
            OverlayController.actions.collect { received.add(it) }
        }

        OverlayController.dispatch(OverlayAction.StopExecution)

        assertEquals(1, received.size)
        assertEquals(OverlayAction.StopExecution, received[0])
    }

    @Test
    fun `dispatch emits ExpandFromSearchBar action`() = runTest {
        val received = mutableListOf<OverlayAction>()
        backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) {
            OverlayController.actions.collect { received.add(it) }
        }

        OverlayController.dispatch(OverlayAction.ExpandFromSearchBar)

        assertEquals(1, received.size)
        assertEquals(OverlayAction.ExpandFromSearchBar, received[0])
    }

    @Test
    fun `dispatch preserves FIFO ordering`() = runTest {
        val received = mutableListOf<OverlayAction>()
        backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) {
            OverlayController.actions.collect { received.add(it) }
        }

        OverlayController.dispatch(OverlayAction.QueueMessage("first"))
        OverlayController.dispatch(OverlayAction.StopExecution)
        OverlayController.dispatch(OverlayAction.SetSurfaceMode(OverlaySurfaceMode.SEARCH_BAR))

        assertEquals(3, received.size)
        assertEquals(OverlayAction.QueueMessage("first"), received[0])
        assertEquals(OverlayAction.StopExecution, received[1])
        assertEquals(OverlayAction.SetSurfaceMode(OverlaySurfaceMode.SEARCH_BAR), received[2])
    }

    @Test
    fun `dispatch without collector drops overflow gracefully`() {
        // Dispatch more actions than buffer capacity (16) without a collector.
        // tryEmit() returns false for overflow items — logged, not thrown.
        repeat(20) { i ->
            OverlayController.dispatch(OverlayAction.QueueMessage("msg_$i"))
        }
        // No crash = pass. Buffer overflow is logged via Log.e, not thrown.
    }

    @Test
    fun `dispatch with active collector handles burst beyond buffer capacity`() = runTest {
        // With an eager collector, all dispatches are processed (collector frees
        // buffer space immediately), so even bursts beyond buffer size succeed.
        val received = mutableListOf<OverlayAction>()
        backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) {
            OverlayController.actions.collect { received.add(it) }
        }

        repeat(20) { i ->
            OverlayController.dispatch(OverlayAction.QueueMessage("msg_$i"))
        }

        assertEquals(20, received.size)
        assertEquals("msg_0", (received.first() as OverlayAction.QueueMessage).text)
        assertEquals("msg_19", (received.last() as OverlayAction.QueueMessage).text)
    }

    @Test
    fun `concurrent rapid dispatches do not crash`() = runTest {
        val received = mutableListOf<OverlayAction>()
        backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) {
            OverlayController.actions.collect { received.add(it) }
        }

        // Simulate rapid user taps
        OverlayController.dispatch(OverlayAction.StopExecution)
        OverlayController.dispatch(OverlayAction.QueueMessage("quick"))
        OverlayController.dispatch(OverlayAction.ExpandFromSearchBar)
        OverlayController.dispatch(OverlayAction.Deactivate)

        // All actions should be received (well within buffer capacity)
        assertEquals(4, received.size)
    }

    // --- Debounce guard for activateOverlay (#437) ---

    @Test
    fun `activateOverlay debounces rapid calls`() {
        OverlayController.activateOverlay()
        assertTrue(OverlayController.isOverlayActive.value)

        // Deactivate and immediately re-activate within debounce window
        OverlayController.deactivateOverlay()
        assertFalse(OverlayController.isOverlayActive.value)

        // Second call within debounce window should be skipped
        val result = OverlayController.activateOverlay()
        assertFalse(result, "Second activateOverlay within debounce window should be skipped")
        assertFalse(OverlayController.isOverlayActive.value)
    }

    @Test
    fun `activateOverlay succeeds after debounce window`() {
        OverlayController.activateOverlay()
        assertTrue(OverlayController.isOverlayActive.value)

        // reset() clears the debounce timestamp, simulating window expiry
        OverlayController.reset()
        val result = OverlayController.activateOverlay()
        assertTrue(result, "activateOverlay after debounce window should succeed")
        assertTrue(OverlayController.isOverlayActive.value)
    }

    @Test
    fun `activateOverlay is thread-safe under concurrent calls`() = runBlocking {
        OverlayController.reset()
        val results = java.util.concurrent.ConcurrentLinkedQueue<Boolean>()
        val jobs = (1..10).map {
            launch(Dispatchers.Default) {
                results.add(OverlayController.activateOverlay())
            }
        }
        jobs.joinAll()
        // Exactly one call should win the CAS race within the debounce window
        assertEquals(1, results.count { it }, "Only one concurrent activateOverlay should succeed")
        assertTrue(OverlayController.isOverlayActive.value)
    }

    @Test
    fun `reset clears all state`() {
        OverlayController.activateOverlay()
        OverlayController.updateQueuedMessage("pending")
        OverlayController.updateUnreadCount(3)
        OverlayController.updateInteractionDemand(OverlayInteractionDemand.INPUT_REQUIRED)
        OverlayController.updateOverlayState(
            OverlayState(
                runState = OverlayRunState.EXECUTING,
                steps = emptyList(),
                lines = emptyList(),
                currentStepIndex = 0,
                totalSteps = 0
            )
        )

        OverlayController.reset()

        assertFalse(OverlayController.isOverlayActive.value)
        assertEquals(OverlaySurfaceMode.FULL_APP, OverlayController.surfaceMode.value)
        assertEquals(OverlayState.EMPTY, OverlayController.overlayState.value)
        assertNull(OverlayController.queuedMessage.value)
        assertEquals(0, OverlayController.unreadCount.value)
        assertEquals(OverlayInteractionDemand.NONE, OverlayController.interactionDemand.value)
        assertFalse(OverlayController.userPanelPinned.value)
    }

    @Test
    fun `interaction demand forces panel while active`() {
        OverlayController.activateOverlay()
        assertEquals(OverlaySurfaceMode.SEARCH_BAR, OverlayController.surfaceMode.value)

        OverlayController.updateInteractionDemand(OverlayInteractionDemand.INPUT_REQUIRED)

        assertEquals(OverlaySurfaceMode.PANEL, OverlayController.surfaceMode.value)
    }

    @Test
    fun `clearing interaction demand returns to search bar when panel not pinned`() {
        OverlayController.activateOverlay()
        OverlayController.updateInteractionDemand(OverlayInteractionDemand.INPUT_REQUIRED)
        assertEquals(OverlaySurfaceMode.PANEL, OverlayController.surfaceMode.value)

        OverlayController.updateInteractionDemand(OverlayInteractionDemand.NONE)

        assertEquals(OverlaySurfaceMode.SEARCH_BAR, OverlayController.surfaceMode.value)
    }

    @Test
    fun `pinned panel stays open after interaction demand clears`() {
        OverlayController.activateOverlay()
        OverlayController.updateSurfaceMode(OverlaySurfaceMode.PANEL, fromUser = true)
        OverlayController.updateInteractionDemand(OverlayInteractionDemand.INPUT_REQUIRED)

        OverlayController.updateInteractionDemand(OverlayInteractionDemand.NONE)
        assertEquals(OverlaySurfaceMode.PANEL, OverlayController.surfaceMode.value)

        OverlayController.setUserPanelPinned(false)
        assertEquals(OverlaySurfaceMode.SEARCH_BAR, OverlayController.surfaceMode.value)
    }

    @Test
    fun `mode transition flow - FULL_APP to SEARCH_BAR to PANEL to FULL_APP`() {
        assertEquals(OverlaySurfaceMode.FULL_APP, OverlayController.surfaceMode.value)
        assertFalse(OverlayController.isOverlayActive.value)

        OverlayController.activateOverlay()
        assertEquals(OverlaySurfaceMode.SEARCH_BAR, OverlayController.surfaceMode.value)
        assertTrue(OverlayController.isOverlayActive.value)

        OverlayController.updateSurfaceMode(OverlaySurfaceMode.PANEL)
        assertEquals(OverlaySurfaceMode.PANEL, OverlayController.surfaceMode.value)
        assertTrue(OverlayController.isOverlayActive.value)

        OverlayController.updateSurfaceMode(OverlaySurfaceMode.FULL_APP)
        assertEquals(OverlaySurfaceMode.FULL_APP, OverlayController.surfaceMode.value)
        assertFalse(OverlayController.isOverlayActive.value)
    }


    // --- Toggle button tests (#608/PR #614) ---

    @Test
    fun `toggle deactivates overlay when active`() {
        OverlayController.activateOverlay()
        assertTrue(OverlayController.isOverlayActive.value)

        // Simulate toggle onClick: if active -> deactivate
        val isActive = OverlayController.isOverlayActive.value
        if (isActive) {
            OverlayController.deactivateOverlay()
        } else {
            OverlayController.activateOverlay()
        }

        assertFalse(OverlayController.isOverlayActive.value)
        assertEquals(OverlaySurfaceMode.FULL_APP, OverlayController.surfaceMode.value)
    }

    @Test
    fun `toggle activates overlay when inactive`() {
        assertFalse(OverlayController.isOverlayActive.value)

        // Simulate toggle onClick: if inactive -> activate
        val isActive = OverlayController.isOverlayActive.value
        if (isActive) {
            OverlayController.deactivateOverlay()
        } else {
            OverlayController.activateOverlay()
        }

        assertTrue(OverlayController.isOverlayActive.value)
        assertEquals(OverlaySurfaceMode.SEARCH_BAR, OverlayController.surfaceMode.value)
    }

    // --- Restore hook tests (#608/PR #614) ---

    @Test
    fun `restore hook activates overlay from inactive state`() {
        // Simulate: overlay was never activated (race condition scenario)
        assertFalse(OverlayController.isOverlayActive.value)

        // Restore hook logic: activate only if not already active
        if (!OverlayController.isOverlayActive.value) {
            OverlayController.updateSurfaceMode(OverlaySurfaceMode.SEARCH_BAR)
            OverlayController.activateOverlay()
        }

        assertTrue(OverlayController.isOverlayActive.value)
        assertEquals(OverlaySurfaceMode.SEARCH_BAR, OverlayController.surfaceMode.value)
    }

    @Test
    fun `restore hook skips activation when already active`() {
        // Overlay already running (normal case)
        OverlayController.activateOverlay()
        assertTrue(OverlayController.isOverlayActive.value)
        OverlayController.updateSurfaceMode(OverlaySurfaceMode.SEARCH_BAR)

        // Restore hook: should not re-activate (already active)
        if (!OverlayController.isOverlayActive.value) {
            OverlayController.updateSurfaceMode(OverlaySurfaceMode.SEARCH_BAR)
            OverlayController.activateOverlay()
        }

        // Mode should remain SEARCH_BAR (not overridden)
        assertTrue(OverlayController.isOverlayActive.value)
        assertEquals(OverlaySurfaceMode.SEARCH_BAR, OverlayController.surfaceMode.value)
    }

    // --- Chat foreground state tests (#627) ---

    @Test
    fun `setChatInForeground updates isChatInForeground and reset clears it`() {
        assertFalse(OverlayController.isChatInForeground.value)

        OverlayController.setChatInForeground(true)
        assertTrue(OverlayController.isChatInForeground.value)

        OverlayController.setChatInForeground(false)
        assertFalse(OverlayController.isChatInForeground.value)

        OverlayController.setChatInForeground(true)
        assertTrue(OverlayController.isChatInForeground.value)

        OverlayController.reset()
        assertFalse(OverlayController.isChatInForeground.value)
    }

    @Test
    fun `restore hook with SEARCH_BAR preferred mode activates as SEARCH_BAR`() {
        assertFalse(OverlayController.isOverlayActive.value)

        // Simulate restore with SEARCH_BAR preference
        if (!OverlayController.isOverlayActive.value) {
            OverlayController.updateSurfaceMode(OverlaySurfaceMode.SEARCH_BAR)
            OverlayController.activateOverlay()
        }

        assertTrue(OverlayController.isOverlayActive.value)
        assertEquals(OverlaySurfaceMode.SEARCH_BAR, OverlayController.surfaceMode.value)
    }
}
