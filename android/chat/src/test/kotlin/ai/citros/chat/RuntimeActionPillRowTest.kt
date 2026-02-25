package ai.citros.chat

import ai.citros.core.ActionPill
import ai.citros.core.PillAction
import ai.citros.core.PillStyle
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import kotlin.test.assertEquals

@RunWith(RobolectricTestRunner::class)
class RuntimeActionPillRowTest {

    @get:Rule
    val composeRule = createComposeRule()

    @Test
    fun `runtime pill row renders chat pills`() {
        val pills = listOf(
            ActionPill("yes", "Yes", style = PillStyle.PRIMARY, action = PillAction.Approve("req-1")),
            ActionPill("no", "No", style = PillStyle.DANGER, action = PillAction.Deny("req-1"))
        )

        composeRule.setContent {
            RuntimeActionPillRow(
                pills = pills,
                flavor = CitrosFlavor.LIME,
                onPillTapped = {}
            )
        }

        composeRule.onNodeWithText("Yes").assertIsDisplayed()
        composeRule.onNodeWithText("No").assertIsDisplayed()
        composeRule.onNodeWithTag("runtime_pill_yes").assertIsDisplayed()
        composeRule.onNodeWithTag("runtime_pill_no").assertIsDisplayed()
    }

    @Test
    fun `runtime pill row dispatches tapped pill action`() {
        val pills = listOf(
            ActionPill("other", "Do something else", style = PillStyle.SUBTLE, action = PillAction.Steer("Try a different approach."))
        )
        var tappedLabel: String? = null

        composeRule.setContent {
            RuntimeActionPillRow(
                pills = pills,
                flavor = CitrosFlavor.LIME,
                onPillTapped = { tappedLabel = it.label }
            )
        }

        composeRule.onNodeWithTag("runtime_pill_other").performClick()
        assertEquals("Do something else", tappedLabel)
    }
}
