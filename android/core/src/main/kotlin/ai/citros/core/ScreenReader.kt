package ai.citros.core

import android.accessibilityservice.AccessibilityService
import android.util.Log
import android.accessibilityservice.GestureDescription
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.Path
import android.graphics.Rect
import android.os.Build
import android.os.Bundle
import android.util.Base64
import android.view.Display
import android.view.accessibility.AccessibilityNodeInfo
import android.view.accessibility.AccessibilityWindowInfo
import java.io.ByteArrayOutputStream
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference
import kotlin.coroutines.resume
import kotlin.coroutines.suspendCoroutine

/**
 * Screen reading and control utilities.
 * Works with CitrosAccessibilityService.
 */
object ScreenReader {
    
    private const val TAG = "CitrosScreen"
    private const val PRIVACY_TAG = "CitrosPrivacy"
    private const val SYSTEM_UI_PACKAGE = "com.android.systemui"
    private const val PRIVACY_APP_PLACEHOLDER = PrivacyRedaction.APP_PLACEHOLDER
    
    @Volatile
    private var service: AccessibilityService? = null
    private val startupLock = Any()
    private val privacyListRef = AtomicReference<PrivacyList?>(null)
    private val privacyBlockedPackageOverrideForTests = AtomicReference<String?>(null)
    var privacyList: PrivacyList?
        get() = privacyListRef.get()
        private set(value) {
            privacyListRef.set(value)
        }
    private enum class PrivacyBlockSource {
        READ_SCREEN,
        SCREENSHOT,
        ACTION
    }

    private data class PrivacySignalSnapshot(
        val rootInActiveWindowPackage: String?,
        val foregroundPackage: String?,
        val visiblePackages: List<String?>
    )

    private class PrivacyBlockMetrics {
        private val totalCounter = AtomicInteger(0)
        private val counters = mapOf(
            PrivacyBlockSource.READ_SCREEN to AtomicInteger(0),
            PrivacyBlockSource.SCREENSHOT to AtomicInteger(0),
            PrivacyBlockSource.ACTION to AtomicInteger(0)
        )

        fun emit(source: PrivacyBlockSource) {
            totalCounter.incrementAndGet()
            counters[source]?.incrementAndGet()
        }

        fun reset() {
            totalCounter.set(0)
            counters.values.forEach { it.set(0) }
        }

        fun total(): Int = totalCounter.get()

        fun bySource(source: PrivacyBlockSource): Int = counters[source]?.get() ?: 0

        fun totalCounterRef(): AtomicInteger = totalCounter

        fun sourceCounterRef(source: PrivacyBlockSource): AtomicInteger =
            counters[source] ?: AtomicInteger(0)
    }

    private val privacyMetrics = PrivacyBlockMetrics()
    internal val privacyBlockCounter = privacyMetrics.totalCounterRef()
    internal val privacyReadScreenCounter = privacyMetrics.sourceCounterRef(PrivacyBlockSource.READ_SCREEN)
    internal val privacyScreenshotCounter = privacyMetrics.sourceCounterRef(PrivacyBlockSource.SCREENSHOT)
    internal val privacyActionCounter = privacyMetrics.sourceCounterRef(PrivacyBlockSource.ACTION)

    sealed interface ElementActionResult {
        data object Success : ElementActionResult
        data object PrivacyBlocked : ElementActionResult
        data object ElementNotFound : ElementActionResult
        data object ServiceUnavailable : ElementActionResult
        data object GestureDispatchFailed : ElementActionResult
    }

    /**
     * Delay in milliseconds after hiding the overlay before capturing.
     * Allows the window manager to process the visibility change.
     */
    internal const val SCREENSHOT_OVERLAY_HIDE_DELAY_MS = 200L

    /**
     * Optional hook called when the tool loop starts to hide the overlay.
     *
     * Covers the **entire** tool loop duration (not per-tool-call) — the overlay
     * stays hidden from the first tool call until the loop exits (success, error,
     * or cancellation). This prevents the overlay from intercepting touch gestures
     * dispatched by the accessibility service to the target app (e.g. FABs covered
     * by the overlay bubble). Must be safe to call from any coroutine context.
     *
     * Set by [ChatActivity][ai.citros.chat.ChatActivity] in `onCreate` and
     * cleared in `onDestroy`.
     */
    var toolLoopOverlayHideHook: (suspend () -> Unit)? = null

    /**
     * Optional hook called when the tool loop ends to restore overlay visibility.
     *
     * Covers the **entire** tool loop duration — always called in a `finally` block,
     * even if the loop is cancelled or crashes, guaranteeing the overlay is restored.
     * Paired with [toolLoopOverlayHideHook].
     *
     * Set by [ChatActivity][ai.citros.chat.ChatActivity] in `onCreate` and
     * cleared in `onDestroy`.
     */
    var toolLoopOverlayRestoreHook: (suspend () -> Unit)? = null

    /**
     * Optional hook called before [takeScreenshot] to hide overlays.
     * Set by the chat module so screenshots don't capture the Citros overlay.
     * Must be safe to call from any coroutine context; implementations should
     * switch to Main dispatcher internally if needed.
     *
     * Uses [View.INVISIBLE] (not GONE) to preserve layout — the overlay
     * view keeps its position in the window manager so it can be restored
     * without re-layout.
     */
    var screenshotOverlayHook: (suspend () -> Unit)? = null

    /**
     * Optional hook called after [takeScreenshot] to restore overlay visibility.
     * Always invoked in a finally block, even if capture fails.
     */
    var screenshotOverlayRestoreHook: (suspend () -> Unit)? = null
    
    fun attach(accessibilityService: AccessibilityService) {
        synchronized(startupLock) {
            service = accessibilityService
            resetPrivacyCounters()
        }
    }

    /**
     * Atomically publish both service attachment and privacy-list configuration.
     *
     * This avoids a startup window where other threads could observe an attached
     * service before privacy enforcement is configured.
     */
    fun attach(accessibilityService: AccessibilityService, privacyList: PrivacyList?) {
        synchronized(startupLock) {
            privacyListRef.set(privacyList)
            service = accessibilityService
            resetPrivacyCounters()
        }
    }

    fun configurePrivacyList(list: PrivacyList?) {
        synchronized(startupLock) {
            privacyList = list
        }
    }
    
    fun detach() {
        synchronized(startupLock) {
            service = null
        }
    }
    
    fun isAttached(): Boolean = synchronized(startupLock) { service != null }

    /**
     * Wait for the accessibility service to (re)attach.
     *
     * After a force-stop or process restart, Android may take several seconds to
     * rebind the accessibility service. This method polls [isAttached] at short
     * intervals until the service reconnects or the timeout expires.
     *
     * @param timeoutMs Maximum time to wait (default 5 000 ms)
     * @param pollIntervalMs Polling interval (default 250 ms)
     * @return `true` if the service attached within the timeout, `false` otherwise
     */
    suspend fun waitForAttachment(timeoutMs: Long = 5000L, pollIntervalMs: Long = 250L): Boolean {
        if (isAttached()) return true
        val deadline = System.currentTimeMillis() + timeoutMs
        while (true) {
            kotlinx.coroutines.delay(pollIntervalMs)
            if (isAttached()) return true
            if (System.currentTimeMillis() >= deadline) return false
        }
    }

    /**
     * Get the attached AccessibilityService instance.
     * INTERNAL USE ONLY: Used by ClipboardHelper for paste actions.
     * Do not call directly from other components — use ScreenReader's
     * public methods for screen interaction.
     */
    internal fun getService(): AccessibilityService? = synchronized(startupLock) { service }
    
    /**
     * Get display dimensions in pixels.
     */
    fun getDisplaySize(): Pair<Int, Int> {
        val svc = service ?: return Pair(1080, 2400) // Sensible default
        val dm = svc.resources.displayMetrics
        return Pair(dm.widthPixels, dm.heightPixels)
    }
    
    /**
     * Launch an app by name using PackageManager.
     * Searches launchable apps by label (case-insensitive, partial match).
     */
    fun launchApp(appName: String): Boolean {
        val svc = service ?: return false
        val pm = svc.packageManager
        
        // Query all launchable apps
        val mainIntent = Intent(Intent.ACTION_MAIN).apply {
            addCategory(Intent.CATEGORY_LAUNCHER)
        }
        val launchables = pm.queryIntentActivities(mainIntent, 0)
        
        // Find by label — try exact match first, then contains
        val match = launchables.find {
            it.loadLabel(pm).toString().equals(appName, ignoreCase = true)
        } ?: launchables.find {
            it.loadLabel(pm).toString().contains(appName, ignoreCase = true)
        } ?: return false
        
        val intent = pm.getLaunchIntentForPackage(match.activityInfo.packageName) ?: return false
        intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        svc.startActivity(intent)
        return true
    }
    
    /** Target width for screenshot compression (~720p). */
    private const val SCREENSHOT_TARGET_WIDTH = 720

    /**
     * Take a screenshot and return it as a base64-encoded PNG.
     * Compresses to ~720p width to reduce token usage for vision models.
     * Requires API 30+ (Android 11) for AccessibilityService.takeScreenshot().
     *
     * @return [ScreenshotResult] with success, privacy block, or failure reason
     */
    suspend fun takeScreenshot(): ScreenshotResult {
        val svc = service ?: return ScreenshotResult.Failed("Accessibility service not attached")
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) {
            return ScreenshotResult.Failed("Screenshot requires Android 11+")
        }

        if (getPrivacyBlockedPackage(svc) != null) {
            recordPrivacyBlock(source = PrivacyBlockSource.SCREENSHOT)
            logPrivacyBlock(source = PrivacyBlockSource.SCREENSHOT)
            return ScreenshotResult.PrivacyBlocked
        }

        // Hide overlay before capture so it doesn't appear in the screenshot
        try {
            screenshotOverlayHook?.invoke()
        } catch (e: kotlin.coroutines.cancellation.CancellationException) {
            throw e  // Don't swallow coroutine cancellation
        } catch (e: Exception) {
            Log.w(TAG, "takeScreenshot: overlay hide hook failed: ${e.message}")
        }

        val bitmap = try {
            if (screenshotOverlayHook != null) kotlinx.coroutines.delay(SCREENSHOT_OVERLAY_HIDE_DELAY_MS)
            captureScreen(svc)
        } finally {
            // Always restore overlay, even if capture fails
            try {
                screenshotOverlayRestoreHook?.invoke()
            } catch (e: kotlin.coroutines.cancellation.CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(TAG, "takeScreenshot: overlay restore hook failed: ${e.message}")
            }
        } ?: return ScreenshotResult.Failed("Screenshot capture failed")

        var scaled: Bitmap? = null
        return try {
            scaled = scaleBitmap(bitmap, SCREENSHOT_TARGET_WIDTH)
            val base64 = encodeBitmapToBase64Png(scaled)
            ScreenshotResult.Success(base64)
        } catch (_: Exception) {
            ScreenshotResult.Failed("Screenshot encoding failed")
        } finally {
            if (scaled != null && scaled !== bitmap) scaled.recycle()
            bitmap.recycle()
        }
    }

    /**
     * Capture screen via AccessibilityService.takeScreenshot (API 30+).
     * Returns the raw bitmap from the display.
     */
    private suspend fun captureScreen(svc: AccessibilityService): Bitmap? {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) return null

        return suspendCoroutine { cont ->
            svc.takeScreenshot(
                Display.DEFAULT_DISPLAY,
                svc.mainExecutor,
                object : AccessibilityService.TakeScreenshotCallback {
                    override fun onSuccess(result: AccessibilityService.ScreenshotResult) {
                        val hardwareBuffer = result.hardwareBuffer
                        val bitmap = Bitmap.wrapHardwareBuffer(
                            hardwareBuffer,
                            result.colorSpace
                        )
                        hardwareBuffer.close()
                        cont.resume(bitmap)
                    }

                    override fun onFailure(errorCode: Int) {
                        cont.resume(null)
                    }
                }
            )
        }
    }

    /**
     * Scale bitmap to target width while maintaining aspect ratio.
     * Returns the original bitmap if already at or below target width.
     * Never returns null — always returns either the original or a new scaled bitmap.
     *
     * Visibility: internal for unit testing (see ScreenReaderTest).
     */
    internal fun scaleBitmap(bitmap: Bitmap, targetWidth: Int): Bitmap {
        if (bitmap.width <= targetWidth) return bitmap
        val aspectRatio = bitmap.height.toFloat() / bitmap.width.toFloat()
        val targetHeight = (targetWidth * aspectRatio).toInt()
        return Bitmap.createScaledBitmap(bitmap, targetWidth, targetHeight, true)
    }

    /**
     * Encode a bitmap to base64 PNG string.
     * Uses [Base64.NO_WRAP] to avoid newlines in the output.
     *
     * Visibility: internal for unit testing (see ScreenReaderTest).
     */
    internal fun encodeBitmapToBase64Png(bitmap: Bitmap): String {
        val stream = ByteArrayOutputStream()
        bitmap.compress(Bitmap.CompressFormat.PNG, 100, stream)
        return Base64.encodeToString(stream.toByteArray(), Base64.NO_WRAP)
    }

    /**
     * Get current screen content as structured text.
     *
     * Uses [AccessibilityService.getWindows] to find the best non-overlay application
     * window, so the agent never reads the Citros overlay as "the screen". Falls back
     * to [rootInActiveWindow] when the windows API is unavailable.
     */
    fun getScreenContent(): ScreenContent {
        val svc = service ?: return ScreenContent(elements = emptyList(), packageName = null)
        val snapshot = capturePrivacySignalSnapshot(svc)
        return getScreenContentInternal(svc, snapshot)
    }

    private fun getScreenContentInternal(
        svc: AccessibilityService,
        snapshot: PrivacySignalSnapshot
    ): ScreenContent {
        if (getPrivacyBlockedPackage(snapshot) != null) {
            recordPrivacyBlock(source = PrivacyBlockSource.READ_SCREEN)
            logPrivacyBlock(source = PrivacyBlockSource.READ_SCREEN)
            return ScreenContent(
                elements = emptyList(),
                packageName = PRIVACY_APP_PLACEHOLDER,
                privacyMode = true
            )
        }

        // Try window-aware approach: pick the best app window that isn't us
        val appRoot = findAppWindowRoot(svc)
        if (appRoot != null) {
            val pkg = appRoot.packageName?.toString()
            val elements = mutableListOf<ScreenElement>()
            collectElements(appRoot, elements, 0)
            appRoot.recycle()  // Caller owns the node returned by findAppWindowRoot
            Log.d(TAG, "getScreenContent: window-aware path, package=$pkg, elements=${elements.size}")
            return ScreenContent(elements = elements, packageName = pkg)
        }

        // Fallback: rootInActiveWindow — reject self-package to prevent agent loop reading its own UI
        val root = svc.rootInActiveWindow ?: run {
            Log.w(TAG, "getScreenContent: no windows and no rootInActiveWindow")
            return ScreenContent(elements = emptyList(), packageName = null)
        }
        val pkg = root.packageName?.toString()
        val selfPackage = svc.packageName
        if (pkg == selfPackage) {
            root.recycle()
            Log.w(TAG, "getScreenContent: FALLBACK rejected — rootInActiveWindow is self ($pkg). No target app visible.")
            return ScreenContent(elements = emptyList(), packageName = null)
        }
        val elements = mutableListOf<ScreenElement>()
        collectElements(root, elements, 0)
        root.recycle()
        Log.d(TAG, "getScreenContent: FALLBACK rootInActiveWindow, package=$pkg, elements=${elements.size}")
        return ScreenContent(elements = elements, packageName = pkg)
    }

    internal fun detectForegroundPackage(svc: AccessibilityService): String? {
        val root = findAppWindowRoot(svc) ?: return null
        return try {
            root.packageName?.toString()
        } finally {
            root.recycle()
        }
    }

    internal fun isPrivacyBlocked(svc: AccessibilityService): Boolean =
        getPrivacyBlockedPackage(capturePrivacySignalSnapshot(svc)) != null

    private fun getPrivacyBlockedPackage(svc: AccessibilityService): String? =
        getPrivacyBlockedPackage(capturePrivacySignalSnapshot(svc))

    private fun getPrivacyBlockedPackage(snapshot: PrivacySignalSnapshot): String? {
        privacyBlockedPackageOverrideForTests.get()?.let { return it }
        return resolvePrivacyBlockedPackage(
            rootInActiveWindowPackage = snapshot.rootInActiveWindowPackage,
            foregroundPackage = snapshot.foregroundPackage,
            visiblePackages = snapshot.visiblePackages,
            privacyList = privacyList
        )
    }

    internal fun setPrivacyBlockOverrideForTests(packageName: String?) {
        privacyBlockedPackageOverrideForTests.set(packageName)
    }

    internal fun clearPrivacyBlockOverrideForTests() {
        privacyBlockedPackageOverrideForTests.set(null)
    }

    internal fun resolvePrivacyBlockedPackage(
        rootInActiveWindowPackage: String?,
        foregroundPackage: String?,
        visiblePackages: List<String?>,
        privacyList: PrivacyList?
    ): String? {
        val list = privacyList ?: return null

        fun normalize(pkg: String?): String? = pkg?.trim()?.takeIf { it.isNotEmpty() }
        fun isPrivate(pkg: String?): Boolean = normalize(pkg)?.let(list::isPrivate) == true

        // Fail-secure dual check: block if either source reports a private package.
        if (isPrivate(rootInActiveWindowPackage)) return normalize(rootInActiveWindowPackage)
        if (isPrivate(foregroundPackage)) return normalize(foregroundPackage)

        // Notification shade/system UI can render notification text from other apps without
        // exposing source package per node. Fail secure whenever systemui is foreground and
        // privacy mode is active for at least one app.
        if ((normalize(rootInActiveWindowPackage) == SYSTEM_UI_PACKAGE ||
                normalize(foregroundPackage) == SYSTEM_UI_PACKAGE) &&
            list.getAll().isNotEmpty()
        ) {
            return SYSTEM_UI_PACKAGE
        }

        // Split-screen/multi-window: block if any visible app window is private.
        for (pkg in visiblePackages) {
            if (isPrivate(pkg)) return normalize(pkg)
        }
        return null
    }

    private fun logPrivacyBlock(source: PrivacyBlockSource) {
        Log.d(
            PRIVACY_TAG,
            "privacy_block source=${source.logValue()} blocked=true"
        )
    }

    private fun capturePrivacySignalSnapshot(svc: AccessibilityService): PrivacySignalSnapshot =
        PrivacySignalSnapshot(
            rootInActiveWindowPackage = getRootInActiveWindowPackage(svc),
            foregroundPackage = detectForegroundPackage(svc),
            visiblePackages = getVisibleApplicationPackages(svc)
        )

    private fun getRootInActiveWindowPackage(svc: AccessibilityService): String? {
        val root = svc.rootInActiveWindow ?: return null
        return try {
            root.packageName?.toString()
        } finally {
            root.recycle()
        }
    }

    private fun getVisibleApplicationPackages(svc: AccessibilityService): List<String?> {
        val windows = try {
            svc.windows
        } catch (e: Exception) {
            Log.w(PRIVACY_TAG, "getVisibleApplicationPackages: getWindows() threw: ${e.message}")
            null
        } ?: return emptyList()

        val packages = mutableListOf<String?>()
        for (window in windows) {
            if (window.type != AccessibilityWindowInfo.TYPE_APPLICATION) continue
            val root = window.root ?: continue
            try {
                packages.add(root.packageName?.toString())
            } finally {
                root.recycle()
            }
        }
        return packages
    }

    /**
     * Candidate window data extracted from [AccessibilityWindowInfo] for testability.
     * Production code builds these from the real windows API; tests can construct directly.
     *
     * This abstraction decouples window selection logic from Android framework dependencies,
     * enabling unit testing of [pickBestWindow] without Robolectric service mocks.
     */
    internal data class WindowCandidate(
        val packageName: String?,
        val isActive: Boolean,
        val isFocused: Boolean,
        val root: AccessibilityNodeInfo?
    )

    /**
     * Find the root [AccessibilityNodeInfo] of the best non-Citros application window.
     *
     * Prefers [AccessibilityWindowInfo.TYPE_APPLICATION] windows whose root package is
     * not our own. If multiple match, picks the one that is active/focused.
     *
     * @return The root node of the best window, or `null` if none found.
     *         **Caller must recycle the returned node when done.**
     */
    internal fun findAppWindowRoot(svc: AccessibilityService): AccessibilityNodeInfo? {
        val selfPackage = svc.packageName
        val windows = try { svc.windows } catch (e: Exception) {
            Log.w(TAG, "findAppWindowRoot: getWindows() threw: ${e.message}")
            null
        }
        if (windows.isNullOrEmpty()) {
            Log.d(TAG, "findAppWindowRoot: no windows (null=${windows == null}, empty=${windows?.isEmpty()})")
            return null
        }

        val candidates = windows.mapNotNull { window ->
            if (window.type != AccessibilityWindowInfo.TYPE_APPLICATION) return@mapNotNull null
            val root = window.root ?: return@mapNotNull null
            val isActive = try { window.isActive } catch (_: Exception) { false }
            val isFocused = try { window.isFocused } catch (_: Exception) { false }
            WindowCandidate(
                packageName = root.packageName?.toString(),
                isActive = isActive,
                isFocused = isFocused,
                root = root
            )
        }

        Log.d(
            TAG,
            "findAppWindowRoot: ${windows.size} windows, ${candidates.size} app candidates, active=${candidates.count { it.isActive }}, focused=${candidates.count { it.isFocused }}"
        )
        val result = pickBestWindow(candidates, selfPackage)
        Log.d(TAG, "findAppWindowRoot: selected=${result != null}")
        return result
    }

    /**
     * Select the best non-self window from candidates. Pure selection logic, testable
     * without Android framework mocks.
     *
     * Recycles all [AccessibilityNodeInfo] roots that are NOT returned.
     *
     * @param candidates Window data with root nodes
     * @param selfPackage Our package name to filter out
     * @return The best root node, or `null`. **Caller must recycle the returned node.**
     */
    internal fun pickBestWindow(
        candidates: List<WindowCandidate>,
        selfPackage: String?
    ): AccessibilityNodeInfo? {
        var bestRoot: AccessibilityNodeInfo? = null
        var bestIsActive = false

        for (candidate in candidates) {
            val root = candidate.root
            if (root == null) continue

            if (candidate.packageName == selfPackage) {
                root.recycle()
                continue
            }

            val isActive = candidate.isActive || candidate.isFocused
            if (bestRoot == null || (isActive && !bestIsActive)) {
                // Replace previous best — recycle the old one we're discarding
                bestRoot?.recycle()
                bestRoot = root
                bestIsActive = isActive
            } else {
                // This candidate lost — recycle it immediately
                root.recycle()
            }
        }
        return bestRoot
    }
    
    private fun collectElements(node: AccessibilityNodeInfo, elements: MutableList<ScreenElement>, depth: Int) {
        val text = node.text?.toString()
        val contentDesc = node.contentDescription?.toString()
        val className = node.className?.toString()?.substringAfterLast(".")
        val isClickable = node.isClickable
        val isEditable = node.isEditable
        val bounds = Rect()
        node.getBoundsInScreen(bounds)
        
        // Only include meaningful elements
        if (text != null || contentDesc != null || isClickable || isEditable) {
            elements.add(ScreenElement(
                id = elements.size,
                text = text,
                contentDescription = contentDesc,
                className = className,
                isClickable = isClickable,
                isEditable = isEditable,
                bounds = bounds,
                depth = depth
            ))
        }
        
        // Recurse into children
        for (i in 0 until node.childCount) {
            val child = node.getChild(i) ?: continue
            collectElements(child, elements, depth + 1)
            child.recycle()
        }
    }
    
    /**
     * Click on an element by ID.
     */
    fun clickElement(elementId: Int): Boolean {
        return clickElementDetailed(elementId) is ElementActionResult.Success
    }

    fun clickElementDetailed(elementId: Int): ElementActionResult {
        val svc = getService() ?: return ElementActionResult.ServiceUnavailable
        val snapshot = capturePrivacySignalSnapshot(svc)
        if (getPrivacyBlockedPackage(snapshot) != null) {
            recordPrivacyBlock(source = PrivacyBlockSource.ACTION)
            logPrivacyBlock(source = PrivacyBlockSource.ACTION)
            return ElementActionResult.PrivacyBlocked
        }
        val content = getScreenContentInternal(svc, snapshot)
        if (content.privacyMode) {
            return ElementActionResult.PrivacyBlocked
        }
        val element = content.elements.getOrNull(elementId) ?: return ElementActionResult.ElementNotFound
        val dispatched = clickAt(element.bounds.centerX(), element.bounds.centerY())
        return if (dispatched) ElementActionResult.Success else ElementActionResult.GestureDispatchFailed
    }
    
    /** Default long-press duration in milliseconds. Matches ViewConfiguration default. */
    private const val LONG_PRESS_DURATION_MS = 500L

    /**
     * Long-press an element by ID.
     */
    fun longPressElement(elementId: Int, durationMs: Long = LONG_PRESS_DURATION_MS): Boolean {
        return longPressElementDetailed(elementId, durationMs) is ElementActionResult.Success
    }

    fun longPressElementDetailed(
        elementId: Int,
        durationMs: Long = LONG_PRESS_DURATION_MS
    ): ElementActionResult {
        val svc = getService() ?: return ElementActionResult.ServiceUnavailable
        val snapshot = capturePrivacySignalSnapshot(svc)
        if (getPrivacyBlockedPackage(snapshot) != null) {
            recordPrivacyBlock(source = PrivacyBlockSource.ACTION)
            logPrivacyBlock(source = PrivacyBlockSource.ACTION)
            return ElementActionResult.PrivacyBlocked
        }
        val content = getScreenContentInternal(svc, snapshot)
        if (content.privacyMode) {
            return ElementActionResult.PrivacyBlocked
        }
        val element = content.elements.getOrNull(elementId) ?: return ElementActionResult.ElementNotFound
        val dispatched = longPressAt(element.bounds.centerX(), element.bounds.centerY(), durationMs)
        return if (dispatched) ElementActionResult.Success else ElementActionResult.GestureDispatchFailed
    }

    /**
     * Long-press at screen coordinates.
     * @param durationMs Hold duration; defaults to [LONG_PRESS_DURATION_MS].
     */
    fun longPressAt(x: Int, y: Int, durationMs: Long = LONG_PRESS_DURATION_MS): Boolean {
        val svc = service ?: return false
        
        val path = Path().apply {
            moveTo(x.toFloat(), y.toFloat())
        }
        
        val gesture = GestureDescription.Builder()
            .addStroke(GestureDescription.StrokeDescription(path, 0, durationMs))
            .build()
        
        return svc.dispatchGesture(gesture, null, null)
    }

    /**
     * Click at screen coordinates.
     */
    fun clickAt(x: Int, y: Int): Boolean {
        val svc = service ?: return false
        
        val path = Path().apply {
            moveTo(x.toFloat(), y.toFloat())
        }
        
        val gesture = GestureDescription.Builder()
            .addStroke(GestureDescription.StrokeDescription(path, 0, 100))
            .build()
        
        return svc.dispatchGesture(gesture, null, null)
    }
    
    /**
     * Type text into the focused field.
     *
     * Uses a multi-strategy approach to find the right input field:
     * 1. Window-aware root (findAppWindowRoot) — targets the actual app, not our overlay
     * 2. findFocus(FOCUS_INPUT) — asks the framework directly for the input-focused node
     * 3. Tree walk for isEditable && isFocused — traditional approach
     * 4. Tree walk for isEditable only — relaxed, for apps that don't report focus
     * 5. Fallback to rootInActiveWindow — last resort
     */
    fun typeText(text: String): Boolean {
        val svc = service ?: return false

        val args = Bundle().apply {
            putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, text)
        }

        // Strategy 1: Use window-aware root (same as getScreenContent)
        val appRoot = findAppWindowRoot(svc)
        if (appRoot != null) {
            // Strategy 2: Ask framework for input-focused node within app window
            val inputFocused = appRoot.findFocus(AccessibilityNodeInfo.FOCUS_INPUT)
            if (inputFocused != null) {
                Log.d(TAG, "typeText: found via FOCUS_INPUT in app window (editable=${inputFocused.isEditable})")
                val result = inputFocused.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, args)
                inputFocused.recycle()
                appRoot.recycle()
                return result
            }
            inputFocused?.recycle()

            // Strategy 3: Walk tree for focused editable
            val focused = findFocusedEditText(appRoot)
            if (focused != null) {
                Log.d(TAG, "typeText: found via tree walk (focused+editable) in app window")
                val result = focused.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, args)
                focused.recycle()
                appRoot.recycle()
                return result
            }

            // Strategy 4: Relaxed — any editable node (drop isFocused requirement)
            // NOTE: On multi-input screens (login forms), this grabs the first
            // editable node depth-first, which may not be the intended field.
            // Future improvement: prefer the node nearest lastTappedCoordinates.
            val editable = findEditableNode(appRoot)
            if (editable != null) {
                Log.d(TAG, "typeText: found via relaxed tree walk (editable only) in app window")
                val focusResult = editable.performAction(AccessibilityNodeInfo.ACTION_FOCUS)
                if (!focusResult) {
                    Log.w(TAG, "typeText: ACTION_FOCUS failed on editable node, attempting SET_TEXT anyway")
                }
                val result = editable.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, args)
                editable.recycle()
                appRoot.recycle()
                return result
            }

            appRoot.recycle()
        }

        // Strategy 5: Fallback to rootInActiveWindow (original behavior)
        // Intentionally does NOT try relaxed search here — rootInActiveWindow
        // may return our own overlay root, and blindly targeting the first
        // editable node in our own UI would be wrong.
        val root = svc.rootInActiveWindow
        if (root != null) {
            val focused = findFocusedEditText(root)
            if (focused != null) {
                Log.d(TAG, "typeText: found via rootInActiveWindow fallback")
                val result = focused.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, args)
                focused.recycle()
                root.recycle()
                return result
            }
            root.recycle()
        }

        Log.w(TAG, "typeText: no editable field found in any window")
        return false
    }

    private fun findFocusedEditText(node: AccessibilityNodeInfo): AccessibilityNodeInfo? {
        if (node.isEditable && node.isFocused) {
            return AccessibilityNodeInfo.obtain(node)
        }

        for (i in 0 until node.childCount) {
            val child = node.getChild(i) ?: continue
            val result = findFocusedEditText(child)
            child.recycle()
            if (result != null) return result
        }

        return null
    }

    /**
     * Find any editable node in the tree (relaxed — ignores focus state).
     * Returns the first editable node found via depth-first traversal.
     */
    private fun findEditableNode(node: AccessibilityNodeInfo): AccessibilityNodeInfo? {
        if (node.isEditable) {
            return AccessibilityNodeInfo.obtain(node)
        }

        for (i in 0 until node.childCount) {
            val child = node.getChild(i) ?: continue
            val result = findEditableNode(child)
            child.recycle()
            if (result != null) return result
        }

        return null
    }
    
    /**
     * Swipe gesture.
     */
    fun swipe(startX: Int, startY: Int, endX: Int, endY: Int, durationMs: Long = 300): Boolean {
        val svc = service ?: return false
        
        val path = Path().apply {
            moveTo(startX.toFloat(), startY.toFloat())
            lineTo(endX.toFloat(), endY.toFloat())
        }
        
        val gesture = GestureDescription.Builder()
            .addStroke(GestureDescription.StrokeDescription(path, 0, durationMs))
            .build()
        
        return svc.dispatchGesture(gesture, null, null)
    }
    
    /**
     * Press back button.
     */
    fun pressBack(): Boolean {
        return service?.performGlobalAction(AccessibilityService.GLOBAL_ACTION_BACK) ?: false
    }
    
    /**
     * Press home button.
     */
    fun pressHome(): Boolean {
        return service?.performGlobalAction(AccessibilityService.GLOBAL_ACTION_HOME) ?: false
    }
    
    /**
     * Open recents/app switcher.
     */
    fun openRecents(): Boolean {
        return service?.performGlobalAction(AccessibilityService.GLOBAL_ACTION_RECENTS) ?: false
    }
    
    /**
     * Open notifications.
     */
    fun openNotifications(): Boolean {
        return service?.performGlobalAction(AccessibilityService.GLOBAL_ACTION_NOTIFICATIONS) ?: false
    }

    fun getPrivacyBlockCount(): Int = privacyMetrics.total()

    fun getPrivacyReadScreenBlockCount(): Int = privacyMetrics.bySource(PrivacyBlockSource.READ_SCREEN)

    fun getPrivacyScreenshotBlockCount(): Int = privacyMetrics.bySource(PrivacyBlockSource.SCREENSHOT)

    fun getPrivacyActionBlockCount(): Int = privacyMetrics.bySource(PrivacyBlockSource.ACTION)

    private fun recordPrivacyBlock(source: PrivacyBlockSource) {
        privacyMetrics.emit(source)
    }

    private fun resetPrivacyCounters() {
        privacyMetrics.reset()
    }

    private fun PrivacyBlockSource.logValue(): String = when (this) {
        PrivacyBlockSource.READ_SCREEN -> "read_screen"
        PrivacyBlockSource.SCREENSHOT -> "screenshot"
        PrivacyBlockSource.ACTION -> "action"
    }
}
