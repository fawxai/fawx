package ai.citros.chat

import android.content.Context
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class OnboardingFlowInstrumentedTest {

    @get:Rule
    val composeRule = createAndroidComposeRule<ChatActivity>()

    private lateinit var context: Context

    @Before
    fun setUp() {
        context = ApplicationProvider.getApplicationContext()
        clearPrefs()
        composeRule.activityRule.scenario.recreate()
        composeRule.waitForIdle()
    }

    @Test
    fun startsOnboardingWhenOnboardingNotComplete() {
        composeRule.onNodeWithTag("onboarding_flow_root").assertIsDisplayed()
    }

    @Test
    fun skipsOnboardingWhenAlreadyCompleted() {
        context.getSharedPreferences(ONBOARDING_PREFS, Context.MODE_PRIVATE)
            .edit()
            .putBoolean(PREF_ONBOARDING_COMPLETE, true)
            .commit()

        composeRule.activityRule.scenario.recreate()
        composeRule.waitForIdle()

        composeRule.onAllNodesWithTag("onboarding_flow_root").assertCountEquals(0)
        composeRule.onNodeWithTag("signin_prompt_root").assertIsDisplayed()
    }

    @Test
    fun completingByoPathPersistsOnboardingState() {
        composeRule.onNodeWithText("Get Started").performClick()
        composeRule.onNodeWithText("Continue").performClick() // flavor
        composeRule.onNodeWithText("Continue").performClick() // personality
        composeRule.onNodeWithText("Continue").performClick() // acquainted
        composeRule.onNodeWithTag("permissions_continue_btn").performClick() // permissions
        composeRule.onNodeWithText("Continue").performClick() // trust
        composeRule.onNodeWithTag("paywall_plan_byo").performClick() // choose your plan
        composeRule.onNodeWithTag("api_key_field").performTextInput("sk-ant-test-key-1234567890")
        composeRule.onNodeWithTag("api_key_start_btn").performClick() // API key
        composeRule.onNodeWithTag("ready_start_btn").performClick() // ready

        composeRule.waitForIdle()

        val prefs = context.getSharedPreferences(ONBOARDING_PREFS, Context.MODE_PRIVATE)
        assertTrue(prefs.getBoolean(PREF_ONBOARDING_COMPLETE, false))
        assertEquals("byo", prefs.getString(PREF_SELECTED_TIER, null))
        assertNotNull(prefs.getString(PREF_SELECTED_FLAVOR, null))
        assertTrue(prefs.getLong(PREF_TRIAL_START_MS, 0L) > 0L)

        composeRule.onNodeWithTag("signin_prompt_root").assertIsDisplayed()
    }

    @Test
    fun waitlistFlowPersistsTierAndEmailThenContinuesByo() {
        composeRule.onNodeWithText("Get Started").performClick()
        composeRule.onNodeWithText("Continue").performClick() // flavor
        composeRule.onNodeWithText("Continue").performClick() // personality
        composeRule.onNodeWithText("Continue").performClick() // acquainted
        composeRule.onNodeWithTag("permissions_continue_btn").performClick() // permissions
        composeRule.onNodeWithText("Continue").performClick() // trust

        composeRule.onNodeWithTag("paywall_plan_base").performClick()
        composeRule.onNodeWithTag("waitlist_email_field").performTextInput("test@example.com")
        composeRule.onNodeWithText("Continue With BYO For Now").performClick()
        composeRule.onNodeWithTag("api_key_field").performTextInput("sk-ant-test-key-1234567890")
        composeRule.onNodeWithTag("api_key_start_btn").performClick() // API key
        composeRule.onNodeWithTag("ready_start_btn").performClick() // ready

        composeRule.waitForIdle()

        val prefs = context.getSharedPreferences(ONBOARDING_PREFS, Context.MODE_PRIVATE)
        assertTrue(prefs.getBoolean(PREF_ONBOARDING_COMPLETE, false))
        assertEquals("base", prefs.getString(PREF_WAITLIST_TIER, null))
        assertEquals("test@example.com", prefs.getString(PREF_WAITLIST_EMAIL, null))
        assertEquals("byo", prefs.getString(PREF_SELECTED_TIER, null))
    }

    private fun clearPrefs() {
        context.getSharedPreferences(ONBOARDING_PREFS, Context.MODE_PRIVATE).edit().clear().commit()
        context.getSharedPreferences("citros", Context.MODE_PRIVATE).edit().clear().commit()
        context.getSharedPreferences("citros_wallet", Context.MODE_PRIVATE).edit().clear().commit()
    }
}
