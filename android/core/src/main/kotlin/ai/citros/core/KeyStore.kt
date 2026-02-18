package ai.citros.core

/**
 * Secure storage for API keys.
 *
 * This is a pure interface with no Android dependencies. The production implementation
 * (using EncryptedSharedPreferences) lives in the :chat module. For testing, use
 * [InMemoryKeyStore].
 *
 * Keys are stored by wallet entry ID (see [WalletKey.id]). The actual API key/token
 * is the value.
 */
interface KeyStore {
    /**
     * Retrieve the API key for a wallet entry.
     *
     * @param keyId The wallet entry ID ([WalletKey.id])
     * @return The raw API key/token, or null if not found
     */
    fun get(keyId: String): String?

    /**
     * Store an API key for a wallet entry.
     *
     * @param keyId The wallet entry ID ([WalletKey.id])
     * @param value The raw API key/token to store
     */
    fun put(keyId: String, value: String)

    /**
     * Remove an API key from storage.
     *
     * @param keyId The wallet entry ID ([WalletKey.id])
     */
    fun remove(keyId: String)

    /**
     * Clear all stored API keys.
     *
     * Use with caution - this wipes all credentials.
     */
    fun clear()
}
