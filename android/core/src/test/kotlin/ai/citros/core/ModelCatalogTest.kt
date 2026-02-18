package ai.citros.core

import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/**
 * Tests for [ModelCatalog] JSON parsing and model filtering.
 *
 * Network-dependent tests (actual API calls) are not included here —
 * these test the parsing and classification logic with sample responses.
 */
class ModelCatalogTest {

    // ========== Anthropic response parsing ==========

    @Test
    fun `parses Anthropic models response`() {
        val json = """
        {
          "data": [
            {"type": "model", "id": "claude-sonnet-4-5-20250929", "display_name": "Claude Sonnet 4.5"},
            {"type": "model", "id": "claude-opus-4-6", "display_name": "Claude Opus 4"},
            {"type": "model", "id": "claude-haiku-4-5-20251001", "display_name": "Claude Haiku 4.5"}
          ],
          "has_more": false
        }
        """.trimIndent()

        val models = ModelCatalog.parseModels(json, Provider.ANTHROPIC)

        assertEquals(3, models.size)
        val ids = models.map { it.id }.toSet()
        assertTrue(ids.contains("claude-sonnet-4-5-20250929"))
        assertTrue(ids.contains("claude-opus-4-6"))
        assertTrue(ids.contains("claude-haiku-4-5-20251001"))
    }

    @Test
    fun `classifies Anthropic models by tier`() {
        val json = """
        {
          "data": [
            {"type": "model", "id": "claude-sonnet-4-5-20250929"},
            {"type": "model", "id": "claude-opus-4-6"},
            {"type": "model", "id": "claude-haiku-4-5-20251001"}
          ]
        }
        """.trimIndent()

        val models = ModelCatalog.parseModels(json, Provider.ANTHROPIC)
        val byId = models.associateBy { it.id }

        assertEquals(ModelTier.STANDARD, byId["claude-sonnet-4-5-20250929"]?.tier)
        assertEquals(ModelTier.FLAGSHIP, byId["claude-opus-4-6"]?.tier)
        assertEquals(ModelTier.SMALL, byId["claude-haiku-4-5-20251001"]?.tier)
    }

    @Test
    fun `extracts display_name from Anthropic response`() {
        val json = """
        {
          "data": [
            {"type": "model", "id": "claude-sonnet-4-5-20250929", "display_name": "Claude Sonnet 4.5"}
          ]
        }
        """.trimIndent()

        val models = ModelCatalog.parseModels(json, Provider.ANTHROPIC)
        assertEquals("Claude Sonnet 4.5", models[0].displayName)
    }

    // ========== OpenAI response parsing ==========

    @Test
    fun `parses OpenAI models and filters non-chat`() {
        val json = """
        {
          "data": [
            {"id": "gpt-4o", "object": "model", "created": 1715367049, "owned_by": "system"},
            {"id": "gpt-4o-mini", "object": "model", "created": 1715367049, "owned_by": "system"},
            {"id": "text-embedding-3-small", "object": "model", "created": 1705948997, "owned_by": "system"},
            {"id": "tts-1", "object": "model", "created": 1705948997, "owned_by": "system"},
            {"id": "dall-e-3", "object": "model", "created": 1705948997, "owned_by": "system"},
            {"id": "whisper-1", "object": "model", "created": 1705948997, "owned_by": "system"},
            {"id": "o1", "object": "model", "created": 1705948997, "owned_by": "system"},
            {"id": "o1-mini", "object": "model", "created": 1705948997, "owned_by": "system"}
          ]
        }
        """.trimIndent()

        val models = ModelCatalog.parseModels(json, Provider.OPENAI)
        val ids = models.map { it.id }.toSet()

        // Chat models included
        assertTrue(ids.contains("gpt-4o"))
        assertTrue(ids.contains("gpt-4o-mini"))
        assertTrue(ids.contains("o1"))
        assertTrue(ids.contains("o1-mini"))

        // Non-chat models excluded
        assertFalse(ids.contains("text-embedding-3-small"))
        assertFalse(ids.contains("tts-1"))
        assertFalse(ids.contains("dall-e-3"))
        assertFalse(ids.contains("whisper-1"))
    }

    @Test
    fun `classifies OpenAI models by tier`() {
        val json = """
        {
          "data": [
            {"id": "gpt-4o", "object": "model"},
            {"id": "gpt-4o-mini", "object": "model"},
            {"id": "o1", "object": "model"},
            {"id": "o1-mini", "object": "model"}
          ]
        }
        """.trimIndent()

        val models = ModelCatalog.parseModels(json, Provider.OPENAI)
        val byId = models.associateBy { it.id }

        assertEquals(ModelTier.STANDARD, byId["gpt-4o"]?.tier)
        assertEquals(ModelTier.SMALL, byId["gpt-4o-mini"]?.tier)
        assertEquals(ModelTier.FLAGSHIP, byId["o1"]?.tier)
        assertEquals(ModelTier.SMALL, byId["o1-mini"]?.tier)
    }

    // ========== OpenRouter response parsing ==========

    @Test
    fun `parses OpenRouter models with name field`() {
        val json = """
        {
          "data": [
            {"id": "anthropic/claude-sonnet-4.5", "name": "Claude Sonnet 4.5", "context_length": 200000},
            {"id": "anthropic/claude-haiku-4.5", "name": "Claude Haiku 4.5", "context_length": 200000},
            {"id": "openai/gpt-4o", "name": "GPT-4o", "context_length": 128000}
          ]
        }
        """.trimIndent()

        val models = ModelCatalog.parseModels(json, Provider.OPENROUTER)

        assertEquals(3, models.size)
        // OpenRouter uses "name" not "display_name"
        assertEquals("Claude Sonnet 4.5", models.find { it.id == "anthropic/claude-sonnet-4.5" }?.displayName)
    }

    // ========== Edge cases ==========

    @Test
    fun `handles empty data array`() {
        val json = """{"data": []}"""
        val models = ModelCatalog.parseModels(json, Provider.ANTHROPIC)
        assertTrue(models.isEmpty())
    }

    @Test
    fun `handles missing data field`() {
        val json = """{"models": []}"""
        val models = ModelCatalog.parseModels(json, Provider.ANTHROPIC)
        assertTrue(models.isEmpty())
    }

    @Test
    fun `skips entries without id`() {
        val json = """
        {
          "data": [
            {"type": "model", "display_name": "No ID Model"},
            {"type": "model", "id": "claude-sonnet-4-5-20250929"}
          ]
        }
        """.trimIndent()

        val models = ModelCatalog.parseModels(json, Provider.ANTHROPIC)
        assertEquals(1, models.size)
        assertEquals("claude-sonnet-4-5-20250929", models[0].id)
    }

    @Test
    fun `handles extra unknown JSON fields gracefully`() {
        val json = """
        {
          "data": [
            {"id": "claude-sonnet-4-5-20250929", "unknown_field": true, "nested": {"foo": "bar"}}
          ],
          "extra_top_level": "ignored"
        }
        """.trimIndent()

        val models = ModelCatalog.parseModels(json, Provider.ANTHROPIC)
        assertEquals(1, models.size)
    }

    // ========== isChatCapableModel filtering ==========

    @Test
    fun `Anthropic filter accepts claude models only`() {
        assertTrue(ModelCatalog.isChatCapableModel("claude-sonnet-4-5-20250929", Provider.ANTHROPIC))
        assertTrue(ModelCatalog.isChatCapableModel("claude-haiku-4-5-20251001", Provider.ANTHROPIC))
        assertFalse(ModelCatalog.isChatCapableModel("some-other-model", Provider.ANTHROPIC))
    }

    @Test
    fun `OpenAI filter excludes embeddings and utility models`() {
        assertTrue(ModelCatalog.isChatCapableModel("gpt-4o", Provider.OPENAI))
        assertTrue(ModelCatalog.isChatCapableModel("o1", Provider.OPENAI))
        assertFalse(ModelCatalog.isChatCapableModel("text-embedding-3-small", Provider.OPENAI))
        assertFalse(ModelCatalog.isChatCapableModel("tts-1", Provider.OPENAI))
        assertFalse(ModelCatalog.isChatCapableModel("dall-e-3", Provider.OPENAI))
        assertFalse(ModelCatalog.isChatCapableModel("whisper-1", Provider.OPENAI))
    }

    @Test
    fun `OpenRouter filter accepts known chat families`() {
        assertTrue(ModelCatalog.isChatCapableModel("anthropic/claude-sonnet-4.5", Provider.OPENROUTER))
        assertTrue(ModelCatalog.isChatCapableModel("openai/gpt-4o", Provider.OPENROUTER))
        assertTrue(ModelCatalog.isChatCapableModel("google/gemini-pro", Provider.OPENROUTER))
        assertTrue(ModelCatalog.isChatCapableModel("meta-llama/llama-3.1-70b", Provider.OPENROUTER))
    }

    // ========== Results sorted by tier ==========

    @Test
    fun `results sorted by tier then ID`() {
        val json = """
        {
          "data": [
            {"id": "claude-haiku-4-5-20251001"},
            {"id": "claude-sonnet-4-5-20250929"},
            {"id": "claude-opus-4-6"}
          ]
        }
        """.trimIndent()

        val models = ModelCatalog.parseModels(json, Provider.ANTHROPIC)
        // FLAGSHIP (0) < STANDARD (1) < SMALL (2)
        assertEquals("claude-opus-4-6", models[0].id)
        assertEquals("claude-sonnet-4-5-20250929", models[1].id)
        assertEquals("claude-haiku-4-5-20251001", models[2].id)
    }

    // ========== Cache ==========

    @Test
    fun `clearCache empties all entries`() {
        ModelCatalog.clearCache()
        // Should not throw, and cache should be empty
        val cached = ModelCatalog.getCachedModels(Provider.ANTHROPIC)
        assertEquals(null, cached)
    }
}
