package ai.citros.chat

import ai.citros.core.OverlayRunState
import ai.citros.core.OverlayState
import androidx.annotation.MainThread
import java.util.concurrent.atomic.AtomicLong
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * Surface mode for the overlay display.
 *
 * - [FULL_APP]: Normal Activity mode — overlay service is not running.
 * - [PANEL]: Floating overlay (~40% height, bottom-anchored, draggable).
 * - [SEARCH_BAR]: Docked search bar overlay in the Pixel bottom search slot.
 * - [DYNAMIC_ISLAND]: Top-centered compact status island.
 */
enum class OverlaySurfaceMode {
    FULL_APP,
    PANEL,
    SEARCH_BAR,
    DYNAMIC_ISLAND;

    /** Serialize to the SharedPreferences string representation. */
    fun toPrefValue(): String = when (this) {
        FULL_APP -> "full_app"
        PANEL -> "panel"
        SEARCH_BAR -> "search_bar"
        DYNAMIC_ISLAND -> "dynamic_island"
    }

    companion object {
        /** Deserialize from SharedPreferences string, defaulting to [SEARCH_BAR]. */
        fun fromPrefValue(value: String?): OverlaySurfaceMode = when (value) {
            "bubble", "search_bar" -> SEARCH_BAR
            "full_app" -> FULL_APP
            "mini_chat", "panel" -> PANEL
            "dynamic_island" -> DYNAMIC_ISLAND
            else -> SEARCH_BAR
        }
    }
}

/**
 * Minimal intent for whether the overlay should force the panel open.
 *
 * Keep this coarse to avoid conflicting booleans.
 */
enum class OverlayInteractionDemand {
    NONE,
    INPUT_REQUIRED,
    PERMISSION_REQUIRED,
    ERROR_ACTION_REQUIRED
}

/**
 * Shared state bridge between [ChatViewModel] and [OverlayService].
 *
 * Enforces unidirectional data flow (#463):
 * - **Actions flow up:** OverlayService dispatches [OverlayAction] via [dispatch].
 *   ChatActivity collects [actions] and routes them to ChatViewModel or back
 *   to this controller's internal update methods.
 * - **State flows down:** ChatActivity observes ChatViewModel state and calls
 *   [updateOverlayState], [updateSurfaceMode], etc. OverlayService reads
 *   [overlayState], [surfaceMode], etc. via [StateFlow.collectAsState].
 *
 * OverlayService MUST NOT call update methods directly — only [dispatch].
 * ChatActivity is the sole mediator between ViewModel and overlay state.
 */
object OverlayController {

    /**
     * Debounce window for [activateOverlay] to prevent double-activation (#437).
     * During tool execution, activateOverlay() is called from two code paths:
     * 1. Tool start (immediate, via LaunchedEffect on isLoading)
     * 2. Screenshot capture (~100-200ms later, via tool status update)
     * 500ms provides a safe margin while remaining imperceptible to users.
     */
    private const val ACTIVATE_DEBOUNCE_MS = 500L
    private const val AUTO_SURFACE_SWITCH_COOLDOWN_MS = 800L

    /** Clock source for testability. Override in tests to control time. */
    internal var clock: () -> Long = { System.currentTimeMillis() }

    /** Timestamp of last successful [activateOverlay] call. Thread-safe via CAS. */
    private val lastActivateTimestampMs = AtomicLong(0L)
    private val lastAutoSurfaceSwitchMs = AtomicLong(0L)

    /** Read-only state flows observed by OverlayService. */

    private val _overlayState = MutableStateFlow(OverlayState.EMPTY)
    /** Current overlay state derived from ChatViewModel messages and loading state. */
    val overlayState: StateFlow<OverlayState> = _overlayState.asStateFlow()

    private val _surfaceMode = MutableStateFlow(OverlaySurfaceMode.FULL_APP)
    /** Current surface mode (FULL_APP, PANEL, SEARCH_BAR, DYNAMIC_ISLAND). */
    val surfaceMode: StateFlow<OverlaySurfaceMode> = _surfaceMode.asStateFlow()

    private val _isOverlayActive = MutableStateFlow(false)
    /** Whether the overlay service should be running. */
    val isOverlayActive: StateFlow<Boolean> = _isOverlayActive.asStateFlow()

    private val _isChatInForeground = MutableStateFlow(false)
    /**
     * Whether [ChatActivity] is currently in the foreground.
     * When true, [OverlayService] suppresses overlay visibility to avoid
     * redundantly showing the overlay on top of the full-screen chat (#627).
     */
    val isChatInForeground: StateFlow<Boolean> = _isChatInForeground.asStateFlow()

    private val _queuedMessage = MutableStateFlow<String?>(null)
    /** Queued follow-up message text. */
    val queuedMessage: StateFlow<String?> = _queuedMessage.asStateFlow()

    private val _unreadCount = MutableStateFlow(0)
    /** Unread message count for search bar + island badges. */
    val unreadCount: StateFlow<Int> = _unreadCount.asStateFlow()

    private val _currentToolStatus = MutableStateFlow<String?>(null)
    /**
     * Live tool execution status label (e.g. "Opening app...", "Searching the web...").
     * Set via [updateToolStatus] when a tool starts, cleared when the tool completes
     * or the execution loop ends.
     */
    val currentToolStatus: StateFlow<String?> = _currentToolStatus.asStateFlow()

    private val _interactionDemand = MutableStateFlow(OverlayInteractionDemand.NONE)
    /** Whether current execution needs explicit user interaction in panel mode. */
    val interactionDemand: StateFlow<OverlayInteractionDemand> = _interactionDemand.asStateFlow()

    private val _userPanelPinned = MutableStateFlow(false)
    /** True when the user explicitly opened/pinned the panel. */
    val userPanelPinned: StateFlow<Boolean> = _userPanelPinned.asStateFlow()

    private val _useIslandWhenIdle = MutableStateFlow(true)
    /** Whether idle overlay should prefer Dynamic Island over Search Bar. */
    val useIslandWhenIdle: StateFlow<Boolean> = _useIslandWhenIdle.asStateFlow()

    private val _showSearchBarWhenIdle = MutableStateFlow(true)
    /** Whether Search Bar is allowed as idle fallback when island mode is disabled. */
    val showSearchBarWhenIdle: StateFlow<Boolean> = _showSearchBarWhenIdle.asStateFlow()

    /** Action dispatch: written by OverlayService, collected by ChatActivity. */

    /**
     * Buffer capacity for the action flow. Sized to absorb a burst of rapid
     * user taps (e.g. Stop → Queue → Search Bar) without blocking the UI thread.
     * 16 is generous — normal usage is 1-2 actions per user gesture.
     */
    private const val ACTION_BUFFER_CAPACITY: Int = 16

    private val _actions = MutableSharedFlow<OverlayAction>(extraBufferCapacity = ACTION_BUFFER_CAPACITY)
    /** Actions dispatched from the overlay UI. ChatActivity collects this. */
    val actions: SharedFlow<OverlayAction> = _actions.asSharedFlow()

    /**
     * Dispatch an action from the overlay UI.
     *
     * This is the ONLY write method OverlayService should call.
     * ChatActivity collects [actions] and routes them appropriately.
     */
    fun dispatch(action: OverlayAction) {
        if (!_actions.tryEmit(action)) {
            android.util.Log.e("OverlayController", "Dropped action: $action (buffer full)")
        }
    }

    /** Internal update methods — called only by the ChatActivity mediator. */

    /**
     * Update the overlay state from ChatViewModel.
     * Called by ChatActivity when messages or loading state changes.
     */
    internal fun updateOverlayState(state: OverlayState) {
        _overlayState.value = state
        reconcileSurfaceMode()
    }

    /**
     * Update the surface mode.
     * Called by ChatActivity mediator when processing [OverlayAction.SetSurfaceMode].
     */
    internal fun updateSurfaceMode(mode: OverlaySurfaceMode, fromUser: Boolean = false) {
        _surfaceMode.value = mode
        if (mode == OverlaySurfaceMode.FULL_APP) {
            _isOverlayActive.value = false
            _userPanelPinned.value = false
            return
        }
        if (fromUser) {
            _userPanelPinned.value = mode == OverlaySurfaceMode.PANEL
        } else if (mode != OverlaySurfaceMode.PANEL) {
            _userPanelPinned.value = false
        }
    }

    /**
     * Activate the overlay. Sets [isOverlayActive] to true and switches
     * to the configured idle surface mode if currently in FULL_APP.
     * Called by ChatActivity when tool execution starts.
     *
     * Uses [AtomicLong.compareAndSet] to prevent concurrent double-activation (#437).
     * If called within [ACTIVATE_DEBOUNCE_MS] of the previous activation, the call
     * is skipped. Thread-safe: CAS ensures only one caller wins in a race.
     *
     * @return true if activation succeeded, false if skipped due to debounce
     */
    internal fun activateOverlay(): Boolean {
        val now = clock()
        val last = lastActivateTimestampMs.get()
        if (now - last < ACTIVATE_DEBOUNCE_MS) {
            return false
        }
        if (!lastActivateTimestampMs.compareAndSet(last, now)) {
            // Another thread won the CAS race — skip this call
            return false
        }
        _isOverlayActive.value = true
        if (_surfaceMode.value == OverlaySurfaceMode.FULL_APP) {
            _surfaceMode.value = preferredIdleSurfaceMode()
        }
        reconcileSurfaceMode(force = true)
        return true
    }

    /**
     * Deactivate the overlay. Resets to FULL_APP and inactive.
     * Called by ChatActivity mediator when processing [OverlayAction.Deactivate].
     */
    internal fun deactivateOverlay() {
        _isOverlayActive.value = false
        _surfaceMode.value = OverlaySurfaceMode.FULL_APP
        _userPanelPinned.value = false
        _interactionDemand.value = OverlayInteractionDemand.NONE
    }

    /** Update the queued message text. Called by ChatActivity mediator. */
    internal fun updateQueuedMessage(text: String?) {
        _queuedMessage.value = text?.takeIf { it.isNotBlank() }
        reconcileSurfaceMode()
    }

    /** Update the unread count. Called by ChatActivity mediator. */
    internal fun updateUnreadCount(count: Int) {
        _unreadCount.value = count.coerceAtLeast(0)
    }

    internal fun updateToolStatus(status: String?) {
        _currentToolStatus.value = status
    }

    /**
     * Update whether ChatActivity is in the foreground.
     * Called from ChatActivity.onResume() (true) and onPause() (false).
     * OverlayService observes this to suppress overlay visibility (#627).
     */
    @MainThread
    internal fun setChatInForeground(inForeground: Boolean) {
        _isChatInForeground.value = inForeground
    }

    internal fun updateInteractionDemand(demand: OverlayInteractionDemand) {
        _interactionDemand.value = demand
        reconcileSurfaceMode(force = true)
    }

    internal fun updateIdleSurfacePreference(useIslandWhenIdle: Boolean) {
        _useIslandWhenIdle.value = useIslandWhenIdle
        reconcileSurfaceMode(force = true)
    }

    internal fun updateSearchBarIdlePreference(showSearchBarWhenIdle: Boolean) {
        _showSearchBarWhenIdle.value = showSearchBarWhenIdle
        reconcileSurfaceMode(force = true)
    }

    internal fun setUserPanelPinned(pinned: Boolean) {
        _userPanelPinned.value = pinned
        reconcileSurfaceMode(force = true)
    }

    internal fun preferredIdleSurfaceMode(): OverlaySurfaceMode =
        when {
            _useIslandWhenIdle.value -> OverlaySurfaceMode.DYNAMIC_ISLAND
            _showSearchBarWhenIdle.value -> OverlaySurfaceMode.SEARCH_BAR
            else -> OverlaySurfaceMode.FULL_APP
        }

    /** Reset unread count to zero. Called by ChatActivity mediator. */
    internal fun resetUnreadCount() {
        _unreadCount.value = 0
    }

    private fun resolveDesiredSurfaceMode(): OverlaySurfaceMode = when {
        _userPanelPinned.value -> OverlaySurfaceMode.PANEL
        _interactionDemand.value != OverlayInteractionDemand.NONE -> OverlaySurfaceMode.PANEL
        !_queuedMessage.value.isNullOrBlank() -> OverlaySurfaceMode.PANEL
        else -> preferredIdleSurfaceMode()
    }

    private fun canAutoSwitchNow(force: Boolean): Boolean {
        if (force) {
            lastAutoSurfaceSwitchMs.set(clock())
            return true
        }
        val now = clock()
        val last = lastAutoSurfaceSwitchMs.get()
        if (now - last < AUTO_SURFACE_SWITCH_COOLDOWN_MS) return false
        return lastAutoSurfaceSwitchMs.compareAndSet(last, now)
    }

    private fun reconcileSurfaceMode(force: Boolean = false) {
        if (!_isOverlayActive.value) return
        val current = _surfaceMode.value
        if (current == OverlaySurfaceMode.FULL_APP) return
        val desired = resolveDesiredSurfaceMode()
        if (desired == current) return
        if (!canAutoSwitchNow(force)) return
        if (desired == OverlaySurfaceMode.FULL_APP) {
            _surfaceMode.value = OverlaySurfaceMode.FULL_APP
            _isOverlayActive.value = false
            _userPanelPinned.value = false
        } else {
            _surfaceMode.value = desired
        }
    }

    /**
     * Reset all state to defaults.
     * Used during testing and sign-out.
     */
    fun reset() {
        _overlayState.value = OverlayState.EMPTY
        _surfaceMode.value = OverlaySurfaceMode.FULL_APP
        _isOverlayActive.value = false
        _queuedMessage.value = null
        _isChatInForeground.value = false
        _unreadCount.value = 0
        _interactionDemand.value = OverlayInteractionDemand.NONE
        _userPanelPinned.value = false
        _useIslandWhenIdle.value = true
        _showSearchBarWhenIdle.value = true
        lastActivateTimestampMs.set(0L)
        lastAutoSurfaceSwitchMs.set(0L)
    }
}
