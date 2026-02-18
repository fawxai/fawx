package ai.citros.core

import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class ModelConfigTest {

    // ========== actionModelForChat (deprecated) — always returns above-floor default ==========

    @Suppress("DEPRECATION")
    @Test
    fun `actionModelForChat returns default action model for Anthropic`() {
        // Regardless of chat model, action model is the provider's default (Sonnet-tier)
        assertEquals(ModelConfig.ACTION_MODEL, ModelConfig.actionModelForChat(Provider.ANTHROPIC, "claude-sonnet-4-5-20250929"))
        assertEquals(ModelConfig.ACTION_MODEL, ModelConfig.actionModelForChat(Provider.ANTHROPIC, "claude-opus-4-5-20251101"))
        assertEquals(ModelConfig.ACTION_MODEL, ModelConfig.actionModelForChat(Provider.ANTHROPIC, "claude-haiku-4-5-20251001"))
    }

    @Suppress("DEPRECATION")
    @Test
    fun `actionModelForChat returns default action model for OpenRouter`() {
        assertEquals(ModelConfig.OPENROUTER_ACTION_MODEL, ModelConfig.actionModelForChat(Provider.OPENROUTER, "anthropic/claude-sonnet-4.5"))
        assertEquals(ModelConfig.OPENROUTER_ACTION_MODEL, ModelConfig.actionModelForChat(Provider.OPENROUTER, "anthropic/claude-opus-4.5"))
        assertEquals(ModelConfig.OPENROUTER_ACTION_MODEL, ModelConfig.actionModelForChat(Provider.OPENROUTER, "anthropic/claude-haiku-4.5"))
    }

    @Suppress("DEPRECATION")
    @Test
    fun `actionModelForChat returns default action model for OpenAI`() {
        assertEquals(ModelConfig.OPENAI_ACTION_MODEL, ModelConfig.actionModelForChat(Provider.OPENAI, "gpt-4o"))
        assertEquals(ModelConfig.OPENAI_ACTION_MODEL, ModelConfig.actionModelForChat(Provider.OPENAI, "o1"))
        assertEquals(ModelConfig.OPENAI_ACTION_MODEL, ModelConfig.actionModelForChat(Provider.OPENAI, "gpt-4o-mini"))
    }

    @Suppress("DEPRECATION")
    @Test
    fun `actionModelForChat always returns above-floor model`() {
        // Verify for every provider that the result passes the floor check
        for (provider in Provider.entries) {
            val result = ModelConfig.actionModelForChat(provider, "any-model")
            assertTrue(
                ModelConfig.isModelAboveFloor(provider, result),
                "actionModelForChat($provider) returned below-floor model: $result"
            )
        }
    }

    @Suppress("DEPRECATION")
    @Test
    fun `actionModelForChat handles unknown models`() {
        assertEquals(ModelConfig.ACTION_MODEL, ModelConfig.actionModelForChat(Provider.ANTHROPIC, "claude-unknown-future"))
        assertEquals(ModelConfig.OPENROUTER_ACTION_MODEL, ModelConfig.actionModelForChat(Provider.OPENROUTER, "anthropic/claude-future-5.0"))
        assertEquals(ModelConfig.OPENAI_ACTION_MODEL, ModelConfig.actionModelForChat(Provider.OPENAI, "gpt-5-turbo"))
    }

    // ========== Constants — all above floor ==========

    @Test
    fun `action model constants are above floor`() {
        assertTrue(ModelConfig.isModelAboveFloor(Provider.ANTHROPIC, ModelConfig.ACTION_MODEL),
            "ACTION_MODEL must be above floor")
        assertTrue(ModelConfig.isModelAboveFloor(Provider.OPENROUTER, ModelConfig.OPENROUTER_ACTION_MODEL),
            "OPENROUTER_ACTION_MODEL must be above floor")
        assertTrue(ModelConfig.isModelAboveFloor(Provider.OPENAI, ModelConfig.OPENAI_ACTION_MODEL),
            "OPENAI_ACTION_MODEL must be above floor")
    }

    // ========== isModelAboveFloor delegates to ModelClassifier ==========

    @Test
    fun `isModelAboveFloor rejects haiku`() {
        assertFalse(ModelConfig.isModelAboveFloor(Provider.ANTHROPIC, "claude-haiku-4-5-20251001"))
        assertFalse(ModelConfig.isModelAboveFloor(Provider.ANTHROPIC, "claude-3-5-haiku-20241022"))
    }

    @Test
    fun `isModelAboveFloor rejects mini models`() {
        assertFalse(ModelConfig.isModelAboveFloor(Provider.OPENAI, "gpt-4o-mini"))
        assertFalse(ModelConfig.isModelAboveFloor(Provider.OPENAI, "o1-mini"))
    }

    @Test
    fun `isModelAboveFloor accepts sonnet`() {
        assertTrue(ModelConfig.isModelAboveFloor(Provider.ANTHROPIC, "claude-sonnet-4-5-20250929"))
    }

    @Test
    fun `isModelAboveFloor accepts opus`() {
        assertTrue(ModelConfig.isModelAboveFloor(Provider.ANTHROPIC, "claude-opus-4-6"))
    }

    @Test
    fun `isModelAboveFloor accepts gpt-4o`() {
        assertTrue(ModelConfig.isModelAboveFloor(Provider.OPENAI, "gpt-4o"))
    }

    @Test
    fun `isModelAboveFloor accepts unknown models (permissive)`() {
        assertTrue(ModelConfig.isModelAboveFloor(Provider.ANTHROPIC, "future-model-2027"))
    }

    // ========== Model Validation (#268) ==========

    @Test
    fun `validateModel accepts known Anthropic models`() {
        val result = ModelConfig.validateModel(Provider.ANTHROPIC, "claude-sonnet-4-5-20250929")
        assertTrue(result.valid)
        assertNull(result.message)
    }

    @Test
    fun `validateModel rejects unknown model with suggestion`() {
        val result = ModelConfig.validateModel(Provider.ANTHROPIC, "claude-sonnet-4-5-20250930")
        assertFalse(result.valid)
        assertTrue(result.message!!.contains("Unknown model"))
        assertTrue(result.message!!.contains("Did you mean"))
    }

    @Test
    fun `validateModel rejects completely wrong model`() {
        val result = ModelConfig.validateModel(Provider.ANTHROPIC, "gpt-4o")
        assertFalse(result.valid)
        assertTrue(result.message!!.contains("Known models"))
    }

    @Test
    fun `validateModel works for OpenRouter`() {
        assertTrue(ModelConfig.validateModel(Provider.OPENROUTER, "anthropic/claude-sonnet-4.5").valid)
        assertFalse(ModelConfig.validateModel(Provider.OPENROUTER, "anthropic/claude-sonnet-4.6").valid)
    }

    @Test
    fun `validateModel works for OpenAI`() {
        assertTrue(ModelConfig.validateModel(Provider.OPENAI, "gpt-4o").valid)
        assertFalse(ModelConfig.validateModel(Provider.OPENAI, "gpt-5").valid)
    }

    @Test
    fun `validateModel rejects empty model ID`() {
        val result = ModelConfig.validateModel(Provider.ANTHROPIC, "")
        assertFalse(result.valid)
        assertTrue(result.message!!.contains("blank"))
    }

    @Test
    fun `validateModel rejects blank model ID`() {
        val result = ModelConfig.validateModel(Provider.ANTHROPIC, "   ")
        assertFalse(result.valid)
        assertTrue(result.message!!.contains("blank"))
    }

    @Test
    fun `validateModel suggests closest match via Levenshtein distance`() {
        val result = ModelConfig.validateModel(Provider.ANTHROPIC, "claude-sonnet-4-5-20250930")
        assertFalse(result.valid)
        assertTrue(result.message!!.contains("claude-sonnet-4-5-20250929"),
            "Should suggest closest model, got: ${result.message}")
    }

    @Test
    fun `allKnownModels returns union of chat and action models`() {
        val anthropic = ModelConfig.allKnownModels(Provider.ANTHROPIC)
        assertTrue(anthropic.contains("claude-sonnet-4-5-20250929"))
        assertTrue(anthropic.contains("claude-opus-4-6"))
        // Haiku removed from chat models (#455) and was never in action models
        assertFalse(anthropic.contains("claude-haiku-4-5-20251001"))
        assertFalse(anthropic.contains("claude-3-5-haiku-20241022"))
    }

    // ========== ProviderConfig Validation ==========

    // ========== Curated Model Lists (#456) ==========

    @Test
    fun `anthropic chat models include haiku for chat but not action`() {
        val chatModels = ModelConfig.chatModelsForProvider(Provider.ANTHROPIC)
        assertTrue(chatModels.any { it.contains("haiku") }, "Chat should include Haiku")

        val actionModels = ModelConfig.actionModelsForProvider(Provider.ANTHROPIC)
        assertFalse(actionModels.any { it.contains("haiku") }, "Action should not include Haiku (below floor)")
    }

    @Test
    fun `openai chat models include mini but action does not`() {
        val chatModels = ModelConfig.chatModelsForProvider(Provider.OPENAI)
        assertTrue(chatModels.any { it.contains("mini") }, "Chat should include gpt-4o-mini")

        val actionModels = ModelConfig.actionModelsForProvider(Provider.OPENAI)
        assertFalse(actionModels.any { it.contains("mini") }, "Action should not include mini (below floor)")
    }

    @Test
    fun `openrouter chat models include multiple providers`() {
        val models = ModelConfig.chatModelsForProvider(Provider.OPENROUTER)
        assertTrue(models.size >= 4, "OpenRouter should have at least 4 curated models")
        assertTrue(models.any { it.startsWith("anthropic/") }, "Should include Anthropic models")
    }

    @Test
    fun `all action models meet floor`() {
        Provider.entries.forEach { provider ->
            val actionModels = ModelConfig.actionModelsForProvider(provider)
            actionModels.forEach { model ->
                assertTrue(
                    ModelConfig.isModelAboveFloor(provider, model),
                    "Action model '$model' for $provider should be above floor"
                )
            }
        }
    }

    // ========== ProviderConfig Validation ==========

    @Test
    fun `ProviderConfig validateModels returns empty for default config`() {
        val config = ProviderConfig.anthropic("sk-ant-api03-test")
        assertTrue(config.validateModels().isEmpty())
    }

    @Test
    fun `ProviderConfig validateModels warns on unknown chat model`() {
        val config = ProviderConfig.anthropic("sk-ant-api03-test").copy(
            chatModelId = "claude-nonexistent-99"
        )
        val warnings = config.validateModels()
        assertEquals(1, warnings.size)
        assertTrue(warnings[0].contains("Chat model"))
    }

    @Test
    fun `ProviderConfig validateModels warns on both unknown models`() {
        val config = ProviderConfig.anthropic("sk-ant-api03-test").copy(
            chatModelId = "bad-chat",
            actionModelId = "bad-action"
        )
        val warnings = config.validateModels()
        assertEquals(2, warnings.size)
    }
}
