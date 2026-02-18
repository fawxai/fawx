package ai.citros.chat

import android.content.Context
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.test.core.app.ApplicationProvider
import org.junit.Rule
import org.junit.Ignore
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * Tests verifying text conformance with citros-ui-mocks.html for PR #302.
 * These tests ensure the onboarding flow displays the correct user-facing strings
 * as specified in issues #292, #293, and #296.
 */
@RunWith(RobolectricTestRunner::class)
@Ignore("Robolectric + Compose touch injection broken — see #361")
class OnboardingFlowTextTest {

    @get:Rule
    val composeRule = createComposeRule()

    private val context: Context
        get() = ApplicationProvider.getApplicationContext()

    @Test
    fun testWelcomeScreenText() {
        composeRule.setContent {
            OnboardingFlow(context = context, onFinished = {})
        }

        // Verify welcome screen displays correct title and subtitle
        composeRule.onNodeWithText("Welcome to Citros").assertExists()
        composeRule.onNodeWithText("AI that uses your phone", useUnmergedTree = true).assertExists()
    }

    @Test
    fun testFlavorScreenText() {
        composeRule.setContent {
            OnboardingFlow(context = context, onFinished = {})
        }

        // Navigate to flavor screen
        composeRule.onNodeWithText("Continue").performClick()
        composeRule.waitForIdle()

        // Verify flavor screen displays correct title and subtitle
        composeRule.onNodeWithText("Choose Your Flavor").assertExists()
        composeRule.onNodeWithText("This sets your personal color theme", useUnmergedTree = true).assertExists()
    }

    @Test
    fun testPersonalityScreenText() {
        composeRule.setContent {
            OnboardingFlow(context = context, onFinished = {})
        }

        // Navigate to personality screen
        composeRule.onNodeWithText("Continue").performClick()
        composeRule.waitForIdle()
        composeRule.onNodeWithText("Next").performClick()
        composeRule.waitForIdle()

        // Verify personality screen displays correct title and subtitle
        composeRule.onNodeWithText("Personalize Citros").assertExists()
        composeRule.onNodeWithText("Tell me how you like things", useUnmergedTree = true).assertExists()
    }

    @Test
    fun testPersonalityComfortLevelQuestion() {
        composeRule.setContent {
            OnboardingFlow(context = context, onFinished = {})
        }

        // Navigate to personality screen
        composeRule.onNodeWithText("Continue").performClick()
        composeRule.waitForIdle()
        composeRule.onNodeWithText("Next").performClick()
        composeRule.waitForIdle()

        // Verify comfort level question text
        composeRule.onNodeWithText("What's your comfort level?", useUnmergedTree = true).assertExists()
    }

    @Test
    fun testPersonalitySaveContinueButton() {
        composeRule.setContent {
            OnboardingFlow(context = context, onFinished = {})
        }

        // Navigate to personality screen
        composeRule.onNodeWithText("Continue").performClick()
        composeRule.waitForIdle()
        composeRule.onNodeWithText("Next").performClick()
        composeRule.waitForIdle()

        // Verify CTA button text
        composeRule.onNodeWithText("Save & Continue").assertExists()
    }

    @Test
    fun testPaywallFooterText() {
        composeRule.setContent {
            OnboardingFlow(context = context, onFinished = {})
        }

        // Navigate through onboarding to paywall screen
        composeRule.onNodeWithText("Continue").performClick()
        composeRule.waitForIdle()
        composeRule.onNodeWithText("Next").performClick()
        composeRule.waitForIdle()
        composeRule.onNodeWithText("Save & Continue").performClick()
        composeRule.waitForIdle()
        
        // Skip onboarding chat if present
        try {
            composeRule.onNodeWithText("Skip").performClick()
            composeRule.waitForIdle()
        } catch (e: Exception) {
            // Chat might be disabled, continue
        }

        // Verify paywall footer text
        composeRule.onNodeWithText("Cancel anytime · Usage resets monthly · All plans include phone control", useUnmergedTree = true)
            .assertExists()
    }

    @Test
    fun testPaywallSkipButtonText() {
        composeRule.setContent {
            OnboardingFlow(context = context, onFinished = {})
        }

        // Navigate through onboarding to paywall screen
        composeRule.onNodeWithText("Continue").performClick()
        composeRule.waitForIdle()
        composeRule.onNodeWithText("Next").performClick()
        composeRule.waitForIdle()
        composeRule.onNodeWithText("Save & Continue").performClick()
        composeRule.waitForIdle()
        
        // Skip onboarding chat if present
        try {
            composeRule.onNodeWithText("Skip").performClick()
            composeRule.waitForIdle()
        } catch (e: Exception) {
            // Chat might be disabled, continue
        }

        // Verify skip button text with arrow
        composeRule.onNodeWithText("I'll decide later →").assertExists()
    }

    @Test
    fun testAllOnboardingTextUpdatesInSequence() {
        composeRule.setContent {
            OnboardingFlow(context = context, onFinished = {})
        }

        // Step 1: Welcome screen
        composeRule.onNodeWithText("Welcome to Citros").assertExists()
        composeRule.onNodeWithText("AI that uses your phone", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Continue").performClick()
        composeRule.waitForIdle()

        // Step 2: Flavor screen
        composeRule.onNodeWithText("Choose Your Flavor").assertExists()
        composeRule.onNodeWithText("This sets your personal color theme", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Next").performClick()
        composeRule.waitForIdle()

        // Step 3: Personality screen
        composeRule.onNodeWithText("Personalize Citros").assertExists()
        composeRule.onNodeWithText("Tell me how you like things", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("What's your comfort level?", useUnmergedTree = true).assertExists()
        composeRule.onNodeWithText("Save & Continue").assertExists()
    }
}
