@file:OptIn(ExperimentalMaterial3Api::class)

package com.fawx.app.ui.preview

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.SmartToy
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.tooling.preview.Preview

// ─── Theme ───
import com.fawx.app.ui.theme.FawxTheme

// ─── Flavor (NOTE: consolidate to one canonical location in your project) ───
import com.fawx.app.ui.onboarding.FawxFlavor

// ─── Components ───
import com.fawx.app.ui.components.*

// ─── Onboarding ───
import com.fawx.app.ui.onboarding.*

// ─── Chat ───
import com.fawx.app.ui.chat.*

// ─── Overlay ───
import com.fawx.app.ui.overlay.*

// ─── Settings ───
import com.fawx.app.ui.settings.*


// ╔══════════════════════════════════════════════════════════════════╗
// ║  INTEGRATION NOTE                                               ║
// ║                                                                  ║
// ║  FawxFlavor is defined in 3 places right now:                  ║
// ║    - onboarding/FawxFlavor.kt                                 ║
// ║    - overlay/OverlayBubble.kt (local copy)                      ║
// ║    - settings imports from theme package                        ║
// ║                                                                  ║
// ║  Before compiling: move FawxFlavor to one canonical package   ║
// ║  (e.g., com.fawx.app.ui.theme.FawxFlavor) and update all   ║
// ║  imports. Same for the Message data class (components vs        ║
// ║  overlay have different definitions).                            ║
// ╚══════════════════════════════════════════════════════════════════╝


// ════════════════════════════════════════════════════════════════════
//  1. THEME / COMPONENTS
// ════════════════════════════════════════════════════════════════════

@Preview(
    name = "Hero Sphere",
    showBackground = true,
    backgroundColor = 0xFF050505,
    widthDp = 300,
    heightDp = 300
)
@Composable
private fun PreviewHeroSphere() {
    FawxTheme {
        FawxHeroSphere(
            flavor = FawxFlavor.TANGERINE,
            size = androidx.compose.ui.unit.Dp(200f)
        )
    }
}

@Preview(
    name = "Hero Badge",
    showBackground = true,
    backgroundColor = 0xFF050505,
    widthDp = 120,
    heightDp = 120
)
@Composable
private fun PreviewHeroBadge() {
    FawxTheme {
        CitrusHeroBadge(flavor = FawxFlavor.TANGERINE, size = 68)
    }
}

@Preview(
    name = "Primary Button",
    showBackground = true,
    backgroundColor = 0xFF050505,
    widthDp = 360,
    heightDp = 80
)
@Composable
private fun PreviewPrimaryButton() {
    FawxTheme {
        FawxPrimaryButton(
            text = "Get Started",
            onClick = {},
            flavor = FawxFlavor.TANGERINE,
            modifier = Modifier
        )
    }
}

@Preview(
    name = "Secondary Button",
    showBackground = true,
    backgroundColor = 0xFF050505,
    widthDp = 360,
    heightDp = 80
)
@Composable
private fun PreviewSecondaryButton() {
    FawxTheme {
        FawxSecondaryButton(
            text = "Skip for now",
            onClick = {},
            flavor = FawxFlavor.TANGERINE,
            modifier = Modifier
        )
    }
}

@Preview(
    name = "Step Header",
    showBackground = true,
    backgroundColor = 0xFF050505,
    widthDp = 393,
    heightDp = 80
)
@Composable
private fun PreviewStepHeader() {
    FawxTheme {
        FawxStepHeader(
            title = "Choose Your Flavor",
            stepIndex = 2,
            totalSteps = 7,
            onBack = {}
        )
    }
}

@Preview(
    name = "Personality Chips",
    showBackground = true,
    backgroundColor = 0xFF050505,
    widthDp = 393,
    heightDp = 60
)
@Composable
private fun PreviewPersonalityChips() {
    FawxTheme {
        androidx.compose.foundation.layout.Row(
            horizontalArrangement = androidx.compose.foundation.layout.Arrangement.spacedBy(
                androidx.compose.ui.unit.dp(8)
            ),
            modifier = Modifier.padding(androidx.compose.ui.unit.dp(16))
        ) {
            PersonalityOptionChip(text = "Casual", selected = true, onClick = {})
            PersonalityOptionChip(text = "Professional", selected = false, onClick = {})
            PersonalityOptionChip(text = "Playful", selected = false, onClick = {})
        }
    }
}

@Preview(
    name = "Flavor Card – Selected",
    showBackground = true,
    backgroundColor = 0xFF050505,
    widthDp = 393,
    heightDp = 100
)
@Composable
private fun PreviewFlavorCard() {
    FawxTheme {
        FlavorOptionCard(
            flavor = FawxFlavor.TANGERINE,
            selected = true,
            onClick = {},
            modifier = Modifier.padding(horizontal = androidx.compose.ui.unit.dp(16))
        )
    }
}

@Preview(
    name = "Plan Card – Recommended",
    showBackground = true,
    backgroundColor = 0xFF050505,
    widthDp = 393,
    heightDp = 320
)
@Composable
private fun PreviewPlanCard() {
    FawxTheme {
        PlanCard(
            plan = FawxPlanSpec(
                title = "Bring Your Own Key",
                subtitle = "Use your API key",
                details = listOf(
                    "Unlimited actions",
                    "All phone controls",
                    "Priority support"
                ),
                ctaText = "Continue with Key",
                isRecommended = true
            ),
            onSelect = {},
            modifier = Modifier.padding(androidx.compose.ui.unit.dp(16))
        )
    }
}

@Preview(
    name = "Message Bubbles",
    showBackground = true,
    backgroundColor = 0xFF050505,
    widthDp = 393,
    heightDp = 300
)
@Composable
private fun PreviewMessageBubbles() {
    FawxTheme {
        androidx.compose.foundation.layout.Column(
            modifier = Modifier.padding(androidx.compose.ui.unit.dp(16)),
            verticalArrangement = androidx.compose.foundation.layout.Arrangement.spacedBy(
                androidx.compose.ui.unit.dp(8)
            )
        ) {
            PortedMessageBubble(
                message = Message(
                    content = "Open my email and check for new messages",
                    role = MessageRole.USER,
                    timestamp = 1L
                ),
                flavor = FawxFlavor.TANGERINE
            )
            PortedMessageBubble(
                message = Message(
                    content = "I'll open Gmail and check your inbox now.",
                    role = MessageRole.ASSISTANT,
                    timestamp = 2L
                ),
                flavor = FawxFlavor.TANGERINE
            )
            PortedMessageBubble(
                message = Message(
                    content = "📱 Opening Gmail...",
                    role = MessageRole.ACTION,
                    timestamp = 3L
                ),
                flavor = FawxFlavor.TANGERINE
            )
        }
    }
}

@Preview(
    name = "Loading Indicator",
    showBackground = true,
    backgroundColor = 0xFF050505,
    widthDp = 200,
    heightDp = 60
)
@Composable
private fun PreviewLoadingIndicator() {
    FawxTheme {
        PortedLoadingIndicator(
            flavor = FawxFlavor.TANGERINE,
            label = "Thinking"
        )
    }
}

@Preview(
    name = "Chat Empty State",
    showBackground = true,
    backgroundColor = 0xFF050505,
    widthDp = 393,
    heightDp = 400
)
@Composable
private fun PreviewChatEmptyState() {
    FawxTheme {
        ChatEmptyState(
            flavor = FawxFlavor.TANGERINE,
            onSuggestion = {}
        )
    }
}


// ════════════════════════════════════════════════════════════════════
//  2. ONBOARDING SCREENS
// ════════════════════════════════════════════════════════════════════

@Preview(
    name = "Onboarding – Welcome",
    showBackground = true,
    backgroundColor = 0xFF050505,
    widthDp = 393,
    heightDp = 852
)
@Composable
private fun PreviewWelcomeScreen() {
    FawxTheme {
        WelcomeScreen(
            flavor = FawxFlavor.TANGERINE,
            onGetStarted = {}
        )
    }
}

@Preview(
    name = "Onboarding – Flavor",
    showBackground = true,
    backgroundColor = 0xFF050505,
    widthDp = 393,
    heightDp = 852
)
@Composable
private fun PreviewFlavorScreen() {
    FawxTheme {
        FlavorScreen(
            selectedFlavor = FawxFlavor.TANGERINE,
            onFlavorSelected = {},
            onContinue = {},
            onBack = {}
        )
    }
}

@Preview(
    name = "Onboarding – Conversation Style",
    showBackground = true,
    backgroundColor = 0xFF050505,
    widthDp = 393,
    heightDp = 852
)
@Composable
private fun PreviewConversationStyleScreen() {
    FawxTheme {
        ConversationStyleScreen(
            flavor = FawxFlavor.TANGERINE,
            selectedTone = "Casual",
            selectedDetail = null,
            selectedAutonomy = null,
            onToneSelected = {},
            onDetailSelected = {},
            onAutonomySelected = {},
            onContinue = {},
            onBack = {}
        )
    }
}

@Preview(
    name = "Onboarding – Paywall",
    showBackground = true,
    backgroundColor = 0xFF050505,
    widthDp = 393,
    heightDp = 852
)
@Composable
private fun PreviewPaywallScreen() {
    FawxTheme {
        PaywallScreen(
            flavor = FawxFlavor.TANGERINE,
            onPlanSelected = {},
            onBack = {}
        )
    }
}

@Preview(
    name = "Onboarding – API Key",
    showBackground = true,
    backgroundColor = 0xFF050505,
    widthDp = 393,
    heightDp = 852
)
@Composable
private fun PreviewApiKeyScreen() {
    FawxTheme {
        ApiKeyScreen(
            flavor = FawxFlavor.TANGERINE,
            selectedProvider = "Anthropic",
            apiKey = "",
            validationState = ValidationState.Idle,
            onProviderSelected = {},
            onApiKeyChanged = {},
            onValidate = {},
            onContinue = {},
            onSkip = {},
            onBack = {}
        )
    }
}

@Preview(
    name = "Onboarding – Permissions",
    showBackground = true,
    backgroundColor = 0xFF050505,
    widthDp = 393,
    heightDp = 852
)
@Composable
private fun PreviewPermissionsScreen() {
    FawxTheme {
        PermissionsScreen(
            flavor = FawxFlavor.TANGERINE,
            accessibilityGranted = true,
            notificationsGranted = false,
            overlayGranted = false,
            onRequestAccessibility = {},
            onRequestNotifications = {},
            onRequestOverlay = {},
            onContinue = {},
            onBack = {}
        )
    }
}

@Preview(
    name = "Onboarding – Ready",
    showBackground = true,
    backgroundColor = 0xFF050505,
    widthDp = 393,
    heightDp = 852
)
@Composable
private fun PreviewReadyScreen() {
    FawxTheme {
        ReadyScreen(
            flavor = FawxFlavor.TANGERINE,
            onStartChatting = {}
        )
    }
}


// ════════════════════════════════════════════════════════════════════
//  3. MAIN CHAT
// ════════════════════════════════════════════════════════════════════

@Preview(
    name = "Chat – Empty",
    showBackground = true,
    backgroundColor = 0xFF050505,
    widthDp = 393,
    heightDp = 852
)
@Composable
private fun PreviewChatScreenEmpty() {
    FawxTheme {
        ChatScreen(
            flavor = FawxFlavor.TANGERINE,
            messages = emptyList(),
            isLoading = false,
            inputText = "",
            walletState = WalletState(
                providerIcon = Icons.Default.SmartToy,
                shortModelName = "Claude 3.5"
            ),
            onInputChanged = {},
            onSend = {},
            onSuggestion = {},
            onSettingsClick = {},
            onOverlayClick = {},
            onModelChipClick = {}
        )
    }
}

@Preview(
    name = "Chat – With Messages",
    showBackground = true,
    backgroundColor = 0xFF050505,
    widthDp = 393,
    heightDp = 852
)
@Composable
private fun PreviewChatScreenWithMessages() {
    FawxTheme {
        ChatScreen(
            flavor = FawxFlavor.TANGERINE,
            messages = listOf(
                Message(
                    content = "Check my calendar for tomorrow",
                    role = MessageRole.USER,
                    timestamp = 1L
                ),
                Message(
                    content = "I'll look at your calendar. You have 3 events tomorrow:\n\n• 9:00 AM – Team standup\n• 12:00 PM – Lunch with Sarah\n• 3:00 PM – Design review",
                    role = MessageRole.ASSISTANT,
                    timestamp = 2L
                ),
                Message(
                    content = "📱 Opening Google Calendar...",
                    role = MessageRole.ACTION,
                    timestamp = 3L
                ),
                Message(
                    content = "Cancel the lunch with Sarah",
                    role = MessageRole.USER,
                    timestamp = 4L
                ),
            ),
            isLoading = true,
            inputText = "",
            walletState = WalletState(
                providerIcon = Icons.Default.SmartToy,
                shortModelName = "Claude 3.5"
            ),
            onInputChanged = {},
            onSend = {},
            onSuggestion = {},
            onSettingsClick = {},
            onOverlayClick = {},
            onModelChipClick = {}
        )
    }
}

@Preview(
    name = "Chat – Input Bar",
    showBackground = true,
    backgroundColor = 0xFF050505,
    widthDp = 393,
    heightDp = 80
)
@Composable
private fun PreviewChatInputBar() {
    FawxTheme {
        ChatInputBar(
            inputText = "Open my email",
            flavor = FawxFlavor.TANGERINE,
            onInputChanged = {},
            onSend = {}
        )
    }
}

@Preview(
    name = "Chat – Top Bar",
    showBackground = true,
    backgroundColor = 0xFF050505,
    widthDp = 393,
    heightDp = 64
)
@Composable
private fun PreviewChatTopBar() {
    FawxTheme {
        ChatTopBar(
            walletState = WalletState(
                providerIcon = Icons.Default.SmartToy,
                shortModelName = "Claude 3.5"
            ),
            onModelChipClick = {},
            onOverlayClick = {},
            onSettingsClick = {}
        )
    }
}


// ════════════════════════════════════════════════════════════════════
//  4. OVERLAY SYSTEM
// ════════════════════════════════════════════════════════════════════

@Preview(
    name = "Overlay – Mini Chat",
    showBackground = true,
    backgroundColor = 0xFF050505,
    widthDp = 393,
    heightDp = 400
)
@Composable
private fun PreviewOverlayMiniChat() {
    FawxTheme {
        // Note: OverlayMiniChat uses its own FawxFlavor from overlay package.
        // You'll need to consolidate the flavor enum before this compiles.
        OverlayMiniChat(
            flavor = com.fawx.app.ui.overlay.FawxFlavor.TANGERINE,
            logLines = listOf(
                OverlayLogLine("Open Gmail", LogLineType.USER),
                OverlayLogLine("Launching Gmail app...", LogLineType.SYSTEM),
                OverlayLogLine("Navigating to inbox...", LogLineType.SYSTEM),
                OverlayLogLine("Found 3 new emails", LogLineType.SYSTEM),
                OverlayLogLine("Read the first one", LogLineType.QUEUED)
            ),
            currentStep = 2,
            totalSteps = 5,
            runState = OverlayRunState.RUNNING,
            inputText = "",
            onInputChanged = {},
            onSend = {},
            onStop = {},
            onResume = {},
            onRetry = {},
            onExpandToFull = {},
            onCollapseToBubble = {}
        )
    }
}

@Preview(
    name = "Overlay – Mini Chat (Paused)",
    showBackground = true,
    backgroundColor = 0xFF050505,
    widthDp = 393,
    heightDp = 400
)
@Composable
private fun PreviewOverlayMiniChatPaused() {
    FawxTheme {
        OverlayMiniChat(
            flavor = com.fawx.app.ui.overlay.FawxFlavor.TANGERINE,
            logLines = listOf(
                OverlayLogLine("Set a timer for 5 minutes", LogLineType.USER),
                OverlayLogLine("Opening Clock app...", LogLineType.SYSTEM)
            ),
            currentStep = 1,
            totalSteps = 3,
            runState = OverlayRunState.STOPPED,
            inputText = "",
            onInputChanged = {},
            onSend = {},
            onStop = {},
            onResume = {},
            onRetry = {},
            onExpandToFull = {},
            onCollapseToBubble = {}
        )
    }
}

@Preview(
    name = "Overlay – Bubble (Idle)",
    showBackground = true,
    backgroundColor = 0xFF1A1A2E,
    widthDp = 100,
    heightDp = 100
)
@Composable
private fun PreviewOverlayBubbleIdle() {
    FawxTheme {
        OverlayBubble(
            flavor = com.fawx.app.ui.overlay.FawxFlavor.TANGERINE,
            isExecuting = false,
            unreadCount = 0,
            onClick = {},
            onLongPress = {}
        )
    }
}

@Preview(
    name = "Overlay – Bubble (Executing + Badge)",
    showBackground = true,
    backgroundColor = 0xFF1A1A2E,
    widthDp = 100,
    heightDp = 100
)
@Composable
private fun PreviewOverlayBubbleExecuting() {
    FawxTheme {
        OverlayBubble(
            flavor = com.fawx.app.ui.overlay.FawxFlavor.TANGERINE,
            isExecuting = true,
            unreadCount = 3,
            onClick = {},
            onLongPress = {}
        )
    }
}

@Preview(
    name = "Overlay – Preview Screen",
    showBackground = true,
    backgroundColor = 0xFF050505,
    widthDp = 393,
    heightDp = 852
)
@Composable
private fun PreviewOverlayPreviewScreen() {
    FawxTheme {
        OverlayPreviewScreen(
            flavor = com.fawx.app.ui.overlay.FawxFlavor.TANGERINE,
            selectedMode = OverlayMode.MINI_CHAT,
            onModeSelected = {},
            onBack = {}
        )
    }
}


// ════════════════════════════════════════════════════════════════════
//  5. SETTINGS
// ════════════════════════════════════════════════════════════════════

@Preview(
    name = "Settings – Hub",
    showBackground = true,
    backgroundColor = 0xFF050505,
    widthDp = 393,
    heightDp = 852
)
@Composable
private fun PreviewSettingsHub() {
    FawxTheme {
        SettingsHubScreen(
            flavor = com.fawx.app.ui.theme.FawxFlavor.TANGERINE,
            activeKeyLabel = "My Anthropic Key",
            activeModelName = "claude-3.5-sonnet",
            onBack = {},
            onNavigate = {}
        )
    }
}

@Preview(
    name = "Settings – API Keys (with keys)",
    showBackground = true,
    backgroundColor = 0xFF050505,
    widthDp = 393,
    heightDp = 852
)
@Composable
private fun PreviewApiKeysSettings() {
    FawxTheme {
        ApiKeysSettingsScreen(
            flavor = com.fawx.app.ui.theme.FawxFlavor.TANGERINE,
            keys = listOf(
                ApiKeyEntry(
                    id = "1",
                    provider = "Anthropic",
                    label = "My Anthropic Key",
                    maskedKey = "sk-ant-...x8Kz",
                    health = ApiKeyHealth.HEALTHY,
                    isActive = true
                ),
                ApiKeyEntry(
                    id = "2",
                    provider = "OpenAI",
                    label = "OpenAI Backup",
                    maskedKey = "sk-...m4Rp",
                    health = ApiKeyHealth.UNKNOWN,
                    isActive = false
                )
            ),
            activeKeyId = "1",
            selectedChatModel = "claude-3.5-sonnet",
            selectedActionModel = "claude-3.5-haiku",
            chatModels = listOf("claude-3.5-sonnet", "claude-3-opus", "claude-3.5-haiku"),
            actionModels = listOf("claude-3.5-haiku", "claude-3.5-sonnet"),
            onAddKey = {},
            onDeleteKey = {},
            onSetActive = {},
            onChatModelSelected = {},
            onActionModelSelected = {},
            onBack = {}
        )
    }
}

@Preview(
    name = "Settings – API Keys (empty)",
    showBackground = true,
    backgroundColor = 0xFF050505,
    widthDp = 393,
    heightDp = 852
)
@Composable
private fun PreviewApiKeysSettingsEmpty() {
    FawxTheme {
        ApiKeysSettingsScreen(
            flavor = com.fawx.app.ui.theme.FawxFlavor.TANGERINE,
            keys = emptyList(),
            activeKeyId = null,
            selectedChatModel = null,
            selectedActionModel = null,
            chatModels = emptyList(),
            actionModels = emptyList(),
            onAddKey = {},
            onDeleteKey = {},
            onSetActive = {},
            onChatModelSelected = {},
            onActionModelSelected = {},
            onBack = {}
        )
    }
}

@Preview(
    name = "Settings – Models",
    showBackground = true,
    backgroundColor = 0xFF050505,
    widthDp = 393,
    heightDp = 852
)
@Composable
private fun PreviewModelsSettings() {
    FawxTheme {
        ModelsSettingsScreen(
            flavor = com.fawx.app.ui.theme.FawxFlavor.TANGERINE,
            hasActiveKey = true,
            chatModels = listOf("claude-3.5-sonnet", "claude-3-opus", "claude-3.5-haiku", "gpt-4o"),
            actionModels = listOf("claude-3.5-haiku", "claude-3.5-sonnet", "gpt-4o-mini"),
            selectedChatModel = "claude-3.5-sonnet",
            selectedActionModel = "claude-3.5-haiku",
            onChatModelSelected = {},
            onActionModelSelected = {},
            onBack = {}
        )
    }
}

@Preview(
    name = "Settings – Trust Level",
    showBackground = true,
    backgroundColor = 0xFF050505,
    widthDp = 393,
    heightDp = 852
)
@Composable
private fun PreviewTrustSettings() {
    FawxTheme {
        TrustSettingsScreen(
            flavor = com.fawx.app.ui.theme.FawxFlavor.TANGERINE,
            selectedLevel = TrustLevel.ASK_RISKY,
            onLevelSelected = {},
            onBack = {}
        )
    }
}

@Preview(
    name = "Settings – Appearance",
    showBackground = true,
    backgroundColor = 0xFF050505,
    widthDp = 393,
    heightDp = 852
)
@Composable
private fun PreviewAppearanceSettings() {
    FawxTheme {
        AppearanceSettingsScreen(
            flavor = com.fawx.app.ui.theme.FawxFlavor.TANGERINE,
            selectedFlavor = com.fawx.app.ui.theme.FawxFlavor.TANGERINE,
            autoClearTimeout = AutoClearTimeout.NEVER,
            themeMode = ThemeMode.DARK,
            onFlavorSelected = {},
            onAutoClearSelected = {},
            onThemeModeSelected = {},
            onBack = {}
        )
    }
}

@Preview(
    name = "Settings – Phone Control",
    showBackground = true,
    backgroundColor = 0xFF050505,
    widthDp = 393,
    heightDp = 852
)
@Composable
private fun PreviewPhoneControlSettings() {
    FawxTheme {
        PhoneControlSettingsScreen(
            flavor = com.fawx.app.ui.theme.FawxFlavor.TANGERINE,
            accessibilityGranted = true,
            overlayGranted = false,
            defaultOverlayMode = com.fawx.app.ui.settings.OverlayMode.MINI_CHAT,
            onRequestAccessibility = {},
            onRequestOverlay = {},
            onOverlayModeSelected = {},
            onBack = {}
        )
    }
}

@Preview(
    name = "Settings – About",
    showBackground = true,
    backgroundColor = 0xFF050505,
    widthDp = 393,
    heightDp = 852
)
@Composable
private fun PreviewAboutSettings() {
    FawxTheme {
        AboutSettingsScreen(
            flavor = com.fawx.app.ui.theme.FawxFlavor.TANGERINE,
            onBack = {}
        )
    }
}


// ════════════════════════════════════════════════════════════════════
//  6. FLAVOR VARIATIONS (see all 5 flavors side-by-side)
// ════════════════════════════════════════════════════════════════════

@Preview(
    name = "Flavors – Lemon Welcome",
    showBackground = true,
    backgroundColor = 0xFF050505,
    widthDp = 393,
    heightDp = 852
)
@Composable
private fun PreviewWelcomeLemon() {
    FawxTheme {
        WelcomeScreen(flavor = FawxFlavor.LEMON, onGetStarted = {})
    }
}

@Preview(
    name = "Flavors – Lime Welcome",
    showBackground = true,
    backgroundColor = 0xFF050505,
    widthDp = 393,
    heightDp = 852
)
@Composable
private fun PreviewWelcomeLime() {
    FawxTheme {
        WelcomeScreen(flavor = FawxFlavor.LIME, onGetStarted = {})
    }
}

@Preview(
    name = "Flavors – Blood Orange Welcome",
    showBackground = true,
    backgroundColor = 0xFF050505,
    widthDp = 393,
    heightDp = 852
)
@Composable
private fun PreviewWelcomeBloodOrange() {
    FawxTheme {
        WelcomeScreen(flavor = FawxFlavor.BLOOD_ORANGE, onGetStarted = {})
    }
}

@Preview(
    name = "Flavors – Grapefruit Welcome",
    showBackground = true,
    backgroundColor = 0xFF050505,
    widthDp = 393,
    heightDp = 852
)
@Composable
private fun PreviewWelcomeGrapefruit() {
    FawxTheme {
        WelcomeScreen(flavor = FawxFlavor.GRAPEFRUIT, onGetStarted = {})
    }
}
