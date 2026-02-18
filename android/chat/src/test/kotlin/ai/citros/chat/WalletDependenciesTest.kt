package ai.citros.chat

import ai.citros.core.WalletManager
import org.junit.Test
import kotlin.test.assertFailsWith
import kotlin.test.assertSame

class WalletDependenciesTest {

    @Test
    fun `resolveWalletScope reuses shared wallet manager when overrides are absent`() {
        val keyStore = InMemoryKeyStore()
        val storage = InMemoryWalletStorage()
        val sharedManager = WalletManager(storage, keyStore)
        val deps = WalletDependencies(
            keyStore = keyStore,
            walletStorage = storage,
            walletManager = sharedManager
        )

        val resolved = resolveWalletScope(
            scopedWalletDependencies = deps,
            keyStoreOverride = null,
            walletStorageOverride = null
        )

        assertSame(sharedManager, resolved.walletManager)
        assertSame(keyStore, resolved.keyStore)
        assertSame(storage, resolved.walletStorage)
    }

    @Test
    fun `resolveWalletScope throws without dependencies and without overrides`() {
        assertFailsWith<IllegalStateException> {
            resolveWalletScope(
                scopedWalletDependencies = null,
                keyStoreOverride = null,
                walletStorageOverride = null
            )
        }
    }

    private class InMemoryWalletStorage : ai.citros.core.WalletStorage {
        private var state: ai.citros.core.WalletState? = null

        override fun loadState(): ai.citros.core.WalletState? = state

        override fun saveState(state: ai.citros.core.WalletState) {
            this.state = state
        }
    }

    private class InMemoryKeyStore : ai.citros.core.KeyStore {
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
}
