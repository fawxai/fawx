package ai.citros.core

import android.util.Log
import java.util.UUID

/**
 * Persistent storage for wallet state.
 *
 * This is an interface to allow different storage backends (SharedPreferences,
 * in-memory for tests, etc.).
 */
interface WalletStorage {
    /**
     * Load the wallet state from storage.
     *
     * @return The saved state, or null if no state has been saved yet
     */
    fun loadState(): WalletState?

    /**
     * Save the wallet state to storage.
     *
     * @param state The state to save
     */
    fun saveState(state: WalletState)
}

/**
 * Manager for wallet key storage, selection, and migration.
 *
 * Coordinates between [WalletStorage] (for metadata) and [KeyStore] (for raw API keys).
 *
 * All mutating operations automatically persist state to storage.
 *
 * **Thread Safety:** All public methods are synchronized. Safe to call from multiple
 * threads (e.g., UI thread for settings + IO thread for agent loop).
 *
 * **Atomicity:** State metadata is written before raw keys to prevent orphaned
 * credentials in [KeyStore]. On load, entries referencing missing keys are cleaned up.
 *
 * @param storage Backend for persisting [WalletState]
 * @param keyStore Backend for securely storing raw API keys
 */
class WalletManager(
    private val storage: WalletStorage,
    private val keyStore: KeyStore
) {
    companion object {
        private const val TAG = "WalletManager"

        /**
         * Default label for keys imported from legacy single-token storage.
         */
        private const val LEGACY_MIGRATION_LABEL = "Imported Key"
    }

    /**
     * Load the current wallet state, or return a default empty state if none exists.
     *
     * Also performs orphan cleanup: if any [WalletKey] entries reference IDs that
     * are missing from [KeyStore] (e.g., because keyStore.put() failed after
     * saveState() succeeded), those entries are removed and the cleaned state
     * is persisted.
     *
     * @return The wallet state (either loaded, cleaned, or default)
     */
    @Synchronized
    fun loadOrDefault(): WalletState {
        val state = storage.loadState() ?: WalletState(
            keys = emptyList(),
            activeKeyId = null,
            chatModelId = ModelConfig.CHAT_MODEL,
            actionModelId = ModelConfig.ACTION_MODEL
        )

        // Orphan cleanup: remove entries referencing keys missing from KeyStore.
        // This handles the case where saveState() succeeded but keyStore.put() failed.
        val validKeys = state.keys.filter {
            try {
                keyStore.get(it.id) != null
            } catch (e: Exception) {
                Log.w(TAG, "KeyStore access failed for ${it.id}, treating as orphaned: ${e.message}")
                false
            }
        }
        if (validKeys.size != state.keys.size) {
            val cleaned = state.copy(
                keys = validKeys,
                activeKeyId = if (validKeys.any { it.id == state.activeKeyId }) state.activeKeyId else null
            )
            storage.saveState(cleaned)
            return cleaned
        }

        return state
    }

    /**
     * Add a new API key to the wallet.
     *
     * Generates a unique ID, adds metadata to state, then stores the raw key in [KeyStore].
     * State is written first so that if keyStore.put() fails, the orphaned metadata entry
     * will be cleaned up on the next [loadOrDefault] call. This prevents invisible orphaned
     * credentials in KeyStore that can never be removed.
     *
     * @param provider The API provider (or will be auto-detected from [rawKey] if null)
     * @param label User-facing name for this key (e.g., "Personal Account")
     * @param rawKey The actual API key/token
     * @return The created [WalletKey] metadata
     * @throws IllegalArgumentException if rawKey or label is blank
     */
    @Synchronized
    fun addKey(provider: Provider, label: String, rawKey: String): WalletKey {
        require(rawKey.isNotBlank()) { "API key cannot be blank" }
        require(label.isNotBlank()) { "Label cannot be blank" }

        val id = UUID.randomUUID().toString()
        val now = System.currentTimeMillis()

        val walletKey = WalletKey(
            id = id,
            provider = provider,
            label = label,
            addedAt = now
        )

        // Save state FIRST (metadata), then store raw key.
        // If keyStore.put() fails, loadOrDefault() will clean up the orphaned entry.
        // Reverse order would leave an orphaned key in KeyStore with no state reference.
        val currentState = loadOrDefault()
        val updatedState = currentState.copy(
            keys = currentState.keys + walletKey
        )
        storage.saveState(updatedState)
        keyStore.put(id, rawKey)

        return walletKey
    }

    /**
     * Remove a key from the wallet.
     *
     * Removes both the metadata (from state) and the raw key (from [KeyStore]).
     * If removing the currently active key, clears [WalletState.activeKeyId].
     *
     * @param keyId The wallet entry ID to remove
     */
    @Synchronized
    fun removeKey(keyId: String) {
        val currentState = loadOrDefault()

        // Remove from state
        val updatedKeys = currentState.keys.filter { it.id != keyId }
        val updatedActiveKeyId = if (currentState.activeKeyId == keyId) null else currentState.activeKeyId

        val updatedState = currentState.copy(
            keys = updatedKeys,
            activeKeyId = updatedActiveKeyId
        )
        storage.saveState(updatedState)

        // Remove raw key
        keyStore.remove(keyId)
    }

    /**
     * Set the active key for API calls.
     *
     * @param keyId The wallet entry ID to activate
     * @throws IllegalArgumentException if keyId is not found in the wallet
     */
    @Synchronized
    fun setActiveKey(keyId: String) {
        val currentState = loadOrDefault()
        require(currentState.keys.any { it.id == keyId }) {
            "Key ID $keyId not found in wallet"
        }
        val updatedState = currentState.copy(activeKeyId = keyId)
        storage.saveState(updatedState)
    }

    /**
     * Change the chat model selection.
     *
     * Model IDs are not validated against the catalog to allow flexibility for new models
     * that may be released by providers. Invalid model IDs will fail at API call time with
     * a provider-specific error.
     *
     * @param modelId The model ID to use for chat (provider-specific format)
     */
    @Synchronized
    fun setChatModel(modelId: String) {
        val currentState = loadOrDefault()
        val updatedState = currentState.copy(chatModelId = modelId)
        storage.saveState(updatedState)
    }

    /**
     * Change the action model selection.
     *
     * Model IDs are not validated against the catalog to allow flexibility for new models
     * that may be released by providers. Invalid model IDs will fail at API call time with
     * a provider-specific error.
     *
     * @param modelId The model ID to use for action loop (provider-specific format)
     */
    @Synchronized
    fun setActionModel(modelId: String) {
        val currentState = loadOrDefault()
        val updatedState = currentState.copy(actionModelId = modelId)
        storage.saveState(updatedState)
    }

    /**
     * Get the currently active provider configuration.
     *
     * Convenience method that delegates to [WalletState.activeConfig].
     *
     * @return The active [ProviderConfig], or null if no key is active or configured
     */
    @Synchronized
    fun activeConfig(): ProviderConfig? {
        return loadOrDefault().activeConfig(keyStore)
    }

    /**
     * Migrate from legacy single-token storage to the wallet system.
     *
     * Creates a wallet entry from the old format, sets it as active, and configures
     * default models for the provider.
     *
     * **Idempotent:** If the wallet already contains keys (i.e., migration was already
     * performed), returns the first existing key without creating duplicates.
     *
     * **Model selection:** Overwrites any existing model selections with provider defaults.
     * Does not preserve user's custom model choices from legacy settings.
     *
     * @param token The legacy API key/token
     * @param provider The provider type (if known), or null to auto-detect from [token]
     * @param authKind Legacy auth kind hint ("oauth", "api_key", etc.). If "oauth",
     *   suggests OpenAI provider for auto-detection.
     * @return The created wallet entry, or existing first key if already migrated
     * @throws IllegalArgumentException if token is blank (via [addKey])
     */
    @Synchronized
    fun migrateFromLegacy(token: String, provider: Provider?, authKind: String?): WalletKey {
        // Idempotency guard: skip if wallet already has keys (migration already ran)
        val existingState = loadOrDefault()
        if (existingState.keys.isNotEmpty()) {
            val existing = existingState.keys.first()
            if (provider != null && existing.provider != provider) {
                Log.w(TAG, "migrateFromLegacy skipped: wallet non-empty. " +
                    "Requested provider=$provider but existing key uses ${existing.provider}")
            }
            return existing
        }

        // Determine provider (explicit, auth kind hint, or auto-detect)
        val detectedProvider = provider
            ?: if (authKind == "oauth") Provider.OPENAI else null
            ?: ProviderConfig.detectProvider(token)
            ?: Provider.OPENAI  // Fallback to OpenAI if can't detect

        // Add key with generic label
        val walletKey = addKey(detectedProvider, LEGACY_MIGRATION_LABEL, token)

        // Set as active
        setActiveKey(walletKey.id)

        // Set default models for the provider
        setChatModel(ModelConfig.defaultChatModel(detectedProvider))
        setActionModel(ModelConfig.defaultActionModel(detectedProvider))

        return walletKey
    }
}
