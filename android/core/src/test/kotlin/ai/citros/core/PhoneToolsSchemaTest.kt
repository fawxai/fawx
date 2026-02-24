package ai.citros.core

import org.junit.Test
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class PhoneToolsSchemaTest {

    @Test
    fun `web_search schema uses query and does not expose provider endpoint`() {
        @Suppress("UNCHECKED_CAST")
        val schema = PhoneTools.WEB_SEARCH.inputSchema
        val properties = schema["properties"] as Map<String, *>
        val required = schema["required"] as List<*>

        assertTrue("query" in properties)
        assertTrue("count" in properties)
        assertFalse("provider_endpoint" in properties)
        assertTrue("query" in required)
    }
}
