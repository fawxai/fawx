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
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.style.TextAlign
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
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.dp
import androidx.compose.ui.text.style.TextDecoration
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
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
internal const val PREF_SELECTED_FLAVOR_OPTION = "selected_flavor_option"
internal const val PREF_PERSONALITY_TONE = "personality_tone"
internal const val PREF_PERSONALITY_EXPLANATION = "personality_explanation"
internal const val PREF_PERSONALITY_TRUST = "personality_trust"
internal const val PREF_SELECTED_TIER = "selected_tier"
internal const val PREF_TRIAL_START_MS = "trial_start_ms"
internal const val PREF_WAITLIST_EMAIL = "waitlist_email"
internal const val PREF_WAITLIST_TIER = "waitlist_tier"
internal const val PREF_PAYWALL_SEEN = "paywall_seen"
internal const val PREF_THEME_MODE = "theme_mode"
internal const val THEME_MODE_DEFAULT = "system"
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
    val option = prefs.getString(PREF_SELECTED_FLAVOR_OPTION, null)
    if (option == CitrosFlavor.NONE.storageValue) {
        return CitrosFlavor.NONE
    }
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
    ACQUAINTED,
    ONBOARD_CHAT,
    PAYWALL,
    API_KEY,
    PERMISSIONS,
    TRUST,
    READY
}
private fun onboardingBackTarget(step: OnboardingStep): OnboardingStep? = when (step) {
    OnboardingStep.FLAVOR -> OnboardingStep.WELCOME
    OnboardingStep.PERSONALITY -> OnboardingStep.FLAVOR
    OnboardingStep.ACQUAINTED -> OnboardingStep.PERSONALITY
    OnboardingStep.PERMISSIONS -> OnboardingStep.ACQUAINTED
    OnboardingStep.TRUST -> OnboardingStep.PERMISSIONS
    OnboardingStep.PAYWALL -> OnboardingStep.TRUST
    OnboardingStep.API_KEY -> OnboardingStep.PAYWALL
    OnboardingStep.WELCOME,
    OnboardingStep.ONBOARD_CHAT,
    OnboardingStep.READY -> null
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
@OptIn(ExperimentalLayoutApi::class)
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
    val systemIsDarkTheme = LocalCitrosIsDark.current
    var selectedThemeMode by rememberSaveable {
        mutableStateOf(
            (prefs.getString(PREF_THEME_MODE, THEME_MODE_DEFAULT) ?: THEME_MODE_DEFAULT)
                .takeIf { it == "dark" || it == "light" }
                ?: if (systemIsDarkTheme) "dark" else "light"
        )
    }
    val isDarkTheme = when (selectedThemeMode) {
        "dark" -> true
        "light" -> false
        else -> systemIsDarkTheme
    }
    val directiveSurfaces = remember(isDarkTheme) { citrosDirectiveSurfaces(isDarkTheme) }
    val directiveFlavorTokens = remember(selectedFlavor, directiveSurfaces) {
        citrosDirectiveFlavorTokens(selectedFlavor, directiveSurfaces)
    }
    var tone by rememberSaveable { mutableStateOf("Balanced") }
    var explanation by rememberSaveable { mutableStateOf("Balanced") }
    var trust by rememberSaveable { mutableStateOf("Ask for sensitive actions only") }
    var trustLevel by rememberSaveable { mutableStateOf("Balanced") }
    var acquaintedName by rememberSaveable {
        mutableStateOf(prefs.getString(PREF_USER_NAME, "") ?: "")
    }
    var acquaintedInterests by rememberSaveable { mutableStateOf(emptyList<String>()) }
    val acquaintedInterestOptions = remember {
        listOf(
            "Productivity", "Fitness", "Finance",
            "Travel", "Music", "News", "Shopping",
            "Cooking", "Work", "Social"
        )
    }
    var selectedFlavorOption by rememberSaveable {
        mutableStateOf(
            prefs.getString(PREF_SELECTED_FLAVOR_OPTION, selectedFlavor.storageValue)
                ?: selectedFlavor.storageValue
        )
    }
    var conversationStyle by rememberSaveable { mutableStateOf("Balanced") }
    val onboardingAccentColor = if (selectedFlavorOption == "none") {
        if (isDarkTheme) Color.White else Color.Black
    } else {
        selectedFlavor.primary
    }
    val onboardingFutureDotColor = if (isDarkTheme) {
        Color(0xFF48484A)
    } else {
        Color(0xFFC7C7CC)
    }
    var showWaitlistSheet by rememberSaveable { mutableStateOf(false) }
    var waitlistTier by rememberSaveable { mutableStateOf("base") }
    var waitlistEmail by rememberSaveable { mutableStateOf("") }
    fun completeOnboarding(selectedTier: String = "byo", startTrial: Boolean = true) {
        prefs.edit()
            .putBoolean(PREF_ONBOARDING_COMPLETE, true)
            .putString(PREF_SELECTED_FLAVOR, selectedFlavor.storageValue)
            .putString(PREF_SELECTED_FLAVOR_OPTION, selectedFlavorOption)
            .putString(PREF_THEME_MODE, selectedThemeMode)
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
                .background(directiveSurfaces.background)
                .testTag("onboarding_flow_root")
        ) {
        val welcomeFullHeight = this@BoxWithConstraints.maxHeight
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
                    step == OnboardingStep.ACQUAINTED ||
                    step == OnboardingStep.API_KEY ||
                    step == OnboardingStep.PERMISSIONS ||
                    step == OnboardingStep.TRUST ||
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
                            .height(welcomeFullHeight)
                            .background(if (isDarkTheme) Color.Black else Color(0xFFE5E5EA))
                            .citrosFlavorWash(
                                washColor = directiveFlavorTokens.washColor,
                                centerXFraction = 0.5f,
                                centerYFraction = 0.42f,
                                radiusFraction = 0.86f
                            ),
                        contentAlignment = Alignment.Center
                    ) {
                            OnboardingProgressDots(
                                stepIndex = 1,
                                totalSteps = 9,
                                accentColor = onboardingAccentColor,
                                futureColor = onboardingFutureDotColor,
                                modifier = Modifier
                                    .align(Alignment.TopCenter)
                                    .statusBarsPadding()
                                .padding(top = 12.dp)
                        )
                        Column(
                            modifier = Modifier
                                .fillMaxWidth()
                                .padding(horizontal = 34.dp),
                            horizontalAlignment = Alignment.CenterHorizontally
                        ) {
                            CitrosDirectiveOrb(
                                flavor = selectedFlavor,
                                size = 80.dp
                            )
                            Spacer(Modifier.height(24.dp))
                            Text(
                                text = "Citros",
                                style = CitrosTypography.headlineMedium,
                                fontWeight = FontWeight.SemiBold,
                                color = if (isDarkTheme) Color.White else Color.Black
                            )
                            Spacer(Modifier.height(10.dp))
                            Text(
                                text = "Your phone, thinking ahead.",
                                style = CitrosTypography.bodyLarge,
                                color = if (isDarkTheme) Color(0xFF8E8E93) else Color(0xFF6D6D72),
                                textAlign = TextAlign.Center
                            )
                            Spacer(Modifier.height(30.dp))
                            Box(
                                modifier = Modifier
                                    .fillMaxWidth(0.92f)
                                    .height(54.dp)
                                    .background(onboardingAccentColor, RoundedCornerShape(14.dp))
                                    .testTag(TEST_TAG_ONBOARDING_CONTINUE_WELCOME)
                                    .clickable { step = OnboardingStep.FLAVOR },
                                contentAlignment = Alignment.Center
                            ) {
                                Text(
                                    text = "Get Started",
                                    style = CitrosTypography.titleMedium,
                                    fontWeight = FontWeight.SemiBold,
                                    color = contrastOn(onboardingAccentColor)
                                )
                            }
                        }
                    }
                }
                OnboardingStep.FLAVOR -> {
                    Box(
                        modifier = Modifier.fillMaxSize()
                    ) {
                        Column(
                            modifier = Modifier
                                .fillMaxSize()
                                .padding(start = 6.dp, end = 6.dp, top = 10.dp, bottom = 110.dp),
                            horizontalAlignment = Alignment.CenterHorizontally,
                            verticalArrangement = Arrangement.spacedBy(14.dp)
                        ) {
                            OnboardingProgressDots(
                                stepIndex = 2,
                                totalSteps = 9,
                                accentColor = onboardingAccentColor,
                                futureColor = onboardingFutureDotColor
                            )
                            CitrosIcon(
                                imageVector = CitrosIcons.Palette,
                                contentDescription = null,
                                tint = onboardingAccentColor,
                                modifier = Modifier.size(20.dp)
                            )
                            Text(
                                text = "Make It Yours",
                                style = CitrosTypography.headlineMedium,
                                fontWeight = FontWeight.SemiBold,
                                color = directiveSurfaces.labelPrimary
                            )
                            Text(
                                "Pick a flavor and theme. You can change these anytime.",
                                style = CitrosTypography.bodyLarge,
                                color = directiveSurfaces.labelSecondary,
                                textAlign = TextAlign.Center,
                                modifier = Modifier.fillMaxWidth(0.88f)
                            )
                            Spacer(Modifier.height(2.dp))
                            Text(
                                text = "FLAVOR",
                                style = CitrosTypography.labelLarge,
                                color = directiveSurfaces.labelSecondary.copy(alpha = 0.85f)
                            )
                            Row(
                                modifier = Modifier.fillMaxWidth(),
                                horizontalArrangement = Arrangement.spacedBy(8.dp)
                            ) {
                                Box(modifier = Modifier.weight(1f), contentAlignment = Alignment.Center) {
                                    OnboardingFlavorOption(
                                        label = "None",
                                        selected = selectedFlavorOption == "none",
                                        flavor = CitrosFlavor.NONE,
                                        isDarkTheme = isDarkTheme,
                                        onClick = {
                                            selectedFlavorOption = "none"
                                            selectedFlavor = CitrosFlavor.NONE
                                        }
                                    )
                                }
                                Box(modifier = Modifier.weight(1f), contentAlignment = Alignment.Center) {
                                    OnboardingFlavorOption(
                                        label = CitrosFlavor.LEMON.displayName,
                                        selected = selectedFlavorOption == CitrosFlavor.LEMON.storageValue,
                                        flavor = CitrosFlavor.LEMON,
                                        isDarkTheme = isDarkTheme,
                                        onClick = {
                                            selectedFlavorOption = CitrosFlavor.LEMON.storageValue
                                            selectedFlavor = CitrosFlavor.LEMON
                                        }
                                    )
                                }
                                Box(modifier = Modifier.weight(1f), contentAlignment = Alignment.Center) {
                                    OnboardingFlavorOption(
                                        label = CitrosFlavor.TANGERINE.displayName,
                                        selected = selectedFlavorOption == CitrosFlavor.TANGERINE.storageValue,
                                        flavor = CitrosFlavor.TANGERINE,
                                        isDarkTheme = isDarkTheme,
                                        onClick = {
                                            selectedFlavorOption = CitrosFlavor.TANGERINE.storageValue
                                            selectedFlavor = CitrosFlavor.TANGERINE
                                        }
                                    )
                                }
                            }
                            Row(
                                modifier = Modifier.fillMaxWidth(),
                                horizontalArrangement = Arrangement.spacedBy(8.dp)
                            ) {
                                Box(modifier = Modifier.weight(1f), contentAlignment = Alignment.Center) {
                                    OnboardingFlavorOption(
                                        label = CitrosFlavor.LIME.displayName,
                                        selected = selectedFlavorOption == CitrosFlavor.LIME.storageValue,
                                        flavor = CitrosFlavor.LIME,
                                        isDarkTheme = isDarkTheme,
                                        onClick = {
                                            selectedFlavorOption = CitrosFlavor.LIME.storageValue
                                            selectedFlavor = CitrosFlavor.LIME
                                        }
                                    )
                                }
                                Box(modifier = Modifier.weight(1f), contentAlignment = Alignment.Center) {
                                    OnboardingFlavorOption(
                                        label = CitrosFlavor.BLOOD_ORANGE.displayName,
                                        selected = selectedFlavorOption == CitrosFlavor.BLOOD_ORANGE.storageValue,
                                        flavor = CitrosFlavor.BLOOD_ORANGE,
                                        isDarkTheme = isDarkTheme,
                                        onClick = {
                                            selectedFlavorOption = CitrosFlavor.BLOOD_ORANGE.storageValue
                                            selectedFlavor = CitrosFlavor.BLOOD_ORANGE
                                        }
                                    )
                                }
                                Box(modifier = Modifier.weight(1f), contentAlignment = Alignment.Center) {
                                    OnboardingFlavorOption(
                                        label = CitrosFlavor.GRAPEFRUIT.displayName,
                                        selected = selectedFlavorOption == CitrosFlavor.GRAPEFRUIT.storageValue,
                                        flavor = CitrosFlavor.GRAPEFRUIT,
                                        isDarkTheme = isDarkTheme,
                                        onClick = {
                                            selectedFlavorOption = CitrosFlavor.GRAPEFRUIT.storageValue
                                            selectedFlavor = CitrosFlavor.GRAPEFRUIT
                                        }
                                    )
                                }
                            }
                            Spacer(Modifier.height(2.dp))
                            Text(
                                text = "THEME",
                                style = CitrosTypography.labelLarge,
                                color = directiveSurfaces.labelSecondary.copy(alpha = 0.85f)
                            )
                            Row(
                                modifier = Modifier.fillMaxWidth(),
                                horizontalArrangement = Arrangement.spacedBy(14.dp, Alignment.CenterHorizontally)
                            ) {
                                OnboardingThemeOptionCard(
                                    label = "Dark",
                                    selected = selectedThemeMode == "dark",
                                    accentColor = onboardingAccentColor,
                                    isDarkPreview = true,
                                    isDarkTheme = isDarkTheme,
                                    onClick = {
                                        selectedThemeMode = "dark"
                                        prefs.edit().putString(PREF_THEME_MODE, "dark").apply()
                                    }
                                )
                                OnboardingThemeOptionCard(
                                    label = "Light",
                                    selected = selectedThemeMode == "light",
                                    accentColor = onboardingAccentColor,
                                    isDarkPreview = false,
                                    isDarkTheme = isDarkTheme,
                                    onClick = {
                                        selectedThemeMode = "light"
                                        prefs.edit().putString(PREF_THEME_MODE, "light").apply()
                                    }
                                )
                            }
                            Spacer(Modifier.weight(1f))
                        }
                        Column(
                            modifier = Modifier
                                .align(Alignment.BottomCenter)
                                .fillMaxWidth()
                                .padding(horizontal = 20.dp, vertical = 14.dp),
                            horizontalAlignment = Alignment.CenterHorizontally,
                            verticalArrangement = Arrangement.spacedBy(10.dp)
                        ) {
                            Box(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .height(50.dp)
                                    .background(onboardingAccentColor, RoundedCornerShape(14.dp))
                                    .testTag(TEST_TAG_ONBOARDING_CONTINUE_FLAVOR)
                                    .clickable {
                                        prefs.edit()
                                            .putString(PREF_THEME_MODE, selectedThemeMode)
                                            .putString(PREF_SELECTED_FLAVOR, selectedFlavor.storageValue)
                                            .putString(PREF_SELECTED_FLAVOR_OPTION, selectedFlavorOption)
                                            .apply()
                                        step = OnboardingStep.PERSONALITY
                                    },
                                contentAlignment = Alignment.Center
                            ) {
                                Text(
                                    text = "Continue",
                                    style = CitrosTypography.titleMedium,
                                    fontWeight = FontWeight.SemiBold,
                                    color = contrastOn(onboardingAccentColor)
                                )
                            }
                            Text(
                                text = "Back",
                                style = CitrosTypography.labelLarge,
                                color = onboardingAccentColor.copy(alpha = 0.90f),
                                modifier = Modifier
                                    .testTag(TEST_TAG_ONBOARDING_BACK_FLAVOR)
                                    .clickable {
                                    onboardingBackTarget(OnboardingStep.FLAVOR)?.let { step = it }
                                }
                            )
                        }
                    }
                }
                OnboardingStep.PERSONALITY -> {
                    Box(
                        modifier = Modifier.fillMaxSize()
                    ) {
                        Column(
                            modifier = Modifier
                                .fillMaxSize()
                                .padding(start = 6.dp, end = 6.dp, top = 10.dp, bottom = 92.dp),
                            horizontalAlignment = Alignment.CenterHorizontally,
                            verticalArrangement = Arrangement.spacedBy(12.dp)
                        ) {
                            OnboardingProgressDots(
                                stepIndex = 3,
                                totalSteps = 9,
                                accentColor = onboardingAccentColor,
                                futureColor = onboardingFutureDotColor
                            )
                            CitrosIcon(
                                imageVector = CitrosIcons.ChatBubble,
                                contentDescription = null,
                                tint = onboardingAccentColor,
                                modifier = Modifier.size(20.dp)
                            )
                            Text(
                                text = "How Should I Talk?",
                                style = CitrosTypography.headlineMedium,
                                fontWeight = FontWeight.SemiBold,
                                color = directiveSurfaces.labelPrimary
                            )
                            Text(
                                text = "Choose how Citros communicates with you.",
                                style = CitrosTypography.bodyLarge,
                                color = directiveSurfaces.labelSecondary,
                                textAlign = TextAlign.Center,
                                modifier = Modifier.fillMaxWidth(0.90f)
                            )
                            OnboardingConversationStyleCard(
                                title = "Concise",
                                description = "Short and to the point. Fewer follow-ups.",
                                sample = "\"Done. Reminder set for 3 PM.\"",
                                selected = conversationStyle == "Concise",
                                accentColor = onboardingAccentColor,
                                isDarkTheme = isDarkTheme,
                                onClick = { conversationStyle = "Concise" }
                            )
                            OnboardingConversationStyleCard(
                                title = "Balanced",
                                description = "Clear explanations without over-talking.",
                                sample = "\"Done — I set a reminder for 3 PM. Want me to add an agenda too?\"",
                                selected = conversationStyle == "Balanced",
                                accentColor = onboardingAccentColor,
                                isDarkTheme = isDarkTheme,
                                onClick = { conversationStyle = "Balanced" }
                            )
                            OnboardingConversationStyleCard(
                                title = "Thorough",
                                description = "Detailed reasoning and proactive suggestions.",
                                sample = "\"I've set a reminder for 3 PM, 10 minutes before your meeting with Sarah. I noticed there's no agenda attached — should I draft one?\"",
                                selected = conversationStyle == "Thorough",
                                accentColor = onboardingAccentColor,
                                isDarkTheme = isDarkTheme,
                                onClick = { conversationStyle = "Thorough" }
                            )
                            Spacer(Modifier.height(4.dp))
                        }
                        Column(
                            modifier = Modifier
                                .align(Alignment.BottomCenter)
                                .fillMaxWidth()
                                .padding(horizontal = 20.dp, vertical = 10.dp),
                            horizontalAlignment = Alignment.CenterHorizontally,
                            verticalArrangement = Arrangement.spacedBy(8.dp)
                        ) {
                            Box(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .height(48.dp)
                                    .background(onboardingAccentColor, RoundedCornerShape(14.dp))
                                    .testTag(TEST_TAG_ONBOARDING_CONTINUE_PERSONALITY)
                                    .clickable {
                                        when (conversationStyle) {
                                            "Concise" -> {
                                                tone = "Concise"
                                                explanation = "Brief"
                                            }
                                            "Thorough" -> {
                                                tone = "Thorough"
                                                explanation = "Detailed"
                                            }
                                            else -> {
                                                tone = "Balanced"
                                                explanation = "Balanced"
                                            }
                                        }
                                        step = OnboardingStep.ACQUAINTED
                                    },
                                contentAlignment = Alignment.Center
                            ) {
                                Text(
                                    text = "Continue",
                                    style = CitrosTypography.titleMedium,
                                    fontWeight = FontWeight.SemiBold,
                                    color = contrastOn(onboardingAccentColor)
                                )
                            }
                            Text(
                                text = "Back",
                                style = CitrosTypography.labelLarge,
                                color = onboardingAccentColor.copy(alpha = 0.90f),
                                modifier = Modifier
                                    .testTag(TEST_TAG_ONBOARDING_BACK_PERSONALITY)
                                    .clickable {
                                    onboardingBackTarget(OnboardingStep.PERSONALITY)?.let { step = it }
                                }
                            )
                        }
                    }
                }
                OnboardingStep.ACQUAINTED -> {
                    val cardColor = if (isDarkTheme) Color(0xFF1C1C22) else Color(0xFFE0E1E8)
                    val chipColor = if (isDarkTheme) Color(0xFF24252C) else Color(0xFFE3E4EA)
                    val chipTextColor = if (isDarkTheme) Color(0xFFB8BAC5) else Color(0xFF7B7D87)
                    val fieldTextColor = if (isDarkTheme) Color(0xFFE9EAF2) else Color(0xFF4A4C57)
                    Box(modifier = Modifier.fillMaxSize()) {
                        Column(
                            modifier = Modifier
                                .fillMaxSize()
                                .padding(start = 6.dp, end = 6.dp, top = 10.dp, bottom = 122.dp),
                            horizontalAlignment = Alignment.CenterHorizontally,
                            verticalArrangement = Arrangement.spacedBy(12.dp)
                        ) {
                            OnboardingProgressDots(
                                stepIndex = 4,
                                totalSteps = 9,
                                accentColor = onboardingAccentColor,
                                futureColor = onboardingFutureDotColor
                            )
                            CitrosIcon(
                                imageVector = CitrosIcons.Person,
                                contentDescription = null,
                                tint = onboardingAccentColor,
                                modifier = Modifier.size(20.dp)
                            )
                            Text(
                                text = "Let's Get Acquainted",
                                style = CitrosTypography.headlineMedium,
                                fontWeight = FontWeight.SemiBold,
                                color = directiveSurfaces.labelPrimary
                            )
                            Text(
                                text = "Help Citros understand you so it can be more helpful.",
                                style = CitrosTypography.bodyLarge,
                                color = directiveSurfaces.labelSecondary,
                                textAlign = TextAlign.Center,
                                modifier = Modifier.fillMaxWidth(0.86f)
                            )
                            Column(
                                modifier = Modifier.fillMaxWidth(),
                                verticalArrangement = Arrangement.spacedBy(8.dp)
                            ) {
                                Text(
                                    text = "WHAT SHOULD I CALL YOU?",
                                    style = CitrosTypography.labelLarge,
                                    color = directiveSurfaces.labelSecondary.copy(alpha = 0.86f)
                                )
                                OutlinedTextField(
                                    value = acquaintedName,
                                    onValueChange = { acquaintedName = it.take(40) },
                                    placeholder = { Text("Your name", color = fieldTextColor.copy(alpha = 0.7f)) },
                                    singleLine = true,
                                    shape = RoundedCornerShape(12.dp),
                                    colors = OutlinedTextFieldDefaults.colors(
                                        focusedContainerColor = cardColor,
                                        unfocusedContainerColor = cardColor,
                                        disabledContainerColor = cardColor,
                                        focusedBorderColor = Color.Transparent,
                                        unfocusedBorderColor = Color.Transparent,
                                        disabledBorderColor = Color.Transparent,
                                        focusedTextColor = fieldTextColor,
                                        unfocusedTextColor = fieldTextColor,
                                        cursorColor = onboardingAccentColor
                                    ),
                                    modifier = Modifier.fillMaxWidth()
                                )
                            }
                            Column(
                                modifier = Modifier.fillMaxWidth(),
                                verticalArrangement = Arrangement.spacedBy(10.dp)
                            ) {
                                Text(
                                    text = "I'M INTERESTED IN",
                                    style = CitrosTypography.labelLarge,
                                    color = directiveSurfaces.labelSecondary.copy(alpha = 0.86f)
                                )
                                FlowRow(
                                    modifier = Modifier.fillMaxWidth(),
                                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                                    verticalArrangement = Arrangement.spacedBy(8.dp)
                                ) {
                                    acquaintedInterestOptions.forEach { interest ->
                                        val selected = interest in acquaintedInterests
                                        Box(
                                            modifier = Modifier
                                                .background(
                                                    if (selected) onboardingAccentColor.copy(alpha = if (isDarkTheme) 0.26f else 0.20f) else chipColor,
                                                    RoundedCornerShape(999.dp)
                                                )
                                                .border(
                                                    width = 1.dp,
                                                    color = if (selected) onboardingAccentColor else Color.Transparent,
                                                    shape = RoundedCornerShape(999.dp)
                                                )
                                                .clickable {
                                                    acquaintedInterests = if (selected) {
                                                        acquaintedInterests - interest
                                                    } else {
                                                        (acquaintedInterests + interest).take(10)
                                                    }
                                                }
                                                .padding(horizontal = 14.dp, vertical = 9.dp)
                                        ) {
                                            Text(
                                                text = interest,
                                                style = CitrosTypography.bodyLarge,
                                                color = if (selected) {
                                                    if (isDarkTheme) Color.White else Color.Black
                                                } else {
                                                    chipTextColor
                                                }
                                            )
                                        }
                                    }
                                }
                            }
                            Spacer(Modifier.weight(1f))
                        }
                        Column(
                            modifier = Modifier
                                .align(Alignment.BottomCenter)
                                .fillMaxWidth()
                                .padding(horizontal = 20.dp, vertical = 12.dp),
                            horizontalAlignment = Alignment.CenterHorizontally,
                            verticalArrangement = Arrangement.spacedBy(8.dp)
                        ) {
                            Box(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .height(50.dp)
                                    .background(onboardingAccentColor, RoundedCornerShape(14.dp))
                                    .testTag(TEST_TAG_ONBOARDING_CONTINUE_ACQUAINTED)
                                    .clickable {
                                        val nameValue = acquaintedName.trim()
                                        prefs.edit()
                                            .putString(PREF_USER_NAME, if (nameValue.isBlank()) "You" else nameValue)
                                            .putString(PREF_USER_ADDRESS, if (nameValue.isBlank()) "You" else nameValue)
                                            .putString(
                                                PREF_USER_CONTEXT,
                                                acquaintedInterests.joinToString(", ").ifBlank { "general" }
                                            )
                                            .apply()
                                        step = OnboardingStep.PERMISSIONS
                                    },
                                contentAlignment = Alignment.Center
                            ) {
                                Text(
                                    text = "Continue",
                                    style = CitrosTypography.titleMedium,
                                    fontWeight = FontWeight.SemiBold,
                                    color = contrastOn(onboardingAccentColor)
                                )
                            }
                            Text(
                                text = "Back",
                                style = CitrosTypography.labelLarge,
                                color = onboardingAccentColor.copy(alpha = 0.90f),
                                modifier = Modifier
                                    .testTag(TEST_TAG_ONBOARDING_BACK_ACQUAINTED)
                                    .clickable {
                                    onboardingBackTarget(OnboardingStep.ACQUAINTED)?.let { step = it }
                                }
                            )
                        }
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
                            accent = onboardingAccentColor
                        ),
                        CitrosPlanSpec(
                            id = "base",
                            title = "Citros Base - $9/mo",
                            subtitle = "All models included with a monthly usage cap",
                            details = "$5 cap. Great for getting started.",
                            cta = "Join Waitlist",
                            accent = onboardingAccentColor,
                            recommended = true,
                            comingSoon = true
                        ),
                        CitrosPlanSpec(
                            id = "super",
                            title = "Citros Super - $29/mo",
                            subtitle = "Full catalog with higher monthly usage cap",
                            details = "$50 cap for power users.",
                            cta = "Join Waitlist",
                            accent = onboardingAccentColor,
                            comingSoon = true
                        )
                    )
                    Box(modifier = Modifier.fillMaxSize()) {
                        Column(
                            modifier = Modifier
                                .fillMaxSize()
                                .padding(top = 10.dp, bottom = 72.dp),
                            horizontalAlignment = Alignment.CenterHorizontally,
                            verticalArrangement = Arrangement.spacedBy(14.dp)
                        ) {
                            OnboardingProgressDots(
                                stepIndex = 7,
                                totalSteps = 9,
                                accentColor = onboardingAccentColor,
                                futureColor = onboardingFutureDotColor
                            )
                            CitrosIcon(
                                imageVector = CitrosIcons.Star,
                                contentDescription = null,
                                tint = onboardingAccentColor,
                                modifier = Modifier.size(20.dp)
                            )
                            Text(
                                text = "Choose Your Plan",
                                style = CitrosTypography.headlineMedium,
                                fontWeight = FontWeight.SemiBold,
                                color = directiveSurfaces.labelPrimary,
                                textAlign = TextAlign.Center,
                                modifier = Modifier.fillMaxWidth()
                            )
                            Text(
                                text = "Base and Super are coming soon. You can continue now with your own API key.",
                                style = CitrosTypography.bodyMedium,
                                color = onboardingAccentColor.copy(alpha = 0.90f),
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
                        Column(
                            modifier = Modifier
                                .align(Alignment.BottomCenter)
                                .fillMaxWidth()
                                .padding(start = 12.dp, top = 8.dp, end = 12.dp, bottom = 8.dp),
                            horizontalAlignment = Alignment.CenterHorizontally,
                            verticalArrangement = Arrangement.spacedBy(0.dp)
                        ) {
                            Text(
                                text = "Back",
                                style = CitrosTypography.labelLarge,
                                color = onboardingAccentColor.copy(alpha = 0.90f),
                                modifier = Modifier
                                    .testTag(TEST_TAG_ONBOARDING_BACK_PAYWALL)
                                    .clickable {
                                    onboardingBackTarget(OnboardingStep.PAYWALL)?.let { step = it }
                                }
                            )
                        }
                    }
                }
                OnboardingStep.API_KEY -> {
                    var apiKeyText by rememberSaveable { mutableStateOf("") }
                    var apiKeyVisible by rememberSaveable { mutableStateOf(false) }
                    var apiKeyLabel by rememberSaveable { mutableStateOf("") }
                    var selectedApiProvider by rememberSaveable { mutableStateOf(Provider.ANTHROPIC) }
                    var connectionStatus by rememberSaveable { mutableStateOf<String?>(null) }
                    var isValidating by rememberSaveable { mutableStateOf(false) }
                    val validationScope = rememberCoroutineScope()
                    val cardColor = if (isDarkTheme) Color(0xFF1C1C22) else Color(0xFFE0E1E8)
                    val fieldTextColor = if (isDarkTheme) Color(0xFFE9EAF2) else Color(0xFF4A4C57)
                    val disabledButtonColor = if (isDarkTheme) Color(0xFF2F3038) else Color(0xFFD7D8E0)
                    val disabledButtonTextColor = if (isDarkTheme) Color(0xFF767884) else Color(0xFFA3A5B0)
                    val canValidate = apiKeyText.trim().isNotBlank() && !isValidating
                    val canStart = apiKeyText.trim().isNotBlank() && !isValidating
                    val providerUrl = providerKeyUrl(selectedApiProvider).removePrefix("https://")
                    Box(modifier = Modifier.fillMaxSize()) {
                        Column(
                            modifier = Modifier
                                .fillMaxSize()
                                .padding(top = 10.dp, bottom = 192.dp),
                            verticalArrangement = Arrangement.spacedBy(12.dp)
                        ) {
                            OnboardingProgressDots(
                                stepIndex = 8,
                                totalSteps = 9,
                                accentColor = onboardingAccentColor,
                                futureColor = onboardingFutureDotColor,
                                modifier = Modifier.align(Alignment.CenterHorizontally)
                            )
                            Text(
                                text = "Connect a Provider",
                                style = CitrosTypography.headlineMedium,
                                fontWeight = FontWeight.SemiBold,
                                color = directiveSurfaces.labelPrimary,
                                textAlign = TextAlign.Center,
                                modifier = Modifier.fillMaxWidth()
                            )
                            Text(
                                text = "Paste your API key to get started.",
                                style = CitrosTypography.bodyLarge,
                                color = directiveSurfaces.labelSecondary,
                                modifier = Modifier.fillMaxWidth()
                            )
                            Box(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .background(cardColor, RoundedCornerShape(12.dp))
                                    .padding(4.dp)
                            ) {
                                Row(
                                    modifier = Modifier.fillMaxWidth(),
                                    horizontalArrangement = Arrangement.spacedBy(6.dp)
                                ) {
                                    Provider.entries.forEach { provider ->
                                        val selected = selectedApiProvider == provider
                                        Box(
                                            modifier = Modifier
                                                .weight(1f)
                                                .background(
                                                    if (selected) {
                                                        if (isDarkTheme) Color(0xFF34353D) else Color(0xFFD9DAE2)
                                                    } else {
                                                        Color.Transparent
                                                    },
                                                    RoundedCornerShape(10.dp)
                                                )
                                                .clickable {
                                                    selectedApiProvider = provider
                                                    connectionStatus = null
                                                }
                                                .padding(horizontal = 8.dp, vertical = 10.dp),
                                            contentAlignment = Alignment.Center
                                        ) {
                                            Text(
                                                text = ProviderUi.displayName(provider),
                                                style = CitrosTypography.bodyMedium,
                                                color = if (selected) {
                                                    directiveSurfaces.labelPrimary
                                                } else {
                                                    directiveSurfaces.labelSecondary.copy(alpha = 0.75f)
                                                }
                                            )
                                        }
                                    }
                                }
                            }
                            Text(
                                text = "Get a key from $providerUrl",
                                style = CitrosTypography.bodyMedium,
                                color = onboardingAccentColor.copy(alpha = 0.92f),
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
                            Text(
                                text = "API KEY",
                                style = CitrosTypography.labelLarge,
                                color = directiveSurfaces.labelSecondary.copy(alpha = 0.86f)
                            )
                            OutlinedTextField(
                                value = apiKeyText,
                                onValueChange = {
                                    apiKeyText = it
                                    connectionStatus = null
                                },
                                placeholder = {
                                    Text(ProviderUi.keyPlaceholder(selectedApiProvider), color = fieldTextColor.copy(alpha = 0.7f))
                                },
                                visualTransformation = if (apiKeyVisible) {
                                    VisualTransformation.None
                                } else {
                                    PasswordVisualTransformation()
                                },
                                trailingIcon = {
                                    CitrosIconButton(onClick = { apiKeyVisible = !apiKeyVisible }) {
                                        CitrosIcon(
                                            imageVector = if (apiKeyVisible) CitrosIcons.VisibilityOff else CitrosIcons.Visibility,
                                            contentDescription = if (apiKeyVisible) "Hide key" else "Show key",
                                            tint = directiveSurfaces.labelSecondary.copy(alpha = 0.7f)
                                        )
                                    }
                                },
                                singleLine = true,
                                shape = RoundedCornerShape(12.dp),
                                colors = OutlinedTextFieldDefaults.colors(
                                    focusedContainerColor = cardColor,
                                    unfocusedContainerColor = cardColor,
                                    disabledContainerColor = cardColor,
                                    focusedBorderColor = Color.Transparent,
                                    unfocusedBorderColor = Color.Transparent,
                                    disabledBorderColor = Color.Transparent,
                                    focusedTextColor = fieldTextColor,
                                    unfocusedTextColor = fieldTextColor,
                                    cursorColor = onboardingAccentColor
                                ),
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .testTag("api_key_field")
                            )
                            Text(
                                text = "LABEL (optional)",
                                style = CitrosTypography.labelLarge,
                                color = directiveSurfaces.labelSecondary.copy(alpha = 0.86f)
                            )
                            OutlinedTextField(
                                value = apiKeyLabel,
                                onValueChange = { apiKeyLabel = it },
                                placeholder = { Text("e.g. Personal key", color = fieldTextColor.copy(alpha = 0.7f)) },
                                singleLine = true,
                                shape = RoundedCornerShape(12.dp),
                                colors = OutlinedTextFieldDefaults.colors(
                                    focusedContainerColor = cardColor,
                                    unfocusedContainerColor = cardColor,
                                    disabledContainerColor = cardColor,
                                    focusedBorderColor = Color.Transparent,
                                    unfocusedBorderColor = Color.Transparent,
                                    disabledBorderColor = Color.Transparent,
                                    focusedTextColor = fieldTextColor,
                                    unfocusedTextColor = fieldTextColor,
                                    cursorColor = onboardingAccentColor
                                ),
                                modifier = Modifier.fillMaxWidth()
                            )
                            Text(
                                text = "We encrypt keys at rest with Android Keystore-backed AES-256-GCM " +
                                    "(EncryptedSharedPreferences + MasterKey) and transmit them only over HTTPS/TLS directly to your selected provider.",
                                style = CitrosTypography.bodySmall,
                                color = directiveSurfaces.labelSecondary.copy(alpha = 0.84f),
                                modifier = Modifier.fillMaxWidth()
                            )
                            connectionStatus?.let { status ->
                                val isSuccess = status.startsWith("SUCCESS:")
                                val isPending = status.startsWith("PENDING:")
                                val isWarning = status.startsWith("WARN:")
                                val statusText = status.substringAfter(": ", status)
                                val statusColor = when {
                                    isSuccess -> Color(0xFF22C55E)
                                    isPending -> onboardingAccentColor.copy(alpha = 0.90f)
                                    isWarning -> directiveSurfaces.labelSecondary.copy(alpha = 0.92f)
                                    else -> CitrosColorScheme.error
                                }
                                Row(
                                    modifier = Modifier.fillMaxWidth(),
                                    verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.spacedBy(8.dp)
                                ) {
                                    if (isSuccess) {
                                        Box(
                                            modifier = Modifier
                                                .size(20.dp)
                                                .background(Color(0xFF22C55E).copy(alpha = 0.18f), CircleShape),
                                            contentAlignment = Alignment.Center
                                        ) {
                                            CitrosIcon(
                                                imageVector = CitrosIcons.SearchBarCheck,
                                                contentDescription = null,
                                                tint = Color(0xFF22C55E),
                                                modifier = Modifier.size(10.dp)
                                            )
                                        }
                                    }
                                    Text(
                                        text = statusText,
                                        style = CitrosTypography.bodySmall,
                                        color = statusColor
                                    )
                                }
                            }
                        }
                        Column(
                            modifier = Modifier
                                .align(Alignment.BottomCenter)
                                .fillMaxWidth()
                                .padding(horizontal = 0.dp, vertical = 12.dp),
                            horizontalAlignment = Alignment.CenterHorizontally,
                            verticalArrangement = Arrangement.spacedBy(8.dp)
                        ) {
                            Box(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .height(50.dp)
                                    .background(
                                        if (canValidate) onboardingAccentColor else disabledButtonColor,
                                        RoundedCornerShape(14.dp)
                                    )
                                    .clickable(enabled = canValidate) {
                                        val trimmed = apiKeyText.trim()
                                        val formatError = when {
                                            trimmed.isEmpty() -> "ERROR: Please enter a key"
                                            trimmed.length < 20 -> "ERROR: Key too short (minimum 20 characters)"
                                            !trimmed.startsWith(providerRequiredPrefix(selectedApiProvider)) ->
                                                "ERROR: ${ProviderUi.displayName(selectedApiProvider)} keys must start with ${providerRequiredPrefix(selectedApiProvider)}"
                                            else -> null
                                        }
                                        if (formatError != null) {
                                            connectionStatus = formatError
                                        } else {
                                            isValidating = true
                                            connectionStatus = "PENDING: Testing connection..."
                                            validationScope.launch {
                                                val status = validateApiCredential(trimmed, selectedApiProvider)
                                                connectionStatus = when (status) {
                                                    ApiKeyValidationStatus.VALID -> "SUCCESS: Key is valid — connection successful!"
                                                    ApiKeyValidationStatus.INVALID -> "ERROR: Invalid key — check your key and try again"
                                                    ApiKeyValidationStatus.EXPIRED -> "ERROR: Key has expired — generate a new one"
                                                    ApiKeyValidationStatus.UNKNOWN -> "WARN: Could not verify — check your internet connection"
                                                }
                                                isValidating = false
                                            }
                                        }
                                    },
                                contentAlignment = Alignment.Center
                            ) {
                                Text(
                                    text = if (isValidating) "Validating..." else "Validate Key",
                                    style = CitrosTypography.titleMedium,
                                    fontWeight = FontWeight.SemiBold,
                                    color = if (canValidate) {
                                        contrastOn(onboardingAccentColor)
                                    } else {
                                        disabledButtonTextColor
                                    }
                                )
                            }
                            Box(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .height(50.dp)
                                    .background(
                                        if (canStart) onboardingAccentColor else disabledButtonColor,
                                        RoundedCornerShape(14.dp)
                                    )
                                    .testTag("api_key_start_btn")
                                    .clickable(enabled = canStart) {
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
                                        step = OnboardingStep.READY
                                    },
                                contentAlignment = Alignment.Center
                            ) {
                                Text(
                                    text = "Start Chatting",
                                    style = CitrosTypography.titleMedium,
                                    fontWeight = FontWeight.SemiBold,
                                    color = if (canStart) {
                                        contrastOn(onboardingAccentColor)
                                    } else {
                                        disabledButtonTextColor
                                    }
                                )
                            }
                            Text(
                                text = "Back",
                                style = CitrosTypography.labelLarge,
                                color = onboardingAccentColor.copy(alpha = 0.90f),
                                modifier = Modifier
                                    .testTag(TEST_TAG_ONBOARDING_BACK_API_KEY)
                                    .clickable {
                                    onboardingBackTarget(OnboardingStep.API_KEY)?.let { step = it }
                                }
                            )
                        }
                    }
                }
                OnboardingStep.PERMISSIONS -> {
                    val cardColor = if (isDarkTheme) Color(0xFF1C1C22) else Color(0xFFE0E1E8)
                    val grantButtonColor = if (isDarkTheme) Color(0xFF3A3B44) else Color(0xFFD0D1D9)
                    val grantedColor = Color(0xFF22C55E)
                    val lifecycleOwner = LocalLifecycleOwner.current
                    var accessibilityGranted by remember {
                        mutableStateOf(CitrosAccessibilityService.isEnabled(context))
                    }
                    var overlayGranted by remember {
                        mutableStateOf(OverlayPermission.canDrawOverlays(context))
                    }
                    var notificationGranted by remember {
                        mutableStateOf(CitrosNotificationListener.isEnabled(context))
                    }
                    fun refreshPermissionFlags() {
                        accessibilityGranted = CitrosAccessibilityService.isEnabled(context)
                        overlayGranted = OverlayPermission.canDrawOverlays(context)
                        notificationGranted = CitrosNotificationListener.isEnabled(context)
                    }
                    LaunchedEffect(Unit) {
                        refreshPermissionFlags()
                    }
                    DisposableEffect(lifecycleOwner) {
                        val observer = LifecycleEventObserver { _, event ->
                            if (event == Lifecycle.Event.ON_RESUME) {
                                refreshPermissionFlags()
                            }
                        }
                        lifecycleOwner.lifecycle.addObserver(observer)
                        onDispose {
                            lifecycleOwner.lifecycle.removeObserver(observer)
                        }
                    }
                    Box(modifier = Modifier.fillMaxSize()) {
                        Column(
                            modifier = Modifier
                                .fillMaxSize()
                                .padding(top = 10.dp, bottom = 122.dp),
                            horizontalAlignment = Alignment.CenterHorizontally,
                            verticalArrangement = Arrangement.spacedBy(10.dp)
                        ) {
                            OnboardingProgressDots(
                                stepIndex = 5,
                                totalSteps = 9,
                                accentColor = onboardingAccentColor,
                                futureColor = onboardingFutureDotColor
                            )
                            CitrosIcon(
                                imageVector = CitrosIcons.Shield,
                                contentDescription = null,
                                tint = onboardingAccentColor,
                                modifier = Modifier.size(20.dp)
                            )
                            Text(
                                text = "Permissions",
                                style = CitrosTypography.headlineMedium,
                                fontWeight = FontWeight.SemiBold,
                                color = directiveSurfaces.labelPrimary
                            )
                            Text(
                                text = "Citros needs these to act on your behalf.",
                                style = CitrosTypography.bodyLarge,
                                color = directiveSurfaces.labelSecondary,
                                textAlign = TextAlign.Center,
                                modifier = Modifier.fillMaxWidth(0.88f)
                            )
                            Spacer(Modifier.height(8.dp))
                            OnboardingPermissionCard(
                                title = "Accessibility Service",
                                subtitle = "Read and interact with screen content",
                                granted = accessibilityGranted,
                                isDarkTheme = isDarkTheme,
                                cardColor = cardColor,
                                grantButtonColor = grantButtonColor,
                                grantedColor = grantedColor,
                                onGrant = {
                                    runCatching {
                                        context.startActivity(
                                            android.content.Intent(android.provider.Settings.ACTION_ACCESSIBILITY_SETTINGS)
                                        )
                                    }.onFailure { error ->
                                        Log.w("OnboardingFlow", "Intent launch failed", error)
                                    }
                                }
                            )
                            OnboardingPermissionCard(
                                title = "Overlay Permission",
                                subtitle = "Show Citros over other apps",
                                granted = overlayGranted,
                                isDarkTheme = isDarkTheme,
                                cardColor = cardColor,
                                grantButtonColor = grantButtonColor,
                                grantedColor = grantedColor,
                                onGrant = {
                                    runCatching {
                                        context.startActivity(OverlayPermission.buildPermissionIntent(context))
                                    }.onFailure { error ->
                                        Log.w("OnboardingFlow", "Intent launch failed", error)
                                    }
                                }
                            )
                            OnboardingPermissionCard(
                                title = "Notification Access",
                                subtitle = "Read and manage notifications",
                                granted = notificationGranted,
                                isDarkTheme = isDarkTheme,
                                cardColor = cardColor,
                                grantButtonColor = grantButtonColor,
                                grantedColor = grantedColor,
                                onGrant = {
                                    runCatching {
                                        CitrosNotificationListener.openSettings(context)
                                    }.onFailure { error ->
                                        Log.w("OnboardingFlow", "Intent launch failed", error)
                                    }
                                }
                            )
                            Spacer(Modifier.weight(1f))
                        }
                        Column(
                            modifier = Modifier
                                .align(Alignment.BottomCenter)
                                .fillMaxWidth()
                                .padding(horizontal = 20.dp, vertical = 12.dp),
                            horizontalAlignment = Alignment.CenterHorizontally,
                            verticalArrangement = Arrangement.spacedBy(8.dp)
                        ) {
                            Box(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .height(50.dp)
                                    .background(onboardingAccentColor, RoundedCornerShape(14.dp))
                                    .clickable { step = OnboardingStep.TRUST }
                                    .testTag("permissions_continue_btn"),
                                contentAlignment = Alignment.Center
                            ) {
                                Text(
                                    text = "Continue",
                                    style = CitrosTypography.titleMedium,
                                    fontWeight = FontWeight.SemiBold,
                                    color = contrastOn(onboardingAccentColor)
                                )
                            }
                            Text(
                                text = "Back",
                                style = CitrosTypography.labelLarge,
                                color = onboardingAccentColor.copy(alpha = 0.90f),
                                modifier = Modifier
                                    .testTag(TEST_TAG_ONBOARDING_BACK_PERMISSIONS)
                                    .clickable {
                                    onboardingBackTarget(OnboardingStep.PERMISSIONS)?.let { step = it }
                                }
                            )
                        }
                    }
                }
                OnboardingStep.TRUST -> {
                    val trustCards = listOf(
                        "Cautious" to "Ask before every action",
                        "Balanced" to "Ask for sensitive actions only",
                        "Autonomous" to "Act independently"
                    )
                    val cardColor = if (isDarkTheme) Color(0xFF1C1C22) else Color(0xFFE0E1E8)
                    Box(modifier = Modifier.fillMaxSize()) {
                        Column(
                            modifier = Modifier
                                .fillMaxSize()
                                .padding(top = 10.dp, bottom = 122.dp),
                            horizontalAlignment = Alignment.CenterHorizontally,
                            verticalArrangement = Arrangement.spacedBy(10.dp)
                        ) {
                            OnboardingProgressDots(
                                stepIndex = 6,
                                totalSteps = 9,
                                accentColor = onboardingAccentColor,
                                futureColor = onboardingFutureDotColor
                            )
                            CitrosIcon(
                                imageVector = CitrosIcons.Phone,
                                contentDescription = null,
                                tint = onboardingAccentColor,
                                modifier = Modifier.size(20.dp)
                            )
                            Text(
                                text = "Choose Trust Level",
                                style = CitrosTypography.headlineMedium,
                                fontWeight = FontWeight.SemiBold,
                                color = directiveSurfaces.labelPrimary
                            )
                            Text(
                                text = "How much should Citros ask before acting?",
                                style = CitrosTypography.bodyLarge,
                                color = directiveSurfaces.labelSecondary,
                                textAlign = TextAlign.Center,
                                modifier = Modifier.fillMaxWidth(0.88f)
                            )
                            Spacer(Modifier.height(8.dp))
                            trustCards.forEach { (title, subtitle) ->
                                val selected = trustLevel == title
                                Box(
                                    modifier = Modifier
                                        .fillMaxWidth()
                                        .background(cardColor, RoundedCornerShape(14.dp))
                                        .border(
                                            width = if (selected) 1.5.dp else 1.dp,
                                            color = if (selected) onboardingAccentColor else Color.Transparent,
                                            shape = RoundedCornerShape(14.dp)
                                        )
                                        .clickable { trustLevel = title }
                                        .padding(horizontal = 16.dp, vertical = 14.dp)
                                ) {
                                    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                                        Text(
                                            text = title,
                                            style = CitrosTypography.headlineSmall,
                                            fontWeight = FontWeight.SemiBold,
                                            color = directiveSurfaces.labelPrimary
                                        )
                                        Text(
                                            text = subtitle,
                                            style = CitrosTypography.bodyLarge,
                                            color = directiveSurfaces.labelSecondary
                                        )
                                    }
                                }
                            }
                            Spacer(Modifier.weight(1f))
                        }
                        Column(
                            modifier = Modifier
                                .align(Alignment.BottomCenter)
                                .fillMaxWidth()
                                .padding(horizontal = 20.dp, vertical = 12.dp),
                            horizontalAlignment = Alignment.CenterHorizontally,
                            verticalArrangement = Arrangement.spacedBy(8.dp)
                        ) {
                            Box(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .height(50.dp)
                                    .background(onboardingAccentColor, RoundedCornerShape(14.dp))
                                    .testTag(TEST_TAG_ONBOARDING_CONTINUE_TRUST)
                                    .clickable {
                                        trust = when (trustLevel) {
                                            "Cautious" -> "Ask before every action"
                                            "Autonomous" -> "Act independently"
                                            else -> "Ask for sensitive actions only"
                                        }
                                        step = OnboardingStep.PAYWALL
                                    },
                                contentAlignment = Alignment.Center
                            ) {
                                Text(
                                    text = "Continue",
                                    style = CitrosTypography.titleMedium,
                                    fontWeight = FontWeight.SemiBold,
                                    color = contrastOn(onboardingAccentColor)
                                )
                            }
                            Text(
                                text = "Back",
                                style = CitrosTypography.labelLarge,
                                color = onboardingAccentColor.copy(alpha = 0.90f),
                                modifier = Modifier
                                    .testTag(TEST_TAG_ONBOARDING_BACK_TRUST)
                                    .clickable {
                                    onboardingBackTarget(OnboardingStep.TRUST)?.let { step = it }
                                }
                            )
                        }
                    }
                }
                OnboardingStep.READY -> {
                    val noFlavorSelected = selectedFlavorOption == "none"
                    Box(
                        modifier = Modifier
                            .fillMaxWidth()
                            .height(welcomeFullHeight)
                            .background(directiveSurfaces.background)
                            .citrosFlavorWash(
                                washColor = if (noFlavorSelected) null else directiveFlavorTokens.washColor,
                                centerXFraction = 0.5f,
                                centerYFraction = 0.46f,
                                radiusFraction = 0.82f
                            ),
                        contentAlignment = Alignment.Center
                    ) {
                        Column(
                            modifier = Modifier
                                .fillMaxSize()
                                .padding(top = 10.dp, bottom = 122.dp),
                            horizontalAlignment = Alignment.CenterHorizontally
                        ) {
                            OnboardingProgressDots(
                                stepIndex = 9,
                                totalSteps = 9,
                                accentColor = onboardingAccentColor,
                                futureColor = onboardingFutureDotColor
                            )
                            Spacer(Modifier.weight(1f))
                            CitrosDirectiveOrb(
                                flavor = selectedFlavor,
                                size = 64.dp,
                                modifier = Modifier
                                    .testTag("ready_start_btn")
                                    .clickable {
                                        val tier = prefs.getString(PREF_SELECTED_TIER, "byo") ?: "byo"
                                        completeOnboarding(selectedTier = tier, startTrial = true)
                                    }
                            )
                            Spacer(Modifier.height(18.dp))
                            Text(
                                text = "You're all set",
                                style = CitrosTypography.headlineMedium,
                                fontWeight = FontWeight.SemiBold,
                                color = directiveSurfaces.labelPrimary
                            )
                            Spacer(Modifier.height(10.dp))
                            Text(
                                text = "Citros is ready. Say something or tap the orb to get started.",
                                style = CitrosTypography.bodyLarge,
                                color = directiveSurfaces.labelSecondary,
                                textAlign = TextAlign.Center,
                                modifier = Modifier.fillMaxWidth(0.82f)
                            )
                            Spacer(Modifier.weight(1f))
                        }
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
            scrimColor = onboardingAccentColor.copy(alpha = 0.22f),
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
                    baseColor = if (isDarkTheme) {
                        Color(0xE6070709)
                    } else {
                        CitrosColorScheme.surface.copy(alpha = 0.92f)
                    },
                    borderColor = onboardingAccentColor.copy(alpha = 0.44f),
                    borderWidth = 1.dp,
                    highlightColor = onboardingAccentColor,
                    warmth = 0.88f,
                    contentPadding = PaddingValues(horizontal = 18.dp, vertical = 18.dp)
                ) {
                    Column(
                        modifier = Modifier.fillMaxWidth(),
                        verticalArrangement = Arrangement.spacedBy(12.dp)
                    ) {
                        Text(
                            "Coming Soon",
                            style = CitrosTypography.titleLarge,
                            fontWeight = FontWeight.SemiBold,
                            color = onboardingAccentColor.copy(alpha = 0.96f)
                        )
                        Text(
                            "$waitlistTierLabel is almost ready. Leave your email and we will notify you at launch.",
                            style = CitrosTypography.bodyMedium,
                            color = onboardingAccentColor.copy(alpha = 0.84f)
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
                                focusedBorderColor = onboardingAccentColor,
                                unfocusedBorderColor = onboardingAccentColor.copy(alpha = 0.42f),
                                focusedLabelColor = onboardingAccentColor,
                                unfocusedLabelColor = onboardingAccentColor.copy(alpha = 0.74f),
                                cursorColor = onboardingAccentColor
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
                            tintColor = onboardingAccentColor
                        )
                        Text(
                            text = "Continue With BYO For Now",
                            style = CitrosTypography.labelLarge.copy(
                                textDecoration = TextDecoration.Underline
                            ),
                            color = onboardingAccentColor.copy(alpha = 0.9f),
                            textAlign = TextAlign.Center,
                            modifier = Modifier
                                .fillMaxWidth()
                                .clickable {
                                    recordWaitlistSelection()
                                    showWaitlistSheet = false
                                    prefs.edit()
                                        .putString(PREF_SELECTED_TIER, "byo")
                                        .putBoolean(PREF_PAYWALL_SEEN, true)
                                        .apply()
                                    step = OnboardingStep.API_KEY
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
    val isDarkTheme = LocalCitrosIsDark.current
    val surfaces = remember(isDarkTheme) { citrosDirectiveSurfaces(isDarkTheme) }
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .statusBarsPadding()
            .padding(horizontal = 16.dp, vertical = 12.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp)
    ) {
        CitrosStepHeader(
            title = null,
            stepIndex = 4,
            totalSteps = 7,
            onBack = onBack,
            backLabelColor = flavor.primary.copy(alpha = 0.88f),
            activeProgressColor = flavor.primary,
            inactiveProgressColor = flavor.primary.copy(alpha = 0.24f),
            centerTitle = true,
            showStepCounter = false,
            progressStyle = CitrosStepProgressStyle.DOTS
        )
        Text(
            text = "Getting to know eachother",
            style = CitrosTypography.headlineSmall,
            fontWeight = FontWeight.SemiBold,
            color = surfaces.labelPrimary
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
                    style = CitrosTypography.labelLarge,
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
            style = CitrosTypography.labelSmall,
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
                            style = CitrosTypography.labelSmall,
                            modifier = Modifier.semantics { contentDescription = "${chip.label} icon" }
                        )
                        Text(
                            text = "${chip.label}:",
                            style = CitrosTypography.labelSmall,
                            color = flavor.primary.copy(alpha = 0.80f)
                        )
                        Text(
                            text = chip.value,
                            style = CitrosTypography.labelSmall,
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
                style = CitrosTypography.labelLarge,
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
            .background(CitrosColorScheme.background)
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
                    style = CitrosTypography.bodySmall,
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
                        CitrosIconButton(
                            onClick = { sendUserMessage() },
                            enabled = draftMessage.isNotBlank() && !isAssistantTyping
                        ) {
                            CitrosIcon(
                                CitrosIcons.Send,
                                contentDescription = "Send",
                                tint = if (draftMessage.isNotBlank()) {
                                    flavor.primary
                                } else {
                                    CitrosColorScheme.onSurface.copy(alpha = 0.35f)
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
                    style = CitrosTypography.bodyMedium,
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
                        style = CitrosTypography.bodyMedium,
                        color = CitrosColorScheme.onSurface.copy(alpha = 0.92f)
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
private fun OnboardingPermissionCard(
    title: String,
    subtitle: String,
    granted: Boolean,
    isDarkTheme: Boolean,
    cardColor: Color,
    grantButtonColor: Color,
    grantedColor: Color,
    onGrant: () -> Unit
) {
    val titleColor = if (isDarkTheme) Color.White else Color(0xFF101114)
    val subtitleColor = if (isDarkTheme) Color(0xFFB3B5C1) else CitrosColorScheme.onSurfaceVariant
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .background(cardColor, RoundedCornerShape(14.dp))
            .padding(horizontal = 14.dp, vertical = 12.dp)
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(10.dp)
        ) {
            Column(
                modifier = Modifier.weight(1f),
                verticalArrangement = Arrangement.spacedBy(2.dp)
            ) {
                Text(
                    text = title,
                    style = CitrosTypography.headlineSmall,
                    fontWeight = FontWeight.SemiBold,
                    color = titleColor
                )
                Text(
                    text = subtitle,
                    style = CitrosTypography.bodyMedium,
                    color = subtitleColor
                )
            }
            if (granted) {
                Box(
                    modifier = Modifier
                        .background(
                            color = grantedColor.copy(alpha = if (isDarkTheme) 0.24f else 0.20f),
                            shape = RoundedCornerShape(10.dp)
                        )
                        .padding(horizontal = 10.dp, vertical = 8.dp),
                    contentAlignment = Alignment.Center
                ) {
                    Text(
                        text = "Granted",
                        style = CitrosTypography.titleSmall,
                        color = grantedColor,
                        fontWeight = FontWeight.SemiBold
                    )
                }
            } else {
                Box(
                    modifier = Modifier
                        .background(grantButtonColor, RoundedCornerShape(10.dp))
                        .clickable(onClick = onGrant)
                        .padding(horizontal = 14.dp, vertical = 8.dp),
                    contentAlignment = Alignment.Center
                ) {
                    Text(
                        text = "Grant",
                        style = CitrosTypography.titleSmall,
                        fontWeight = FontWeight.SemiBold
                    )
                }
            }
        }
    }
}
@Composable
private fun OnboardingProgressDots(
    stepIndex: Int,
    totalSteps: Int,
    accentColor: Color,
    futureColor: Color,
    modifier: Modifier = Modifier
) {
    Row(
        modifier = modifier,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        repeat(totalSteps) { index ->
            val dotIndex = index + 1
            val isCurrent = dotIndex == stepIndex
            val color = when {
                dotIndex < stepIndex -> accentColor.copy(alpha = 0.45f)
                isCurrent -> accentColor
                else -> futureColor
            }
            Box(
                modifier = Modifier
                    .size(width = if (isCurrent) 16.dp else 6.dp, height = 6.dp)
                    .background(color = color, shape = CircleShape)
            )
        }
    }
}
@Composable
private fun OnboardingFlavorOption(
    label: String,
    selected: Boolean,
    flavor: CitrosFlavor,
    isDarkTheme: Boolean,
    onClick: () -> Unit
) {
    val selectedBorderColor = if (isDarkTheme) Color.White else Color.Black
    val unselectedBorderColor = if (isDarkTheme) Color.Transparent else Color.Transparent
    val labelColor = if (selected) {
        if (isDarkTheme) Color.White else Color.Black
    } else {
        if (isDarkTheme) Color(0x80EBEBF5) else Color(0x803C3C43)
    }
    Column(
        modifier = Modifier
            .widthIn(min = 54.dp)
            .clickable(onClick = onClick),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(7.dp)
    ) {
        Box(
            modifier = Modifier
                .size(48.dp)
                .border(
                    width = if (selected) 3.dp else 1.dp,
                    color = if (selected) selectedBorderColor else unselectedBorderColor,
                    shape = CircleShape
                ),
            contentAlignment = Alignment.Center
        ) {
            CitrosDirectiveOrb(
                flavor = flavor,
                size = 40.dp
            )
        }
        Text(
            text = label,
            style = CitrosTypography.bodySmall,
            color = labelColor
        )
    }
}
@Composable
private fun OnboardingThemeOptionCard(
    label: String,
    selected: Boolean,
    accentColor: Color,
    isDarkPreview: Boolean,
    isDarkTheme: Boolean,
    onClick: () -> Unit
) {
    val previewBackground = if (isDarkPreview) Color(0xFF1D1E23) else Color(0xFFE9E9EC)
    val previewLineColor = if (isDarkPreview) Color(0xFF2E3038) else Color(0xFFD3D3D8)
    val borderColor = when {
        selected -> accentColor
        isDarkTheme -> Color.White.copy(alpha = 0.38f)
        else -> Color.Black.copy(alpha = 0.18f)
    }
    val labelColor = when {
        selected -> if (isDarkTheme) Color.White else Color.Black
        else -> if (isDarkTheme) Color(0x80EBEBF5) else Color(0x803C3C43)
    }
    Column(
        modifier = Modifier
            .widthIn(min = 136.dp, max = 164.dp)
            .clickable(onClick = onClick),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(8.dp)
    ) {
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .height(78.dp)
                .background(previewBackground, RoundedCornerShape(12.dp))
                .border(if (selected) 2.dp else 1.dp, borderColor, RoundedCornerShape(12.dp))
                .padding(horizontal = 12.dp, vertical = 10.dp)
        ) {
            Box(
                modifier = Modifier
                    .align(Alignment.CenterStart)
                    .fillMaxWidth(0.72f)
                    .height(12.dp)
                    .background(previewLineColor, RoundedCornerShape(999.dp))
            )
            Box(
                modifier = Modifier
                    .align(Alignment.BottomEnd)
                    .fillMaxWidth(0.45f)
                    .height(12.dp)
                    .background(accentColor, RoundedCornerShape(999.dp))
            )
        }
        Text(
            text = label,
            style = CitrosTypography.titleMedium,
            color = labelColor
        )
    }
}
@Composable
private fun OnboardingConversationStyleCard(
    title: String,
    description: String,
    sample: String,
    selected: Boolean,
    accentColor: Color,
    isDarkTheme: Boolean,
    onClick: () -> Unit
) {
    val cardColor = if (isDarkTheme) Color(0xFF1C1C22) else Color(0xFFD9DAE1)
    val quoteColor = if (isDarkTheme) Color(0xFF2E2F36) else Color(0xFFCBCDD5)
    val titleColor = if (isDarkTheme) Color.White else Color.Black
    val secondaryColor = if (isDarkTheme) Color(0x99EBEBF5) else Color(0x993C3C43)
    val quoteTextColor = if (isDarkTheme) Color(0xB3EBEBF5) else Color(0xB33C3C43)
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .background(cardColor, RoundedCornerShape(14.dp))
            .border(
                width = if (selected) 1.5.dp else 1.dp,
                color = if (selected) accentColor else Color.Transparent,
                shape = RoundedCornerShape(14.dp)
            )
            .clickable(onClick = onClick)
            .padding(horizontal = 14.dp, vertical = 12.dp)
    ) {
        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(
                text = title,
                style = CitrosTypography.headlineSmall,
                fontWeight = FontWeight.SemiBold,
                color = titleColor
            )
            Text(
                text = description,
                style = CitrosTypography.bodyLarge,
                color = secondaryColor,
                maxLines = 2,
                overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis
            )
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .background(quoteColor, RoundedCornerShape(12.dp))
                    .padding(horizontal = 12.dp, vertical = 8.dp)
            ) {
                Text(
                    text = sample,
                    style = CitrosTypography.bodyLarge,
                    color = quoteTextColor,
                    maxLines = 3,
                    overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis
                )
            }
        }
    }
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
            style = CitrosTypography.titleSmall.copy(
                fontSize = CitrosTypography.titleSmall.fontSize * scale
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
