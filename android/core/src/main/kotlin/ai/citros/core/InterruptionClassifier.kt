package ai.citros.core

/**
 * Classifies window/app events into InterruptionEvents.
 * Pure logic — no Android dependencies. Testable in :core.
 */
object InterruptionClassifier {
    /** Known phone/dialer packages that indicate incoming calls. */
    private val PHONE_PACKAGES = setOf(
        "com.android.dialer", "com.google.android.dialer",
        "com.android.incallui", "com.samsung.android.incallui"
    )

    /** System packages that indicate system dialogs. */
    private val SYSTEM_PACKAGES = setOf("android", "com.android.systemui")

    /**
     * Classify a window state change into an InterruptionEvent.
     *
     * Note: If the user opens Citros itself (e.g., chat overlay), it will be
     * classified as an AppSwitch. This is intentional — the user IS interrupting
     * the agent by interacting with the Citros UI directly.
     *
     * @param newPackage Package name of the new foreground app
     * @param expectedPackage Package the agent expects to be in foreground (null = unknown)
     * @param isAgentAction Whether this change was initiated by the agent
     * @return InterruptionEvent or null if this is expected/agent-initiated
     */
    fun classifyWindowChange(
        newPackage: String,
        expectedPackage: String?,
        isAgentAction: Boolean
    ): InterruptionEvent? {
        if (isAgentAction) return null
        if (expectedPackage != null && newPackage == expectedPackage) return null

        return when {
            PHONE_PACKAGES.contains(newPackage) ->
                InterruptionEvent.ExternalInterrupt("Incoming phone call detected")
            SYSTEM_PACKAGES.contains(newPackage) ->
                InterruptionEvent.ExternalInterrupt("System dialog appeared")
            else ->
                InterruptionEvent.AppSwitch(
                    previousApp = expectedPackage ?: "unknown",
                    newApp = newPackage
                )
        }
    }
}
