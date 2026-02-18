package ai.citros.chat

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import ai.citros.core.ModelConfig
import ai.citros.core.Message
import ai.citros.core.Provider
import ai.citros.core.WalletState

@Composable
internal fun ProviderModelChip(
    walletState: WalletState,
    onClick: () -> Unit,
    flavor: CitrosFlavor = CitrosFlavor.TANGERINE,
    modifier: Modifier = Modifier
) {
    val activeKey = walletState.keys.find { it.id == walletState.activeKeyId } ?: return
    val accent = lerp(ProviderUi.brandColor(activeKey.provider), flavor.primary, 0.45f)
    CitrosLiquidGlassSurface(
        modifier = modifier,
        shape = RoundedCornerShape(999.dp),
        onClick = onClick,
        borderColor = accent.copy(alpha = 0.46f),
        borderWidth = 1.dp,
        highlightColor = accent,
        warmth = 1.02f,
        contentPadding = androidx.compose.foundation.layout.PaddingValues(horizontal = 10.dp, vertical = 7.dp)
    ) {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(6.dp)
        ) {
            Text(ProviderUi.icon(activeKey.provider), style = MaterialTheme.typography.labelMedium)
            Text(
                text = shortModelName(walletState.chatModelId),
                style = MaterialTheme.typography.labelMedium,
                fontWeight = FontWeight.SemiBold,
                color = accent
            )
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)
@Composable
internal fun QuickSwitcherSheet(
    walletState: WalletState,
    keyStore: ai.citros.core.KeyStore,
    flavor: CitrosFlavor = CitrosFlavor.TANGERINE,
    onDismiss: () -> Unit,
    onSelectKey: (String) -> Unit,
    onSelectChatModel: (String) -> Unit,
    onSelectActionModel: (String) -> Unit,
    onManageKeys: () -> Unit
) {
    val activeKey = walletState.keys.find { it.id == walletState.activeKeyId }
    val provider = activeKey?.provider

    ModalBottomSheet(
        onDismissRequest = onDismiss,
        containerColor = Color(0xF4060606),
        contentColor = MaterialTheme.colorScheme.onSurface,
        dragHandle = {
            Box(
                modifier = Modifier
                    .padding(top = 12.dp, bottom = 4.dp)
                    .size(width = 44.dp, height = 5.dp)
                    .background(flavor.primary.copy(alpha = 0.42f), RoundedCornerShape(999.dp))
            )
        }
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp)
        ) {
            Text(
                "Quick Switcher",
                style = MaterialTheme.typography.titleLarge,
                fontWeight = FontWeight.SemiBold,
                color = flavor.primary
            )

            if (walletState.keys.isEmpty()) {
                Text(
                    "No keys available. Add one in Settings.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.72f)
                )
            } else {
                Text(
                    "Active Key",
                    style = MaterialTheme.typography.labelMedium,
                    color = flavor.primary.copy(alpha = 0.78f)
                )
                activeKey?.let { key ->
                    KeySwitchRow(
                        label = key.label,
                        provider = key.provider,
                        flavor = flavor,
                        maskedKey = maskApiKey(keyStore.get(key.id)),
                        selected = true,
                        onClick = { onSelectKey(key.id) }
                    )
                }

                val otherKeys = walletState.keys.filter { it.id != walletState.activeKeyId }
                if (otherKeys.isNotEmpty()) {
                    Text(
                        "Other Keys",
                        style = MaterialTheme.typography.labelMedium,
                        color = flavor.primary.copy(alpha = 0.78f)
                    )
                    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        otherKeys.forEach { key ->
                            KeySwitchRow(
                                label = key.label,
                                provider = key.provider,
                                flavor = flavor,
                                maskedKey = maskApiKey(keyStore.get(key.id)),
                                selected = false,
                                onClick = { onSelectKey(key.id) }
                            )
                        }
                    }
                }

                provider?.let {
                    val chatModels = ModelConfig.chatModelsForProvider(it)
                    val actionModels = ModelConfig.actionModelsForProvider(it)

                    Text(
                        "Chat Model",
                        style = MaterialTheme.typography.labelMedium,
                        color = flavor.primary.copy(alpha = 0.78f)
                    )
                    FlowRow(
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                        verticalArrangement = Arrangement.spacedBy(8.dp)
                    ) {
                        chatModels.forEach { modelId ->
                            FilterChip(
                                selected = modelId == walletState.chatModelId,
                                onClick = { onSelectChatModel(modelId) },
                                label = { Text(shortModelName(modelId)) },
                                colors = FilterChipDefaults.filterChipColors(
                                    containerColor = MaterialTheme.colorScheme.surface.copy(alpha = 0.52f),
                                    labelColor = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.84f),
                                    selectedContainerColor = flavor.primary.copy(alpha = 0.20f),
                                    selectedLabelColor = lerp(flavor.primary, Color.White, 0.42f)
                                )
                            )
                        }
                    }

                    Text(
                        "Action Model",
                        style = MaterialTheme.typography.labelMedium,
                        color = flavor.primary.copy(alpha = 0.78f)
                    )
                    FlowRow(
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                        verticalArrangement = Arrangement.spacedBy(8.dp)
                    ) {
                        actionModels.forEach { modelId ->
                            FilterChip(
                                selected = modelId == walletState.actionModelId,
                                onClick = { onSelectActionModel(modelId) },
                                label = { Text(shortModelName(modelId)) },
                                colors = FilterChipDefaults.filterChipColors(
                                    containerColor = MaterialTheme.colorScheme.surface.copy(alpha = 0.52f),
                                    labelColor = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.84f),
                                    selectedContainerColor = flavor.primary.copy(alpha = 0.20f),
                                    selectedLabelColor = lerp(flavor.primary, Color.White, 0.42f)
                                )
                            )
                        }
                    }
                }
            }

            TextButton(
                onClick = onManageKeys,
                modifier = Modifier.align(Alignment.End)
            ) {
                Icon(Icons.Default.Settings, contentDescription = null, tint = flavor.primary)
                Spacer(Modifier.width(6.dp))
                Text("Manage Keys", color = flavor.primary)
            }

            Spacer(Modifier.height(8.dp))
        }
    }
}

@Composable
private fun KeySwitchRow(
    label: String,
    provider: Provider,
    flavor: CitrosFlavor,
    maskedKey: String,
    selected: Boolean,
    onClick: () -> Unit
) {
    val accent = lerp(ProviderUi.brandColor(provider), flavor.primary, 0.42f)
    CitrosLiquidGlassSurface(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(14.dp),
        onClick = onClick,
        borderColor = if (selected) accent.copy(alpha = 0.62f) else MaterialTheme.colorScheme.outline.copy(alpha = 0.34f),
        borderWidth = if (selected) 1.3.dp else 1.dp,
        highlightColor = if (selected) accent else flavor.primary,
        warmth = if (selected) 1.10f else 0.76f,
        contentPadding = androidx.compose.foundation.layout.PaddingValues(horizontal = 12.dp, vertical = 10.dp)
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(10.dp)
        ) {
            Text(ProviderUi.icon(provider))
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    label,
                    style = MaterialTheme.typography.bodyMedium,
                    color = if (selected) accent else MaterialTheme.colorScheme.onSurface
                )
                Text(
                    maskedKey,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.7f)
                )
            }
            if (selected) {
                Box(
                    modifier = Modifier
                        .size(9.dp)
                        .background(accent, CircleShape)
                )
            }
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun ChatEmptyState(
    flavor: CitrosFlavor,
    onSuggestion: (String) -> Unit
) {
    val suggestions = listOf(
        "Set a timer for 10 minutes",
        "Open my email",
        "What's on my calendar?",
        "Take a screenshot"
    )

    Column(
        modifier = Modifier
            .fillMaxWidth(),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(12.dp)
    ) {
        Text(
            "Hey there! What can I help you with?",
            style = MaterialTheme.typography.titleMedium,
            fontWeight = FontWeight.SemiBold,
            color = flavor.primary
        )
        FlowRow(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.CenterHorizontally),
            verticalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            suggestions.forEach { suggestion ->
                CitrosLiquidGlassSurface(
                    onClick = { onSuggestion(suggestion) },
                    shape = RoundedCornerShape(999.dp),
                    borderColor = flavor.primary.copy(alpha = 0.32f),
                    borderWidth = 1.dp,
                    highlightColor = flavor.primary,
                    warmth = 0.86f,
                    contentPadding = androidx.compose.foundation.layout.PaddingValues(horizontal = 12.dp, vertical = 8.dp)
                ) {
                    Text(
                        suggestion,
                        style = MaterialTheme.typography.labelLarge,
                        color = lerp(flavor.primary, Color.White, 0.42f),
                        textAlign = TextAlign.Center
                    )
                }
            }
        }
    }
}

/** Alpha for normal user message bubble background. */
private const val USER_MESSAGE_ALPHA = 0.45f
/** Alpha for steer (mid-loop redirect) message bubble background. */
private const val STEER_MESSAGE_ALPHA = 0.22f

@Composable
internal fun PortedMessageBubble(
    message: Message,
    flavor: CitrosFlavor
) {
    val isUser = message.role == "user"
    val isSteer = isUser && message.isSteer
    val isAction = !isUser && (
        message.content.startsWith("🤖") ||
            message.content.startsWith("📱") ||
            message.content.startsWith("👁") ||
            message.content.contains("[Tools:")
        )

    val userText = lerp(flavor.primary, Color.White, 0.40f)

    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = if (isUser) Arrangement.End else Arrangement.Start
    ) {
        Column(horizontalAlignment = if (isUser) Alignment.End else Alignment.Start) {
            val bubbleShape = RoundedCornerShape(
                topStart = 16.dp,
                topEnd = 16.dp,
                bottomStart = if (isUser) 16.dp else 4.dp,
                bottomEnd = if (isUser) 4.dp else 16.dp
            )
            CitrosLiquidGlassSurface(
                shape = bubbleShape,
                baseColor = when {
                    isSteer -> flavor.tint.copy(alpha = STEER_MESSAGE_ALPHA)
                    isUser -> flavor.tint.copy(alpha = USER_MESSAGE_ALPHA)
                    isAction -> MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.50f)
                    else -> MaterialTheme.colorScheme.surface.copy(alpha = 0.42f)
                },
                borderColor = when {
                    isUser -> flavor.primary.copy(alpha = if (isSteer) 0.56f else 0.42f)
                    isAction -> flavor.primary.copy(alpha = 0.34f)
                    else -> MaterialTheme.colorScheme.outline.copy(alpha = 0.32f)
                },
                borderWidth = 1.dp,
                highlightColor = if (isUser || isAction) flavor.primary else null,
                warmth = if (isUser) 1.06f else 0.80f,
                modifier = Modifier.widthIn(max = 320.dp),
                contentPadding = androidx.compose.foundation.layout.PaddingValues(12.dp)
            ) {
                Text(
                    text = message.content,
                    color = if (isUser) userText else MaterialTheme.colorScheme.onSurface,
                    style = MaterialTheme.typography.bodyMedium
                )
            }
            if (isSteer) {
                Text(
                    text = "↗ redirected",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.6f),
                    modifier = Modifier.padding(top = 2.dp, end = 4.dp)
                )
            }
        }
    }
}

@Composable
internal fun PortedLoadingIndicator(flavor: CitrosFlavor = CitrosFlavor.TANGERINE, label: String = "Thinking") {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.Start
    ) {
        CitrosLiquidGlassSurface(
            shape = RoundedCornerShape(16.dp),
            baseColor = MaterialTheme.colorScheme.surface.copy(alpha = 0.42f),
            borderColor = flavor.primary.copy(alpha = 0.34f),
            borderWidth = 1.dp,
            highlightColor = flavor.primary,
            warmth = 0.82f,
            contentPadding = androidx.compose.foundation.layout.PaddingValues(horizontal = 14.dp, vertical = 10.dp)
        ) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(6.dp)
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
                                flavor.primary.copy(alpha = alpha),
                                CircleShape
                            )
                    )
                }
                Spacer(Modifier.width(4.dp))
                Text(
                    label,
                    style = MaterialTheme.typography.bodySmall,
                    color = flavor.primary.copy(alpha = 0.92f)
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
