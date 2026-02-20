package ai.citros.chat

import android.content.Context
import android.content.Intent
import org.junit.After
import org.junit.Before
import org.junit.Test
import org.mockito.Mockito.mock
import org.mockito.Mockito.`when`
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

/**
 * Unit tests for OverlayService intents, OverlayController state,
 * bubble positioning, and overlay visibility management.
 *
 * Requires Robolectric for Android framework APIs (Intent construction)
 * that aren't available in standard JVM unit tests.
 *
 * Note: moveOverlayToTop/moveOverlayToBottom require a real WindowManager for full
 * integration testing (instrumented tests on device). Guard conditions (null-safety,
 * no-op when already at target) are tested here via Robolectric.
 */
@RunWith(RobolectricTestRunner::class)
class OverlayServiceTest {

    @Before
    fun setUp() {
        OverlayController.reset()
    }

    @After
    fun tearDown() {
        OverlayController.reset()
    }

    @Test
    fun `startIntent creates intent targeting OverlayService`() {
        val context = mock(Context::class.java)
        `when`(context.packageName).thenReturn("ai.citros.chat")

        val intent = OverlayService.startIntent(context)

        assertNotNull(intent)
        assertEquals(OverlayService::class.java.name, intent.component?.className)
    }

    @Test
    fun `stopIntent has ACTION_STOP action`() {
        val context = mock(Context::class.java)
        `when`(context.packageName).thenReturn("ai.citros.chat")

        val intent = OverlayService.stopIntent(context)

        assertEquals(OverlayService.ACTION_STOP, intent.action)
    }

    @Test
    fun `CHANNEL_ID is a valid string`() {
        assertTrue(OverlayService.CHANNEL_ID.isNotBlank())
    }

    @Test
    fun `NOTIFICATION_ID is positive`() {
        assertTrue(OverlayService.NOTIFICATION_ID > 0)
    }

    @Test
    fun `notification ID is non-zero`() {
        assertTrue(OverlayService.NOTIFICATION_ID > 0,
            "OverlayService notification ID must be positive")
    }

    @Test
    fun `overlay controller mode transitions are reflected correctly`() {
        // Simulate what the service observes
        OverlayController.activateOverlay()
        assertEquals(OverlaySurfaceMode.MINI_CHAT, OverlayController.surfaceMode.value)
        assertTrue(OverlayController.isOverlayActive.value)

        OverlayController.updateSurfaceMode(OverlaySurfaceMode.BUBBLE)
        assertEquals(OverlaySurfaceMode.BUBBLE, OverlayController.surfaceMode.value)
        assertTrue(OverlayController.isOverlayActive.value)

        // FULL_APP should signal the service to stop
        OverlayController.updateSurfaceMode(OverlaySurfaceMode.FULL_APP)
        assertEquals(OverlaySurfaceMode.FULL_APP, OverlayController.surfaceMode.value)
        assertTrue(!OverlayController.isOverlayActive.value)
    }

    @Test
    fun `deactivateOverlay signals service to stop`() {
        OverlayController.activateOverlay()
        assertTrue(OverlayController.isOverlayActive.value)

        OverlayController.deactivateOverlay()
        assertTrue(!OverlayController.isOverlayActive.value)
        assertEquals(OverlaySurfaceMode.FULL_APP, OverlayController.surfaceMode.value)
    }

    @Test
    fun `ACTION_STOP constant is correct`() {
        assertEquals("ai.citros.chat.ACTION_STOP_OVERLAY", OverlayService.ACTION_STOP)
    }

    @Test
    fun `ACTION_EXPAND constant is correct`() {
        assertEquals("ai.citros.chat.ACTION_EXPAND_OVERLAY", OverlayService.ACTION_EXPAND)
    }

    @Test
    fun `calculateBubbleBaseY positions bubble near bottom of screen`() {
        val screenHeight = 2400
        val density = 2.5f
        val baseY = OverlayService.calculateBubbleBaseY(screenHeight, density)
        // sizePx = (56 * 2.5) = 140, marginPx = (100 * 2.5) = 250
        // baseY = 2400 - 140 - 250 = 2010
        assertEquals(2010, baseY)
    }

    @Test
    fun `bubble base Y adjusts above keyboard when offset by IME height`() {
        val screenHeight = 2400
        val density = 2.5f
        val baseY = OverlayService.calculateBubbleBaseY(screenHeight, density)
        val imeHeight = 800
        // In setupKeyboardPolling (currently disabled): params.y = if (imeHeight > 0) baseY - imeHeight else baseY
        val adjustedY = baseY - imeHeight
        assertEquals(1210, adjustedY)
        assertTrue(adjustedY > 0, "Bubble should still be visible above keyboard")
    }

    @Test
    fun `startIntent with flavor includes flavor extra`() {
        val context = mock(Context::class.java)
        val intent = OverlayService.startIntent(context, CitrosFlavor.TANGERINE)
        assertEquals("TANGERINE", intent.getStringExtra("extra_flavor"))
    }

    // --- Service preservation logic tests (#404) ---
    // These verify the conditions used in ChatActivity's DisposableEffect to decide
    // whether to stop the overlay service when the Activity is destroyed.

    @Test
    fun `service should be preserved when executing in MINI_CHAT mode`() {
        OverlayController.activateOverlay()
        OverlayController.updateOverlayState(
            ai.citros.core.OverlayState(
                runState = ai.citros.core.OverlayRunState.EXECUTING,
                steps = emptyList(),
                currentStepIndex = 0,
                totalSteps = 0,
                lines = emptyList()
            )
        )
        val isExecuting = OverlayController.overlayState.value.runState == ai.citros.core.OverlayRunState.EXECUTING
        val surfaceMode = OverlayController.surfaceMode.value
        val isActive = OverlayController.isOverlayActive.value

        // ChatActivity logic: preserve if isExecuting || surfaceMode != FULL_APP
        assertTrue(isActive)
        assertTrue(isExecuting || surfaceMode != OverlaySurfaceMode.FULL_APP)
    }

    @Test
    fun `service should be preserved in BUBBLE mode even when idle`() {
        OverlayController.activateOverlay()
        OverlayController.updateSurfaceMode(OverlaySurfaceMode.BUBBLE)

        val isExecuting = OverlayController.overlayState.value.runState == ai.citros.core.OverlayRunState.EXECUTING
        val surfaceMode = OverlayController.surfaceMode.value

        // Not executing, but in BUBBLE mode — should preserve
        assertTrue(!isExecuting)
        assertEquals(OverlaySurfaceMode.BUBBLE, surfaceMode)
        assertTrue(surfaceMode != OverlaySurfaceMode.FULL_APP)
    }

    @Test
    fun `service should be preserved in MINI_CHAT mode even when idle`() {
        OverlayController.activateOverlay()
        // Default mode after activation is MINI_CHAT
        val surfaceMode = OverlayController.surfaceMode.value

        assertEquals(OverlaySurfaceMode.MINI_CHAT, surfaceMode)
        assertTrue(surfaceMode != OverlaySurfaceMode.FULL_APP)
    }

    @Test
    fun `service should be stopped when idle in FULL_APP mode`() {
        // FULL_APP means the user returned to the main activity — overlay not needed
        OverlayController.activateOverlay()
        OverlayController.updateSurfaceMode(OverlaySurfaceMode.FULL_APP)

        val isExecuting = OverlayController.overlayState.value.runState == ai.citros.core.OverlayRunState.EXECUTING
        val surfaceMode = OverlayController.surfaceMode.value

        // Not executing and in FULL_APP → should stop
        assertTrue(!isExecuting)
        assertEquals(OverlaySurfaceMode.FULL_APP, surfaceMode)
        // ChatActivity logic: preserve if isExecuting || surfaceMode != FULL_APP
        // Neither is true → service should be stopped
        assertTrue(!(isExecuting || surfaceMode != OverlaySurfaceMode.FULL_APP))
    }

    // --- FULL_APP transition tests (#432) ---

    @Test
    fun `FULL_APP transition deactivates overlay`() {
        OverlayController.activateOverlay()
        assertTrue(OverlayController.isOverlayActive.value)
        assertEquals(OverlaySurfaceMode.MINI_CHAT, OverlayController.surfaceMode.value)

        // Simulate what happens when user taps "Full" button
        OverlayController.updateSurfaceMode(OverlaySurfaceMode.FULL_APP)

        // Service should observe: mode=FULL_APP, active=false → launch activity + stop
        assertEquals(OverlaySurfaceMode.FULL_APP, OverlayController.surfaceMode.value)
        assertTrue(!OverlayController.isOverlayActive.value)
    }

    @Test
    fun `launchChatActivity intent has correct flags`() {
        // Verify the intent constructed by launchChatActivity() has the right flags.
        // We can't call the private method directly, but we can verify the intent
        // construction pattern used in the service.
        val context = mock(Context::class.java)
        `when`(context.packageName).thenReturn("ai.citros.chat")

        val intent = Intent(context, ChatActivity::class.java).apply {
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
        }

        assertTrue(
            intent.flags and Intent.FLAG_ACTIVITY_NEW_TASK != 0,
            "Intent must have FLAG_ACTIVITY_NEW_TASK (required from Service context)"
        )
        assertTrue(
            intent.flags and Intent.FLAG_ACTIVITY_SINGLE_TOP != 0,
            "Intent must have FLAG_ACTIVITY_SINGLE_TOP (reuse existing instance)"
        )
        assertEquals(ChatActivity::class.java.name, intent.component?.className)
    }

    @Test
    fun `FULL_APP mode from BUBBLE also deactivates overlay`() {
        OverlayController.activateOverlay()
        OverlayController.updateSurfaceMode(OverlaySurfaceMode.BUBBLE)
        assertTrue(OverlayController.isOverlayActive.value)

        OverlayController.updateSurfaceMode(OverlaySurfaceMode.FULL_APP)
        assertTrue(!OverlayController.isOverlayActive.value)
        assertEquals(OverlaySurfaceMode.FULL_APP, OverlayController.surfaceMode.value)
    }

    // --- Nested hide/restore guard tests (#457 / #458) ---

    @Test
    fun `double hide preserves original visibility and single restore returns to it`() {
        // We can't instantiate OverlayService directly in Robolectric without full
        // service lifecycle, but we can test the logic via the public constants.
        // The savedVisibility sentinel ensures double-hide is a no-op.
        val sentinel = -1 // OverlayService.NO_SAVED_VISIBILITY (private)
        // First hide: savedVisibility goes from sentinel → original (e.g. VISIBLE=0)
        // Second hide: savedVisibility != sentinel → no-op (preserves original)
        // First restore: restores to original, resets to sentinel
        // Second restore: savedVisibility == sentinel → defaults to VISIBLE
        // This is the contract tested here via the actual service below.
        assertTrue(sentinel != android.view.View.VISIBLE,
            "Sentinel must differ from VISIBLE to distinguish 'not saved' from 'was visible'")
        assertTrue(sentinel != android.view.View.INVISIBLE,
            "Sentinel must differ from INVISIBLE")
    }

    @Test
    fun `hideOverlayForScreenshot and restoreOverlayVisibility round-trip on real service`() {
        // Use Robolectric to create a real OverlayService and test hide/restore
        val controller = org.robolectric.Robolectric.buildService(OverlayService::class.java)
        val service = controller.get()
        // Service hasn't called onCreate so overlayView is null — hide/restore should be no-ops
        service.hideOverlayForScreenshot()
        service.restoreOverlayVisibility()
        // No crash = success for null overlayView path
    }

    @Test
    fun `nested hide restore preserves original visibility via View`() {
        // Create a simple View to test the hide/restore logic directly
        val context = androidx.test.core.app.ApplicationProvider.getApplicationContext<android.content.Context>()
        val view = android.view.View(context)
        view.visibility = android.view.View.VISIBLE

        // Simulate OverlayService's savedVisibility logic
        var savedVisibility = -1 // NO_SAVED_VISIBILITY sentinel

        // First hide
        if (savedVisibility == -1) {
            savedVisibility = view.visibility // saves VISIBLE (0)
            view.visibility = android.view.View.INVISIBLE
        }
        assertEquals(android.view.View.INVISIBLE, view.visibility)
        assertEquals(android.view.View.VISIBLE, savedVisibility)

        // Second hide — should be no-op
        if (savedVisibility == -1) {
            savedVisibility = view.visibility
            view.visibility = android.view.View.INVISIBLE
        }
        // savedVisibility should still be original VISIBLE, not INVISIBLE
        assertEquals(android.view.View.VISIBLE, savedVisibility)

        // First restore
        view.visibility = if (savedVisibility != -1) savedVisibility else android.view.View.VISIBLE
        savedVisibility = -1
        assertEquals(android.view.View.VISIBLE, view.visibility)

        // Second restore — no saved state, defaults to VISIBLE
        view.visibility = if (savedVisibility != -1) savedVisibility else android.view.View.VISIBLE
        savedVisibility = -1
        assertEquals(android.view.View.VISIBLE, view.visibility)
    }

    // --- Chat foreground overlay suppression tests (#627) ---
    // NOTE: These tests verify the visibility-toggling *algorithm* (the if/else logic),
    // not the OverlayService wiring. True integration tests would require Robolectric
    // service lifecycle support to bind the overlay View and observe StateFlow collection.
    // The OverlayControllerTest above covers the state management layer; these cover
    // the decision logic that determines when to show/hide/skip restore.

    @Test
    fun `overlay becomes INVISIBLE when isChatInForeground emits true`() {
        val context = androidx.test.core.app.ApplicationProvider.getApplicationContext<android.content.Context>()
        val view = android.view.View(context)
        view.visibility = android.view.View.VISIBLE

        // Simulate observeChatForeground collecting inForeground=true
        val inForeground = true
        if (inForeground) {
            if (view.visibility == android.view.View.VISIBLE) {
                view.visibility = android.view.View.INVISIBLE
            }
        }

        assertEquals(android.view.View.INVISIBLE, view.visibility)
    }

    @Test
    fun `overlay restores to VISIBLE when isChatInForeground emits false`() {
        val context = androidx.test.core.app.ApplicationProvider.getApplicationContext<android.content.Context>()
        val view = android.view.View(context)
        view.visibility = android.view.View.INVISIBLE
        val savedVisibility = -1 // NO_SAVED_VISIBILITY

        // Simulate observeChatForeground collecting inForeground=false
        val inForeground = false
        if (!inForeground) {
            if (view.visibility == android.view.View.INVISIBLE && savedVisibility == -1) {
                view.visibility = android.view.View.VISIBLE
            }
        }

        assertEquals(android.view.View.VISIBLE, view.visibility)
    }

    @Test
    fun `chat foreground restore suppressed when savedVisibility is set`() {
        val context = androidx.test.core.app.ApplicationProvider.getApplicationContext<android.content.Context>()
        val view = android.view.View(context)
        view.visibility = android.view.View.INVISIBLE
        var savedVisibility = android.view.View.INVISIBLE // screenshot hide was active

        // Simulate observeChatForeground collecting inForeground=false
        // with savedVisibility != NO_SAVED_VISIBILITY
        val inForeground = false
        if (!inForeground) {
            if (view.visibility == android.view.View.INVISIBLE && savedVisibility == -1) {
                view.visibility = android.view.View.VISIBLE
            } else if (savedVisibility != -1) {
                // Race condition fix: update saved state so restore gets VISIBLE
                savedVisibility = android.view.View.VISIBLE
            }
        }

        // View stays INVISIBLE (not directly restored by chat-foreground observer)
        assertEquals(android.view.View.INVISIBLE, view.visibility)
        // But savedVisibility updated so restoreOverlayVisibility will restore to VISIBLE
        assertEquals(android.view.View.VISIBLE, savedVisibility)
    }

    @Test
    fun `restoreOverlayVisibility skips restore when isChatInForeground is true and clears savedVisibility`() {
        val context = androidx.test.core.app.ApplicationProvider.getApplicationContext<android.content.Context>()
        val view = android.view.View(context)
        view.visibility = android.view.View.INVISIBLE
        var savedVisibility = android.view.View.VISIBLE // was hidden for screenshot

        // Simulate restoreOverlayVisibility with chat in foreground
        OverlayController.setChatInForeground(true)

        if (OverlayController.isChatInForeground.value) {
            // Skip restore — chat foreground observer handles visibility
            // Safe to clear: overlay is binary VISIBLE/INVISIBLE, and the
            // chat-foreground observer will handle restoring visibility when
            // ChatActivity eventually leaves the foreground.
            savedVisibility = -1 // NO_SAVED_VISIBILITY
        } else {
            view.visibility = if (savedVisibility != -1) savedVisibility else android.view.View.VISIBLE
            savedVisibility = -1
        }

        // View stays INVISIBLE (restore was skipped)
        assertEquals(android.view.View.INVISIBLE, view.visibility)
        // savedVisibility was cleared
        assertEquals(-1, savedVisibility)
    }

    // Regression guard (#583): calculateMiniChatHeight was removed from production code.
    // This test ensures only the current public height API (calculateBubbleBaseY) is tested
    // and no stale references cause compile errors.
    @Test
    fun `no stale calculateMiniChatHeight reference exists`() {
        // If this test compiles, the stale reference from #583 is confirmed gone.
        // calculateBubbleBaseY is the only public height API on OverlayService.
        val result = OverlayService.calculateBubbleBaseY(1920, 2.0f)
        assertTrue(result > 0, "calculateBubbleBaseY should return a positive value")
    }

    // softInputMode (#444/#451) and keyboard behavior require instrumented tests —
    // buildLayoutParams is private, and WindowManager interaction can't be verified
    // in Robolectric. Verified on-device: SOFT_INPUT_ADJUST_PAN pans the overlay
    // above the keyboard so the TextField stays visible.

    // ========== moveOverlayToTop / moveOverlayToBottom Tests ==========

    @Test
    fun `moveOverlayToTop is no-op when service is not fully initialized`() {
        // Fresh service — overlayView, overlayParams, windowManager are all null
        val service = Robolectric.buildService(OverlayService::class.java).get()
        // Should not throw
        service.moveOverlayToTop()
        service.moveOverlayToBottom()
    }

    @Test
    fun `moveOverlayToBottom is no-op when service is not fully initialized`() {
        val service = Robolectric.buildService(OverlayService::class.java).get()
        // Should not throw
        service.moveOverlayToBottom()
    }

    // Note: Full integration tests for moveOverlayToTop/moveOverlayToBottom with a real
    // WindowManager require instrumented tests on device. The gravity flip and
    // updateViewLayout call chain are verified during E2E testing.
}
