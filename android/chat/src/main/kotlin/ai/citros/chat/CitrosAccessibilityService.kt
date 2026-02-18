package ai.citros.chat

import android.accessibilityservice.AccessibilityService
import android.accessibilityservice.AccessibilityServiceInfo
import android.content.Intent
import android.provider.Settings
import android.view.accessibility.AccessibilityEvent
import ai.citros.core.ClipboardHelper
import ai.citros.core.ScreenReader

class CitrosAccessibilityService : AccessibilityService() {

    override fun onServiceConnected() {
        super.onServiceConnected()
        
        serviceInfo = AccessibilityServiceInfo().apply {
            eventTypes = AccessibilityEvent.TYPES_ALL_MASK
            feedbackType = AccessibilityServiceInfo.FEEDBACK_GENERIC
            flags = AccessibilityServiceInfo.FLAG_INCLUDE_NOT_IMPORTANT_VIEWS or
                    AccessibilityServiceInfo.FLAG_REPORT_VIEW_IDS or
                    AccessibilityServiceInfo.FLAG_RETRIEVE_INTERACTIVE_WINDOWS
            notificationTimeout = 100
        }
        
        ScreenReader.attach(this)
        ClipboardHelper.attach(this)
    }

    override fun onAccessibilityEvent(event: AccessibilityEvent?) {
        // We primarily use on-demand screen reading, not event listening
    }

    override fun onInterrupt() {
        // Required override
    }

    override fun onDestroy() {
        ClipboardHelper.detach()
        ScreenReader.detach()
        super.onDestroy()
    }

    companion object {
        fun isEnabled(context: android.content.Context): Boolean {
            val serviceName = "${context.packageName}/${CitrosAccessibilityService::class.java.canonicalName}"
            val enabledServices = Settings.Secure.getString(
                context.contentResolver,
                Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES
            ) ?: return false
            return enabledServices.contains(serviceName)
        }

        fun openSettings(context: android.content.Context) {
            val intent = Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS).apply {
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            }
            context.startActivity(intent)
        }
    }
}
