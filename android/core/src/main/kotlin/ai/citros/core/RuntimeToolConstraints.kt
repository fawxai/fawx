package ai.citros.core

import java.util.UUID

/** Scope/lifespan for a runtime tool constraint. */
enum class ConstraintScope {
    /**
     * Applies to exactly one task (user request + follow-up action loop).
     *
     * TASK constraints are active only while their `createdTaskId` matches the
     * current task id. They remain queued/inactive when bound to a future task,
     * then expire once that task has finished.
     */
    TASK,

    /** Applies only to the next model turn. */
    TURN,

    /** Persists across turns/tasks until explicitly cleared or expired. */
    SESSION
}

/**
 * Structured runtime tool constraint.
 *
 * @param allowedTools Optional allow-list for this constraint. When multiple
 * active constraints provide non-empty allow-lists, effective policy is strict
 * intersection (a tool must appear in every active allow-list).
 * @param disallowedTools Explicit deny-list. Deny wins over allow.
 */
data class RuntimeToolConstraint(
    val id: String = UUID.randomUUID().toString(),
    val scope: ConstraintScope,
    val allowedTools: Set<String> = emptySet(),
    val disallowedTools: Set<String> = emptySet(),
    val source: String,
    val reason: String? = null,
    val createdTurn: Int = 0,
    val createdTaskId: Int = 0,
    val expiresAfterTurns: Int? = null,
    val enabled: Boolean = true
)

/** Internal resolution result used for filtering and execution gating. */
internal data class ToolConstraintDecision(
    val allowed: Boolean,
    val reason: String? = null
)
