package ai.citros.core

import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/**
 * Tests for [ModelClassifier] tier classification and floor enforcement.
 *
 * The classifier is the security-critical layer that determines which models
 * are permitted in the action loop. These tests verify classification across
 * all providers and naming conventions.
 */
class ModelClassifierTest {

    // ========== SMALL tier (prohibited in action loop) ==========

    @Test
    fun `haiku models are SMALL`() {
        assertEquals(ModelTier.SMALL, ModelClassifier.classify("claude-haiku-4-5-20251001"))
        assertEquals(ModelTier.SMALL, ModelClassifier.classify("claude-3-5-haiku-20241022"))
    }

    @Test
    fun `openrouter haiku is SMALL`() {
        assertEquals(ModelTier.SMALL, ModelClassifier.classify("anthropic/claude-haiku-4.5"))
        assertEquals(ModelTier.SMALL, ModelClassifier.classify("anthropic/claude-3.5-haiku"))
    }

    @Test
    fun `gpt-4o-mini is SMALL`() {
        assertEquals(ModelTier.SMALL, ModelClassifier.classify("gpt-4o-mini"))
    }

    @Test
    fun `o1-mini is SMALL`() {
        assertEquals(ModelTier.SMALL, ModelClassifier.classify("o1-mini"))
    }

    @Test
    fun `o3-mini is SMALL`() {
        assertEquals(ModelTier.SMALL, ModelClassifier.classify("o3-mini"))
    }

    @Test
    fun `openrouter mini models are SMALL`() {
        assertEquals(ModelTier.SMALL, ModelClassifier.classify("openai/gpt-4o-mini"))
        assertEquals(ModelTier.SMALL, ModelClassifier.classify("openai/o1-mini"))
        assertEquals(ModelTier.SMALL, ModelClassifier.classify("openai/o3-mini"))
    }

    @Test
    fun `mini suffix always means SMALL regardless of prefix`() {
        // Even future models with -mini should be classified as SMALL
        assertEquals(ModelTier.SMALL, ModelClassifier.classify("gpt-5-mini"))
        assertEquals(ModelTier.SMALL, ModelClassifier.classify("claude-future-mini"))
    }

    // ========== FLAGSHIP tier ==========

    @Test
    fun `opus models are FLAGSHIP`() {
        assertEquals(ModelTier.FLAGSHIP, ModelClassifier.classify("claude-opus-4-6"))
        assertEquals(ModelTier.FLAGSHIP, ModelClassifier.classify("claude-opus-4-5-20251101"))
        assertEquals(ModelTier.FLAGSHIP, ModelClassifier.classify("claude-opus-4-20250514"))
    }

    @Test
    fun `openrouter opus is FLAGSHIP`() {
        assertEquals(ModelTier.FLAGSHIP, ModelClassifier.classify("anthropic/claude-opus-4.5"))
    }

    @Test
    fun `o1 is FLAGSHIP`() {
        assertEquals(ModelTier.FLAGSHIP, ModelClassifier.classify("o1"))
        assertEquals(ModelTier.FLAGSHIP, ModelClassifier.classify("o1-preview"))
    }

    @Test
    fun `o3 is FLAGSHIP`() {
        assertEquals(ModelTier.FLAGSHIP, ModelClassifier.classify("o3"))
    }

    @Test
    fun `openrouter o1 and o3 are FLAGSHIP`() {
        assertEquals(ModelTier.FLAGSHIP, ModelClassifier.classify("openai/o1"))
        assertEquals(ModelTier.FLAGSHIP, ModelClassifier.classify("openai/o3"))
    }

    @Test
    fun `gpt-5 is FLAGSHIP`() {
        assertEquals(ModelTier.FLAGSHIP, ModelClassifier.classify("gpt-5"))
        assertEquals(ModelTier.FLAGSHIP, ModelClassifier.classify("gpt-5-turbo"))
    }

    // ========== STANDARD tier ==========

    @Test
    fun `sonnet models are STANDARD`() {
        assertEquals(ModelTier.STANDARD, ModelClassifier.classify("claude-sonnet-4-5-20250929"))
        assertEquals(ModelTier.STANDARD, ModelClassifier.classify("claude-sonnet-4-20250514"))
    }

    @Test
    fun `openrouter sonnet is STANDARD`() {
        assertEquals(ModelTier.STANDARD, ModelClassifier.classify("anthropic/claude-sonnet-4.5"))
    }

    @Test
    fun `gpt-4o is STANDARD`() {
        assertEquals(ModelTier.STANDARD, ModelClassifier.classify("gpt-4o"))
    }

    @Test
    fun `gpt-4-turbo is STANDARD`() {
        assertEquals(ModelTier.STANDARD, ModelClassifier.classify("gpt-4-turbo"))
    }

    // ========== Unknown models default to STANDARD ==========

    @Test
    fun `unknown model defaults to STANDARD`() {
        assertEquals(ModelTier.STANDARD, ModelClassifier.classify("some-future-model-v9"))
    }

    @Test
    fun `empty string defaults to STANDARD`() {
        assertEquals(ModelTier.STANDARD, ModelClassifier.classify(""))
    }

    // ========== Floor enforcement ==========

    @Test
    fun `isAboveFloor rejects SMALL models`() {
        assertFalse(ModelClassifier.isAboveFloor("claude-haiku-4-5-20251001"))
        assertFalse(ModelClassifier.isAboveFloor("gpt-4o-mini"))
        assertFalse(ModelClassifier.isAboveFloor("o1-mini"))
    }

    @Test
    fun `isAboveFloor accepts STANDARD models`() {
        assertTrue(ModelClassifier.isAboveFloor("claude-sonnet-4-5-20250929"))
        assertTrue(ModelClassifier.isAboveFloor("gpt-4o"))
    }

    @Test
    fun `isAboveFloor accepts FLAGSHIP models`() {
        assertTrue(ModelClassifier.isAboveFloor("claude-opus-4-6"))
        assertTrue(ModelClassifier.isAboveFloor("o1"))
    }

    @Test
    fun `isAboveFloor accepts unknown models (permissive default)`() {
        assertTrue(ModelClassifier.isAboveFloor("brand-new-model-2027"))
    }

    // ========== Case insensitivity ==========

    @Test
    fun `classification is case insensitive`() {
        assertEquals(ModelTier.SMALL, ModelClassifier.classify("Claude-Haiku-4-5-20251001"))
        assertEquals(ModelTier.FLAGSHIP, ModelClassifier.classify("Claude-Opus-4-6"))
        assertEquals(ModelTier.STANDARD, ModelClassifier.classify("Claude-Sonnet-4-5-20250929"))
    }

    // ========== Edge cases: -mini takes precedence ==========

    @Test
    fun `mini suffix overrides other patterns`() {
        // A hypothetical "opus-mini" should still be SMALL
        assertEquals(ModelTier.SMALL, ModelClassifier.classify("claude-opus-mini"))
        // A hypothetical "sonnet-mini" should still be SMALL
        assertEquals(ModelTier.SMALL, ModelClassifier.classify("claude-sonnet-mini"))
    }
}
