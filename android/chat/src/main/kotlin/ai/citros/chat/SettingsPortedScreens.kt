package ai.citros.chat

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.Settings
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.VolumeUp
import androidx.compose.material.icons.filled.Brush
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.Key
import androidx.compose.material.icons.filled.PhoneAndroid
import androidx.compose.material.icons.filled.Security
import androidx.compose.material.icons.filled.Tune
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shadow
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import ai.citros.core.WalletManager
import android.accessibilityservice.AccessibilityServiceInfo
import android.view.accessibility.AccessibilityManager
import androidx.compose.foundation.Image
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.ui.draw.clip
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.res.painterResource

/**
 * Helper function to check if accessibility service is enabled for the app
 */
private fun isAccessibilityServiceEnabled(context: Context): Boolean {
    val accessibilityManager = context.getSystemService(Context.ACCESSIBILITY_SERVICE) as AccessibilityManager
    val enabledServices = accessibilityManager.getEnabledAccessibilityServiceList(AccessibilityServiceInfo.FEEDBACK_ALL_MASK)
    val packageName = context.packageName
    return enabledServices.any { it.resolveInfo.serviceInfo.packageName == packageName }
}

@Composable
private fun SettingsSubPageScaffold(
    flavor: CitrosFlavor,
    title: String,
    onBack: () -> Unit,
    scrollable: Boolean = true,
    content: @Composable androidx.compose.foundation.layout.ColumnScope.() -> Unit
) {
    val visuals = remember(flavor) { citrosSplashVisualTokens(flavor) }
    Scaffold(containerColor = Color.Transparent) { padding ->
        Box(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
        ) {
            CitrosHeroShaderSphere(
                flavor = flavor,
                modifier = Modifier.fillMaxSize()
            )
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .background(Color.Black.copy(alpha = 0.44f))
            )

            Column(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(horizontal = 16.dp)
            ) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(top = 6.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.SpaceBetween
                ) {
                    CitrosLiquidGlassSurface(
                        modifier = Modifier.size(40.dp),
                        shape = RoundedCornerShape(999.dp),
                        onClick = onBack,
                        borderColor = flavor.primary.copy(alpha = 0.44f),
                        borderWidth = 1.dp,
                        highlightColor = flavor.primary,
                        warmth = 1.02f
                    ) {
                        Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                            Icon(
                                imageVector = Icons.AutoMirrored.Filled.ArrowBack,
                                contentDescription = "Back",
                                tint = flavor.primary.copy(alpha = 0.96f)
                            )
                        }
                    }
                    Text(
                        title,
                        style = MaterialTheme.typography.headlineSmall.copy(
                            shadow = Shadow(
                                color = visuals.hero.deep.copy(alpha = 0.70f),
                                offset = Offset(0f, 2f),
                                blurRadius = 14f
                            )
                        ),
                        fontWeight = FontWeight.SemiBold,
                        color = flavor.primary
                    )
                    Spacer(modifier = Modifier.size(40.dp))
                }

                val bodyModifier = Modifier
                    .fillMaxSize()
                    .padding(top = 10.dp)

                if (scrollable) {
                    Column(
                        modifier = bodyModifier.verticalScroll(rememberScrollState()),
                        verticalArrangement = Arrangement.spacedBy(12.dp),
                        content = content
                    )
                } else {
                    Column(
                        modifier = bodyModifier,
                        verticalArrangement = Arrangement.spacedBy(12.dp),
                        content = content
                    )
                }
            }
        }
    }
}

@Composable
private fun SettingsGlassPillButton(
    text: String,
    tint: Color,
    onClick: () -> Unit,
    modifier: Modifier = Modifier
) {
    CitrosLiquidGlassSurface(
        modifier = modifier,
        shape = RoundedCornerShape(999.dp),
        onClick = onClick,
        borderColor = tint.copy(alpha = 0.42f),
        borderWidth = 1.dp,
        highlightColor = tint,
        warmth = 1.04f,
        contentPadding = PaddingValues(horizontal = 14.dp, vertical = 8.dp)
    ) {
        Text(
            text,
            style = MaterialTheme.typography.labelLarge,
            fontWeight = FontWeight.SemiBold,
            color = tint.copy(alpha = 0.95f)
        )
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun SettingsHubScreen(
    context: Context,
    walletManager: WalletManager,
    onBack: () -> Unit,
    onOpenWallet: () -> Unit,
    onOpenModels: () -> Unit,
    onOpenTrust: () -> Unit,
    onOpenPhoneControl: () -> Unit,
    onOpenSound: () -> Unit,
    onOpenAppearance: () -> Unit,
    onOpenAbout: () -> Unit
) {
    val walletState = remember { walletManager.loadOrDefault() }
    val activeKey = walletState.keys.find { it.id == walletState.activeKeyId }
    val flavor = remember { readSelectedFlavor(context) }
    val visuals = remember(flavor) { citrosSplashVisualTokens(flavor) }

    Scaffold(
        containerColor = Color.Transparent
    ) { padding ->
        Box(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
        ) {
            CitrosHeroShaderSphere(
                flavor = flavor,
                modifier = Modifier.fillMaxSize()
            )
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .background(Color.Black.copy(alpha = 0.42f))
            )

            Column(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(horizontal = 16.dp)
                    .verticalScroll(rememberScrollState()),
                verticalArrangement = Arrangement.spacedBy(12.dp)
            ) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(top = 6.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.SpaceBetween
                ) {
                    CitrosLiquidGlassSurface(
                        modifier = Modifier
                            .width(40.dp)
                            .height(40.dp),
                        shape = androidx.compose.foundation.shape.RoundedCornerShape(999.dp),
                        onClick = onBack,
                        borderColor = flavor.primary.copy(alpha = 0.44f),
                        borderWidth = 1.dp,
                        highlightColor = flavor.primary,
                        warmth = 1.02f
                    ) {
                        Box(
                            modifier = Modifier.fillMaxSize(),
                            contentAlignment = Alignment.Center
                        ) {
                            Icon(
                                imageVector = Icons.AutoMirrored.Filled.ArrowBack,
                                contentDescription = "Back",
                                tint = flavor.primary.copy(alpha = 0.96f)
                            )
                        }
                    }
                    Text(
                        "Settings",
                        style = MaterialTheme.typography.headlineSmall.copy(
                            shadow = Shadow(
                                color = visuals.hero.deep.copy(alpha = 0.70f),
                                offset = Offset(0f, 2f),
                                blurRadius = 14f
                            )
                        ),
                        fontWeight = FontWeight.SemiBold,
                        color = flavor.primary
                    )
                    Spacer(modifier = Modifier.width(40.dp))
                }

                CitrosLiquidGlassSurface(
                    modifier = Modifier.fillMaxWidth(),
                    shape = androidx.compose.foundation.shape.RoundedCornerShape(20.dp),
                    borderColor = flavor.primary.copy(alpha = 0.38f),
                    borderWidth = 1.dp,
                    highlightColor = flavor.primary,
                    warmth = 1.06f,
                    contentPadding = androidx.compose.foundation.layout.PaddingValues(
                        horizontal = 14.dp,
                        vertical = 14.dp
                    )
                ) {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(12.dp)
                    ) {
                        CitrosLiquidGlassSurface(
                            modifier = Modifier
                                .width(42.dp)
                                .height(42.dp),
                            shape = RoundedCornerShape(10.dp),
                            borderColor = flavor.primary.copy(alpha = 0.34f),
                            borderWidth = 1.dp,
                            highlightColor = flavor.primary,
                            warmth = 0.90f
                        ) {
                            Image(
                                painter = painterResource(id = launcherIconForegroundResForFlavor(flavor)),
                                contentDescription = "Citros app icon",
                                modifier = Modifier
                                    .fillMaxSize()
                                    .clip(RoundedCornerShape(10.dp)),
                                contentScale = ContentScale.Crop
                            )
                        }
                        Column(modifier = Modifier.weight(1f)) {
                            Text(
                                "Citros",
                                style = MaterialTheme.typography.titleMedium,
                                fontWeight = FontWeight.SemiBold,
                                color = flavor.primary.copy(alpha = 0.96f)
                            )
                            Text(
                                activeKey?.let { "${it.label} · ${walletState.chatModelId}" } ?: "No active API key",
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.78f)
                            )
                        }
                    }
                }

                SettingsNavCard(
                    icon = Icons.Default.Key,
                    title = "API Keys",
                    subtitle = "Manage your provider keys",
                    flavor = flavor,
                    onClick = onOpenWallet
                )
                SettingsNavCard(
                    icon = Icons.Default.Tune,
                    title = "Models",
                    subtitle = "Chat & action model selection",
                    flavor = flavor,
                    onClick = onOpenModels
                )
                SettingsNavCard(
                    icon = Icons.AutoMirrored.Filled.VolumeUp,
                    title = "Sound & Haptics",
                    subtitle = "Voice, sounds, haptic feedback",
                    flavor = flavor,
                    onClick = onOpenSound
                )
                SettingsNavCard(
                    icon = Icons.Default.Security,
                    title = "Trust Level",
                    subtitle = "Permission tier settings",
                    flavor = flavor,
                    onClick = onOpenTrust
                )
                SettingsNavCard(
                    icon = Icons.Default.PhoneAndroid,
                    title = "Phone Control",
                    subtitle = "Accessibility & overlay",
                    flavor = flavor,
                    onClick = onOpenPhoneControl
                )
                SettingsNavCard(
                    icon = Icons.Default.Brush,
                    title = "Appearance",
                    subtitle = "Theme & flavor settings",
                    flavor = flavor,
                    onClick = onOpenAppearance
                )
                SettingsNavCard(
                    icon = Icons.Default.Info,
                    title = "About",
                    subtitle = "Version, licenses",
                    flavor = flavor,
                    onClick = onOpenAbout
                )

                Spacer(Modifier.height(20.dp))
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun TrustSettingsScreen(
    context: Context,
    onBack: () -> Unit
) {
    val prefs = remember(context) { context.getSharedPreferences(ONBOARDING_PREFS, Context.MODE_PRIVATE) }
    val flavor = remember { readSelectedFlavor(context) }
    var selected by rememberSaveable {
        mutableStateOf(prefs.getString(PREF_PERSONALITY_TRUST, "Ask for risky stuff") ?: "Ask for risky stuff")
    }

    val options = listOf(
        "Ask before everything",
        "Ask for risky stuff",
        "Full autonomy"
    )

    SettingsSubPageScaffold(
        flavor = flavor,
        title = "Trust Level",
        onBack = onBack
    ) {
        Text(
            "Choose how much autonomy Citros should have while controlling your phone.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.78f)
        )
        options.forEach { option ->
            val isSelected = selected == option
            CitrosLiquidGlassSurface(
                modifier = Modifier.fillMaxWidth(),
                shape = RoundedCornerShape(16.dp),
                onClick = {
                    selected = option
                    prefs.edit().putString(PREF_PERSONALITY_TRUST, option).apply()
                },
                borderColor = if (isSelected) flavor.primary.copy(alpha = 0.66f) else flavor.primary.copy(alpha = 0.30f),
                borderWidth = if (isSelected) 1.6.dp else 1.dp,
                highlightColor = if (isSelected) flavor.primary else null,
                warmth = if (isSelected) 1.08f else 0.84f,
                contentPadding = PaddingValues(14.dp)
            ) {
                Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Text(
                        option,
                        style = MaterialTheme.typography.titleMedium,
                        color = if (isSelected) flavor.primary.copy(alpha = 0.96f)
                        else MaterialTheme.colorScheme.onSurface
                    )
                    Text(
                        when (option) {
                            "Ask before everything" -> "Citros asks before every phone action."
                            "Ask for risky stuff" -> "Citros asks before sensitive actions like send/delete/purchase."
                            else -> "Citros executes without confirmation dialogs."
                        },
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.76f)
                    )
                }
            }
        }
        Spacer(Modifier.height(6.dp))
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun AppearanceSettingsScreen(
    context: Context,
    onBack: () -> Unit
) {
    val prefs = remember(context) { context.getSharedPreferences(ONBOARDING_PREFS, Context.MODE_PRIVATE) }
    var selectedFlavor by rememberSaveable {
        mutableStateOf(readSelectedFlavor(context))
    }
    var themeMode by rememberSaveable {
        mutableStateOf(prefs.getString(PREF_THEME_MODE, THEME_MODE_DEFAULT) ?: THEME_MODE_DEFAULT)
    }

    SettingsSubPageScaffold(
        flavor = selectedFlavor,
        title = "Appearance",
        onBack = onBack
    ) {
        Text(
            "Flavor",
            style = MaterialTheme.typography.titleSmall,
            fontWeight = FontWeight.SemiBold,
            color = selectedFlavor.primary.copy(alpha = 0.94f)
        )
        CitrosFlavor.entries.forEach { flavor ->
            FlavorOptionCard(
                flavor = flavor,
                selected = selectedFlavor == flavor,
                onClick = {
                    selectedFlavor = flavor
                    prefs.edit().putString(PREF_SELECTED_FLAVOR, flavor.storageValue).apply()
                    OverlayService.instance?.refreshAppearanceFromPrefs()
                    // Avoid alias flips while overlay is active; they can destabilize
                    // the running task and close the current screen on some devices.
                    if (!OverlayController.isOverlayActive.value) {
                        runCatching {
                            syncLauncherIconWithPreferences(context)
                        }.onFailure { error ->
                            android.util.Log.w("AppearanceSettings", "Failed to sync launcher icon", error)
                        }
                    }
                }
            )
        }

        Spacer(Modifier.height(6.dp))
        Text(
            "Auto-clear",
            style = MaterialTheme.typography.titleSmall,
            fontWeight = FontWeight.SemiBold,
            color = selectedFlavor.primary.copy(alpha = 0.94f)
        )
        Text(
            "Automatically clear conversation history after inactivity",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.72f)
        )
        run {
            val chatPrefs = remember(context) {
                context.getSharedPreferences(CITROS_PREFS, Context.MODE_PRIVATE)
            }
            var selectedTimeout by rememberSaveable {
                mutableStateOf(
                    chatPrefs.getLong("idle_timeout_ms", ConversationLifecycle.DEFAULT_TIMEOUT_MS)
                )
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                ConversationLifecycle.TIMEOUT_OPTIONS.forEach { (label, timeoutMs) ->
                    val selected = selectedTimeout == timeoutMs
                    CitrosLiquidGlassSurface(
                        modifier = Modifier.weight(1f),
                        shape = RoundedCornerShape(999.dp),
                        onClick = {
                            selectedTimeout = timeoutMs
                            chatPrefs
                                .edit()
                                .putLong("idle_timeout_ms", timeoutMs)
                                .apply()
                        },
                        borderColor = if (selected) {
                            selectedFlavor.primary.copy(alpha = 0.62f)
                        } else {
                            selectedFlavor.primary.copy(alpha = 0.28f)
                        },
                        borderWidth = if (selected) 1.6.dp else 1.dp,
                        highlightColor = if (selected) selectedFlavor.primary else null,
                        warmth = if (selected) 1.10f else 0.80f,
                        contentPadding = PaddingValues(vertical = 10.dp)
                    ) {
                        Box(
                            modifier = Modifier.fillMaxWidth(),
                            contentAlignment = Alignment.Center
                        ) {
                            Text(
                                label,
                                style = MaterialTheme.typography.labelSmall,
                                color = if (selected) selectedFlavor.primary.copy(alpha = 0.96f)
                                else MaterialTheme.colorScheme.onSurface.copy(alpha = 0.84f),
                                maxLines = 1
                            )
                        }
                    }
                }
            }
        }

        Spacer(Modifier.height(6.dp))
        Text(
            "Theme",
            style = MaterialTheme.typography.titleSmall,
            fontWeight = FontWeight.SemiBold,
            color = selectedFlavor.primary.copy(alpha = 0.94f)
        )
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            listOf("dark", "light", "system").forEach { mode ->
                val selected = themeMode == mode
                CitrosLiquidGlassSurface(
                    modifier = Modifier.weight(1f),
                    shape = RoundedCornerShape(999.dp),
                    onClick = {
                        themeMode = mode
                        prefs.edit().putString(PREF_THEME_MODE, mode).apply()
                        OverlayService.instance?.refreshAppearanceFromPrefs()
                    },
                    borderColor = if (selected) {
                        selectedFlavor.primary.copy(alpha = 0.62f)
                    } else {
                        selectedFlavor.primary.copy(alpha = 0.28f)
                    },
                    borderWidth = if (selected) 1.6.dp else 1.dp,
                    highlightColor = if (selected) selectedFlavor.primary else null,
                    warmth = if (selected) 1.10f else 0.80f,
                    contentPadding = PaddingValues(vertical = 10.dp)
                ) {
                    Box(
                        modifier = Modifier.fillMaxWidth(),
                        contentAlignment = Alignment.Center
                    ) {
                        Text(
                            mode.replaceFirstChar { it.uppercase() },
                            color = if (selected) selectedFlavor.primary.copy(alpha = 0.96f)
                            else MaterialTheme.colorScheme.onSurface.copy(alpha = 0.84f)
                        )
                    }
                }
            }
        }
        Spacer(Modifier.height(6.dp))
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun AboutSettingsScreen(onBack: () -> Unit) {
    val context = LocalContext.current
    val flavor = remember { readSelectedFlavor(context) }
    SettingsSubPageScaffold(
        flavor = flavor,
        title = "About",
        onBack = onBack
    ) {
        Text(
            "Citros",
            style = MaterialTheme.typography.headlineMedium,
            fontWeight = FontWeight.SemiBold,
            color = flavor.primary.copy(alpha = 0.96f)
        )
        Text(
            "AI phone agent for Android",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.78f)
        )
        CitrosLiquidGlassSurface(
            modifier = Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(16.dp),
            borderColor = flavor.primary.copy(alpha = 0.34f),
            borderWidth = 1.dp,
            highlightColor = flavor.primary,
            warmth = 0.92f,
            contentPadding = PaddingValues(14.dp)
        ) {
            Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                Text("Version 0.1.0", color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.9f))
                Text("Runtime: Rust + Kotlin", color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.86f))
                Text("UI: Jetpack Compose", color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.86f))
                Text("Min SDK: 28", color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.86f))
            }
        }
        Text(
            "Made with citrus intent.",
            style = MaterialTheme.typography.bodySmall,
            color = flavor.primary.copy(alpha = 0.74f)
        )
        Spacer(Modifier.height(6.dp))
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun SoundSettingsScreen(
    voiceManager: ai.citros.core.VoiceManager?,
    onBack: () -> Unit
) {
    val context = LocalContext.current
    val flavor = remember { readSelectedFlavor(context) }
    val autoSpeak = voiceManager?.autoSpeakResponses?.collectAsState()?.value ?: false
    val autoSend = voiceManager?.autoSendAfterVoice?.collectAsState()?.value ?: false
    SettingsSubPageScaffold(
        flavor = flavor,
        title = "Sound & Haptics",
        onBack = onBack,
        scrollable = true
    ) {
        Spacer(Modifier.height(12.dp))
        // Voice Output section
        CitrosLiquidGlassSurface(
            modifier = Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(20.dp),
            borderColor = flavor.primary.copy(alpha = 0.36f),
            borderWidth = 1.dp,
            highlightColor = flavor.primary,
            warmth = 0.92f,
            contentPadding = PaddingValues(horizontal = 18.dp, vertical = 16.dp)
        ) {
            Column(modifier = Modifier.fillMaxWidth()) {
                Text(
                    "Voice Output",
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.SemiBold,
                    color = flavor.primary.copy(alpha = 0.96f)
                )
                Spacer(Modifier.height(16.dp))
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Column(modifier = Modifier.weight(1f)) {
                        Text(
                            "Read responses aloud",
                            style = MaterialTheme.typography.bodyLarge,
                            color = MaterialTheme.colorScheme.onSurface
                        )
                        Text(
                            "Speak AI responses using on-device TTS",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.6f)
                        )
                    }
                    Switch(
                        checked = autoSpeak,
                        onCheckedChange = { voiceManager?.setAutoSpeakResponses(it) },
                        colors = SwitchDefaults.colors(
                            checkedThumbColor = flavor.primary,
                            checkedTrackColor = flavor.primary.copy(alpha = 0.3f)
                        )
                    )
                }
                Spacer(Modifier.height(12.dp))
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Column(modifier = Modifier.weight(1f)) {
                        Text(
                            "Auto-send voice input",
                            style = MaterialTheme.typography.bodyLarge,
                            color = MaterialTheme.colorScheme.onSurface
                        )
                        Text(
                            "Send message immediately after voice recognition",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.6f)
                        )
                    }
                    Switch(
                        checked = autoSend,
                        onCheckedChange = { voiceManager?.setAutoSendAfterVoice(it) },
                        colors = SwitchDefaults.colors(
                            checkedThumbColor = flavor.primary,
                            checkedTrackColor = flavor.primary.copy(alpha = 0.3f)
                        )
                    )
                }
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun PhoneControlSettingsScreen(
    context: Context,
    onBack: () -> Unit
) {
    val overlayPermissionGranted = Settings.canDrawOverlays(context)
    val accessibilityEnabled = isAccessibilityServiceEnabled(context)
    val flavor = remember { readSelectedFlavor(context) }
    val okColor = Color(0xFF88F5B4)
    val warningColor = Color(0xFFFF8A8A)

    SettingsSubPageScaffold(
        flavor = flavor,
        title = "Phone Control",
        onBack = onBack
    ) {
        Text(
            "Citros needs these permissions to control your phone:",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.78f)
        )

        CitrosLiquidGlassSurface(
            modifier = Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(16.dp),
            borderColor = if (accessibilityEnabled) okColor.copy(alpha = 0.44f) else flavor.primary.copy(alpha = 0.34f),
            borderWidth = 1.dp,
            highlightColor = if (accessibilityEnabled) okColor else flavor.primary,
            warmth = 0.92f,
            contentPadding = PaddingValues(16.dp)
        ) {
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Text("Accessibility Service", style = MaterialTheme.typography.titleMedium)
                    Text(
                        if (accessibilityEnabled) "✓ Granted" else "⚠ Not granted",
                        style = MaterialTheme.typography.bodySmall,
                        color = if (accessibilityEnabled) okColor else warningColor
                    )
                }
                Text(
                    "Required for automated actions like tapping, scrolling, and reading screen content",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.76f)
                )
                SettingsGlassPillButton(
                    text = "Open Settings",
                    tint = if (accessibilityEnabled) okColor else flavor.primary,
                    onClick = {
                        context.startActivity(Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS))
                    }
                )
            }
        }

        CitrosLiquidGlassSurface(
            modifier = Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(16.dp),
            borderColor = if (overlayPermissionGranted) okColor.copy(alpha = 0.44f) else flavor.primary.copy(alpha = 0.34f),
            borderWidth = 1.dp,
            highlightColor = if (overlayPermissionGranted) okColor else flavor.primary,
            warmth = 0.92f,
            contentPadding = PaddingValues(16.dp)
        ) {
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Text("Display over other apps", style = MaterialTheme.typography.titleMedium)
                    Text(
                        if (overlayPermissionGranted) "✓ Granted" else "⚠ Not granted",
                        style = MaterialTheme.typography.bodySmall,
                        color = if (overlayPermissionGranted) okColor else warningColor
                    )
                }
                Text(
                    "Allows Citros to show confirmation dialogs and status indicators",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.76f)
                )
                SettingsGlassPillButton(
                    text = "Open Settings",
                    tint = if (overlayPermissionGranted) okColor else flavor.primary,
                    onClick = {
                        val intent = Intent(Settings.ACTION_MANAGE_OVERLAY_PERMISSION)
                        intent.data = Uri.parse("package:${context.packageName}")
                        context.startActivity(intent)
                    }
                )
            }
        }

        CitrosLiquidGlassSurface(
            modifier = Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(16.dp),
            borderColor = flavor.primary.copy(alpha = 0.34f),
            borderWidth = 1.dp,
            highlightColor = flavor.primary,
            warmth = 0.90f,
            contentPadding = PaddingValues(16.dp)
        ) {
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Text(
                    "Default Overlay Mode",
                    style = MaterialTheme.typography.titleSmall,
                    fontWeight = FontWeight.SemiBold,
                    color = flavor.primary.copy(alpha = 0.94f)
                )
                Text(
                    "How the overlay appears when the agent starts working",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.72f)
                )
                run {
                    val chatPrefs = remember(context) {
                        context.getSharedPreferences(CITROS_PREFS, Context.MODE_PRIVATE)
                    }
                    var selectedMode by rememberSaveable {
                        mutableStateOf(
                            chatPrefs.getString(PREF_DEFAULT_OVERLAY_MODE, OverlaySurfaceMode.MINI_CHAT.toPrefValue())
                                ?: OverlaySurfaceMode.MINI_CHAT.toPrefValue()
                        )
                    }
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        listOf(
                            OverlaySurfaceMode.MINI_CHAT to "Mini Chat",
                            OverlaySurfaceMode.BUBBLE to "Bubble"
                        ).forEach { (mode, label) ->
                            val value = mode.toPrefValue()
                            val selected = selectedMode == value
                            CitrosLiquidGlassSurface(
                                modifier = Modifier.weight(1f),
                                shape = RoundedCornerShape(999.dp),
                                onClick = {
                                    selectedMode = value
                                    chatPrefs
                                        .edit()
                                        .putString(PREF_DEFAULT_OVERLAY_MODE, value)
                                        .apply()
                                },
                                borderColor = if (selected) {
                                    flavor.primary.copy(alpha = 0.62f)
                                } else {
                                    flavor.primary.copy(alpha = 0.28f)
                                },
                                borderWidth = if (selected) 1.6.dp else 1.dp,
                                highlightColor = if (selected) flavor.primary else null,
                                warmth = if (selected) 1.10f else 0.80f,
                                contentPadding = PaddingValues(vertical = 10.dp)
                            ) {
                                Box(
                                    modifier = Modifier.fillMaxWidth(),
                                    contentAlignment = Alignment.Center
                                ) {
                                    Text(
                                        label,
                                        color = if (selected) flavor.primary.copy(alpha = 0.96f)
                                        else MaterialTheme.colorScheme.onSurface.copy(alpha = 0.84f)
                                    )
                                }
                            }
                        }
                    }
                }
            }
        }
        Spacer(Modifier.height(6.dp))
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun ModelsSettingsScreen(
    walletManager: WalletManager,
    onBack: () -> Unit
) {
    val context = LocalContext.current
    val flavor = remember { readSelectedFlavor(context) }
    var walletState by remember { mutableStateOf(walletManager.loadOrDefault()) }
    val activeProvider = walletState.keys.find { it.id == walletState.activeKeyId }?.provider

    SettingsSubPageScaffold(
        flavor = flavor,
        title = "Models",
        onBack = onBack
    ) {
        if (activeProvider != null) {
            ModelSelectionSection(
                activeProvider = activeProvider,
                chatModelId = walletState.chatModelId,
                actionModelId = walletState.actionModelId,
                flavor = flavor,
                onChatChange = { modelId ->
                    walletManager.setChatModel(modelId)
                    walletState = walletManager.loadOrDefault()
                },
                onActionChange = { modelId ->
                    walletManager.setActionModel(modelId)
                    walletState = walletManager.loadOrDefault()
                }
            )
        } else {
            CitrosLiquidGlassSurface(
                modifier = Modifier.fillMaxWidth(),
                shape = RoundedCornerShape(18.dp),
                borderColor = flavor.primary.copy(alpha = 0.34f),
                borderWidth = 1.dp,
                highlightColor = flavor.primary,
                warmth = 0.92f,
                contentPadding = PaddingValues(16.dp)
            ) {
                Column(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalAlignment = Alignment.CenterHorizontally
                ) {
                    Icon(
                        imageVector = Icons.Filled.Key,
                        contentDescription = "No API Key",
                        tint = flavor.primary.copy(alpha = 0.95f),
                        modifier = Modifier.padding(12.dp)
                    )
                    Text(
                        "No API Key Active",
                        style = MaterialTheme.typography.titleLarge,
                        fontWeight = FontWeight.SemiBold,
                        color = flavor.primary.copy(alpha = 0.96f)
                    )
                    Spacer(Modifier.height(8.dp))
                    Text(
                        "Add an API key in Settings → API Keys to configure model preferences",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.76f)
                    )
                }
            }
        }
        Spacer(Modifier.height(6.dp))
    }
}

@Composable
private fun SettingsNavCard(
    icon: ImageVector,
    title: String,
    subtitle: String,
    flavor: CitrosFlavor,
    onClick: () -> Unit
) {
    CitrosLiquidGlassSurface(
        modifier = Modifier
            .fillMaxWidth(),
        shape = androidx.compose.foundation.shape.RoundedCornerShape(16.dp),
        onClick = onClick,
        borderColor = flavor.primary.copy(alpha = 0.34f),
        borderWidth = 1.dp,
        highlightColor = flavor.primary,
        warmth = 0.92f,
        contentPadding = androidx.compose.foundation.layout.PaddingValues(14.dp)
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp)
        ) {
            CitrosLiquidGlassSurface(
                shape = androidx.compose.foundation.shape.RoundedCornerShape(10.dp),
                borderColor = flavor.primary.copy(alpha = 0.30f),
                borderWidth = 1.dp,
                highlightColor = flavor.primary,
                warmth = 1.08f,
                contentPadding = androidx.compose.foundation.layout.PaddingValues(8.dp)
            ) {
                Icon(
                    imageVector = icon,
                    contentDescription = null,
                    tint = flavor.primary.copy(alpha = 0.96f)
                )
            }
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    title,
                    style = MaterialTheme.typography.titleMedium,
                    color = flavor.primary.copy(alpha = 0.94f)
                )
                Text(
                    subtitle,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.78f)
                )
            }
            Text("›", color = flavor.primary.copy(alpha = 0.72f))
        }
    }
}
