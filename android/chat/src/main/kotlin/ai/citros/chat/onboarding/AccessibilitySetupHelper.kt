package ai.citros.chat.onboarding

import android.content.ActivityNotFoundException
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.provider.Settings
import kotlinx.coroutines.delay
import kotlinx.coroutines.withTimeoutOrNull

/**
 * Helps users enable the Citros accessibility service during onboarding.
 *
 * Provides methods to check service status, open system settings, and
 * poll for permission grant with timeout.
 */
class AccessibilitySetupHelper(
    private val context: Context,
    private val serviceClass: String = DEFAULT_SERVICE_CLASS
) {

    /**
     * Launch the system accessibility settings screen.
     * Uses the standard ACTION_ACCESSIBILITY_SETTINGS intent — no OEM-specific extras.
     */
    fun openAccessibilitySettings(): Boolean {
        val intent = Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS).apply {
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        }
        return try {
            context.startActivity(intent)
            true
        } catch (_: ActivityNotFoundException) {
            false
        }
    }

    /**
     * Check if the Citros accessibility service is currently enabled.
     *
     * Reads the secure setting `enabled_accessibility_services` and checks
     * whether the Citros service component is listed.
     */
    fun isAccessibilityEnabled(): Boolean {
        val enabledServices = Settings.Secure.getString(
            context.contentResolver,
            Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES
        ) ?: return false

        val expectedComponent = ComponentName(context.packageName, serviceClass)
            .flattenToString()

        return enabledServices.split(':').any { service ->
            ComponentName.unflattenFromString(service)?.flattenToString() == expectedComponent
        }
    }

    /**
     * Poll for accessibility permission grant.
     *
     * Checks every [pollIntervalMs] whether the service is enabled.
     * Returns true as soon as permission is granted, or false on timeout.
     *
     * @param timeoutMs Maximum time to wait (default 30 seconds)
     * @param pollIntervalMs Time between checks (default 500ms)
     */
    suspend fun waitForPermission(
        timeoutMs: Long = 30_000,
        pollIntervalMs: Long = 500
    ): Boolean {
        return withTimeoutOrNull(timeoutMs) {
            while (!isAccessibilityEnabled()) {
                delay(pollIntervalMs)
            }
            true
        } ?: false
    }

    companion object {
        const val DEFAULT_SERVICE_CLASS = "ai.citros.chat.CitrosAccessibilityService"
    }
}
