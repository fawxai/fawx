package ai.citros.core

import android.app.Notification
import android.app.PendingIntent
import android.app.RemoteInput
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Bundle
import android.service.notification.NotificationListenerService
import android.service.notification.StatusBarNotification

/**
 * Parsed notification data for tool consumption.
 * Uses stable [key] from StatusBarNotification for identification,
 * preventing race conditions when notifications change between reads.
 */
data class ParsedNotification(
    val key: String,
    val packageName: String,
    val appName: String,
    val title: String?,
    val text: String?,
    val subText: String?,
    val postTime: Long,
    val isOngoing: Boolean,
    val actions: List<NotificationAction>
)

/**
 * A notification action (button) that can be invoked.
 */
data class NotificationAction(
    val index: Int,
    val title: String,
    val hasRemoteInput: Boolean
)

/**
 * Manages active notifications via NotificationListenerService.
 * Provides read, tap, dismiss, and reply operations.
 *
 * ## Architecture
 * The NotificationListenerService runs in its own process/lifecycle.
 * This singleton bridges between the service and the agent tools.
 * The service attaches/detaches itself, and tools query through this helper.
 *
 * ## Notification Identification
 * Notifications are identified by their stable `key` (from StatusBarNotification.key),
 * not ephemeral indices. This prevents race conditions when notifications change
 * between a read and a subsequent action.
 */
object NotificationHelper {

    private var service: NotificationListenerService? = null

    fun attach(listenerService: NotificationListenerService) {
        service = listenerService
    }

    fun detach() {
        service = null
    }

    fun isAttached(): Boolean = service != null

    /**
     * Get all active (non-dismissed) notifications, parsed into structured data.
     *
     * @param includeOngoing Whether to include ongoing notifications (e.g., music player, foreground service)
     * @return List of parsed notifications, sorted by post time (newest first)
     * @throws NotificationAccessDeniedException if notification access has been revoked
     */
    fun getActiveNotifications(includeOngoing: Boolean = false): List<ParsedNotification> {
        val svc = service ?: return emptyList()
        return try {
            svc.activeNotifications
                ?.filter { includeOngoing || !it.isOngoing }
                ?.sortedByDescending { it.postTime }
                ?.map { sbn -> parseNotification(sbn) }
                ?: emptyList()
        } catch (e: SecurityException) {
            throw NotificationAccessDeniedException(
                "Notification access denied. Re-enable in Settings → Apps → Special access → Notification access.",
                e
            )
        }
    }

    /**
     * Parse a StatusBarNotification into our structured format.
     */
    private fun parseNotification(sbn: StatusBarNotification): ParsedNotification {
        val notification = sbn.notification
        val extras = notification.extras

        val actions = notification.actions?.mapIndexed { actionIndex, action ->
            NotificationAction(
                index = actionIndex,
                title = action.title?.toString() ?: "Action $actionIndex",
                hasRemoteInput = action.remoteInputs?.isNotEmpty() == true
            )
        } ?: emptyList()

        return ParsedNotification(
            key = sbn.key,
            packageName = sbn.packageName,
            appName = getAppName(sbn.packageName),
            title = extras.getCharSequence(Notification.EXTRA_TITLE)?.toString(),
            text = extras.getCharSequence(Notification.EXTRA_TEXT)?.toString(),
            subText = extras.getCharSequence(Notification.EXTRA_SUB_TEXT)?.toString(),
            postTime = sbn.postTime,
            isOngoing = sbn.isOngoing,
            actions = actions
        )
    }

    /**
     * Get human-readable app name from package name.
     */
    private fun getAppName(packageName: String): String {
        val svc = service ?: return packageName
        return try {
            val pm = svc.packageManager
            val appInfo = pm.getApplicationInfo(packageName, 0)
            pm.getApplicationLabel(appInfo).toString()
        } catch (e: PackageManager.NameNotFoundException) {
            packageName
        }
    }

    /**
     * Find a notification by its stable key.
     * Returns the raw StatusBarNotification or null if not found/dismissed.
     */
    private fun findByKey(key: String): StatusBarNotification? {
        val svc = service ?: return null
        return try {
            svc.activeNotifications?.find { it.key == key }
        } catch (e: SecurityException) {
            throw NotificationAccessDeniedException(
                "Notification access denied. Re-enable in Settings → Apps → Special access → Notification access.",
                e
            )
        }
    }

    /**
     * Tap (open) a notification by its stable key.
     * This sends the notification's contentIntent.
     *
     * @param key The stable notification key from [ParsedNotification.key]
     * @return true if the intent was sent
     */
    fun tapNotification(key: String): Boolean {
        val sbn = findByKey(key) ?: return false
        val contentIntent = sbn.notification.contentIntent ?: return false

        return try {
            contentIntent.send()
            true
        } catch (e: PendingIntent.CanceledException) {
            false
        }
    }

    /**
     * Dismiss a notification by its stable key.
     *
     * @param key The stable notification key from [ParsedNotification.key]
     * @return true if dismissal was requested
     * @throws NotificationAccessDeniedException if notification access has been revoked
     */
    fun dismissNotification(key: String): Boolean {
        val svc = service ?: return false
        val sbn = findByKey(key) ?: return false
        return try {
            svc.cancelNotification(sbn.key)
            true
        } catch (e: SecurityException) {
            throw NotificationAccessDeniedException(
                "Notification access denied. Re-enable in Settings → Apps → Special access → Notification access.",
                e
            )
        }
    }

    /**
     * Reply to a notification using its inline reply action.
     * Finds the first action with a RemoteInput and sends the reply text.
     *
     * @param key The stable notification key from [ParsedNotification.key]
     * @param text Reply text to send
     * @return true if the reply was sent
     */
    fun replyToNotification(key: String, text: String): Boolean {
        val svc = service ?: return false
        val sbn = findByKey(key) ?: return false
        val notification = sbn.notification

        // Find the first action with remote input (inline reply)
        val replyAction = notification.actions?.firstOrNull { action ->
            action.remoteInputs?.isNotEmpty() == true
        } ?: return false

        val remoteInput = replyAction.remoteInputs?.firstOrNull() ?: return false

        // Build the reply intent
        val replyIntent = Intent()
        val replyBundle = Bundle().apply {
            putCharSequence(remoteInput.resultKey, text)
        }
        RemoteInput.addResultsToIntent(arrayOf(remoteInput), replyIntent, replyBundle)

        return try {
            replyAction.actionIntent.send(svc, 0, replyIntent)
            true
        } catch (e: PendingIntent.CanceledException) {
            false
        }
    }

    /**
     * Format notifications as readable text for the agent.
     * Uses stable keys for notification identification.
     */
    fun formatForPrompt(notifications: List<ParsedNotification>): String {
        if (notifications.isEmpty()) return "No notifications"

        return buildString {
            appendLine("Notifications (${notifications.size}):")
            notifications.forEach { n ->
                appendLine("[${n.key}] ${n.appName}: ${n.title ?: "(no title)"}")
                n.text?.let { appendLine("    ${it.take(100)}") }
                if (n.actions.isNotEmpty()) {
                    val actionStr = n.actions.joinToString(", ") { a ->
                        "${a.title}${if (a.hasRemoteInput) " [reply]" else ""}"
                    }
                    appendLine("    Actions: $actionStr")
                }
            }
        }
    }
}

/**
 * Thrown when notification access permission has been revoked.
 */
class NotificationAccessDeniedException(
    message: String,
    cause: Throwable? = null
) : Exception(message, cause)
