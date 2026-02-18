package ai.citros.core

/**
 * Result of evaluating a [BoundaryCheck] at a tool boundary.
 *
 * Four possible outcomes:
 * - [Continue] — no issue, the loop proceeds normally
 * - [Inject] — append a message to the current tool result (e.g. stuck warning)
 * - [Steer] — inject user messages mid-loop and skip remaining tool calls in the batch
 * - [Stop] — exit the loop with a reason (e.g. cancelled, max_steps)
 *
 * Priority: [Stop] > [Steer] > [Inject].
 * [Stop] short-circuits. [Steer] takes priority over [Inject] (user intent > system warnings).
 * Multiple [Inject] results are concatenated.
 *
 * See docs/specs/citros-architecture-roadmap.md §1.2
 */
sealed class CheckResult {
    /** No issue — continue the loop. */
    object Continue : CheckResult()

    /** Append [message] to the current tool result and continue. */
    data class Inject(val message: String) : CheckResult()

    /** Exit the loop. [reason] becomes [LoopResult.Completed.exitReason]. */
    data class Stop(val reason: String) : CheckResult()

    /**
     * Inject user steer messages and skip remaining tool calls in the current batch.
     *
     * Unlike [Inject] (which appends to tool results), steer messages are added as
     * first-class user messages in conversation history. The model sees them as direct
     * user turns, which carry significantly more weight than incidental text in tool output.
     *
     * Semantics:
     * - Remaining tool calls in the batch are **skipped**
     * - Each message in [userMessages] is added via [ToolExecutionDelegate.addSteerMessage]
     * - The loop **continues** with a fresh API call (does not exit)
     *
     * Takes priority over [Inject] — user intent overrides system warnings.
     *
     * @param userMessages Messages from the user to inject into conversation history
     */
    data class Steer(val userMessages: List<String>) : CheckResult()
}

/**
 * Snapshot of the loop's state at a tool boundary, passed to [BoundaryCheck]es.
 *
 * Built by [AgentExecutor] after each tool call. Checks read from this
 * immutable snapshot — they never mutate it.
 *
 * @param step Current step number (starts at 1, incremented at the beginning of each loop iteration)
 * @param maxSteps Maximum steps allowed before [StepLimitCheck] fires
 * @param lastToolName Name of the tool call that just executed
 * @param lastScreenHash Hash of the current screen content, or null if screen unavailable
 * @param isCancelled Whether the user has requested cancellation
 * @param pendingSteerMessages User messages queued for mid-loop injection via [SteerCheck]
 */
data class LoopState(
    val step: Int,
    val maxSteps: Int,
    val lastToolName: String,
    val lastScreenHash: Int?,
    val isCancelled: Boolean,
    val pendingSteerMessages: List<String> = emptyList()
)

/**
 * A check that runs at tool boundaries during the agent execution loop.
 *
 * A "tool boundary" is the point after a tool call executes and its result
 * is formatted, but before the result is committed to conversation history.
 * This is where we evaluate whether to continue, inject warnings into the
 * tool result, stop the loop, or inject user steer messages.
 *
 * Boundary checks formalize what were previously ad-hoc inline conditions
 * in the while loop: stuck detection, step limits, cancellation, and
 * mid-loop message injection (steer).
 *
 * Each check evaluates the current [LoopState] and returns a [CheckResult].
 * Checks may be stateful (e.g. [StuckDetectionCheck] tracks screen hashes
 * across calls). Create fresh instances per [AgentExecutor.run] invocation.
 *
 * Default evaluation order: [CancellationCheck], [StepLimitCheck],
 * [StuckDetectionCheck], [SteerCheck]. [Stop] results short-circuit —
 * remaining checks are skipped. [Steer] takes priority over [Inject].
 *
 * See docs/specs/citros-architecture-roadmap.md §1.2
 */
interface BoundaryCheck {
    /**
     * Evaluate this check against the current loop state.
     *
     * Suspend to support checks that need to wait (e.g. [AccessibilityGateCheck]
     * waiting for service reconnection). Existing checks that don't suspend are
     * backward compatible — non-suspending bodies work in suspend functions.
     *
     * @param state Immutable snapshot of the loop's state
     * @return [CheckResult] indicating whether to continue, inject, or stop
     */
    suspend fun check(state: LoopState): CheckResult
}

/**
 * Stops the loop when the user has requested cancellation.
 *
 * Should be first in the check list so cancellation is handled before
 * any other check runs.
 */
class CancellationCheck : BoundaryCheck {
    override suspend fun check(state: LoopState): CheckResult {
        return if (state.isCancelled) CheckResult.Stop("cancelled") else CheckResult.Continue
    }
}

/**
 * Stops the loop when the step count reaches the configured maximum.
 *
 * Reads [LoopState.step] and [LoopState.maxSteps] — no internal state.
 */
class StepLimitCheck : BoundaryCheck {
    override suspend fun check(state: LoopState): CheckResult {
        return if (state.step >= state.maxSteps) CheckResult.Stop("max_steps") else CheckResult.Continue
    }
}

/**
 * Injects a warning when the agent appears stuck.
 *
 * Wraps [StuckDetector] and maintains detection state across calls within
 * a single loop execution. Create a fresh instance per [AgentExecutor.run]
 * — use [withDefaults] for standard thresholds.
 *
 * Two detection modes (delegated to [StuckDetector]):
 * 1. Screen hash repetition — identical hashes in rolling window
 * 2. Consecutive waits with no screen change
 *
 * @param stuckDetector The detector instance to delegate to
 */
class StuckDetectionCheck(
    private val stuckDetector: StuckDetector
) : BoundaryCheck {
    private val detectorState = StuckDetector.State()

    override suspend fun check(state: LoopState): CheckResult {
        val warning = stuckDetector.check(
            detectorState, state.lastToolName, state.lastScreenHash
        )
        return if (warning != null) CheckResult.Inject(warning) else CheckResult.Continue
    }

    companion object {
        /** Create a [StuckDetectionCheck] with default [StuckDetector] thresholds. */
        fun withDefaults(): StuckDetectionCheck = StuckDetectionCheck(StuckDetector())
    }
}

/**
 * Injects user steer messages when the user sends a message during an active tool loop.
 *
 * Stateless — reads [LoopState.pendingSteerMessages] which is populated by
 * [AgentExecutor] from the [steerMessageSource] lambda at each boundary.
 *
 * Should be last in the default check list:
 * - [CancellationCheck] and [StepLimitCheck] should short-circuit before steer fires
 *   (cancellation invalidates steer; step limits are hard ceilings)
 * - [StuckDetectionCheck] should run first so its [CheckResult.Inject] warnings are
 *   delivered in the tool result when no steer occurs. If steer DOES fire, it takes
 *   priority via [evaluateBoundaryChecks] and stuck warnings are discarded — user
 *   intent overrides system warnings.
 *
 * See h1-pr3-steer-design.md for full design rationale including pressure test
 * against OpenClaw's steer implementation.
 */
class SteerCheck : BoundaryCheck {
    override suspend fun check(state: LoopState): CheckResult {
        if (state.pendingSteerMessages.isEmpty()) return CheckResult.Continue
        return CheckResult.Steer(state.pendingSteerMessages)
    }
}

/**
 * Gates the tool loop on accessibility service availability.
 *
 * When the accessibility service is detached, this check retries reconnection
 * with exponential backoff. Each retry doubles the wait time up to
 * [maxRetries] attempts, with per-attempt timeout of [baseTimeoutMs] * 2^attempt.
 *
 * Placed AFTER [CancellationCheck] in the default check list so user
 * cancel is respected even during an accessibility wait (coroutine
 * cancellation handles in-progress waits).
 *
 * @param isAvailable Lambda returning true if accessibility service is currently connected
 * @param waitForReconnect Suspend lambda that waits up to the given ms for reconnection
 * @param onReconnected Called after successful reconnection (e.g., to refresh screen)
 * @param onLost Called when all retries are exhausted
 * @param baseTimeoutMs Base timeout for the first reconnection attempt (default 2000ms)
 * @param maxRetries Maximum reconnection attempts with exponential backoff (default 3)
 *
 * See h1-pr5-tool-gating-pressure-test.md for design rationale.
 */
class AccessibilityGateCheck(
    private val isAvailable: () -> Boolean,
    private val waitForReconnect: suspend (Long) -> Boolean,
    private val onReconnected: suspend () -> Unit,
    private val onLost: () -> Unit,
    private val baseTimeoutMs: Long = DEFAULT_BASE_TIMEOUT_MS,
    private val maxRetries: Int = DEFAULT_MAX_RETRIES
) : BoundaryCheck {
    override suspend fun check(state: LoopState): CheckResult {
        if (isAvailable()) {
            return CheckResult.Continue
        }

        // Exponential backoff: baseTimeout * 2^attempt (capped at maxRetries)
        for (attempt in 0 until maxRetries) {
            val timeoutMs = baseTimeoutMs * (1L shl attempt) // 2s, 4s, 8s...
            val reconnected = waitForReconnect(timeoutMs)
            if (reconnected) {
                    onReconnected()
                return CheckResult.Continue
            }
        }

        onLost()
        return CheckResult.Stop("accessibility_lost")
    }

    companion object {
        /** Default base timeout for first reconnection attempt. */
        const val DEFAULT_BASE_TIMEOUT_MS = 2000L
        /** Default maximum reconnection attempts. */
        const val DEFAULT_MAX_RETRIES = 3
    }
}
