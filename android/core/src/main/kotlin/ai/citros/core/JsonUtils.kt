package ai.citros.core

import android.util.Log
import androidx.annotation.VisibleForTesting
import kotlinx.serialization.json.*
import java.util.LinkedHashMap

/**
 * Utilities for JSON serialization and deserialization.
 * 
 * These utilities work around kotlinx.serialization's limitations with
 * runtime type `Any`, enabling conversion between Kotlin types and JsonElement.
 */
object JsonUtils {
    /**
     * Abstraction for logging operations used by JsonUtils.
     *
     * Allows tests to inject a custom logger while production code
     * continues using Android logging by default.
     */
    interface Logger {
        /** Log a debug message. */
        fun d(tag: String, message: String)

        /** Log an error message with an optional throwable. */
        fun e(tag: String, message: String, throwable: Throwable? = null)
    }

    private object AndroidLogger : Logger {
        private val fallbackLogger = java.util.logging.Logger.getLogger(JsonUtils::class.java.name)

        override fun d(tag: String, message: String) {
            runCatching { Log.d(tag, message) }
                .onFailure { fallbackLogger.info("$tag: $message") }
        }

        override fun e(tag: String, message: String, throwable: Throwable?) {
            runCatching { Log.e(tag, message, throwable) }
                .onFailure { fallbackLogger.severe("$tag: $message${throwable?.let { " (${it.message})" } ?: ""}") }
        }
    }

    private const val TAG = "JsonUtils"
    private const val MAX_DEPTH = 100

    // Tracks the first N distinct fallback types (not LRU). Additional distinct
    // types are counted in fallbackOverflowCount to keep memory bounded.
    private const val MAX_TRACKED_FALLBACK_TYPES = 100

    private val defaultLogger: Logger = AndroidLogger
    // @Volatile guarantees visibility for logger swaps across threads.
    // Test logger injection is expected during test setup/teardown, not concurrently with conversion work.
    @Volatile
    private var logger: Logger = defaultLogger
    private val telemetryLock = Any()

    // Protected by telemetryLock
    private var fallbackCount: Long = 0
    private var fallbackOverflowCount: Long = 0
    private val fallbackByType = LinkedHashMap<String, Long>()
    
    /**
     * Recursively convert Any to JsonElement for kotlinx.serialization.
     * 
     * This function works around kotlinx.serialization's inability to serialize
     * `Map<String, Any>` at runtime. It manually converts Kotlin types to their
     * JSON equivalents.
     * 
     * **Supported types:**
     * - `null` → `JsonNull`
     * - `String` → `JsonPrimitive(String)`
     * - `Number` (Int, Long, Double, Float, etc.) → `JsonPrimitive(Number)`
     * - `Boolean` → `JsonPrimitive(Boolean)`
     * - `Map<*, *>` → `JsonObject` (recursively)
     * - `List<*>` → `JsonArray` (recursively)
     * - `JsonElement` → passthrough (prevents double-encoding)
     * - Other types → `JsonPrimitive(toString())` with warning
     * 
     * **Depth Limit:**
     * Maximum nesting depth is 100 levels to prevent stack overflow on deeply
     * nested structures. This is sufficient for typical tool schemas (2-3 levels).
     * 
     * **Thread Safety:**
     * Conversion is thread-safe. Fallback telemetry updates are synchronized and
     * only apply on the unexpected-type path.
     * 
     * **Example:**
     * ```kotlin
     * val data = mapOf(
     *     "name" to "John",
     *     "age" to 30,
     *     "active" to true,
     *     "tags" to listOf("kotlin", "android")
     * )
     * val jsonElement = JsonUtils.anyToJsonElement(data)
     * // Result: {"name":"John","age":30,"active":true,"tags":["kotlin","android"]}
     * ```
     * 
     * @param value The value to convert
     * @param depth Current recursion depth (internal parameter, do not set manually)
     * @return JsonElement representation
     * @throws IllegalArgumentException if nesting depth exceeds MAX_DEPTH
     */
    fun anyToJsonElement(value: Any?, depth: Int = 0): JsonElement {
        require(depth < MAX_DEPTH) { 
            "JSON nesting too deep (max $MAX_DEPTH levels). This may indicate a circular reference or malformed data structure."
        }
        
        return when (value) {
            null -> JsonNull
            is String -> JsonPrimitive(value)
            is Number -> JsonPrimitive(value)
            is Boolean -> JsonPrimitive(value)
            is Map<*, *> -> buildJsonObject {
                value.forEach { (k, v) -> 
                    put(k.toString(), anyToJsonElement(v, depth + 1))
                }
            }
            is List<*> -> buildJsonArray {
                value.forEach { 
                    add(anyToJsonElement(it, depth + 1))
                }
            }
            is JsonElement -> value
            else -> {
                // Lightweight telemetry + warning for unexpected types
                val typeName = value::class.simpleName ?: "Unknown"
                recordFallback(typeName)

                val preview = value.toString().take(50)
                logger.d(TAG, "Unexpected type $typeName converted to string: $preview")
                JsonPrimitive(value.toString())
            }
        }
    }

    /**
     * Snapshot of fallback telemetry for [anyToJsonElement].
     */
    data class FallbackTelemetry(
        val totalCount: Long,
        val overflowCount: Long,
        val countByType: Map<String, Long>
    )

    /**
     * Returns an atomic snapshot of in-memory fallback telemetry.
     */
    fun getFallbackTelemetry(): FallbackTelemetry = synchronized(telemetryLock) {
        FallbackTelemetry(
            totalCount = fallbackCount,
            overflowCount = fallbackOverflowCount,
            countByType = fallbackByType.toMap()
        )
    }

    /**
     * Resets fallback telemetry counters atomically.
     * Intended for tests or periodic flushes to external metrics.
     */
    fun resetFallbackTelemetry() = synchronized(telemetryLock) {
        fallbackCount = 0
        fallbackOverflowCount = 0
        fallbackByType.clear()
    }

    @VisibleForTesting
    fun setLoggerForTests(logger: Logger) {
        this.logger = logger
    }

    @VisibleForTesting
    fun resetLoggerForTests() {
        logger = defaultLogger
    }

    @VisibleForTesting
    fun getLoggerForTests(): Logger = logger

    @VisibleForTesting
    fun injectFallbackStateForTests(
        count: Long,
        overflowCount: Long,
        byType: Map<String, Long>
    ) = synchronized(telemetryLock) {
        fallbackCount = count
        fallbackOverflowCount = overflowCount
        fallbackByType.clear()
        fallbackByType.putAll(byType)
    }

    private fun recordFallback(typeName: String) = synchronized(telemetryLock) {
        fallbackCount += 1
        if (fallbackByType.containsKey(typeName)) {
            fallbackByType[typeName] = fallbackByType.getValue(typeName) + 1
            return
        }

        if (fallbackByType.size < MAX_TRACKED_FALLBACK_TYPES) {
            fallbackByType[typeName] = 1
        } else {
            fallbackOverflowCount += 1
        }
    }
    
    /**
     * Parse a JsonElement to its natural Kotlin type.
     * 
     * This is the inverse of [anyToJsonElement], converting structured JSON
     * back to Kotlin primitives and collections.
     * 
     * **Type mapping:**
     * - `JsonNull` → `null`
     * - `JsonPrimitive(String)` → `String`
     * - `JsonPrimitive(Number)` → `Int` (if fits), `Long` (if fits), or `Double`
     * - `JsonPrimitive(Boolean)` → `Boolean`
     * - `JsonArray` → `List<Any?>` (recursively)
     * - `JsonObject` → `Map<String, Any?>` (recursively)
     * 
     * @param element The JsonElement to parse
     * @param depth Current recursion depth (internal parameter, do not set manually)
     * @return The natural Kotlin representation, or null if element is JsonNull
     * @throws IllegalArgumentException if nesting depth exceeds MAX_DEPTH
     */
    fun parseJsonElement(element: JsonElement, depth: Int = 0): Any? {
        require(depth < MAX_DEPTH) { 
            "JSON nesting too deep (max $MAX_DEPTH levels)"
        }
        
        return when (element) {
            is JsonNull -> null
            is JsonPrimitive -> {
                when {
                    element.isString -> element.content
                    element.content == "true" -> true
                    element.content == "false" -> false
                    else -> {
                        // For numbers, prefer Int if it fits, otherwise Long, otherwise Double
                        element.content.toIntOrNull()
                            ?: element.content.toLongOrNull()
                            ?: element.content.toDoubleOrNull()
                            ?: element.content
                    }
                }
            }
            is JsonArray -> element.map { parseJsonElement(it, depth + 1) }
            is JsonObject -> parseJsonObjectToMap(element, depth + 1)
        }
    }
    
    /**
     * Convert a JsonObject to a Map, preserving JSON types.
     * 
     * This is a specialized version of [parseJsonElement] for objects.
     * 
     * @param jsonObject The JsonObject to convert
     * @param depth Current recursion depth (internal parameter)
     * @return Map with string keys and typed values
     * @throws IllegalArgumentException if nesting depth exceeds MAX_DEPTH
     */
    fun parseJsonObjectToMap(jsonObject: JsonObject, depth: Int = 0): Map<String, Any> {
        require(depth < MAX_DEPTH) { 
            "JSON nesting too deep (max $MAX_DEPTH levels)"
        }
        
        val result = mutableMapOf<String, Any>()
        jsonObject.forEach { (key, value) ->
            parseJsonElement(value, depth)?.let { result[key] = it }
        }
        return result
    }
}
