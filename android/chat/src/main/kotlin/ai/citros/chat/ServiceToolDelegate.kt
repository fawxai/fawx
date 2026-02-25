package ai.citros.chat

import ai.citros.core.OutputVerbosity
import ai.citros.core.PhoneAgentApi
import ai.citros.core.ScreenContent
import ai.citros.core.ScreenReader
import ai.citros.core.ToolCall
import ai.citros.core.ToolErrorCode
import ai.citros.core.ToolExecutionDelegate
import ai.citros.core.ToolResult
import android.util.Log

/**
 * Service-owned implementation of [ToolExecutionDelegate].
 *
 * Decouples tool execution from ChatViewModel so the AgentExecutor can
 * run in AgentService's coroutine scope, surviving activity destruction.
 */
class ServiceToolDelegate(
    private val phoneAgentApi: PhoneAgentApi,
    private val outputVerbosity: OutputVerbosity = OutputVerbosity.NORMAL,
    private val onConfirmationRequested: ((ToolCall, String, String?, String) -> Unit)? = null,
    private val awaitConfirmationDecision: (suspend (String) -> Boolean)? = null,
    private val onOfferChoicesRequested: ((String, String, List<String>) -> Unit)? = null,
    private val awaitOfferChoiceDecision: (suspend (String) -> String?)? = null
) : ToolExecutionDelegate {

    companion object {
        private const val TAG = "ServiceToolDelegate"

        // Keep settle timings aligned with ChatViewModel path.
        const val DELAY_DEFAULT_MS = 500L
        const val DELAY_AFTER_TAP_MS = 800L
    }

    override suspend fun executeToolCall(toolCall: ToolCall, screenContent: ScreenContent?): ToolResult {
        if (toolCall.name == "offer_choices") {
            return executeOfferChoices(toolCall)
        }
        val isUiMutating = isUiMutatingTool(toolCall.name)
        if (isUiMutating) InterruptionDetector.markAgentAction()
        return try {
            phoneAgentApi.executeToolCall(toolCall, screenContent)
        } catch (e: Exception) {
            Log.w(TAG, "executeToolCall(${toolCall.name}) failed: ${e.message}")
            ToolResult("Error: ${e.message?.take(100)}", isError = true)
        } finally {
            if (isUiMutating) InterruptionDetector.clearAgentAction()
        }
    }

    override suspend fun refreshScreen(): ScreenContent? {
        return try {
            if (ScreenReader.isAttached()) ScreenReader.getScreenContent() else null
        } catch (e: Exception) {
            Log.w(TAG, "refreshScreen failed: ${e.message}")
            null
        }
    }

    override suspend fun refreshScreenAfterTool(toolName: String, actionResult: String): ScreenContent? {
        return try {
            if (!ScreenReader.isAttached()) return null

            val usesSmartPoll = toolName == "open_app" || toolName == "press_home" || toolName == "press_back"
            val screen = if (usesSmartPoll && !actionResult.startsWith("Failed")) {
                val before = ScreenReader.getScreenContent()
                val beforePackage = before?.packageName
                var latest: ScreenContent? = before
                var tries = 0
                while (tries < 3) {
                    kotlinx.coroutines.delay(200)
                    latest = ScreenReader.getScreenContent()
                    if (beforePackage != null && latest?.packageName != null && latest?.packageName != beforePackage) {
                        break
                    }
                    tries++
                }
                latest
            } else {
                ScreenReader.getScreenContent()
            }

            screen?.packageName?.let { InterruptionDetector.setExpectedPackage(it) }
            screen
        } catch (e: Exception) {
            Log.w(TAG, "refreshScreenAfterTool failed: ${e.message}")
            null
        }
    }

    override suspend fun settleDelay(toolName: String, actionResult: String) {
        val usesSmartPoll = toolName == "open_app" || toolName == "press_home" || toolName == "press_back"
        if (usesSmartPoll) return // Smart poll tools don't use fixed delays

        val delayMs = when (toolName) {
            "think", "wait" -> 0L // think is no-op; wait already sleeps internally
            "tap", "tap_text", "long_press" -> DELAY_AFTER_TAP_MS
            else -> DELAY_DEFAULT_MS
        }

        if (delayMs > 0) kotlinx.coroutines.delay(delayMs)
    }

    override fun formatToolResult(actionSummary: String, screenContent: ScreenContent?): String {
        return phoneAgentApi.formatToolResult(actionSummary, screenContent)
    }

    override fun isUiMutatingTool(toolName: String): Boolean {
        return toolName in PhoneAgentApi.UI_MUTATING_TOOLS
    }

    override fun isScreenReaderAvailable(): Boolean = ScreenReader.isAttached()

    override suspend fun waitForAccessibility(timeoutMs: Long): Boolean {
        return ScreenReader.waitForAttachment(timeoutMs = timeoutMs)
    }

    override fun accessibilityWaitMs(): Long = 5000L

    override fun outputVerbosity(): OutputVerbosity = outputVerbosity

    override fun addToolResult(toolCallId: String, result: String, toolName: String?, isError: Boolean) {
        phoneAgentApi.addToolResult(toolCallId, result, toolName, isError)
    }

    override fun addSteerMessage(text: String) {
        phoneAgentApi.addSteerMessage(text)
    }

    override fun onStepStarted(step: Int, maxSteps: Int) {
        phoneAgentApi.currentToolStep = step
    }

    override suspend fun requestUserConfirmation(
        toolCall: ToolCall,
        requestId: String,
        reason: String,
        timeoutMs: Long,
        reasonCode: String? = null
    ): Boolean {
        // timeoutMs is enforced by AgentExecutor via withTimeoutOrNull around this call.
        // ServiceToolDelegate intentionally only bridges request/response plumbing.
        onConfirmationRequested?.invoke(toolCall, requestId, reasonCode, reason)
        val waiter = awaitConfirmationDecision ?: return false
        return waiter(requestId)
    }

    private suspend fun executeOfferChoices(toolCall: ToolCall): ToolResult {
        val question = (toolCall.input["question"] as? String)?.trim()
            ?.takeIf { it.isNotEmpty() }
            ?: return ToolResult(
                "Failed: offer_choices: question is required",
                isError = true,
                errorCode = ToolErrorCode.INVALID_INPUT
            )

        val rawChoices = toolCall.input["choices"] as? List<*>
            ?: return ToolResult(
                "Failed: offer_choices: choices must be an array of strings",
                isError = true,
                errorCode = ToolErrorCode.INVALID_INPUT
            )

        val choices = rawChoices.map { it as? String }
        if (choices.any { it.isNullOrBlank() }) {
            return ToolResult(
                "Failed: offer_choices: choices must contain only non-empty strings",
                isError = true,
                errorCode = ToolErrorCode.INVALID_INPUT
            )
        }

        val normalizedChoices = choices.filterNotNull().map { it.trim() }
        if (normalizedChoices.size !in 2..5) {
            return ToolResult(
                "Failed: offer_choices: choices must include 2-5 options",
                isError = true,
                errorCode = ToolErrorCode.INVALID_INPUT
            )
        }

        val requestId = "offer_choices:${toolCall.id.ifBlank { System.nanoTime().toString() }}"
        val notify = onOfferChoicesRequested
        val waiter = awaitOfferChoiceDecision
        if (notify == null || waiter == null) {
            return ToolResult(
                "Failed: offer_choices: runtime choice handler is not configured",
                isError = true,
                errorCode = ToolErrorCode.NOT_CONFIGURED
            )
        }

        notify(requestId, question, normalizedChoices)
        val selection = waiter(requestId)
            ?: return ToolResult(
                "Failed: offer_choices: no choice selected",
                isError = true,
                errorCode = ToolErrorCode.EXECUTION_FAILED
            )

        return ToolResult(selection)
    }
}
