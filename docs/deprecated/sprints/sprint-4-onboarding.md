# Sprint 4 Stream D: Zero-Infra Onboarding

*Install → add key → run your first phone task in under 2 minutes.*

**Status:** Spec
**Issues:** #470, #596, #571
**Prerequisite:** Sprints 0-3 complete
**Estimated PRs:** 3-4
**Node:** Either (Mac Mini or MacBook Pro)

---

## Problem

Fawx's current first-run experience is hostile:

1. **Install the APK.** (Sideload only — no Play Store.)
2. **Open the app.** See a blank chat screen.
3. **Find Settings.** No guidance on what to do first.
4. **Figure out API keys.** Which provider? Where do I get a key? What format? What's OpenRouter?
5. **Enter a key.** Hope you got it right. No validation feedback.
6. **Select a model.** From a list of IDs that mean nothing to a non-technical user.
7. **Grant accessibility.** System Settings → Accessibility → find Fawx → toggle.
8. **Maybe it works now?**

That's 8 steps, ~5 minutes, zero guidance. Every step is a drop-off point. The "zero-infra" promise — no VPS, no gateway, just a phone — is true architecturally but false experientially.

### Competitive context

- **OpenClaw:** Requires a VPS + config file + Telegram bot, but once set up, the assistant guides you through everything conversationally.
- **ChatGPT mobile:** Open app → sign in → chat. Three steps. (No phone control, but the onboarding bar is set.)
- **Fawx target:** Open app → guided setup → first task. Three steps, with phone control.

---

## Solution

### Architecture: Conversational Onboarding Agent

Replace the current "figure it out" flow with a lightweight onboarding agent that runs in the chat interface itself. No special UI screens — the chat IS the onboarding.

```
App Launch (first run detected)
  ↓
Onboarding Agent activates
  ↓
"Hey! I'm Fawx — I can control your phone to help you get things done.
 Let's get set up. It'll take about a minute."
  ↓
Step 1: API Key
  "I need an API key to think. Do you have one, or should I help you get one?"
  → [I have a key] → key entry flow
  → [Help me get one] → provider guide with deep links
  ↓
Step 2: Model Selection
  "Got it! Here's what I recommend:"
  → Smart defaults based on key type (Anthropic → Sonnet, OpenRouter → auto-detect)
  → One-tap accept or manual override
  ↓
Step 3: Accessibility Permission
  "One more thing — I need accessibility access to see and interact with your screen.
   I'll open the settings for you."
  → Deep link to Accessibility settings
  → Detect when permission granted, continue automatically
  ↓
Step 4: First Task
  "All set! Try saying: 'Open the weather app'"
  → Run first task with training-wheels (extra patience, verbose feedback)
  → Celebrate success 🎉
```

### Design Principles

1. **Chat-native.** The onboarding happens in the same chat interface the user will use for everything else. No separate "setup wizard" screens. This teaches the interaction pattern while setting up.

2. **Resumable.** If the user kills the app mid-setup, the agent picks up where it left off. State is persisted in SharedPreferences.

3. **Skippable.** Power users can dismiss the onboarding and configure manually. The agent detects existing valid config and skips completed steps.

4. **Conversational, not scripted.** The onboarding agent uses a real LLM (local prompt, no API key needed for this step — use hardcoded responses for pre-key steps, LLM for post-key steps).

5. **Progressive disclosure.** Don't mention advanced features (playbooks, voice I/O, steer) during onboarding. Let users discover them naturally.

---

## Design

### PR 1: Onboarding State Machine + Key Entry

**OnboardingState:**

```kotlin
enum class OnboardingStep {
    WELCOME,
    API_KEY_CHOICE,      // "Have a key" vs "Help me get one"
    API_KEY_ENTRY,       // Key input with validation
    API_KEY_PROVIDER_GUIDE, // Deep links to provider key pages
    MODEL_SELECTION,     // Smart defaults + override
    ACCESSIBILITY_PROMPT,// Deep link to settings
    ACCESSIBILITY_WAIT,  // Polling for permission grant
    FIRST_TASK,          // Guided first task
    COMPLETE             // Onboarding done, normal mode
}

/**
 * State Transition Diagram:
 *
 *  ┌─────────┐
 *  │ WELCOME │
 *  └────┬────┘
 *       │
 *  ┌────▼──────────┐
 *  │ API_KEY_CHOICE │──── "Help me" ────┐
 *  └────┬───────────┘                   │
 *       │ "I have a key"         ┌──────▼──────────────┐
 *  ┌────▼──────────┐             │ API_KEY_PROVIDER_GUIDE │
 *  │ API_KEY_ENTRY │◄────────────└──────────────────────┘
 *  └────┬──────────┘                  (got key, back)
 *       │ valid key
 *       │ invalid? ──► retry (loop back to API_KEY_ENTRY)
 *       │ network error? ──► retry / proceed with warning
 *       │ unknown provider? ──► manual provider select, then validate
 *  ┌────▼──────────────┐
 *  │ MODEL_SELECTION   │
 *  └────┬──────────────┘
 *       │
 *  ┌────▼────────────────┐
 *  │ ACCESSIBILITY_PROMPT │
 *  └────┬────────────────┘
 *       │
 *  ┌────▼─────────────────┐
 *  │ ACCESSIBILITY_WAIT   │──── timeout ──► [Try again] loops back
 *  └────┬─────────────────┘              ──► [Skip] ──► FIRST_TASK (chat-only mode)
 *       │ granted                        ──► [Open Settings] loops back
 *  ┌────▼──────────┐
 *  │ FIRST_TASK    │
 *  └────┬──────────┘
 *       │ success or skip
 *  ┌────▼──────────┐
 *  │ COMPLETE      │
 *  └───────────────┘
 *
 *  Any step: user can dismiss onboarding → COMPLETE (manual config)
 *  App killed at any step: resumes from persisted currentStep
 */

class OnboardingManager(private val prefs: SharedPreferences) {
    var currentStep: OnboardingStep
        get() = OnboardingStep.valueOf(prefs.getString("onboarding_step", "WELCOME")!!)
        set(value) = prefs.edit().putString("onboarding_step", value.name).apply()

    val isComplete: Boolean
        get() = currentStep == OnboardingStep.COMPLETE

    /**
     * Check if onboarding should activate.
     * Skip if: valid API key exists AND accessibility granted AND model selected.
     */
    fun shouldActivate(): Boolean {
        if (isComplete) return false
        if (hasValidApiKey() && hasAccessibility() && hasModelSelected()) {
            currentStep = OnboardingStep.COMPLETE
            return false
        }
        return true
    }
}
```

**Key entry flow:**

```kotlin
class ApiKeyValidator {
    sealed class ValidationResult {
        object Valid : ValidationResult()
        data class Invalid(val reason: String) : ValidationResult()
        data class ProviderDetected(val provider: Provider) : ValidationResult()
        data class ProviderUnknown(val hint: String) : ValidationResult()
        data class NetworkError(val message: String) : ValidationResult()
    }

    /**
     * Validate and auto-detect provider from key format.
     *
     * Prefix detection is a HINT only — prefixes evolve across provider versions.
     * API validation (a lightweight health-check call) is the primary signal.
     *
     * Current prefix hints (may become stale):
     * - sk-ant-* → Anthropic (note: older keys used sk-ant-api03-*, newer may differ)
     * - sk-or-* → OpenRouter
     * - sk-proj-*, sk-* → OpenAI (project keys use sk-proj-; legacy keys use sk-)
     * - gsk_* → Groq
     * - xai-* → xAI
     *
     * If prefix doesn't match any known pattern → ProviderUnknown:
     *   prompt user to select their provider manually from a list,
     *   then validate via API call against the selected provider.
     *
     * If API validation fails with a network/timeout error → NetworkError:
     *   show retry option, allow proceeding with unvalidated key (with warning).
     */
    suspend fun validate(key: String): ValidationResult
}
```

**Provider guide (#571):**
When user says "Help me get one," show provider cards with:
- Provider name + logo
- What models you get
- Price indication (free tier? pay-as-you-go?)
- Deep link to their API key page
- "I recommend Anthropic for the best phone control experience"

```kotlin
data class ProviderGuide(
    val provider: Provider,
    val displayName: String,
    val description: String,
    val keyPageUrl: String,
    val recommendation: String?,
    val hasFreeCredits: Boolean
)

val PROVIDER_GUIDES = listOf(
    ProviderGuide(
        provider = Provider.ANTHROPIC,
        displayName = "Anthropic (Claude)",
        description = "Best phone control quality. Pay-as-you-go.",
        keyPageUrl = "https://console.anthropic.com/settings/keys",
        recommendation = "Recommended — Claude Sonnet is our best-tested model",
        hasFreeCredits = false
    ),
    ProviderGuide(
        provider = Provider.OPENROUTER,
        displayName = "OpenRouter",
        description = "Access many models with one key. Some free models available.",
        keyPageUrl = "https://openrouter.ai/keys",
        recommendation = null,
        hasFreeCredits = true
    ),
    // ... more providers
)
```

**Test strategy:**
- Unit test: `OnboardingManager` state transitions, step skipping, resume-after-kill
- Unit test: `ApiKeyValidator` format detection and validation
- Unit test: `shouldActivate()` with various config states

---

### PR 2: Smart Model Selection + Accessibility Flow

**Smart model selection:**

After key validation, auto-detect the best model:

```kotlin
class ModelRecommender {
    /**
     * Recommend a model based on provider and available models.
     * Priority: Sonnet 4.5 > Sonnet 4 > Haiku > whatever's available
     * For OpenRouter: query available models, pick best Claude variant
     */
    suspend fun recommend(provider: Provider, apiKey: String): ModelRecommendation {
        val available = fetchAvailableModels(provider, apiKey)
        val recommended = available
            .sortedByDescending { MODEL_PREFERENCE_ORDER.indexOf(it.id) }
            .firstOrNull()
            ?: available.first()

        return ModelRecommendation(
            model = recommended,
            reason = "Best balance of quality and speed for phone control",
            alternatives = available.filter { it != recommended }
        )
    }
}
```

**UI: Pill-button selection** (not a dropdown):

```
┌──────────────────────────────────────────┐
│  I recommend Claude Sonnet 4.5:          │
│  Fast, accurate, great at phone control  │
│                                          │
│  [✅ Use Sonnet 4.5]  [See other models] │
└──────────────────────────────────────────┘
```

**Accessibility deep link:**

```kotlin
class AccessibilitySetupHelper(private val context: Context) {
    /**
     * Open accessibility settings with Fawx pre-selected if possible.
     */
    /**
     * Open accessibility settings using the standard system intent.
     * Note: We intentionally do NOT use undocumented extras like
     * ":settings:fragment_args_key" — these vary by Android version
     * and OEM (Samsung, Xiaomi, etc.) and are unreliable.
     */
    fun openAccessibilitySettings() {
        val intent = Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS).apply {
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        }
        context.startActivity(intent)
        // The onboarding agent shows a text instruction:
        // "Look for 'Fawx' in the list and toggle it on."
    }

    /**
     * Poll for accessibility permission.
     * Returns when granted or timeout (30s).
     *
     * On timeout, the UI shows:
     *   "I didn't detect accessibility permission."
     *   [Try again] → restart polling
     *   [Skip for now] → agent works in chat-only mode (no phone control),
     *       shows persistent banner: "Phone control disabled — enable in Settings"
     *   [Open Settings manually] → re-launch ACTION_ACCESSIBILITY_SETTINGS
     *
     * If skipped, the agent disables all phone-control tools and operates
     * as a chat-only assistant until the user grants permission from Settings.
     */
    suspend fun waitForPermission(timeoutMs: Long = 30_000): Boolean {
        val start = System.currentTimeMillis()
        while (System.currentTimeMillis() - start < timeoutMs) {
            if (isAccessibilityEnabled()) return true
            delay(500)
        }
        return false
    }
}
```

**Test strategy:**
- Unit test: `ModelRecommender` with various provider/model lists
- Unit test: `AccessibilitySetupHelper` permission detection
- Integration test: full onboarding flow with mock API responses

---

### PR 3: First Task + Guided Experience

**First task with training wheels:**

After setup completes, the agent suggests a simple task and runs it with extra patience:

```kotlin
class FirstTaskGuide {
    // Ordered so tasks that DON'T depend on text input come first.
    // "Search for pizza near me" and "Set a timer for 5 minutes" are
    // deprioritized because they depend on the text input fix (Stream B).
    // TODO: Re-evaluate this list after Stream B merges — text-input-dependent
    // tasks can be promoted once RobustTextInput is shipped.
    val suggestedTasks = listOf(
        "Open the weather app",         // No text input needed
        "Take a screenshot",            // No text input needed
        "What's on my screen?",         // No text input needed
        "What time is it?",             // No text input needed
        "Open Settings",                // No text input needed
        // --- Text-input-dependent (deprioritized until Stream B merges) ---
        // "Search for pizza near me",
        // "Set a timer for 5 minutes",
    )

    /**
     * First task runs with adjusted parameters:
     * - Higher max tool steps (15 instead of 10) for exploration
     * - Verbose progress feedback ("Opening app..." "Reading screen...")
     * - Success celebration on completion
     */
    fun firstTaskConfig(): AgentExecutorConfig {
        return AgentExecutorConfig(
            maxToolSteps = 15,
            outputVerbosity = OutputVerbosity.VERBOSE,
            progressFeedback = true
        )
    }
}
```

**Voice during onboarding:**

If the user attempts voice input during onboarding (e.g., taps the mic button before SherpaOnnx models are downloaded), the STT consent flow from Stream B kicks in. The onboarding agent should:
1. Recognize the AWAITING_CONSENT state from `SttManager`
2. Present the consent choice inline in the onboarding chat (not a separate dialog)
3. If user chooses "Type instead" or "Wait for download," smoothly continue onboarding via text input
4. If user chooses "Use cloud STT," proceed with voice and show the cloud indicator

This ensures the privacy consent is handled consistently whether voice is attempted during or after onboarding.

**Success flow:**
```
User: "Open the weather app"
Agent: 🔍 Reading your screen...
Agent: 📱 Opening Weather app...
Agent: ✅ Weather app is open! I can see it's 72°F and sunny.

🎉 Nice — your first task! Here's what I can do:
• Control any app on your phone
• Search the web and summarize results
• Remember things for later
• Take screenshots and read your screen

Just ask me anything!
```

**Onboarding analytics (local only):**

```kotlin
data class OnboardingMetrics(
    val startedAt: Long,
    val completedAt: Long?,
    val stepsCompleted: List<OnboardingStep>,
    val keyEntryAttempts: Int,
    val providerSelected: Provider?,
    val modelSelected: String?,
    val accessibilityGrantTimeMs: Long?,
    val firstTaskSuccess: Boolean?
)
```

Stored locally in SharedPreferences. No network telemetry. Used for future funnel analysis if we add opt-in analytics.

**Test strategy:**
- Unit test: `FirstTaskGuide` config adjustments
- Unit test: `OnboardingMetrics` persistence
- Integration test: complete onboarding flow end-to-end (mock LLM)
- Manual regression: fresh install on Pixel

---

### PR 4 (Optional): Onboarding Polish

If time allows after the core PRs:

- **Animated welcome:** Brief animation showing the Fawx logo + tagline on first launch (Compose Canvas, not a static image)
- **Key paste detection:** Auto-detect when user pastes an API key from clipboard, skip manual entry
- **Permission re-prompt:** If user backs out of accessibility settings without granting, offer to try again with a clearer explanation of why it's needed
- **Model preview:** "Try a free model first" option for users without API keys (route through free OpenRouter models)

---

## UX Copy Guidelines

The onboarding agent's tone should match SOUL.md:
- **Conversational, not robotic.** "Let's get set up" not "Please complete the following configuration steps."
- **Helpful, not condescending.** Assume the user is smart but unfamiliar with API keys.
- **Brief, not verbose.** Each message fits on one screen. No scrolling during onboarding.
- **Honest about requirements.** "You'll need an API key — it's like a password that lets me think" not "experience the power of AI."

---

## Out of Scope

1. **Account system.** No Fawx accounts, no server-side auth. BYO keys only.
2. **Payment/billing.** No Fawx-managed API proxying. Users pay their provider directly.
3. **Play Store.** Still sideload-only. The onboarding assumes the user already has the APK.
4. **Multi-device sync.** Onboarding state is local to the device.

## Blindspots

1. **Accessibility settings vary by OEM.** Samsung, Xiaomi, and others put accessibility settings in different places or rename/reorganize the list. We use only `ACTION_ACCESSIBILITY_SETTINGS` (no undocumented OEM extras) and add a text instruction ("Look for 'Fawx' in the list"). Some OEMs may bury accessibility under additional menus.

2. **Key validation requires network.** If the user is offline, we can detect the provider from key format but can't verify it works. We should allow proceeding with a warning.

3. **Model list may be stale.** If we hardcode model preferences, they drift as providers release new models. The `ModelRecommender` should fetch available models dynamically when possible, with a hardcoded fallback.

4. **"Help me get one" flow exits the app** to open a browser for the provider's key page. The user might not come back. Consider an in-app WebView, but that adds complexity and potential auth issues.
