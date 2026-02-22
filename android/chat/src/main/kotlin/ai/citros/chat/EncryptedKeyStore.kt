package ai.citros.chat

import android.content.Context
import android.content.SharedPreferences
import android.os.Build
import android.os.Looper
import android.util.Log
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
 * **Test behavior:** Robolectric does not provide a functional AndroidKeyStore provider.
 * In that environment only, this class falls back to regular SharedPreferences so
 * ChatActivity/Onboarding compose tests can initialize wallet dependencies.
 */
class EncryptedKeyStore(context: Context) : KeyStore {
    companion object {
        private const val TAG = "EncryptedKeyStore"
        private const val ENCRYPTED_PREFS_NAME = "citros_keystore"
        private const val ROBOLECTRIC_FALLBACK_PREFS_NAME = "citros_keystore_robolectric"
    }

    private val appContext = context.applicationContext

    private val prefs: SharedPreferences = createPreferences(appContext)

    private fun createPreferences(context: Context): SharedPreferences {
        return try {
            val masterKey = MasterKey.Builder(context)
                .setKeyScheme(MasterKey.KeyScheme.AES256_GCM)
                .build()

            EncryptedSharedPreferences.create(
                context,
                ENCRYPTED_PREFS_NAME,
                masterKey,
                EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
                EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM
            )
        } catch (e: Exception) {
            if (isRobolectricEnvironment()) {
                Log.w(
                    TAG,
                    "AndroidKeyStore unavailable under Robolectric; using plaintext SharedPreferences fallback for tests",
                    e
                )
                context.getSharedPreferences(ROBOLECTRIC_FALLBACK_PREFS_NAME, Context.MODE_PRIVATE)
            } else {
                throw e
            }
        }
    }

    private fun isRobolectricEnvironment(): Boolean {
        val fingerprint = Build.FINGERPRINT?.lowercase() ?: ""
        val model = Build.MODEL?.lowercase() ?: ""
        return fingerprint.contains("robolectric") ||
            model.contains("robolectric") ||
            System.getProperty("robolectric") != null
    }

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
