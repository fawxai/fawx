package ai.citros.core

/**
 * Fingerprint of the rendered UI structure at a point in time.
 *
 * `structuralHash` is expected to change when visible UI structure changes in a user-meaningful
 * way (new screen, dialog, layout/content mutation) and stay stable when structure is unchanged.
 * It is intentionally coarse and may ignore non-structural rendering noise.
 */
data class ScreenFingerprint(
    val structuralHash: Int,
    val packageName: String?
)

data class ActionFailure(
    val toolCall: ToolCall,
    val result: ToolResult,
    val screenBefore: ScreenFingerprint?,
    val screenAfter: ScreenFingerprint?,
    val consecutiveFailures: Int,
    val foregroundApp: String?,
    val failureType: FailureType
)

enum class FailureType {
    NO_EFFECT,
    TARGET_NOT_FOUND,
    UNEXPECTED_STATE,
    TOOL_ERROR,
    WRONG_OUTCOME
}

data class RecoveryAction(
    val description: String,
    val toolName: String,
    val toolInput: Map<String, Any>
)

interface RecoveryStrategy {
    val name: String
    fun appliesTo(failure: ActionFailure): Boolean
    fun recover(failure: ActionFailure): List<RecoveryAction>?
}

class TapRecoveryStrategy : RecoveryStrategy {
    override val name: String = "tap_recovery"

    override fun appliesTo(failure: ActionFailure): Boolean =
        failure.failureType == FailureType.NO_EFFECT &&
            failure.toolCall.name in setOf("tap", "tap_text") &&
            failure.consecutiveFailures <= 3

    override fun recover(failure: ActionFailure): List<RecoveryAction>? {
        return when (failure.toolCall.name) {
            "tap" -> {
                val textHint = failure.toolCall.input["text_hint"] as? String
                if (textHint.isNullOrBlank()) {
                    listOf(
                        RecoveryAction(
                            description = "Coordinate tap had no effect; try a small scroll then retry",
                            toolName = "scroll",
                            toolInput = mapOf("direction" to "down")
                        )
                    )
                } else {
                    listOf(
                        RecoveryAction(
                            description = "Coordinate tap had no effect; try text-based tap",
                            toolName = "tap_text",
                            toolInput = mapOf("text" to textHint)
                        )
                    )
                }
            }

            "tap_text" -> listOf(
                RecoveryAction(
                    description = "Text tap had no effect; scroll and retry",
                    toolName = "scroll",
                    toolInput = mapOf("direction" to "down")
                )
            )

            else -> null
        }
    }
}

class DialogRecoveryStrategy : RecoveryStrategy {
    override val name: String = "dialog_recovery"

    override fun appliesTo(failure: ActionFailure): Boolean =
        failure.failureType == FailureType.UNEXPECTED_STATE && failure.consecutiveFailures < 2

    override fun recover(failure: ActionFailure): List<RecoveryAction> = listOf(
        RecoveryAction(
            description = "Unexpected dialog/state detected; pressing back",
            toolName = "press_back",
            toolInput = emptyMap()
        )
    )
}

class AppResetRecoveryStrategy : RecoveryStrategy {
    override val name: String = "app_reset_recovery"

    override fun appliesTo(failure: ActionFailure): Boolean =
        failure.failureType == FailureType.UNEXPECTED_STATE &&
            !failure.foregroundApp.isNullOrBlank() &&
            failure.consecutiveFailures >= 2

    override fun recover(failure: ActionFailure): List<RecoveryAction> = listOf(
        RecoveryAction(
            description = "Wrong app state after repeated failures; returning home",
            toolName = "press_home",
            toolInput = emptyMap()
        )
    )
}

class GracefulCancelStrategy : RecoveryStrategy {
    override val name: String = "graceful_cancel"

    override fun appliesTo(failure: ActionFailure): Boolean = failure.consecutiveFailures >= 5

    override fun recover(failure: ActionFailure): List<RecoveryAction> = listOf(
        RecoveryAction(
            description = "Stuck after ${failure.consecutiveFailures} failures; returning home",
            toolName = "press_home",
            toolInput = emptyMap()
        )
    )
}

class RecoveryManager(
    private val strategies: List<RecoveryStrategy> = listOf(
        TapRecoveryStrategy(),
        AppResetRecoveryStrategy(),
        DialogRecoveryStrategy(),
        GracefulCancelStrategy()
    )
) {
    fun evaluateFailure(failure: ActionFailure): String? {
        val strategy = strategies.firstOrNull { it.appliesTo(failure) } ?: return null
        val actions = strategy.recover(failure).orEmpty()
        if (actions.isEmpty()) return null

        return buildString {
            appendLine()
            appendLine("⚠️ RECOVERY (${strategy.name}):")
            actions.forEach { action ->
                appendLine("  → ${action.description}")
                appendLine("    Suggested: ${action.toolName}(${action.toolInput})")
            }
            appendLine("Follow the suggestion above, or try a different approach.")
        }
    }
}

private val UI_MUTATING_TOOLS_FOR_RECOVERY = setOf(
    "tap", "tap_text", "type_text", "swipe", "scroll", "press_back", "press_home", "open_app", "long_press", "paste"
)

/**
 * Detect whether a tool execution should be recorded as a failure signal for recovery.
 *
 * `consecutiveFailures` is owned by the caller and represents the current streak **before** this
 * tool result is evaluated. When this function detects a failure, it returns an [ActionFailure]
 * with `consecutiveFailures + 1`; when it returns `null`, the caller should treat this attempt as
 * non-failure and reset/maintain its own counter according to higher-level policy.
 */
fun detectFailure(
    toolCall: ToolCall,
    result: ToolResult,
    screenBefore: ScreenFingerprint?,
    screenAfter: ScreenFingerprint?,
    consecutiveFailures: Int,
    foregroundPackage: String? = screenAfter?.packageName
): ActionFailure? {
    if (result.isError) {
        return ActionFailure(
            toolCall = toolCall,
            result = result,
            screenBefore = screenBefore,
            screenAfter = screenAfter,
            consecutiveFailures = consecutiveFailures + 1,
            foregroundApp = foregroundPackage,
            failureType = FailureType.TOOL_ERROR
        )
    }

    if (toolCall.name in UI_MUTATING_TOOLS_FOR_RECOVERY &&
        screenBefore != null &&
        screenAfter != null &&
        screenBefore.structuralHash == screenAfter.structuralHash
    ) {
        return ActionFailure(
            toolCall = toolCall,
            result = result,
            screenBefore = screenBefore,
            screenAfter = screenAfter,
            consecutiveFailures = consecutiveFailures + 1,
            foregroundApp = screenAfter.packageName,
            failureType = FailureType.NO_EFFECT
        )
    }

    if (screenBefore?.packageName != null &&
        screenAfter?.packageName != null &&
        screenBefore.packageName != screenAfter.packageName &&
        toolCall.name !in setOf("open_app", "press_home", "press_back")
    ) {
        return ActionFailure(
            toolCall = toolCall,
            result = result,
            screenBefore = screenBefore,
            screenAfter = screenAfter,
            consecutiveFailures = consecutiveFailures + 1,
            foregroundApp = screenAfter.packageName,
            failureType = FailureType.UNEXPECTED_STATE
        )
    }

    return null
}
