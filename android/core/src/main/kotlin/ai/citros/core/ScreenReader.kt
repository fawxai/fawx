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
import kotlin.coroutines.resume
import kotlin.coroutines.suspendCoroutine

/**
 * Screen reading and control utilities.
 * Works with CitrosAccessibilityService.
 */
object ScreenReader {
    
    private const val TAG = "CitrosScreen"
    
    private var service: AccessibilityService? = null

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
        service = accessibilityService
    }
    
    fun detach() {
        service = null
    }
    
    fun isAttached(): Boolean = service != null

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
    internal fun getService(): AccessibilityService? = service
    
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
     * @return Base64-encoded PNG string, or null if capture failed
     */
    suspend fun takeScreenshot(): String? {
        val svc = service ?: return null
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) return null

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
        } ?: return null

        var scaled: Bitmap? = null
        return try {
            scaled = scaleBitmap(bitmap, SCREENSHOT_TARGET_WIDTH)
            val base64 = encodeBitmapToBase64Png(scaled)
            base64
        } catch (_: Exception) {
            null
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

        Log.d(TAG, "findAppWindowRoot: ${windows.size} windows, ${candidates.size} app candidates: ${candidates.map { "${it.packageName ?: "unknown"}(active=${it.isActive},focused=${it.isFocused})" }}, self=$selfPackage")
        val result = pickBestWindow(candidates, selfPackage)
        Log.d(TAG, "findAppWindowRoot: selected=${result?.packageName}")
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
        val content = getScreenContent()
        val element = content.elements.getOrNull(elementId) ?: return false
        return clickAt(element.bounds.centerX(), element.bounds.centerY())
    }
    
    /** Default long-press duration in milliseconds. Matches ViewConfiguration default. */
    private const val LONG_PRESS_DURATION_MS = 500L

    /**
     * Long-press an element by ID.
     */
    fun longPressElement(elementId: Int, durationMs: Long = LONG_PRESS_DURATION_MS): Boolean {
        val content = getScreenContent()
        val element = content.elements.getOrNull(elementId) ?: return false
        return longPressAt(element.bounds.centerX(), element.bounds.centerY(), durationMs)
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
     */
    fun typeText(text: String): Boolean {
        val svc = service ?: return false
        val root = svc.rootInActiveWindow ?: return false
        
        val focused = findFocusedEditText(root)
        if (focused != null) {
            val args = Bundle().apply {
                putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, text)
            }
            val result = focused.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, args)
            focused.recycle()
            root.recycle()
            return result
        }
        
        root.recycle()
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
}

data class ScreenContent(
    val elements: List<ScreenElement>,
    val packageName: String?
) {
    companion object {
        /** Default maximum elements included in prompt text. */
        const val DEFAULT_ELEMENT_CAP = 40
    }

    /**
     * Format screen content as prompt text for the LLM.
     *
     * @param elementCap Maximum elements to include (default [DEFAULT_ELEMENT_CAP]).
     *   Complex apps may have 100+ elements; higher caps give the model more context
     *   at the cost of more tokens. Use model-tier-aware values for optimization.
     *   Clamped to 1..200 to prevent degenerate cases.
     */
    fun toPromptText(elementCap: Int = DEFAULT_ELEMENT_CAP): String {
        val cap = elementCap.coerceIn(1, 200)
        if (elements.isEmpty() && packageName == null) {
            return "No target app is visible. The Citros overlay is in the foreground. Use open_app to launch the app you need."
        }
        val sb = StringBuilder()
        sb.appendLine("App: ${packageName ?: "unknown"}")
        
        // Prioritize interactive and labeled elements, cap at configured limit
        val prioritized = elements
            .sortedByDescending { e ->
                var score = 0
                if (e.isClickable) score += 3
                if (e.isEditable) score += 4
                if (e.text != null) score += 2
                if (e.contentDescription != null) score += 1
                score
            }
            .take(cap)
            .sortedBy { it.id }  // Restore visual order
        
        prioritized.forEach { element ->
            val desc = buildString {
                // Indent by depth for hierarchy hints (2 spaces per level, depths 0-4 shown, 5+ clamped to 4)
                val indent = "  ".repeat(element.depth.coerceAtMost(4))
                append("$indent[${element.id}]")
                element.text?.let { append(" \"${it.take(50)}\"") }
                element.contentDescription?.let { append(" (${it.take(30)})") }
                if (element.isClickable) append(" [click]")
                if (element.isEditable) append(" [edit]")
            }
            sb.appendLine(desc)
        }
        
        if (elements.size > cap) {
            sb.appendLine("(${elements.size - cap} more elements hidden)")
        }
        
        return sb.toString()
    }
}

data class ScreenElement(
    val id: Int,
    val text: String?,
    val contentDescription: String?,
    val className: String?,
    val isClickable: Boolean,
    val isEditable: Boolean,
    val bounds: Rect,
    /** Nesting depth in the accessibility tree (0 = top-level). */
    val depth: Int = 0
)
