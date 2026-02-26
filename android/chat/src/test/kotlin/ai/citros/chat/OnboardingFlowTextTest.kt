package ai.citros.chat

import android.content.Context
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performSemanticsAction
import androidx.test.core.app.ApplicationProvider
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

@RunWith(RobolectricTestRunner::class)
class OnboardingFlowTextTest {

    @get:Rule
    val composeRule = createComposeRule()

    private val context: Context
        get() = ApplicationProvider.getApplicationContext()

    private fun clickByTag(tag: String) {
        composeRule.onNodeWithTag(tag).performSemanticsAction(SemanticsActions.OnClick)
        composeRule.waitForIdle()
    }

    private fun clickByText(text: String, useUnmergedTree: Boolean = false) {
        composeRule.onNodeWithText(text, useUnmergedTree = useUnmergedTree)
            .performSemanticsAction(SemanticsActions.OnClick)
        composeRule.waitForIdle()
    }

    private fun launchOnboarding() {
        composeRule.setContent {
            OnboardingFlow(
                context = context,
                walletDependencies = createTestWalletDependencies(context),
                onFinished = {}
            )
        }
    }

    private fun navigateToPaywall() {
        clickByTag(TEST_TAG_ONBOARDING_CONTINUE_WELCOME)
        clickByTag(TEST_TAG_ONBOARDING_CONTINUE_FLAVOR)
        clickByTag(TEST_TAG_ONBOARDING_CONTINUE_PERSONALITY)
        clickByTag(TEST_TAG_ONBOARDING_CONTINUE_ACQUAINTED)
        clickByTag("permissions_continue_btn")
        clickByTag(TEST_TAG_ONBOARDING_CONTINUE_TRUST)
    }

    @Test
    fun testWelcomeScreenText() {
        launchOnboarding()

        composeRule.onNodeWithText("Citros").assertExists()
        composeRule.onNodeWithText("Your phone, thinking ahead.", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Get Started").assertExists()
    }

    @Test
    fun testFlavorScreenText() {
        launchOnboarding()

        clickByTag(TEST_TAG_ONBOARDING_CONTINUE_WELCOME)

        composeRule.onNodeWithText("Make It Yours").assertExists()
        composeRule.onNodeWithText("Pick a flavor and theme. You can change these anytime.", useUnmergedTree = true)
            .assertExists()
    }

    @Test
    fun testPersonalityScreenText() {
        launchOnboarding()

        clickByTag(TEST_TAG_ONBOARDING_CONTINUE_WELCOME)
        clickByTag(TEST_TAG_ONBOARDING_CONTINUE_FLAVOR)

        composeRule.onNodeWithText("How Should I Talk?").assertExists()
        composeRule.onNodeWithText("Choose how Citros communicates with you.", useUnmergedTree = true).assertExists()
    }

    @Test
    fun testPersonalityComfortLevelQuestion() {
        launchOnboarding()

        clickByTag(TEST_TAG_ONBOARDING_CONTINUE_WELCOME)
        clickByTag(TEST_TAG_ONBOARDING_CONTINUE_FLAVOR)

        composeRule.onNodeWithText("Concise").assertExists()
        composeRule.onNodeWithText("Balanced").assertExists()
        composeRule.onNodeWithText("Thorough").assertExists()
    }

    @Test
    fun testPersonalitySaveContinueButton() {
        launchOnboarding()

        clickByTag(TEST_TAG_ONBOARDING_CONTINUE_WELCOME)
        clickByTag(TEST_TAG_ONBOARDING_CONTINUE_FLAVOR)

        composeRule.onNodeWithTag(TEST_TAG_ONBOARDING_CONTINUE_PERSONALITY).assertExists()
        composeRule.onNodeWithText("Continue").assertExists()
    }

    @Test
    fun testPaywallFooterText() {
        launchOnboarding()

        navigateToPaywall()

        composeRule.onNodeWithText(
            "Base and Super are coming soon. You can continue now with your own API key.",
            useUnmergedTree = true
        ).assertExists()
    }

    @Test
    fun testPaywallSkipButtonText() {
        launchOnboarding()

        navigateToPaywall()

        composeRule.onNodeWithText("Bring Your Own Key - Free").assertExists()
        composeRule.onNodeWithText("Select").assertExists()
    }

    @Test
    fun testAllOnboardingTextUpdatesInSequence() {
        launchOnboarding()

        composeRule.onNodeWithText("Citros").assertExists()
        composeRule.onNodeWithText("Your phone, thinking ahead.", useUnmergedTree = true).assertExists()
        clickByTag(TEST_TAG_ONBOARDING_CONTINUE_WELCOME)

        composeRule.onNodeWithText("Make It Yours").assertExists()
        composeRule.onNodeWithText("Pick a flavor and theme. You can change these anytime.", useUnmergedTree = true)
            .assertExists()
        clickByTag(TEST_TAG_ONBOARDING_CONTINUE_FLAVOR)

        composeRule.onNodeWithText("How Should I Talk?").assertExists()
        composeRule.onNodeWithText("Choose how Citros communicates with you.", useUnmergedTree = true).assertExists()
        clickByTag(TEST_TAG_ONBOARDING_CONTINUE_PERSONALITY)
    }
}
