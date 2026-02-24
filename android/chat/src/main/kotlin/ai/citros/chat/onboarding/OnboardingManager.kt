package ai.citros.chat.onboarding

import android.content.SharedPreferences

class OnboardingManager(
    private val prefs: SharedPreferences,
    private val hasValidApiKey: () -> Boolean = { false },
    private val hasAccessibility: () -> Boolean = { false },
    private val hasModelSelected: () -> Boolean = { false }
) {
    var currentStep: OnboardingStep
        get() {
            val stored = prefs.getString(PREF_ONBOARDING_STEP, OnboardingStep.WELCOME.name)
            return runCatching { OnboardingStep.valueOf(stored ?: OnboardingStep.WELCOME.name) }
                .getOrDefault(OnboardingStep.WELCOME)
        }
        set(value) {
            prefs.edit().putString(PREF_ONBOARDING_STEP, value.name).apply()
        }

    val isComplete: Boolean
        get() = currentStep == OnboardingStep.COMPLETE

    fun shouldActivate(): Boolean {
        if (isComplete) return false

        if (hasValidApiKey() && hasAccessibility() && hasModelSelected()) {
            currentStep = OnboardingStep.COMPLETE
            return false
        }

        return true
    }

    fun onWelcomeContinue() = transitionTo(OnboardingStep.WELCOME, OnboardingStep.API_KEY_CHOICE)

    fun chooseIHaveKey() = transitionTo(OnboardingStep.API_KEY_CHOICE, OnboardingStep.API_KEY_ENTRY)

    fun chooseHelpMeGetOne() = transitionTo(
        OnboardingStep.API_KEY_CHOICE,
        OnboardingStep.API_KEY_PROVIDER_GUIDE
    )

    fun onProviderGuideGotKey() = transitionTo(
        OnboardingStep.API_KEY_PROVIDER_GUIDE,
        OnboardingStep.API_KEY_ENTRY
    )

    fun onApiKeyValidated() = transitionTo(OnboardingStep.API_KEY_ENTRY, OnboardingStep.MODEL_SELECTION)

    fun onModelSelected() = transitionTo(OnboardingStep.MODEL_SELECTION, OnboardingStep.ACCESSIBILITY_PROMPT)

    fun onAccessibilityPromptShown() = transitionTo(
        OnboardingStep.ACCESSIBILITY_PROMPT,
        OnboardingStep.ACCESSIBILITY_WAIT
    )

    fun onAccessibilityGranted() = transitionTo(OnboardingStep.ACCESSIBILITY_WAIT, OnboardingStep.FIRST_TASK)

    fun onAccessibilityRetry() = transitionTo(
        OnboardingStep.ACCESSIBILITY_WAIT,
        OnboardingStep.ACCESSIBILITY_PROMPT
    )

    fun onAccessibilitySkip() = transitionTo(OnboardingStep.ACCESSIBILITY_WAIT, OnboardingStep.FIRST_TASK)

    fun onFirstTaskFinished() = transitionTo(OnboardingStep.FIRST_TASK, OnboardingStep.COMPLETE)

    fun dismiss() {
        currentStep = OnboardingStep.COMPLETE
    }

    private fun transitionTo(from: OnboardingStep, to: OnboardingStep) {
        if (currentStep == from) {
            currentStep = to
        }
    }

    companion object {
        private const val PREF_ONBOARDING_STEP = "onboarding_step"
    }
}
