package ai.citros.core

/**
 * Shared test fakes for AgentExecutor tests.
 */

open class FakeToolExecutionDelegate : ToolExecutionDelegate {
    var executeResult: ToolResult = ToolResult("Success")
    var onExecuteThrow: Throwable? = null
    var onExecute: (suspend (ToolCall, ScreenContent?) -> ToolResult)? = null
    var refreshScreenResult: ScreenContent? = null
    var refreshAfterToolResult: ScreenContent? = null
    var refreshAfterToolCalled = false
    var screenReaderAvailable = true
    var accessibilityWaitResult = true
    var uiMutatingTools: Set<String> = PhoneAgentApi.UI_MUTATING_TOOLS
    val toolResults = mutableListOf<Pair<String, String>>()
    var lastStepStarted = 0

    override suspend fun executeToolCall(toolCall: ToolCall, screenContent: ScreenContent?): ToolResult {
        onExecuteThrow?.let { throw it }
        return onExecute?.invoke(toolCall, screenContent) ?: executeResult
    }

    var refreshScreenCalled = false
    override suspend fun refreshScreen(): ScreenContent? {
        refreshScreenCalled = true
        return refreshScreenResult
    }

    override suspend fun refreshScreenAfterTool(toolName: String, actionResult: String): ScreenContent? {
        refreshAfterToolCalled = true
        return refreshAfterToolResult
    }

    override suspend fun settleDelay(toolName: String, actionResult: String) {}

    override fun formatToolResult(actionSummary: String, screenContent: ScreenContent?): String {
        return if (screenContent != null) "$actionSummary\n\nSCREEN:\n${screenContent.packageName}"
        else actionSummary
    }

    override fun isUiMutatingTool(toolName: String): Boolean = toolName in uiMutatingTools

    override fun isScreenReaderAvailable(): Boolean = screenReaderAvailable

    override suspend fun waitForAccessibility(timeoutMs: Long): Boolean = accessibilityWaitResult

    override fun accessibilityWaitMs(): Long = 100L

    override fun outputVerbosity(): OutputVerbosity = OutputVerbosity.NORMAL

    override fun addToolResult(toolCallId: String, result: String, toolName: String?, isError: Boolean) {
        toolResults.add(toolCallId to result)
    }

    val steerMessages = mutableListOf<String>()
    override fun addSteerMessage(text: String) {
        steerMessages.add(text)
    }

    override fun onStepStarted(step: Int, maxSteps: Int) {
        lastStepStarted = step
    }
}

class FakeLoopProgressListener : LoopProgressListener {
    val toolStarted = mutableListOf<Triple<String, Int, Int>>()
    val toolResults = mutableListOf<Triple<String, String, OutputVisibility>>()
    val toolResultsWithError = mutableListOf<List<Any>>()
    val toolErrors = mutableListOf<Triple<String, String, ErrorSeverity>>()
    var accessibilityLostCalled = false

    override fun onToolStarted(toolName: String, toolIndex: Int, batchSize: Int) {
        toolStarted.add(Triple(toolName, toolIndex, batchSize))
    }

    override fun onToolResult(toolName: String, result: String, visibility: OutputVisibility, isError: Boolean) {
        toolResults.add(Triple(toolName, result, visibility))
        toolResultsWithError.add(listOf(toolName, result, visibility, isError))
    }

    override fun onToolError(toolName: String, errorText: String, severity: ErrorSeverity) {
        toolErrors.add(Triple(toolName, errorText, severity))
    }

    override fun onAccessibilityLost() {
        accessibilityLostCalled = true
    }
}
