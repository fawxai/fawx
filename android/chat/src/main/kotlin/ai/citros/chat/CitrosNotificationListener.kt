package ai.citros.chat

import android.content.ComponentName
import android.content.Context
import android.provider.Settings
import android.service.notification.NotificationListenerService
import ai.citros.core.NotificationHelper

/**
 * NotificationListenerService for reading and interacting with device notifications.
 *
 * This runs separately from the AccessibilityService and requires its own permission
 * grant (Settings → Apps → Special access → Notification access).
 *
 * ## Lifecycle
 * - System binds this service when notification access is granted
 * - Attaches to [NotificationHelper] for tool access
 * - Detaches on disconnect
 *
 * ## On-demand Reading
 * Notification data is read on-demand via [NotificationHelper.getActiveNotifications]
 * rather than event-driven processing. The overridable `onNotificationPosted` and
 * `onNotificationRemoved` callbacks are not overridden since we don't need them.
 */
class CitrosNotificationListener : NotificationListenerService() {

    override fun onListenerConnected() {
        super.onListenerConnected()
        NotificationHelper.attach(this)
    }

    override fun onListenerDisconnected() {
        NotificationHelper.detach()
        super.onListenerDisconnected()
    }

    override fun onDestroy() {
        // Defensive: detach is idempotent. onListenerDisconnected() should fire first,
        // but onDestroy ensures cleanup if the disconnected callback is skipped.
        NotificationHelper.detach()
        super.onDestroy()
    }

    companion object {
        /**
         * Check if notification listener permission is granted.
         */
        fun isEnabled(context: Context): Boolean {
            val componentName = ComponentName(context, CitrosNotificationListener::class.java)
            val flat = Settings.Secure.getString(
                context.contentResolver,
                "enabled_notification_listeners"
            ) ?: return false
            return flat.contains(componentName.flattenToString())
        }

        /**
         * Open notification access settings.
         */
        fun openSettings(context: Context) {
            val intent = android.content.Intent(
                Settings.ACTION_NOTIFICATION_LISTENER_SETTINGS
            ).apply {
                addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK)
            }
            context.startActivity(intent)
        }
    }
}
