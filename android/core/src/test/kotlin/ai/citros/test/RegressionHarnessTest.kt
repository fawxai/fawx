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
    fun `ResponseContains fails when expected text is blank`() {
        val task = RegressionTask(
            id = "t-blank-response-criterion",
            name = "Blank criterion",
            userMessage = "Do thing",
            successCriteria = listOf(SuccessCriterion.ResponseContains("   "))
        )
        val outcome = RegressionOutcome(
            status = RegressionStatus.COMPLETED,
            stepsUsed = 1,
            responseText = "Some response text"
        )

        val results = SuccessCriteriaEvaluator().evaluate(task, outcome)
        assertEquals(1, results.size)
        assertFalse(results.single().passed)
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
    fun `AppInForeground criterion has pass and fail cases`() {
        val task = RegressionTask(
            id = "criterion-app",
            name = "App foreground",
            userMessage = "open settings",
            successCriteria = listOf(SuccessCriterion.AppInForeground("com.android.settings"))
        )

        val passing = SuccessCriteriaEvaluator().evaluate(
            task,
            RegressionOutcome(status = RegressionStatus.COMPLETED, stepsUsed = 1, screenPackageName = "com.android.settings")
        )
        val failing = SuccessCriteriaEvaluator().evaluate(
            task,
            RegressionOutcome(status = RegressionStatus.COMPLETED, stepsUsed = 1, screenPackageName = "com.other.app")
        )

        assertTrue(passing.single().passed)
        assertFalse(failing.single().passed)
    }

    @Test
    fun `ScreenContainsText criterion has pass and fail cases`() {
        val task = RegressionTask(
            id = "criterion-screen",
            name = "Screen contains",
            userMessage = "find text",
            successCriteria = listOf(SuccessCriterion.ScreenContainsText("Do Not Disturb"))
        )

        val passing = SuccessCriteriaEvaluator().evaluate(
            task,
            RegressionOutcome(status = RegressionStatus.COMPLETED, stepsUsed = 1, screenText = "Quick settings\nDo Not Disturb")
        )
        val failing = SuccessCriteriaEvaluator().evaluate(
            task,
            RegressionOutcome(status = RegressionStatus.COMPLETED, stepsUsed = 1, screenText = "Quick settings\nWi-Fi")
        )

        assertTrue(passing.single().passed)
        assertFalse(failing.single().passed)
    }

    @Test
    fun `StepsLessThan criterion has pass and fail cases`() {
        val task = RegressionTask(
            id = "criterion-steps",
            name = "Step budget",
            userMessage = "do thing",
            successCriteria = listOf(SuccessCriterion.StepsLessThan(3))
        )

        val passing = SuccessCriteriaEvaluator().evaluate(
            task,
            RegressionOutcome(status = RegressionStatus.FAILED, stepsUsed = 2)
        )
        val failing = SuccessCriteriaEvaluator().evaluate(
            task,
            RegressionOutcome(status = RegressionStatus.COMPLETED, stepsUsed = 3)
        )

        assertTrue(passing.single().passed)
        assertFalse(failing.single().passed)
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
        assertEquals(
            setOf(
                "nav-001", "nav-002", "info-001", "multi-001", "msg-001",
                "nav-003", "nav-004", "search-001", "screen-001", "notif-001"
            ),
            ids
        )
    }

    @Test
    fun `runner returns failed result when scripted outcome does not satisfy criteria`() = runTest {
        val task = RegressionTask(
            id = "t-fail",
            name = "Failure path",
            userMessage = "Do thing",
            successCriteria = listOf(
                SuccessCriterion.CompletedWithinSteps,
                SuccessCriterion.ResponseContains("ok")
            ),
            maxSteps = 3
        )
        val driver = object : RegressionHarnessDriver {
            override suspend fun enforcePrecondition(precondition: Precondition) = Unit

            override suspend fun execute(task: RegressionTask): RegressionOutcome {
                return RegressionOutcome(
                    status = RegressionStatus.FAILED,
                    stepsUsed = 4,
                    responseText = "not ok"
                )
            }
        }

        val result = RegressionRunner(driver).run(task)
        assertFalse(result.passed)
        assertEquals(1, result.criteriaResults.count { it.passed })
        assertEquals(1, result.criteriaResults.count { !it.passed })
    }
}
