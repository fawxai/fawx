package ai.citros.core

import kotlinx.serialization.Serializable

/**
 * A stored credential in the key wallet.
 *
 * The actual API key is stored separately in [KeyStore] and retrieved by [id].
 * This object contains only metadata about the key.
 *
 * @param id Unique identifier (UUID) for this wallet entry
 * @param provider The API provider this key is for (Anthropic, OpenRouter, OpenAI)
 * @param label User-editable name for this key (e.g., "Anthropic Personal", "Work Account")
 * @param addedAt Timestamp when this key was added (epoch milliseconds)
 * @param expiresAt Optional expiry timestamp (epoch milliseconds). Null for non-expiring API keys.
 *   Used for OAuth tokens or time-limited credentials. When set, the key should be
 *   refreshed or replaced before this time. Currently informational — no automatic
 *   refresh is performed.
 */
@Serializable
data class WalletKey(
    val id: String,
    val provider: Provider,
    val label: String,
    val addedAt: Long,
    val expiresAt: Long? = null
)
