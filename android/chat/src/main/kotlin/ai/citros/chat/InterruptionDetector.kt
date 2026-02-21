package ai.citros.chat

import ai.citros.core.InterruptionClassifier
import ai.citros.core.InterruptionEvent
import android.accessibilityservice.AccessibilityService
import android.util.Log
import android.view.accessibility.AccessibilityEvent
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference

/**
 * Detects user interruptions during agent tool loops by monitoring
 * accessibility events. Produces InterruptionEvents consumed by
 * AgentExecutor via its interruptionSource lambda.
 *
 * Lifecycle: attach() when accessibility service connects, detach() on destroy.
 * Monitoring: startMonitoring() when tool loop begins, stopMonitoring() when it ends.
 *
 * Only TYPE_WINDOW_STATE_CHANGED is monitored (safe \u2014 avoids ghost input
 * regression from TYPE_VIEW_CLICKED or text events, see PR #662).
 */
object InterruptionDetector {
    private const val TAG = "CitrosInterrupt"

    /** The pending interruption event, atomically drained by AgentExecutor. */
    private val pendingEvent = AtomicReference<InterruptionEvent?>(null)

    /** Whether monitoring is active (tool loop running). */
    private val monitoring = AtomicBoolean(false)

    /** Whether the current event is agent-initiated. */
    private val agentActionInProgress = AtomicBoolean(false)

    /** Package the agent expects to be in foreground. */
    @Volatile
    private var expectedPackage: String? = null

    /** Debounce: timestamp of last interruption to prevent rapid-fire events. */
    @Volatile
    private var lastInterruptionTimeMs: Long = 0

    /** Debounce: timestamp of last agent action completion. */
    @Volatile
    private var lastAgentActionTimeMs: Long = 0

    /** Minimum ms between interruptions. */
    private const val INTERRUPTION_COOLDOWN_MS = 500L

    /** Suppress events within this window after agent actions. */
    private const val AGENT_SETTLE_MS = 200L

    private var service: AccessibilityService? = null

    fun attach(service: AccessibilityService) {
        this.service = service
    }

    /**
     * Detach from the accessibility service.
     * Order matters: stopMonitoring() must run before nulling service,
     * because it calls updateEventTypes() which reads the service reference.
     */
    fun detach() {
        stopMonitoring()
        service = null
    }

    /**
     * Start monitoring for interruptions. Enables TYPE_WINDOW_STATE_CHANGED
     * on the accessibility service. Called when tool loop begins.
     */
    fun startMonitoring(currentForegroundPackage: String? = null) {
        expectedPackage = currentForegroundPackage
        pendingEvent.set(null)
        lastInterruptionTimeMs = 0
        monitoring.set(true)
        updateEventTypes()
    }

    /**
     * Stop monitoring. Resets event types to 0. Called when tool loop ends.
     */
    fun stopMonitoring() {
        monitoring.set(false)
        pendingEvent.set(null)
        agentActionInProgress.set(false)
        expectedPackage = null
        updateEventTypes()
    }

    /**
     * Mark that the agent is about to perform a UI action.
     * Events during this window are suppressed as agent-initiated.
     */
    fun markAgentAction() {
        agentActionInProgress.set(true)
    }

    /**
     * Clear the agent action flag after action completes.
     * Records timestamp for settle delay suppression.
     */
    fun clearAgentAction() {
        agentActionInProgress.set(false)
        lastAgentActionTimeMs = System.currentTimeMillis()
    }

    /**
     * Update the expected foreground package (e.g., after agent opens an app).
     */
    fun setExpectedPackage(pkg: String?) {
        expectedPackage = pkg
    }

    /**
     * Drain the pending interruption event. Called by AgentExecutor
     * via interruptionSource lambda. Returns null if no interruption.
     */
    fun drain(): InterruptionEvent? = pendingEvent.getAndSet(null)

    /**
     * Called by CitrosAccessibilityService.onAccessibilityEvent().
     * Classifies the event and queues it if it's a user interruption.
     *
     * Note: Currently only produces AppSwitch and ExternalInterrupt events
     * via TYPE_WINDOW_STATE_CHANGED. UserTouch detection (TYPE_VIEW_CLICKED)
     * is deferred to a future phase due to ghost input regression risk (PR #662).
     */
    fun onAccessibilityEvent(event: AccessibilityEvent) {
        if (!monitoring.get()) return

        // Only handle window state changes
        if (event.eventType != AccessibilityEvent.TYPE_WINDOW_STATE_CHANGED) return

        val pkg = event.packageName?.toString() ?: return

        // Suppress if agent action is in progress
        if (agentActionInProgress.get()) return

        // Suppress during settle window after agent action
        val now = System.currentTimeMillis()
        if (now - lastAgentActionTimeMs < AGENT_SETTLE_MS) return

        // Debounce rapid events
        if (now - lastInterruptionTimeMs < INTERRUPTION_COOLDOWN_MS) return

        // Classify the event
        val interruption = InterruptionClassifier.classifyWindowChange(
            newPackage = pkg,
            expectedPackage = expectedPackage,
            isAgentAction = false // already checked above
        ) ?: return

        // Queue the event (first one wins \u2014 CAS ensures no overwrite)
        if (pendingEvent.compareAndSet(null, interruption)) {
            lastInterruptionTimeMs = now
            Log.d(TAG, "Interruption detected: \$interruption")
        }
    }

    /**
     * Dynamically update accessibility service event types.
     * TYPE_WINDOW_STATE_CHANGED when monitoring, 0 when not.
     */
    private fun updateEventTypes() {
        val svc = service ?: return
        try {
            val info = svc.serviceInfo ?: return
            info.eventTypes = if (monitoring.get()) {
                AccessibilityEvent.TYPE_WINDOW_STATE_CHANGED
            } else {
                0
            }
            svc.serviceInfo = info
        } catch (e: Exception) {
            Log.w(TAG, "Failed to update event types: \${e.message}")
        }
    }
}
