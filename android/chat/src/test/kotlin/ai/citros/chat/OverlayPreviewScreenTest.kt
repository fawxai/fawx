package ai.citros.chat

import android.content.Context
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.longClick
import androidx.test.core.app.ApplicationProvider
import org.junit.Rule
import org.junit.Ignore
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import kotlin.test.assertTrue

@RunWith(RobolectricTestRunner::class)
@Ignore("Robolectric + Compose touch injection broken — see #361")
class OverlayPreviewScreenTest {

    @get:Rule
    val composeRule = createComposeRule()

    @Test
    fun testSurfaceModeTransitions() {
        val context = ApplicationProvider.getApplicationContext<Context>()

        composeRule.setContent {
            OverlayPreviewScreen(context = context, onBack = {})
        }

        composeRule.onNodeWithText("Bubble").performClick()
        composeRule.onNodeWithContentDescription("Overlay bubble").assertExists()

        composeRule.onNodeWithText("Full App").performClick()
        composeRule.onNodeWithText("Return").assertExists()

        composeRule.onNodeWithText("Mini-Chat").performClick()
        composeRule.onNodeWithText("Queue a follow-up...", useUnmergedTree = true).assertExists()
    }

    @Test
    fun testRunStateTransitionsAndUndoStopFlow() {
        val context = ApplicationProvider.getApplicationContext<Context>()

        composeRule.setContent {
            OverlayPreviewScreen(context = context, onBack = {})
        }

        composeRule.onNodeWithText("Stop").performClick()
        composeRule.onNodeWithText("Action stopped").assertExists()
        composeRule.onNodeWithText("Undo").performClick()

        composeRule.onNodeWithText("Stop").assertExists()
    }

    @Test
    fun testStepTickerProgression() {
        val context = ApplicationProvider.getApplicationContext<Context>()

        composeRule.mainClock.autoAdvance = false
        composeRule.setContent {
            OverlayPreviewScreen(context = context, onBack = {})
        }

        composeRule.onNodeWithText("Opening Settings", useUnmergedTree = true).assertExists()

        composeRule.mainClock.advanceTimeBy(1500)
        composeRule.waitForIdle()

        composeRule.onNodeWithText("Scrolling to Wi-Fi", useUnmergedTree = true).assertExists()
    }

    @Test
    fun testBubbleQuickActionsAndDismissOverlayFlow() {
        val context = ApplicationProvider.getApplicationContext<Context>()

        composeRule.mainClock.autoAdvance = false
        composeRule.setContent {
            OverlayPreviewScreen(context = context, onBack = {})
        }

        composeRule.onNodeWithText("Bubble").performClick()
        composeRule.onNodeWithContentDescription("Overlay bubble").performTouchInput { longClick() }
        composeRule.onNodeWithText("Dismiss Overlay").performClick()

        composeRule.mainClock.advanceTimeBy(260)
        composeRule.waitForIdle()

        composeRule.onNodeWithText("Return").assertExists()
        composeRule.onNodeWithContentDescription("Message input").assertExists()
    }


    @Test
    fun testLiveSurfaceModeCallbacks() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        var navigatedToChat = false
        var minimized = false

        composeRule.setContent {
            OverlayPreviewScreen(
                context = context,
                onBack = {},
                viewModel = ChatViewModel(),
                onOverlayMinimized = { minimized = true },
                onNavigateToChat = { navigatedToChat = true }
            )
        }

        composeRule.onNodeWithText("Full App").performClick()
        composeRule.runOnIdle {
            assertTrue(navigatedToChat)
        }

        composeRule.onNodeWithText("Bubble").performClick()
        composeRule.onNodeWithContentDescription("Overlay bubble").performTouchInput { longClick() }
        composeRule.onNodeWithText("Dismiss Overlay").performClick()
        composeRule.runOnIdle {
            assertTrue(minimized)
        }
    }

}
