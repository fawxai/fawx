package ai.citros.chat

import android.content.Context
import ai.citros.core.WalletState
import ai.citros.core.WalletStorage
import kotlinx.serialization.json.Json
import kotlinx.serialization.encodeToString
import kotlinx.serialization.decodeFromString

/**
 * Production WalletStorage implementation using Android SharedPreferences.
 *
 * Stores wallet metadata (key IDs, labels, model selections) as JSON in SharedPreferences.
 * Does NOT store raw API keys - those live in [EncryptedKeyStore].
 *
 * **Format:** Single JSON string under key "wallet_state"
 *
 * **Backup/Recovery:** On each successful save, the previous state is preserved as
 * "wallet_state_backup". If the primary state fails to deserialize (corruption, schema
 * change), the backup is attempted before returning null. On successful recovery,
 * the backup is promoted to primary so subsequent reads don't hit the fallback path.
 *
 * **Thread Safety:** Callers MUST serialize access to this instance. This class does not
 * perform internal synchronization. [WalletManager] provides synchronized access via
 * `@Synchronized`. Direct usage without external locking risks backup corruption from
 * concurrent writes (Thread A and Thread B could both read the same "current" state as
 * backup, causing one thread's previous state to be lost). Writes use
 * [SharedPreferences.Editor.commit] (synchronous) to guarantee immediate visibility.
 *
 * @param context Android context for accessing SharedPreferences
 */
/**
 * **Security Note:** Wallet metadata (key IDs, labels, model selections) is stored in
 * plaintext SharedPreferences. This is acceptable because:
 * - Raw API keys are stored separately in [EncryptedKeyStore] (AES256-GCM)
 * - Key IDs are UUIDs with no intrinsic value without the encrypted key material
 * - MODE_PRIVATE enforces OS-level app sandboxing on non-rooted devices
 * - On rooted devices (e.g. development Pixel), root processes can read all app data
 *   regardless of encryption — this is a known and accepted trade-off for the target hardware
 */
class SharedPreferencesWalletStorage(context: Context) : WalletStorage {
    private val prefs = context.getSharedPreferences("citros_wallet", Context.MODE_PRIVATE)

    companion object {
        private const val KEY_STATE = "wallet_state"
        private const val KEY_BACKUP = "wallet_state_backup"
        private const val TAG = "WalletStorage"
    }

    override fun loadState(): WalletState? {
        val json = prefs.getString(KEY_STATE, null) ?: return null
        return try {
            Json.decodeFromString<WalletState>(json)
        } catch (e: Exception) {
            android.util.Log.e(TAG, "Failed to deserialize wallet state: ${e.message}", e)
            // Primary state corrupted — try backup
            loadBackup()
        }
    }

    override fun saveState(state: WalletState) {
        // Preserve current state as backup before overwriting
        val currentJson = prefs.getString(KEY_STATE, null)
        val editor = prefs.edit()
        if (currentJson != null) {
            editor.putString(KEY_BACKUP, currentJson)
        }
        editor.putString(KEY_STATE, Json.encodeToString(state))
        // Use commit() (synchronous) to guarantee immediate visibility.
        // WalletManager's @Synchronized ensures only one thread writes at a time.
        editor.commit()
    }

    /**
     * Attempt to load wallet state from the backup key.
     * On success, promotes backup to primary so future reads succeed directly.
     * Returns null if backup is also missing or corrupted.
     */
    private fun loadBackup(): WalletState? {
        val backupJson = prefs.getString(KEY_BACKUP, null) ?: return null
        return try {
            val state = Json.decodeFromString<WalletState>(backupJson)
            android.util.Log.w(TAG, "Recovered wallet state from backup")
            // Promote backup to primary so subsequent reads don't need fallback
            prefs.edit().putString(KEY_STATE, backupJson).commit()
            state
        } catch (e: Exception) {
            android.util.Log.e(TAG, "Backup also corrupted: ${e.message}", e)
            null
        }
    }
}
