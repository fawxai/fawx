package ai.citros.chat

import android.accessibilityservice.AccessibilityService
import android.accessibilityservice.AccessibilityServiceInfo
import android.content.ComponentName
import android.content.Intent
import android.provider.Settings
import android.view.accessibility.AccessibilityEvent
import ai.citros.core.ClipboardHelper
import ai.citros.core.ScreenReader

class CitrosAccessibilityService : AccessibilityService() {

    override fun onServiceConnected() {
        super.onServiceConnected()
        
        serviceInfo = AccessibilityServiceInfo().apply {
            // Start with no event listening. InterruptionDetector dynamically
            // toggles TYPE_WINDOW_STATE_CHANGED when monitoring is active.
            eventTypes = 0
            feedbackType = AccessibilityServiceInfo.FEEDBACK_GENERIC
            flags = AccessibilityServiceInfo.FLAG_INCLUDE_NOT_IMPORTANT_VIEWS or
                    AccessibilityServiceInfo.FLAG_REPORT_VIEW_IDS or
                    AccessibilityServiceInfo.FLAG_RETRIEVE_INTERACTIVE_WINDOWS
            notificationTimeout = 250
        }
        
        ScreenReader.attach(this, SharedPrefsPrivacyList(applicationContext))
        InterruptionDetector.attach(this)
        ClipboardHelper.attach(this)
    }

    override fun onAccessibilityEvent(event: AccessibilityEvent?) {
        // Forward to InterruptionDetector for classification.
        // Only fires when monitoring is active (eventTypes dynamically toggled).
        event?.let { InterruptionDetector.onAccessibilityEvent(it) }
    }

    override fun onInterrupt() {
        // Required override
    }

    override fun onDestroy() {
        ClipboardHelper.detach()
        InterruptionDetector.detach()
        ScreenReader.detach()
        super.onDestroy()
    }

    companion object {
        fun isEnabled(context: android.content.Context): Boolean {
            val expectedComponent = ComponentName(context, CitrosAccessibilityService::class.java)
            val enabledServices = Settings.Secure.getString(
                context.contentResolver,
                Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES
            ) ?: return false
            return enabledServices
                .split(':')
                .mapNotNull { entry -> ComponentName.unflattenFromString(entry.trim()) }
                .any { enabled ->
                    enabled.packageName == expectedComponent.packageName &&
                        enabled.className == expectedComponent.className
                }
        }

        fun openSettings(context: android.content.Context) {
            val intent = Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS).apply {
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            }
            context.startActivity(intent)
        }
    }
}
