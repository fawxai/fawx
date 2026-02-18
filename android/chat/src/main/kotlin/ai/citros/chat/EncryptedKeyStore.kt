package ai.citros.chat

import android.content.Context
import android.content.SharedPreferences
import android.os.Looper
import androidx.security.crypto.EncryptedSharedPreferences
import androidx.security.crypto.MasterKey
import ai.citros.core.KeyStore

/**
 * Production KeyStore implementation using Android EncryptedSharedPreferences.
 *
 * Stores API keys encrypted at rest using AES256 encryption with keys stored in the
 * Android Keystore system. This provides hardware-backed encryption on supported devices.
 *
 * **Security Properties:**
 * - Keys are encrypted at rest (AES256-GCM)
 * - Key encryption keys (KEK) stored in Android Keystore
 * - Hardware-backed encryption on devices with Secure Element/TEE
 * - Keys are scoped to this app only (OS-enforced sandboxing)
 *
 * **Thread Safety:** Not thread-safe. All calls must be from the main thread.
 * Thread violations will throw IllegalStateException.
 *
 * @param context Android context for accessing SharedPreferences
 */
class EncryptedKeyStore(context: Context) : KeyStore {
    private val masterKey = MasterKey.Builder(context)
        .setKeyScheme(MasterKey.KeyScheme.AES256_GCM)
        .build()

    private val prefs: SharedPreferences = EncryptedSharedPreferences.create(
        context,
        "citros_keystore",
        masterKey,
        EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
        EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM
    )

    private fun assertMainThread() {
        check(Looper.getMainLooper().isCurrentThread) {
            "EncryptedKeyStore must be accessed from the main thread"
        }
    }

    override fun get(keyId: String): String? {
        assertMainThread()
        return prefs.getString(keyId, null)
    }

    override fun put(keyId: String, value: String) {
        assertMainThread()
        prefs.edit().putString(keyId, value).apply()
    }

    override fun remove(keyId: String) {
        assertMainThread()
        prefs.edit().remove(keyId).apply()
    }

    override fun clear() {
        assertMainThread()
        prefs.edit().clear().apply()
    }
}
