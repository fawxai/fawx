package ai.citros.test

import kotlinx.coroutines.test.runTest
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class RegressionHarnessTest {

    @Test
    fun `RegressionTask defaults are scaffold-safe`() {
        val task = RegressionTask(
            id = "t-1",
            name = "Demo",
            userMessage = "Do thing",
            successCriteria = listOf(SuccessCriterion.CompletedWithinSteps)
        )

        assertEquals(15, task.maxSteps)
        assertEquals(60_000, task.maxTimeMs)
        assertTrue(task.preconditions.isEmpty())
        assertTrue(task.tags.isEmpty())
    }

    @Test
    fun `criteria evaluator passes when all criteria match`() {
        val task = RegressionTask(
            id = "t-2",
            name = "Open app",
            userMessage = "Open settings",
            successCriteria = listOf(
                SuccessCriterion.CompletedWithinSteps,
                SuccessCriterion.AppInForeground("com.android.settings"),
                SuccessCriterion.ScreenContainsText("Settings"),
                SuccessCriterion.ResponseContains("opened"),
                SuccessCriterion.StepsLessThan(4)
            ),
            maxSteps = 5
        )
        val outcome = RegressionOutcome(
            status = RegressionStatus.COMPLETED,
            stepsUsed = 2,
            responseText = "Opened Settings successfully",
            screenPackageName = "com.android.settings",
            screenText = "Settings\nNetwork & internet"
        )

        val results = SuccessCriteriaEvaluator().evaluate(task, outcome)
        assertTrue(results.all { it.passed })
    }

    @Test
    fun `ResponseContains fails safely when responseText is null`() {
        val task = RegressionTask(
            id = "t-null-response",
            name = "Null response",
            userMessage = "Do thing",
            successCriteria = listOf(SuccessCriterion.ResponseContains("expected"))
        )
        val outcome = RegressionOutcome(
            status = RegressionStatus.COMPLETED,
            stepsUsed = 1,
            responseText = null
        )

        val results = SuccessCriteriaEvaluator().evaluate(task, outcome)
        assertEquals(1, results.size)
        assertFalse(results.single().passed)
    }

    @Test
    fun `CompletedWithinSteps fails for TIMED_OUT outcome`() {
        val task = RegressionTask(
            id = "t-timeout",
            name = "Timeout",
            userMessage = "Do thing",
            successCriteria = listOf(SuccessCriterion.CompletedWithinSteps),
            maxSteps = 5
        )
        val outcome = RegressionOutcome(
            status = RegressionStatus.TIMED_OUT,
            stepsUsed = 2
        )

        val results = SuccessCriteriaEvaluator().evaluate(task, outcome)
        assertFalse(results.single().passed)
    }

    @Test
    fun `CompletedWithinSteps and StepsLessThan are intentionally distinct`() {
        val task = RegressionTask(
            id = "t-distinct",
            name = "Distinct criteria",
            userMessage = "Do thing",
            successCriteria = listOf(
                SuccessCriterion.CompletedWithinSteps,
                SuccessCriterion.StepsLessThan(5)
            ),
            maxSteps = 5
        )
        val outcome = RegressionOutcome(
            status = RegressionStatus.FAILED,
            stepsUsed = 4
        )

        val results = SuccessCriteriaEvaluator().evaluate(task, outcome)
        assertFalse(results[0].passed)
        assertTrue(results[1].passed)
    }

    @Test
    fun `criteria evaluator marks failures with mixed outcome`() {
        val task = RegressionTask(
            id = "t-3",
            name = "Weather",
            userMessage = "What's weather",
            successCriteria = listOf(
                SuccessCriterion.CompletedWithinSteps,
                SuccessCriterion.ResponseContains("temperature"),
                SuccessCriterion.StepsLessThan(3)
            ),
            maxSteps = 2
        )
        val outcome = RegressionOutcome(
            status = RegressionStatus.FAILED,
            stepsUsed = 4,
            responseText = "Could not complete"
        )

        val results = SuccessCriteriaEvaluator().evaluate(task, outcome)
        assertFalse(results.all { it.passed })
        assertEquals(3, results.count { !it.passed })
    }

    @Test
    fun `runner enforces preconditions and returns aggregated result`() = runTest {
        val task = RegressionTask(
            id = "t-4",
            name = "Runner",
            userMessage = "Run",
            preconditions = listOf(
                Precondition.HomeScreen,
                Precondition.AppInForeground("com.android.launcher")
            ),
            successCriteria = listOf(SuccessCriterion.StepsLessThan(5))
        )
        val enforced = mutableListOf<Precondition>()
        val driver = object : RegressionHarnessDriver {
            override suspend fun enforcePrecondition(precondition: Precondition) {
                enforced += precondition
            }

            override suspend fun execute(task: RegressionTask): RegressionOutcome {
                return RegressionOutcome(
                    status = RegressionStatus.COMPLETED,
                    stepsUsed = 2,
                    elapsedMs = 1234
                )
            }
        }

        val result = RegressionRunner(driver).run(task)
        assertEquals(2, enforced.size)
        assertTrue(result.passed)
        assertEquals("t-4", result.taskId)
        assertEquals(2, result.stepsUsed)
        assertEquals(1234, result.elapsedMs)
    }

    @Test
    fun `runner treats empty successCriteria as not passed`() = runTest {
        val task = RegressionTask(
            id = "t-empty",
            name = "No criteria",
            userMessage = "Do thing",
            successCriteria = emptyList()
        )
        val driver = object : RegressionHarnessDriver {
            override suspend fun enforcePrecondition(precondition: Precondition) = Unit

            override suspend fun execute(task: RegressionTask): RegressionOutcome {
                return RegressionOutcome(
                    status = RegressionStatus.COMPLETED,
                    stepsUsed = 1
                )
            }
        }

        val result = RegressionRunner(driver).run(task)
        assertTrue(result.criteriaResults.isEmpty())
        assertFalse(result.passed)
    }

    @Test
    fun `runner passes maxTimeMs through to driver unchanged`() = runTest {
        val task = RegressionTask(
            id = "t-time-budget",
            name = "Budget passthrough",
            userMessage = "Do thing",
            successCriteria = listOf(SuccessCriterion.StepsLessThan(3)),
            maxTimeMs = 9_999
        )
        var capturedMaxTimeMs: Long? = null
        val driver = object : RegressionHarnessDriver {
            override suspend fun enforcePrecondition(precondition: Precondition) = Unit

            override suspend fun execute(task: RegressionTask): RegressionOutcome {
                capturedMaxTimeMs = task.maxTimeMs
                return RegressionOutcome(
                    status = RegressionStatus.COMPLETED,
                    stepsUsed = 1
                )
            }
        }

        val result = RegressionRunner(driver).run(task)
        assertEquals(9_999, capturedMaxTimeMs)
        assertTrue(result.passed)
    }

    @Test
    fun `suite seeds include expected starter tasks`() {
        val ids = REGRESSION_SUITE_SEEDS.map { it.id }.toSet()
        assertEquals(setOf("nav-001", "nav-002", "info-001", "multi-001", "msg-001"), ids)
    }
}
