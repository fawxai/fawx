package ai.citros.chat

import android.content.Context
import android.content.SharedPreferences
import android.os.Looper
import android.util.Log
import androidx.annotation.VisibleForTesting
import androidx.security.crypto.EncryptedSharedPreferences
import androidx.security.crypto.MasterKey
import ai.citros.core.KeyStore
import java.security.GeneralSecurityException

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
 * **Test behavior:** Plaintext fallback is disabled by default. Robolectric tests
 * must opt in via constructor wiring with [allowPlaintextFallbackForTests].
 */
class EncryptedKeyStore @VisibleForTesting constructor(
    context: Context,
    private val allowPlaintextFallbackForTests: Boolean = false,
    private val encryptedPrefsFactory: (Context) -> SharedPreferences = { ctx -> createEncryptedPreferences(ctx) },
    private val isRobolectricRuntime: () -> Boolean = { detectRobolectricRuntime() }
) : KeyStore {
    companion object {
        private const val TAG = "EncryptedKeyStore"
        private const val ENCRYPTED_PREFS_NAME = "citros_keystore"
        private const val ROBOLECTRIC_FALLBACK_PREFS_NAME = "citros_keystore_robolectric"

        private fun createEncryptedPreferences(context: Context): SharedPreferences {
            val masterKey = MasterKey.Builder(context)
                .setKeyScheme(MasterKey.KeyScheme.AES256_GCM)
                .build()

            return EncryptedSharedPreferences.create(
                context,
                ENCRYPTED_PREFS_NAME,
                masterKey,
                EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
                EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM
            )
        }

        private fun detectRobolectricRuntime(): Boolean {
            return runCatching {
                Class.forName("org.robolectric.RuntimeEnvironment")
            }.isSuccess
        }
    }

    private val appContext = context.applicationContext

    private val prefs: SharedPreferences = createPreferences(appContext)

    private fun createPreferences(context: Context): SharedPreferences {
        return try {
            encryptedPrefsFactory(context)
        } catch (error: GeneralSecurityException) {
            fallbackToPlaintextForTestsOrThrow(context, error)
        } catch (error: IllegalStateException) {
            fallbackToPlaintextForTestsOrThrow(context, error)
        }
    }

    private fun fallbackToPlaintextForTestsOrThrow(
        context: Context,
        error: Throwable
    ): SharedPreferences {
        if (!(allowPlaintextFallbackForTests && isRobolectricRuntime())) {
            throw error
        }
        Log.w(
            TAG,
            "EncryptedKeyStore unavailable under Robolectric; using plaintext SharedPreferences fallback for tests",
            error
        )
        return context.getSharedPreferences(ROBOLECTRIC_FALLBACK_PREFS_NAME, Context.MODE_PRIVATE)
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
