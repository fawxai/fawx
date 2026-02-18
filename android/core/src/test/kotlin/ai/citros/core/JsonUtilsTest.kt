package ai.citros.core

import kotlinx.serialization.json.*
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test

class JsonUtilsTest {

    @Before
    fun setUp() {
        JsonUtils.resetFallbackTelemetry()
        JsonUtils.resetLoggerForTests()
    }
    
    // ==================== anyToJsonElement Tests ====================
    
    @Test
    fun `anyToJsonElement handles null`() {
        val result = JsonUtils.anyToJsonElement(null)
        assertTrue(result is JsonNull)
        assertEquals(JsonNull, result)
    }
    
    @Test
    fun `anyToJsonElement handles String`() {
        val result = JsonUtils.anyToJsonElement("hello")
        assertTrue(result is JsonPrimitive)
        assertEquals("hello", (result as JsonPrimitive).content)
    }
    
    @Test
    fun `anyToJsonElement handles empty String`() {
        val result = JsonUtils.anyToJsonElement("")
        assertTrue(result is JsonPrimitive)
        assertEquals("", (result as JsonPrimitive).content)
    }
    
    @Test
    fun `anyToJsonElement handles Int`() {
        val result = JsonUtils.anyToJsonElement(42)
        assertTrue(result is JsonPrimitive)
        assertEquals(42, (result as JsonPrimitive).int)
    }
    
    @Test
    fun `anyToJsonElement handles Long`() {
        val result = JsonUtils.anyToJsonElement(9876543210L)
        assertTrue(result is JsonPrimitive)
        assertEquals(9876543210L, (result as JsonPrimitive).long)
    }
    
    @Test
    fun `anyToJsonElement handles Double`() {
        val result = JsonUtils.anyToJsonElement(3.14159)
        assertTrue(result is JsonPrimitive)
        assertEquals(3.14159, (result as JsonPrimitive).double, 0.00001)
    }
    
    @Test
    fun `anyToJsonElement handles Float`() {
        val result = JsonUtils.anyToJsonElement(2.71828f)
        assertTrue(result is JsonPrimitive)
        assertEquals(2.71828f, (result as JsonPrimitive).float, 0.00001f)
    }
    
    @Test
    fun `anyToJsonElement handles Boolean true`() {
        val result = JsonUtils.anyToJsonElement(true)
        assertTrue(result is JsonPrimitive)
        assertTrue((result as JsonPrimitive).boolean)
    }
    
    @Test
    fun `anyToJsonElement handles Boolean false`() {
        val result = JsonUtils.anyToJsonElement(false)
        assertTrue(result is JsonPrimitive)
        assertFalse((result as JsonPrimitive).boolean)
    }
    
    @Test
    fun `anyToJsonElement handles empty Map`() {
        val result = JsonUtils.anyToJsonElement(emptyMap<String, Any>())
        assertTrue(result is JsonObject)
        assertEquals(0, (result as JsonObject).size)
    }
    
    @Test
    fun `anyToJsonElement handles simple Map`() {
        val input = mapOf("name" to "Alice", "age" to 30)
        val result = JsonUtils.anyToJsonElement(input)
        
        assertTrue(result is JsonObject)
        val obj = result as JsonObject
        assertEquals("Alice", obj["name"]?.jsonPrimitive?.content)
        assertEquals(30, obj["age"]?.jsonPrimitive?.int)
    }
    
    @Test
    fun `anyToJsonElement handles nested Map`() {
        val input = mapOf(
            "user" to mapOf(
                "name" to "Bob",
                "settings" to mapOf("theme" to "dark")
            )
        )
        val result = JsonUtils.anyToJsonElement(input)
        
        assertTrue(result is JsonObject)
        val obj = result as JsonObject
        val user = obj["user"]?.jsonObject
        assertNotNull(user)
        assertEquals("Bob", user!!["name"]?.jsonPrimitive?.content)
        
        val settings = user["settings"]?.jsonObject
        assertNotNull(settings)
        assertEquals("dark", settings!!["theme"]?.jsonPrimitive?.content)
    }
    
    @Test
    fun `anyToJsonElement handles empty List`() {
        val result = JsonUtils.anyToJsonElement(emptyList<Any>())
        assertTrue(result is JsonArray)
        assertEquals(0, (result as JsonArray).size)
    }
    
    @Test
    fun `anyToJsonElement handles simple List`() {
        val input = listOf(1, 2, 3)
        val result = JsonUtils.anyToJsonElement(input)
        
        assertTrue(result is JsonArray)
        val arr = result as JsonArray
        assertEquals(3, arr.size)
        assertEquals(1, arr[0].jsonPrimitive.int)
        assertEquals(2, arr[1].jsonPrimitive.int)
        assertEquals(3, arr[2].jsonPrimitive.int)
    }
    
    @Test
    fun `anyToJsonElement handles nested List`() {
        val input = listOf(1, listOf(2, 3), listOf(4, listOf(5)))
        val result = JsonUtils.anyToJsonElement(input)
        
        assertTrue(result is JsonArray)
        val arr = result as JsonArray
        assertEquals(3, arr.size)
        assertEquals(1, arr[0].jsonPrimitive.int)
        
        val nested1 = arr[1].jsonArray
        assertEquals(2, nested1.size)
        assertEquals(2, nested1[0].jsonPrimitive.int)
        
        val nested2 = arr[2].jsonArray
        val doubleNested = nested2[1].jsonArray
        assertEquals(5, doubleNested[0].jsonPrimitive.int)
    }
    
    @Test
    fun `anyToJsonElement handles mixed nested structures`() {
        val input = mapOf(
            "name" to "Charlie",
            "tags" to listOf("kotlin", "android"),
            "metadata" to mapOf(
                "created" to "2024-01-01",
                "versions" to listOf(1, 2, 3)
            )
        )
        val result = JsonUtils.anyToJsonElement(input)
        
        assertTrue(result is JsonObject)
        val obj = result as JsonObject
        assertEquals("Charlie", obj["name"]?.jsonPrimitive?.content)
        
        val tags = obj["tags"]?.jsonArray
        assertNotNull(tags)
        assertEquals(2, tags!!.size)
        assertEquals("kotlin", tags[0].jsonPrimitive.content)
        
        val metadata = obj["metadata"]?.jsonObject
        assertNotNull(metadata)
        assertEquals("2024-01-01", metadata!!["created"]?.jsonPrimitive?.content)
        
        val versions = metadata["versions"]?.jsonArray
        assertNotNull(versions)
        assertEquals(3, versions!!.size)
    }
    
    @Test
    fun `anyToJsonElement handles JsonElement passthrough`() {
        val input = buildJsonObject {
            put("key", "value")
        }
        val result = JsonUtils.anyToJsonElement(input)
        
        // Should return the same JsonElement
        assertTrue(result is JsonObject)
        assertEquals(input, result)
    }
    
    @Test
    fun `anyToJsonElement handles unknown types with toString fallback`() {
        class CustomClass(val value: String) {
            override fun toString() = "CustomClass($value)"
        }

        JsonUtils.resetFallbackTelemetry()

        val input = CustomClass("test")
        val result = JsonUtils.anyToJsonElement(input)

        assertTrue(result is JsonPrimitive)
        assertEquals("CustomClass(test)", (result as JsonPrimitive).content)

        val telemetry = JsonUtils.getFallbackTelemetry()
        assertEquals(1, telemetry.totalCount)
        assertEquals(0, telemetry.overflowCount)
        assertEquals(1L, telemetry.countByType["CustomClass"])
    }

    @Test
    fun `setLoggerForTests allows injection of custom logger for fallback warnings`() {
        class CustomClass {
            override fun toString() = "Custom"
        }

        val debugMessages = mutableListOf<String>()
        val errorMessages = mutableListOf<String>()
        JsonUtils.setLoggerForTests(object : JsonUtils.Logger {
            override fun d(tag: String, message: String) {
                debugMessages += "$tag::$message"
            }

            override fun e(tag: String, message: String, throwable: Throwable?) {
                errorMessages += "$tag::$message"
            }
        })

        JsonUtils.anyToJsonElement(CustomClass())

        assertEquals(1, debugMessages.size)
        assertTrue(debugMessages.first().contains("Unexpected type CustomClass converted to string"))
        assertTrue(errorMessages.isEmpty())
    }

    @Test
    fun `resetLoggerForTests restores default logger implementation`() {
        val firstLogger = JsonUtils.getLoggerForTests()

        JsonUtils.setLoggerForTests(object : JsonUtils.Logger {
            override fun d(tag: String, message: String) = Unit

            override fun e(tag: String, message: String, throwable: Throwable?) = Unit
        })

        JsonUtils.resetLoggerForTests()

        assertSame(firstLogger, JsonUtils.getLoggerForTests())

        class ResetTest {
            override fun toString() = "test"
        }

        JsonUtils.anyToJsonElement(ResetTest())
    }

    @Test
    fun `anyToJsonElement fallback telemetry aggregates by type`() {
        class A {
            override fun toString() = "A"
        }

        class B {
            override fun toString() = "B"
        }

        JsonUtils.resetFallbackTelemetry()

        JsonUtils.anyToJsonElement(A())
        JsonUtils.anyToJsonElement(A())
        JsonUtils.anyToJsonElement(B())

        val telemetry = JsonUtils.getFallbackTelemetry()
        assertEquals(3, telemetry.totalCount)
        assertEquals(2L, telemetry.countByType["A"])
        assertEquals(1L, telemetry.countByType["B"])
        assertEquals(0, telemetry.overflowCount)
    }

    @Test
    fun `anyToJsonElement resetFallbackTelemetry clears metrics`() {
        class Custom {
            override fun toString() = "Custom"
        }

        JsonUtils.anyToJsonElement(Custom())
        assertTrue(JsonUtils.getFallbackTelemetry().totalCount > 0)

        JsonUtils.resetFallbackTelemetry()

        val telemetry = JsonUtils.getFallbackTelemetry()
        assertEquals(0, telemetry.totalCount)
        assertEquals(0, telemetry.overflowCount)
        assertTrue(telemetry.countByType.isEmpty())
    }

    @Test
    fun `anyToJsonElement overflow counter increments when tracked type limit is exceeded`() {
        JsonUtils.injectFallbackStateForTests(
            count = 100L,
            overflowCount = 0L,
            byType = (0 until 100).associate { "TrackedType$it" to 1L }
        )

        class OverflowType {
            override fun toString() = "overflow"
        }

        JsonUtils.anyToJsonElement(OverflowType())

        val telemetry = JsonUtils.getFallbackTelemetry()
        assertEquals(101L, telemetry.totalCount)
        assertEquals(1L, telemetry.overflowCount)
        assertEquals(100, telemetry.countByType.size)
        assertFalse(telemetry.countByType.containsKey("OverflowType"))
    }

    @Test
    fun `anyToJsonElement standard types do not increment fallback telemetry`() {
        JsonUtils.resetFallbackTelemetry()
        JsonUtils.anyToJsonElement(mapOf("key" to "value", "count" to 42, "ok" to true))
        JsonUtils.anyToJsonElement(listOf(1, 2, 3))
        JsonUtils.anyToJsonElement(buildJsonObject { put("already", "json") })

        val telemetry = JsonUtils.getFallbackTelemetry()
        assertEquals(0L, telemetry.totalCount)
        assertEquals(0L, telemetry.overflowCount)
        assertTrue(telemetry.countByType.isEmpty())
    }

    @Test
    fun `anyToJsonElement enforces depth limit`() {
        // Create a deeply nested map
        var deepMap: Any = "bottom"
        for (i in 1..150) {
            deepMap = mapOf("level" to deepMap)
        }
        
        // Should throw IllegalArgumentException due to depth limit
        try {
            JsonUtils.anyToJsonElement(deepMap)
            fail("Expected IllegalArgumentException due to depth limit")
        } catch (e: IllegalArgumentException) {
            assertTrue(e.message?.contains("nesting too deep") == true)
        }
    }
    
    @Test
    fun `anyToJsonElement handles Map with non-string keys`() {
        val input = mapOf(1 to "one", 2 to "two")
        val result = JsonUtils.anyToJsonElement(input)
        
        assertTrue(result is JsonObject)
        val obj = result as JsonObject
        assertEquals("one", obj["1"]?.jsonPrimitive?.content)
        assertEquals("two", obj["2"]?.jsonPrimitive?.content)
    }
    
    // ==================== parseJsonElement Tests ====================
    
    @Test
    fun `parseJsonElement handles JsonNull`() {
        val result = JsonUtils.parseJsonElement(JsonNull)
        assertNull(result)
    }
    
    @Test
    fun `parseJsonElement handles String primitive`() {
        val input = JsonPrimitive("hello")
        val result = JsonUtils.parseJsonElement(input)
        assertEquals("hello", result)
    }
    
    @Test
    fun `parseJsonElement handles Int primitive`() {
        val input = JsonPrimitive(42)
        val result = JsonUtils.parseJsonElement(input)
        assertEquals(42, result)
    }
    
    @Test
    fun `parseJsonElement handles Long primitive`() {
        val input = JsonPrimitive(9876543210L)
        val result = JsonUtils.parseJsonElement(input)
        assertEquals(9876543210L, result)
    }
    
    @Test
    fun `parseJsonElement handles Double primitive`() {
        val input = JsonPrimitive(3.14159)
        val result = JsonUtils.parseJsonElement(input)
        assertEquals(3.14159, (result as Double), 0.00001)
    }
    
    @Test
    fun `parseJsonElement handles Boolean true`() {
        val input = JsonPrimitive(true)
        val result = JsonUtils.parseJsonElement(input)
        assertEquals(true, result)
    }
    
    @Test
    fun `parseJsonElement handles Boolean false`() {
        val input = JsonPrimitive(false)
        val result = JsonUtils.parseJsonElement(input)
        assertEquals(false, result)
    }
    
    @Test
    fun `parseJsonElement handles JsonArray`() {
        val input = buildJsonArray {
            add(1)
            add("two")
            add(true)
        }
        val result = JsonUtils.parseJsonElement(input)
        
        assertTrue(result is List<*>)
        val list = result as List<*>
        assertEquals(3, list.size)
        assertEquals(1, list[0])
        assertEquals("two", list[1])
        assertEquals(true, list[2])
    }
    
    @Test
    fun `parseJsonElement handles JsonObject`() {
        val input = buildJsonObject {
            put("name", "Alice")
            put("age", 30)
            put("active", true)
        }
        val result = JsonUtils.parseJsonElement(input)
        
        assertTrue(result is Map<*, *>)
        val map = result as Map<*, *>
        assertEquals("Alice", map["name"])
        assertEquals(30, map["age"])
        assertEquals(true, map["active"])
    }
    
    @Test
    fun `parseJsonElement handles nested structures`() {
        val input = buildJsonObject {
            put("user", buildJsonObject {
                put("name", "Bob")
                put("tags", buildJsonArray {
                    add("kotlin")
                    add("android")
                })
            })
        }
        val result = JsonUtils.parseJsonElement(input)
        
        assertTrue(result is Map<*, *>)
        val map = result as Map<*, *>
        val user = map["user"] as Map<*, *>
        assertEquals("Bob", user["name"])
        
        val tags = user["tags"] as List<*>
        assertEquals(2, tags.size)
        assertEquals("kotlin", tags[0])
    }
    
    @Test
    fun `parseJsonElement enforces depth limit`() {
        // Create a deeply nested JsonArray
        var deepArray = buildJsonArray { add("bottom") }
        for (i in 1..150) {
            deepArray = buildJsonArray { add(deepArray) }
        }
        
        // Should throw IllegalArgumentException due to depth limit
        try {
            JsonUtils.parseJsonElement(deepArray)
            fail("Expected IllegalArgumentException due to depth limit")
        } catch (e: IllegalArgumentException) {
            assertTrue(e.message?.contains("nesting too deep") == true)
        }
    }
    
    // ==================== parseJsonObjectToMap Tests ====================
    
    @Test
    fun `parseJsonObjectToMap handles empty object`() {
        val input = buildJsonObject {}
        val result = JsonUtils.parseJsonObjectToMap(input)
        
        assertTrue(result.isEmpty())
    }
    
    @Test
    fun `parseJsonObjectToMap handles simple object`() {
        val input = buildJsonObject {
            put("x", 100)
            put("y", 200)
        }
        val result = JsonUtils.parseJsonObjectToMap(input)
        
        assertEquals(2, result.size)
        assertEquals(100, result["x"])
        assertEquals(200, result["y"])
    }
    
    @Test
    fun `parseJsonObjectToMap handles nested object`() {
        val input = buildJsonObject {
            put("position", buildJsonObject {
                put("x", 10)
                put("y", 20)
            })
        }
        val result = JsonUtils.parseJsonObjectToMap(input)
        
        val position = result["position"] as Map<*, *>
        assertEquals(10, position["x"])
        assertEquals(20, position["y"])
    }
    
    @Test
    fun `parseJsonObjectToMap handles null values`() {
        val input = buildJsonObject {
            put("nullable", JsonNull)
            put("text", "value")
        }
        val result = JsonUtils.parseJsonObjectToMap(input)
        
        // Null values are filtered out (not included in result map)
        assertEquals(1, result.size)
        assertEquals("value", result["text"])
        assertFalse(result.containsKey("nullable"))
    }
    
    // ==================== Null Filtering Edge Cases (#236) ====================
    // Null filtering prevents JSON bloat in API payloads and aligns with provider
    // expectations — most LLM APIs reject or misinterpret null-valued fields.

    @Test
    fun `parseJsonObjectToMap filters nulls in nested objects recursively`() {
        val input = buildJsonObject {
            put("outer", buildJsonObject {
                put("valid", "yes")
                put("nullField", JsonNull)
                put("inner", buildJsonObject {
                    put("deep", "value")
                    put("deepNull", JsonNull)
                })
            })
        }
        val result = JsonUtils.parseJsonObjectToMap(input)

        val outer = result["outer"] as Map<*, *>
        assertEquals("yes", outer["valid"])
        assertFalse(outer.containsKey("nullField"))

        val inner = outer["inner"] as Map<*, *>
        assertEquals("value", inner["deep"])
        assertFalse(inner.containsKey("deepNull"))
    }

    @Test
    fun `parseJsonElement handles arrays containing nulls`() {
        val input = buildJsonArray {
            add(1)
            add(JsonNull)
            add("three")
            add(JsonNull)
        }
        val result = JsonUtils.parseJsonElement(input)

        assertTrue(result is List<*>)
        val list = result as List<*>
        assertEquals(4, list.size)
        assertEquals(1, list[0])
        assertNull(list[1])
        assertEquals("three", list[2])
        assertNull(list[3])
    }

    @Test
    fun `parseJsonObjectToMap preserves empty strings distinct from null`() {
        val input = buildJsonObject {
            put("empty", "")
            put("nullVal", JsonNull)
            put("space", " ")
        }
        val result = JsonUtils.parseJsonObjectToMap(input)

        assertEquals("", result["empty"])
        assertEquals(" ", result["space"])
        assertFalse(result.containsKey("nullVal"))
    }

    @Test
    fun `parseJsonObjectToMap handles object where all values are null`() {
        val input = buildJsonObject {
            put("a", JsonNull)
            put("b", JsonNull)
            put("c", JsonNull)
        }
        val result = JsonUtils.parseJsonObjectToMap(input)

        assertTrue(result.isEmpty())
    }

    @Test
    fun `anyToJsonElement handles map with null values`() {
        val input = mapOf("a" to "value", "b" to null, "c" to 42)
        val result = JsonUtils.anyToJsonElement(input)

        assertTrue(result is JsonObject)
        val obj = result as JsonObject
        assertEquals("value", obj["a"]?.jsonPrimitive?.content)
        assertTrue(obj["b"] is JsonNull)
        assertEquals(42, obj["c"]?.jsonPrimitive?.int)
    }

    @Test
    fun `anyToJsonElement handles map with empty string keys`() {
        val input = mapOf("" to "emptyKey", "normal" to "value")
        val result = JsonUtils.anyToJsonElement(input)

        assertTrue(result is JsonObject)
        val obj = result as JsonObject
        assertEquals("emptyKey", obj[""]?.jsonPrimitive?.content)
        assertEquals("value", obj["normal"]?.jsonPrimitive?.content)
    }

    @Test
    fun `anyToJsonElement handles list with null values`() {
        val input = listOf("a", null, "c")
        val result = JsonUtils.anyToJsonElement(input)

        assertTrue(result is JsonArray)
        val arr = result as JsonArray
        assertEquals(3, arr.size)
        assertEquals("a", arr[0].jsonPrimitive.content)
        assertTrue(arr[1] is JsonNull)
        assertEquals("c", arr[2].jsonPrimitive.content)
    }

    // ==================== Round-trip Tests ====================
    
    @Test
    fun `round-trip conversion preserves data`() {
        val original = mapOf(
            "name" to "Test",
            "count" to 42,
            "active" to true,
            "tags" to listOf("a", "b", "c"),
            "metadata" to mapOf("key" to "value")
        )
        
        val jsonElement = JsonUtils.anyToJsonElement(original)
        val parsed = JsonUtils.parseJsonElement(jsonElement)
        
        assertEquals(original, parsed)
    }
    
    @Test
    fun `round-trip handles complex tool schema`() {
        val toolSchema = mapOf(
            "type" to "object",
            "properties" to mapOf(
                "element_id" to mapOf(
                    "type" to "integer",
                    "description" to "The element ID to tap"
                ),
                "long_press" to mapOf(
                    "type" to "boolean",
                    "description" to "Whether to long press",
                    "default" to false
                )
            ),
            "required" to listOf("element_id")
        )
        
        val jsonElement = JsonUtils.anyToJsonElement(toolSchema)
        val parsed = JsonUtils.parseJsonElement(jsonElement)
        
        assertEquals(toolSchema, parsed)
    }
}
