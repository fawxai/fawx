package ai.citros.core

import android.content.res.Resources
import android.graphics.Rect
import android.os.Build
import android.view.KeyEvent
import android.view.accessibility.AccessibilityNodeInfo
import android.view.accessibility.AccessibilityWindowInfo
import java.util.concurrent.TimeUnit

/**
 * Handles Google Maps suggestion submission without triggering dropdown-dismiss loops.
 *
 * @param sleepMs Sleep hook used between tier attempts.
 * Callers should consider threading implications (for example, avoid blocking the main thread).
 */
class MapsSuggestionStrategy(
    private val resources: Resources,
    private val readScreen: () -> ScreenContent,
    private val tapAt: (x: Int, y: Int) -> Boolean,
    private val pressEnterKey: () -> Boolean,
    private val sleepMs: (Long) -> Unit = { Thread.sleep(it) }
) {
    data class Outcome(val text: String, val success: Boolean)

    fun handleMapsSuggestion(searchField: AccessibilityNodeInfo, query: String): Outcome {
        // Tier 1: IME search action (API 30+ via ACTION_IME_ENTER, fallback to Enter key on older)
        val imeDispatched = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            searchField.performAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_IME_ENTER.id)
        } else {
            // Pre-API 30: no direct IME enter action available; skip to next tier
            false
        }
        if (imeDispatched) {
            sleepMs(1500)
            if (containsMapResult(readScreen())) {
                return Outcome("Search submitted via IME", success = true)
            }
        }

        // Tier 2: Text-match tap on suggestions from window root
        var window: AccessibilityWindowInfo? = null
        var root: AccessibilityNodeInfo? = null
        val suggestions = mutableListOf<AccessibilityNodeInfo>()
        try {
            window = searchField.window
            root = window?.root
            suggestions += root?.findAccessibilityNodeInfosByText(query).orEmpty()
            // Referential inequality is intentional: findAccessibilityNodeInfosByText returns fresh node instances.
            val suggestionNode = suggestions.firstOrNull { it !== searchField && it.isClickable }
            if (suggestionNode != null && suggestionNode.performAction(AccessibilityNodeInfo.ACTION_CLICK)) {
                sleepMs(1000)
                if (containsMapResult(readScreen())) {
                    return Outcome("Selected suggestion via text match", success = true)
                }
            }
        } finally {
            suggestions.forEach { if (it !== searchField) it.recycle() }
            root?.recycle()
            window?.recycle()
        }

        // Tier 3: Density-aware spatial tap (48dp below search field)
        val bounds = Rect()
        searchField.getBoundsInScreen(bounds)
        val x = bounds.centerX()
        val y = bounds.bottom + dpToPx(SUGGESTION_OFFSET_DP)
        if (tapAt(x, y)) {
            sleepMs(1000)
            if (containsMapResult(readScreen())) {
                return Outcome("Navigation via spatial tap", success = true)
            }
        }

        // Final fallback: raw enter key
        pressEnterKey()
        // TODO(spec/maps-harness-e2e): Add harness regression coverage for the Maps fallback path per spec.
        return Outcome("Search submitted via Enter fallback", success = false)
    }

    internal fun dpToPx(dp: Int): Int =
        (dp * resources.displayMetrics.density + 0.5f).toInt()

    internal fun containsMapResult(screen: ScreenContent): Boolean {
        if (screen.privacyMode) return false
        if (screen.packageName != GOOGLE_MAPS_PACKAGE) return false

        val textBlob = buildString {
            screen.elements.forEach {
                append(it.text.orEmpty())
                append(' ')
                append(it.contentDescription.orEmpty())
                append(' ')
            }
        }.lowercase()

        var matched = 0
        for (indicator in MAP_RESULT_INDICATORS) {
            if (textBlob.contains(indicator)) {
                matched++
                if (matched >= MIN_INDICATOR_MATCHES) return true
            }
        }
        return false
    }

    companion object {
        internal const val GOOGLE_MAPS_PACKAGE = "com.google.android.apps.maps"
        internal const val SUGGESTION_OFFSET_DP = 48
        internal const val MIN_INDICATOR_MATCHES = 2
        internal val MAP_RESULT_INDICATORS = listOf(
            "directions", "start", "route", "save", "share", "call", "reviews",
            "eta", "nearby", "minutes", "minute", "hour", "mi", "km", "open now"
        )

        fun defaultEnterKeyPress(): Boolean {
            return try {
                val process = ProcessBuilder("sh", "-c", "input keyevent ${KeyEvent.KEYCODE_ENTER}")
                    .redirectErrorStream(true)
                    .start()
                if (!process.waitFor(5, TimeUnit.SECONDS)) {
                    process.destroyForcibly()
                    return false
                }
                process.exitValue() == 0
            } catch (_: Exception) {
                false
            }
        }
    }
}
