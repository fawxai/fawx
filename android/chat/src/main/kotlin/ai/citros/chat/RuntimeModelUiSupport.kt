package ai.citros.chat

import android.util.Log
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import ai.citros.core.ModelCatalog
import ai.citros.core.ModelConfig
import ai.citros.core.Provider
import ai.citros.core.ProviderConfig

internal data class RuntimeModelSelectionCorrection(
    val chatModelId: String,
    val actionModelId: String,
    val notices: List<String>
)

internal fun computeRuntimeModelSelectionCorrection(
    provider: Provider,
    selectedChatModelId: String,
    selectedActionModelId: String,
    chatModels: List<String>,
    actionModels: List<String>
): RuntimeModelSelectionCorrection {
    var chatModelId = selectedChatModelId
    var actionModelId = selectedActionModelId
    val notices = mutableListOf<String>()

    if (chatModels.isNotEmpty() && chatModelId !in chatModels) {
        val fallbackChat = ModelConfig.defaultChatModel(provider)
            .takeIf { it in chatModels }
            ?: chatModels.first()
        chatModelId = fallbackChat
        notices += "Chat model unavailable for current provider; switched to $fallbackChat"
    }

    val actionModelInvalid = actionModels.isNotEmpty() && (
        actionModelId !in actionModels || !ModelConfig.isModelAboveFloor(provider, actionModelId)
        )
    if (actionModelInvalid) {
        val fallbackAction = ModelConfig.defaultActionModel(provider)
            .takeIf { it in actionModels }
            ?: actionModels.first()
        actionModelId = fallbackAction
        notices += "Action model unavailable for current provider; switched to $fallbackAction"
    }

    return RuntimeModelSelectionCorrection(
        chatModelId = chatModelId,
        actionModelId = actionModelId,
        notices = notices
    )
}

@Composable
internal fun rememberModelCatalogRefreshTick(
    activeConfig: ProviderConfig?,
    extraKey: Any? = null,
    logTag: String
): Int {
    var modelCatalogRefreshTick by remember(activeConfig?.provider, activeConfig?.headers?.hashCode(), extraKey) {
        mutableIntStateOf(0)
    }

    LaunchedEffect(activeConfig?.provider, activeConfig?.headers?.hashCode(), extraKey) {
        val config = activeConfig ?: return@LaunchedEffect
        runCatching { ModelCatalog.getModels(config) }
            .onFailure {
                Log.w(logTag, "Model catalog refresh failed for ${config.provider}: ${it.message}")
            }
        modelCatalogRefreshTick += 1
    }

    return modelCatalogRefreshTick
}
