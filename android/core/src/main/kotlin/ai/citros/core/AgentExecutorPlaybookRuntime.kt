package ai.citros.core

/**
 * Runtime coordinator that integrates playbook replay with normal AgentExecutor exploration.
 *
 * Flow:
 * 1) classify task intent
 * 2) find playbook match
 * 3) execute replay path
 * 4) branch on replay status (SUCCESS/PARTIAL/ABANDONED/CANCELLED)
 * 5) fallback to exploration loop when replay is partial/abandoned or no match exists
 */
class AgentExecutorPlaybookRuntime(
    private val intentClassifier: TaskIntentClassifier,
    private val playbookMatcher: PlaybookMatcher,
    private val parameterResolver: PlaybookParameterResolver,
    private val playbookExecutor: PlaybookExecutor,
    private val explorationRunner: ExplorationRunner
) {
    suspend fun run(userMessage: String, cancellationToken: CancellationToken): RuntimeExecutionResult {
        val intent = intentClassifier.classify(userMessage)
        val match = intent?.let { playbookMatcher.findMatch(it.appPackage, it.taskType) }

        if (match == null) {
            val fallback = explorationRunner.run(userMessage)
            return RuntimeExecutionResult(
                outcome = RuntimeOutcome.EXPLORATION,
                replay = null,
                explorationResult = fallback
            )
        }

        val params = parameterResolver.extractFromMessage(userMessage, match.playbook)
        val replayResult = playbookExecutor.execute(match, params, cancellationToken)
        val replayTrace = ReplayRuntimeTrace(
            playbookId = match.playbook.id,
            confidence = match.confidence,
            status = replayResult.status,
            stepsCompleted = replayResult.stepsCompleted,
            stepsDiverged = replayResult.stepsDiverged,
            trace = replayResult.trace
        )

        return when (replayResult.status) {
            PlaybookStatus.SUCCESS -> RuntimeExecutionResult(
                outcome = RuntimeOutcome.REPLAY_SUCCESS,
                replay = replayTrace,
                explorationResult = null
            )

            PlaybookStatus.CANCELLED -> RuntimeExecutionResult(
                outcome = RuntimeOutcome.CANCELLED,
                replay = replayTrace,
                explorationResult = null
            )

            PlaybookStatus.PARTIAL,
            PlaybookStatus.ABANDONED -> {
                val fallback = explorationRunner.run(userMessage)
                RuntimeExecutionResult(
                    outcome = RuntimeOutcome.EXPLORATION_AFTER_REPLAY,
                    replay = replayTrace,
                    explorationResult = fallback
                )
            }
        }
    }
}

fun interface TaskIntentClassifier {
    fun classify(userMessage: String): PlaybookIntent?
}

fun interface PlaybookParameterResolver {
    fun extractFromMessage(userMessage: String, playbook: PlaybookEntity): Map<String, String>
}

fun interface ExplorationRunner {
    suspend fun run(userMessage: String): ExplorationResult
}

data class PlaybookIntent(
    val appPackage: String,
    val taskType: String
)

data class ReplayRuntimeTrace(
    val playbookId: Long,
    val confidence: Float,
    val status: PlaybookStatus,
    val stepsCompleted: Int,
    val stepsDiverged: Int,
    val trace: List<PlaybookStepTrace>
)

data class ExplorationResult(
    val status: TaskStatus,
    val responseText: String?
)

enum class RuntimeOutcome {
    REPLAY_SUCCESS,
    EXPLORATION,
    EXPLORATION_AFTER_REPLAY,
    CANCELLED
}

data class RuntimeExecutionResult(
    val outcome: RuntimeOutcome,
    val replay: ReplayRuntimeTrace?,
    val explorationResult: ExplorationResult?
)
