package ai.citros.chat.onboarding

import ai.citros.core.Provider
import kotlinx.coroutines.test.runTest
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class ModelRecommenderTest {

    @Test
    fun `anthropic key recommends sonnet 4_5`() = runTest {
        val recommender = ModelRecommender(FakeModelFetcher(ModelRecommender.ANTHROPIC_MODELS))
        val result = recommender.recommend(Provider.ANTHROPIC, "sk-ant-test")

        assertEquals("claude-sonnet-4-5-latest", result.model)
        assertEquals("Best balance of quality and speed for phone control", result.reason)
        assertTrue(result.alternatives.contains("claude-sonnet-4-latest"))
        assertTrue(result.alternatives.contains("claude-haiku-4-5-latest"))
    }

    @Test
    fun `openrouter with mock model list picks best claude`() = runTest {
        val models = listOf(
            AvailableModel("anthropic/claude-haiku-4-5-latest", "Haiku"),
            AvailableModel("anthropic/claude-sonnet-4-latest", "Sonnet 4"),
            AvailableModel("meta-llama/llama-3-70b", "Llama 3 70B")
        )
        val recommender = ModelRecommender(FakeModelFetcher(models))
        val result = recommender.recommend(Provider.OPENROUTER, "sk-or-test")

        assertEquals("anthropic/claude-sonnet-4-latest", result.model)
        assertTrue(result.alternatives.contains("anthropic/claude-haiku-4-5-latest"))
    }

    @Test
    fun `openrouter with all claude models picks sonnet 4_5`() = runTest {
        val models = listOf(
            AvailableModel("anthropic/claude-sonnet-4-latest", "Sonnet 4"),
            AvailableModel("anthropic/claude-sonnet-4-5-latest", "Sonnet 4.5"),
            AvailableModel("anthropic/claude-haiku-4-5-latest", "Haiku 4.5")
        )
        val recommender = ModelRecommender(FakeModelFetcher(models))
        val result = recommender.recommend(Provider.OPENROUTER, "sk-or-test")

        assertEquals("anthropic/claude-sonnet-4-5-latest", result.model)
    }

    @Test
    fun `network failure uses hardcoded fallback`() = runTest {
        val recommender = ModelRecommender(FailingModelFetcher())
        val result = recommender.recommend(Provider.ANTHROPIC, "sk-ant-test")

        assertEquals("claude-sonnet-4-5-latest", result.model)
        assertTrue(result.alternatives.isNotEmpty())
    }

    @Test
    fun `network failure for openrouter uses openrouter fallback`() = runTest {
        val recommender = ModelRecommender(FailingModelFetcher())
        val result = recommender.recommend(Provider.OPENROUTER, "sk-or-test")

        assertEquals("anthropic/claude-sonnet-4-5-latest", result.model)
    }

    @Test
    fun `empty model list falls back to defaults`() = runTest {
        val recommender = ModelRecommender(FakeModelFetcher(emptyList()))
        val result = recommender.recommend(Provider.ANTHROPIC, "sk-ant-test")

        assertEquals("claude-sonnet-4-5-latest", result.model)
    }

    @Test
    fun `openai fallback returns gpt-4o`() = runTest {
        val recommender = ModelRecommender(FailingModelFetcher())
        val result = recommender.recommend(Provider.OPENAI, "sk-test")

        assertEquals("gpt-4o", result.model)
    }

    @Test
    fun `non-claude models sorted last`() = runTest {
        val models = listOf(
            AvailableModel("meta-llama/llama-3-70b", "Llama"),
            AvailableModel("anthropic/claude-haiku-4-5-latest", "Haiku")
        )
        val recommender = ModelRecommender(FakeModelFetcher(models))
        val result = recommender.recommend(Provider.OPENROUTER, "sk-or-test")

        assertEquals("anthropic/claude-haiku-4-5-latest", result.model)
    }

    @Test
    fun `parseOpenRouterModels extracts only claude from mixed list`() {
        val json = """
            {"data":[
              {"id":"anthropic/claude-sonnet-4-5-latest","name":"Claude Sonnet 4.5"},
              {"id":"openai/gpt-4o","name":"GPT-4o"},
              {"id":"anthropic/claude-haiku-4-5-latest","name":"Claude Haiku 4.5"}
            ]}
        """.trimIndent()

        val parsed = DefaultModelFetcher.parseOpenRouterModels(json)

        assertEquals(2, parsed.size)
        assertEquals("anthropic/claude-sonnet-4-5-latest", parsed[0].id)
        assertEquals("anthropic/claude-haiku-4-5-latest", parsed[1].id)
    }

    @Test
    fun `parseOpenRouterModels returns empty list on empty data`() {
        val parsed = DefaultModelFetcher.parseOpenRouterModels("{" + "\"data\":[]}")
        assertTrue(parsed.isEmpty())
    }

    @Test
    fun `parseOpenRouterModels returns empty list on malformed json`() {
        val parsed = DefaultModelFetcher.parseOpenRouterModels("{bad json")
        assertTrue(parsed.isEmpty())
    }

    @Test
    fun `parseOpenRouterModels returns empty list when no claude models`() {
        val json = """
            {"data":[
              {"id":"openai/gpt-4o","name":"GPT-4o"},
              {"id":"meta-llama/llama-3.3-70b","name":"Llama"}
            ]}
        """.trimIndent()

        val parsed = DefaultModelFetcher.parseOpenRouterModels(json)
        assertTrue(parsed.isEmpty())
    }

    @Test
    fun `parseOpenRouterModels preserves special characters in model id`() {
        val json = """
            {"data":[{"id":"anthropic/claude-sonnet-4.5:beta@2026","name":"Claude"}]}
        """.trimIndent()

        val parsed = DefaultModelFetcher.parseOpenRouterModels(json)

        assertEquals(1, parsed.size)
        assertEquals("anthropic/claude-sonnet-4.5:beta@2026", parsed.first().id)
    }

    @Test
    fun `available model exposes displayId`() {
        val model = AvailableModel("anthropic/claude-sonnet-4-5-latest", "Claude")
        assertEquals("claude-sonnet-4-5-latest", model.displayId)
    }

    private class FakeModelFetcher(private val models: List<AvailableModel>) : ModelFetcher {
        override suspend fun fetchAvailableModels(
            provider: Provider,
            apiKey: String
        ): List<AvailableModel> = models
    }

    private class FailingModelFetcher : ModelFetcher {
        override suspend fun fetchAvailableModels(
            provider: Provider,
            apiKey: String
        ): List<AvailableModel> = throw RuntimeException("Network error")
    }
}
