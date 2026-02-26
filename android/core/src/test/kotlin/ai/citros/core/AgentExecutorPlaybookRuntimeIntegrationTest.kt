package ai.citros.core

import android.graphics.Rect
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class AgentExecutorPlaybookRuntimeIntegrationTest {

    @Test
    fun `P22 full record-match-replay cycle uses playbook on second execution`() = runBlocking {
        val dao = RuntimeInMemoryPlaybookDao()
        val recorder = ExecutionRecorder(
            playbookDao = dao,
            parameterExtractor = ParameterExtractor(entityExtractor = HeuristicEntityExtractor()),
            nowMs = { 1000L }
        )

        // first execution (exploration) records a usable playbook
        recorder.onTaskStarted("Send a text to Mom saying hi")
        recorder.onToolExecuted(
            ToolCall("1", "open_app", mapOf("app_name" to "Messages")),
            screen("com.msg", "home").toFingerprint(),
            screen("com.msg", "compose").toFingerprint(),
            ToolResult("ok"),
            failure = null
        )
        recorder.onToolExecuted(
            ToolCall("2", "tap_text", mapOf("text" to "Mom")),
            screen("com.msg", "compose").toFingerprint(),
            screen("com.msg", "thread").toFingerprint(),
            ToolResult("ok"),
            failure = null
        )
        recorder.onToolExecuted(
            ToolCall("3", "type_text", mapOf("text" to "hi")),
            screen("com.msg", "thread").toFingerprint(),
            screen("com.msg", "thread").toFingerprint(),
            ToolResult("ok"),
            failure = null
        )
        recorder.onTaskCompleted(TaskStatus.COMPLETED, "sent")

        val match = PlaybookMatcher(dao).findMatch("com.msg", "send_message")
        assertNotNull(match)

        var explorationCalls = 0
        val runtime = runtimeUnderTest(
            dao = dao,
            intent = PlaybookIntent("com.msg", "send_message"),
            parameters = mapOf("recipient" to "Mom", "message" to "hi"),
            currentScreen = screen("com.msg", "home"),
            onExplore = {
                explorationCalls++
                ExplorationResult(TaskStatus.COMPLETED, "fallback")
            }
        )

        val result = runtime.run("Send a text to Mom saying hi", neverCancelled())
        assertEquals(RuntimeOutcome.REPLAY_SUCCESS, result.outcome)
        assertEquals(0, explorationCalls)
        assertEquals(PlaybookStatus.SUCCESS, result.replay!!.status)
    }

    @Test
    fun `P23 replay with different parameters keeps same path but substitutes values`() = runBlocking {
        val dao = RuntimeInMemoryPlaybookDao()
        val id = seedSingleStepPlaybook(dao)

        val seenInputs = mutableListOf<Map<String, Any>>()
        val runtime = runtimeUnderTest(
            dao = dao,
            intent = PlaybookIntent("com.msg", "send_message"),
            parameters = mapOf("recipient" to "Dad"),
            currentScreen = screen("com.msg", "home"),
            toolRunner = object : PlaybookToolRunner {
                override fun execute(toolCall: ToolCall, screenContent: ScreenContent): ToolResult {
                    seenInputs += toolCall.input
                    return ToolResult("ok")
                }
            }
        )

        val result = runtime.run("Send a text to Dad", neverCancelled())
        assertEquals(RuntimeOutcome.REPLAY_SUCCESS, result.outcome)
        assertEquals(PlaybookStatus.SUCCESS, result.replay!!.status)
        assertEquals(id, result.replay!!.playbookId)
        assertEquals("Dad", seenInputs.single()["text"])
    }

    @Test
    fun `P24 divergence triggers fallback exploration and can record a new playbook`() = runBlocking {
        val dao = RuntimeInMemoryPlaybookDao()
        seedDivergentPlaybook(dao)

        val recorder = ExecutionRecorder(
            playbookDao = dao,
            parameterExtractor = ParameterExtractor(entityExtractor = HeuristicEntityExtractor()),
            nowMs = { 2000L }
        )

        var explored = false
        val runtime = runtimeUnderTest(
            dao = dao,
            intent = PlaybookIntent("com.msg", "send_message"),
            parameters = emptyMap(),
            currentScreen = screen("com.msg", "unexpected"),
            onExplore = {
                explored = true
                // fallback run records a fresh successful path
                recorder.onTaskStarted("Send a text to Dad")
                recorder.onToolExecuted(
                    ToolCall("a", "tap_text", mapOf("text" to "Dad")),
                    screen("com.msg", "picker").toFingerprint(),
                    screen("com.msg", "thread").toFingerprint(),
                    ToolResult("ok"),
                    failure = null
                )
                recorder.onToolExecuted(
                    ToolCall("b", "type_text", mapOf("text" to "yo")),
                    screen("com.msg", "thread").toFingerprint(),
                    screen("com.msg", "thread").toFingerprint(),
                    ToolResult("ok"),
                    failure = null
                )
                recorder.onToolExecuted(
                    ToolCall("c", "tap", mapOf("element_id" to 9)),
                    screen("com.msg", "thread").toFingerprint(),
                    screen("com.msg", "thread").toFingerprint(),
                    ToolResult("ok"),
                    failure = null
                )
                recorder.onTaskCompleted(TaskStatus.COMPLETED, "sent")
                ExplorationResult(TaskStatus.COMPLETED, "explored")
            }
        )

        val result = runtime.run("Send a text to Dad", neverCancelled())
        assertEquals(RuntimeOutcome.EXPLORATION_AFTER_REPLAY, result.outcome)
        assertTrue(explored)
        assertEquals(PlaybookStatus.ABANDONED, result.replay!!.status)
        assertTrue(dao.findByAppAndType("com.msg", "send_message").size >= 2)
    }

    @Test
    fun `P25 null intent classifier result falls back to exploration`() = runBlocking {
        val dao = RuntimeInMemoryPlaybookDao()
        var explorationCalls = 0

        val runtime = runtimeUnderTest(
            dao = dao,
            intent = null,
            parameters = emptyMap(),
            currentScreen = screen("com.msg", "home"),
            onExplore = {
                explorationCalls++
                ExplorationResult(TaskStatus.COMPLETED, "explored")
            }
        )

        val result = runtime.run("Do something unknown", neverCancelled())
        assertEquals(RuntimeOutcome.EXPLORATION, result.outcome)
        assertNull(result.replay)
        assertEquals(1, explorationCalls)
    }

    @Test
    fun `P26 cancelled replay returns CANCELLED and does not fallback to exploration`() = runBlocking {
        val dao = RuntimeInMemoryPlaybookDao()
        seedSingleStepPlaybook(dao)

        var explorationCalls = 0
        val runtime = runtimeUnderTest(
            dao = dao,
            intent = PlaybookIntent("com.msg", "send_message"),
            parameters = mapOf("recipient" to "Mom"),
            currentScreen = screen("com.msg", "home"),
            onExplore = {
                explorationCalls++
                ExplorationResult(TaskStatus.COMPLETED, "should-not-run")
            }
        )

        val result = runtime.run(
            "Send a text to Mom",
            object : CancellationToken {
                override val isCancelled: Boolean = true
            }
        )

        assertEquals(RuntimeOutcome.CANCELLED, result.outcome)
        assertNotNull(result.replay)
        assertEquals(PlaybookStatus.CANCELLED, result.replay!!.status)
        assertNull(result.explorationResult)
        assertEquals(0, explorationCalls)
    }

    private fun runtimeUnderTest(
        dao: RuntimeInMemoryPlaybookDao,
        intent: PlaybookIntent?,
        parameters: Map<String, String>,
        currentScreen: ScreenContent,
        toolRunner: PlaybookToolRunner = object : PlaybookToolRunner {
            override fun execute(toolCall: ToolCall, screenContent: ScreenContent): ToolResult = ToolResult("ok")
        },
        onExplore: suspend () -> ExplorationResult = { ExplorationResult(TaskStatus.COMPLETED, "done") }
    ): AgentExecutorPlaybookRuntime {
        val matcher = PlaybookMatcher(dao)
        val executor = PlaybookExecutor(
            toolRunner = toolRunner,
            screenReader = object : PlaybookScreenReader {
                override fun getScreenContent(): ScreenContent = currentScreen
            },
            playbookDao = dao,
            llmFallback = object : PlaybookLlmFallback {
                override suspend fun executeSingleStep(screenContent: ScreenContent): ToolResult = ToolResult("fallback")
            },
            delayFn = {}
        )

        return AgentExecutorPlaybookRuntime(
            intentClassifier = TaskIntentClassifier { intent },
            playbookMatcher = matcher,
            parameterResolver = PlaybookParameterResolver { _, _ -> parameters },
            playbookExecutor = executor,
            explorationRunner = ExplorationRunner { onExplore() }
        )
    }

    private fun seedSingleStepPlaybook(dao: RuntimeInMemoryPlaybookDao): Long {
        val id = dao.insertPlaybook(
            PlaybookEntity(appPackage = "com.msg", taskType = "send_message", confidence = 0.9f, successCount = 2, createdAt = 1L, lastUsedAt = 1L)
        )
        dao.insertStep(
            PlaybookStepEntity(
                playbookId = id,
                stepOrder = 0,
                screenFingerprint = ScreenFingerprinting.compute(screen("com.msg", "home")).structuralHash,
                screenPackage = "com.msg",
                toolName = "tap_text",
                toolInputTemplate = "{\"text\":\"{recipient}\"}",
                selectorStrategy = "text",
                selectorValue = "{recipient}",
                expectedNextFingerprint = ScreenFingerprinting.compute(screen("com.msg", "home")).structuralHash,
                settleTimeMs = 0
            )
        )
        return id
    }

    private fun seedDivergentPlaybook(dao: RuntimeInMemoryPlaybookDao): Long {
        val id = dao.insertPlaybook(
            PlaybookEntity(appPackage = "com.msg", taskType = "send_message", confidence = 0.9f, successCount = 2, createdAt = 1L, lastUsedAt = 1L)
        )
        repeat(3) { i ->
            dao.insertStep(
                PlaybookStepEntity(
                    playbookId = id,
                    stepOrder = i,
                    screenFingerprint = "no-match-$i",
                    screenPackage = "com.msg",
                    toolName = "tap",
                    toolInputTemplate = "{}",
                    selectorStrategy = "id",
                    selectorValue = "x",
                    settleTimeMs = 0
                )
            )
        }
        return id
    }

    private fun screen(pkg: String, label: String): ScreenContent = ScreenContent(
        packageName = pkg,
        elements = listOf(
            ScreenElement(
                id = 1,
                text = label,
                contentDescription = null,
                className = "android.widget.TextView",
                isClickable = true,
                isEditable = false,
                bounds = Rect(0, 0, 10, 10),
                depth = 1
            )
        )
    )

    private fun neverCancelled() = object : CancellationToken {
        override val isCancelled: Boolean = false
    }
}

private class RuntimeInMemoryPlaybookDao : PlaybookDao {
    private val playbooks = linkedMapOf<Long, PlaybookEntity>()
    private val steps = linkedMapOf<Long, MutableList<PlaybookStepEntity>>()
    private var nextId = 1L

    override fun insertPlaybook(entity: PlaybookEntity): Long {
        val id = nextId++
        playbooks[id] = entity.copy(id = id)
        return id
    }

    override fun insertStep(entity: PlaybookStepEntity): Long {
        val list = steps.getOrPut(entity.playbookId) { mutableListOf() }
        val id = (list.size + 1).toLong()
        list += entity.copy(id = id)
        return id
    }

    override fun findByAppAndType(appPackage: String, taskType: String): List<PlaybookEntity> =
        playbooks.values.filter { it.appPackage == appPackage && it.taskType == taskType }

    override fun getPlaybook(playbookId: Long): PlaybookEntity? = playbooks[playbookId]

    override fun getSteps(playbookId: Long): List<PlaybookStepEntity> =
        steps[playbookId].orEmpty().sortedBy { it.stepOrder }

    override fun incrementSuccess(playbookId: Long) {
        val current = playbooks.getValue(playbookId)
        playbooks[playbookId] = current.copy(successCount = current.successCount + 1)
    }

    override fun incrementFail(playbookId: Long) {
        val current = playbooks.getValue(playbookId)
        playbooks[playbookId] = current.copy(failCount = current.failCount + 1)
    }

    override fun updateConfidence(playbookId: Long, confidence: Float) {
        val current = playbooks.getValue(playbookId)
        playbooks[playbookId] = current.copy(confidence = confidence)
    }

    override fun recordExecution(playbookId: Long, success: Boolean) {
        val current = playbooks.getValue(playbookId)
        val updated = if (success) {
            val successCount = current.successCount + 1
            current.copy(successCount = successCount, confidence = successCount.toFloat() / (successCount + current.failCount))
        } else {
            val failCount = current.failCount + 1
            current.copy(failCount = failCount, confidence = current.successCount.toFloat() / (current.successCount + failCount))
        }
        playbooks[playbookId] = updated
    }
}
