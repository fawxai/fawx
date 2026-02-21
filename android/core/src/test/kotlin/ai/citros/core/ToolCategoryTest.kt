package ai.citros.core

import kotlinx.coroutines.test.runTest
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertFailsWith
import kotlin.test.assertTrue

class ToolCategoryTest {

    @Test
    fun `all tools in ALL and API_TOOLS have category assignment`() {
        val allTools = (PhoneTools.ALL + PhoneTools.API_TOOLS)
        assertTrue(allTools.isNotEmpty())
        allTools.forEach { tool ->
            assertTrue(
                PhoneTools.hasCategoryAssignment(tool.name),
                "Tool '${tool.name}' is missing an explicit category assignment"
            )
            val category = PhoneTools.categoryOf(tool.name)
            assertTrue(
                category in ToolCategory.entries,
                "Tool '${tool.name}' is missing a valid category assignment"
            )
        }
    }

    @Test
    fun `categoryOf throws for unknown tool`() {
        assertFailsWith<IllegalArgumentException> {
            PhoneTools.categoryOf("nonexistent_tool")
        }
    }

    @Test
    fun `core category tools are always included regardless of filter`() {
        val tools = PhoneTools.getToolsForCategories(setOf(ToolCategory.CLIPBOARD), ModelTier.STANDARD)
        val names = tools.map { it.name }.toSet()

        PhoneTools.CORE_TOOL_NAMES.forEach { coreName ->
            assertTrue(coreName in names, "Expected CORE tool '$coreName' to always be included")
        }
    }

    @Test
    fun `getToolsForCategories with empty set returns only core tools`() {
        val tools = PhoneTools.getToolsForCategories(emptySet(), ModelTier.STANDARD)
        val names = tools.map { it.name }.toSet()

        assertEquals(PhoneTools.CORE_TOOL_NAMES, names)
    }

    @Test
    fun `getToolsForCategories with all categories returns ALL plus API_TOOLS for non SMALL`() {
        val tools = PhoneTools.getToolsForCategories(ToolCategory.entries.toSet(), ModelTier.STANDARD)
        val names = tools.map { it.name }.toSet()
        val expected = (PhoneTools.ALL + PhoneTools.API_TOOLS).map { it.name }.toSet()

        assertEquals(expected, names)
    }

    @Test
    fun `small tier excludes research tools`() {
        val tools = PhoneTools.getToolsForCategories(ToolCategory.entries.toSet(), ModelTier.SMALL)
        val names = tools.map { it.name }.toSet()

        val researchTools = PhoneTools.toolsForCategory(ToolCategory.RESEARCH).map { it.name }.toSet()
        assertTrue(researchTools.isNotEmpty())
        researchTools.forEach { name ->
            assertFalse(name in names, "SMALL tier should exclude research tool '$name'")
        }
    }

    @Test
    fun `request_tools returns tool descriptions for requested categories`() = runTest {
        val client = DummyProviderClient()
        val agent = PhoneAgentApi(chatClient = client, actionClient = client).also {
            it.phoneControlOverride = true
        }

        val result = agent.executeToolCall(
            ToolCall(
                id = "r1",
                name = "request_tools",
                input = mapOf("categories" to listOf("research", "memory"))
            ),
            screenContent = null
        )

        assertFalse(result.isError)
        assertTrue(result.text.contains("web_search"), "Expected research tools in request_tools response")
        assertTrue(result.text.contains("remember"), "Expected memory tools in request_tools response")
    }

    @Test
    fun `request_tools missing categories returns error`() = runTest {
        val client = DummyProviderClient()
        val agent = PhoneAgentApi(chatClient = client, actionClient = client).also {
            it.phoneControlOverride = true
        }

        val result = agent.executeToolCall(
            ToolCall(
                id = "r-missing",
                name = "request_tools",
                input = emptyMap()
            ),
            screenContent = null
        )

        assertTrue(result.isError)
        assertTrue(result.text.contains("Missing required parameter: categories"))
    }

    @Test
    fun `request_tools empty categories returns error`() = runTest {
        val client = DummyProviderClient()
        val agent = PhoneAgentApi(chatClient = client, actionClient = client).also {
            it.phoneControlOverride = true
        }

        val result = agent.executeToolCall(
            ToolCall(
                id = "r-empty",
                name = "request_tools",
                input = mapOf("categories" to emptyList<String>())
            ),
            screenContent = null
        )

        assertTrue(result.isError)
        assertTrue(result.text.contains("at least one category"))
        assertTrue(result.text.contains("navigation"))
    }

    @Test
    fun `request_tools invalid category returns error with valid categories`() = runTest {
        val client = DummyProviderClient()
        val agent = PhoneAgentApi(chatClient = client, actionClient = client).also {
            it.phoneControlOverride = true
        }

        val result = agent.executeToolCall(
            ToolCall(
                id = "r-invalid",
                name = "request_tools",
                input = mapOf("categories" to listOf("nonexistent"))
            ),
            screenContent = null
        )

        assertTrue(result.isError)
        assertTrue(result.text.contains("Invalid categories: nonexistent"))
        assertTrue(result.text.contains("Available:"))
        assertTrue(result.text.contains("research"))
    }

    @Test
    fun `request_tools mixed valid and invalid categories returns error`() = runTest {
        val client = DummyProviderClient()
        val agent = PhoneAgentApi(chatClient = client, actionClient = client).also {
            it.phoneControlOverride = true
        }

        val result = agent.executeToolCall(
            ToolCall(
                id = "r-mixed",
                name = "request_tools",
                input = mapOf("categories" to listOf("memory", "nonexistent"))
            ),
            screenContent = null
        )

        assertTrue(result.isError)
        assertTrue(result.text.contains("Invalid categories: nonexistent"))
    }

    @Test
    fun `request_tools rejects core category input`() = runTest {
        val client = DummyProviderClient()
        val agent = PhoneAgentApi(chatClient = client, actionClient = client).also {
            it.phoneControlOverride = true
        }

        val result = agent.executeToolCall(
            ToolCall(
                id = "r-core",
                name = "request_tools",
                input = mapOf("categories" to listOf("core"))
            ),
            screenContent = null
        )

        assertTrue(result.isError)
        assertTrue(result.text.contains("Invalid categories: core"))
        assertTrue(result.text.contains("Available:"))
    }

    @Test
    fun `request_tools filters web_browse by TinyFish key presence`() = runTest {
        val client = DummyProviderClient()
        val withoutTinyFish = PhoneAgentApi(chatClient = client, actionClient = client).also {
            it.phoneControlOverride = true
        }
        val withTinyFish = PhoneAgentApi(chatClient = client, actionClient = client, tinyFishApiKey = "test-key").also {
            it.phoneControlOverride = true
        }

        val withoutKeyResult = withoutTinyFish.executeToolCall(
            ToolCall(
                id = "r-research-no-key",
                name = "request_tools",
                input = mapOf("categories" to listOf("research"))
            ),
            screenContent = null
        )
        val withKeyResult = withTinyFish.executeToolCall(
            ToolCall(
                id = "r-research-with-key",
                name = "request_tools",
                input = mapOf("categories" to listOf("research"))
            ),
            screenContent = null
        )

        assertFalse(withoutKeyResult.isError)
        assertFalse(withoutKeyResult.text.contains("web_browse"))
        assertFalse(withKeyResult.isError)
        assertTrue(withKeyResult.text.contains("web_browse"))
    }

    @Test
    fun `request_tools respects small model tier exclusions`() = runTest {
        val smallClient = DummyProviderClient(modelId = "claude-3-5-haiku-20241022")
        val agent = PhoneAgentApi(chatClient = smallClient, actionClient = smallClient).also {
            it.phoneControlOverride = true
        }

        val result = agent.executeToolCall(
            ToolCall(
                id = "r-small",
                name = "request_tools",
                input = mapOf("categories" to listOf("research"))
            ),
            screenContent = null
        )

        assertFalse(result.isError)
        assertFalse(result.text.contains("web_search"), "SMALL tier should not expose research tools")
        assertTrue(result.text.contains("read_screen"), "Core tools should still be listed")
    }

    @Test
    fun `buildToolsSection with subset produces shorter output than full`() {
        val full = PhoneAgentPrompts.buildToolsSection(ToolCategory.entries.toSet(), ModelTier.STANDARD)
        val subset = PhoneAgentPrompts.buildToolsSection(setOf(ToolCategory.NAVIGATION), ModelTier.STANDARD)

        assertTrue(subset.length < full.length, "Subset tool section should be shorter than full section")
    }

    @Test
    fun `legacy tools section names match dynamic full tools section names`() {
        val legacy = PhoneAgentPrompts.LEGACY_SECTION_TOOLS
        val dynamic = PhoneAgentPrompts.buildToolsSectionDynamic(ToolCategory.entries.toSet(), ModelTier.STANDARD)

        val nameRegex = Regex("""^- ([a-z_]+)""", RegexOption.MULTILINE)
        val legacyNames = nameRegex.findAll(legacy).map { it.groupValues[1] }.toSet()
        val dynamicNames = nameRegex.findAll(dynamic).map { it.groupValues[1] }.toSet()

        assertEquals(dynamicNames, legacyNames)
    }

    @Test
    fun `backward compatibility getToolsForModel still works`() {
        val client = DummyProviderClient()
        val agent = PhoneAgentApi(chatClient = client, actionClient = client).also {
            it.phoneControlOverride = true
        }

        val standardTools = agent.getToolsForModel("claude-sonnet-4-5-20250929").map { it.name }.toSet()
        assertTrue("web_search" in standardTools)

        val smallTools = agent.getToolsForModel("claude-3-5-haiku-20241022").map { it.name }.toSet()
        assertFalse("web_search" in smallTools)
    }

    private class DummyProviderClient(
        override val modelId: String? = null
    ) : ProviderClient {
        override val provider: Provider = Provider.ANTHROPIC

        override suspend fun chat(conversation: Conversation): Result<String> = Result.success("")

        override suspend fun chatWithTools(
            messages: List<Message>,
            systemPrompt: String?,
            tools: List<Tool>,
            tokenLimit: Int?
        ): Result<ChatResponse> = Result.success(
            ChatResponse(text = "", toolCalls = emptyList(), stopReason = "end_turn")
        )

        override suspend fun describeImage(base64Image: String, prompt: String, maxTokens: Int): Result<String> =
            Result.success("")
    }
}
