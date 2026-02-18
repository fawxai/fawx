package ai.citros.chat

import ai.citros.core.Provider
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class ProviderUiTest {

    @Test
    fun `all providers have non blank ui values`() {
        Provider.entries.forEach { provider ->
            assertTrue(ProviderUi.displayName(provider).isNotBlank())
            assertTrue(ProviderUi.icon(provider).isNotBlank())
            assertTrue(ProviderUi.keyPlaceholder(provider).isNotBlank())
        }
    }

    @Test
    fun `all providers have non transparent brand colors`() {
        Provider.entries.forEach { provider ->
            assertTrue(ProviderUi.brandColor(provider).alpha > 0f)
        }
    }

    @Test
    fun `anthropic has correct display values`() {
        assertEquals("Anthropic", ProviderUi.displayName(Provider.ANTHROPIC))
        assertEquals("🟠", ProviderUi.icon(Provider.ANTHROPIC))
        assertEquals("sk-ant-...", ProviderUi.keyPlaceholder(Provider.ANTHROPIC))
    }

    @Test
    fun `openai has correct display values`() {
        assertEquals("OpenAI", ProviderUi.displayName(Provider.OPENAI))
        assertEquals("🟢", ProviderUi.icon(Provider.OPENAI))
        assertEquals("sk-...", ProviderUi.keyPlaceholder(Provider.OPENAI))
    }

    @Test
    fun `openrouter has correct display values`() {
        assertEquals("OpenRouter", ProviderUi.displayName(Provider.OPENROUTER))
        assertEquals("🔷", ProviderUi.icon(Provider.OPENROUTER))
        assertEquals("sk-or-...", ProviderUi.keyPlaceholder(Provider.OPENROUTER))
    }

    @Test
    fun `all key placeholders end with ellipsis`() {
        Provider.entries.forEach { provider ->
            assertTrue(
                ProviderUi.keyPlaceholder(provider).endsWith("..."),
                "keyPlaceholder for $provider should end with ..."
            )
        }
    }
}
