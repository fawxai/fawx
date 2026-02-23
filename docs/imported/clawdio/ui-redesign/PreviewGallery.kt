@file:OptIn(ExperimentalMaterial3Api::class)

package com.citros.app.ui.preview

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.SmartToy
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.tooling.preview.Preview

// ─── Theme ───
import com.citros.app.ui.theme.CitrosTheme

// ─── Flavor (NOTE: consolidate to one canonical location in your project) ───
import com.citros.app.ui.onboarding.CitrosFlavor

// ─── Components ───
import com.citros.app.ui.components.*

// ─── Onboarding ───
import com.citros.app.ui.onboarding.*

// ─── Chat ───
import com.citros.app.ui.chat.*

// ─── Overlay ───
import com.citros.app.ui.overlay.*

// ─── Settings ───
import com.citros.app.ui.settings.*


// ╔══════════════════════════════════════════════════════════════════╗
// ║  INTEGRATION NOTE                                               ║
// ║                                                                  ║
// ║  CitrosFlavor is defined in 3 places right now:                  ║
// ║    - onboarding/CitrosFlavor.kt                                 ║
// ║    - overlay/OverlayBubble.kt (local copy)                      ║
// ║    - settings imports from theme package                        ║
// ║                                                                  ║
// ║  Before compiling: move CitrosFlavor to one canonical package   ║
// ║  (e.g., com.citros.app.ui.theme.CitrosFlavor) and update all   ║
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
    CitrosTheme {
        CitrosHeroSphere(
            flavor = CitrosFlavor.TANGERINE,
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
    CitrosTheme {
        CitrusHeroBadge(flavor = CitrosFlavor.TANGERINE, size = 68)
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
    CitrosTheme {
        CitrosPrimaryButton(
            text = "Get Started",
            onClick = {},
            flavor = CitrosFlavor.TANGERINE,
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
    CitrosTheme {
        CitrosSecondaryButton(
            text = "Skip for now",
            onClick = {},
            flavor = CitrosFlavor.TANGERINE,
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
    CitrosTheme {
        CitrosStepHeader(
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
    CitrosTheme {
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
    CitrosTheme {
        FlavorOptionCard(
            flavor = CitrosFlavor.TANGERINE,
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
    CitrosTheme {
        PlanCard(
            plan = CitrosPlanSpec(
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
    CitrosTheme {
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
                flavor = CitrosFlavor.TANGERINE
            )
            PortedMessageBubble(
                message = Message(
                    content = "I'll open Gmail and check your inbox now.",
                    role = MessageRole.ASSISTANT,
                    timestamp = 2L
                ),
                flavor = CitrosFlavor.TANGERINE
            )
            PortedMessageBubble(
                message = Message(
                    content = "📱 Opening Gmail...",
                    role = MessageRole.ACTION,
                    timestamp = 3L
                ),
                flavor = CitrosFlavor.TANGERINE
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
    CitrosTheme {
        PortedLoadingIndicator(
            flavor = CitrosFlavor.TANGERINE,
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
    CitrosTheme {
        ChatEmptyState(
            flavor = CitrosFlavor.TANGERINE,
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
    CitrosTheme {
        WelcomeScreen(
            flavor = CitrosFlavor.TANGERINE,
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
    CitrosTheme {
        FlavorScreen(
            selectedFlavor = CitrosFlavor.TANGERINE,
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
    CitrosTheme {
        ConversationStyleScreen(
            flavor = CitrosFlavor.TANGERINE,
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
    CitrosTheme {
        PaywallScreen(
            flavor = CitrosFlavor.TANGERINE,
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
    CitrosTheme {
        ApiKeyScreen(
            flavor = CitrosFlavor.TANGERINE,
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
    CitrosTheme {
        PermissionsScreen(
            flavor = CitrosFlavor.TANGERINE,
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
    CitrosTheme {
        ReadyScreen(
            flavor = CitrosFlavor.TANGERINE,
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
    CitrosTheme {
        ChatScreen(
            flavor = CitrosFlavor.TANGERINE,
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
    CitrosTheme {
        ChatScreen(
            flavor = CitrosFlavor.TANGERINE,
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
    CitrosTheme {
        ChatInputBar(
            inputText = "Open my email",
            flavor = CitrosFlavor.TANGERINE,
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
    CitrosTheme {
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
    CitrosTheme {
        // Note: OverlayMiniChat uses its own CitrosFlavor from overlay package.
        // You'll need to consolidate the flavor enum before this compiles.
        OverlayMiniChat(
            flavor = com.citros.app.ui.overlay.CitrosFlavor.TANGERINE,
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
    CitrosTheme {
        OverlayMiniChat(
            flavor = com.citros.app.ui.overlay.CitrosFlavor.TANGERINE,
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
    CitrosTheme {
        OverlayBubble(
            flavor = com.citros.app.ui.overlay.CitrosFlavor.TANGERINE,
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
    CitrosTheme {
        OverlayBubble(
            flavor = com.citros.app.ui.overlay.CitrosFlavor.TANGERINE,
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
    CitrosTheme {
        OverlayPreviewScreen(
            flavor = com.citros.app.ui.overlay.CitrosFlavor.TANGERINE,
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
    CitrosTheme {
        SettingsHubScreen(
            flavor = com.citros.app.ui.theme.CitrosFlavor.TANGERINE,
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
    CitrosTheme {
        ApiKeysSettingsScreen(
            flavor = com.citros.app.ui.theme.CitrosFlavor.TANGERINE,
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
    CitrosTheme {
        ApiKeysSettingsScreen(
            flavor = com.citros.app.ui.theme.CitrosFlavor.TANGERINE,
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
    CitrosTheme {
        ModelsSettingsScreen(
            flavor = com.citros.app.ui.theme.CitrosFlavor.TANGERINE,
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
    CitrosTheme {
        TrustSettingsScreen(
            flavor = com.citros.app.ui.theme.CitrosFlavor.TANGERINE,
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
    CitrosTheme {
        AppearanceSettingsScreen(
            flavor = com.citros.app.ui.theme.CitrosFlavor.TANGERINE,
            selectedFlavor = com.citros.app.ui.theme.CitrosFlavor.TANGERINE,
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
    CitrosTheme {
        PhoneControlSettingsScreen(
            flavor = com.citros.app.ui.theme.CitrosFlavor.TANGERINE,
            accessibilityGranted = true,
            overlayGranted = false,
            defaultOverlayMode = com.citros.app.ui.settings.OverlayMode.MINI_CHAT,
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
    CitrosTheme {
        AboutSettingsScreen(
            flavor = com.citros.app.ui.theme.CitrosFlavor.TANGERINE,
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
    CitrosTheme {
        WelcomeScreen(flavor = CitrosFlavor.LEMON, onGetStarted = {})
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
    CitrosTheme {
        WelcomeScreen(flavor = CitrosFlavor.LIME, onGetStarted = {})
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
    CitrosTheme {
        WelcomeScreen(flavor = CitrosFlavor.BLOOD_ORANGE, onGetStarted = {})
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
    CitrosTheme {
        WelcomeScreen(flavor = CitrosFlavor.GRAPEFRUIT, onGetStarted = {})
    }
}
