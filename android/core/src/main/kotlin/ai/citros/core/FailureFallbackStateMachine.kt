package ai.citros.core

import android.util.Log

fun interface FallbackStateLogger {
    fun debug(tag: String, message: String)
}

private val androidFallbackStateLogger = FallbackStateLogger { tag, message ->
    Log.d(tag, message)
}

enum class FailureClass { BLOCKED, LOW_SIGNAL_DYNAMIC, PARTIAL, UNTRUSTED }

enum class FallbackAction { ALTERNATE_SOURCE, NARROWED_QUERY, SUMMARIZE_UNCERTAINTY, EXPLICIT_BLOCKER }

data class FallbackTransition(
    val fromClass: FailureClass?,
    val toClass: FailureClass,
    val attempt: Int,
    val maxRetries: Int,
    val action: FallbackAction
)

class FailureFallbackStateMachine(
    private val retryBudgetByClass: Map<FailureClass, Int> = mapOf(
        FailureClass.BLOCKED to 0,
        FailureClass.LOW_SIGNAL_DYNAMIC to 2,
        FailureClass.PARTIAL to 1,
        FailureClass.UNTRUSTED to 1
    ),
    private val logger: FallbackStateLogger = androidFallbackStateLogger
) {
    companion object {
        private const val LOG_TAG = "CitrosFallbackSM"
    }

    private var currentClass: FailureClass? = null
    private var attempt: Int = -1

    fun reset() {
        currentClass = null
        attempt = -1
    }

    fun transition(toClass: FailureClass): FallbackTransition {
        val from = currentClass
        attempt = if (currentClass == toClass) attempt + 1 else 0
        currentClass = toClass
        val maxRetries = retryBudgetByClass[toClass] ?: 0
        val action = if (maxRetries <= 0 || attempt > maxRetries) {
            FallbackAction.EXPLICIT_BLOCKER
        } else {
            when (attempt) {
                0 -> FallbackAction.ALTERNATE_SOURCE
                1 -> FallbackAction.NARROWED_QUERY
                else -> FallbackAction.SUMMARIZE_UNCERTAINTY
            }
        }

        logger.debug(
            LOG_TAG,
            "transition from=${from ?: "NONE"} to=$toClass attempt=$attempt maxRetries=$maxRetries action=$action"
        )

        return FallbackTransition(from, toClass, attempt, maxRetries, action)
    }
}

fun FallbackTransition.toLoopDirective(): String {
    val guidance = when (action) {
        FallbackAction.ALTERNATE_SOURCE -> "Try an alternate source or tool path for the same goal."
        FallbackAction.NARROWED_QUERY -> "Retry with a narrower scope and explicit constraints."
        FallbackAction.SUMMARIZE_UNCERTAINTY -> "Summarize uncertainty and what is still unknown before next action."
        FallbackAction.EXPLICIT_BLOCKER -> "Stop retries and return an explicit blocker with what is needed to proceed."
    }
    return "\n[SYSTEM_FALLBACK class=$toClass attempt=$attempt/$maxRetries action=$action] $guidance"
}
