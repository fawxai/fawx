package ai.citros.core

import kotlinx.serialization.Serializable

/**
 * Active wallet configuration.
 *
 * Contains the list of available keys, the currently active key selection,
 * and the selected model IDs for chat and action modes.
 *
 * @param keys All stored wallet entries (metadata only, not actual API keys)
 * @param activeKeyId The ID of the currently active key, or null if none selected
 * @param chatModelId Model ID to use for high-capability chat
 * @param actionModelId Model ID to use for fast action loop iterations
 */
@Serializable
data class WalletState(
    val keys: List<WalletKey>,
    val activeKeyId: String?,
    val chatModelId: String,
    val actionModelId: String
) {
    /**
     * Resolve the active wallet configuration into a [ProviderConfig].
     *
     * This combines:
     * - The active key's provider and raw API key (from [KeyStore])
     * - The selected chat and action model IDs from this state
     *
     * **Null return scenarios:**
     * Returns `null` in three distinct cases (callers cannot distinguish between them):
     * 1. No active key selected ([activeKeyId] is null) — normal state before user selects a key
     * 2. Active key not found in [keys] — inconsistent state, possible data corruption
     * 3. Raw key not found in [keyStore] — missing credential, possible KeyStore corruption
     *
     * TODO(Phase 2): Replace with `Result<ProviderConfig>` to provide specific error types
     * for better error handling and user messaging.
     *
     * @param keyStore Storage for retrieving the actual API key
     * @return A complete [ProviderConfig] ready to use, or null if unable to resolve
     */
    fun activeConfig(keyStore: KeyStore): ProviderConfig? {
        val keyId = activeKeyId ?: return null
        val walletKey = keys.find { it.id == keyId } ?: return null
        val rawKey = keyStore.get(keyId) ?: return null

        return when (walletKey.provider) {
            Provider.ANTHROPIC -> ProviderConfig(
                provider = Provider.ANTHROPIC,
                baseUrl = "https://api.anthropic.com/v1/messages",
                chatModelId = chatModelId,
                actionModelId = actionModelId,
                headers = mapOf(
                    "x-api-key" to rawKey,
                    "anthropic-version" to ProviderConfig.ANTHROPIC_API_VERSION,
                    "anthropic-beta" to ProviderConfig.ANTHROPIC_PROMPT_CACHING_BETA
                )
            )
            Provider.OPENROUTER -> ProviderConfig(
                provider = Provider.OPENROUTER,
                baseUrl = "https://openrouter.ai/api/v1/chat/completions",
                chatModelId = chatModelId,
                actionModelId = actionModelId,
                headers = mapOf(
                    "Authorization" to "Bearer $rawKey",
                    "HTTP-Referer" to "https://citros.ai",
                    "X-Title" to "Citros"
                )
            )
            Provider.OPENAI -> ProviderConfig(
                provider = Provider.OPENAI,
                baseUrl = "https://api.openai.com/v1/chat/completions",
                chatModelId = chatModelId,
                actionModelId = actionModelId,
                headers = mapOf(
                    "Authorization" to "Bearer $rawKey"
                )
            )
        }
    }
}
