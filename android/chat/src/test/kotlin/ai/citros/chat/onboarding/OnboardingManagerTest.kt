package ai.citros.chat.onboarding

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

@RunWith(RobolectricTestRunner::class)
class OnboardingManagerTest {

    private lateinit var context: Context

    @Before
    fun setUp() {
        context = ApplicationProvider.getApplicationContext()
        context.getSharedPreferences("test_onboarding", Context.MODE_PRIVATE).edit().clear().commit()
    }

    @Test
    fun `state transitions through full happy path`() {
        val manager = manager()

        assertEquals(OnboardingStep.WELCOME, manager.currentStep)

        manager.onWelcomeContinue()
        assertEquals(OnboardingStep.API_KEY_CHOICE, manager.currentStep)

        manager.chooseIHaveKey()
        assertEquals(OnboardingStep.API_KEY_ENTRY, manager.currentStep)

        manager.onApiKeyValidated()
        assertEquals(OnboardingStep.MODEL_SELECTION, manager.currentStep)

        manager.onModelSelected()
        assertEquals(OnboardingStep.ACCESSIBILITY_PROMPT, manager.currentStep)

        manager.onAccessibilityPromptShown()
        assertEquals(OnboardingStep.ACCESSIBILITY_WAIT, manager.currentStep)

        manager.onAccessibilityGranted()
        assertEquals(OnboardingStep.FIRST_TASK, manager.currentStep)

        manager.onFirstTaskFinished()
        assertEquals(OnboardingStep.COMPLETE, manager.currentStep)
        assertTrue(manager.isComplete)
    }

    @Test
    fun `state transitions through provider guide happy path`() {
        val manager = manager()

        manager.onWelcomeContinue()
        assertEquals(OnboardingStep.API_KEY_CHOICE, manager.currentStep)

        manager.chooseHelpMeGetOne()
        assertEquals(OnboardingStep.API_KEY_PROVIDER_GUIDE, manager.currentStep)

        manager.onProviderGuideGotKey()
        assertEquals(OnboardingStep.API_KEY_ENTRY, manager.currentStep)

        manager.onApiKeyValidated()
        assertEquals(OnboardingStep.MODEL_SELECTION, manager.currentStep)
    }

    @Test
    fun `transition from wrong step is no-op`() {
        val manager = manager()

        assertEquals(OnboardingStep.WELCOME, manager.currentStep)
        manager.onApiKeyValidated()
        assertEquals(OnboardingStep.WELCOME, manager.currentStep)
    }

    @Test
    fun `accessibility retry loops back to wait and skip proceeds to first task`() {
        val manager = manager()
        manager.currentStep = OnboardingStep.ACCESSIBILITY_WAIT

        manager.onAccessibilityRetry()
        assertEquals(OnboardingStep.ACCESSIBILITY_PROMPT, manager.currentStep)

        manager.onAccessibilityPromptShown()
        assertEquals(OnboardingStep.ACCESSIBILITY_WAIT, manager.currentStep)

        manager.onAccessibilitySkip()
        assertEquals(OnboardingStep.FIRST_TASK, manager.currentStep)
    }

    @Test
    fun `shouldActivate returns false and completes when valid config exists`() {
        val manager = manager(
            hasValidApiKey = { true },
            hasAccessibility = { true },
            hasModelSelected = { true }
        )

        assertFalse(manager.shouldActivate())
        assertEquals(OnboardingStep.COMPLETE, manager.currentStep)
    }

    @Test
    fun `shouldActivate returns false when already complete`() {
        val manager = manager(
            hasValidApiKey = { throw AssertionError("hasValidApiKey should not be checked when complete") },
            hasAccessibility = { throw AssertionError("hasAccessibility should not be checked when complete") },
            hasModelSelected = { throw AssertionError("hasModelSelected should not be checked when complete") }
        )
        manager.currentStep = OnboardingStep.COMPLETE

        assertFalse(manager.shouldActivate())
        assertEquals(OnboardingStep.COMPLETE, manager.currentStep)
    }

    @Test
    fun `resume after kill restores persisted step`() {
        val prefs = context.getSharedPreferences("test_onboarding", Context.MODE_PRIVATE)
        val manager = OnboardingManager(prefs)
        manager.currentStep = OnboardingStep.ACCESSIBILITY_WAIT

        val recreated = OnboardingManager(prefs)
        assertEquals(OnboardingStep.ACCESSIBILITY_WAIT, recreated.currentStep)
    }

    @Test
    fun `dismiss at any step jumps to complete`() {
        val manager = manager()
        OnboardingStep.entries.filter { it != OnboardingStep.COMPLETE }.forEach { step ->
            manager.currentStep = step
            manager.dismiss()
            assertEquals(OnboardingStep.COMPLETE, manager.currentStep)
            assertTrue(manager.isComplete)
        }
    }

    @Test
    fun `shouldActivate with partial configuration states`() {
        assertTrue(manager().shouldActivate())

        assertTrue(manager(hasValidApiKey = { true }).shouldActivate())

        assertTrue(
            manager(
                hasValidApiKey = { true },
                hasModelSelected = { true }
            ).shouldActivate()
        )

        val fullyConfigured = manager(
            hasValidApiKey = { true },
            hasAccessibility = { true },
            hasModelSelected = { true }
        )
        assertFalse(fullyConfigured.shouldActivate())
    }

    private fun manager(
        hasValidApiKey: () -> Boolean = { false },
        hasAccessibility: () -> Boolean = { false },
        hasModelSelected: () -> Boolean = { false }
    ): OnboardingManager {
        val prefs = context.getSharedPreferences("test_onboarding", Context.MODE_PRIVATE)
        prefs.edit().clear().commit()
        return OnboardingManager(
            prefs = prefs,
            hasValidApiKey = hasValidApiKey,
            hasAccessibility = hasAccessibility,
            hasModelSelected = hasModelSelected
        )
    }
}
