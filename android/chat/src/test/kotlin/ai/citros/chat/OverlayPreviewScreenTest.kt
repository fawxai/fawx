package ai.citros.chat

import android.content.Context
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.performSemanticsAction
import androidx.test.core.app.ApplicationProvider
import kotlin.test.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

@RunWith(RobolectricTestRunner::class)
class OverlayPreviewScreenTest {

    @get:Rule
    val composeRule = createComposeRule()

    private fun clickByDescription(description: String) {
        composeRule.onNodeWithContentDescription(description)
            .performSemanticsAction(SemanticsActions.OnClick)
        composeRule.waitForIdle()
    }

    @Test
    fun testSurfaceModeTransitions() {
        val context = ApplicationProvider.getApplicationContext<Context>()

        composeRule.setContent {
            OverlayPreviewScreen(context = context, onBack = {})
        }

        clickByDescription("Search Bar mode not selected")
        composeRule.onNodeWithContentDescription("Search Bar mode selected").assertExists()

        clickByDescription("Full App mode not selected")
        composeRule.onNodeWithContentDescription("Full App mode selected").assertExists()

        clickByDescription("Panel mode not selected")
        composeRule.onNodeWithContentDescription("Panel mode selected").assertExists()
    }

    @Test
    fun testRunStateTransitions() {
        val context = ApplicationProvider.getApplicationContext<Context>()

        composeRule.setContent {
            OverlayPreviewScreen(context = context, onBack = {})
        }

        clickByDescription("Stop state not selected")
        composeRule.onNodeWithContentDescription("Stop state selected").assertExists()
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

        clickByDescription("Full App mode not selected")
        composeRule.runOnIdle {
            assertTrue(navigatedToChat)
        }
    }
}
