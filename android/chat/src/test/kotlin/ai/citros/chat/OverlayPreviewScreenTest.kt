package ai.citros.chat

import android.content.Context
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.test.core.app.ApplicationProvider
import kotlin.test.assertTrue
import org.junit.Ignore
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

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

        composeRule.onNodeWithText("Search Bar").performClick()
        composeRule.onNodeWithContentDescription("Overlay search bar").assertExists()

        composeRule.onNodeWithText("Full App").performClick()
        composeRule.onNodeWithText("Return").assertExists()

        composeRule.onNodeWithText("Panel").performClick()
        composeRule.onNodeWithText("Message", useUnmergedTree = true).assertExists()
    }

    @Test
    fun testRunStateTransitions() {
        val context = ApplicationProvider.getApplicationContext<Context>()

        composeRule.setContent {
            OverlayPreviewScreen(context = context, onBack = {})
        }

        composeRule.onNodeWithText("Stop").performClick()
        composeRule.onNodeWithText("Stopped").assertExists()
    }

    @Test
    fun testLiveFullAppCallback() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        var navigatedToChat = false

        composeRule.setContent {
            OverlayPreviewScreen(
                context = context,
                onBack = {},
                viewModel = ChatViewModel(),
                onNavigateToChat = { navigatedToChat = true }
            )
        }

        composeRule.onNodeWithText("Full App").performClick()
        composeRule.runOnIdle {
            assertTrue(navigatedToChat)
        }
    }
}
