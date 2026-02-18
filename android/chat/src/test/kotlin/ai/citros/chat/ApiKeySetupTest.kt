package ai.citros.chat

import ai.citros.core.Provider
import org.junit.Test
import kotlin.test.assertEquals

class ApiKeySetupTest {

    @Test
    fun `providerDashboardUrl returns expected URLs`() {
        assertEquals(
            "https://console.anthropic.com/settings/keys",
            providerDashboardUrl(Provider.ANTHROPIC)
        )
        assertEquals(
            "https://platform.openai.com/api-keys",
            providerDashboardUrl(Provider.OPENAI)
        )
        assertEquals(
            "https://openrouter.ai/keys",
            providerDashboardUrl(Provider.OPENROUTER)
        )
    }

    @Test
    fun `isValidKeyFormat validates known prefixes`() {
        assertEquals(true, isValidKeyFormat("sk-ant-api-123", Provider.ANTHROPIC))
        assertEquals(true, isValidKeyFormat("sk-proj-123", Provider.OPENAI))
        assertEquals(true, isValidKeyFormat("oa-123", Provider.OPENAI))
        assertEquals(true, isValidKeyFormat("sk-or-123", Provider.OPENROUTER))
        assertEquals(false, isValidKeyFormat("invalid-token", Provider.OPENAI))
        assertEquals(false, isValidKeyFormat("sk-proj-123", Provider.OPENROUTER))
    }

    @Test
    fun `isValidKeyFormat accepts session and oauth tokens for any provider`() {
        assertEquals(true, isValidKeyFormat("sess-abc123", Provider.ANTHROPIC))
        assertEquals(true, isValidKeyFormat("sess-abc123", Provider.OPENAI))
        assertEquals(true, isValidKeyFormat("oauth_abc123", Provider.ANTHROPIC))
        assertEquals(true, isValidKeyFormat("oauth_abc123", Provider.OPENAI))
    }

    @Test
    fun `isValidKeyFormat trims surrounding whitespace`() {
        assertEquals(true, isValidKeyFormat("  sk-proj-123  ", Provider.OPENAI))
        assertEquals(true, isValidKeyFormat("\n\tsk-or-abc\t", Provider.OPENROUTER))
    }

    @Test
    fun `isValidKeyFormat rejects whitespace only`() {
        assertEquals(false, isValidKeyFormat("   ", Provider.OPENAI))
        assertEquals(false, isValidKeyFormat("\n\t", Provider.ANTHROPIC))
    }
}
