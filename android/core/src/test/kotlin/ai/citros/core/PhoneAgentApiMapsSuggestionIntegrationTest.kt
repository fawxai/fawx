package ai.citros.core

import android.accessibilityservice.AccessibilityService
import android.content.res.Resources
import android.graphics.Rect
import android.util.DisplayMetrics
import android.view.accessibility.AccessibilityNodeInfo
import android.view.accessibility.AccessibilityWindowInfo
import org.junit.After
import org.junit.Test
import org.mockito.kotlin.any
import org.mockito.kotlin.doReturn
import org.mockito.kotlin.eq
import org.mockito.kotlin.mock
import org.mockito.kotlin.whenever
import kotlin.test.assertTrue

class PhoneAgentApiMapsSuggestionIntegrationTest {

    @After
    fun tearDown() {
        ScreenReader.detach()
    }

    @Test
    fun `executeMapsSuggestionStrategyIfApplicable uses structured success not text matching`() {
        val service = mock<AccessibilityService>()
        val resources = mock<Resources>()
        val displayMetrics = DisplayMetrics().apply { density = 2f }
        whenever(resources.displayMetrics).thenReturn(displayMetrics)
        whenever(service.resources).thenReturn(resources)

        val appWindow = mock<AccessibilityWindowInfo>()
        val appRoot = mock<AccessibilityNodeInfo>()
        whenever(appWindow.type).thenReturn(AccessibilityWindowInfo.TYPE_APPLICATION)
        whenever(appWindow.root).thenReturn(appRoot)
        whenever(appWindow.isActive).thenReturn(true)
        whenever(service.windows).thenReturn(listOf(appWindow))

        val searchField = mock<AccessibilityNodeInfo>()
        whenever(appRoot.findFocus(AccessibilityNodeInfo.FOCUS_INPUT)).thenReturn(searchField)
        whenever(searchField.performAction(eq(android.R.id.accessibilityActionImeEnter))).thenReturn(false)

        val tier2Window = mock<AccessibilityWindowInfo>()
        val tier2Root = mock<AccessibilityNodeInfo>()
        val suggestion = mock<AccessibilityNodeInfo>()
        whenever(searchField.window).thenReturn(tier2Window)
        whenever(tier2Window.root).thenReturn(tier2Root)
        whenever(tier2Root.findAccessibilityNodeInfosByText("Pizza")).thenReturn(listOf(suggestion))
        whenever(suggestion.isClickable).thenReturn(true)
        whenever(suggestion.performAction(AccessibilityNodeInfo.ACTION_CLICK)).thenReturn(true)

        val bounds = Rect(100, 200, 300, 260)
        org.mockito.kotlin.doAnswer {
            val rectArg = it.arguments[0] as Rect
            rectArg.set(bounds)
            null
        }.whenever(searchField).getBoundsInScreen(any())

        var readCount = 0
        val mockClient = mock<ProviderClient> {
            on { provider } doReturn Provider.ANTHROPIC
        }
        val api = PhoneAgentApi(
            chatClient = mockClient,
            actionClient = mockClient,
            getScreenContent = {
                readCount++
                if (readCount == 1) {
                    ScreenContent(emptyList(), MapsSuggestionStrategy.GOOGLE_MAPS_PACKAGE)
                } else {
                    ScreenContent(
                        elements = listOf(ScreenElement(0, "Save", "Share", null, true, false, Rect(0, 0, 1, 1))),
                        packageName = MapsSuggestionStrategy.GOOGLE_MAPS_PACKAGE
                    )
                }
            }
        )

        ScreenReader.attach(service)

        val handled = api.executeMapsSuggestionStrategyIfApplicable("Pizza")
        assertTrue(handled)
    }
}
