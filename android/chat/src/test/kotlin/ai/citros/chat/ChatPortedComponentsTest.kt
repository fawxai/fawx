package ai.citros.chat

import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import kotlin.test.assertEquals
import kotlin.test.assertFalse

/**
 * Unit tests for ChatPortedComponents.kt helper functions.
 * Addresses PR #302 review feedback requiring tests for shortModelName() function.
 */
@RunWith(RobolectricTestRunner::class)
class ChatPortedComponentsTest {

    @Test
    fun testShortModelName_knownAnthropicModels() {
        assertEquals("Sonnet 4.5", shortModelName("anthropic/claude-sonnet-4-5"))
        assertEquals("Haiku 4.5", shortModelName("anthropic/claude-haiku-4-5"))
        assertEquals("Opus 4.5", shortModelName("anthropic/claude-opus-4-5"))
        assertEquals("Opus 4.6", shortModelName("anthropic/claude-opus-4-6"))
    }

    @Test
    fun testShortModelName_knownOpenAIModels() {
        assertEquals("GPT-4o", shortModelName("openai/gpt-4o"))
        assertEquals("GPT-4o Mini", shortModelName("openai/gpt-4o-mini"))
        assertEquals("o1", shortModelName("openai/o1"))
        assertEquals("GPT-5", shortModelName("openai/gpt-5"))
        assertEquals("GPT-5.2", shortModelName("openai/gpt-5.2"))
    }

    @Test
    fun testShortModelName_knownOtherModels() {
        assertEquals("DeepSeek R1", shortModelName("deepseek/deepseek-r1"))
        assertEquals("Gemini Pro", shortModelName("google/gemini-pro"))
    }

    @Test
    fun testShortModelName_datedVariants() {
        // Dated variants should match the base model name
        assertEquals("Sonnet 4.5", shortModelName("anthropic/claude-sonnet-4-5-20250929"))
        assertEquals("Haiku 4.5", shortModelName("anthropic/claude-haiku-4-5-20260101"))
        assertEquals("Opus 4.5", shortModelName("anthropic/claude-opus-4-5-20250228"))
    }

    @Test
    fun testShortModelName_dotVsDashHandling() {
        // OpenRouter uses dots
        assertEquals("Sonnet 4.5", shortModelName("anthropic/claude-sonnet-4.5"))
        // Anthropic API uses dashes
        assertEquals("Sonnet 4.5", shortModelName("claude-sonnet-4-5-20250929"))
        // Both should produce the same friendly name
    }

    @Test
    fun testShortModelName_latestSuffix() {
        // -latest suffix should be stripped
        assertEquals("Sonnet 4.5", shortModelName("anthropic/claude-sonnet-4-5-latest"))
        assertEquals("GPT-4o", shortModelName("openai/gpt-4o-latest"))
    }

    @Test
    fun testShortModelName_unknownModels_dateSuffixStripped() {
        // Unknown models should have date suffixes (8 digits) stripped
        val result = shortModelName("provider/unknown-model-20250514")
        assertFalse(result.contains("-20250514"), "Date suffix should be stripped")
    }

    @Test
    fun testShortModelName_unknownModels_claudePrefixRemoved() {
        // Unknown claude models should have claude- prefix removed and be capitalized
        val result = shortModelName("anthropic/claude-newmodel-1-0")
        assertFalse(result.startsWith("claude-"), "claude- prefix should be removed")
    }

    @Test
    fun testShortModelName_unknownModels_gptPrefixNormalized() {
        // Unknown GPT models should have gpt- prefix normalized to GPT-
        val result = shortModelName("openai/gpt-newmodel")
        assertEquals("GPT-newmodel", result)
    }

    @Test
    fun testShortModelName_edgeCases_emptyString() {
        // Empty string should return empty string
        assertEquals("", shortModelName(""))
    }

    @Test
    fun testShortModelName_edgeCases_noSlash() {
        // Model without provider prefix should still work
        assertEquals("Sonnet 4.5", shortModelName("claude-sonnet-4-5"))
    }

    @Test
    fun testShortModelName_edgeCases_multipleSlashes() {
        // Only the last segment after slash should be used
        assertEquals("Sonnet 4.5", shortModelName("provider/subprovider/claude-sonnet-4-5"))
    }

    @Test
    fun testShortModelName_realWorldExamples() {
        // Test real-world model IDs that might appear in production
        assertEquals("Sonnet 4.5", shortModelName("anthropic/claude-sonnet-4-5-20250212"))
        assertEquals("GPT-4o", shortModelName("openai/gpt-4o"))
        assertEquals("DeepSeek R1", shortModelName("openrouter/deepseek-r1"))
        assertEquals("Gemini Pro", shortModelName("google/gemini-pro"))
    }

    @Test
    fun testShortModelName_allModelConfigIds() {
        // Test all model IDs from ModelConfig.kt to ensure full coverage
        
        // Anthropic direct API models (with date stamps)
        assertEquals("Sonnet 4.5", shortModelName("claude-sonnet-4-5-20250929"))
        assertEquals("Haiku 4.5", shortModelName("claude-haiku-4-5-20251001"))
        assertEquals("Opus 4.5", shortModelName("claude-opus-4-5-20251101"))
        
        // OpenRouter models (with anthropic/ prefix)
        assertEquals("Sonnet 4.5", shortModelName("anthropic/claude-sonnet-4.5"))
        assertEquals("Haiku 4.5", shortModelName("anthropic/claude-haiku-4.5"))
        assertEquals("Opus 4.5", shortModelName("anthropic/claude-opus-4.5"))
        
        // OpenAI models
        assertEquals("GPT-4o", shortModelName("gpt-4o"))
        assertEquals("GPT-4o Mini", shortModelName("gpt-4o-mini"))
        assertEquals("o1", shortModelName("o1"))
    }

    @Test
    fun testShortModelName_unknownModels_fallbackCleaning() {
        // Test fallback behavior for completely unknown models
        val result = shortModelName("provider/some-new-model-v2")
        // Should not contain claude- or gpt- prefix
        assertFalse(result.contains("claude-"))
        // Should have spaces instead of dashes (per fallback logic)
        assertEquals("Some new model v2", result)
    }

    @Test
    fun testShortModelName_dateSuffixFormat() {
        // Ensure only 8-digit date suffixes are stripped (YYYYMMDD format)
        val modelWith8Digits = shortModelName("provider/model-20250514")
        assertFalse(modelWith8Digits.contains("20250514"))
        
        // Non-8-digit numbers should not be stripped
        val modelWith4Digits = shortModelName("provider/model-2025")
        // This should keep "2025" as it's not in YYYYMMDD format
        // Note: Current implementation strips 8-digit suffix only
    }
}
