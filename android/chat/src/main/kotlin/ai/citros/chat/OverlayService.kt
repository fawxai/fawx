package ai.citros.chat

import java.lang.ref.WeakReference
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.SharedPreferences
import android.graphics.PixelFormat
import android.os.Build
import android.os.IBinder
import android.util.Log
import android.view.Gravity
import android.view.HapticFeedbackConstants
import android.view.View
import android.view.WindowInsets
import android.view.WindowInsetsAnimation
import android.view.WindowManager
import android.view.animation.DecelerateInterpolator
import androidx.annotation.MainThread
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.ComposeView
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.LifecycleRegistry
import androidx.lifecycle.setViewTreeLifecycleOwner
import androidx.savedstate.SavedStateRegistry
import androidx.savedstate.SavedStateRegistryController
import androidx.savedstate.SavedStateRegistryOwner
import androidx.savedstate.setViewTreeSavedStateRegistryOwner
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.drop
import kotlinx.coroutines.launch

/**
 * Foreground service that renders the Citros overlay on top of other apps
 * using [WindowManager] with [WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY].
 *
 * Supports two visual modes:
 * - **MINI_CHAT**: Bottom-anchored floating panel (~40% screen height, draggable).
 * - **BUBBLE**: Small circular indicator (~56dp, draggable, tap to expand).
 *
 * The service observes [OverlayController] flows for state updates and mode changes.
 * A [ComposeView] renders the overlay UI using the same composables from
 * [OverlayContent].
 *
 * Lifecycle: Started/stopped by [ChatActivity] based on [OverlayController.isOverlayActive].
 */
class OverlayService : Service(), LifecycleOwner, SavedStateRegistryOwner {

    companion object {
        private const val TAG = "OverlayService"

        /** Weak reference to the running instance for screenshot overlay hiding.
         *  Uses WeakReference to avoid leaking the service if onDestroy is not called
         *  (e.g. OOM kill). */
        @Volatile
        private var instanceRef: WeakReference<OverlayService>? = null

        val instance: OverlayService?
            get() = instanceRef?.get()

        const val CHANNEL_ID = "citros_overlay_channel"
        const val NOTIFICATION_ID = 2001
        const val ACTION_STOP = "ai.citros.chat.ACTION_STOP_OVERLAY"
        const val ACTION_EXPAND = "ai.citros.chat.ACTION_EXPAND_OVERLAY"
        private const val EXTRA_FLAVOR = "extra_flavor"

        private const val MINI_CHAT_HEIGHT_FRACTION = 0.4f
        /** Max fraction of available space above keyboard for mini chat. */
        private const val BUBBLE_SIZE_DP = 56
        private const val BUBBLE_MARGIN_BOTTOM_DP = 100

        /** Sentinel value indicating no visibility has been saved yet. */
        private const val NO_SAVED_VISIBILITY = -1

        /**
         * Build an intent to start the overlay service.
         *
         * @param context Application or Activity context
         * @param flavor Optional flavor to pass to the service for theming consistency.
         *               If null, the service reads from SharedPreferences.
         */
        internal fun startIntent(context: Context, flavor: CitrosFlavor? = null): Intent =
            Intent(context, OverlayService::class.java).apply {
                flavor?.let { putExtra(EXTRA_FLAVOR, it.name) }
            }

        /**
         * Build an intent to stop the overlay service.
         */
        fun stopIntent(context: Context): Intent =
            Intent(context, OverlayService::class.java).apply {
                action = ACTION_STOP
            }

        /** Calculate bubble Y position. Exposed for testing. */
        internal fun calculateBubbleBaseY(screenHeight: Int, density: Float): Int {
            val sizePx = (BUBBLE_SIZE_DP * density).toInt()
            return screenHeight - sizePx - (BUBBLE_MARGIN_BOTTOM_DP * density).toInt()
        }


    }

    // Initialized lazily in onCreate() to avoid leaking 'this' during construction (#1)
    private lateinit var lifecycleRegistry: LifecycleRegistry
    private lateinit var savedStateRegistryController: SavedStateRegistryController

    override val lifecycle: Lifecycle get() = lifecycleRegistry
    override val savedStateRegistry: SavedStateRegistry
        get() = savedStateRegistryController.savedStateRegistry

    private var windowManager: WindowManager? = null
    private var overlayView: View? = null
    private var overlayParams: WindowManager.LayoutParams? = null
    /** Tracks IME visibility to avoid redundant layout updates. */
    private var lastImeVisible = false
    private var currentMode: OverlaySurfaceMode = OverlaySurfaceMode.BUBBLE
    private val serviceScope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
    private var modeObserverJob: Job? = null
    private var foregroundObserverJob: Job? = null
    private var selectedFlavor by mutableStateOf(CitrosFlavor.TANGERINE)
    private var selectedThemeMode by mutableStateOf(THEME_MODE_DEFAULT)
    private var onboardingPrefs: SharedPreferences? = null
    private var onboardingPrefsListener: SharedPreferences.OnSharedPreferenceChangeListener? = null

    // Drag state
    private var animationJob: Job? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        instanceRef = WeakReference(this)

        // Initialize lifecycle components here, not as class-level properties (#1)
        lifecycleRegistry = LifecycleRegistry(this)
        savedStateRegistryController = SavedStateRegistryController.create(this)
        savedStateRegistryController.performRestore(null)
        lifecycleRegistry.currentState = Lifecycle.State.CREATED

        // Safe WindowManager acquisition (#4)
        windowManager = getSystemService(Context.WINDOW_SERVICE) as? WindowManager
        if (windowManager == null) {
            Log.e(TAG, "Failed to get WindowManager — cannot show overlay")
            stopSelf()
            return
        }

        // Read appearance preferences for live theming.
        selectedFlavor = readSelectedFlavor(this)
        selectedThemeMode = readThemeMode(this)
        registerOnboardingPrefsListener()

        createNotificationChannel()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            startForeground(
                NOTIFICATION_ID,
                buildNotification("Citros is controlling your phone"),
                android.content.pm.ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE
            )
        } else {
            startForeground(NOTIFICATION_ID, buildNotification("Citros is controlling your phone"))
        }

        // Set STARTED before showing overlay to avoid Compose lifecycle mismatch (#2)
        lifecycleRegistry.currentState = Lifecycle.State.STARTED
        showOverlay()
        observeModeChanges()
        observeChatForeground()
        Log.d(TAG, "OverlayService created")
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        // Always trust persisted appearance prefs as source of truth.
        refreshAppearanceFromPrefs()

        when (intent?.action) {
            ACTION_STOP -> {
                OverlayController.deactivateOverlay()
                stopSelf()
                return START_NOT_STICKY
            }
            ACTION_EXPAND -> {
                OverlayController.updateSurfaceMode(OverlaySurfaceMode.MINI_CHAT)
                return START_STICKY
            }
        }
        return START_STICKY
    }

    /**
     * Force-refresh overlay appearance from persisted prefs.
     * Used when settings change while the overlay service is already running.
     */
    fun refreshAppearanceFromPrefs() {
        selectedFlavor = readSelectedFlavor(this)
        selectedThemeMode = readThemeMode(this)
    }

    override fun onTaskRemoved(rootIntent: Intent?) {
        Log.d(TAG, "onTaskRemoved: overlay service task removed")
        super.onTaskRemoved(rootIntent)
    }

    override fun onDestroy() {
        instanceRef = null
        Log.d(TAG, "onDestroy: overlay service being destroyed", Exception("stack trace"))
        // Proper lifecycle state transitions (#6)
        if (lifecycleRegistry.currentState.isAtLeast(Lifecycle.State.STARTED)) {
            lifecycleRegistry.currentState = Lifecycle.State.CREATED
        }
        lifecycleRegistry.currentState = Lifecycle.State.DESTROYED

        modeObserverJob?.cancel()
        foregroundObserverJob?.cancel()
        serviceScope.cancel()
        unregisterOnboardingPrefsListener()
        removeOverlay()
        Log.d(TAG, "OverlayService destroyed")
        super.onDestroy()
    }

    private fun registerOnboardingPrefsListener() {
        val prefs = getSharedPreferences(ONBOARDING_PREFS, Context.MODE_PRIVATE)
        onboardingPrefs = prefs
        if (onboardingPrefsListener != null) return
        onboardingPrefsListener = SharedPreferences.OnSharedPreferenceChangeListener { sharedPrefs, key ->
            when (key) {
                PREF_SELECTED_FLAVOR -> {
                    val nextFlavor = CitrosFlavor.fromStorage(
                        sharedPrefs.getString(PREF_SELECTED_FLAVOR, CitrosFlavor.TANGERINE.storageValue)
                    )
                    serviceScope.launch {
                        selectedFlavor = nextFlavor
                    }
                }
                PREF_THEME_MODE -> {
                    val nextMode =
                        sharedPrefs.getString(PREF_THEME_MODE, THEME_MODE_DEFAULT) ?: THEME_MODE_DEFAULT
                    serviceScope.launch {
                        selectedThemeMode = nextMode
                    }
                }
            }
        }
        prefs.registerOnSharedPreferenceChangeListener(onboardingPrefsListener)
    }

    private fun unregisterOnboardingPrefsListener() {
        val prefs = onboardingPrefs
        val listener = onboardingPrefsListener
        if (prefs != null && listener != null) {
            prefs.unregisterOnSharedPreferenceChangeListener(listener)
        }
        onboardingPrefsListener = null
        onboardingPrefs = null
    }

    private fun observeModeChanges() {
        modeObserverJob = serviceScope.launch {
            // Drop the first emission to avoid stopping the service before it's fully started (#8).
            // The initial state was already applied in showOverlay().
            combine(
                OverlayController.surfaceMode,
                OverlayController.isOverlayActive
            ) { mode, active -> mode to active }
                .drop(1)
                .collect { (mode, active) ->
                    if (!active || mode == OverlaySurfaceMode.FULL_APP) {
                        if (mode == OverlaySurfaceMode.FULL_APP) {
                            launchChatActivity()
                        }
                        stopSelf()
                    } else {
                        updateOverlayLayout(mode)
                        updateNotification(mode)
                    }
                }
        }
    }

    /**
     * Observe [OverlayController.isChatInForeground] and suppress overlay visibility
     * when ChatActivity is in the foreground (#627). The overlay is redundant when the
     * user is already viewing the full-screen chat. When the user leaves ChatActivity
     * (e.g. switches to another app), the overlay is restored.
     *
     * Uses [View.INVISIBLE] (not GONE) to preserve WindowManager layout position,
     * consistent with [hideOverlayForScreenshot].
     *
     * **Known limitation:** In split-screen or PiP mode, [onPause] fires even though
     * ChatActivity is partially visible, causing the overlay to restore prematurely.
     * Acceptable trade-off for single-window (99% of usage).
     */
    private fun observeChatForeground() {
        foregroundObserverJob = serviceScope.launch {
            OverlayController.isChatInForeground.collect { inForeground ->
                val view = overlayView ?: return@collect
                if (inForeground) {
                    if (view.visibility == View.VISIBLE) {
                        view.visibility = View.INVISIBLE
                        Log.d(TAG, "Overlay suppressed: ChatActivity is in foreground (#627)")
                    }
                } else {
                    if (view.visibility == View.INVISIBLE && savedVisibility == NO_SAVED_VISIBILITY) {
                        view.visibility = View.VISIBLE
                        Log.d(TAG, "Overlay restored: ChatActivity left foreground (#627)")
                    } else if (savedVisibility != NO_SAVED_VISIBILITY) {
                        // Screenshot hide was active during chat foreground — update saved state
                        // so restoreOverlayVisibility() restores to VISIBLE, not INVISIBLE
                        savedVisibility = View.VISIBLE
                    }
                }
            }
        }
    }

    /**
     * Launch [ChatActivity] in full-screen mode, bringing it to the front.
     *
     * Uses [Intent.FLAG_ACTIVITY_NEW_TASK] because activities cannot be started
     * from a Service without a task context. [Intent.FLAG_ACTIVITY_SINGLE_TOP]
     * reuses an existing ChatActivity instance if already in the back stack,
     * delivering the intent via [ChatActivity.onNewIntent] (no special handling
     * needed — the default behavior of bringing the activity to the foreground
     * is sufficient).
     *
     * Note: [startActivity] dispatches the intent to the system synchronously;
     * the activity will launch even if [stopSelf] is called immediately after.
     *
     * Called when the user taps "Full" on the overlay mini-chat.
     */
    private fun launchChatActivity() {
        val intent = Intent(this, ChatActivity::class.java).apply {
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
        }
        startActivity(intent)
    }

    private fun showOverlay() {
        val wm = windowManager ?: return

        if (!OverlayPermission.canDrawOverlays(this)) {
            Log.w(TAG, "Cannot draw overlays — permission not granted")
            stopSelf()
            return
        }

        val mode = OverlayController.surfaceMode.value
        currentMode = mode
        val params = buildLayoutParams(mode)
        overlayParams = params

        val composeView = ComposeView(this).apply {
            setViewTreeLifecycleOwner(this@OverlayService)
            setViewTreeSavedStateRegistryOwner(this@OverlayService)
            // Exclude overlay from accessibility tree so ScreenReader reads the
            // underlying app, not overlay elements. Without this, the agentic loop
            // sees Citros UI elements (overlay) instead of the target app (#431).
            importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
            setContent {
                CitrosChatTheme(themeMode = selectedThemeMode, flavor = selectedFlavor) {
                    OverlayServiceContent(selectedFlavor)
                }
            }
        }

        // Wrap ComposeView in DraggableOverlayFrame to intercept drag gestures
        // before Compose consumes them. ComposeView is a ViewGroup that dispatches
        // touch events to Compose children first, preventing View-level touch listeners
        // from ever firing. DraggableOverlayFrame uses onInterceptTouchEvent to steal
        // the gesture once drag threshold is exceeded.
        val dragFrame = DraggableOverlayFrame(this).apply {
            // Lifecycle owners must be on the root view added to WindowManager.
            // Compose walks UP the view tree to find them; ComposeView is a child
            // of this frame, so it finds DraggableOverlayFrame first.
            setViewTreeLifecycleOwner(this@OverlayService)
            setViewTreeSavedStateRegistryOwner(this@OverlayService)
            addView(composeView)
            overlayParams = params
            callback = createDragCallback()
            // Only enable drag interception in bubble mode. In mini-chat mode,
            // Compose scroll (LazyColumn) conflicts with frame-level drag via
            // requestDisallowInterceptTouchEvent.
            dragEnabled = mode == OverlaySurfaceMode.BUBBLE
        }
        overlayView = dragFrame

        try {
            wm.addView(dragFrame, params)
            setupImeDetection(dragFrame)
        } catch (e: Exception) {
            Log.e(TAG, "Failed to add overlay view", e)
            stopSelf()
        }
    }

    /** Saved visibility state before [hideOverlayForScreenshot], or [NO_SAVED_VISIBILITY] if not saved. */
    private var savedVisibility: Int = NO_SAVED_VISIBILITY

    /**
     * When true, the overlay is hidden for the entire tool loop duration.
     * [restoreOverlayVisibility] becomes a no-op while this is set,
     * preventing per-screenshot restore from re-showing the overlay mid-loop.
     */
    private var toolLoopHideActive = false

    /**
     * Hide the overlay for the duration of a tool loop (#626, #646).
     *
     * Sets the view to [View.INVISIBLE] and adds
     * [FLAG_NOT_TOUCHABLE][WindowManager.LayoutParams.FLAG_NOT_TOUCHABLE] so the
     * InputDispatcher skips this window entirely. Without FLAG_NOT_TOUCHABLE, an
     * INVISIBLE overlay window still consumes [dispatchGesture][android.accessibilityservice.AccessibilityService.dispatchGesture]
     * taps -- preventing the agent from tapping elements in the app underneath
     * (bottom nav tabs, list items, etc.).
     *
     * Per-screenshot hide/restore calls ([hideOverlayForScreenshot] /
     * [restoreOverlayVisibility]) become no-ops while the tool loop hide is active,
     * ensuring the overlay stays hidden and non-touchable for the full loop duration.
     *
     * Call [restoreAfterToolLoop] to undo.
     */
    @MainThread
    fun hideForToolLoop() {
        if (toolLoopHideActive) return
        toolLoopHideActive = true
        // If overlayView is null (service not fully initialized), the flag is still
        // set so that hideOverlayForScreenshot/restoreOverlayVisibility become no-ops.
        // restoreAfterToolLoop will clear the flag harmlessly.
        overlayView?.let { view ->
            if (savedVisibility == NO_SAVED_VISIBILITY) {
                savedVisibility = view.visibility
            }
            view.visibility = View.INVISIBLE
            makeWindowNotTouchable(true)
        }
    }

    /**
     * Restore overlay after a tool loop completes.
     * Counterpart to [hideForToolLoop].
     */
    @MainThread
    @android.annotation.SuppressLint("WrongConstant")
    fun restoreAfterToolLoop() {
        if (!toolLoopHideActive) return
        toolLoopHideActive = false
        overlayView?.let { view ->
            if (OverlayController.isChatInForeground.value) {
                Log.d(TAG, "restoreAfterToolLoop: skipped -- ChatActivity still in foreground")
                savedVisibility = NO_SAVED_VISIBILITY
                // Clear FLAG_NOT_TOUCHABLE even though view stays INVISIBLE.
                // Safe: ChatActivity is the full-screen foreground window, so
                // it covers the overlay -- no touches reach this window anyway.
                // When ChatActivity leaves, observeChatForeground restores visibility.
                makeWindowNotTouchable(false)
                return
            }
            view.visibility = if (savedVisibility != NO_SAVED_VISIBILITY) savedVisibility else View.VISIBLE
            savedVisibility = NO_SAVED_VISIBILITY
            makeWindowNotTouchable(false)
        }
    }

    /**
     * Temporarily hide the overlay view so it doesn't appear in screenshots.
     * Uses [View.INVISIBLE] (not GONE) to preserve the view's layout position
     * in the window manager, avoiding re-layout on restore.
     * Call [restoreOverlayVisibility] to restore the previous state.
     *
     * No-op while [hideForToolLoop] is active -- the overlay is already hidden
     * and must stay that way until the tool loop ends.
     *
     * Guarded against double-hide: if already hidden (savedVisibility is set),
     * subsequent calls are no-ops to preserve the original visibility state.
     */
    fun hideOverlayForScreenshot() {
        if (toolLoopHideActive) return
        overlayView?.let {
            if (savedVisibility == NO_SAVED_VISIBILITY) {
                savedVisibility = it.visibility
                it.visibility = View.INVISIBLE
            }
        }
    }

    /**
     * Restore overlay visibility to the state before [hideOverlayForScreenshot].
     *
     * No-op while [hideForToolLoop] is active -- the tool loop owns the
     * visibility state until [restoreAfterToolLoop] is called.
     */
    @android.annotation.SuppressLint("WrongConstant")
    fun restoreOverlayVisibility() {
        if (toolLoopHideActive) return
        overlayView?.let {
            // Re-check: don't restore if ChatActivity is still in foreground (#627 race fix)
            if (OverlayController.isChatInForeground.value) {
                Log.d(TAG, "restoreOverlayVisibility: skipped — ChatActivity still in foreground")
                // Safe to clear: overlay is binary VISIBLE/INVISIBLE, and the
                // chat-foreground observer will handle restoring visibility when
                // ChatActivity eventually leaves the foreground.
                savedVisibility = NO_SAVED_VISIBILITY
                return
            }
            it.visibility = if (savedVisibility != NO_SAVED_VISIBILITY) savedVisibility else View.VISIBLE
            savedVisibility = NO_SAVED_VISIBILITY
        }
    }

    /**
     * Toggle [FLAG_NOT_TOUCHABLE][WindowManager.LayoutParams.FLAG_NOT_TOUCHABLE] on
     * the overlay window. When [notTouchable] is true the InputDispatcher will skip
     * this window entirely, allowing gestures to fall through to windows behind it.
     */
    private fun makeWindowNotTouchable(notTouchable: Boolean) {
        val wm = windowManager ?: return
        val view = overlayView ?: return
        val params = overlayParams ?: return
        if (notTouchable) {
            params.flags = params.flags or WindowManager.LayoutParams.FLAG_NOT_TOUCHABLE
        } else {
            params.flags = params.flags and WindowManager.LayoutParams.FLAG_NOT_TOUCHABLE.inv()
        }
        try {
            wm.updateViewLayout(view, params)
        } catch (e: Exception) {
            Log.w(TAG, "makeWindowNotTouchable($notTouchable) failed: ${e.message}")
        }
    }

    /**
     * Move the mini-chat overlay to the top of the screen.
     * Called when the queue input TextField gains focus (#451) or
     * after a drag-to-top / swipe-up gesture (#408).
     * Resets x/y offsets so gravity-based positioning takes over.
     */
    fun moveOverlayToTop() {
        Log.d(TAG, "moveOverlayToTop called, view=${overlayView != null}, params=${overlayParams != null}")
        val view = overlayView ?: return
        val params = overlayParams ?: return
        val wm = windowManager ?: return
        // Use VERTICAL_GRAVITY_MASK to correctly isolate vertical gravity.
        // Gravity.TOP=0x30 and Gravity.BOTTOM=0x50 share bit 0x10, so a bare
        // bitwise AND incorrectly detects TOP when gravity is actually BOTTOM.
        val verticalGravity = params.gravity and Gravity.VERTICAL_GRAVITY_MASK
        val needsUpdate = verticalGravity != Gravity.TOP || params.x != 0 || params.y != 0
        if (!needsUpdate) return
        params.gravity = Gravity.TOP or Gravity.CENTER_HORIZONTAL
        params.x = 0
        params.y = 0
        try {
            wm.updateViewLayout(view, params)
        } catch (e: Exception) {
            Log.w(TAG, "moveOverlayToTop failed: ${e.message}")
        }
    }

    /**
     * Move the mini-chat overlay back to the bottom of the screen.
     * Called when the queue input TextField loses focus (#451) or
     * after a drag/snap gesture (#408).
     * Resets x/y offsets so gravity-based positioning takes over.
     */
    fun moveOverlayToBottom() {
        val view = overlayView ?: return
        val params = overlayParams ?: return
        val wm = windowManager ?: return
        // Use VERTICAL_GRAVITY_MASK — see moveOverlayToTop for why bare AND fails.
        val verticalGravity = params.gravity and Gravity.VERTICAL_GRAVITY_MASK
        val needsUpdate = verticalGravity != Gravity.BOTTOM || params.x != 0 || params.y != 0
        if (!needsUpdate) return
        params.gravity = Gravity.BOTTOM or Gravity.CENTER_HORIZONTAL
        params.x = 0
        params.y = 0
        try {
            wm.updateViewLayout(view, params)
        } catch (e: Exception) {
            Log.w(TAG, "moveOverlayToBottom failed: ${e.message}")
        }
    }

    /**
     * Remove the overlay view from WindowManager and dispose its Compose composition.
     *
     * Calls [ComposeView.disposeComposition] to prevent memory leaks (#3).
     */
    private fun removeOverlay() {
        overlayView?.let { view ->
            // Dispose Compose composition before removing from WindowManager.
            // overlayView is a DraggableOverlayFrame, so find the ComposeView child.
            (view as? DraggableOverlayFrame)?.let { frame ->
                for (i in 0 until frame.childCount) {
                    (frame.getChildAt(i) as? ComposeView)?.disposeComposition()
                }
            }
            try {
                windowManager?.removeView(view)
            } catch (e: Exception) {
                Log.w(TAG, "Failed to remove overlay view", e)
            }
        }
        overlayView = null
        overlayParams = null
    }

    private fun createDragCallback(): DraggableOverlayFrame.Callback {
        return object : DraggableOverlayFrame.Callback {
            override fun onDragMove(x: Int, y: Int) {
                val wm = windowManager ?: return
                val view = overlayView ?: return
                val params = overlayParams ?: return
                try {
                    wm.updateViewLayout(view, params)
                } catch (e: Exception) {
                    Log.w(TAG, "Drag move failed: ${e.message}")
                }
            }
            override fun onDragEnd(velocityY: Float, rawX: Float, rawY: Float) {
                handleDragEnd(velocityY, rawX, rawY)
            }
        }
    }

    private fun handleDragEnd(velocityY: Float, rawX: Float, rawY: Float) {
        val view = overlayView ?: return
        val dm = resources.displayMetrics
        val density = dm.density

        when (currentMode) {
            OverlaySurfaceMode.MINI_CHAT -> {
                val action = OverlayGestureHelper.classifyMiniChatGesture(velocityY, rawX, dm.heightPixels)
                when (action) {
                    MiniChatGestureAction.SNAP_TO_TOP -> moveOverlayToTop()
                    MiniChatGestureAction.SNAP_TO_BOTTOM -> moveOverlayToBottom()
                    MiniChatGestureAction.MINIMIZE_TO_BUBBLE -> {
                        OverlayController.updateSurfaceMode(OverlaySurfaceMode.BUBBLE)
                    }
                }
                if (action != MiniChatGestureAction.SNAP_TO_BOTTOM) {
                    view.performHapticFeedback(HapticFeedbackConstants.LONG_PRESS)
                }
                announceGesture(view, action)
            }
            OverlaySurfaceMode.BUBBLE -> {
                val action = OverlayGestureHelper.classifyBubbleGesture(velocityY)
                when (action) {
                    BubbleGestureAction.EXPAND_TO_MINI_CHAT -> {
                        OverlayController.updateSurfaceMode(OverlaySurfaceMode.MINI_CHAT)
                        OverlayController.resetUnreadCount()
                    }
                    BubbleGestureAction.SNAP_TO_CORNER -> {
                        val bubbleSizePx = (BUBBLE_SIZE_DP * density).toInt()
                        val marginPx = (16 * density).toInt()
                        val bottomMarginPx = (BUBBLE_MARGIN_BOTTOM_DP * density).toInt()
                        val params = overlayParams ?: return
                        val (targetX, targetY) = OverlayGestureHelper.snapBubbleToCorner(
                            releaseX = params.x,
                            releaseY = params.y,
                            screenWidth = dm.widthPixels,
                            screenHeight = dm.heightPixels,
                            bubbleSizePx = bubbleSizePx,
                            marginPx = marginPx,
                            bottomMarginPx = bottomMarginPx
                        )
                        animateToPosition(view, params, targetX, targetY)
                    }
                }
                view.performHapticFeedback(HapticFeedbackConstants.LONG_PRESS)
                announceGesture(view, action)
            }
            else -> {}
        }
    }

    private fun buildLayoutParams(mode: OverlaySurfaceMode): WindowManager.LayoutParams {
        val dm = resources.displayMetrics
        val density = dm.density

        return when (mode) {
            OverlaySurfaceMode.MINI_CHAT -> {
                val height = (dm.heightPixels * MINI_CHAT_HEIGHT_FRACTION).toInt()
                WindowManager.LayoutParams(
                    WindowManager.LayoutParams.MATCH_PARENT,
                    height,
                    WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY,
                    WindowManager.LayoutParams.FLAG_NOT_TOUCH_MODAL or
                        WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN or
                        WindowManager.LayoutParams.FLAG_HARDWARE_ACCELERATED,
                    PixelFormat.TRANSLUCENT
                ).apply {
                    gravity = Gravity.BOTTOM or Gravity.CENTER_HORIZONTAL
                    // Pan the overlay so the focused TextField stays visible above
                    // the keyboard. ADJUST_RESIZE caused flickering on Pixel 10 Pro
                    // (see #401); ADJUST_PAN lets the system shift the view without
                    // re-layout, which avoids that issue.
                    softInputMode = WindowManager.LayoutParams.SOFT_INPUT_ADJUST_NOTHING
                    x = 0
                    y = 0
                }
            }
            OverlaySurfaceMode.BUBBLE -> {
                val sizePx = (BUBBLE_SIZE_DP * density).toInt()
                val marginPx = (16 * density).toInt()
                WindowManager.LayoutParams(
                    sizePx,
                    sizePx,
                    WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY,
                    WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE or
                        WindowManager.LayoutParams.FLAG_LAYOUT_NO_LIMITS or
                        WindowManager.LayoutParams.FLAG_HARDWARE_ACCELERATED,
                    PixelFormat.TRANSLUCENT
                ).apply {
                    gravity = Gravity.TOP or Gravity.START
                    x = dm.widthPixels - sizePx - marginPx
                    y = calculateBubbleBaseY(dm)
                }
            }
            OverlaySurfaceMode.FULL_APP -> {
                // Should not reach here — FULL_APP stops the service
                buildLayoutParams(OverlaySurfaceMode.MINI_CHAT)
            }
        }
    }

    private fun updateOverlayLayout(mode: OverlaySurfaceMode) {
        val wm = windowManager ?: return
        val view = overlayView ?: return
        currentMode = mode
        val newParams = buildLayoutParams(mode)
        overlayParams = newParams
        try {
            wm.updateViewLayout(view, newParams)
            // Update DraggableOverlayFrame's params reference and drag state for the new mode
            (view as? DraggableOverlayFrame)?.apply {
                overlayParams = newParams
                dragEnabled = mode == OverlaySurfaceMode.BUBBLE
            }
        } catch (e: Exception) {
            Log.w(TAG, "Failed to update overlay layout", e)
            // Recreate if update fails
            removeOverlay()
            showOverlay()
        }
    }

    /**
     * Calculate the default bottom-right Y position for the bubble overlay.
     */
    private fun calculateBubbleBaseY(dm: android.util.DisplayMetrics): Int =
        calculateBubbleBaseY(dm.heightPixels, dm.density)

    // Animation constants sourced from OverlayGestureHelper for consistency.

    /**
     * Animate the overlay view from its current position to (targetX, targetY)
     * using a decelerate interpolator for a natural spring-like feel (#408).
     */
    private fun animateToPosition(
        view: View,
        params: WindowManager.LayoutParams,
        targetX: Int,
        targetY: Int
    ) {
        val wm = windowManager ?: return
        val startX = params.x
        val startY = params.y
        val interpolator = DecelerateInterpolator(OverlayGestureHelper.DECELERATE_FACTOR)
        val startTime = System.currentTimeMillis()

        animationJob?.cancel() // Cancel any in-flight animation to prevent conflicts
        animationJob = serviceScope.launch {
            var elapsed = 0L
            while (elapsed < OverlayGestureHelper.SNAP_ANIMATION_DURATION_MS) {
                elapsed = System.currentTimeMillis() - startTime
                val fraction = interpolator.getInterpolation(
                    (elapsed.toFloat() / OverlayGestureHelper.SNAP_ANIMATION_DURATION_MS).coerceIn(0f, 1f)
                )
                params.x = startX + ((targetX - startX) * fraction).toInt()
                params.y = startY + ((targetY - startY) * fraction).toInt()
                try {
                    wm.updateViewLayout(view, params)
                } catch (e: Exception) {
                    Log.w(TAG, "animateToPosition: layout update failed", e)
                    return@launch
                }
                kotlinx.coroutines.delay(OverlayGestureHelper.ANIMATION_FRAME_INTERVAL_MS)
            }
            // Ensure final position is exact
            params.x = targetX
            params.y = targetY
            try {
                wm.updateViewLayout(view, params)
            } catch (e: Exception) {
                Log.w(TAG, "animateToPosition: final layout update failed", e)
            }
        }
    }


    /**
     * Detect keyboard (IME) show/hide via WindowInsetsAnimation.Callback (API 30+).
     * More reliable than TextField.onFocusChanged because:
     * - Focus doesn't change when keyboard is dismissed via back gesture
     * - Focus doesn't re-trigger when tapping an already-focused TextField
     * - Works consistently over both the host app and other apps
     */
    private fun setupImeDetection(view: android.view.View) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            view.setWindowInsetsAnimationCallback(
                object : WindowInsetsAnimation.Callback(DISPATCH_MODE_CONTINUE_ON_SUBTREE) {
                    override fun onProgress(
                        insets: WindowInsets,
                        runningAnimations: MutableList<WindowInsetsAnimation>
                    ): WindowInsets = insets

                    override fun onEnd(animation: WindowInsetsAnimation) {
                        super.onEnd(animation)
                        val rootInsets = view.rootWindowInsets ?: return
                        val imeVisible = rootInsets.isVisible(WindowInsets.Type.ime())
                        Log.d(TAG, "IME animation ended: imeVisible=$imeVisible, lastImeVisible=$lastImeVisible, mode=$currentMode")
                        if (imeVisible != lastImeVisible) {
                            lastImeVisible = imeVisible
                            if (currentMode == OverlaySurfaceMode.MINI_CHAT) {
                                if (imeVisible) moveOverlayToTop() else moveOverlayToBottom()
                            }
                        }
                    }
                }
            )
            Log.d(TAG, "IME detection registered via WindowInsetsAnimation.Callback")
        } else {
            Log.w(TAG, "IME detection requires API 30+, current: ${Build.VERSION.SDK_INT}")
        }
    }

    /** Announce gesture result to TalkBack users (#408). */
    private fun announceGesture(view: View, action: MiniChatGestureAction) {
        val text = when (action) {
            MiniChatGestureAction.SNAP_TO_TOP -> "Docked to top"
            MiniChatGestureAction.SNAP_TO_BOTTOM -> return // No announcement for default position
            MiniChatGestureAction.MINIMIZE_TO_BUBBLE -> "Minimized to bubble"
        }
        view.announceForAccessibility(text)
    }

    /** Announce gesture result to TalkBack users (#408). */
    private fun announceGesture(view: View, action: BubbleGestureAction) {
        val text = when (action) {
            BubbleGestureAction.SNAP_TO_CORNER -> "Snapped to corner"
            BubbleGestureAction.EXPAND_TO_MINI_CHAT -> "Expanded to mini chat"
        }
        view.announceForAccessibility(text)
    }

    private fun createNotificationChannel() {
        val channel = NotificationChannel(
            CHANNEL_ID,
            "Citros Overlay",
            NotificationManager.IMPORTANCE_LOW
        ).apply {
            description = "Shows while Citros is controlling your phone"
            setShowBadge(false)
        }
        // Null-safe system service access (#7)
        val nm = getSystemService(NotificationManager::class.java)
        if (nm != null) {
            nm.createNotificationChannel(channel)
        } else {
            Log.e(TAG, "NotificationManager not available — cannot create channel")
        }
    }

    private fun buildNotification(statusText: String): Notification {
        val openIntent = packageManager.getLaunchIntentForPackage(packageName)?.apply {
            flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP
        }
        val openPending = openIntent?.let {
            PendingIntent.getActivity(
                this, 0, it,
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
            )
        }

        val stopPendingIntent = PendingIntent.getService(
            this, 1,
            Intent(this, OverlayService::class.java).apply { action = ACTION_STOP },
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )

        return Notification.Builder(this, CHANNEL_ID)
            .setContentTitle("Citros")
            .setContentText(statusText)
            .setSmallIcon(android.R.drawable.ic_dialog_info)
            .apply { openPending?.let { setContentIntent(it) } }
            .addAction(Notification.Action.Builder(null, "Stop", stopPendingIntent).build())
            .setOngoing(true)
            .build()
    }

    private fun updateNotification(mode: OverlaySurfaceMode) {
        val statusText = when (mode) {
            OverlaySurfaceMode.MINI_CHAT -> "Executing phone actions — mini-chat active"
            OverlaySurfaceMode.BUBBLE -> "Executing phone actions — tap bubble to expand"
            OverlaySurfaceMode.FULL_APP -> "Citros is running"
        }
        val notification = buildNotification(statusText)
        // Null-safe system service access (#7)
        val nm = getSystemService(NotificationManager::class.java)
        if (nm != null) {
            @android.annotation.SuppressLint("NotificationPermission") // POST_NOTIFICATIONS checked in ChatActivity before starting service
            nm.notify(NOTIFICATION_ID, notification)
        } else {
            Log.e(TAG, "NotificationManager not available — cannot update notification")
        }
    }
}

private fun readThemeMode(context: Context): String {
    val prefs = context.getSharedPreferences(ONBOARDING_PREFS, Context.MODE_PRIVATE)
    return prefs.getString(PREF_THEME_MODE, THEME_MODE_DEFAULT) ?: THEME_MODE_DEFAULT
}

/**
 * Root composable rendered inside the overlay service's [ComposeView].
 *
 * Observes [OverlayController] flows and delegates to the appropriate
 * overlay composable based on the current surface mode.
 *
 * @param flavor The selected [CitrosFlavor] for theming consistency with the main app.
 */
@Composable
private fun OverlayServiceContent(flavor: CitrosFlavor) {
    val overlayState by OverlayController.overlayState.collectAsState()
    val surfaceMode by OverlayController.surfaceMode.collectAsState()
    val queuedMessage by OverlayController.queuedMessage.collectAsState()
    val unreadCount by OverlayController.unreadCount.collectAsState()
    val toolStatus by OverlayController.currentToolStatus.collectAsState()

    // Priority order for step label:
    // 1. Live tool status during execution (most current — from onToolStarted)
    // 2. "Waiting..." when no steps exist yet
    // 3. Current step from overlayState (historical — from completed tool results)
    val currentStep = run {
        val liveLabel = toolStatus
        if (liveLabel != null && overlayState.runState == ai.citros.core.OverlayRunState.EXECUTING) {
            val stepNum = overlayState.steps.size + 1
            ai.citros.core.OverlayStep(step = stepNum, total = 0, label = liveLabel)
        } else if (overlayState.steps.isEmpty()) {
            ai.citros.core.OverlayStep(step = 0, total = 0, label = "Waiting...")
        } else {
            val idx = overlayState.currentStepIndex.coerceIn(0, overlayState.steps.lastIndex)
            overlayState.steps[idx]
        }
    }

    // Build lines including queued message if present
    val lines = buildList {
        addAll(overlayState.lines)
        if (!queuedMessage.isNullOrBlank()) {
            add(
                ai.citros.core.OverlayLine(
                    id = (overlayState.lines.maxOfOrNull { it.id } ?: 0) + 1,
                    type = ai.citros.core.OverlayLineType.QUEUED,
                    text = queuedMessage!!
                )
            )
        }
    }

    var queuedDraft by androidx.compose.runtime.remember { mutableStateOf(queuedMessage.orEmpty()) }

    // Sync external queued message changes into the draft
    androidx.compose.runtime.LaunchedEffect(queuedMessage) {
        queuedDraft = queuedMessage.orEmpty()
    }

    when (surfaceMode) {
        OverlaySurfaceMode.MINI_CHAT -> {
            OverlayMiniChatContent(
                flavor = flavor,
                runState = overlayState.runState,
                currentStep = currentStep,
                lines = lines,
                queuedMessageDraft = queuedDraft,
                onQueuedDraftChange = {
                    queuedDraft = it
                    // Don't set OverlayController.queuedMessage on every keystroke —
                    // only on explicit Queue tap (#445). Draft is local UI state.
                },
                onSubmitQueuedMessage = {
                    if (queuedDraft.isNotBlank()) {
                        OverlayController.dispatch(OverlayAction.QueueMessage(queuedDraft))
                        queuedDraft = "" // Clear input after submission
                    }
                },
                onStopAction = {
                    OverlayController.dispatch(OverlayAction.StopExecution)
                },
                onResumeOrRetry = {
                    OverlayController.dispatch(OverlayAction.ResumeExecution)
                },
                onOpenFull = {
                    OverlayController.updateSurfaceMode(OverlaySurfaceMode.FULL_APP)
                },
                onOpenBubble = {
                    OverlayController.updateSurfaceMode(OverlaySurfaceMode.BUBBLE)
                }
            )
        }
        OverlaySurfaceMode.BUBBLE -> {
            OverlayBubbleContent(
                flavor = flavor,
                runState = overlayState.runState,
                unreadCount = unreadCount,
                onExpand = {
                    OverlayController.updateSurfaceMode(OverlaySurfaceMode.MINI_CHAT)
                    OverlayController.resetUnreadCount()
                },
                onStopAction = {
                    OverlayController.dispatch(OverlayAction.StopExecution)
                },
                onDismiss = {
                    OverlayController.deactivateOverlay()
                }
            )
        }
        OverlaySurfaceMode.FULL_APP -> {
            // Service should not be running in FULL_APP mode
        }
    }
}
