package ai.citros.chat

import android.content.Context
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Visibility
import androidx.compose.material.icons.filled.VisibilityOff
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.dp
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Shadow
import androidx.compose.ui.text.style.TextDecoration
import android.util.Log
import ai.citros.core.AgentFileManager
import ai.citros.core.AnthropicClient
import ai.citros.core.Conversation
import ai.citros.core.OpenAiClient
import ai.citros.core.OpenRouterClient
import ai.citros.core.Provider
import ai.citros.core.ProviderClient
import ai.citros.core.ProviderConfig
import ai.citros.core.WalletKey
import ai.citros.core.WalletManager
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.doubleOrNull
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

internal const val ONBOARDING_PREFS = "citros_onboarding"
internal const val PREF_ONBOARDING_COMPLETE = "onboarding_complete"
internal const val PREF_SELECTED_FLAVOR = "selected_flavor"
internal const val PREF_PERSONALITY_TONE = "personality_tone"
internal const val PREF_PERSONALITY_EXPLANATION = "personality_explanation"
internal const val PREF_PERSONALITY_TRUST = "personality_trust"
internal const val PREF_SELECTED_TIER = "selected_tier"
internal const val PREF_TRIAL_START_MS = "trial_start_ms"
internal const val PREF_WAITLIST_EMAIL = "waitlist_email"
internal const val PREF_WAITLIST_TIER = "waitlist_tier"
internal const val PREF_PAYWALL_SEEN = "paywall_seen"
internal const val PREF_THEME_MODE = "theme_mode"
internal const val THEME_MODE_DEFAULT = "dark"
internal const val PREF_ONBOARDING_CHAT_SEEN = "onboarding_chat_seen"
internal const val PREF_ONBOARDING_CHAT_COMPLETE = "onboarding_chat_complete"
internal const val PREF_ONBOARDING_CHAT_SKIPPED = "onboarding_chat_skipped"
internal const val PREF_ONBOARDING_CHAT_TRANSCRIPT = "onboarding_chat_transcript"
internal const val PREF_AGENT_NAME = "agent_name"
internal const val PREF_AGENT_NATURE = "agent_nature"
internal const val PREF_AGENT_VIBE = "agent_vibe"
internal const val PREF_AGENT_EMOJI = "agent_emoji"
internal const val PREF_USER_NAME = "user_name"
internal const val PREF_USER_ADDRESS = "user_address"
internal const val PREF_RELATIONSHIP_STYLE = "relationship_style"
internal const val PREF_BOUNDARIES = "boundaries"
internal const val PREF_USER_CONTEXT = "user_context"

internal fun shouldShowOnboarding(context: Context): Boolean {
    val prefs = context.getSharedPreferences(ONBOARDING_PREFS, Context.MODE_PRIVATE)
    return !prefs.getBoolean(PREF_ONBOARDING_COMPLETE, false)
}

internal fun readSelectedFlavor(context: Context): CitrosFlavor {
    val prefs = context.getSharedPreferences(ONBOARDING_PREFS, Context.MODE_PRIVATE)
    return CitrosFlavor.fromStorage(
        prefs.getString(PREF_SELECTED_FLAVOR, CitrosFlavor.TANGERINE.storageValue)
    )
}

private fun providerKeyUrl(provider: Provider): String = when (provider) {
    Provider.ANTHROPIC -> "https://console.anthropic.com/settings/keys"
    Provider.OPENAI -> "https://platform.openai.com/api-keys"
    Provider.OPENROUTER -> "https://openrouter.ai/keys"
}

private fun providerRequiredPrefix(provider: Provider): String {
    val placeholder = ProviderUi.keyPlaceholder(provider)
    require(placeholder.endsWith("...")) { "Invalid placeholder format: $placeholder" }
    return placeholder.removeSuffix("...")
}

private enum class OnboardingStep {
    WELCOME,
    FLAVOR,
    PERSONALITY,
    ONBOARD_CHAT,
    PAYWALL,
    API_KEY,
    PERMISSIONS,
    READY
}

internal data class OnboardingChatLine(
    val id: Int,
    val role: String,
    val text: String
)

private data class IdentitySummaryChip(
    val icon: String,
    val label: String,
    val value: String
)

internal data class OnboardingIdentityProfile(
    val agentName: String,
    val agentNature: String,
    val agentVibe: String,
    val agentEmoji: String,
    val userName: String,
    val userAddress: String,
    val relationshipStyle: String,
    val boundaries: String,
    val userContext: String,
    val confidence: Float = 1f
)

private data class OnboardingChatClients(
    val conversationClient: ProviderClient,
    val extractionClient: ProviderClient
)

private data class FallbackAssistantLine(
    val text: String,
    val nextCursor: Int,
    val showSummary: Boolean
)

internal data class OnboardingScriptStep(
    val question: String,           // AI message template (with {var} placeholders)
    val captureAs: String?,         // Variable name to store user's response (null for final step)
    val responseTemplate: String?   // Optional follow-up acknowledgment before next question
)

private val onboardingJson = Json { ignoreUnknownKeys = true }
private const val ONBOARDING_COMPLETE_TOKEN = "[ONBOARDING_COMPLETE]"
// Trigger auto-extraction after 10 messages to balance between:
// - Waiting for sufficient context (avoid premature extraction)
// - Not making users wait too long to see their profile summary
private const val MIN_MESSAGES_FOR_AUTO_EXTRACTION = 10
// Brief delay before showing typing indicator for natural conversational feel
private const val TYPING_INDICATOR_DELAY_MS = 250L

private const val ONBOARDING_CHAT_SYSTEM_PROMPT = """
You are a newly activated AI assistant inside the user's phone. This is your first conversation with them.

Goals:
1) Establish your identity (name, nature, vibe, emoji)
2) Learn the user's preferred name/address
3) Learn interaction preferences and boundaries
4) Understand what they want help with

Style rules:
- Be conversational, warm, and concise (2-3 sentences).
- Avoid rigid questionnaires or corporate filler.
- Ask one focused question at a time.
- Offer suggestions when the user seems unsure.
- Do not mention internal tokens, extraction, or app mechanics.

Completion:
- When basics are clearly covered, close warmly.
- Append [ONBOARDING_COMPLETE] at the very end of your final message.
"""

private const val ONBOARDING_EXTRACTION_PROMPT = """
Extract onboarding identity/profile fields from the transcript.
Return strict JSON:
{
  "agent_name": "string|null",
  "agent_nature": "string|null",
  "agent_vibe": "string|null",
  "agent_emoji": "string|null",
  "user_name": "string|null",
  "user_address": "string|null",
  "relationship_style": "string|null",
  "boundaries": "string|null",
  "user_context": "string|null",
  "missing_fields": ["..."],
  "confidence": 0.0
}
Use null for unknowns. Do not add any commentary outside JSON.
"""

internal val onboardingScriptSteps = listOf(
    OnboardingScriptStep(
        question = "Hey! I'm brand new here, just woke up on your phone. " +
            "Before we get rolling, I'd love to figure out who I am and how you'd like me to work with you. " +
            "First — what should you call me? Pick a name for your AI assistant!",
        captureAs = "agentName",
        responseTemplate = "{agentName} — I like it! 🍋"
    ),
    OnboardingScriptStep(
        question = "And what's your name? I want to address you properly.",
        captureAs = "userName", 
        responseTemplate = "Nice to meet you, {userName}!"
    ),
    OnboardingScriptStep(
        question = "How should I talk to you: casual and direct, or more polished?",
        captureAs = "style",
        responseTemplate = "Got it, {userName}!"
    ),
    OnboardingScriptStep(
        question = "Any boundaries I should know about? Like should I always ask before sending messages or making purchases?",
        captureAs = "boundaries",
        responseTemplate = "Perfect. I'll remember that. I think I've got a good picture now — " +
            "I'm {agentName}, you're {userName}, and I'll keep things {style} while respecting your boundaries."
    )
)

private fun defaultIdentityProfile(skipped: Boolean): OnboardingIdentityProfile {
    return if (skipped) {
        OnboardingIdentityProfile(
            agentName = "Citros",
            agentNature = "citrus guide",
            agentVibe = "friendly and helpful",
            agentEmoji = "🍊",
            userName = "You",
            userAddress = "You",
            relationshipStyle = "casual, clear, and practical",
            boundaries = "ask before sending, deleting, or purchasing",
            userContext = "new user"
        )
    } else {
        OnboardingIdentityProfile(
            agentName = "Zest",
            agentNature = "citrus spirit",
            agentVibe = "chill but sharp",
            agentEmoji = "🍋",
            userName = "Joe",
            userAddress = "Joe",
            relationshipStyle = "casual, no corporate speak",
            boundaries = "ask before send/delete/purchase",
            userContext = "night owl"
        )
    }
}

private fun OnboardingIdentityProfile.asSummaryChips(): List<IdentitySummaryChip> = listOf(
    IdentitySummaryChip(icon = "🏷", label = "Name", value = agentName),
    IdentitySummaryChip(icon = agentEmoji, label = "Emoji", value = agentEmoji),
    IdentitySummaryChip(icon = "🎭", label = "Vibe", value = agentVibe),
    IdentitySummaryChip(icon = "🧑", label = "You", value = userName),
    IdentitySummaryChip(icon = "💬", label = "Style", value = relationshipStyle),
    IdentitySummaryChip(icon = "🛡", label = "Boundaries", value = boundaries)
)

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun OnboardingFlow(
    context: Context,
    walletDependencies: WalletDependencies = provideWalletDependencies(context),
    onFinished: () -> Unit
) {
    val prefs = remember(context) {
        context.getSharedPreferences(ONBOARDING_PREFS, Context.MODE_PRIVATE)
    }
    val agentFileManager = remember(context) {
        AgentFileManager.fromContext(context.applicationContext)
    }

    var step by rememberSaveable { mutableStateOf(OnboardingStep.WELCOME) }
    var selectedFlavor by rememberSaveable {
        mutableStateOf(
            CitrosFlavor.fromStorage(prefs.getString(PREF_SELECTED_FLAVOR, CitrosFlavor.TANGERINE.storageValue))
        )
    }
    val splashVisuals = remember(selectedFlavor) { citrosSplashVisualTokens(selectedFlavor) }
    var tone by rememberSaveable { mutableStateOf("Balanced") }
    var explanation by rememberSaveable { mutableStateOf("Balanced") }
    var trust by rememberSaveable { mutableStateOf("Ask for risky stuff") }

    var showWaitlistSheet by rememberSaveable { mutableStateOf(false) }
    var waitlistTier by rememberSaveable { mutableStateOf("base") }
    var waitlistEmail by rememberSaveable { mutableStateOf("") }

    fun completeOnboarding(selectedTier: String = "byo", startTrial: Boolean = true) {
        prefs.edit()
            .putBoolean(PREF_ONBOARDING_COMPLETE, true)
            .putString(PREF_SELECTED_FLAVOR, selectedFlavor.storageValue)
            .putString(PREF_PERSONALITY_TONE, tone)
            .putString(PREF_PERSONALITY_EXPLANATION, explanation)
            .putString(PREF_PERSONALITY_TRUST, trust)
            .putString(PREF_SELECTED_TIER, selectedTier)
            .putBoolean(PREF_PAYWALL_SEEN, true)
            .apply {
                if (startTrial) {
                    putLong(PREF_TRIAL_START_MS, System.currentTimeMillis())
                }
            }
            .apply()

        runCatching {
            setLauncherIconFlavor(context, selectedFlavor)
        }.onFailure { error ->
            Log.w("OnboardingFlow", "Failed to apply launcher icon flavor", error)
        }

        onFinished()
    }

    fun recordWaitlistSelection() {
        prefs.edit()
            .putString(PREF_WAITLIST_TIER, waitlistTier)
            .putString(PREF_WAITLIST_EMAIL, waitlistEmail.trim())
            .apply()
    }

    fun persistOnboardingChatProfile(
        skipped: Boolean,
        profile: OnboardingIdentityProfile,
        transcript: List<OnboardingChatLine>
    ) {
        prefs.edit()
            .putBoolean(PREF_ONBOARDING_CHAT_SEEN, transcript.isNotEmpty() || skipped)
            .putBoolean(PREF_ONBOARDING_CHAT_COMPLETE, !skipped)
            .putBoolean(PREF_ONBOARDING_CHAT_SKIPPED, skipped)
            .putString(PREF_ONBOARDING_CHAT_TRANSCRIPT, serializeTranscript(transcript))
            .putString(PREF_AGENT_NAME, profile.agentName)
            .putString(PREF_AGENT_NATURE, profile.agentNature)
            .putString(PREF_AGENT_VIBE, profile.agentVibe)
            .putString(PREF_AGENT_EMOJI, profile.agentEmoji)
            .putString(PREF_USER_NAME, profile.userName)
            .putString(PREF_USER_ADDRESS, profile.userAddress)
            .putString(PREF_RELATIONSHIP_STYLE, profile.relationshipStyle)
            .putString(PREF_BOUNDARIES, profile.boundaries)
            .putString(PREF_USER_CONTEXT, profile.userContext)
            .apply()
    }

    if (step == OnboardingStep.ONBOARD_CHAT) {
        OnboardingChatStep(
            context = context,
            flavor = selectedFlavor,
            walletDependencies = walletDependencies,
            onBack = { step = OnboardingStep.PERSONALITY },
            onSkip = { transcript ->
                val profile = defaultIdentityProfile(skipped = true)
                persistOnboardingChatProfile(
                    skipped = true,
                    profile = profile,
                    transcript = transcript
                )
                runCatching {
                    OnboardingPersistence.persistIdentityProfile(agentFileManager, profile)
                }.onFailure { error ->
                    Log.e("OnboardingFlow", "Failed to persist skipped onboarding identity", error)
                }
                step = OnboardingStep.PAYWALL
            },
            onContinue = { profile, transcript ->
                persistOnboardingChatProfile(
                    skipped = false,
                    profile = profile,
                    transcript = transcript
                )
                runCatching {
                    OnboardingPersistence.persistIdentityProfile(agentFileManager, profile)
                }.onFailure { error ->
                    Log.e("OnboardingFlow", "Failed to persist onboarding identity", error)
                }
                step = OnboardingStep.PAYWALL
            }
        )
    } else {
        val isHeroFullScreenStep = step == OnboardingStep.WELCOME || step == OnboardingStep.READY
        val horizontalContentPadding = if (isHeroFullScreenStep) 0.dp else 24.dp
        val topContentPadding = if (isHeroFullScreenStep) 0.dp else 40.dp
        val bottomContentPadding = if (isHeroFullScreenStep) 0.dp else 96.dp
        BoxWithConstraints(
            modifier = Modifier
                .fillMaxSize()
                .background(MaterialTheme.colorScheme.background)
                .testTag("onboarding_flow_root")
        ) {
        val welcomeFullHeight = this@BoxWithConstraints.maxHeight
        val personalityBodyScale = when {
            welcomeFullHeight >= 900.dp -> 1.20f
            welcomeFullHeight >= 820.dp -> 1.12f
            welcomeFullHeight >= 740.dp -> 1.06f
            else -> 1.0f
        }
        val personalitySectionSpacing = 20.dp * personalityBodyScale
        val columnModifier = Modifier
            .fillMaxWidth()
            // defaultMinSize ensures Column fills screen height, allowing CenterVertically to work
            .defaultMinSize(minHeight = maxHeight)
            .padding(
                start = horizontalContentPadding,
                end = horizontalContentPadding,
                top = topContentPadding,
                bottom = bottomContentPadding
            )
            .let { base ->
                if (
                    step == OnboardingStep.WELCOME ||
                    step == OnboardingStep.READY ||
                    step == OnboardingStep.FLAVOR ||
                    step == OnboardingStep.PERSONALITY ||
                    step == OnboardingStep.PAYWALL
                ) {
                    base
                } else {
                    base.verticalScroll(rememberScrollState())
                }
            }
        Column(
            modifier = columnModifier,
            verticalArrangement = Arrangement.spacedBy(16.dp, Alignment.CenterVertically),
            horizontalAlignment = Alignment.CenterHorizontally
        ) {
            when (step) {
                OnboardingStep.WELCOME -> {
                    Box(
                        modifier = Modifier
                            .fillMaxWidth()
                            .height(welcomeFullHeight),
                        contentAlignment = Alignment.Center
                    ) {
                        CitrosHeroShaderSphere(
                            flavor = selectedFlavor,
                            modifier = Modifier.fillMaxSize()
                        )
                        Column(
                            horizontalAlignment = Alignment.CenterHorizontally,
                            verticalArrangement = Arrangement.spacedBy(8.dp)
                        ) {
                            Text(
                                "citros",
                                style = MaterialTheme.typography.displayMedium.copy(
                                    fontSize = MaterialTheme.typography.displayMedium.fontSize * 2f
                                ),
                                fontWeight = FontWeight.Bold,
                                color = splashVisuals.brandTitleColor
                            )
                        }
                        CitrusLiquidGlassButton(
                            text = "Wake Up",
                            onClick = { step = OnboardingStep.FLAVOR },
                            modifier = Modifier
                                .align(Alignment.BottomCenter)
                                .fillMaxWidth()
                                .padding(horizontal = 24.dp, vertical = 52.dp)
                        )
                    }
                }

                OnboardingStep.FLAVOR -> {
                    Box(
                        modifier = Modifier.fillMaxSize()
                    ) {
                        Column(
                            modifier = Modifier
                                .fillMaxSize(),
                            verticalArrangement = Arrangement.spacedBy(16.dp)
                        ) {
                            CitrosStepHeader(
                                title = "Choose Your Citros",
                                stepIndex = 1,
                                totalSteps = 7,
                                onBack = { step = OnboardingStep.WELCOME },
                                titleColor = selectedFlavor.primary,
                                backLabelColor = selectedFlavor.primary.copy(alpha = 0.88f),
                                stepCounterColor = selectedFlavor.primary.copy(alpha = 0.72f),
                                activeProgressColor = selectedFlavor.primary,
                                inactiveProgressColor = selectedFlavor.primary.copy(alpha = 0.24f),
                                titleShadow = Shadow(
                                    color = splashVisuals.hero.deep.copy(alpha = 0.55f),
                                    offset = Offset(0f, 2f),
                                    blurRadius = 16f
                                ),
                                centerTitle = true
                            )

                            Text(
                                "Pick the accent style for Citros. You can change it later in Settings.",
                                style = MaterialTheme.typography.bodyMedium,
                                color = MaterialTheme.colorScheme.onBackground.copy(alpha = 0.72f)
                            )

                            Column(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .weight(1f),
                                verticalArrangement = Arrangement.Center
                            ) {
                                Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                                    CitrosFlavor.entries.forEach { flavor ->
                                        FlavorOptionCard(
                                            flavor = flavor,
                                            selected = selectedFlavor == flavor,
                                            onClick = { selectedFlavor = flavor }
                                        )
                                    }
                                }
                            }
                        }

                        CitrusLiquidGlassButton(
                            text = "Continue",
                            onClick = { step = OnboardingStep.PERSONALITY },
                            modifier = Modifier
                                .align(Alignment.BottomCenter)
                                .fillMaxWidth(0.72f)
                                .padding(bottom = 16.dp),
                            tintColor = selectedFlavor.primary
                        )
                    }
                }

                OnboardingStep.PERSONALITY -> {
                    Box(
                        modifier = Modifier.fillMaxSize()
                    ) {
                        Column(
                            modifier = Modifier
                                .fillMaxSize(),
                            verticalArrangement = Arrangement.spacedBy(16.dp)
                        ) {
                            CitrosStepHeader(
                                title = "Conversation Style",
                                stepIndex = 2,
                                totalSteps = 7,
                                onBack = { step = OnboardingStep.FLAVOR },
                                titleColor = selectedFlavor.primary,
                                backLabelColor = selectedFlavor.primary.copy(alpha = 0.88f),
                                stepCounterColor = selectedFlavor.primary.copy(alpha = 0.72f),
                                activeProgressColor = selectedFlavor.primary,
                                inactiveProgressColor = selectedFlavor.primary.copy(alpha = 0.24f),
                                titleShadow = Shadow(
                                    color = splashVisuals.hero.deep.copy(alpha = 0.55f),
                                    offset = Offset(0f, 2f),
                                    blurRadius = 16f
                                ),
                                centerTitle = true
                            )

                            Column(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .weight(1f),
                                verticalArrangement = Arrangement.Center,
                                horizontalAlignment = Alignment.CenterHorizontally
                            ) {
                                Column(
                                    modifier = Modifier.fillMaxWidth(0.92f),
                                    verticalArrangement = Arrangement.spacedBy(personalitySectionSpacing),
                                    horizontalAlignment = Alignment.CenterHorizontally
                                ) {
                                    PersonalityQuestion(
                                        question = "How should I talk to you?",
                                        selected = tone,
                                        options = listOf("Casual", "Professional", "Playful"),
                                        flavor = selectedFlavor,
                                        bodyScale = personalityBodyScale,
                                        onSelect = { tone = it }
                                    )

                                    PersonalityQuestion(
                                        question = "How much should I explain?",
                                        selected = explanation,
                                        options = listOf("Brief", "Balanced", "Detailed"),
                                        flavor = selectedFlavor,
                                        bodyScale = personalityBodyScale,
                                        onSelect = { explanation = it }
                                    )

                                    PersonalityQuestion(
                                        question = "Comfort level",
                                        selected = trust,
                                        options = listOf(
                                            "Ask before everything",
                                            "Ask for risky stuff",
                                            "Full autonomy"
                                        ),
                                        flavor = selectedFlavor,
                                        bodyScale = personalityBodyScale,
                                        onSelect = { trust = it }
                                    )
                                }
                            }
                        }

                        CitrusLiquidGlassButton(
                            text = "Continue",
                            onClick = { step = OnboardingStep.ONBOARD_CHAT },
                            modifier = Modifier
                                .align(Alignment.BottomCenter)
                                .fillMaxWidth(0.72f)
                                .padding(bottom = 16.dp),
                            tintColor = selectedFlavor.primary
                        )
                    }
                }

                OnboardingStep.PAYWALL -> {
                    val plans = listOf(
                        CitrosPlanSpec(
                            id = "byo",
                            title = "Bring Your Own Key - Free",
                            subtitle = "Use your own Anthropic, OpenAI, or OpenRouter key",
                            details = "All models, no app-level limits. 2-day trial starts now.",
                            cta = "Select",
                            accent = selectedFlavor.primary
                        ),
                        CitrosPlanSpec(
                            id = "base",
                            title = "Citros Base - $9/mo",
                            subtitle = "All models included with a monthly usage cap",
                            details = "$5 cap. Great for getting started.",
                            cta = "Join Waitlist",
                            accent = selectedFlavor.primary,
                            recommended = true,
                            comingSoon = true
                        ),
                        CitrosPlanSpec(
                            id = "super",
                            title = "Citros Super - $29/mo",
                            subtitle = "Full catalog with higher monthly usage cap",
                            details = "$50 cap for power users.",
                            cta = "Join Waitlist",
                            accent = selectedFlavor.primary,
                            comingSoon = true
                        )
                    )

                    Box(
                        modifier = Modifier.fillMaxSize()
                    ) {
                        Column(
                            modifier = Modifier
                                .fillMaxSize(),
                            verticalArrangement = Arrangement.spacedBy(16.dp)
                        ) {
                            CitrosStepHeader(
                                title = "Choose Your Plan",
                                stepIndex = 4,
                                totalSteps = 7,
                                onBack = { step = OnboardingStep.ONBOARD_CHAT },
                                titleColor = selectedFlavor.primary,
                                backLabelColor = selectedFlavor.primary.copy(alpha = 0.88f),
                                stepCounterColor = selectedFlavor.primary.copy(alpha = 0.72f),
                                activeProgressColor = selectedFlavor.primary,
                                inactiveProgressColor = selectedFlavor.primary.copy(alpha = 0.24f),
                                titleShadow = Shadow(
                                    color = splashVisuals.hero.deep.copy(alpha = 0.55f),
                                    offset = Offset(0f, 2f),
                                    blurRadius = 16f
                                ),
                                centerTitle = true
                            )

                            Text(
                                "Base and Super are coming soon. You can continue now with your own API key.",
                                style = MaterialTheme.typography.bodyMedium,
                                color = selectedFlavor.primary.copy(alpha = 0.90f),
                                textAlign = TextAlign.Center,
                                modifier = Modifier.fillMaxWidth()
                            )

                            LazyColumn(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .weight(1f),
                                verticalArrangement = Arrangement.spacedBy(12.dp),
                                contentPadding = PaddingValues(bottom = 8.dp)
                            ) {
                                items(plans, key = { it.id }) { plan ->
                                    PlanCard(
                                        plan = plan,
                                        testTag = "paywall_plan_${plan.id}",
                                        onSelect = {
                                            if (plan.id == "byo") {
                                                prefs.edit()
                                                    .putString(PREF_SELECTED_TIER, "byo")
                                                    .putBoolean(PREF_PAYWALL_SEEN, true)
                                                    .apply()
                                                step = OnboardingStep.API_KEY
                                            } else {
                                                waitlistTier = plan.id
                                                showWaitlistSheet = true
                                            }
                                        }
                                    )
                                }
                            }
                        }

                        Text(
                            text = "I'll decide later",
                            style = MaterialTheme.typography.labelMedium.copy(
                                textDecoration = TextDecoration.Underline
                            ),
                            color = selectedFlavor.primary.copy(alpha = 0.86f),
                            modifier = Modifier
                                .align(Alignment.BottomCenter)
                                .clickable {
                                    prefs.edit()
                                        .putString(PREF_SELECTED_TIER, "byo")
                                        .putBoolean(PREF_PAYWALL_SEEN, true)
                                        .apply()
                                    step = OnboardingStep.API_KEY
                                }
                                .padding(bottom = 20.dp)
                        )
                    }
                }

                OnboardingStep.API_KEY -> {
                    Box(
                        modifier = Modifier.fillMaxSize()
                    ) {
                        Column(
                            modifier = Modifier
                                .fillMaxSize(),
                            verticalArrangement = Arrangement.spacedBy(16.dp)
                        ) {
                            CitrosStepHeader(
                                title = "Connect an AI Provider",
                                stepIndex = 5,
                                totalSteps = 7,
                                onBack = { step = OnboardingStep.PAYWALL },
                                titleColor = selectedFlavor.primary,
                                backLabelColor = selectedFlavor.primary.copy(alpha = 0.88f),
                                stepCounterColor = selectedFlavor.primary.copy(alpha = 0.72f),
                                activeProgressColor = selectedFlavor.primary,
                                inactiveProgressColor = selectedFlavor.primary.copy(alpha = 0.24f),
                                titleShadow = Shadow(
                                    color = splashVisuals.hero.deep.copy(alpha = 0.55f),
                                    offset = Offset(0f, 2f),
                                    blurRadius = 16f
                                ),
                                centerTitle = true
                            )

                            Text(
                                "Paste your API key to get started",
                                style = MaterialTheme.typography.bodyMedium,
                                color = selectedFlavor.primary.copy(alpha = 0.9f),
                                textAlign = TextAlign.Center,
                                modifier = Modifier.fillMaxWidth()
                            )

                            var apiKeyText by rememberSaveable { mutableStateOf("") }
                            var apiKeyVisible by rememberSaveable { mutableStateOf(false) }
                            var apiKeyLabel by rememberSaveable { mutableStateOf("") }
                            var selectedApiProvider by rememberSaveable { mutableStateOf(Provider.ANTHROPIC) }
                            var connectionStatus by rememberSaveable { mutableStateOf<String?>(null) }
                            var isValidating by rememberSaveable { mutableStateOf(false) }
                            val validationScope = rememberCoroutineScope()

                            CitrosLiquidGlassSurface(
                                modifier = Modifier.fillMaxWidth(),
                                shape = RoundedCornerShape(20.dp),
                                borderColor = selectedFlavor.primary.copy(alpha = 0.30f),
                                borderWidth = 1.dp,
                                highlightColor = selectedFlavor.primary,
                                warmth = 0.94f,
                                contentPadding = PaddingValues(horizontal = 12.dp, vertical = 12.dp)
                            ) {
                                Column(
                                    modifier = Modifier.fillMaxWidth(),
                                    verticalArrangement = Arrangement.spacedBy(12.dp)
                                ) {
                                    Row(
                                        modifier = Modifier.fillMaxWidth(),
                                        horizontalArrangement = Arrangement.spacedBy(8.dp)
                                    ) {
                                        Provider.entries.forEach { provider ->
                                            PersonalityOptionChip(
                                                text = ProviderUi.displayName(provider),
                                                selected = selectedApiProvider == provider,
                                                flavor = selectedFlavor,
                                                scale = 1.05f,
                                                onClick = {
                                                    selectedApiProvider = provider
                                                    connectionStatus = null
                                                }
                                            )
                                        }
                                    }

                                    val providerUrl = providerKeyUrl(selectedApiProvider).removePrefix("https://")

                                    Text(
                                        "Get a key from $providerUrl",
                                        style = MaterialTheme.typography.bodySmall.copy(
                                            textDecoration = TextDecoration.Underline
                                        ),
                                        color = selectedFlavor.primary.copy(alpha = 0.9f),
                                        textAlign = TextAlign.Center,
                                        modifier = Modifier
                                            .fillMaxWidth()
                                            .clickable {
                                                val url = providerKeyUrl(selectedApiProvider)
                                                try {
                                                    context.startActivity(
                                                        android.content.Intent(
                                                            android.content.Intent.ACTION_VIEW,
                                                            android.net.Uri.parse(url)
                                                        )
                                                    )
                                                } catch (e: Exception) {
                                                    Log.w("OnboardingFlow", "Intent launch failed", e)
                                                }
                                            }
                                    )
                                }
                            }

                            val inputColors = OutlinedTextFieldDefaults.colors(
                                focusedBorderColor = selectedFlavor.primary,
                                unfocusedBorderColor = selectedFlavor.primary.copy(alpha = 0.38f),
                                focusedLabelColor = selectedFlavor.primary,
                                unfocusedLabelColor = selectedFlavor.primary.copy(alpha = 0.72f),
                                cursorColor = selectedFlavor.primary
                            )

                            CitrosLiquidGlassSurface(
                                modifier = Modifier.fillMaxWidth(),
                                shape = RoundedCornerShape(20.dp),
                                borderColor = selectedFlavor.primary.copy(alpha = 0.30f),
                                borderWidth = 1.dp,
                                highlightColor = selectedFlavor.primary,
                                warmth = 0.92f,
                                contentPadding = PaddingValues(horizontal = 12.dp, vertical = 12.dp)
                            ) {
                                Column(
                                    modifier = Modifier.fillMaxWidth(),
                                    verticalArrangement = Arrangement.spacedBy(8.dp)
                                ) {
                                    OutlinedTextField(
                                        value = apiKeyText,
                                        onValueChange = {
                                            apiKeyText = it
                                            connectionStatus = null
                                        },
                                        label = { Text("API Key") },
                                        placeholder = { Text(ProviderUi.keyPlaceholder(selectedApiProvider)) },
                                        visualTransformation = if (apiKeyVisible) VisualTransformation.None else PasswordVisualTransformation(),
                                        trailingIcon = {
                                            IconButton(onClick = { apiKeyVisible = !apiKeyVisible }) {
                                                Icon(
                                                    imageVector = if (apiKeyVisible) Icons.Default.VisibilityOff else Icons.Default.Visibility,
                                                    contentDescription = if (apiKeyVisible) "Hide key" else "Show key",
                                                    tint = MaterialTheme.colorScheme.onSurfaceVariant
                                                )
                                            }
                                        },
                                        singleLine = true,
                                        shape = RoundedCornerShape(16.dp),
                                        colors = inputColors,
                                        modifier = Modifier.fillMaxWidth()
                                    )

                                    OutlinedTextField(
                                        value = apiKeyLabel,
                                        onValueChange = { apiKeyLabel = it },
                                        label = { Text("Label (optional)") },
                                        placeholder = { Text("e.g. Personal Anthropic") },
                                        singleLine = true,
                                        shape = RoundedCornerShape(16.dp),
                                        colors = inputColors,
                                        modifier = Modifier.fillMaxWidth()
                                    )

                                    connectionStatus?.let { status ->
                                        val statusColor = when {
                                            status.startsWith("✅") -> selectedFlavor.primary.copy(alpha = 0.95f)
                                            status.startsWith("🔄") -> selectedFlavor.primary.copy(alpha = 0.82f)
                                            else -> MaterialTheme.colorScheme.error
                                        }
                                        Text(
                                            status,
                                            style = MaterialTheme.typography.bodySmall,
                                            color = statusColor
                                        )
                                    }
                                }
                            }

                            Spacer(Modifier.weight(1f))

                            CitrusLiquidGlassButton(
                                text = if (isValidating) "Validating..." else "Validate Key",
                                onClick = {
                                    val trimmed = apiKeyText.trim()
                                    val formatError = when {
                                        trimmed.isEmpty() -> "❌ Please enter a key"
                                        trimmed.length < 20 -> "❌ Key too short (minimum 20 characters)"
                                        !trimmed.startsWith(providerRequiredPrefix(selectedApiProvider)) ->
                                            "❌ ${ProviderUi.displayName(selectedApiProvider)} keys must start with ${providerRequiredPrefix(selectedApiProvider)}"
                                        else -> null
                                    }
                                    if (formatError != null) {
                                        connectionStatus = formatError
                                        return@CitrusLiquidGlassButton
                                    }
                                    isValidating = true
                                    connectionStatus = "🔄 Testing connection..."
                                    validationScope.launch {
                                        val status = validateApiCredential(trimmed, selectedApiProvider)
                                        connectionStatus = when (status) {
                                            ApiKeyValidationStatus.VALID -> "✅ Key is valid — connection successful!"
                                            ApiKeyValidationStatus.INVALID -> "❌ Invalid key — check your key and try again"
                                            ApiKeyValidationStatus.EXPIRED -> "❌ Key has expired — generate a new one"
                                            ApiKeyValidationStatus.UNKNOWN -> "⚠️ Could not verify — check your internet connection"
                                        }
                                        isValidating = false
                                    }
                                },
                                modifier = Modifier.fillMaxWidth(),
                                enabled = !isValidating,
                                tintColor = selectedFlavor.primary.copy(alpha = 0.88f)
                            )

                            CitrusLiquidGlassButton(
                                text = "Start Chatting",
                                onClick = {
                                    if (apiKeyText.isNotBlank()) {
                                        val walletManager = walletDependencies.walletManager
                                        val provider = selectedApiProvider

                                        val label = apiKeyLabel.trim().ifEmpty { defaultLabelFor(provider) }
                                        val newKey = walletManager.addKey(provider, label, apiKeyText.trim())

                                        runCatching {
                                            walletManager.setActiveKey(newKey.id)
                                            walletManager.setChatModel(ai.citros.core.ModelConfig.defaultChatModel(provider))
                                            walletManager.setActionModel(ai.citros.core.ModelConfig.defaultActionModel(provider))
                                        }.onFailure { error ->
                                            Log.e("OnboardingFlow", "Failed to activate key ${newKey.id}", error)
                                        }

                                        prefs.edit()
                                            .putString("onboarding_api_provider", provider.name.lowercase())
                                            .apply()
                                    }
                                    step = OnboardingStep.PERMISSIONS
                                },
                                modifier = Modifier
                                    .fillMaxWidth(0.72f)
                                    .align(Alignment.CenterHorizontally),
                                tintColor = selectedFlavor.primary
                            )

                            Text(
                                text = "Skip for now",
                                style = MaterialTheme.typography.labelLarge.copy(
                                    textDecoration = TextDecoration.Underline
                                ),
                                color = selectedFlavor.primary.copy(alpha = 0.88f),
                                textAlign = TextAlign.Center,
                                modifier = Modifier
                                    .align(Alignment.CenterHorizontally)
                                    .testTag("api_key_skip_btn")
                                    .clickable { step = OnboardingStep.PERMISSIONS }
                                    .padding(vertical = 4.dp)
                            )
                        }
                    }
                }

                OnboardingStep.PERMISSIONS -> {
                    Box(
                        modifier = Modifier.fillMaxSize()
                    ) {
                        Column(
                            modifier = Modifier
                                .fillMaxSize()
                                .padding(bottom = 92.dp),
                            verticalArrangement = Arrangement.spacedBy(16.dp)
                        ) {
                            CitrosStepHeader(
                                title = "Phone Control",
                                stepIndex = 6,
                                totalSteps = 7,
                                onBack = { step = OnboardingStep.API_KEY },
                                titleColor = selectedFlavor.primary,
                                backLabelColor = selectedFlavor.primary.copy(alpha = 0.88f),
                                stepCounterColor = selectedFlavor.primary.copy(alpha = 0.72f),
                                activeProgressColor = selectedFlavor.primary,
                                inactiveProgressColor = selectedFlavor.primary.copy(alpha = 0.24f),
                                titleShadow = Shadow(
                                    color = splashVisuals.hero.deep.copy(alpha = 0.55f),
                                    offset = Offset(0f, 2f),
                                    blurRadius = 16f
                                ),
                                centerTitle = true
                            )

                            Text(
                                "Enable phone control to let Citros interact with your screen",
                                style = MaterialTheme.typography.bodyMedium,
                                color = selectedFlavor.primary.copy(alpha = 0.9f),
                                textAlign = TextAlign.Center,
                                modifier = Modifier.fillMaxWidth()
                            )

                            CitrosLiquidGlassSurface(
                                modifier = Modifier.fillMaxWidth(),
                                shape = RoundedCornerShape(18.dp),
                                borderColor = selectedFlavor.primary.copy(alpha = 0.32f),
                                borderWidth = 1.dp,
                                highlightColor = selectedFlavor.primary,
                                warmth = 0.92f,
                                contentPadding = PaddingValues(horizontal = 14.dp, vertical = 14.dp)
                            ) {
                                Row(
                                    modifier = Modifier.fillMaxWidth(),
                                    verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.spacedBy(12.dp)
                                ) {
                                    Column(
                                        modifier = Modifier.weight(1f),
                                        verticalArrangement = Arrangement.spacedBy(4.dp)
                                    ) {
                                        Text(
                                            "Accessibility Service",
                                            style = MaterialTheme.typography.titleSmall,
                                            fontWeight = FontWeight.SemiBold,
                                            color = selectedFlavor.primary.copy(alpha = 0.94f)
                                        )
                                        Text(
                                            "Let Citros read and interact with your screen",
                                            style = MaterialTheme.typography.bodySmall,
                                            color = selectedFlavor.primary.copy(alpha = 0.76f)
                                        )
                                    }
                                    CitrosLiquidGlassSurface(
                                        shape = RoundedCornerShape(999.dp),
                                        onClick = {
                                            try {
                                                context.startActivity(
                                                    android.content.Intent(android.provider.Settings.ACTION_ACCESSIBILITY_SETTINGS)
                                                )
                                            } catch (e: Exception) {
                                                Log.w("OnboardingFlow", "Intent launch failed", e)
                                            }
                                        },
                                        borderColor = selectedFlavor.primary.copy(alpha = 0.44f),
                                        borderWidth = 1.dp,
                                        highlightColor = selectedFlavor.primary,
                                        warmth = 1.04f,
                                        contentPadding = PaddingValues(horizontal = 14.dp, vertical = 8.dp)
                                    ) {
                                        Text(
                                            "Enable",
                                            style = MaterialTheme.typography.labelLarge,
                                            fontWeight = FontWeight.SemiBold,
                                            color = selectedFlavor.primary.copy(alpha = 0.95f)
                                        )
                                    }
                                }
                            }

                            CitrosLiquidGlassSurface(
                                modifier = Modifier.fillMaxWidth(),
                                shape = RoundedCornerShape(18.dp),
                                borderColor = selectedFlavor.primary.copy(alpha = 0.32f),
                                borderWidth = 1.dp,
                                highlightColor = selectedFlavor.primary,
                                warmth = 0.92f,
                                contentPadding = PaddingValues(horizontal = 14.dp, vertical = 14.dp)
                            ) {
                                Row(
                                    modifier = Modifier.fillMaxWidth(),
                                    verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.spacedBy(12.dp)
                                ) {
                                    Column(
                                        modifier = Modifier.weight(1f),
                                        verticalArrangement = Arrangement.spacedBy(4.dp)
                                    ) {
                                        Text(
                                            "Overlay Permission",
                                            style = MaterialTheme.typography.titleSmall,
                                            fontWeight = FontWeight.SemiBold,
                                            color = selectedFlavor.primary.copy(alpha = 0.94f)
                                        )
                                        Text(
                                            "Show Citros as a floating bubble",
                                            style = MaterialTheme.typography.bodySmall,
                                            color = selectedFlavor.primary.copy(alpha = 0.76f)
                                        )
                                    }
                                    CitrosLiquidGlassSurface(
                                        shape = RoundedCornerShape(999.dp),
                                        onClick = {
                                            try {
                                                context.startActivity(
                                                    android.content.Intent(
                                                        android.provider.Settings.ACTION_MANAGE_OVERLAY_PERMISSION,
                                                        android.net.Uri.parse("package:${context.packageName}")
                                                    )
                                                )
                                            } catch (e: Exception) {
                                                Log.w("OnboardingFlow", "Intent launch failed", e)
                                            }
                                        },
                                        borderColor = selectedFlavor.primary.copy(alpha = 0.44f),
                                        borderWidth = 1.dp,
                                        highlightColor = selectedFlavor.primary,
                                        warmth = 1.04f,
                                        contentPadding = PaddingValues(horizontal = 14.dp, vertical = 8.dp)
                                    ) {
                                        Text(
                                            "Enable",
                                            style = MaterialTheme.typography.labelLarge,
                                            fontWeight = FontWeight.SemiBold,
                                            color = selectedFlavor.primary.copy(alpha = 0.95f)
                                        )
                                    }
                                }
                            }

                            Spacer(Modifier.weight(1f))

                            Text(
                                "You can enable these later in Settings → Phone Control",
                                style = MaterialTheme.typography.bodySmall,
                                color = selectedFlavor.primary.copy(alpha = 0.72f),
                                textAlign = TextAlign.Center,
                                modifier = Modifier.fillMaxWidth()
                            )
                        }

                        CitrusLiquidGlassButton(
                            text = "Continue",
                            onClick = { step = OnboardingStep.READY },
                            modifier = Modifier
                                .align(Alignment.BottomCenter)
                                .fillMaxWidth(0.72f)
                                .padding(bottom = 16.dp)
                                .testTag("permissions_continue_btn"),
                            tintColor = selectedFlavor.primary
                        )
                    }
                }

                OnboardingStep.READY -> {
                    Box(
                        modifier = Modifier
                            .fillMaxWidth()
                            .height(welcomeFullHeight),
                        contentAlignment = Alignment.Center
                    ) {
                        CitrosHeroShaderSphere(
                            flavor = selectedFlavor,
                            modifier = Modifier.fillMaxSize()
                        )

                        Text(
                            "Let's go!",
                            style = MaterialTheme.typography.displaySmall.copy(
                                fontSize = MaterialTheme.typography.displaySmall.fontSize * 1.45f
                            ),
                            fontWeight = FontWeight.Bold,
                            color = selectedFlavor.primary,
                            textAlign = TextAlign.Center,
                            modifier = Modifier.fillMaxWidth(0.9f)
                        )

                        CitrusLiquidGlassButton(
                            text = "Hello",
                            onClick = {
                                val tier = prefs.getString(PREF_SELECTED_TIER, "byo") ?: "byo"
                                completeOnboarding(selectedTier = tier, startTrial = true)
                            },
                            modifier = Modifier
                                .align(Alignment.BottomCenter)
                                .fillMaxWidth()
                                .padding(horizontal = 24.dp, vertical = 52.dp)
                                .testTag("ready_start_btn"),
                            tintColor = selectedFlavor.primary
                        )
                    }
                }

                OnboardingStep.ONBOARD_CHAT -> Unit
            }
        }
        } // BoxWithConstraints
    }

    if (showWaitlistSheet) {
        val validEmail = isLikelyValidEmail(waitlistEmail)
        val waitlistTierLabel = when (waitlistTier) {
            "base" -> "Citros Base"
            "super" -> "Citros Super"
            else -> "This tier"
        }

        ModalBottomSheet(
            onDismissRequest = { showWaitlistSheet = false },
            containerColor = Color.Transparent,
            scrimColor = selectedFlavor.primary.copy(alpha = 0.22f),
            dragHandle = null
        ) {
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 14.dp, vertical = 8.dp)
            ) {
                CitrosLiquidGlassSurface(
                    modifier = Modifier.fillMaxWidth(),
                    shape = RoundedCornerShape(28.dp),
                    baseColor = Color(0xE6070709),
                    borderColor = selectedFlavor.primary.copy(alpha = 0.44f),
                    borderWidth = 1.dp,
                    highlightColor = selectedFlavor.primary,
                    warmth = 0.88f,
                    contentPadding = PaddingValues(horizontal = 18.dp, vertical = 18.dp)
                ) {
                    Column(
                        modifier = Modifier.fillMaxWidth(),
                        verticalArrangement = Arrangement.spacedBy(12.dp)
                    ) {
                        Text(
                            "Coming Soon",
                            style = MaterialTheme.typography.titleLarge,
                            fontWeight = FontWeight.SemiBold,
                            color = selectedFlavor.primary.copy(alpha = 0.96f)
                        )
                        Text(
                            "$waitlistTierLabel is almost ready. Leave your email and we will notify you at launch.",
                            style = MaterialTheme.typography.bodyMedium,
                            color = selectedFlavor.primary.copy(alpha = 0.84f)
                        )

                        OutlinedTextField(
                            value = waitlistEmail,
                            onValueChange = { waitlistEmail = it.trimStart() },
                            label = { Text("Email") },
                            placeholder = { Text("you@example.com") },
                            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Email),
                            singleLine = true,
                            shape = RoundedCornerShape(16.dp),
                            colors = OutlinedTextFieldDefaults.colors(
                                focusedBorderColor = selectedFlavor.primary,
                                unfocusedBorderColor = selectedFlavor.primary.copy(alpha = 0.42f),
                                focusedLabelColor = selectedFlavor.primary,
                                unfocusedLabelColor = selectedFlavor.primary.copy(alpha = 0.74f),
                                cursorColor = selectedFlavor.primary
                            ),
                            modifier = Modifier
                                .fillMaxWidth()
                                .testTag("waitlist_email_field")
                        )

                        CitrusLiquidGlassButton(
                            text = "Notify Me",
                            onClick = {
                                recordWaitlistSelection()
                                showWaitlistSheet = false
                            },
                            enabled = validEmail,
                            modifier = Modifier.fillMaxWidth(),
                            tintColor = selectedFlavor.primary
                        )

                        Text(
                            text = "Continue With BYO For Now",
                            style = MaterialTheme.typography.labelLarge.copy(
                                textDecoration = TextDecoration.Underline
                            ),
                            color = selectedFlavor.primary.copy(alpha = 0.9f),
                            textAlign = TextAlign.Center,
                            modifier = Modifier
                                .fillMaxWidth()
                                .clickable {
                                    recordWaitlistSelection()
                                    showWaitlistSheet = false
                                    completeOnboarding(selectedTier = "byo", startTrial = true)
                                }
                                .padding(vertical = 6.dp)
                        )
                    }
                }

                Spacer(Modifier.height(6.dp))
            }
        }
    }
}

@Composable
private fun OnboardingChatHeader(
    flavor: CitrosFlavor,
    onBack: () -> Unit,
    onSkip: () -> Unit
) {
    val heroDeep = remember(flavor) { citrosSplashVisualTokens(flavor).hero.deep }

    Column(
        modifier = Modifier
            .fillMaxWidth()
            .statusBarsPadding()
            .padding(horizontal = 16.dp, vertical = 12.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp)
    ) {
        CitrosStepHeader(
            title = "Getting to know each other",
            stepIndex = 3,
            totalSteps = 7,
            onBack = onBack,
            titleColor = flavor.primary,
            backLabelColor = flavor.primary.copy(alpha = 0.88f),
            stepCounterColor = flavor.primary.copy(alpha = 0.72f),
            activeProgressColor = flavor.primary,
            inactiveProgressColor = flavor.primary.copy(alpha = 0.24f),
            titleShadow = Shadow(
                color = heroDeep.copy(alpha = 0.55f),
                offset = Offset(0f, 2f),
                blurRadius = 16f
            ),
            centerTitle = true
        )
        Row(
            modifier = Modifier
                .fillMaxWidth(),
            horizontalArrangement = Arrangement.End
        ) {
            CitrosLiquidGlassSurface(
                shape = RoundedCornerShape(999.dp),
                onClick = onSkip,
                borderColor = flavor.primary.copy(alpha = 0.58f),
                borderWidth = 1.dp,
                highlightColor = flavor.primary,
                warmth = 0.96f,
                contentPadding = PaddingValues(horizontal = 14.dp, vertical = 8.dp),
                modifier = Modifier
                    .testTag("onboarding_chat_skip_btn")
                    .semantics { role = androidx.compose.ui.semantics.Role.Button }
            ) {
                Text(
                    text = "Skip",
                    style = MaterialTheme.typography.labelLarge,
                    color = flavor.primary.copy(alpha = 0.92f)
                )
            }
        }
    }
}

@Composable
@OptIn(ExperimentalLayoutApi::class)
private fun OnboardingIdentitySummary(
    profile: OnboardingIdentityProfile,
    flavor: CitrosFlavor,
    onContinue: () -> Unit,
    onEdit: () -> Unit
) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(
            text = "Identity Summary",
            style = MaterialTheme.typography.labelSmall,
            color = flavor.primary.copy(alpha = 0.74f),
            modifier = Modifier.padding(top = 8.dp, bottom = 4.dp)
        )
        FlowRow(
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            profile.asSummaryChips().forEach { chip ->
                CitrosLiquidGlassSurface(
                    shape = RoundedCornerShape(999.dp),
                    borderColor = flavor.primary.copy(alpha = 0.36f),
                    borderWidth = 1.dp,
                    highlightColor = flavor.primary,
                    warmth = 0.82f,
                    contentPadding = PaddingValues(horizontal = 10.dp, vertical = 7.dp)
                ) {
                    Row(
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(4.dp)
                    ) {
                        Text(
                            chip.icon,
                            style = MaterialTheme.typography.labelSmall,
                            modifier = Modifier.semantics { contentDescription = "${chip.label} icon" }
                        )
                        Text(
                            text = "${chip.label}:",
                            style = MaterialTheme.typography.labelSmall,
                            color = flavor.primary.copy(alpha = 0.80f)
                        )
                        Text(
                            text = chip.value,
                            style = MaterialTheme.typography.labelSmall,
                            color = flavor.primary.copy(alpha = 0.96f),
                            fontWeight = FontWeight.SemiBold
                        )
                    }
                }
            }
        }
        CitrusLiquidGlassButton(
            text = "Looks good - continue",
            onClick = onContinue,
            modifier = Modifier
                .fillMaxWidth()
                .testTag("onboarding_chat_continue_btn")
                .semantics { contentDescription = "Continue with this identity profile" },
            tintColor = flavor.primary
        )
        CitrosLiquidGlassSurface(
            onClick = onEdit,
            shape = RoundedCornerShape(999.dp),
            borderColor = flavor.primary.copy(alpha = 0.42f),
            borderWidth = 1.dp,
            highlightColor = flavor.primary,
            warmth = 0.82f,
            contentPadding = PaddingValues(horizontal = 14.dp, vertical = 10.dp),
            modifier = Modifier
                .fillMaxWidth()
                .semantics { contentDescription = "Edit identity details" }
        ) {
            Text(
                text = "Edit details",
                style = MaterialTheme.typography.labelLarge,
                color = flavor.primary.copy(alpha = 0.92f),
                modifier = Modifier.align(Alignment.Center)
            )
        }
    }
}

@Composable
@OptIn(ExperimentalLayoutApi::class)
private fun OnboardingChatStep(
    context: Context,
    flavor: CitrosFlavor,
    walletDependencies: WalletDependencies,
    onBack: () -> Unit,
    onSkip: (List<OnboardingChatLine>) -> Unit,
    onContinue: (OnboardingIdentityProfile, List<OnboardingChatLine>) -> Unit
) {
    val prefs = remember(context) {
        context.getSharedPreferences(ONBOARDING_PREFS, Context.MODE_PRIVATE)
    }
    val restoredTranscript = remember(prefs) {
        deserializeTranscript(prefs.getString(PREF_ONBOARDING_CHAT_TRANSCRIPT, null))
    }
    val clients = remember(context, walletDependencies) { createOnboardingChatClients(walletDependencies.walletManager) }
    val coroutineScope = rememberCoroutineScope()

    // Clean up client resources when composable is disposed
    DisposableEffect(clients) {
        onDispose {
            // ProviderClient instances may hold network connections or coroutine jobs
            // Currently ProviderClient interface doesn't expose a close() method,
            // but this effect ensures we're tracking lifecycle properly for future cleanup needs
        }
    }

    val chatLines = remember { mutableStateListOf<OnboardingChatLine>().apply { addAll(restoredTranscript) } }
    var nextMessageId by rememberSaveable {
        mutableIntStateOf((restoredTranscript.maxOfOrNull { it.id } ?: 0) + 1)
    }
    // scriptCursor removed - was unused in the new scripted flow
    var currentStepIndex by rememberSaveable { mutableIntStateOf(0) }
    val capturedVariables = remember { mutableMapOf<String, String>() }

    var draftMessage by rememberSaveable { mutableStateOf("") }
    var isAssistantTyping by rememberSaveable { mutableStateOf(false) }
    var statusMessage by rememberSaveable { mutableStateOf<String?>(null) }
    var summaryProfile by remember {
        mutableStateOf(
            if (prefs.getBoolean(PREF_ONBOARDING_CHAT_COMPLETE, false)) {
                loadIdentityProfileFromPrefs(prefs)
            } else {
                null
            }
        )
    }

    fun persistDraftTranscript() {
        prefs.edit()
            .putBoolean(PREF_ONBOARDING_CHAT_SEEN, chatLines.isNotEmpty())
            .putBoolean(PREF_ONBOARDING_CHAT_COMPLETE, false)
            .putBoolean(PREF_ONBOARDING_CHAT_SKIPPED, false)
            .putString(PREF_ONBOARDING_CHAT_TRANSCRIPT, serializeTranscript(chatLines))
            .apply()
    }

    fun appendLine(role: String, text: String) {
        val content = text.trim()
        if (content.isEmpty()) return
        chatLines.add(
            OnboardingChatLine(
                id = nextMessageId,
                role = role,
                text = content
            )
        )
        nextMessageId += 1
        persistDraftTranscript()
    }

    fun completeWithProfile(profile: OnboardingIdentityProfile) {
        onContinue(profile, chatLines.toList())
    }

    fun queueFallbackAssistantResponse() {
        if (currentStepIndex >= onboardingScriptSteps.size) {
            // Script complete, build profile from captured variables
            summaryProfile = buildProfileFromCapturedVariables(capturedVariables)
            return
        }

        val currentStep = onboardingScriptSteps[currentStepIndex]
        
        // Only show question if it exists (not null)
        currentStep.question?.let { question ->
            val questionText = substituteVariables(question, capturedVariables)
            appendLine(role = "assistant", text = questionText)
        }
        
        if (currentStepIndex == onboardingScriptSteps.size - 1) {
            // This was the last step
            summaryProfile = buildProfileFromCapturedVariables(capturedVariables)
        }
    }

    fun handleFallbackUserInput(userInput: String) {
        // Capture the user input for the current step
        if (currentStepIndex < onboardingScriptSteps.size) {
            val currentStep = onboardingScriptSteps[currentStepIndex]
            currentStep.captureAs?.let { variableName ->
                capturedVariables[variableName] = cleanCapturedInput(variableName, userInput.trim())
            }
            
            // Move to next step
            currentStepIndex++
            
            // If there's a response template, show it first
            currentStep.responseTemplate?.let { template ->
                val responseText = substituteVariables(template, capturedVariables)
                appendLine(role = "assistant", text = responseText)
            }
            
            // Queue next question or complete
            queueFallbackAssistantResponse()
        }
    }

    fun maybeExtractProfile(force: Boolean = false) {
        if (summaryProfile != null || chatLines.isEmpty()) return
        val shouldExtract = force || chatLines.size >= MIN_MESSAGES_FOR_AUTO_EXTRACTION
        if (!shouldExtract) return

        val extractionClients = clients
        if (extractionClients == null) {
            summaryProfile = defaultIdentityProfile(skipped = false)
            return
        }

        coroutineScope.launch {
            try {
                val extracted = extractProfileFromTranscript(
                    client = extractionClients.extractionClient,
                    transcript = chatLines.toList()
                )
                summaryProfile = extracted ?: defaultIdentityProfile(skipped = false)
            } catch (e: Exception) {
                Log.w("OnboardingChat", "Profile extraction failed", e)
                summaryProfile = defaultIdentityProfile(skipped = false)
            }
        }
    }

    fun sendUserMessage() {
        if (isAssistantTyping) return
        val content = draftMessage.trim()
        if (content.isEmpty()) return

        draftMessage = ""
        statusMessage = null
        appendLine(role = "user", text = content)

        coroutineScope.launch {
            try {
                isAssistantTyping = true
                delay(TYPING_INDICATOR_DELAY_MS)

                val responseText = if (clients == null) {
                    handleFallbackUserInput(content)
                    null
                } else {
                    val conversation = buildConversationFromTranscript(chatLines.toList())
                    val result = clients.conversationClient.chat(conversation)
                    result.getOrElse { error ->
                        statusMessage = "Live onboarding unavailable: ${error.message ?: "provider error"}"
                        queueFallbackAssistantResponse()
                        null
                    }
                }

                responseText?.let { raw ->
                    val (cleanText, hasCompletionToken) = sanitizeAssistantResponse(raw)
                    appendLine(
                        role = "assistant",
                        text = if (cleanText.isNotEmpty()) {
                            cleanText
                        } else {
                            "I think I have enough to get started together."
                        }
                    )
                    if (hasCompletionToken) {
                        maybeExtractProfile(force = true)
                    } else {
                        maybeExtractProfile(force = false)
                    }
                }
            } catch (e: Exception) {
                Log.e("OnboardingChat", "Send message failed", e)
                statusMessage = "Message failed: ${e.message}"
            } finally {
                isAssistantTyping = false
            }
        }
    }

    LaunchedEffect(Unit) {
        if (chatLines.isNotEmpty()) {
            maybeExtractProfile(force = false)
            return@LaunchedEffect
        }

        if (clients == null) {
            queueFallbackAssistantResponse()
            statusMessage = "No active API key yet. Running preview onboarding chat."
            return@LaunchedEffect
        }

        isAssistantTyping = true
        val openingSeed = Conversation().apply {
            addUser("Start this onboarding identity conversation now.")
        }
        val opening = clients.conversationClient.chat(openingSeed).getOrElse { error ->
            statusMessage = "Live onboarding unavailable: ${error.message ?: "provider error"}"
            null
        }
        if (opening == null) {
            queueFallbackAssistantResponse()
        } else {
            val (cleanText, hasCompletionToken) = sanitizeAssistantResponse(opening)
            appendLine(
                role = "assistant",
                text = if (cleanText.isNotEmpty()) {
                    cleanText
                } else {
                    "I think I have enough to get started together."
                }
            )
            if (hasCompletionToken) {
                maybeExtractProfile(force = true)
            }
        }
        isAssistantTyping = false
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background)
            .imePadding()
            .testTag("onboarding_chat_step_root")
    ) {
        OnboardingChatHeader(
            flavor = flavor,
            onBack = onBack,
            onSkip = { onSkip(chatLines.toList()) }
        )

        statusMessage?.let { status ->
            CitrosLiquidGlassSurface(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 16.dp),
                shape = RoundedCornerShape(12.dp),
                borderColor = flavor.primary.copy(alpha = 0.32f),
                borderWidth = 1.dp,
                highlightColor = flavor.primary,
                warmth = 0.78f,
                contentPadding = PaddingValues(horizontal = 12.dp, vertical = 10.dp)
            ) {
                Text(
                    text = status,
                    style = MaterialTheme.typography.bodySmall,
                    color = flavor.primary.copy(alpha = 0.90f)
                )
            }
        }

        val chatListState = rememberLazyListState()

        // Auto-scroll to bottom when new messages arrive
        LaunchedEffect(chatLines.size, isAssistantTyping) {
            if (chatLines.isNotEmpty()) {
                kotlinx.coroutines.yield()
                chatListState.animateScrollToItem((chatListState.layoutInfo.totalItemsCount - 1).coerceAtLeast(0))
            }
        }

        LazyColumn(
            state = chatListState,
            modifier = Modifier.weight(1f),
            contentPadding = PaddingValues(horizontal = 16.dp, vertical = 12.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp)
        ) {
            items(chatLines, key = { it.id }) { line ->
                OnboardingChatMessageBubble(
                    line = line,
                    flavor = flavor
                )
            }

            if (isAssistantTyping) {
                item {
                    OnboardingChatTypingBubble(flavor = flavor)
                }
            }

            summaryProfile?.let { profile ->
                item {
                    OnboardingIdentitySummary(
                        profile = profile,
                        flavor = flavor,
                        onContinue = { completeWithProfile(profile) },
                        onEdit = { summaryProfile = null }
                    )
                }
            }
        }

        CitrosLiquidGlassSurface(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 10.dp, vertical = 8.dp),
            shape = RoundedCornerShape(24.dp),
            borderColor = flavor.primary.copy(alpha = 0.30f),
            borderWidth = 1.dp,
            highlightColor = flavor.primary,
            warmth = 0.88f,
            contentPadding = PaddingValues(horizontal = 14.dp, vertical = 10.dp)
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically
            ) {
                OutlinedTextField(
                    value = draftMessage,
                    onValueChange = { draftMessage = it },
                    placeholder = { Text("Type a message...") },
                    singleLine = true,
                    shape = RoundedCornerShape(999.dp),
                    colors = OutlinedTextFieldDefaults.colors(
                        focusedBorderColor = flavor.primary,
                        focusedLabelColor = flavor.primary,
                        cursorColor = flavor.primary
                    ),
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Send),
                    keyboardActions = KeyboardActions(
                        onSend = {
                            if (draftMessage.isNotBlank() && !isAssistantTyping) {
                                sendUserMessage()
                            }
                        }
                    ),
                    modifier = Modifier.weight(1f),
                    trailingIcon = {
                        IconButton(
                            onClick = { sendUserMessage() },
                            enabled = draftMessage.isNotBlank() && !isAssistantTyping
                        ) {
                            Icon(
                                Icons.AutoMirrored.Filled.Send,
                                contentDescription = "Send",
                                tint = if (draftMessage.isNotBlank()) {
                                    flavor.primary
                                } else {
                                    MaterialTheme.colorScheme.onSurface.copy(alpha = 0.35f)
                                }
                            )
                        }
                    }
                )
            }
        }
    }
}

@Composable
private fun OnboardingChatMessageBubble(
    line: OnboardingChatLine,
    flavor: CitrosFlavor
) {
    val isUser = line.role == "user"
    if (isUser) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.End
        ) {
            CitrosLiquidGlassSurface(
                modifier = Modifier.widthIn(max = 320.dp),
                shape = RoundedCornerShape(16.dp),
                borderColor = flavor.primary.copy(alpha = 0.44f),
                borderWidth = 1.dp,
                highlightColor = flavor.primary,
                warmth = 1.04f,
                contentPadding = PaddingValues(horizontal = 12.dp, vertical = 10.dp)
            ) {
                Text(
                    text = line.text,
                    style = MaterialTheme.typography.bodyMedium,
                    color = flavor.primary.copy(alpha = 0.96f)
                )
            }
        }
    } else {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.Start
        ) {
            Row(
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalAlignment = Alignment.Top
            ) {
                CitrusHeroBadge(flavor = flavor, size = 28)
                CitrosLiquidGlassSurface(
                    modifier = Modifier.widthIn(max = 330.dp),
                    shape = RoundedCornerShape(16.dp),
                    borderColor = flavor.primary.copy(alpha = 0.28f),
                    borderWidth = 1.dp,
                    warmth = 0.74f,
                    contentPadding = PaddingValues(horizontal = 12.dp, vertical = 10.dp)
                ) {
                    Text(
                        text = line.text,
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.92f)
                    )
                }
            }
        }
    }
}

@Composable
private fun OnboardingChatTypingBubble(flavor: CitrosFlavor) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.Start
    ) {
        Row(
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            CitrusHeroBadge(flavor = flavor, size = 28)
            CitrosLiquidGlassSurface(
                shape = RoundedCornerShape(16.dp),
                borderColor = flavor.primary.copy(alpha = 0.28f),
                borderWidth = 1.dp,
                warmth = 0.74f,
                contentPadding = PaddingValues(horizontal = 12.dp, vertical = 12.dp)
            ) {
                Row(
                    horizontalArrangement = Arrangement.spacedBy(5.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    repeat(3) { index ->
                        val alpha = when (index) {
                            0 -> 0.45f
                            1 -> 0.7f
                            else -> 1f
                        }
                        Box(
                            modifier = Modifier
                                .size(6.dp)
                                .background(
                                    color = flavor.primary.copy(alpha = alpha),
                                    shape = CircleShape
                                )
                        )
                    }
                }
            }
        }
    }
}

private fun createOnboardingChatClients(walletManager: WalletManager): OnboardingChatClients? {
    return runCatching {
        val activeConfig = walletManager.activeConfig() ?: return null

        OnboardingChatClients(
            conversationClient = createProviderClientForOnboarding(
                config = activeConfig,
                systemPrompt = ONBOARDING_CHAT_SYSTEM_PROMPT.trimIndent()
            ),
            extractionClient = createProviderClientForOnboarding(
                config = activeConfig,
                systemPrompt = ONBOARDING_EXTRACTION_PROMPT.trimIndent()
            )
        )
    }.getOrNull()
}

private fun createProviderClientForOnboarding(
    config: ProviderConfig,
    systemPrompt: String
): ProviderClient {
    return when (config.provider) {
        Provider.ANTHROPIC -> AnthropicClient(config = config, systemPrompt = systemPrompt)
        Provider.OPENAI -> OpenAiClient(config = config, systemPrompt = systemPrompt)
        Provider.OPENROUTER -> OpenRouterClient(config = config, systemPrompt = systemPrompt)
    }
}

private fun loadIdentityProfileFromPrefs(
    prefs: android.content.SharedPreferences
): OnboardingIdentityProfile? {
    val agentName = prefs.getString(PREF_AGENT_NAME, null) ?: return null
    val agentNature = prefs.getString(PREF_AGENT_NATURE, null) ?: return null
    val agentVibe = prefs.getString(PREF_AGENT_VIBE, null) ?: return null
    val agentEmoji = prefs.getString(PREF_AGENT_EMOJI, null) ?: return null
    val userName = prefs.getString(PREF_USER_NAME, null) ?: return null
    val userAddress = prefs.getString(PREF_USER_ADDRESS, null) ?: userName
    val relationshipStyle = prefs.getString(PREF_RELATIONSHIP_STYLE, null) ?: "casual and clear"
    val boundaries = prefs.getString(PREF_BOUNDARIES, null) ?: "ask before risky actions"
    val userContext = prefs.getString(PREF_USER_CONTEXT, null) ?: "new user"

    return OnboardingIdentityProfile(
        agentName = agentName,
        agentNature = agentNature,
        agentVibe = agentVibe,
        agentEmoji = agentEmoji,
        userName = userName,
        userAddress = userAddress,
        relationshipStyle = relationshipStyle,
        boundaries = boundaries,
        userContext = userContext,
        confidence = 1f
    )
}

// Removed restoreScriptCursor and nextFallbackAssistantLine - these were leftover 
// from the old script system and referenced undefined onboardingChatScript

internal fun sanitizeAssistantResponse(raw: String): Pair<String, Boolean> {
    val hasCompletionToken = raw.contains(ONBOARDING_COMPLETE_TOKEN)
    val text = raw.replace(ONBOARDING_COMPLETE_TOKEN, "").trim()
    return text to hasCompletionToken
}

private fun buildConversationFromTranscript(transcript: List<OnboardingChatLine>): Conversation {
    val conversation = Conversation()
    transcript.forEach { line ->
        when (line.role) {
            "assistant" -> conversation.addAssistant(line.text)
            else -> conversation.addUser(line.text)
        }
    }
    return conversation
}

private suspend fun extractProfileFromTranscript(
    client: ProviderClient,
    transcript: List<OnboardingChatLine>
): OnboardingIdentityProfile? {
    if (transcript.isEmpty()) return null

    val extractionInput = transcript.joinToString(separator = "\n") { line ->
        "${line.role.uppercase()}: ${line.text}"
    }
    val conversation = Conversation().apply { addUser(extractionInput) }
    val raw = client.chat(conversation).getOrNull() ?: return null
    return parseProfileFromExtractionJson(raw)
}

private fun parseProfileFromExtractionJson(raw: String): OnboardingIdentityProfile? {
    val jsonObject = extractFirstJsonObject(raw) ?: return null
    val fallback = defaultIdentityProfile(skipped = false)

    fun field(name: String): String? {
        val value = jsonObject[name]?.jsonPrimitive?.contentOrNull?.trim()
        return value?.takeIf { it.isNotBlank() && !it.equals("null", ignoreCase = true) }
    }

    val confidence = jsonObject["confidence"]?.jsonPrimitive?.doubleOrNull?.toFloat() ?: fallback.confidence

    return fallback.copy(
        agentName = field("agent_name") ?: fallback.agentName,
        agentNature = field("agent_nature") ?: fallback.agentNature,
        agentVibe = field("agent_vibe") ?: fallback.agentVibe,
        agentEmoji = field("agent_emoji") ?: fallback.agentEmoji,
        userName = field("user_name") ?: fallback.userName,
        userAddress = field("user_address") ?: fallback.userAddress,
        relationshipStyle = field("relationship_style") ?: fallback.relationshipStyle,
        boundaries = field("boundaries") ?: fallback.boundaries,
        userContext = field("user_context") ?: fallback.userContext,
        confidence = confidence.coerceIn(0f, 1f)
    )
}

internal fun extractFirstJsonObject(raw: String): JsonObject? {
    val start = raw.indexOf('{')
    if (start < 0) return null

    var depth = 0
    var inString = false
    var escaping = false

    for (index in start until raw.length) {
        val char = raw[index]

        if (inString) {
            if (escaping) {
                escaping = false
            } else if (char == '\\') {
                escaping = true
            } else if (char == '"') {
                inString = false
            }
            continue
        }

        when (char) {
            '"' -> inString = true
            '{' -> depth += 1
            '}' -> {
                depth -= 1
                if (depth == 0) {
                    val candidate = raw.substring(start, index + 1)
                    return runCatching {
                        onboardingJson.parseToJsonElement(candidate).jsonObject
                    }.getOrNull()
                }
            }
        }
    }

    return null
}

internal fun serializeTranscript(transcript: List<OnboardingChatLine>): String {
    val payload = buildJsonArray {
        transcript.forEach { line ->
            add(
                buildJsonObject {
                    put("id", JsonPrimitive(line.id))
                    put("role", JsonPrimitive(line.role))
                    put("text", JsonPrimitive(line.text))
                }
            )
        }
    }
    return payload.toString()
}

internal fun deserializeTranscript(raw: String?): List<OnboardingChatLine> {
    if (raw.isNullOrBlank()) return emptyList()

    val jsonArray: JsonArray = runCatching {
        onboardingJson.parseToJsonElement(raw).jsonArray
    }.getOrElse { return emptyList() }

    return jsonArray.mapIndexedNotNull { index, element ->
        try {
            val obj = element.jsonObject
            val role = obj["role"]?.jsonPrimitive?.contentOrNull ?: return@mapIndexedNotNull null
            val text = obj["text"]?.jsonPrimitive?.contentOrNull ?: return@mapIndexedNotNull null
            val id = obj["id"]?.jsonPrimitive?.intOrNull ?: (index + 1)
            OnboardingChatLine(id = id, role = role, text = text)
        } catch (e: Exception) {
            // Skip malformed entries
            null
        }
    }.sortedBy { it.id }
}

@Composable
@OptIn(ExperimentalLayoutApi::class)
private fun PersonalityQuestion(
    question: String,
    selected: String,
    options: List<String>,
    flavor: CitrosFlavor,
    bodyScale: Float = 1f,
    onSelect: (String) -> Unit
) {
    val scale = bodyScale.coerceIn(1f, 1.22f)
    Column(
        modifier = Modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(8.dp * scale),
        horizontalAlignment = Alignment.CenterHorizontally
    ) {
        Text(
            text = question,
            style = MaterialTheme.typography.titleSmall.copy(
                fontSize = MaterialTheme.typography.titleSmall.fontSize * scale
            ),
            fontWeight = FontWeight.SemiBold,
            color = flavor.primary.copy(alpha = 0.92f),
            textAlign = TextAlign.Center,
            modifier = Modifier.fillMaxWidth()
        )
        FlowRow(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(8.dp * scale, Alignment.CenterHorizontally),
            verticalArrangement = Arrangement.spacedBy(8.dp * scale)
        ) {
            options.forEach { option ->
                PersonalityOptionChip(
                    text = option,
                    selected = selected == option,
                    flavor = flavor,
                    scale = scale,
                    onClick = { onSelect(option) }
                )
            }
        }
    }
}

internal fun isLikelyValidEmail(raw: String): Boolean {
    val email = raw.trim()
    if (email.isEmpty()) return false
    // Use Android's built-in email validation for robust pattern matching
    return android.util.Patterns.EMAIL_ADDRESS.matcher(email).matches()
}

/**
 * Clean captured input for name fields by stripping common conversational prefixes.
 * e.g. "My name is Joe" → "Joe", "I'm Joe" → "Joe", "Call me Joe" → "Joe"
 */
internal fun cleanCapturedInput(variableName: String, input: String): String {
    if (variableName !in setOf("userName", "agentName")) return input

    val prefixes = listOf(
        "my name is ", "i'm ", "im ", "i am ", "call me ", "it's ", "its ",
        "you can call me ", "just call me ", "they call me ", "name's ",
        "the name is ", "name is "
    )
    val lower = input.lowercase()
    for (prefix in prefixes) {
        if (lower.startsWith(prefix)) {
            val stripped = input.drop(prefix.length).trim()
            if (stripped.isNotBlank()) return stripped
        }
    }
    return input
}

internal fun substituteVariables(template: String, variables: Map<String, String>): String {
    var result = template
    for ((key, value) in variables) {
        result = result.replace("{$key}", value)
    }
    return result
}

internal fun buildProfileFromCapturedVariables(variables: Map<String, String>): OnboardingIdentityProfile {
    val agentName = variables["agentName"] ?: "Citros"
    val userName = variables["userName"] ?: "You"
    val style = variables["style"] ?: "helpful and friendly"
    val boundaries = variables["boundaries"] ?: "ask before taking actions"
    
    return OnboardingIdentityProfile(
        agentName = agentName,
        agentNature = "citrus spirit",
        agentVibe = "chill but sharp",
        agentEmoji = "🍋",
        userName = userName,
        userAddress = userName,
        relationshipStyle = style,
        boundaries = boundaries,
        userContext = "onboarding completed",
        confidence = 1.0f
    )
}
