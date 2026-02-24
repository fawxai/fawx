package ai.citros.chat.onboarding

import ai.citros.core.Provider
import android.content.Context
import android.provider.Settings
import androidx.test.core.app.ApplicationProvider
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

@RunWith(RobolectricTestRunner::class)
@OptIn(ExperimentalCoroutinesApi::class)
class OnboardingChatIntegrationTest {

    private lateinit var context: Context
    private lateinit var prefs: android.content.SharedPreferences
    private lateinit var accessibilityHelper: AccessibilitySetupHelper

    private val collectedMessages = mutableListOf<OnboardingMessage>()
    private var completeCalled = false

    @Before
    fun setUp() {
        context = ApplicationProvider.getApplicationContext()
        prefs = context.getSharedPreferences("test_onboarding_integration", Context.MODE_PRIVATE)
        prefs.edit().clear().commit()
        accessibilityHelper = AccessibilitySetupHelper(context)
        collectedMessages.clear()
        completeCalled = false
    }

    @Test
    fun `full happy path from welcome to complete`() = runTest {
        val manager = OnboardingManager(prefs)
        val recommender = ModelRecommender(FakeModelFetcher(
            listOf(
                AvailableModel("claude-sonnet-4-5-latest", "Sonnet 4.5"),
                AvailableModel("claude-sonnet-4-latest", "Sonnet 4")
            )
        ))
        val integration = createIntegration(manager, recommender)

        // Activate → shows WELCOME
        assertTrue(integration.activateIfNeeded())
        assertEquals(1, collectedMessages.size)
        assertTrue(collectedMessages.last().text.contains("Citros"))

        // Continue → API_KEY_CHOICE
        integration.onPillTapped(OnboardingAction.CONTINUE)
        assertEquals(2, collectedMessages.size)
        assertTrue(collectedMessages.last().text.contains("API key"))

        // "I have a key" → API_KEY_ENTRY
        integration.onPillTapped(OnboardingAction.I_HAVE_KEY)
        assertEquals(3, collectedMessages.size)
        assertTrue(collectedMessages.last().text.contains("Paste"))

        // Validate key → MODEL_SELECTION (async)
        integration.onApiKeyValidated(Provider.ANTHROPIC, "sk-ant-test")
        advanceUntilIdle()
        assertEquals(4, collectedMessages.size)
        assertTrue(collectedMessages.last().text.contains("recommend"))
        assertTrue(collectedMessages.last().text.contains("sonnet-4-5"))

        // Use recommended model → ACCESSIBILITY_PROMPT
        integration.onPillTapped(OnboardingAction.USE_RECOMMENDED_MODEL)
        assertEquals(5, collectedMessages.size)
        assertTrue(collectedMessages.last().text.contains("accessibility"))

        // Skip accessibility → FIRST_TASK
        integration.onPillTapped(OnboardingAction.SKIP_ACCESSIBILITY)
        assertEquals(OnboardingStep.FIRST_TASK, manager.currentStep)
        assertEquals(6, collectedMessages.size)
        assertTrue(collectedMessages.last().text.contains("All set"))

        // Dismiss → COMPLETE
        integration.onPillTapped(OnboardingAction.DISMISS)
        assertTrue(completeCalled)
        assertTrue(manager.isComplete)
    }

    @Test
    fun `dismiss onboarding mid-flow transitions to complete`() = runTest {
        val manager = OnboardingManager(prefs)
        val integration = createIntegration(manager, ModelRecommender())

        integration.activateIfNeeded()
        integration.onPillTapped(OnboardingAction.CONTINUE) // → API_KEY_CHOICE

        integration.onPillTapped(OnboardingAction.DISMISS)
        assertTrue(completeCalled)
        assertTrue(manager.isComplete)
    }

    @Test
    fun `activateIfNeeded returns false when already complete`() = runTest {
        val manager = OnboardingManager(prefs)
        manager.dismiss() // mark complete

        val integration = createIntegration(manager, ModelRecommender())
        assertFalse(integration.activateIfNeeded())
        assertTrue(collectedMessages.isEmpty())
    }

    @Test
    fun `see other models shows alternatives`() = runTest {
        val manager = OnboardingManager(prefs)
        val recommender = ModelRecommender(FakeModelFetcher(
            listOf(
                AvailableModel("claude-sonnet-4-5-latest", "Sonnet 4.5"),
                AvailableModel("claude-sonnet-4-latest", "Sonnet 4"),
                AvailableModel("claude-haiku-4-5-latest", "Haiku")
            )
        ))
        val integration = createIntegration(manager, recommender)

        integration.activateIfNeeded()
        integration.onPillTapped(OnboardingAction.CONTINUE)
        integration.onPillTapped(OnboardingAction.I_HAVE_KEY)
        integration.onApiKeyValidated(Provider.ANTHROPIC, "sk-ant-test")
        advanceUntilIdle()

        integration.onPillTapped(OnboardingAction.SEE_OTHER_MODELS)
        assertTrue(collectedMessages.last().text.contains("Other available models"))
    }

    @Test
    fun `help me get one goes through provider guide`() = runTest {
        val manager = OnboardingManager(prefs)
        val integration = createIntegration(manager, ModelRecommender())

        integration.activateIfNeeded()
        integration.onPillTapped(OnboardingAction.CONTINUE)
        integration.onPillTapped(OnboardingAction.HELP_ME_GET_ONE)
        assertEquals(OnboardingStep.API_KEY_PROVIDER_GUIDE, manager.currentStep)
        assertTrue(collectedMessages.last().text.contains("providers"))

        integration.onPillTapped(OnboardingAction.GOT_KEY_FROM_GUIDE)
        assertEquals(OnboardingStep.API_KEY_ENTRY, manager.currentStep)
    }

    @Test
    fun `accessibility wait timeout shows retry pill`() = runTest {
        val manager = OnboardingManager(prefs)
        manager.currentStep = OnboardingStep.ACCESSIBILITY_PROMPT
        Settings.Secure.putString(
            context.contentResolver,
            Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES,
            ""
        )

        val integration = createIntegration(manager, ModelRecommender())
        integration.onPillTapped(OnboardingAction.OPEN_SETTINGS)
        advanceUntilIdle()

        val timeoutMessage = collectedMessages.last()
        assertTrue(timeoutMessage.text.contains("didn't detect accessibility"))
        assertTrue(timeoutMessage.pills.any { it.action == OnboardingAction.RETRY_ACCESSIBILITY })
    }

    private fun TestScope.createIntegration(
        manager: OnboardingManager,
        recommender: ModelRecommender
    ): OnboardingChatIntegration {
        val integration = OnboardingChatIntegration(
            onboardingManager = manager,
            modelRecommender = recommender,
            accessibilityHelper = accessibilityHelper,
            scope = this
        )
        integration.onMessage = { collectedMessages.add(it) }
        integration.onComplete = { completeCalled = true }
        return integration
    }

    private class FakeModelFetcher(private val models: List<AvailableModel>) : ModelFetcher {
        override suspend fun fetchAvailableModels(
            provider: Provider,
            apiKey: String
        ): List<AvailableModel> = models
    }
}
