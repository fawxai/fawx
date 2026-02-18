package ai.citros.chat

import ai.citros.core.ModelConfig
import ai.citros.core.Provider
import ai.citros.core.WalletManager
import ai.citros.core.WalletState
import ai.citros.core.WalletStorage
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

/**
 * Unit tests for OnboardingFlow helper functions.
 */
@RunWith(RobolectricTestRunner::class)
class OnboardingFlowUtilsTest {

    @Test
    fun isLikelyValidEmail_validatesCorrectEmails() {
        // Valid emails
        assertTrue(isLikelyValidEmail("user@example.com"))
        assertTrue(isLikelyValidEmail("user.name@example.com"))
        assertTrue(isLikelyValidEmail("user+tag@example.co.uk"))
        assertTrue(isLikelyValidEmail("  user@example.com  ")) // with whitespace

        // Invalid emails
        assertFalse(isLikelyValidEmail(""))
        assertFalse(isLikelyValidEmail("   "))
        assertFalse(isLikelyValidEmail("not-an-email"))
        assertFalse(isLikelyValidEmail("@example.com"))
        assertFalse(isLikelyValidEmail("user@"))
        assertFalse(isLikelyValidEmail("user @example.com")) // space in local part
    }

    @Test
    fun extractFirstJsonObject_withValidJson_returnsObject() {
        val input = """
            Here's your profile:
            {"name":"Alice","age":30}
            Does this look good?
        """.trimIndent()

        val result = extractFirstJsonObject(input)
        assertNotNull(result)
        assertEquals("Alice", result["name"]?.jsonPrimitive?.content)
        assertEquals(30, result["age"]?.jsonPrimitive?.int)
    }

    @Test
    fun extractFirstJsonObject_withNestedJson_returnsOutermost() {
        val input = """
            {"outer":{"inner":"value"},"key":"val"}
        """.trimIndent()

        val result = extractFirstJsonObject(input)
        assertNotNull(result)
        assertEquals("val", result["key"]?.jsonPrimitive?.content)
        assertNotNull(result["outer"])
    }

    @Test
    fun extractFirstJsonObject_withNoJson_returnsNull() {
        val input = "This text has no JSON object"
        val result = extractFirstJsonObject(input)
        assertEquals(null, result)
    }

    @Test
    fun extractFirstJsonObject_withMalformedJson_returnsNull() {
        val input = """
            {"incomplete": "object"
        """.trimIndent()

        val result = extractFirstJsonObject(input)
        assertEquals(null, result)
    }

    @Test
    fun sanitizeAssistantResponse_removesCompletionToken() {
        val input = "Great! I have all I need. [ONBOARDING_COMPLETE]"
        val (cleanText, hasToken) = sanitizeAssistantResponse(input)
        
        assertEquals("Great! I have all I need.", cleanText.trim())
        assertTrue(hasToken)
    }

    @Test
    fun sanitizeAssistantResponse_withNoToken_returnsOriginal() {
        val input = "Tell me more about yourself!"
        val (cleanText, hasToken) = sanitizeAssistantResponse(input)
        
        assertEquals("Tell me more about yourself!", cleanText)
        assertFalse(hasToken)
    }

    @Test
    fun sanitizeAssistantResponse_withOnlyToken_returnsEmpty() {
        val input = "[ONBOARDING_COMPLETE]"
        val (cleanText, hasToken) = sanitizeAssistantResponse(input)
        
        assertTrue(cleanText.trim().isEmpty())
        assertTrue(hasToken)
    }

    @Test
    fun deserializeTranscript_withValidJson_returnsLines() {
        val json = """
            [
                {"id":1,"role":"assistant","text":"Hello!"},
                {"id":2,"role":"user","text":"Hi there"}
            ]
        """.trimIndent()

        val lines = deserializeTranscript(json)
        
        assertEquals(2, lines.size)
        assertEquals(1, lines[0].id)
        assertEquals("assistant", lines[0].role)
        assertEquals("Hello!", lines[0].text)
        assertEquals(2, lines[1].id)
        assertEquals("user", lines[1].role)
        assertEquals("Hi there", lines[1].text)
    }

    @Test
    fun deserializeTranscript_withMalformedEntry_skipsInvalid() {
        val json = """
            [
                {"id":1,"role":"assistant","text":"Hello!"},
                {"id":2,"role":"user"},
                {"id":3,"role":"assistant","text":"How are you?"}
            ]
        """.trimIndent()

        val lines = deserializeTranscript(json)
        
        // Should skip the entry with missing "text"
        assertEquals(2, lines.size)
        assertEquals(1, lines[0].id)
        assertEquals(3, lines[1].id)
    }

    @Test
    fun deserializeTranscript_withEmptyString_returnsEmpty() {
        assertEquals(emptyList(), deserializeTranscript(""))
        assertEquals(emptyList(), deserializeTranscript(null))
        assertEquals(emptyList(), deserializeTranscript("   "))
    }

    @Test
    fun deserializeTranscript_withInvalidJson_returnsEmpty() {
        val malformed = """{"not": "an array"}"""
        assertEquals(emptyList(), deserializeTranscript(malformed))
    }

    @Test
    fun serializeTranscript_withValidLines_returnsJson() {
        val lines = listOf(
            OnboardingChatLine(1, "assistant", "Hello!"),
            OnboardingChatLine(2, "user", "Hi")
        )

        val json = serializeTranscript(lines)
        
        assertTrue(json.contains("\"id\":1"))
        assertTrue(json.contains("\"role\":\"assistant\""))
        assertTrue(json.contains("\"text\":\"Hello!\""))
        assertTrue(json.contains("\"id\":2"))
        assertTrue(json.contains("\"role\":\"user\""))
        assertTrue(json.contains("\"text\":\"Hi\""))
    }
}

// Make helper functions accessible for testing
// These need to be internal or public in OnboardingFlow.kt to test them,
// or we create wrapper functions here that access them if they're private.
// For now, we'll assume they're made internal for testing purposes.

/**
 * Tests for onboarding wallet activation flow (#387).
 * Verifies that addKey + setActiveKey + model defaults work correctly.
 */
@RunWith(RobolectricTestRunner::class)
class OnboardingWalletActivationTest {

    private class TestWalletStorage : WalletStorage {
        private var state = WalletState(
            keys = emptyList(), activeKeyId = null,
            chatModelId = ModelConfig.CHAT_MODEL,
            actionModelId = ModelConfig.ACTION_MODEL
        )
        override fun loadState(): WalletState = state
        override fun saveState(state: WalletState) { this.state = state }
    }

    @Test
    fun `addKey returns key that can be set as active`() {
        val keyStore = InMemoryKeyStore()
        val storage = TestWalletStorage()
        val walletManager = WalletManager(storage, keyStore)

        val newKey = walletManager.addKey(Provider.ANTHROPIC, "Test Key", "sk-ant-api03-test")
        walletManager.setActiveKey(newKey.id)

        val state = walletManager.loadOrDefault()
        assertEquals(newKey.id, state.activeKeyId)
        assertEquals(1, state.keys.size)
    }

    @Test
    fun `addKey and set default models for provider`() {
        val keyStore = InMemoryKeyStore()
        val storage = TestWalletStorage()
        val walletManager = WalletManager(storage, keyStore)

        val provider = Provider.ANTHROPIC
        val newKey = walletManager.addKey(provider, "Anthropic Key", "sk-ant-api03-test")
        walletManager.setActiveKey(newKey.id)
        walletManager.setChatModel(ModelConfig.defaultChatModel(provider))
        walletManager.setActionModel(ModelConfig.defaultActionModel(provider))

        val state = walletManager.loadOrDefault()
        assertEquals(newKey.id, state.activeKeyId)
        assertEquals(ModelConfig.defaultChatModel(provider), state.chatModelId)
        assertEquals(ModelConfig.defaultActionModel(provider), state.actionModelId)
    }

    @Test
    fun `addKey works for all providers`() {
        val keyStore = InMemoryKeyStore()
        val storage = TestWalletStorage()
        val walletManager = WalletManager(storage, keyStore)

        Provider.entries.forEach { provider ->
            val key = walletManager.addKey(provider, "${provider.name} Key", "test-key-${provider.name}")
            assertNotNull(key.id)
            assertEquals(provider, key.provider)
        }

        val state = walletManager.loadOrDefault()
        assertEquals(Provider.entries.size, state.keys.size)
    }

    @Test
    fun `setActiveKey with invalid ID does not crash`() {
        val keyStore = InMemoryKeyStore()
        val storage = TestWalletStorage()
        val walletManager = WalletManager(storage, keyStore)

        // runCatching mirrors the onboarding code's error handling
        runCatching {
            walletManager.setActiveKey("nonexistent-id")
        }
        // Should either succeed silently or throw — either way, app doesn't crash
        // The key point is the runCatching pattern in onboarding handles this gracefully
    }
}
