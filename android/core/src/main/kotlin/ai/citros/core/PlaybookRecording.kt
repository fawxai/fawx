package ai.citros.core

import java.util.Collections
import java.util.concurrent.atomic.AtomicBoolean
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

private val PLAYBOOK_JSON = Json { encodeDefaults = true }

interface AgentExecutionListener {
    fun onTaskStarted(userMessage: String)
    fun onToolExecuted(
        toolCall: ToolCall,
        screenBefore: ScreenFingerprint?,
        screenAfter: ScreenFingerprint?,
        result: ToolResult,
        failure: ActionFailure?
    )

    fun onTaskCompleted(status: TaskStatus, finalResponse: String)
}

data class RecordedStep(
    val toolCall: ToolCall,
    val screenBefore: ScreenFingerprint?,
    val screenAfter: ScreenFingerprint?,
    val result: ToolResult
)

data class RecordingResolvedTemplate(
    val inputs: Map<String, Any>,
    val unresolvedParams: List<String>
) {
    val isComplete: Boolean get() = unresolvedParams.isEmpty()
}

data class Entity(val text: String, val label: String?)

data class ParameterDef(
    val name: String,
    val type: String,
    val sourceField: String,
    val exampleValue: String
)

data class ExtractedParameters(
    val taskType: String,
    val parameters: Map<String, ParameterDef>,
    val schemaJson: String
) {
    fun templatize(input: Map<String, Any>): Map<String, Any> {
        val result = input.toMutableMap()
        for ((paramName, paramDef) in parameters) {
            val current = result[paramDef.sourceField]
            if (current is String && current.equals(paramDef.exampleValue, ignoreCase = true)) {
                result[paramDef.sourceField] = "{$paramName}"
            }
        }
        return result
    }
}

fun interface EntityExtractor {
    fun extract(message: String): List<Entity>
}

class HeuristicEntityExtractor : EntityExtractor {
    override fun extract(message: String): List<Entity> {
        val entities = mutableListOf<Entity>()

        Regex("\"([^\"]+)\"|'([^']+)'").findAll(message).forEach { match ->
            val text = match.groupValues[1].ifEmpty { match.groupValues[2] }
            entities += Entity(text = text, label = null)
        }

        Regex("\\bto\\s+(\\w+)", RegexOption.IGNORE_CASE).find(message)?.let { match ->
            entities += Entity(text = match.groupValues[1], label = "recipient")
        }

        Regex("\\b(?:saying|that says?)\\s+(.+)", RegexOption.IGNORE_CASE).find(message)?.let { match ->
            entities += Entity(text = match.groupValues[1].trimEnd('.', '!', '?'), label = "message")
        }

        return entities
    }
}

fun interface LlmEntityExtractionClient {
    fun extractEntities(message: String): Result<List<Entity>>
}

class ProviderClientEntityExtractionClient(
    private val providerClient: ProviderClient
) : LlmEntityExtractionClient {
    override fun extractEntities(message: String): Result<List<Entity>> = runCatching {
        val prompt = """
            Extract user entities for parameterized phone automation.
            Return strict JSON only:
            {"entities":[{"text":"...","label":"recipient|message|null"}]}
            Preserve original text spans when possible.

            User message: $message
        """.trimIndent()

        val response = runBlocking {
            providerClient.chat(Conversation().apply { addUser(prompt) })
        }.getOrThrow()

        val payload = PLAYBOOK_JSON.parseToJsonElement(response.trim().removeJsonCodeFence()).jsonObject
        payload["entities"]?.jsonArray.orEmpty().mapNotNull { element ->
            val obj = element.jsonObject
            val text = obj["text"]?.jsonPrimitive?.content?.trim().orEmpty()
            if (text.isBlank()) return@mapNotNull null
            val label = obj["label"]?.jsonPrimitive?.contentOrNull?.takeIf { it.isNotBlank() }
            Entity(text = text, label = label)
        }
    }
}

object UnconfiguredEntityExtractionClient : LlmEntityExtractionClient {
    override fun extractEntities(message: String): Result<List<Entity>> =
        Result.failure(IllegalStateException("LLM entity extraction client is not configured"))
}

class LlmEntityExtractor(
    private val client: LlmEntityExtractionClient,
    private val fallback: EntityExtractor = HeuristicEntityExtractor()
) : EntityExtractor {
    override fun extract(message: String): List<Entity> {
        return client.extractEntities(message)
            .getOrElse { fallback.extract(message) }
            .filter { it.text.isNotBlank() }
    }
}

class ParameterExtractor(
    private val entityExtractor: EntityExtractor = LlmEntityExtractor(UnconfiguredEntityExtractionClient)
) {
    fun extract(userMessage: String, steps: List<RecordedStep>): ExtractedParameters {
        val taskType = classifyTaskType(userMessage, steps)
        val entities = entityExtractor.extract(userMessage)
        val paramMap = linkedMapOf<String, ParameterDef>()

        for (entity in entities) {
            for (step in steps) {
                for ((key, value) in step.toolCall.input) {
                    if (value is String && value.contains(entity.text, ignoreCase = true)) {
                        val paramName = entity.label ?: "param_${paramMap.size}"
                        paramMap.putIfAbsent(
                            paramName,
                            ParameterDef(
                                name = paramName,
                                type = "string",
                                sourceField = key,
                                exampleValue = entity.text
                            )
                        )
                    }
                }
            }
        }

        return ExtractedParameters(
            taskType = taskType,
            parameters = paramMap,
            schemaJson = buildSchemaJson(paramMap)
        )
    }

    fun resolveTemplate(template: Map<String, Any>, parameters: Map<String, String>): RecordingResolvedTemplate {
        val unresolved = linkedSetOf<String>()

        fun resolveAny(value: Any): Any = when (value) {
            is String -> resolveString(value, parameters, unresolved)
            is Map<*, *> -> value.entries.associate { (k, v) ->
                k.toString() to (v?.let { resolveAny(it) } ?: "")
            }
            is List<*> -> value.map { item -> item?.let { resolveAny(it) } ?: "" }
            else -> value
        }

        val resolved = template.entries.associate { (key, value) -> key to resolveAny(value) }
        return RecordingResolvedTemplate(inputs = resolved, unresolvedParams = unresolved.toList())
    }

    private fun resolveString(
        template: String,
        parameters: Map<String, String>,
        unresolved: MutableSet<String>
    ): String {
        val tokenRegex = Regex("\\{([^{}]+)}")
        return tokenRegex.replace(template) { match ->
            val paramName = match.groupValues[1]
            val paramValue = parameters[paramName]
            if (paramValue != null) {
                paramValue
            } else {
                unresolved += paramName
                match.value
            }
        }
    }

    private fun classifyTaskType(message: String, steps: List<RecordedStep>): String {
        val lower = message.lowercase()
        return when {
            lower.containsAny("send", "text", "message") && steps.any { it.toolCall.name == "type_text" } -> "send_message"
            lower.containsAny("call", "dial", "phone") -> "make_call"
            lower.containsAny("search", "find", "look up", "google") -> "search"
            lower.containsAny("timer", "alarm") -> "set_timer"
            lower.containsAny("email", "mail") && steps.any { it.toolCall.name == "type_text" } -> "send_email"
            lower.containsAny("open") && steps.size <= 3 -> "open_app"
            lower.containsAny("navigate", "directions") -> "navigate"
            else -> "general_task"
        }
    }

    private fun buildSchemaJson(paramMap: Map<String, ParameterDef>): String {
        val properties = paramMap.mapValues { (_, def) -> mapOf("type" to def.type) }
        val schema = mapOf(
            "type" to "object",
            "properties" to properties,
            "required" to paramMap.keys.toList()
        )
        val schemaElement = JsonUtils.anyToJsonElement(schema) as? JsonObject
            ?: error("Expected schema JsonObject but got non-object JSON")
        return PLAYBOOK_JSON.encodeToString(JsonObject.serializer(), schemaElement)
    }
}

/**
 * Records successful interactive executions and distills them into persisted playbooks.
 *
 * Thread-safety/correctness contract:
 * - Tool callbacks may arrive on different threads, so in-memory capture uses synchronized state.
 * - Persistence is intentionally synchronous and serialized via [persistenceLock] to keep step ordering
 *   deterministic and to avoid callback-thread races producing duplicate/partial DB writes.
 * - [persistedForCurrentTask] guarantees at-most-once persistence per task, even if completion callbacks
 *   are delivered more than once.
 */
class ExecutionRecorder(
    private val playbookDao: PlaybookDao,
    private val parameterExtractor: ParameterExtractor,
    private val nowMs: () -> Long = { System.currentTimeMillis() },
    private val interactiveTools: Set<String> = DEFAULT_INTERACTIVE_TOOLS
) : AgentExecutionListener {
    private val steps = Collections.synchronizedList(mutableListOf<RecordedStep>())
    private val persistenceLock = Any()
    private val persistedForCurrentTask = AtomicBoolean(false)
    @Volatile private var taskDescription: String? = null

    override fun onTaskStarted(userMessage: String) {
        synchronized(steps) { steps.clear() }
        persistedForCurrentTask.set(false)
        taskDescription = userMessage
    }

    override fun onToolExecuted(
        toolCall: ToolCall,
        screenBefore: ScreenFingerprint?,
        screenAfter: ScreenFingerprint?,
        result: ToolResult,
        failure: ActionFailure?
    ) {
        if (failure != null) return
        synchronized(steps) { steps += RecordedStep(toolCall, screenBefore, screenAfter, result) }
    }

    override fun onTaskCompleted(status: TaskStatus, finalResponse: String) {
        if (status != TaskStatus.COMPLETED) return
        if (!persistedForCurrentTask.compareAndSet(false, true)) return

        val recordedSteps = synchronized(steps) { steps.toList() }
        if (recordedSteps.size < 3) return
        if (recordedSteps.none { it.toolCall.name in interactiveTools }) return

        val description = taskDescription ?: return
        synchronized(persistenceLock) {
            distillAndSave(description, recordedSteps)
        }
    }

    private fun distillAndSave(description: String, recordedSteps: List<RecordedStep>) {
        val appPackage = recordedSteps.firstNotNullOfOrNull { it.screenBefore?.packageName } ?: return
        val extracted = parameterExtractor.extract(description, recordedSteps)
        val now = nowMs()
        val playbookId = playbookDao.insertPlaybook(
            PlaybookEntity(
                appPackage = appPackage,
                taskType = extracted.taskType,
                description = description,
                parameterSchema = extracted.schemaJson,
                createdAt = now,
                lastUsedAt = now,
                lastSucceededAt = now
            )
        )

        recordedSteps.forEachIndexed { index, step ->
            val template = extracted.templatize(step.toolCall.input)
            val selector = inferSelector(step.toolCall)
            playbookDao.insertStep(
                PlaybookStepEntity(
                    playbookId = playbookId,
                    stepOrder = index,
                    screenFingerprint = step.screenBefore?.structuralHash ?: "",
                    screenPackage = step.screenBefore?.packageName,
                    screenActivity = step.screenBefore?.activityName,
                    toolName = step.toolCall.name,
                    toolInputTemplate = mapToJson(template),
                    selectorStrategy = selector.first,
                    selectorValue = selector.second,
                    expectedNextFingerprint = step.screenAfter?.structuralHash,
                    settleTimeMs = inferSettleTime(step)
                )
            )
        }
    }

    private fun inferSelector(toolCall: ToolCall): Pair<String, String> {
        val text = toolCall.input["text"] as? String
        if (!text.isNullOrBlank()) return "text_match" to text

        val resourceId = toolCall.input["resource_id"] as? String
        if (!resourceId.isNullOrBlank()) return "resource_id" to resourceId

        val contentDesc = toolCall.input["content_description"] as? String
        if (!contentDesc.isNullOrBlank()) return "content_desc" to contentDesc

        return "none" to ""
    }

    private fun inferSettleTime(step: RecordedStep): Int {
        return if (step.toolCall.name == "open_app") 1500 else 1000
    }

    private fun mapToJson(value: Map<String, Any>): String {
        val jsonObject = JsonUtils.anyToJsonElement(value) as? JsonObject
            ?: error("Expected tool input template JsonObject but got non-object JSON")
        return PLAYBOOK_JSON.encodeToString(JsonObject.serializer(), jsonObject)
    }

    companion object {
        val DEFAULT_INTERACTIVE_TOOLS = setOf(
            "tap", "tap_text", "type_text", "swipe", "scroll",
            "long_press", "open_app", "press_back", "press_home"
        )
    }
}

private fun String.containsAny(vararg needles: String): Boolean = needles.any { contains(it) }

private fun String.removeJsonCodeFence(): String {
    val trimmed = trim()
    if (!trimmed.startsWith("```") || !trimmed.endsWith("```")) return trimmed
    return trimmed
        .removePrefix("```json")
        .removePrefix("```")
        .removeSuffix("```")
        .trim()
}
