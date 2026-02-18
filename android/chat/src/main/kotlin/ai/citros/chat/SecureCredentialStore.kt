package ai.citros.chat

import android.content.Context
import android.content.SharedPreferences
import android.util.Log
import androidx.security.crypto.EncryptedSharedPreferences
import androidx.security.crypto.MasterKey

private const val SECURE_PREFS = "citros_secure"

/**
 * Interface for credential storage, enabling test substitution.
 */
interface CredentialStore {
    fun getString(key: String): String?
    fun putString(key: String, value: String)
    fun remove(key: String)
}

/**
 * Stores sensitive credentials using EncryptedSharedPreferences.
 *
 * @param context Any context. This class immediately extracts `context.applicationContext`
 * and only uses that instance internally to avoid leaking activity contexts.
 */
class SecureCredentialStore(context: Context) : CredentialStore {
    private val appContext = context.applicationContext

    private val prefs: SharedPreferences = runCatching {
        val masterKey = MasterKey.Builder(appContext)
            .setKeyScheme(MasterKey.KeyScheme.AES256_GCM)
            .build()

        EncryptedSharedPreferences.create(
            appContext,
            SECURE_PREFS,
            masterKey,
            EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
            EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM
        )
    }.getOrElse { error ->
        Log.e("SecureCredentialStore", "Failed to initialize encrypted storage", error)
        throw SecurityException(
            "Secure storage is unavailable on this device. Please restart the app and try again.",
            error
        )
    }

    override fun getString(key: String): String? = prefs.getString(key, null)

    override fun putString(key: String, value: String) {
        prefs.edit().putString(key, value).apply()
    }

    override fun remove(key: String) {
        prefs.edit().remove(key).apply()
    }
}
