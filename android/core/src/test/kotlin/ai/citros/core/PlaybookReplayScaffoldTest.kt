package ai.citros.core

import android.graphics.Rect
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class PlaybookReplayScaffoldTest {
    @Test
    fun matcher_returnsHighestConfidenceCandidateAboveThreshold() {
        val dao = InMemoryPlaybookDao()
        val now = 1000L
        val low = dao.insertPlaybook(
            PlaybookEntity(appPackage = "com.msg", taskType = "send_message", confidence = 0.2f, createdAt = now, lastUsedAt = now)
        )
        dao.insertStep(step(low, 0))

        val good = dao.insertPlaybook(
            PlaybookEntity(appPackage = "com.msg", taskType = "send_message", confidence = 0.8f, successCount = 3, createdAt = now, lastUsedAt = now)
        )
        dao.insertStep(step(good, 0))

        val match = PlaybookMatcher(dao).findMatch("com.msg", "send_message")
        assertNotNull(match)
        assertEquals(good, match!!.playbook.id)
        assertEquals(1, match.steps.size)
    }

    @Test
    fun matcher_returnsNullWhenCandidatesBelowThreshold() {
        val dao = InMemoryPlaybookDao()
        val id = dao.insertPlaybook(
            PlaybookEntity(appPackage = "com.msg", taskType = "send_message", confidence = 0.1f, createdAt = 1L, lastUsedAt = 1L)
        )
        dao.insertStep(step(id, 0))

        val match = PlaybookMatcher(dao).findMatch("com.msg", "send_message")
        assertNull(match)
    }

    @Test
    fun executor_allMatched_returnsSuccess_andUpdatesConfidence() = runBlocking {
        val dao = InMemoryPlaybookDao()
        val id = dao.insertPlaybook(
            PlaybookEntity(
                appPackage = "com.msg",
                taskType = "send_message",
                successCount = 2,
                failCount = 0,
                confidence = 1f,
                createdAt = 1L,
                lastUsedAt = 1L
            )
        )

        val screen = stableScreen("com.msg")
        val fingerprint = ScreenFingerprinting.compute(screen).structuralHash
        dao.insertStep(
            PlaybookStepEntity(
                playbookId = id,
                stepOrder = 0,
                screenFingerprint = fingerprint,
                screenPackage = "com.msg",
                toolName = "tap_text",
                toolInputTemplate = "{\"text\":\"{recipient}\"}",
                selectorStrategy = "text_match",
                selectorValue = "{recipient}",
                expectedNextFingerprint = fingerprint,
                settleTimeMs = 0
            )
        )

        val match = PlaybookMatcher(dao).findMatch("com.msg", "send_message")!!
        val executor = PlaybookExecutor(
            toolRunner = object : PlaybookToolRunner {
                override fun execute(toolCall: ToolCall, screenContent: ScreenContent): ToolResult {
                    assertEquals("Mom", toolCall.input["text"])
                    return ToolResult("ok")
                }
            },
            screenReader = object : PlaybookScreenReader {
                override fun getScreenContent(): ScreenContent = screen
            },
            playbookDao = dao,
            llmFallback = object : PlaybookLlmFallback {
                override suspend fun executeSingleStep(screenContent: ScreenContent): ToolResult = ToolResult("fallback")
            },
            delayFn = {}
        )

        val result = executor.execute(match, mapOf("recipient" to "Mom"), neverCancelled())
        assertEquals(PlaybookStatus.SUCCESS, result.status)
        assertEquals(1, result.stepsCompleted)
        assertEquals(0, result.stepsDiverged)
        assertEquals(1.0f, dao.getPlaybook(id)!!.confidence, 0.0001f)
    }

    @Test
    fun executor_highDivergence_returnsAbandoned() = runBlocking {
        val dao = InMemoryPlaybookDao()
        val id = dao.insertPlaybook(
            PlaybookEntity(appPackage = "com.msg", taskType = "send_message", successCount = 1, confidence = 0.5f, createdAt = 1L, lastUsedAt = 1L)
        )
        repeat(3) { order ->
            dao.insertStep(
                PlaybookStepEntity(
                    playbookId = id,
                    stepOrder = order,
                    screenFingerprint = "mismatch-$order",
                    screenPackage = "com.msg",
                    toolName = "tap",
                    toolInputTemplate = "{}",
                    selectorStrategy = "text",
                    selectorValue = "x",
                    settleTimeMs = 0
                )
            )
        }

        val match = PlaybookMatcher(dao).findMatch("com.msg", "send_message")!!
        var fallbackCalls = 0
        val executor = PlaybookExecutor(
            toolRunner = object : PlaybookToolRunner {
                override fun execute(toolCall: ToolCall, screenContent: ScreenContent): ToolResult = ToolResult("ok")
            },
            screenReader = object : PlaybookScreenReader {
                override fun getScreenContent(): ScreenContent = stableScreen("com.msg")
            },
            playbookDao = dao,
            llmFallback = object : PlaybookLlmFallback {
                override suspend fun executeSingleStep(screenContent: ScreenContent): ToolResult {
                    fallbackCalls++
                    return ToolResult("fallback")
                }
            },
            delayFn = {}
        )

        val result = executor.execute(match, emptyMap(), neverCancelled())
        assertEquals(PlaybookStatus.ABANDONED, result.status)
        assertEquals(StepOutcome.SCREEN_MISMATCH, result.trace.first().outcome)
        assertTrue(fallbackCalls >= 2)
        assertTrue(dao.getPlaybook(id)!!.failCount >= 1)
    }

    @Test
    fun executor_cancelled_returnsCancelled() = runBlocking {
        val dao = InMemoryPlaybookDao()
        val id = dao.insertPlaybook(
            PlaybookEntity(appPackage = "com.msg", taskType = "send_message", confidence = 0.8f, createdAt = 1L, lastUsedAt = 1L)
        )
        dao.insertStep(step(id, 0).copy(screenFingerprint = ScreenFingerprinting.compute(stableScreen("com.msg")).structuralHash))
        val match = PlaybookMatcher(dao).findMatch("com.msg", "send_message")!!

        val result = PlaybookExecutor(
            toolRunner = object : PlaybookToolRunner {
                override fun execute(toolCall: ToolCall, screenContent: ScreenContent): ToolResult = ToolResult("ok")
            },
            screenReader = object : PlaybookScreenReader {
                override fun getScreenContent(): ScreenContent = stableScreen("com.msg")
            },
            playbookDao = dao,
            llmFallback = object : PlaybookLlmFallback {
                override suspend fun executeSingleStep(screenContent: ScreenContent): ToolResult = ToolResult("fallback")
            },
            delayFn = {}
        ).execute(match, emptyMap(), object : CancellationToken {
            override val isCancelled: Boolean = true
        })

        assertEquals(PlaybookStatus.CANCELLED, result.status)
        assertEquals(0, result.stepsCompleted)
        assertEquals(0, result.stepsDiverged)
    }

    @Test
    fun executor_partial_whenOnlySomeStepsDiverge() = runBlocking {
        val dao = InMemoryPlaybookDao()
        val id = dao.insertPlaybook(
            PlaybookEntity(appPackage = "com.msg", taskType = "send_message", confidence = 0.8f, createdAt = 1L, lastUsedAt = 1L)
        )
        val screen = stableScreen("com.msg")
        val fingerprint = ScreenFingerprinting.compute(screen).structuralHash
        dao.insertStep(step(id, 0).copy(screenFingerprint = fingerprint, expectedNextFingerprint = fingerprint, settleTimeMs = 0))
        dao.insertStep(step(id, 1).copy(screenFingerprint = fingerprint, toolInputTemplate = "{\"missing\":\"{name}\"}", settleTimeMs = 0))

        val match = PlaybookMatcher(dao).findMatch("com.msg", "send_message")!!
        val result = PlaybookExecutor(
            toolRunner = object : PlaybookToolRunner {
                override fun execute(toolCall: ToolCall, screenContent: ScreenContent): ToolResult = ToolResult("ok")
            },
            screenReader = object : PlaybookScreenReader {
                override fun getScreenContent(): ScreenContent = screen
            },
            playbookDao = dao,
            llmFallback = object : PlaybookLlmFallback {
                override suspend fun executeSingleStep(screenContent: ScreenContent): ToolResult = ToolResult("fallback")
            },
            delayFn = {}
        ).execute(match, emptyMap(), neverCancelled())

        assertEquals(PlaybookStatus.PARTIAL, result.status)
        assertEquals(1, result.stepsCompleted)
        assertEquals(1, result.stepsDiverged)
        assertEquals(StepOutcome.PARAM_UNRESOLVED, result.trace.last().outcome)
    }


    @Test
    fun executor_resolvesNestedTemplateFields() = runBlocking {
        val dao = InMemoryPlaybookDao()
        val id = dao.insertPlaybook(
            PlaybookEntity(appPackage = "com.msg", taskType = "send_message", confidence = 0.8f, createdAt = 1L, lastUsedAt = 1L)
        )
        val screen = stableScreen("com.msg")
        val fingerprint = ScreenFingerprinting.compute(screen).structuralHash
        dao.insertStep(
            step(id, 0).copy(
                screenFingerprint = fingerprint,
                toolInputTemplate = "{\"meta\":{\"recipient\":\"{recipient}\"},\"tags\":[\"{recipient}\"]}",
                expectedNextFingerprint = fingerprint,
                settleTimeMs = 0
            )
        )

        val match = PlaybookMatcher(dao).findMatch("com.msg", "send_message")!!
        val capturedInputs = mutableListOf<Map<String, Any>>()
        val result = PlaybookExecutor(
            toolRunner = object : PlaybookToolRunner {
                override fun execute(toolCall: ToolCall, screenContent: ScreenContent): ToolResult {
                    capturedInputs += toolCall.input
                    return ToolResult("ok")
                }
            },
            screenReader = object : PlaybookScreenReader {
                override fun getScreenContent(): ScreenContent = screen
            },
            playbookDao = dao,
            llmFallback = object : PlaybookLlmFallback {
                override suspend fun executeSingleStep(screenContent: ScreenContent): ToolResult = ToolResult("fallback")
            },
            delayFn = {}
        ).execute(match, mapOf("recipient" to "Mom"), neverCancelled())

        assertEquals(PlaybookStatus.SUCCESS, result.status)
        val meta = capturedInputs.single()["meta"] as Map<*, *>
        assertEquals("Mom", meta["recipient"])
        val tags = capturedInputs.single()["tags"] as List<*>
        assertEquals("Mom", tags.single())
    }


    @Test
    fun executor_cancelledBeforeFirstStep_returnsCancelled() = runBlocking {
        val dao = InMemoryPlaybookDao()
        val id = dao.insertPlaybook(
            PlaybookEntity(appPackage = "com.msg", taskType = "send_message", createdAt = 1L, lastUsedAt = 1L)
        )
        val screen = stableScreen("com.msg")
        val fingerprint = ScreenFingerprinting.compute(screen).structuralHash
        dao.insertStep(
            PlaybookStepEntity(
                playbookId = id,
                stepOrder = 0,
                screenFingerprint = fingerprint,
                screenPackage = "com.msg",
                toolName = "tap",
                toolInputTemplate = "{}",
                selectorStrategy = "text",
                selectorValue = "Send",
                settleTimeMs = 0
            )
        )

        var toolCalls = 0
        val result = PlaybookExecutor(
            toolRunner = object : PlaybookToolRunner {
                override fun execute(toolCall: ToolCall, screenContent: ScreenContent): ToolResult {
                    toolCalls++
                    return ToolResult("ok")
                }
            },
            screenReader = object : PlaybookScreenReader {
                override fun getScreenContent(): ScreenContent = screen
            },
            playbookDao = dao,
            llmFallback = object : PlaybookLlmFallback {
                override suspend fun executeSingleStep(screenContent: ScreenContent): ToolResult = ToolResult("fallback")
            }
        ).execute(
            PlaybookMatcher(dao).findMatch("com.msg", "send_message")!!,
            emptyMap(),
            object : CancellationToken { override val isCancelled = true }
        )

        val playbookAfter = dao.getPlaybook(id)!!
        assertEquals(PlaybookStatus.CANCELLED, result.status)
        assertEquals(0, toolCalls)
        assertEquals(0.5f, playbookAfter.confidence, 0.0001f)
        assertEquals(1L, playbookAfter.lastUsedAt)
    }

    @Test
    fun executor_missingParameter_returnsPartial_andSignalsUnresolvedTrace() = runBlocking {
        val dao = InMemoryPlaybookDao()
        val id = dao.insertPlaybook(
            PlaybookEntity(appPackage = "com.msg", taskType = "send_message", createdAt = 1L, lastUsedAt = 1L)
        )
        val screen = stableScreen("com.msg")
        val fingerprint = ScreenFingerprinting.compute(screen).structuralHash
        dao.insertStep(
            PlaybookStepEntity(
                playbookId = id,
                stepOrder = 0,
                screenFingerprint = fingerprint,
                screenPackage = "com.msg",
                toolName = "tap_text",
                toolInputTemplate = "{\"text\":\"{recipient}\"}",
                selectorStrategy = "text",
                selectorValue = "Send",
                settleTimeMs = 0
            )
        )

        var fallbackCalls = 0
        val result = PlaybookExecutor(
            toolRunner = object : PlaybookToolRunner {
                override fun execute(toolCall: ToolCall, screenContent: ScreenContent): ToolResult = ToolResult("ok")
            },
            screenReader = object : PlaybookScreenReader {
                override fun getScreenContent(): ScreenContent = screen
            },
            playbookDao = dao,
            llmFallback = object : PlaybookLlmFallback {
                override suspend fun executeSingleStep(screenContent: ScreenContent): ToolResult {
                    fallbackCalls++
                    return ToolResult("fallback")
                }
            }
        ).execute(
            PlaybookMatcher(dao).findMatch("com.msg", "send_message")!!,
            emptyMap(),
            neverCancelled()
        )

        assertEquals(PlaybookStatus.PARTIAL, result.status)
        assertEquals(1, fallbackCalls)
        assertEquals(StepOutcome.PARAM_UNRESOLVED, result.trace.single().outcome)
        assertEquals(1, dao.getPlaybook(id)!!.failCount)
    }

    @Test
    fun executor_invalidTemplateJsonObject_throwsHelpfulError() = runBlocking {
        val dao = InMemoryPlaybookDao()
        val id = dao.insertPlaybook(
            PlaybookEntity(appPackage = "com.msg", taskType = "send_message", createdAt = 1L, lastUsedAt = 1L)
        )
        val screen = stableScreen("com.msg")
        val fingerprint = ScreenFingerprinting.compute(screen).structuralHash
        dao.insertStep(
            PlaybookStepEntity(
                playbookId = id,
                stepOrder = 0,
                screenFingerprint = fingerprint,
                screenPackage = "com.msg",
                toolName = "tap",
                toolInputTemplate = "[]",
                selectorStrategy = "text",
                selectorValue = "Send",
                settleTimeMs = 0
            )
        )

        try {
            PlaybookExecutor(
                toolRunner = object : PlaybookToolRunner {
                    override fun execute(toolCall: ToolCall, screenContent: ScreenContent): ToolResult = ToolResult("ok")
                },
                screenReader = object : PlaybookScreenReader {
                    override fun getScreenContent(): ScreenContent = screen
                },
                playbookDao = dao,
                llmFallback = object : PlaybookLlmFallback {
                    override suspend fun executeSingleStep(screenContent: ScreenContent): ToolResult = ToolResult("fallback")
                }
            ).execute(
                PlaybookMatcher(dao).findMatch("com.msg", "send_message")!!,
                emptyMap(),
                neverCancelled()
            )
            throw AssertionError("Expected IllegalArgumentException")
        } catch (ex: IllegalArgumentException) {
            assertTrue(ex.message!!.contains("JSON object"))
        }
    }

    @Test
    fun executor_screenMismatch_recordsScreenMismatchOutcome() = runBlocking {
        val dao = InMemoryPlaybookDao()
        val id = dao.insertPlaybook(
            PlaybookEntity(appPackage = "com.msg", taskType = "send_message", createdAt = 1L, lastUsedAt = 1L)
        )
        dao.insertStep(
            PlaybookStepEntity(
                playbookId = id,
                stepOrder = 0,
                screenFingerprint = "expected-different-fingerprint",
                screenPackage = "com.msg",
                toolName = "tap",
                toolInputTemplate = "{}",
                selectorStrategy = "text",
                selectorValue = "Send",
                settleTimeMs = 0
            )
        )

        var fallbackCalls = 0
        val result = PlaybookExecutor(
            toolRunner = object : PlaybookToolRunner {
                override fun execute(toolCall: ToolCall, screenContent: ScreenContent): ToolResult = ToolResult("ok")
            },
            screenReader = object : PlaybookScreenReader {
                override fun getScreenContent(): ScreenContent = stableScreen("com.msg")
            },
            playbookDao = dao,
            llmFallback = object : PlaybookLlmFallback {
                override suspend fun executeSingleStep(screenContent: ScreenContent): ToolResult {
                    fallbackCalls++
                    return ToolResult("fallback")
                }
            },
            delayFn = {}
        ).execute(PlaybookMatcher(dao).findMatch("com.msg", "send_message")!!, emptyMap(), neverCancelled())

        assertEquals(1, fallbackCalls)
        assertEquals(StepOutcome.SCREEN_MISMATCH, result.trace.single().outcome)
    }

    @Test
    fun executor_usesInjectedDelayFunction() = runBlocking {
        val dao = InMemoryPlaybookDao()
        val id = dao.insertPlaybook(
            PlaybookEntity(appPackage = "com.msg", taskType = "send_message", confidence = 0.8f, createdAt = 1L, lastUsedAt = 1L)
        )
        val screen = stableScreen("com.msg")
        val fingerprint = ScreenFingerprinting.compute(screen).structuralHash
        dao.insertStep(
            step(id, 0).copy(
                screenFingerprint = fingerprint,
                expectedNextFingerprint = fingerprint,
                settleTimeMs = 321
            )
        )

        val delayed = mutableListOf<Long>()
        PlaybookExecutor(
            toolRunner = object : PlaybookToolRunner {
                override fun execute(toolCall: ToolCall, screenContent: ScreenContent): ToolResult = ToolResult("ok")
            },
            screenReader = object : PlaybookScreenReader {
                override fun getScreenContent(): ScreenContent = screen
            },
            playbookDao = dao,
            llmFallback = object : PlaybookLlmFallback {
                override suspend fun executeSingleStep(screenContent: ScreenContent): ToolResult = ToolResult("fallback")
            },
            delayFn = { delayed += it }
        ).execute(PlaybookMatcher(dao).findMatch("com.msg", "send_message")!!, emptyMap(), neverCancelled())

        assertEquals(listOf(321L), delayed)
    }

    private fun step(playbookId: Long, order: Int) = PlaybookStepEntity(
        playbookId = playbookId,
        stepOrder = order,
        screenFingerprint = "fp$order",
        toolName = "tap",
        toolInputTemplate = "{}",
        selectorStrategy = "text",
        selectorValue = "Send"
    )

    private fun stableScreen(pkg: String) = ScreenContent(
        elements = listOf(
            ScreenElement(
                id = 1,
                text = "Mom",
                contentDescription = null,
                className = "android.widget.TextView",
                isClickable = true,
                isEditable = false,
                bounds = Rect(0, 0, 100, 40),
                depth = 1
            )
        ),
        packageName = pkg
    )

    private fun neverCancelled() = object : CancellationToken {
        override val isCancelled: Boolean = false
    }
}

private class InMemoryPlaybookDao : PlaybookDao {
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
        val id = list.size.toLong() + 1
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
        val now = System.currentTimeMillis()
        val updated = if (success) {
            // Mirrors SQL behavior with the same initial-state assumptions (default successCount=1, failCount=0).
            val successCount = current.successCount + 1
            val confidence = successCount.toFloat() / (successCount + current.failCount)
            current.copy(
                successCount = successCount,
                confidence = confidence,
                lastUsedAt = now,
                lastSucceededAt = now
            )
        } else {
            val failCount = current.failCount + 1
            val confidence = current.successCount.toFloat() / (current.successCount + failCount)
            current.copy(
                failCount = failCount,
                confidence = confidence,
                lastUsedAt = now
            )
        }
        playbooks[playbookId] = updated
    }
}
