package ai.citros.chat.onboarding

import ai.citros.core.Provider
import kotlin.math.max
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch

/**
 * Bridges the onboarding state machine with the chat UI.
 *
 * Generates conversational system messages and pill-button options
 * based on the current [OnboardingStep]. The ChatViewModel delegates
 * onboarding display and user interactions to this class.
 */
class OnboardingChatIntegration(
    private val onboardingManager: OnboardingManager,
    private val modelRecommender: ModelRecommender,
    private val accessibilityHelper: AccessibilitySetupHelper,
    private val scope: CoroutineScope,
    private val firstTaskGuide: FirstTaskGuide = FirstTaskGuide(),
    private val metricsTracker: OnboardingMetricsTracker? = null
) {

    /** Callback for adding messages to the chat UI. */
    var onMessage: ((OnboardingMessage) -> Unit)? = null

    /** Callback when onboarding completes (all steps done or dismissed). */
    var onComplete: (() -> Unit)? = null

    /** Callback when user selects a suggested first task. */
    var onFirstTaskSelected: ((SuggestedTask) -> Unit)? = null

    /** Last recommendation cached for "See other models" flow. */
    private var lastRecommendation: ModelRecommendation? = null

    /** API key stored after validation for model recommendation. */
    private var validatedApiKey: String? = null

    /**
     * Check if onboarding should activate and show the first message if so.
     * Call from ChatViewModel init/launch.
     *
     * @return true if onboarding is active
     */
    fun activateIfNeeded(): Boolean {
        if (!onboardingManager.shouldActivate()) return false
        metricsTracker?.load() ?: metricsTracker?.start()
        showMessageForCurrentStep()
        return true
    }

    /**
     * Handle a pill-button tap from the user.
     */
    fun onPillTapped(action: OnboardingAction) {
        when (action) {
            OnboardingAction.CONTINUE -> {
                onboardingManager.onWelcomeContinue()
                showMessageForCurrentStep()
            }
            OnboardingAction.I_HAVE_KEY -> {
                onboardingManager.chooseIHaveKey()
                showMessageForCurrentStep()
            }
            OnboardingAction.HELP_ME_GET_ONE -> {
                onboardingManager.chooseHelpMeGetOne()
                showMessageForCurrentStep()
            }
            OnboardingAction.GOT_KEY_FROM_GUIDE -> {
                onboardingManager.onProviderGuideGotKey()
                showMessageForCurrentStep()
            }
            OnboardingAction.USE_RECOMMENDED_MODEL -> {
                metricsTracker?.recordModel(lastRecommendation?.model.orEmpty())
                onboardingManager.onModelSelected()
                showMessageForCurrentStep()
            }
            OnboardingAction.SEE_OTHER_MODELS -> showAlternativeModels()
            OnboardingAction.OPEN_SETTINGS -> {
                if (accessibilityHelper.openAccessibilitySettings()) {
                    onboardingManager.onAccessibilityPromptShown()
                    waitForAccessibility()
                }
            }
            OnboardingAction.RETRY_ACCESSIBILITY -> {
                onboardingManager.onAccessibilityRetry()
                showMessageForCurrentStep()
            }
            OnboardingAction.SKIP_ACCESSIBILITY -> {
                onboardingManager.skipAccessibility()
                showMessageForCurrentStep()
            }
            OnboardingAction.DISMISS -> {
                onboardingManager.dismiss()
                metricsTracker?.complete()
                onComplete?.invoke()
            }
            is OnboardingAction.FIRST_TASK_SELECTED -> {
                onFirstTaskSelected?.invoke(action.task)
            }
        }
    }

    /**
     * Called when API key validation succeeds.
     * Triggers model recommendation flow.
     */
    fun onApiKeyValidated(provider: Provider, apiKey: String) {
        validatedApiKey = apiKey
        metricsTracker?.recordProvider(provider.name)
        onboardingManager.onApiKeyValidated()
        recommendModel(provider, apiKey)
    }

    /**
     * Called after the selected first task finishes.
     */
    fun onFirstTaskCompleted(success: Boolean) {
        if (onboardingManager.currentStep != OnboardingStep.FIRST_TASK) return
        metricsTracker?.recordFirstTask(success)
        if (success) {
            onMessage?.invoke(OnboardingMessage(text = firstTaskGuide.successMessage(), pills = emptyList()))
        }
        onboardingManager.onFirstTaskFinished()
        metricsTracker?.complete()
        showMessageForCurrentStep()
    }

    private fun showMessageForCurrentStep() {
        metricsTracker?.recordStep(onboardingManager.currentStep.name)
        val message = when (onboardingManager.currentStep) {
            OnboardingStep.WELCOME -> OnboardingMessage(
                text = "Hey! I'm Citros — I can control your phone to help you get things done. Let's get set up. It'll take about a minute.",
                pills = listOf(Pill("Let's go!", OnboardingAction.CONTINUE))
            )
            OnboardingStep.API_KEY_CHOICE -> OnboardingMessage(
                text = "I need an API key to think. Do you have one?",
                pills = listOf(
                    Pill("I have a key", OnboardingAction.I_HAVE_KEY),
                    Pill("Help me get one", OnboardingAction.HELP_ME_GET_ONE)
                )
            )
            OnboardingStep.API_KEY_ENTRY -> OnboardingMessage(
                text = "Paste your API key below.",
                pills = emptyList()
            )
            OnboardingStep.API_KEY_PROVIDER_GUIDE -> OnboardingMessage(
                text = "Here are the providers you can use. Anthropic (Claude) is recommended — best quality for phone control.",
                pills = listOf(Pill("I've got a key now", OnboardingAction.GOT_KEY_FROM_GUIDE))
            )
            OnboardingStep.MODEL_SELECTION -> null // handled by recommendModel()
            OnboardingStep.ACCESSIBILITY_PROMPT -> OnboardingMessage(
                text = "One more thing — I need accessibility access to see and interact with your screen.",
                pills = listOf(
                    Pill("Open Settings", OnboardingAction.OPEN_SETTINGS),
                    Pill("Skip for now", OnboardingAction.SKIP_ACCESSIBILITY)
                )
            )
            OnboardingStep.ACCESSIBILITY_WAIT -> null // handled by waitForAccessibility()
            OnboardingStep.FIRST_TASK -> OnboardingMessage(
                text = "All set! Pick one first task:",
                pills = firstTaskGuide.suggestedTasks.map { task ->
                    Pill("${task.emoji} ${task.text}", OnboardingAction.FIRST_TASK_SELECTED(task))
                }
            )
            OnboardingStep.COMPLETE -> {
                metricsTracker?.complete()
                onComplete?.invoke()
                null
            }
        }
        message?.let { onMessage?.invoke(it) }
    }

    private fun recommendModel(provider: Provider, apiKey: String) {
        scope.launch {
            try {
                val recommendation = modelRecommender.recommend(provider, apiKey)
                lastRecommendation = recommendation
                val displayModel = AvailableModel(recommendation.model, recommendation.model).displayId
                onMessage?.invoke(
                    OnboardingMessage(
                        text = "Got it! I recommend $displayModel: ${recommendation.reason}",
                        pills = listOf(
                            Pill("Use $displayModel", OnboardingAction.USE_RECOMMENDED_MODEL),
                            Pill("See other models", OnboardingAction.SEE_OTHER_MODELS)
                        )
                    )
                )
            } catch (_: Exception) {
                // Fallback: skip model selection
                onboardingManager.onModelSelected()
                showMessageForCurrentStep()
            } finally {
                validatedApiKey = null
            }
        }
    }

    private fun showAlternativeModels() {
        val rec = lastRecommendation ?: return
        val alts = rec.alternatives.joinToString("\n") { "• ${AvailableModel(it, it).displayId}" }
        onMessage?.invoke(
            OnboardingMessage(
                text = "Other available models:\n$alts\n\nOr stick with the recommended one.",
                pills = listOf(Pill("Use ${AvailableModel(rec.model, rec.model).displayId}", OnboardingAction.USE_RECOMMENDED_MODEL))
            )
        )
    }

    private fun waitForAccessibility() {
        scope.launch {
            val waitStart = System.currentTimeMillis()
            val granted = accessibilityHelper.waitForPermission()
            if (granted) {
                val elapsed = max(0L, System.currentTimeMillis() - waitStart)
                metricsTracker?.recordAccessibilityGrant(elapsed)
                onboardingManager.onAccessibilityGranted()
                showMessageForCurrentStep()
            } else {
                onMessage?.invoke(
                    OnboardingMessage(
                        text = "I didn't detect accessibility access yet. You can retry, skip for now (chat-only mode), or open settings again.",
                        pills = listOf(
                            Pill("Open Settings", OnboardingAction.OPEN_SETTINGS),
                            Pill("Retry", OnboardingAction.RETRY_ACCESSIBILITY),
                            Pill("Skip for now", OnboardingAction.SKIP_ACCESSIBILITY)
                        )
                    )
                )
            }
        }
    }
}

/** Actions available as pill buttons during onboarding. */
sealed class OnboardingAction {
    data object CONTINUE : OnboardingAction()
    data object I_HAVE_KEY : OnboardingAction()
    data object HELP_ME_GET_ONE : OnboardingAction()
    data object GOT_KEY_FROM_GUIDE : OnboardingAction()
    data object USE_RECOMMENDED_MODEL : OnboardingAction()
    data object SEE_OTHER_MODELS : OnboardingAction()
    data object OPEN_SETTINGS : OnboardingAction()
    data object RETRY_ACCESSIBILITY : OnboardingAction()
    data object SKIP_ACCESSIBILITY : OnboardingAction()
    data object DISMISS : OnboardingAction()
    data class FIRST_TASK_SELECTED(val task: SuggestedTask) : OnboardingAction()
}

/** A pill button displayed in the chat UI. */
data class Pill(
    val label: String,
    val action: OnboardingAction
)

/** An onboarding message to display in the chat. */
data class OnboardingMessage(
    val text: String,
    val pills: List<Pill>
)
