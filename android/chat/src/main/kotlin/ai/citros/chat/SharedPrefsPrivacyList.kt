package ai.citros.chat

import android.content.Context
import android.content.SharedPreferences
import android.os.Looper
import android.util.Log
import ai.citros.core.PrivacyList
import java.util.WeakHashMap

/**
 * Production privacy list backed by shared preferences.
 *
 * Writes are synchronous (`commit`) for durability. Callers should avoid invoking
 * [add] / [remove] from the main thread to prevent UI jank.
 */
class SharedPrefsPrivacyList internal constructor(
    private val prefs: SharedPreferences,
    private val isMainThread: () -> Boolean,
    private val isDebugBuild: () -> Boolean
) : PrivacyList {
    private val prefsLock: Any = lockForPrefs(prefs)

    constructor(context: Context) : this(
        prefs = context.getSharedPreferences(CITROS_PREFS, Context.MODE_PRIVATE),
        isMainThread = { Looper.getMainLooper().thread == Thread.currentThread() },
        isDebugBuild = { BuildConfig.DEBUG }
    )


    override fun isPrivate(packageName: String): Boolean {
        val normalized = packageName.trim()
        if (normalized.isEmpty()) return false
        return synchronized(prefsLock) {
            val current = prefs.getStringSet(KEY_PRIVACY_APP_LIST, emptySet()) ?: emptySet()
            current.contains(normalized)
        }
    }

    override fun getAll(): Set<String> = synchronized(prefsLock) {
        // Return an immutable snapshot to avoid callers mutating the SharedPreferences-backed set.
        prefs.getStringSet(KEY_PRIVACY_APP_LIST, emptySet())?.toSet() ?: emptySet()
    }

    override fun add(packageName: String) {
        val normalized = packageName.trim()
        if (normalized.isEmpty()) return
        warnIfMainThread("add")
        synchronized(prefsLock) {
            val updated = prefs.getStringSet(KEY_PRIVACY_APP_LIST, emptySet())
                ?.toMutableSet()
                ?: mutableSetOf()
            updated.add(normalized)
            val committed = prefs.edit().putStringSet(KEY_PRIVACY_APP_LIST, updated).commit()
            if (!committed) {
                Log.e(TAG, "add: failed to persist privacy list update (package_redacted=true)")
                throw IllegalStateException("Failed to persist privacy list update")
            }
        }
    }

    override fun remove(packageName: String) {
        val normalized = packageName.trim()
        if (normalized.isEmpty()) return
        warnIfMainThread("remove")
        synchronized(prefsLock) {
            val updated = prefs.getStringSet(KEY_PRIVACY_APP_LIST, emptySet())
                ?.toMutableSet()
                ?: mutableSetOf()
            updated.remove(normalized)
            val committed = prefs.edit().putStringSet(KEY_PRIVACY_APP_LIST, updated).commit()
            if (!committed) {
                Log.e(TAG, "remove: failed to persist privacy list update (package_redacted=true)")
                throw IllegalStateException("Failed to persist privacy list update")
            }
        }
    }

    private fun warnIfMainThread(operation: String) {
        if (isMainThread()) {
            val message = "$operation: called on main thread; synchronous commit may cause jank"
            if (isDebugBuild()) {
                throw IllegalStateException(message)
            }
            Log.w(TAG, message)
        }
    }

    companion object {
        const val KEY_PRIVACY_APP_LIST = "privacy_app_list"
        private const val TAG = "CitrosPrivacyList"
        private val lockRegistry = WeakHashMap<SharedPreferences, Any>()

        private fun lockForPrefs(prefs: SharedPreferences): Any = synchronized(lockRegistry) {
            lockRegistry.getOrPut(prefs) { Any() }
        }
    }
}
