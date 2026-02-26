package ai.citros.core

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject

private val toolCallJson = Json { ignoreUnknownKeys = true }

fun ToolCall.toSerializedToolCall(): SerializedToolCall {
    val inputJson = buildJsonObject {
        input.forEach { (key, value) ->
            put(key, JsonUtils.anyToJsonElement(value))
        }
    }.toString()
    return SerializedToolCall(id = id, name = name, inputJson = inputJson)
}

fun SerializedToolCall.toToolCall(): ToolCall {
    val inputMap = runCatching {
        val element = toolCallJson.parseToJsonElement(inputJson)
        when (element) {
            is JsonObject -> JsonUtils.parseJsonObjectToMap(element)
            else -> emptyMap()
        }
    }.getOrDefault(emptyMap())

    return ToolCall(
        id = id,
        name = name,
        input = inputMap
    )
}
