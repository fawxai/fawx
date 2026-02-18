package ai.citros.chat

import android.content.Context
import androidx.compose.runtime.staticCompositionLocalOf
import ai.citros.core.KeyStore
import ai.citros.core.WalletManager
import ai.citros.core.WalletStorage

/**
 * Shared wallet dependency container for ChatActivity navigation graph.
 *
 * Keeps key-store, wallet storage, and wallet manager scoped together so onboarding,
 * chat, and settings operate on the same wallet state source.
 */
internal data class WalletDependencies(
    val keyStore: KeyStore,
    val walletStorage: WalletStorage,
    val walletManager: WalletManager
)

internal val LocalWalletDependencies = staticCompositionLocalOf<WalletDependencies?> { null }

internal fun provideWalletDependencies(context: Context): WalletDependencies {
    val appContext = context.applicationContext
    val keyStore = EncryptedKeyStore(appContext)
    val walletStorage = SharedPreferencesWalletStorage(appContext)
    val walletManager = WalletManager(walletStorage, keyStore)
    return WalletDependencies(keyStore, walletStorage, walletManager)
}
