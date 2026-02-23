package ai.citros.chat

import android.content.Context
import ai.citros.core.WalletManager

/**
 * Shared test fakes used across multiple test classes.
 * Extracted to reduce duplication between ChatViewModelTest and QuickSwitcherTest.
 */

/** In-memory implementation of [ai.citros.core.KeyStore] for testing. */
internal class InMemoryKeyStore : ai.citros.core.KeyStore {
    private val store = mutableMapOf<String, String>()

    override fun get(keyId: String): String? = store[keyId]

    override fun put(keyId: String, value: String) {
        store[keyId] = value
    }

    override fun remove(keyId: String) {
        store.remove(keyId)
    }

    override fun clear() {
        store.clear()
    }
}

/** In-memory implementation of [CredentialStore] for testing. */
internal class InMemoryCredentialStore : CredentialStore {
    private val store = mutableMapOf<String, String>()

    override fun getString(key: String): String? = store[key]

    override fun putString(key: String, value: String) {
        store[key] = value
    }

    override fun remove(key: String) {
        store.remove(key)
    }
}

/** Wallet dependencies for Compose/Robolectric tests that should avoid AndroidKeyStore. */
internal fun createTestWalletDependencies(context: Context): WalletDependencies {
    val appContext = context.applicationContext
    val keyStore = InMemoryKeyStore()
    val walletStorage = SharedPreferencesWalletStorage(appContext)
    val walletManager = WalletManager(walletStorage, keyStore)
    return WalletDependencies(
        keyStore = keyStore,
        walletStorage = walletStorage,
        walletManager = walletManager
    )
}
