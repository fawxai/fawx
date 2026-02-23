package ai.citros.chat
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import ai.citros.core.ModelConfig
import ai.citros.core.Provider
import ai.citros.core.WalletKey
import ai.citros.core.WalletState
private val QuickSpacingXs = 8.dp
private val QuickSpacingSm = 12.dp
private val QuickSpacingMd = 16.dp
// Row content in the key list is designed around a 56.dp tap target.
private val QuickSwitcherKeyRowHeight = 56.dp
private val QuickSwitcherMaxVisibleKeyRows = 4
private val QuickSwitcherMaxSheetHeight = 560.dp
internal fun abbreviatedModelName(modelId: String): String {
    val normalized = modelId
        .removeSuffix("-latest")
        .substringAfterLast('/')
    val normalizedVersion = normalized.replace('.', '-')
    return when {
        normalizedVersion.contains("claude-sonnet-4-5") -> "Sonnet 4.5"
        normalizedVersion.contains("claude-haiku-4-5") -> "Haiku 4.5"
        normalizedVersion.contains("claude-opus-4") -> "Opus 4"
        normalized.startsWith("gpt-") -> normalized.uppercase().replace("-", " ")
        else -> normalized.replace("-", " ").replaceFirstChar { it.uppercase() }
    }
}
@Composable
internal fun QuickSwitcherToolbarChip(
    provider: Provider,
    chatModelId: String,
    onClick: () -> Unit
) {
    val modelLabel = abbreviatedModelName(chatModelId)
    Surface(
        onClick = onClick,
        shape = RoundedCornerShape(999.dp),
        color = CitrosColorScheme.surfaceVariant,
        tonalElevation = 2.dp,
        modifier = Modifier.semantics {
            contentDescription = "Quick switcher. Provider ${provider.name}. Chat model $modelLabel"
        }
    ) {
        Row(
            modifier = Modifier.padding(horizontal = 10.dp, vertical = 6.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Text(ProviderUi.icon(provider))
            Spacer(Modifier.width(6.dp))
            Text(
                modelLabel,
                style = CitrosTypography.labelLarge,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis
            )
        }
    }
}
@Composable
internal fun QuickSwitcherBottomSheet(
    walletState: WalletState,
    onDismiss: () -> Unit,
    onSelectKey: (WalletKey) -> Unit,
    onSelectChatModel: (String) -> Unit,
    onSelectActionModel: (String) -> Unit,
    onManageKeys: () -> Unit
) {
    val activeKey = walletState.keys.find { it.id == walletState.activeKeyId }
    val provider = activeKey?.provider
    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(max = QuickSwitcherMaxSheetHeight)
                .verticalScroll(rememberScrollState())
                .padding(horizontal = QuickSpacingMd)
                .padding(bottom = QuickSpacingMd)
        ) {
            Text("Quick Switcher", style = CitrosTypography.titleLarge)
            Spacer(Modifier.height(QuickSpacingSm))
            Text("API Keys", style = CitrosTypography.titleMedium)
            Spacer(Modifier.height(QuickSpacingXs))
            LazyColumn(
                verticalArrangement = Arrangement.spacedBy(QuickSpacingXs),
                modifier = Modifier.height(
                    QuickSwitcherKeyRowHeight * walletState.keys.size.coerceAtMost(QuickSwitcherMaxVisibleKeyRows)
                )
            ) {
                items(walletState.keys, key = { it.id }) { key ->
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .clickable { onSelectKey(key) },
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        RadioButton(
                            selected = key.id == walletState.activeKeyId,
                            onClick = { onSelectKey(key) }
                        )
                        Text(
                            ProviderUi.icon(key.provider),
                            modifier = Modifier.semantics {
                                contentDescription = "Provider ${key.provider.name}"
                            }
                        )
                        Spacer(Modifier.width(QuickSpacingXs))
                        Text(key.label, modifier = Modifier.weight(1f))
                        Text(
                            if (key.id == walletState.activeKeyId) "Active" else "Available",
                            style = CitrosTypography.labelSmall,
                            color = CitrosColorScheme.onSurfaceVariant
                        )
                    }
                }
            }
            if (provider != null) {
                Spacer(Modifier.height(QuickSpacingSm))
                Text("Chat Model", style = CitrosTypography.titleSmall)
                Spacer(Modifier.height(QuickSpacingXs))
                Row(horizontalArrangement = Arrangement.spacedBy(QuickSpacingXs)) {
                    ModelConfig.runtimeChatModels(provider).forEach { model ->
                        FilterChip(
                            selected = model == walletState.chatModelId,
                            onClick = { onSelectChatModel(model) },
                            modifier = Modifier.testTag("quick_switcher_chat_model_$model"),
                            label = { Text(abbreviatedModelName(model)) }
                        )
                    }
                }
                Spacer(Modifier.height(QuickSpacingSm))
                Text("Action Model", style = CitrosTypography.titleSmall)
                Spacer(Modifier.height(QuickSpacingXs))
                Row(horizontalArrangement = Arrangement.spacedBy(QuickSpacingXs)) {
                    ModelConfig.runtimeActionModels(provider).forEach { model ->
                        FilterChip(
                            selected = model == walletState.actionModelId,
                            onClick = { onSelectActionModel(model) },
                            modifier = Modifier.testTag("quick_switcher_action_model_$model"),
                            label = { Text(abbreviatedModelName(model)) }
                        )
                    }
                }
            }
            Spacer(Modifier.height(QuickSpacingSm))
            TextButton(
                onClick = onManageKeys,
                modifier = Modifier.testTag("quick_switcher_manage_keys")
            ) {
                Text("Manage Keys")
            }
        }
    }
}
