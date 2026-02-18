package ai.citros.chat

import org.junit.Test
import kotlin.test.assertEquals

class ChatActivityPrefsKeyConstantsTest {

    @Test
    fun `search provider preference keys remain stable`() {
        assertEquals("search_base_url", PREF_SEARCH_BASE_URL)
        assertEquals("brave_api_key", PREF_BRAVE_API_KEY)
    }
}
