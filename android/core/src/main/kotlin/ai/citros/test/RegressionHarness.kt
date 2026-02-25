package ai.citros.test

/** Sprint 1 skeleton model for deterministic regression tasks. */
data class RegressionTask(
    val id: String,
    val name: String,
    val userMessage: String,
    val preconditions: List<Precondition> = emptyList(),
    val successCriteria: List<SuccessCriterion>,
    val maxSteps: Int = 15,
    /**
     * Reserved for runner timing contract wiring.
     *
     * Current skeleton carries this value through task execution, but does not yet enforce
     * wall-clock cancellation in [RegressionRunner].
     */
    val maxTimeMs: Long = 60_000,
    val tags: Set<String> = emptySet()
)

sealed interface Precondition {
    data object HomeScreen : Precondition
    data class AppInForeground(val packageName: String) : Precondition
}

sealed interface SuccessCriterion {
    /**
     * Checks terminal success and budget adherence from the task-level budget ([RegressionTask.maxSteps]).
     */
    data object CompletedWithinSteps : SuccessCriterion

    data class AppInForeground(val packageName: String) : SuccessCriterion
    data class ScreenContainsText(val text: String) : SuccessCriterion
    data class ResponseContains(val text: String) : SuccessCriterion

    /**
     * Checks only the observed step count against its own threshold, regardless of final status.
     *
     * This is intentionally different from [CompletedWithinSteps], which requires
     * [RegressionStatus.COMPLETED].
     */
    data class StepsLessThan(val maxSteps: Int) : SuccessCriterion
}

enum class RegressionStatus {
    COMPLETED,
    FAILED,
    TIMED_OUT
}

data class RegressionOutcome(
    val status: RegressionStatus,
    val stepsUsed: Int,
    val responseText: String? = null,
    val screenPackageName: String? = null,
    val screenText: String = "",
    val elapsedMs: Long = 0
)

data class CriterionEvaluation(
    val criterion: SuccessCriterion,
    val passed: Boolean,
    val detail: String
)

data class RegressionResult(
    val taskId: String,
    val taskName: String,
    val passed: Boolean,
    val criteriaResults: List<CriterionEvaluation>,
    val stepsUsed: Int,
    val elapsedMs: Long,
    val status: RegressionStatus
)

class SuccessCriteriaEvaluator {
    fun evaluate(task: RegressionTask, outcome: RegressionOutcome): List<CriterionEvaluation> {
        return task.successCriteria.map { criterion ->
            when (criterion) {
                SuccessCriterion.CompletedWithinSteps -> {
                    val passed = outcome.status == RegressionStatus.COMPLETED && outcome.stepsUsed <= task.maxSteps
                    CriterionEvaluation(
                        criterion,
                        passed,
                        "status=${outcome.status}, steps=${outcome.stepsUsed}, max=${task.maxSteps}"
                    )
                }

                is SuccessCriterion.AppInForeground -> {
                    val passed = outcome.screenPackageName == criterion.packageName
                    CriterionEvaluation(
                        criterion,
                        passed,
                        "package=${outcome.screenPackageName ?: "<none>"}, expected=${criterion.packageName}"
                    )
                }

                is SuccessCriterion.ScreenContainsText -> {
                    val passed = outcome.screenText.contains(criterion.text, ignoreCase = true)
                    CriterionEvaluation(
                        criterion,
                        passed,
                        "screenContains='${criterion.text}'"
                    )
                }

                is SuccessCriterion.ResponseContains -> {
                    val expected = criterion.text.trim()
                    val response = outcome.responseText.orEmpty()
                    val passed = expected.isNotEmpty() && response.contains(expected, ignoreCase = true)
                    CriterionEvaluation(
                        criterion,
                        passed,
                        "responseContains='${criterion.text}'"
                    )
                }

                is SuccessCriterion.StepsLessThan -> {
                    val passed = outcome.stepsUsed < criterion.maxSteps
                    CriterionEvaluation(
                        criterion,
                        passed,
                        "steps=${outcome.stepsUsed}, threshold=${criterion.maxSteps}"
                    )
                }
            }
        }
    }
}

/** Execution adapter used by the runner skeleton. */
interface RegressionHarnessDriver {
    suspend fun enforcePrecondition(precondition: Precondition)
    suspend fun execute(task: RegressionTask): RegressionOutcome
}

class RegressionRunner(
    private val driver: RegressionHarnessDriver,
    private val evaluator: SuccessCriteriaEvaluator = SuccessCriteriaEvaluator()
) {
    suspend fun run(task: RegressionTask): RegressionResult {
        task.preconditions.forEach { driver.enforcePrecondition(it) }
        val outcome = driver.execute(task)
        val criteriaResults = evaluator.evaluate(task, outcome)
        return RegressionResult(
            taskId = task.id,
            taskName = task.name,
            // Empty criteria is treated as not-passed to avoid vacuous true regressions.
            passed = criteriaResults.isNotEmpty() && criteriaResults.all { it.passed },
            criteriaResults = criteriaResults,
            stepsUsed = outcome.stepsUsed,
            elapsedMs = outcome.elapsedMs,
            status = outcome.status
        )
    }
}

val REGRESSION_SUITE_SEEDS: List<RegressionTask> = listOf(
    RegressionTask(
        id = "nav-001",
        name = "Open Settings",
        userMessage = "Open Settings",
        preconditions = listOf(Precondition.HomeScreen),
        successCriteria = listOf(
            SuccessCriterion.AppInForeground("com.android.settings"),
            SuccessCriterion.StepsLessThan(3)
        ),
        tags = setOf("navigation", "simple")
    ),
    RegressionTask(
        id = "nav-002",
        name = "Open Gmail",
        userMessage = "Open Gmail",
        preconditions = listOf(Precondition.HomeScreen),
        successCriteria = listOf(
            SuccessCriterion.AppInForeground("com.google.android.gm"),
            SuccessCriterion.StepsLessThan(3)
        ),
        tags = setOf("navigation", "simple")
    ),
    RegressionTask(
        id = "info-001",
        name = "Weather query (conversational)",
        userMessage = "What's the weather like?",
        successCriteria = listOf(
            SuccessCriterion.CompletedWithinSteps,
            SuccessCriterion.ResponseContains("temperature")
        ),
        tags = setOf("information", "conversational")
    ),
    RegressionTask(
        id = "multi-001",
        name = "Set a timer",
        userMessage = "Set a timer for 5 minutes",
        preconditions = listOf(Precondition.HomeScreen),
        successCriteria = listOf(
            SuccessCriterion.CompletedWithinSteps,
            SuccessCriterion.StepsLessThan(8)
        ),
        maxSteps = 12,
        tags = setOf("utility", "multi-step")
    ),
    RegressionTask(
        id = "msg-001",
        name = "Open Messages compose",
        userMessage = "Open Messages and start a new message",
        preconditions = listOf(Precondition.HomeScreen),
        successCriteria = listOf(
            SuccessCriterion.AppInForeground("com.google.android.apps.messaging"),
            SuccessCriterion.StepsLessThan(6)
        ),
        tags = setOf("messaging", "multi-step")
    ),
    RegressionTask(
        id = "nav-003",
        name = "Turn on Do Not Disturb",
        userMessage = "Turn on Do Not Disturb",
        successCriteria = listOf(
            SuccessCriterion.AppInForeground("com.android.settings"),
            SuccessCriterion.ScreenContainsText("Do Not Disturb"),
            SuccessCriterion.StepsLessThan(8)
        ),
        tags = setOf("navigation", "settings")
    ),
    RegressionTask(
        id = "nav-004",
        name = "Open the camera and take a photo",
        userMessage = "Open the camera and take a photo",
        successCriteria = listOf(
            SuccessCriterion.AppInForeground("com.google.android.GoogleCamera"),
            SuccessCriterion.StepsLessThan(5)
        ),
        tags = setOf("navigation", "camera")
    ),
    RegressionTask(
        id = "search-001",
        name = "Search for pizza near me",
        userMessage = "Search for pizza near me",
        successCriteria = listOf(
            SuccessCriterion.CompletedWithinSteps,
            SuccessCriterion.ResponseContains("pizza"),
            SuccessCriterion.StepsLessThan(8)
        ),
        tags = setOf("search", "maps")
    ),
    RegressionTask(
        id = "screen-001",
        name = "What's on my screen?",
        userMessage = "What's on my screen?",
        successCriteria = listOf(
            SuccessCriterion.CompletedWithinSteps,
            SuccessCriterion.ResponseContains("screen"),
            SuccessCriterion.StepsLessThan(3)
        ),
        tags = setOf("screen", "awareness")
    ),
    RegressionTask(
        id = "notif-001",
        name = "Read my last notification",
        userMessage = "Read my last notification",
        successCriteria = listOf(
            SuccessCriterion.CompletedWithinSteps,
            SuccessCriterion.StepsLessThan(5)
        ),
        tags = setOf("notifications")
    )
)
