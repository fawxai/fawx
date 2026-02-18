package ai.citros.chat

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings

/**
 * Utility for checking and requesting the SYSTEM_ALERT_WINDOW permission
 * required for displaying overlays on top of other apps.
 */
object OverlayPermission {

    /**
     * Check whether the app has permission to draw overlays.
     *
     * On API 23+ this checks [Settings.canDrawOverlays].
     * On older versions the permission is granted at install time.
     *
     * @param context Application or Activity context
     * @return true if overlay permission is granted
     */
    fun canDrawOverlays(context: Context): Boolean {
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
            Settings.canDrawOverlays(context)
        } else {
            true // Pre-M: permission granted at install time
        }
    }

    /**
     * Build an intent to open the system overlay permission settings for this app.
     *
     * On API 23+ this opens the per-app "Display over other apps" setting.
     * On older versions this returns a general app settings intent.
     *
     * @param context Application or Activity context
     * @return Intent to launch the overlay permission settings
     */
    fun buildPermissionIntent(context: Context): Intent {
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
            Intent(
                Settings.ACTION_MANAGE_OVERLAY_PERMISSION,
                Uri.parse("package:${context.packageName}")
            )
        } else {
            Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS).apply {
                data = Uri.parse("package:${context.packageName}")
            }
        }
    }
}
