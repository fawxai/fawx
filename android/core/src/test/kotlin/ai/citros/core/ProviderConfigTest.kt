package ai.citros.core

import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class ProviderConfigTest {

    @Test
    fun `anthropic config has correct provider and baseUrl`() {
        val config = ProviderConfig.anthropic("sk-ant-api03-test")
        
        assertEquals(Provider.ANTHROPIC, config.provider)
        assertEquals("https://api.anthropic.com/v1/messages", config.baseUrl)
    }

    @Test
    fun `anthropic config has correct model IDs`() {
        val config = ProviderConfig.anthropic("sk-ant-api03-test")

        assertEquals(ModelConfig.CHAT_MODEL, config.chatModelId)
        assertEquals(ModelConfig.ACTION_MODEL, config.actionModelId)
    }

    @Test
    fun `anthropic model IDs use dated versions not latest aliases`() {
        val config = ProviderConfig.anthropic("sk-ant-api03-test")

        assertFalse(config.chatModelId.endsWith("-latest"))
        assertFalse(config.actionModelId.endsWith("-latest"))
    }

    @Test
    fun `anthropic config has correct headers`() {
        val config = ProviderConfig.anthropic("sk-ant-api03-my-key")
        
        assertEquals("sk-ant-api03-my-key", config.headers["x-api-key"])
        assertEquals("2023-06-01", config.headers["anthropic-version"])
    }

    @Test
    fun `openRouter config has correct provider and baseUrl`() {
        val config = ProviderConfig.openRouter("sk-or-test-key")
        
        assertEquals(Provider.OPENROUTER, config.provider)
        assertEquals("https://openrouter.ai/api/v1/chat/completions", config.baseUrl)
    }

    @Test
    fun `openRouter config has correct model IDs`() {
        val config = ProviderConfig.openRouter("sk-or-test-key")
        
        assertEquals(ModelConfig.OPENROUTER_CHAT_MODEL, config.chatModelId)
        assertEquals(ModelConfig.OPENROUTER_ACTION_MODEL, config.actionModelId)
    }

    @Test
    fun `openRouter config has correct headers`() {
        val config = ProviderConfig.openRouter("sk-or-my-key")
        
        assertEquals("Bearer sk-or-my-key", config.headers["Authorization"])
        assertEquals("https://citros.ai", config.headers["HTTP-Referer"])
        assertEquals("Citros", config.headers["X-Title"])
    }

    @Test
    fun `detectProvider recognizes Anthropic API key`() {
        val provider = ProviderConfig.detectProvider("sk-ant-api03-abcdef123456")
        assertEquals(Provider.ANTHROPIC, provider)
    }

    @Test
    fun `detectProvider recognizes Anthropic setup token`() {
        val provider = ProviderConfig.detectProvider("sk-ant-oat01-xyz789")
        assertEquals(Provider.ANTHROPIC, provider)
    }

    @Test
    fun `detectProvider recognizes OpenRouter key`() {
        val provider = ProviderConfig.detectProvider("sk-or-v1-abc123")
        assertEquals(Provider.OPENROUTER, provider)
    }

    @Test
    fun `detectProvider recognizes OpenAI API key`() {
        val provider = ProviderConfig.detectProvider("sk-proj-abc123")
        assertEquals(Provider.OPENAI, provider)
    }

    @Test
    fun `detectProvider returns null for generic JWT tokens`() {
        // Generic JWTs should not be classified as OpenAI to avoid credential leakage
        val provider = ProviderConfig.detectProvider("eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ1c2VyIn0.c2lnbmF0dXJl")
        assertNull(provider)
    }

    @Test
    fun `detectProvider respects preferred provider override`() {
        val provider = ProviderConfig.detectProvider(
            credential = "sk-ant-api03-abcdef",
            preferredProvider = Provider.OPENAI
        )
        assertEquals(Provider.OPENAI, provider)
    }

    @Test
    fun `detectProvider returns null for unknown key format`() {
        val provider = ProviderConfig.detectProvider("some-random-key")
        assertNull(provider)
    }

    @Test
    fun `detectProvider returns null for empty string`() {
        val provider = ProviderConfig.detectProvider("")
        assertNull(provider)
    }

    @Test
    fun `isLikelyOpenAiOauthToken detects OpenAI-specific prefixes`() {
        assertTrue(ProviderConfig.isLikelyOpenAiOauthToken("sess-1234567890"))
        assertTrue(ProviderConfig.isLikelyOpenAiOauthToken("oauth_abcdef"))
        assertTrue(ProviderConfig.isLikelyOpenAiOauthToken("oa-1234567890"))
        assertFalse(ProviderConfig.isLikelyOpenAiOauthToken("sk-ant-api03-abc"))
        assertFalse(ProviderConfig.isLikelyOpenAiOauthToken("not-a-token"))
        // Generic JWTs should not match to avoid misclassifying other providers
        assertFalse(ProviderConfig.isLikelyOpenAiOauthToken("eyJhbGciOiJIUzI1NiJ9.eyJ1c2VyIjoiam9lIn0.c2ln"))
    }

    @Test
    fun `detectProvider returns null for unrecognized token with null preferred provider`() {
        // When token is unrecognized AND preferredProvider = null, detectProvider should return null
        val provider = ProviderConfig.detectProvider(
            credential = "unrecognized-token-12345",
            preferredProvider = null
        )
        assertNull(provider)
    }
}
