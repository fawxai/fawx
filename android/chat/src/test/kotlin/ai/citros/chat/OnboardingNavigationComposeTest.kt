package ai.citros.chat

import android.content.Context
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performSemanticsAction
import androidx.test.core.app.ApplicationProvider
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

@RunWith(RobolectricTestRunner::class)
class OnboardingNavigationComposeTest {

    @get:Rule
    val composeRule = createComposeRule()

    private val context: Context
        get() = ApplicationProvider.getApplicationContext()

    private fun clickByTag(tag: String) {
        composeRule.onNodeWithTag(tag).performSemanticsAction(SemanticsActions.OnClick)
        composeRule.waitForIdle()
    }

    @Test
    fun backButtonsNavigateToExpectedEditedSteps() {
        context.getSharedPreferences(ONBOARDING_PREFS, Context.MODE_PRIVATE)
            .edit()
            .clear()
            .commit()

        composeRule.setContent {
            OnboardingFlow(
                context = context,
                walletDependencies = createTestWalletDependencies(context),
                onFinished = {}
            )
        }

        clickByTag(TEST_TAG_ONBOARDING_CONTINUE_WELCOME)
        clickByTag(TEST_TAG_ONBOARDING_CONTINUE_FLAVOR)
        clickByTag(TEST_TAG_ONBOARDING_CONTINUE_PERSONALITY)
        clickByTag(TEST_TAG_ONBOARDING_CONTINUE_ACQUAINTED)

        composeRule.onNodeWithTag(TEST_TAG_ONBOARDING_BACK_PERMISSIONS).assertExists()
        clickByTag(TEST_TAG_ONBOARDING_BACK_PERMISSIONS)
        composeRule.onNodeWithTag(TEST_TAG_ONBOARDING_CONTINUE_ACQUAINTED).assertExists()

        clickByTag(TEST_TAG_ONBOARDING_CONTINUE_ACQUAINTED)
        clickByTag("permissions_continue_btn")
        composeRule.onNodeWithTag(TEST_TAG_ONBOARDING_BACK_TRUST).assertExists()
        clickByTag(TEST_TAG_ONBOARDING_BACK_TRUST)
        composeRule.onNodeWithTag(TEST_TAG_ONBOARDING_BACK_PERMISSIONS).assertExists()

        clickByTag("permissions_continue_btn")
        clickByTag(TEST_TAG_ONBOARDING_CONTINUE_TRUST)
        composeRule.onNodeWithTag(TEST_TAG_ONBOARDING_BACK_PAYWALL).assertExists()
        clickByTag(TEST_TAG_ONBOARDING_BACK_PAYWALL)
        composeRule.onNodeWithTag(TEST_TAG_ONBOARDING_BACK_TRUST).assertExists()

        clickByTag(TEST_TAG_ONBOARDING_CONTINUE_TRUST)
        clickByTag("paywall_plan_byo")
        composeRule.onNodeWithTag(TEST_TAG_ONBOARDING_BACK_API_KEY).assertExists()
        clickByTag(TEST_TAG_ONBOARDING_BACK_API_KEY)
        composeRule.onNodeWithTag(TEST_TAG_ONBOARDING_BACK_PAYWALL).assertExists()
    }
}
