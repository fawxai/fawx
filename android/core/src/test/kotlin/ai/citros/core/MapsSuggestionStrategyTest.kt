package ai.citros.core

import android.content.res.Resources
import android.graphics.Rect
import android.util.DisplayMetrics
import android.view.accessibility.AccessibilityNodeInfo
import android.view.accessibility.AccessibilityWindowInfo
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.mockito.kotlin.any
import org.mockito.kotlin.eq
import org.mockito.kotlin.mock
import org.mockito.kotlin.never
import org.mockito.kotlin.times
import org.mockito.kotlin.verify
import org.mockito.kotlin.whenever
import org.robolectric.RobolectricTestRunner

@RunWith(RobolectricTestRunner::class)
class MapsSuggestionStrategyTest {

    @Test
    fun `tier 1 dispatches IME action and verifies result`() {
        val resources = resourcesWithDensity(2f)
        val searchField = mock<AccessibilityNodeInfo>()
        whenever(searchField.performAction(eq(AccessibilityNodeInfo.AccessibilityAction.ACTION_IME_ENTER.id))).thenReturn(true)

        val strategy = MapsSuggestionStrategy(
            resources = resources,
            readScreen = {
                ScreenContent(
                    elements = listOf(ScreenElement(0, "Directions", null, null, true, false, Rect(0, 0, 10, 10))),
                    packageName = MapsSuggestionStrategy.GOOGLE_MAPS_PACKAGE
                )
            },
            tapAt = { _, _ -> true },
            pressEnterKey = { true },
            sleepMs = {}
        )

        val result = strategy.handleMapsSuggestion(searchField, "Times Square")

        assertEquals("Search submitted via IME", result.text)
        assertTrue(result.success)
        verify(searchField).performAction(eq(AccessibilityNodeInfo.AccessibilityAction.ACTION_IME_ENTER.id))
    }

    @Test
    fun `tier 1 skips sleep when IME action dispatch fails`() {
        val resources = resourcesWithDensity(2f)
        val searchField = mock<AccessibilityNodeInfo>()
        whenever(searchField.performAction(eq(AccessibilityNodeInfo.AccessibilityAction.ACTION_IME_ENTER.id))).thenReturn(false)

        var slept = false
        var enterPressed = false
        val strategy = MapsSuggestionStrategy(
            resources = resources,
            readScreen = { ScreenContent(emptyList(), MapsSuggestionStrategy.GOOGLE_MAPS_PACKAGE) },
            tapAt = { _, _ -> false },
            pressEnterKey = { enterPressed = true; true },
            sleepMs = { slept = true }
        )

        strategy.handleMapsSuggestion(searchField, "Coffee")
        assertFalse(slept)
        assertTrue(enterPressed)
    }

    @Test
    fun `tier 2 taps clickable text-match suggestion when tier 1 not successful and recycles nodes`() {
        val resources = resourcesWithDensity(2f)
        val searchField = mock<AccessibilityNodeInfo>()
        val window = mock<AccessibilityWindowInfo>()
        val root = mock<AccessibilityNodeInfo>()
        val suggestion = mock<AccessibilityNodeInfo>()
        whenever(searchField.window).thenReturn(window)
        whenever(window.root).thenReturn(root)
        whenever(root.findAccessibilityNodeInfosByText("Coffee"))
            .thenReturn(listOf(searchField, suggestion))
        whenever(suggestion.isClickable).thenReturn(true)

        var reads = 0
        val strategy = MapsSuggestionStrategy(
            resources = resources,
            readScreen = {
                reads++
                if (reads == 1) {
                    ScreenContent(emptyList(), MapsSuggestionStrategy.GOOGLE_MAPS_PACKAGE)
                } else {
                    ScreenContent(
                        elements = listOf(ScreenElement(0, "Directions", null, null, true, false, Rect(0, 0, 1, 1))),
                        packageName = MapsSuggestionStrategy.GOOGLE_MAPS_PACKAGE
                    )
                }
            },
            tapAt = { _, _ -> true },
            pressEnterKey = { true },
            sleepMs = {}
        )

        val result = strategy.handleMapsSuggestion(searchField, "Coffee")

        assertEquals("Selected suggestion via text match", result.text)
        assertTrue(result.success)
        verify(suggestion).performAction(AccessibilityNodeInfo.ACTION_CLICK)
        verify(suggestion).recycle()
        verify(root).recycle()
        verify(window).recycle()
        verify(searchField, never()).recycle()
    }

    @Test
    fun `tier 3 converts dp to px at common densities`() {
        val d1 = MapsSuggestionStrategy(resourcesWithDensity(1f), { ScreenContent(emptyList(), null) }, { _, _ -> true }, { true }, {})
        val d2 = MapsSuggestionStrategy(resourcesWithDensity(2f), { ScreenContent(emptyList(), null) }, { _, _ -> true }, { true }, {})
        val d3 = MapsSuggestionStrategy(resourcesWithDensity(3f), { ScreenContent(emptyList(), null) }, { _, _ -> true }, { true }, {})

        assertEquals(48, d1.dpToPx(48))
        assertEquals(96, d2.dpToPx(48))
        assertEquals(144, d3.dpToPx(48))
    }

    @Test
    fun `fallback chain reaches enter key when all tiers fail`() {
        val resources = resourcesWithDensity(2f)
        val searchField = mock<AccessibilityNodeInfo>()
        val bounds = Rect(100, 200, 400, 260)

        var tapped = false
        var enterPressed = false

        val strategy = MapsSuggestionStrategy(
            resources = resources,
            readScreen = { ScreenContent(emptyList(), MapsSuggestionStrategy.GOOGLE_MAPS_PACKAGE) },
            tapAt = { _, _ -> tapped = true; true },
            pressEnterKey = { enterPressed = true; true },
            sleepMs = {}
        )
        whenever(searchField.performAction(eq(AccessibilityNodeInfo.AccessibilityAction.ACTION_IME_ENTER.id))).thenReturn(true)
        whenever(searchField.window).thenReturn(null)

        org.mockito.kotlin.doAnswer {
            val rectArg = it.arguments[0] as Rect
            rectArg.set(bounds)
            null
        }.whenever(searchField).getBoundsInScreen(any())

        val result = strategy.handleMapsSuggestion(searchField, "Nowhere")

        assertEquals("Search submitted via Enter fallback", result.text)
        assertFalse(result.success)
        assertTrue(tapped)
        assertTrue(enterPressed)
    }

    @Test
    fun `integration dropdown present uses tier 2 and dropdown absent stays tier 1`() {
        val resources = resourcesWithDensity(2f)

        val dropdownSearchField = mock<AccessibilityNodeInfo>()
        val window = mock<AccessibilityWindowInfo>()
        val root = mock<AccessibilityNodeInfo>()
        val suggestion = mock<AccessibilityNodeInfo>()
        whenever(dropdownSearchField.window).thenReturn(window)
        whenever(window.root).thenReturn(root)
        whenever(root.findAccessibilityNodeInfosByText("Pizza"))
            .thenReturn(listOf(suggestion))
        whenever(suggestion.isClickable).thenReturn(true)

        var readCount = 0
        val dropdownStrategy = MapsSuggestionStrategy(
            resources,
            readScreen = {
                readCount++
                if (readCount == 1) ScreenContent(emptyList(), MapsSuggestionStrategy.GOOGLE_MAPS_PACKAGE)
                else ScreenContent(listOf(ScreenElement(0, "Directions", null, null, true, false, Rect(0, 0, 1, 1))), MapsSuggestionStrategy.GOOGLE_MAPS_PACKAGE)
            },
            tapAt = { _, _ -> true },
            pressEnterKey = { true },
            sleepMs = {}
        )
        assertEquals("Selected suggestion via text match", dropdownStrategy.handleMapsSuggestion(dropdownSearchField, "Pizza").text)

        val plainSearchField = mock<AccessibilityNodeInfo>()
        whenever(plainSearchField.performAction(eq(AccessibilityNodeInfo.AccessibilityAction.ACTION_IME_ENTER.id))).thenReturn(true)
        val plainStrategy = MapsSuggestionStrategy(
            resources,
            readScreen = { ScreenContent(listOf(ScreenElement(0, "Directions", null, null, true, false, Rect(0, 0, 1, 1))), MapsSuggestionStrategy.GOOGLE_MAPS_PACKAGE) },
            tapAt = { _, _ -> true },
            pressEnterKey = { true },
            sleepMs = {}
        )
        assertEquals("Search submitted via IME", plainStrategy.handleMapsSuggestion(plainSearchField, "Pizza").text)
        verify(plainSearchField, never()).window
        verify(plainSearchField, times(1)).performAction(eq(AccessibilityNodeInfo.AccessibilityAction.ACTION_IME_ENTER.id))
    }

    @Test
    fun `containsMapResult returns false in privacy mode`() {
        val strategy = MapsSuggestionStrategy(resourcesWithDensity(2f), { ScreenContent(emptyList(), null) }, { _, _ -> true }, { true }, {})
        val screen = ScreenContent(
            elements = listOf(ScreenElement(0, "Directions", "15 min", null, false, false, Rect())),
            packageName = MapsSuggestionStrategy.GOOGLE_MAPS_PACKAGE,
            privacyMode = true
        )

        assertFalse(strategy.containsMapResult(screen))
    }

    @Test
    fun `containsMapResult returns false for wrong package`() {
        val strategy = MapsSuggestionStrategy(resourcesWithDensity(2f), { ScreenContent(emptyList(), null) }, { _, _ -> true }, { true }, {})
        val screen = ScreenContent(
            elements = listOf(ScreenElement(0, "Directions", "15 min", null, false, false, Rect())),
            packageName = "com.example.mapsclone"
        )

        assertFalse(strategy.containsMapResult(screen))
    }

    @Test
    fun `containsMapResult returns false with no indicators`() {
        val strategy = MapsSuggestionStrategy(resourcesWithDensity(2f), { ScreenContent(emptyList(), null) }, { _, _ -> true }, { true }, {})
        val screen = ScreenContent(
            elements = listOf(ScreenElement(0, "Welcome", "Profile", null, false, false, Rect())),
            packageName = MapsSuggestionStrategy.GOOGLE_MAPS_PACKAGE
        )

        assertFalse(strategy.containsMapResult(screen))
    }

    @Test
    fun `containsMapResult matches each indicator category`() {
        val strategy = MapsSuggestionStrategy(resourcesWithDensity(2f), { ScreenContent(emptyList(), null) }, { _, _ -> true }, { true }, {})

        val routeCategory = ScreenContent(
            elements = listOf(ScreenElement(0, "Directions", "Route options", null, false, false, Rect())),
            packageName = MapsSuggestionStrategy.GOOGLE_MAPS_PACKAGE
        )
        val placeActionsCategory = ScreenContent(
            elements = listOf(ScreenElement(0, "Save", "Share", null, false, false, Rect())),
            packageName = MapsSuggestionStrategy.GOOGLE_MAPS_PACKAGE
        )
        val timeDistanceCategory = ScreenContent(
            elements = listOf(ScreenElement(0, "15 minutes", "3 km", null, false, false, Rect())),
            packageName = MapsSuggestionStrategy.GOOGLE_MAPS_PACKAGE
        )
        val nearbyCategory = ScreenContent(
            elements = listOf(ScreenElement(0, "Nearby", "Open now", null, false, false, Rect())),
            packageName = MapsSuggestionStrategy.GOOGLE_MAPS_PACKAGE
        )

        assertTrue(strategy.containsMapResult(routeCategory))
        assertTrue(strategy.containsMapResult(placeActionsCategory))
        assertTrue(strategy.containsMapResult(timeDistanceCategory))
        assertTrue(strategy.containsMapResult(nearbyCategory))
    }

    private fun resourcesWithDensity(density: Float): Resources {
        val displayMetrics = DisplayMetrics().apply { this.density = density }
        val resources = mock<Resources>()
        whenever(resources.displayMetrics).thenReturn(displayMetrics)
        return resources
    }
}
