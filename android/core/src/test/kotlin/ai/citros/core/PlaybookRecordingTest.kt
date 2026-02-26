package ai.citros.core

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PlaybookRecordingTest {
    @Test
    fun resolveTemplate_allParamsFilled_marksComplete() {
        val extractor = ParameterExtractor()

        val resolved = extractor.resolveTemplate(
            template = mapOf("text" to "{recipient}", "app_name" to "Messages"),
            parameters = mapOf("recipient" to "Mom")
        )

        assertTrue(resolved.isComplete)
        assertEquals("Mom", resolved.inputs["text"])
        assertEquals("Messages", resolved.inputs["app_name"])
    }

    @Test
    fun resolveTemplate_missingParam_marksIncomplete() {
        val extractor = ParameterExtractor()

        val resolved = extractor.resolveTemplate(
            template = mapOf("text" to "{recipient}"),
            parameters = emptyMap()
        )

        assertFalse(resolved.isComplete)
        assertEquals(listOf("recipient"), resolved.unresolvedParams)
    }

    @Test
    fun resolveTemplate_embeddedPlaceholders_replacesWithinText() {
        val extractor = ParameterExtractor()

        val resolved = extractor.resolveTemplate(
            template = mapOf("text" to "Hello {recipient}, from {sender}"),
            parameters = mapOf("recipient" to "Mom", "sender" to "Joe")
        )

        assertTrue(resolved.isComplete)
        assertEquals("Hello Mom, from Joe", resolved.inputs["text"])
    }

    @Test
    fun resolveTemplate_nestedMapsAndLists_replacesRecursively() {
        val extractor = ParameterExtractor()

        val resolved = extractor.resolveTemplate(
            template = mapOf(
                "outer" to mapOf(
                    "title" to "Hi {recipient}",
                    "items" to listOf("{message}", mapOf("author" to "{sender}"), 42)
                )
            ),
            parameters = mapOf("recipient" to "Mom", "message" to "On my way", "sender" to "Joe")
        )

        assertTrue(resolved.isComplete)
        val outer = resolved.inputs["outer"] as Map<*, *>
        assertEquals("Hi Mom", outer["title"])
        val items = outer["items"] as List<*>
        assertEquals("On my way", items[0])
        assertEquals("Joe", (items[1] as Map<*, *>)["author"])
        assertEquals(42, items[2])
    }

    @Test
    fun resolveTemplate_nestedMissingParam_marksIncomplete() {
        val extractor = ParameterExtractor()

        val resolved = extractor.resolveTemplate(
            template = mapOf("payload" to listOf(mapOf("text" to "{missing}"))),
            parameters = emptyMap()
        )

        assertFalse(resolved.isComplete)
        assertEquals(listOf("missing"), resolved.unresolvedParams)
    }

    @Test
    fun extract_sendTextToMom_extractsRecipientAndMessage() {
        val extractor = ParameterExtractor()
        val steps = listOf(
            RecordedStep(
                toolCall = ToolCall("1", "tap_text", mapOf("text" to "Mom")),
                screenBefore = null,
                screenAfter = null,
                result = ToolResult("ok")
            ),
            RecordedStep(
                toolCall = ToolCall("2", "type_text", mapOf("text" to "I'll be late")),
                screenBefore = null,
                screenAfter = null,
                result = ToolResult("ok")
            )
        )

        val extracted = extractor.extract("Send a text to Mom saying I'll be late", steps)

        assertEquals("send_message", extracted.taskType)
        assertTrue(extracted.parameters.containsKey("recipient"))
        assertTrue(extracted.parameters.containsKey("message"))
    }

    @Test
    fun extract_usesLlmEntityExtractorContract() {
        val extractor = ParameterExtractor(
            entityExtractor = LlmEntityExtractor(
                client = LlmEntityExtractionClient { _ ->
                    Result.success(
                        listOf(
                            Entity(text = "Mom", label = "recipient"),
                            Entity(text = "I'll be late", label = "message")
                        )
                    )
                }
            )
        )

        val steps = listOf(
            RecordedStep(ToolCall("1", "tap_text", mapOf("text" to "Mom")), null, null, ToolResult("ok")),
            RecordedStep(ToolCall("2", "type_text", mapOf("text" to "I'll be late")), null, null, ToolResult("ok"))
        )

        val extracted = extractor.extract("Send a text to Mom saying I'll be late", steps)

        assertTrue(extracted.parameters.containsKey("recipient"))
        assertTrue(extracted.parameters.containsKey("message"))
    }

    @Test
    fun templatize_replacesOnlyExactMatch() {
        val extracted = ExtractedParameters(
            taskType = "send_message",
            parameters = mapOf(
                "message" to ParameterDef(
                    name = "message",
                    type = "string",
                    sourceField = "text",
                    exampleValue = "I'll be late"
                )
            ),
            schemaJson = "{}"
        )

        val unchanged = extracted.templatize(mapOf("text" to "I'll be late tonight"))
        val replaced = extracted.templatize(mapOf("text" to "I'll be late"))

        assertEquals("I'll be late tonight", unchanged["text"])
        assertEquals("{message}", replaced["text"])
    }

    @Test
    fun templatize_caseMismatch_doesNotReplace() {
        val extracted = ExtractedParameters(
            taskType = "send_message",
            parameters = mapOf(
                "recipient" to ParameterDef(
                    name = "recipient",
                    type = "string",
                    sourceField = "text",
                    exampleValue = "mom"
                )
            ),
            schemaJson = "{}"
        )

        val unchanged = extracted.templatize(mapOf("text" to "MOM"))

        assertEquals("MOM", unchanged["text"])
    }

    @Test
    fun providerClientEntityExtractionClient_parsesPlainJson() {
        val client = ProviderClientEntityExtractionClient(
            providerClient = FakeProviderClient("""{"entities":[{"text":"Mom","label":"recipient"}]}""")
        )

        val result = client.extractEntities("Send a message to Mom")

        assertTrue(result.isSuccess)
        assertEquals(listOf(Entity(text = "Mom", label = "recipient")), result.getOrThrow())
    }

    @Test
    fun providerClientEntityExtractionClient_parsesCodeFencedJson() {
        val client = ProviderClientEntityExtractionClient(
            providerClient = FakeProviderClient(
                """
                ```json
                {"entities":[{"text":"Mom","label":"recipient"}]}
                ```
                """.trimIndent()
            )
        )

        val result = client.extractEntities("Send a message to Mom")

        assertTrue(result.isSuccess)
        assertEquals(listOf(Entity(text = "Mom", label = "recipient")), result.getOrThrow())
    }

    @Test
    fun providerClientEntityExtractionClient_parsesCodeFenceWithoutLanguage() {
        val client = ProviderClientEntityExtractionClient(
            providerClient = FakeProviderClient(
                """
                ```
                {"entities":[{"text":"Mom","label":"recipient"}]}
                ```
                """.trimIndent()
            )
        )

        val result = client.extractEntities("Send a message to Mom")

        assertTrue(result.isSuccess)
        assertEquals(listOf(Entity(text = "Mom", label = "recipient")), result.getOrThrow())
    }

    @Test
    fun providerClientEntityExtractionClient_returnsFailureOnMalformedJson() {
        val client = ProviderClientEntityExtractionClient(
            providerClient = FakeProviderClient("{not-json")
        )

        val result = client.extractEntities("Send a message to Mom")

        assertTrue(result.isFailure)
    }

    @Test
    fun stripJsonCodeFence_handlesExpectedFormats() {
        assertEquals("{\"a\":1}", stripJsonCodeFence("{\"a\":1}"))
        assertEquals("{\"a\":1}", stripJsonCodeFence("```json\n{\"a\":1}\n```"))
        assertEquals("{\"a\":1}", stripJsonCodeFence("```\n{\"a\":1}\n```"))
        assertEquals("already-clean", stripJsonCodeFence("  already-clean  "))
    }

    @Test
    fun recorder_onTaskCompletedCalledConcurrently_persistsAtMostOnce() = runBlocking {
        val dao = FakePlaybookDao()
        val recorder = ExecutionRecorder(dao, ParameterExtractor(), nowMs = { 1234L })

        recorder.onTaskStarted("Send a text to Mom saying hello")
        coroutineScope {
            repeat(8) { idx ->
                launch(Dispatchers.Default) {
                    recorder.onToolExecuted(
                        toolCall = ToolCall("$idx", "tap_text", mapOf("text" to "Mom")),
                        screenBefore = ScreenFingerprint("a$idx", "com.messages"),
                        screenAfter = ScreenFingerprint("b$idx", "com.messages"),
                        result = ToolResult("ok"),
                        failure = null
                    )
                }
            }
        }

        coroutineScope {
            repeat(2) {
                launch(Dispatchers.Default) {
                    recorder.onTaskCompleted(TaskStatus.COMPLETED, "Sent")
                }
            }
        }

        assertEquals(1, dao.playbooks.size)
        assertEquals(8, dao.steps.size)
    }

    @Test
    fun classify_openGmail_mapsToOpenApp() {
        val extractor = ParameterExtractor()

        val extracted = extractor.extract(
            userMessage = "Open Gmail",
            steps = listOf(
                RecordedStep(
                    toolCall = ToolCall("1", "open_app", mapOf("app_name" to "Gmail")),
                    screenBefore = null,
                    screenAfter = null,
                    result = ToolResult("ok")
                )
            )
        )

        assertEquals("open_app", extracted.taskType)
    }

    @Test
    fun recorder_successfulTaskWithInteractiveSteps_persistsPlaybookAndSteps() {
        val dao = FakePlaybookDao()
        val recorder = ExecutionRecorder(dao, ParameterExtractor(), nowMs = { 1234L })

        recorder.onTaskStarted("Send a text to Mom saying hello")
        recorder.onToolExecuted(
            toolCall = ToolCall("1", "tap_text", mapOf("text" to "Mom")),
            screenBefore = ScreenFingerprint("a", "com.messages"),
            screenAfter = ScreenFingerprint("b", "com.messages"),
            result = ToolResult("ok"),
            failure = null
        )
        recorder.onToolExecuted(
            toolCall = ToolCall("2", "type_text", mapOf("text" to "hello")),
            screenBefore = ScreenFingerprint("b", "com.messages"),
            screenAfter = ScreenFingerprint("c", "com.messages"),
            result = ToolResult("ok"),
            failure = null
        )
        recorder.onToolExecuted(
            toolCall = ToolCall("3", "tap_text", mapOf("text" to "Send")),
            screenBefore = ScreenFingerprint("c", "com.messages"),
            screenAfter = ScreenFingerprint("d", "com.messages"),
            result = ToolResult("ok"),
            failure = null
        )

        recorder.onTaskCompleted(TaskStatus.COMPLETED, "Sent")

        assertEquals(1, dao.playbooks.size)
        assertEquals(3, dao.steps.size)
        assertEquals("com.messages", dao.playbooks.single().appPackage)
        assertEquals("send_message", dao.playbooks.single().taskType)
        assertEquals(listOf(0, 1, 2), dao.steps.map { it.stepOrder })

        val step1Template = Json.parseToJsonElement(dao.steps[0].toolInputTemplate).jsonObject
        val step2Template = Json.parseToJsonElement(dao.steps[1].toolInputTemplate).jsonObject
        assertEquals("{recipient}", step1Template["text"]?.jsonPrimitive?.content)
        assertEquals("{message}", step2Template["text"]?.jsonPrimitive?.content)
    }

    @Test
    fun recorder_inferSelector_usesPriorityAndFallback() {
        val dao = FakePlaybookDao()
        val recorder = ExecutionRecorder(dao, ParameterExtractor(), nowMs = { 1234L })

        recorder.onTaskStarted("Do thing")
        recorder.onToolExecuted(
            toolCall = ToolCall("1", "tap", mapOf("text" to "Inbox", "resource_id" to "id/inbox", "content_description" to "Inbox")),
            screenBefore = ScreenFingerprint("a", "com.messages"),
            screenAfter = ScreenFingerprint("b", "com.messages"),
            result = ToolResult("ok"),
            failure = null
        )
        recorder.onToolExecuted(
            toolCall = ToolCall("2", "tap", mapOf("resource_id" to "id/send", "content_description" to "Send")),
            screenBefore = ScreenFingerprint("b", "com.messages"),
            screenAfter = ScreenFingerprint("c", "com.messages"),
            result = ToolResult("ok"),
            failure = null
        )
        recorder.onToolExecuted(
            toolCall = ToolCall("3", "tap", mapOf("content_description" to "Compose")),
            screenBefore = ScreenFingerprint("c", "com.messages"),
            screenAfter = ScreenFingerprint("d", "com.messages"),
            result = ToolResult("ok"),
            failure = null
        )
        recorder.onToolExecuted(
            toolCall = ToolCall("4", "tap", emptyMap()),
            screenBefore = ScreenFingerprint("d", "com.messages"),
            screenAfter = ScreenFingerprint("e", "com.messages"),
            result = ToolResult("ok"),
            failure = null
        )

        recorder.onTaskCompleted(TaskStatus.COMPLETED, "done")

        assertEquals("text_match", dao.steps[0].selectorStrategy)
        assertEquals("Inbox", dao.steps[0].selectorValue)
        assertEquals("resource_id", dao.steps[1].selectorStrategy)
        assertEquals("id/send", dao.steps[1].selectorValue)
        assertEquals("content_desc", dao.steps[2].selectorStrategy)
        assertEquals("Compose", dao.steps[2].selectorValue)
        assertEquals("none", dao.steps[3].selectorStrategy)
        assertEquals("", dao.steps[3].selectorValue)
    }

    @Test
    fun recorder_failedSteps_areFilteredOutOfPersistedRecording() {
        val dao = FakePlaybookDao()
        val recorder = ExecutionRecorder(dao, ParameterExtractor(), nowMs = { 1234L })

        recorder.onTaskStarted("Send a text to Mom saying hello")
        recorder.onToolExecuted(
            toolCall = ToolCall("1", "tap_text", mapOf("text" to "Mom")),
            screenBefore = ScreenFingerprint("a", "com.messages"),
            screenAfter = ScreenFingerprint("b", "com.messages"),
            result = ToolResult("ok"),
            failure = null
        )
        val failedToolCall = ToolCall("2", "tap_text", mapOf("text" to "FAILED"))
        recorder.onToolExecuted(
            toolCall = failedToolCall,
            screenBefore = ScreenFingerprint("b", "com.messages"),
            screenAfter = ScreenFingerprint("c", "com.messages"),
            result = ToolResult("error"),
            failure = ActionFailure(
                toolCall = failedToolCall,
                result = ToolResult("error"),
                screenBefore = ScreenFingerprint("b", "com.messages"),
                screenAfter = ScreenFingerprint("c", "com.messages"),
                consecutiveFailures = 1,
                foregroundApp = "com.messages",
                failureType = FailureType.TOOL_ERROR
            )
        )
        recorder.onToolExecuted(
            toolCall = ToolCall("3", "type_text", mapOf("text" to "hello")),
            screenBefore = ScreenFingerprint("c", "com.messages"),
            screenAfter = ScreenFingerprint("d", "com.messages"),
            result = ToolResult("ok"),
            failure = null
        )
        recorder.onToolExecuted(
            toolCall = ToolCall("4", "tap_text", mapOf("text" to "Send")),
            screenBefore = ScreenFingerprint("d", "com.messages"),
            screenAfter = ScreenFingerprint("e", "com.messages"),
            result = ToolResult("ok"),
            failure = null
        )

        recorder.onTaskCompleted(TaskStatus.COMPLETED, "Sent")

        assertEquals(1, dao.playbooks.size)
        assertEquals(3, dao.steps.size)
        val persistedInputs = dao.steps.map { Json.parseToJsonElement(it.toolInputTemplate).toString() }
        assertTrue(persistedInputs.none { it.contains("FAILED") })
    }

    @Test
    fun recorder_failedTask_doesNotPersistPlaybook() {
        val dao = FakePlaybookDao()
        val recorder = ExecutionRecorder(dao, ParameterExtractor())
        recorder.onTaskStarted("Send a text to Mom")
        repeat(3) {
            recorder.onToolExecuted(
                toolCall = ToolCall("$it", "tap_text", mapOf("text" to "Mom")),
                screenBefore = ScreenFingerprint("a", "com.messages"),
                screenAfter = ScreenFingerprint("b", "com.messages"),
                result = ToolResult("ok"),
                failure = null
            )
        }

        recorder.onTaskCompleted(TaskStatus.FAILED, "failed")

        assertTrue(dao.playbooks.isEmpty())
    }

    @Test
    fun recorder_shortTask_doesNotPersistPlaybook() {
        val dao = FakePlaybookDao()
        val recorder = ExecutionRecorder(dao, ParameterExtractor())
        recorder.onTaskStarted("Send")
        repeat(2) {
            recorder.onToolExecuted(
                toolCall = ToolCall("$it", "tap_text", mapOf("text" to "Mom")),
                screenBefore = ScreenFingerprint("a", "com.messages"),
                screenAfter = ScreenFingerprint("b", "com.messages"),
                result = ToolResult("ok"),
                failure = null
            )
        }

        recorder.onTaskCompleted(TaskStatus.COMPLETED, "ok")

        assertTrue(dao.playbooks.isEmpty())
    }

    private class FakeProviderClient(
        private val response: String
    ) : ProviderClient {
        override val provider: Provider = Provider.ANTHROPIC

        override suspend fun chat(conversation: Conversation): Result<String> = Result.success(response)

        override suspend fun chatWithTools(
            messages: List<Message>,
            systemPrompt: String?,
            tools: List<Tool>,
            tokenLimit: Int?
        ): Result<ChatResponse> = error("unused in PlaybookRecordingTest")

        override suspend fun describeImage(
            base64Image: String,
            prompt: String,
            maxTokens: Int
        ): Result<String> = error("unused in PlaybookRecordingTest")
    }

    private class FakePlaybookDao : PlaybookDao {
        val playbooks = mutableListOf<PlaybookEntity>()
        val steps = mutableListOf<PlaybookStepEntity>()
        private var nextId = 1L

        override fun insertPlaybook(entity: PlaybookEntity): Long {
            val id = nextId++
            playbooks += entity.copy(id = id)
            return id
        }

        override fun insertStep(entity: PlaybookStepEntity): Long {
            val id = nextId++
            steps += entity.copy(id = id)
            return id
        }

        override fun findByAppAndType(appPackage: String, taskType: String): List<PlaybookEntity> =
            playbooks.filter { it.appPackage == appPackage && it.taskType == taskType }

        override fun getPlaybook(playbookId: Long): PlaybookEntity? =
            playbooks.firstOrNull { it.id == playbookId }

        override fun getSteps(playbookId: Long): List<PlaybookStepEntity> =
            steps.filter { it.playbookId == playbookId }.sortedBy { it.stepOrder }

        override fun incrementSuccess(playbookId: Long) {
            // No-op for recording tests.
        }

        override fun incrementFail(playbookId: Long) {
            // No-op for recording tests.
        }

        override fun updateConfidence(playbookId: Long, confidence: Float) {
            // No-op for recording tests.
        }

        override fun recordExecution(playbookId: Long, success: Boolean) {
            // No-op for recording tests.
        }
    }
}
