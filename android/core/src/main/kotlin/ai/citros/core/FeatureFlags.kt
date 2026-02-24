package ai.citros.core

/**
 * Feature flags for gradual rollout of architectural changes.
 *
 * See docs/specs/sprint-0-service-architecture.md (Rollback strategy):
 * Keep AgentExecutor constructable from both ChatViewModel and AgentService
 * behind USE_SERVICE_ARCHITECTURE. During development, defaults to true on
 * debug builds and can be toggled. If the service architecture causes
 * regressions, flipping the flag reverts to the ChatViewModel path without
 * code changes. Remove the flag and legacy path once stable (1-2 releases).
 */
object FeatureFlags {
    /**
     * When true, agent execution is owned by AgentService (foreground service).
     * When false, agent execution runs in ChatViewModel (legacy path).
     *
     * Default: true on debug builds, true on release (flip to false for rollback).
     */
    @Volatile
    var useServiceArchitecture: Boolean = true

    /** Remote rollout switch for action policy enforcement path. */
    @Volatile
    var actionPolicyEnabled: Boolean = true

    /** Rollout switch: emit audit records for allow decisions in addition to required decisions. */
    @Volatile
    var actionPolicyAuditAllowDecisions: Boolean = false

    /**
     * Reset all flags to defaults. Used in tests.
     */
    fun resetToDefaults() {
        useServiceArchitecture = true
        actionPolicyEnabled = true
        actionPolicyAuditAllowDecisions = false
    }
}
