package ai.citros.chat
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import ai.citros.core.ModelConfig
import ai.citros.core.Message
import ai.citros.core.Provider
import ai.citros.core.WalletState
import android.content.Context
import androidx.compose.ui.platform.LocalContext
@Composable
internal fun ProviderModelChip(
    walletState: WalletState,
    onClick: () -> Unit,
    flavor: CitrosFlavor = CitrosFlavor.TANGERINE,
    modifier: Modifier = Modifier
) {
    val activeKey = walletState.keys.find { it.id == walletState.activeKeyId } ?: return
    val isDarkTheme = LocalCitrosIsDark.current
    val surfaces = remember(isDarkTheme) { citrosDirectiveSurfaces(isDarkTheme) }
    val accent = lerp(ProviderUi.brandColor(activeKey.provider), flavor.primary, 0.45f)
    Row(
        modifier = modifier
            .clip(RoundedCornerShape(999.dp))
            .background(surfaces.surface2)
            .border(1.dp, accent.copy(alpha = 0.40f), RoundedCornerShape(999.dp))
            .clickable(onClick = onClick)
            .padding(horizontal = 10.dp, vertical = 7.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp)
    ) {
        Text(
            ProviderUi.icon(activeKey.provider),
            style = CitrosTypography.labelMedium
        )
        Text(
            text = shortModelName(walletState.chatModelId),
            style = CitrosTypography.labelMedium,
            fontWeight = FontWeight.SemiBold,
            color = accent
        )
        Text(
            text = "▾",
            style = CitrosTypography.titleLarge,
            fontWeight = FontWeight.Bold,
            color = surfaces.labelPrimary
        )
    }
}
@Composable
internal fun ChatEmptyState(
    flavor: CitrosFlavor
) {
    val isDarkTheme = LocalCitrosIsDark.current
    val surfaces = remember(isDarkTheme) { citrosDirectiveSurfaces(isDarkTheme) }
    val flavorTokens = remember(flavor, surfaces) {
        citrosDirectiveFlavorTokens(flavor, surfaces)
    }
    CitrosDirectiveWashBox(
        modifier = Modifier.fillMaxWidth(),
        washColor = flavorTokens.washColor,
        centerXFraction = 0.5f,
        centerYFraction = 0.40f,
        radiusFraction = 0.78f
    ) {
        Column(
            modifier = Modifier.fillMaxWidth(),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(14.dp)
        ) {
            CitrosDirectiveOrb(
                flavor = flavor,
                size = 60.dp
            )
            Text(
                "How can I help?",
                style = CitrosTypography.headlineSmall,
                fontWeight = FontWeight.SemiBold,
                color = surfaces.labelPrimary
            )
        }
    }
}
@Composable
internal fun QuickSwitcherSheet(
    walletState: WalletState,
    flavor: CitrosFlavor = CitrosFlavor.TANGERINE,
    onDismiss: () -> Unit,
    onSelectKey: (String) -> Unit,
    onSelectChatModel: (String) -> Unit,
    onSelectActionModel: (String) -> Unit,
    onManageKeys: () -> Unit
) {
    val activeKey = walletState.keys.find { it.id == walletState.activeKeyId }
    val provider = activeKey?.provider
    val context = LocalContext.current
    val isDarkTheme = LocalCitrosIsDark.current
    val surfaces = remember(isDarkTheme) { citrosDirectiveSurfaces(isDarkTheme) }
    val prefs = remember(context) { context.getSharedPreferences(CITROS_PREFS, Context.MODE_PRIVATE) }
    var useLocalOffline by remember {
        mutableStateOf(prefs.getBoolean("models_use_local_offline", true))
    }
    val localModelId = remember(prefs) {
        prefs.getString("local_model", "qwen2.5:3b") ?: "qwen2.5:3b"
    }
    val cloudModels = remember(provider) {
        provider?.let { ModelConfig.chatModelsForProvider(it) } ?: emptyList()
    }
    val modelRows = remember(cloudModels, localModelId) {
        buildList {
            add(
                QuickSwitcherModelRow(
                    modelId = localModelId,
                    title = "llama.cpp",
                    subtitle = "On-device · Fastest",
                    badgeLabel = "Local",
                    badgeColor = null,
                    tier = QuickSwitcherModelTier.FAST
                )
            )
            cloudModels.forEach { modelId ->
                add(
                    QuickSwitcherModelRow(
                        modelId = modelId,
                        title = quickSwitcherModelTitle(modelId),
                        subtitle = quickSwitcherModelSubtitle(modelId),
                        badgeLabel = "Cloud",
                        badgeColor = surfaces.blue,
                        tier = quickSwitcherModelTier(modelId)
                    )
                )
            }
        }
    }
    val selectedAccent = if (flavor == CitrosFlavor.NONE) {
        surfaces.labelSecondary
    } else {
        flavor.primary
    }

    Box(modifier = Modifier.fillMaxSize()) {
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(Color.Black.copy(alpha = if (isDarkTheme) 0.34f else 0.15f))
                .clickable(
                    interactionSource = remember { MutableInteractionSource() },
                    indication = null,
                    onClick = onDismiss
                )
        )
        Column(
            modifier = Modifier
                .align(Alignment.TopCenter)
                .padding(top = 96.dp, start = 16.dp, end = 16.dp)
                .fillMaxWidth()
                .widthIn(max = 428.dp)
                .clip(RoundedCornerShape(20.dp))
                .background(
                    if (isDarkTheme) {
                        Color(0xF51C1C1E)
                    } else {
                        Color(0xF5FFFFFF)
                    }
                )
                .border(
                    width = if (isDarkTheme) 0.dp else 1.dp,
                    color = if (isDarkTheme) Color.Transparent else surfaces.separator,
                    shape = RoundedCornerShape(20.dp)
                )
                .clickable(
                    interactionSource = remember { MutableInteractionSource() },
                    indication = null,
                    onClick = {}
                )
        ) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 16.dp, vertical = 14.dp),
                verticalAlignment = Alignment.CenterVertically
            ) {
                Text(
                    text = "MODEL",
                    style = CitrosTypography.labelLarge,
                    color = surfaces.labelSecondary
                )
                Spacer(Modifier.weight(1f))
                Row(
                    modifier = Modifier.clickable { onManageKeys() },
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(4.dp)
                ) {
                    CitrosIcon(
                        imageVector = CitrosIcons.Settings,
                        contentDescription = null,
                        tint = surfaces.labelTertiary,
                        modifier = Modifier.size(14.dp)
                    )
                    Text(
                        text = "Manage",
                        style = CitrosTypography.bodySmall,
                        color = surfaces.labelTertiary
                    )
                }
            }

            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 8.dp, vertical = 4.dp),
                verticalArrangement = Arrangement.spacedBy(2.dp)
            ) {
                modelRows.forEach { row ->
                    val selected = row.modelId == walletState.chatModelId
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .clip(RoundedCornerShape(16.dp))
                            .background(if (selected) surfaces.surface2 else Color.Transparent)
                            .clickable {
                                if (activeKey != null) {
                                    onSelectKey(activeKey.id)
                                }
                                onSelectChatModel(row.modelId)
                            }
                            .padding(horizontal = 10.dp, vertical = 10.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(10.dp)
                    ) {
                        Box(
                            modifier = Modifier
                                .size(36.dp)
                                .clip(RoundedCornerShape(12.dp))
                                .background(if (selected) surfaces.surface3 else surfaces.surface1),
                            contentAlignment = Alignment.Center
                        ) {
                            QuickSwitcherTierIcon(
                                tier = row.tier,
                                tint = if (selected) selectedAccent else surfaces.labelSecondary
                            )
                        }
                        Column(modifier = Modifier.weight(1f)) {
                            Text(
                                text = row.title,
                                style = CitrosTypography.titleLarge,
                                fontWeight = if (selected) FontWeight.SemiBold else FontWeight.Normal,
                                color = surfaces.labelPrimary
                            )
                            Text(
                                text = row.subtitle,
                                style = CitrosTypography.bodyMedium,
                                color = surfaces.labelTertiary
                            )
                        }
                        Box(
                            modifier = Modifier
                                .clip(RoundedCornerShape(8.dp))
                                .background((row.badgeColor ?: surfaces.green).copy(alpha = 0.17f))
                                .padding(horizontal = 10.dp, vertical = 3.dp)
                        ) {
                            Text(
                                text = row.badgeLabel,
                                style = CitrosTypography.labelLarge,
                                color = row.badgeColor ?: surfaces.green
                            )
                        }
                    }
                }
            }

            HorizontalDivider(color = surfaces.separatorLight, thickness = 0.6.dp)

            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 16.dp, vertical = 12.dp),
                verticalAlignment = Alignment.CenterVertically
            ) {
                Text(
                    text = "Use local when offline",
                    style = CitrosTypography.headlineSmall,
                    color = surfaces.labelPrimary,
                    modifier = Modifier.weight(1f)
                )
                Switch(
                    checked = useLocalOffline,
                    onCheckedChange = {
                        useLocalOffline = it
                        prefs.edit().putBoolean("models_use_local_offline", it).apply()
                    },
                    colors = SwitchDefaults.colors(
                        checkedTrackColor = surfaces.green,
                        checkedThumbColor = Color.White,
                        uncheckedTrackColor = surfaces.surface3,
                        uncheckedThumbColor = Color.White
                    )
                )
            }
        }
    }
}

private data class QuickSwitcherModelRow(
    val modelId: String,
    val title: String,
    val subtitle: String,
    val badgeLabel: String,
    val badgeColor: Color?,
    val tier: QuickSwitcherModelTier
)

private enum class QuickSwitcherModelTier {
    FAST,
    BALANCED,
    CAPABLE
}

private fun quickSwitcherModelTitle(modelId: String): String {
    val label = shortModelName(modelId)
    return when {
        label.startsWith("Sonnet") -> "Claude $label"
        label.startsWith("Haiku") -> "Claude $label"
        label.startsWith("Opus") -> "Claude $label"
        else -> label
    }
}

private fun quickSwitcherModelSubtitle(modelId: String): String {
    val lowered = modelId.lowercase()
    return when {
        lowered.contains("opus") -> "Cloud · Most capable"
        lowered.contains("gpt") -> "Cloud · Fast multi-modal"
        else -> "Cloud · Balanced"
    }
}

private fun quickSwitcherModelTier(modelId: String): QuickSwitcherModelTier {
    val lowered = modelId.lowercase()
    return when {
        lowered.contains("opus") -> QuickSwitcherModelTier.CAPABLE
        else -> QuickSwitcherModelTier.BALANCED
    }
}

@Composable
private fun QuickSwitcherTierIcon(
    tier: QuickSwitcherModelTier,
    tint: Color
) {
    when (tier) {
        QuickSwitcherModelTier.FAST -> {
            Canvas(modifier = Modifier.size(14.dp)) {
                val bolt = Path().apply {
                    moveTo(size.width * 0.62f, size.height * 0.05f)
                    lineTo(size.width * 0.26f, size.height * 0.58f)
                    lineTo(size.width * 0.52f, size.height * 0.58f)
                    lineTo(size.width * 0.40f, size.height * 0.96f)
                    lineTo(size.width * 0.78f, size.height * 0.44f)
                    lineTo(size.width * 0.52f, size.height * 0.44f)
                    close()
                }
                drawPath(
                    path = bolt,
                    color = tint,
                    style = Stroke(width = 1.6.dp.toPx())
                )
            }
        }
        QuickSwitcherModelTier.BALANCED -> {
            Canvas(modifier = Modifier.size(14.dp)) {
                drawCircle(
                    color = tint,
                    radius = size.minDimension * 0.36f,
                    style = Stroke(width = 1.5.dp.toPx())
                )
                drawCircle(
                    color = tint,
                    radius = size.minDimension * 0.11f
                )
            }
        }
        QuickSwitcherModelTier.CAPABLE -> {
            Text(
                text = "★",
                style = CitrosTypography.bodyLarge,
                color = tint
            )
        }
    }
}
/** Alpha for steer (mid-loop redirect) message bubble background. */
private const val STEER_MESSAGE_ALPHA = 0.22f
@Composable
internal fun PortedMessageBubble(
    message: Message,
    flavor: CitrosFlavor
) {
    val isDarkTheme = LocalCitrosIsDark.current
    val surfaces = remember(isDarkTheme) { citrosDirectiveSurfaces(isDarkTheme) }
    val flavorTokens = remember(flavor, surfaces) {
        citrosDirectiveFlavorTokens(flavor, surfaces)
    }
    val isUser = message.role == "user"
    val isSteer = isUser && message.isSteer
    val isAction = !isUser && (
        message.content.startsWith("🤖") ||
            message.content.startsWith("📱") ||
            message.content.startsWith("👁") ||
            message.content.startsWith("📅") ||
            message.content.contains("[Tools:")
        )
    val userText = flavorTokens.userBubbleText
    val assistantBubbleColor = if (isDarkTheme) Color(0xFF2C2D33) else Color(0xFFD9DAE1)
    val actionText = message.content
        .removePrefix("🤖")
        .removePrefix("📱")
        .removePrefix("👁")
        .removePrefix("📅")
        .trim()
    if (isAction) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.Start,
            verticalAlignment = Alignment.CenterVertically
        ) {
            Text(
                text = "🗓",
                style = CitrosTypography.bodySmall,
                color = surfaces.labelTertiary
            )
            Spacer(Modifier.width(6.dp))
            Text(
                text = actionText,
                style = CitrosTypography.bodySmall,
                color = surfaces.labelSecondary
            )
            Spacer(Modifier.width(6.dp))
            Text(
                text = "✓",
                style = CitrosTypography.labelSmall,
                color = surfaces.green
            )
        }
        return
    }
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = if (isUser) Arrangement.End else Arrangement.Start
    ) {
        Column(horizontalAlignment = if (isUser) Alignment.End else Alignment.Start) {
            val bubbleShape = RoundedCornerShape(18.dp)
            Surface(
                modifier = Modifier.widthIn(max = 348.dp),
                shape = bubbleShape,
                color = when {
                    isSteer -> flavor.primary.copy(alpha = STEER_MESSAGE_ALPHA)
                    isUser -> flavor.primary
                    else -> assistantBubbleColor
                },
                border = if (isUser) {
                    null
                } else {
                    null
                }
            ) {
                if (isUser) {
                    Text(
                        text = message.content,
                        modifier = Modifier.padding(horizontal = 12.dp, vertical = 10.dp),
                        color = userText,
                        style = CitrosTypography.bodyMedium
                    )
                } else {
                    MarkdownText(
                        text = message.content,
                        modifier = Modifier.padding(horizontal = 12.dp, vertical = 10.dp),
                        color = surfaces.labelPrimary,
                        style = CitrosTypography.bodyMedium
                    )
                }
            }
            if (isSteer) {
                Text(
                    text = "↗ redirected",
                    style = CitrosTypography.labelSmall,
                    color = surfaces.labelTertiary,
                    modifier = Modifier.padding(top = 2.dp, end = 4.dp)
                )
            }
        }
    }
}
@Composable
internal fun PortedLoadingIndicator(flavor: CitrosFlavor = CitrosFlavor.TANGERINE, label: String = "Thinking") {
    val isDarkTheme = LocalCitrosIsDark.current
    val surfaces = remember(isDarkTheme) { citrosDirectiveSurfaces(isDarkTheme) }
    val flavorTokens = remember(flavor, surfaces) {
        citrosDirectiveFlavorTokens(flavor, surfaces)
    }
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.Start
    ) {
        Surface(
            shape = RoundedCornerShape(18.dp),
            color = surfaces.surface2,
            border = BorderStroke(1.dp, surfaces.separatorLight),
            modifier = Modifier
                .widthIn(max = 260.dp)
                .padding(vertical = 1.dp)
        ) {
            Row(
                modifier = Modifier.padding(horizontal = 12.dp, vertical = 10.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp)
            ) {
                Box(
                    modifier = Modifier
                        .size(6.dp)
                        .background(flavorTokens.orbColor.copy(alpha = 0.64f), CircleShape)
                )
                Text(
                    text = "$label...",
                    style = CitrosTypography.bodySmall.copy(fontStyle = FontStyle.Italic),
                    color = surfaces.labelSecondary
                )
            }
        }
    }
}
internal fun shortModelName(modelId: String): String {
    val raw = modelId.substringAfterLast('/').removeSuffix("-latest")
    // Map known model IDs to clean display names
    val knownModels = mapOf(
        "claude-sonnet-4-5" to "Sonnet 4.5",
        "claude-haiku-4-5" to "Haiku 4.5",
        "claude-opus-4-5" to "Opus 4.5",
        "claude-opus-4-6" to "Opus 4.6",
        // OpenRouter dot-based model IDs
        "claude-sonnet-4.5" to "Sonnet 4.5",
        "claude-haiku-4.5" to "Haiku 4.5",
        "claude-opus-4.5" to "Opus 4.5",
        "gpt-4o" to "GPT-4o",
        "gpt-4o-mini" to "GPT-4o Mini",
        "o1" to "o1",
        "gpt-5" to "GPT-5",
        "gpt-5.2" to "GPT-5.2",
        "deepseek-r1" to "DeepSeek R1",
        "gemini-pro" to "Gemini Pro"
    )
    // Try exact match first, then prefix match for dated variants
    knownModels[raw]?.let { return it }
    knownModels.entries.find { raw.startsWith(it.key) }?.let { return it.value }
    // Fallback: clean up unknown models
    val cleaned = raw.replace(Regex("-\\d{8}$"), "") // strip date suffixes like -20250514
    // Special handling for GPT models: normalize prefix to uppercase
    if (cleaned.startsWith("gpt-")) {
        return cleaned.replace("gpt-", "GPT-")
    }
    // For other models: strip claude- prefix, convert dashes to spaces, capitalize
    return cleaned
        .replace("claude-", "")
        .replace("-", " ")
        .replaceFirstChar { it.uppercase() }
}
