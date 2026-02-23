package ai.citros.chat

import android.content.Context
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
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

        composeRule.onNodeWithTag(TEST_TAG_ONBOARDING_CONTINUE_WELCOME).performClick()
        composeRule.onNodeWithTag(TEST_TAG_ONBOARDING_CONTINUE_FLAVOR).performClick()
        composeRule.onNodeWithTag(TEST_TAG_ONBOARDING_CONTINUE_PERSONALITY).performClick()
        composeRule.onNodeWithTag(TEST_TAG_ONBOARDING_CONTINUE_ACQUAINTED).performClick()

        composeRule.onNodeWithTag(TEST_TAG_ONBOARDING_BACK_PERMISSIONS).assertExists()
        composeRule.onNodeWithTag(TEST_TAG_ONBOARDING_BACK_PERMISSIONS).performClick()
        composeRule.onNodeWithTag(TEST_TAG_ONBOARDING_CONTINUE_ACQUAINTED).assertExists()

        composeRule.onNodeWithTag(TEST_TAG_ONBOARDING_CONTINUE_ACQUAINTED).performClick()
        composeRule.onNodeWithTag("permissions_continue_btn").performClick()
        composeRule.onNodeWithTag(TEST_TAG_ONBOARDING_BACK_TRUST).assertExists()
        composeRule.onNodeWithTag(TEST_TAG_ONBOARDING_BACK_TRUST).performClick()
        composeRule.onNodeWithTag(TEST_TAG_ONBOARDING_BACK_PERMISSIONS).assertExists()

        composeRule.onNodeWithTag("permissions_continue_btn").performClick()
        composeRule.onNodeWithTag(TEST_TAG_ONBOARDING_CONTINUE_TRUST).performClick()
        composeRule.onNodeWithTag(TEST_TAG_ONBOARDING_BACK_PAYWALL).assertExists()
        composeRule.onNodeWithTag(TEST_TAG_ONBOARDING_BACK_PAYWALL).performClick()
        composeRule.onNodeWithTag(TEST_TAG_ONBOARDING_BACK_TRUST).assertExists()

        composeRule.onNodeWithTag(TEST_TAG_ONBOARDING_CONTINUE_TRUST).performClick()
        composeRule.onNodeWithTag("paywall_plan_byo").performClick()
        composeRule.onNodeWithTag(TEST_TAG_ONBOARDING_BACK_API_KEY).assertExists()
        composeRule.onNodeWithTag(TEST_TAG_ONBOARDING_BACK_API_KEY).performClick()
        composeRule.onNodeWithTag(TEST_TAG_ONBOARDING_BACK_PAYWALL).assertExists()
    }
}
