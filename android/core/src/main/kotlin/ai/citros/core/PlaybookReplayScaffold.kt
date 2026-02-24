package ai.citros.core

import kotlinx.coroutines.delay
import kotlinx.serialization.json.Json

/** Matcher + replay executor scaffold for Sprint 2 playbook-guided execution. */
class PlaybookMatcher(
    private val playbookDao: PlaybookDao
) {
    companion object {
        const val MIN_CONFIDENCE = 0.3f
        const val MIN_SUCCESS_COUNT = 1
    }

    fun findMatch(appPackage: String, taskType: String): PlaybookMatch? {
        val best = playbookDao.findByAppAndType(appPackage, taskType)
            .filter { it.confidence >= MIN_CONFIDENCE }
            .filter { it.successCount >= MIN_SUCCESS_COUNT }
            .sortedByDescending { it.confidence }
            .firstOrNull() ?: return null

        return PlaybookMatch(
            playbook = best,
            steps = playbookDao.getSteps(best.id),
            confidence = best.confidence
        )
    }
}

data class PlaybookMatch(
    val playbook: PlaybookEntity,
    val steps: List<PlaybookStepEntity>,
    val confidence: Float
)

interface PlaybookScreenReader {
    fun getScreenContent(): ScreenContent
}

interface PlaybookToolRunner {
    fun execute(toolCall: ToolCall, screenContent: ScreenContent): ToolResult
}

interface PlaybookLlmFallback {
    suspend fun executeSingleStep(screenContent: ScreenContent): ToolResult
}

interface CancellationToken {
    val isCancelled: Boolean
}

class PlaybookExecutor(
    private val toolRunner: PlaybookToolRunner,
    private val screenReader: PlaybookScreenReader,
    private val playbookDao: PlaybookDao,
    private val llmFallback: PlaybookLlmFallback,
    private val delayFn: suspend (Long) -> Unit = { delay(it) }
) {
    companion object {
        const val FINGERPRINT_MATCH_THRESHOLD = 0.7f
        const val MAX_DIVERGENCE_RATIO = 0.5f
    }

    suspend fun execute(
        match: PlaybookMatch,
        parameters: Map<String, String>,
        cancellationToken: CancellationToken
    ): PlaybookExecutionResult {
        var divergedSteps = 0
        var completedSteps = 0
        val trace = mutableListOf<PlaybookStepTrace>()

        for (step in match.steps) {
            if (cancellationToken.isCancelled) {
                return PlaybookExecutionResult(
                    status = PlaybookStatus.CANCELLED,
                    stepsCompleted = completedSteps,
                    stepsDiverged = divergedSteps,
                    trace = trace
                )
            }

            val currentScreen = screenReader.getScreenContent()
            val currentFingerprint = ScreenFingerprinting.compute(currentScreen)
            val expectedFingerprint = ScreenFingerprint(
                structuralHash = step.screenFingerprint,
                packageName = step.screenPackage ?: currentScreen.packageName,
                activityName = step.screenActivity
            )

            val similarity = ScreenFingerprinting.similarity(currentFingerprint, expectedFingerprint)
            if (similarity >= FINGERPRINT_MATCH_THRESHOLD) {
                val resolved = resolveTemplate(parseTemplate(step.toolInputTemplate), parameters)
                if (!resolved.isComplete) {
                    divergedSteps++
                    llmFallback.executeSingleStep(currentScreen)
                    trace += PlaybookStepTrace(step, StepOutcome.PARAM_UNRESOLVED)
                } else {
                    val result = toolRunner.execute(
                        ToolCall(
                            id = "playbook_${step.playbookId}_${step.stepOrder}",
                            name = step.toolName,
                            input = resolved.inputs
                        ),
                        currentScreen
                    )

                    delayFn(step.settleTimeMs.toLong())
                    val afterFingerprint = ScreenFingerprinting.compute(screenReader.getScreenContent())
                    val expectedNext = step.expectedNextFingerprint
                    if (result.isError || (expectedNext != null && afterFingerprint.structuralHash != expectedNext)) {
                        divergedSteps++
                        trace += PlaybookStepTrace(step, StepOutcome.DIVERGED)
                    } else {
                        completedSteps++
                        trace += PlaybookStepTrace(step, StepOutcome.MATCHED)
                    }
                }
            } else {
                divergedSteps++
                llmFallback.executeSingleStep(currentScreen)
                trace += PlaybookStepTrace(step, StepOutcome.SCREEN_MISMATCH)
            }

            val attempted = completedSteps + divergedSteps
            if (attempted >= 3 && divergedSteps.toFloat() / attempted > MAX_DIVERGENCE_RATIO) {
                playbookDao.recordExecution(match.playbook.id, success = false)
                return PlaybookExecutionResult(PlaybookStatus.ABANDONED, completedSteps, divergedSteps, trace)
            }
        }

        val success = divergedSteps == 0
        playbookDao.recordExecution(match.playbook.id, success)
        return PlaybookExecutionResult(
            status = if (success) PlaybookStatus.SUCCESS else PlaybookStatus.PARTIAL,
            stepsCompleted = completedSteps,
            stepsDiverged = divergedSteps,
            trace = trace
        )
    }

    private fun parseTemplate(templateJson: String): Map<String, Any> {
        val parsed = runCatching { Json.parseToJsonElement(templateJson) }
            .getOrElse { throw IllegalArgumentException("toolInputTemplate must be valid JSON", it) }
        val jsonObject = parsed as? kotlinx.serialization.json.JsonObject
            ?: throw IllegalArgumentException("toolInputTemplate must be a JSON object")
        return JsonUtils.parseJsonObjectToMap(jsonObject)
    }

    private fun resolveTemplate(
        template: Map<String, Any>,
        parameters: Map<String, String>
    ): ResolvedTemplate {
        val unresolved = mutableListOf<String>()

        fun resolveValue(value: Any?): Any = when (value) {
            null -> "{null}"
            is String -> {
                if (value.startsWith("{") && value.endsWith("}")) {
                    val paramName = value.substring(1, value.length - 1)
                    val paramValue = parameters[paramName]
                    if (paramValue == null) {
                        unresolved += paramName
                        value
                    } else {
                        paramValue
                    }
                } else {
                    value
                }
            }

            is Map<*, *> -> value.entries.associate { (key, nestedValue) ->
                key.toString() to resolveValue(nestedValue)
            }

            is List<*> -> value.map { nested -> resolveValue(nested) }
            else -> value
        }

        val resolved = template.entries.associate { (key, value) -> key to resolveValue(value) }
        return ResolvedTemplate(resolved, unresolved.distinct(), unresolved.isEmpty())
    }
}

enum class PlaybookStatus {
    SUCCESS,
    PARTIAL,
    ABANDONED,
    CANCELLED
}

enum class StepOutcome {
    MATCHED,
    DIVERGED,
    SCREEN_MISMATCH,
    PARAM_UNRESOLVED
}

data class PlaybookStepTrace(
    val step: PlaybookStepEntity,
    val outcome: StepOutcome
)

data class PlaybookExecutionResult(
    val status: PlaybookStatus,
    val stepsCompleted: Int,
    val stepsDiverged: Int,
    val trace: List<PlaybookStepTrace>
)

/**
 * Nested maps/lists are resolved recursively.
 * Placeholder resolution only supports full-string placeholders (e.g. "{recipient}").
 * Embedded placeholders inside larger strings (e.g. "send to {recipient}") are intentionally not expanded.
 * Missing parameters are kept as unresolved placeholders in [inputs] and surfaced via [unresolvedParams].
 */
data class ResolvedTemplate(
    val inputs: Map<String, Any>,
    val unresolvedParams: List<String>,
    val isComplete: Boolean
)
