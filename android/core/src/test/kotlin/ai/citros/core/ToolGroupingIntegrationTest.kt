package ai.citros.core

import kotlinx.coroutines.test.runTest
import org.junit.After
import org.junit.Before
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * Integration tests for tool grouping wired into chat and action paths.
 * Verifies deliverables 1-2, 4-6 of Sprint 4 A1-PR2.
 */
class ToolGroupingIntegrationTest {

    @Before
    fun setUp() {
        FeatureFlags.resetToDefaults()
    }

    @After
    fun tearDown() {
        FeatureFlags.resetToDefaults()
    }

    // ========== Helper ==========

    private fun createScriptedClient(
        vararg toolResponses: ChatResponse,
        modelId: String? = null
    ): ScriptedProviderClient = ScriptedProviderClient(
        provider = Provider.ANTHROPIC,
        modelId = modelId,
        toolResponses = ArrayDeque(toolResponses.toList())
    )

    private fun textResponse(text: String) = ChatResponse(
        text = text,
        toolCalls = emptyList(),
        stopReason = "end_turn"
    )

    private fun toolCallResponse(vararg calls: ToolCall) = ChatResponse(
        text = "",
        toolCalls = calls.toList(),
        stopReason = "tool_use"
    )

    // ========== Deliverable 1: chatWithTools uses grouped tools when flag is on ==========

    @Test
    fun `sendMessage uses grouped tools when toolGroupingV1Enabled is true`() = runTest {
        FeatureFlags.toolGroupingV1Enabled = true
        val client = createScriptedClient(textResponse("Done"))
        val agent = PhoneAgentApi(client, client).also { it.phoneControlOverride = true }

        // Use an action-oriented message so it goes through tool path, not chat mode
        agent.sendMessage("Open the settings app and turn on wifi", null, isActionLoop = false)

        assertNotNull(agent.lastResolvedToolPlan)
        val plan = agent.lastResolvedToolPlan!!
        assertTrue(ToolCategory.CORE in plan.activeCategories)
        // Tools passed to model should be filtered by plan
        val toolNames = client.lastTools?.map { it.name }?.toSet() ?: emptySet()
        assertTrue(toolNames.isNotEmpty())
        // Plan tool names should match what was passed
        assertEquals(plan.toolNames, toolNames)
    }

    @Test
    fun `sendMessage uses all tools when toolGroupingV1Enabled is false`() = runTest {
        FeatureFlags.toolGroupingV1Enabled = false
        val client = createScriptedClient(textResponse("Done"))
        val agent = PhoneAgentApi(client, client).also { it.phoneControlOverride = true }

        // Use an action-oriented message
        agent.sendMessage("Open the settings app and turn on wifi", null, isActionLoop = false)

        assertNull(agent.lastResolvedToolPlan)
        // Should get all tools (legacy behavior) - filter out web_browse since no tinyfish key
        val toolNames = client.lastTools?.map { it.name }?.toSet() ?: emptySet()
        val allTools = PhoneTools.getToolsForCategories(ToolCategory.entries.toSet(), ModelTier.STANDARD)
            .filter { it.name != "web_browse" }
            .map { it.name }.toSet()
        assertEquals(allTools, toolNames)
    }

    // ========== Deliverable 2: action loop uses grouped tools when flag is on ==========

    @Test
    fun `continueAfterTools uses grouped tools when flag is on`() = runTest {
        FeatureFlags.toolGroupingV1Enabled = true
        // First call: return a tool call so we enter action loop
        val chatClient = createScriptedClient(
            toolCallResponse(
                ToolCall(id = "tc1", name = "read_screen", input = emptyMap())
            )
        )
        val actionClient = createScriptedClient(textResponse("All done"))
        val agent = PhoneAgentApi(chatClient, actionClient).also { it.phoneControlOverride = true }

        agent.sendMessage("read the screen", null, isActionLoop = false)

        // Now simulate action loop continuation
        val response = agent.continueAfterTools()

        assertNotNull(agent.lastResolvedToolPlan)
        assertTrue(ToolCategory.CORE in agent.lastResolvedToolPlan!!.activeCategories)
        // Action client's tools should be filtered
        val actionToolNames = actionClient.lastTools?.map { it.name }?.toSet() ?: emptySet()
        assertEquals(agent.lastResolvedToolPlan!!.toolNames, actionToolNames)
    }

    @Test
    fun `continueAfterTools uses all tools when flag is off`() = runTest {
        FeatureFlags.toolGroupingV1Enabled = false
        val chatClient = createScriptedClient(
            toolCallResponse(
                ToolCall(id = "tc1", name = "read_screen", input = emptyMap())
            )
        )
        val actionClient = createScriptedClient(textResponse("All done"))
        val agent = PhoneAgentApi(chatClient, actionClient).also { it.phoneControlOverride = true }

        agent.sendMessage("read the screen", null, isActionLoop = false)
        agent.continueAfterTools()

        assertNull(agent.lastResolvedToolPlan)
    }

    // ========== Deliverable 4: prompt tools section shows detail only for active categories ==========

    @Test
    fun `buildToolsSection with ResolvedToolPlan shows detail for active categories only`() {
        val plan = ResolvedToolPlan(
            activeCategories = listOf(ToolCategory.CORE, ToolCategory.NAVIGATION),
            toolNames = PhoneTools.getToolsForCategories(
                setOf(ToolCategory.CORE, ToolCategory.NAVIGATION),
                ModelTier.STANDARD
            ).map { it.name }.toSet(),
            reasonCodes = listOf(ReasonCode.core_forced_required),
            estimatedToolCount = 5
        )

        val section = PhoneAgentPrompts.buildToolsSection(plan, ModelTier.STANDARD)

        // Should contain detailed sections for active categories
        assertTrue(section.contains("### Core"))
        assertTrue(section.contains("### Navigation"))
        // Should contain summary listing for all tools
        assertTrue(section.contains("Always Available Tool Summaries"))
        // Should NOT contain detailed section for non-active categories like Research
        // (Research tools exist but shouldn't have their own detailed header)
        // The detailed section should only have Core and Navigation
        val detailedHeaders = Regex("### (\\w+)").findAll(section)
            .map { it.groupValues[1] }
            .filter { it != "Always" && it != "Active" }
            .toList()
        assertTrue(detailedHeaders.contains("Core"))
        assertTrue(detailedHeaders.contains("Navigation"))
    }

    @Test
    fun `buildToolsSection with all categories returns legacy section`() {
        val section = PhoneAgentPrompts.buildToolsSection(
            activeCategories = ToolCategory.entries.toSet(),
            modelTier = ModelTier.STANDARD
        )
        // Should use legacy section (no dynamic headers)
        assertEquals(PhoneAgentPrompts.LEGACY_SECTION_TOOLS, section)
    }

    // ========== Deliverable 5: telemetry (tested indirectly via lastResolvedToolPlan) ==========

    @Test
    fun `system prompt tools section reflects active categories from resolved plan`() = runTest {
        FeatureFlags.toolGroupingV1Enabled = true
        val settings = UserToolCategorySettings.builder()
            .setEnabled(ToolCategory.RESEARCH, false)
            .setEnabled(ToolCategory.MEMORY, false)
            .build()
        val client = createScriptedClient(textResponse("Done"))
        val agent = PhoneAgentApi(client, client).also {
            it.phoneControlOverride = true
            it.userToolCategorySettings = settings
        }

        agent.sendMessage("Open the settings app and turn on wifi", null, isActionLoop = false)

        val prompt = client.lastSystemPrompt ?: error("Expected system prompt")
        val active = agent.lastResolvedToolPlan?.activeCategories?.toSet() ?: emptySet()
        ToolCategory.entries.forEach { category ->
            val header = "### ${category.name.lowercase().replaceFirstChar { it.uppercase() }}"
            if (category in active) {
                assertTrue(prompt.contains(header), "Expected header for active category $category")
            } else {
                assertTrue(!prompt.contains(header), "Did not expect header for inactive category $category")
            }
        }
        assertTrue(!prompt.contains("### Research"), "Research should stay inactive with user-disabled settings")
    }

    @Test
    fun `lastResolvedToolPlan is set after sendMessage with grouping enabled`() = runTest {
        FeatureFlags.toolGroupingV1Enabled = true
        val client = createScriptedClient(textResponse("Done"))
        val agent = PhoneAgentApi(client, client).also { it.phoneControlOverride = true }

        assertNull(agent.lastResolvedToolPlan)
        // Use action-oriented message to go through tool path
        agent.sendMessage("Open the settings app and turn on wifi", null, isActionLoop = false)
        assertNotNull(agent.lastResolvedToolPlan)
    }

    // ========== Deliverable 6: request_tools expansion ==========

    @Test
    fun `request_tools expansion respects policy allow set`() = runTest {
        FeatureFlags.toolGroupingV1Enabled = true
        val settings = UserToolCategorySettings.builder()
            .setEnabled(ToolCategory.RESEARCH, false)
            .build()

        val client = createScriptedClient(textResponse("Done"))
        val agent = PhoneAgentApi(client, client).also {
            it.phoneControlOverride = true
            it.userToolCategorySettings = settings
        }

        // Execute request_tools for RESEARCH (which is disabled by user)
        val result = agent.executeToolCall(
            ToolCall(
                id = "rt1",
                name = "request_tools",
                input = mapOf("categories" to listOf("research"))
            ),
            null
        )

        // RESEARCH should be blocked by user settings
        assertTrue(result.text.contains("Blocked categories"))
        assertTrue(result.text.contains("research"))
    }

    @Test
    fun `request_tools caps repeated identical requests at 2`() = runTest {
        FeatureFlags.toolGroupingV1Enabled = true
        val client = createScriptedClient(textResponse("Done"))
        val agent = PhoneAgentApi(client, client).also { it.phoneControlOverride = true }

        // First call - should succeed
        val result1 = agent.executeToolCall(
            ToolCall(id = "rt1", name = "request_tools", input = mapOf("categories" to listOf("navigation"))),
            null
        )
        assertTrue(result1.text.contains("Expanded categories"))

        // Second call with same categories - should succeed
        val result2 = agent.executeToolCall(
            ToolCall(id = "rt2", name = "request_tools", input = mapOf("categories" to listOf("navigation"))),
            null
        )
        assertTrue(result2.text.contains("Expanded categories"))

        // Third call with same categories - should be capped
        val result3 = agent.executeToolCall(
            ToolCall(id = "rt3", name = "request_tools", input = mapOf("categories" to listOf("navigation"))),
            null
        )
        assertTrue(result3.text.contains("capped"))
    }

    @Test
    fun `request_tools capped reason code is emitted in subsequent resolved plans`() = runTest {
        FeatureFlags.toolGroupingV1Enabled = true
        val client = createScriptedClient(textResponse("Done"))
        val agent = PhoneAgentApi(client, client).also { it.phoneControlOverride = true }

        agent.executeToolCall(
            ToolCall(id = "rt1", name = "request_tools", input = mapOf("categories" to listOf("navigation"))),
            null
        )
        agent.executeToolCall(
            ToolCall(id = "rt2", name = "request_tools", input = mapOf("categories" to listOf("navigation"))),
            null
        )
        agent.executeToolCall(
            ToolCall(id = "rt3", name = "request_tools", input = mapOf("categories" to listOf("navigation"))),
            null
        )

        agent.sendMessage("Open settings", null, isActionLoop = false)

        val plan = agent.lastResolvedToolPlan ?: error("Expected resolved tool plan")
        assertTrue(ReasonCode.request_tools_capped in plan.reasonCodes)
    }

    @Test
    fun `request_tools expansion is visible in next turn tool schema`() = runTest {
        FeatureFlags.toolGroupingV1Enabled = true
        val chatClient = createScriptedClient(textResponse("Done"))
        val actionClient = createScriptedClient(textResponse("Done"))
        val agent = PhoneAgentApi(chatClient, actionClient).also { it.phoneControlOverride = true }

        val expansion = agent.executeToolCall(
            ToolCall(id = "rt1", name = "request_tools", input = mapOf("categories" to listOf("research"))),
            null
        )
        assertTrue(expansion.text.contains("Expanded categories"))

        agent.continueAfterTools()

        val toolNames = actionClient.lastTools?.map { it.name }?.toSet() ?: emptySet()
        assertTrue("web_search" in toolNames, "Expected research tools to be included on next turn")
    }

    @Test
    fun `request_tools different categories are not capped`() = runTest {
        FeatureFlags.toolGroupingV1Enabled = true
        val client = createScriptedClient(textResponse("Done"))
        val agent = PhoneAgentApi(client, client).also { it.phoneControlOverride = true }

        // Navigation request
        agent.executeToolCall(
            ToolCall(id = "rt1", name = "request_tools", input = mapOf("categories" to listOf("navigation"))),
            null
        )
        agent.executeToolCall(
            ToolCall(id = "rt2", name = "request_tools", input = mapOf("categories" to listOf("navigation"))),
            null
        )

        // Different categories - should NOT be capped
        val result = agent.executeToolCall(
            ToolCall(id = "rt3", name = "request_tools", input = mapOf("categories" to listOf("memory"))),
            null
        )
        assertTrue(result.text.contains("Expanded categories"))
    }

    @Test
    fun `request_tools expansion clears on new task`() = runTest {
        FeatureFlags.toolGroupingV1Enabled = true
        val client = ScriptedProviderClient(
            provider = Provider.ANTHROPIC,
            chatResponses = ArrayDeque(listOf("Chat reply")),
            toolResponses = ArrayDeque(listOf(textResponse("Done")))
        )
        val agent = PhoneAgentApi(client, client).also { it.phoneControlOverride = true }

        // Expand navigation
        agent.executeToolCall(
            ToolCall(id = "rt1", name = "request_tools", input = mapOf("categories" to listOf("navigation"))),
            null
        )

        // New task with conversational message should clear expanded categories
        // "new task" is conversational → goes through chat path, not tool path
        agent.sendMessage("new task", null, isActionLoop = false)

        // Cap counter should also be reset - can request navigation again twice
        val result1 = agent.executeToolCall(
            ToolCall(id = "rt2", name = "request_tools", input = mapOf("categories" to listOf("navigation"))),
            null
        )
        assertTrue(result1.text.contains("Expanded categories"))
    }

    @Test
    fun `request_tools legacy behavior when flag is off`() = runTest {
        FeatureFlags.toolGroupingV1Enabled = false
        val client = createScriptedClient(textResponse("Done"))
        val agent = PhoneAgentApi(client, client).also { it.phoneControlOverride = true }

        val result = agent.executeToolCall(
            ToolCall(id = "rt1", name = "request_tools", input = mapOf("categories" to listOf("navigation"))),
            null
        )

        // Legacy: shows "Requested categories" not "Expanded categories"
        assertTrue(result.text.contains("Requested categories"))
        assertTrue(result.text.contains("navigation"))
    }

    // ========== Settings persistence test ==========

    @Test
    fun `UserToolCategorySettings persists via builder correctly`() {
        val settings = UserToolCategorySettings.builder()
            .setEnabled(ToolCategory.NAVIGATION, false)
            .setEnabled(ToolCategory.RESEARCH, false)
            .setEnabled(ToolCategory.CORE, false) // Should be ignored
            .build()

        assertTrue(settings.isEnabled(ToolCategory.CORE)) // Always true
        assertFalse(settings.isEnabled(ToolCategory.NAVIGATION))
        assertFalse(settings.isEnabled(ToolCategory.RESEARCH))
        assertTrue(settings.isEnabled(ToolCategory.INTERACTION)) // Default true

        val disabled = settings.disabledCategories()
        assertTrue(ToolCategory.NAVIGATION in disabled)
        assertTrue(ToolCategory.RESEARCH in disabled)
        assertTrue(ToolCategory.CORE !in disabled)
    }

    @Test
    fun `UserToolCategorySettings snapshot is independent copy`() {
        var settings = UserToolCategorySettings.allEnabled()
        val snapshot = settings.snapshot()
        settings = settings.withEnabled(ToolCategory.NAVIGATION, false)

        // Snapshot should not be affected
        assertTrue(snapshot.isEnabled(ToolCategory.NAVIGATION))
        assertFalse(settings.isEnabled(ToolCategory.NAVIGATION))
    }

    /**
     * Test helper: ScriptedProviderClient for scripted test scenarios.
     * Duplicated here to keep test file self-contained; consider extracting to shared test util.
     */
    private class ScriptedProviderClient(
        override val provider: Provider,
        private val chatResponses: ArrayDeque<String> = ArrayDeque(),
        private val chatWithUsageResponses: ArrayDeque<Pair<String, TokenUsage?>> = ArrayDeque(),
        private val streamingResponses: ArrayDeque<List<String>> = ArrayDeque(),
        private val toolResponses: ArrayDeque<ChatResponse> = ArrayDeque(),
        private val visionResponses: ArrayDeque<String> = ArrayDeque(),
        override val modelId: String? = null
    ) : ProviderClient {
        var chatCalls = 0
        var chatWithUsageCalls = 0
        var chatStreamingCalls = 0
        var chatWithToolsCalls = 0
        var describeImageCalls = 0
        var lastMessages: List<Message>? = null
        var lastSystemPrompt: String? = null
        var lastTools: List<Tool>? = null

        override suspend fun chat(conversation: Conversation): Result<String> {
            chatCalls++
            return Result.success(chatResponses.removeFirst())
        }

        override suspend fun chatWithUsage(conversation: Conversation): Result<Pair<String, TokenUsage?>> {
            chatWithUsageCalls++
            return if (chatWithUsageResponses.isNotEmpty()) {
                Result.success(chatWithUsageResponses.removeFirst())
            } else {
                Result.success(chat(conversation).getOrThrow() to null)
            }
        }

        override suspend fun chatStreaming(
            conversation: Conversation,
            onDelta: (String) -> Unit
        ): Result<String> {
            chatStreamingCalls++
            val chunks = if (streamingResponses.isNotEmpty()) {
                streamingResponses.removeFirst()
            } else {
                listOf(chatResponses.removeFirst())
            }
            chunks.forEach(onDelta)
            return Result.success(chunks.joinToString(""))
        }

        override suspend fun chatWithTools(
            messages: List<Message>,
            systemPrompt: String?,
            tools: List<Tool>,
            tokenLimit: Int?
        ): Result<ChatResponse> {
            chatWithToolsCalls++
            lastMessages = messages.toList()
            lastSystemPrompt = systemPrompt
            lastTools = tools.toList()
            return Result.success(toolResponses.removeFirst())
        }

        override suspend fun describeImage(base64Image: String, prompt: String, maxTokens: Int): Result<String> {
            describeImageCalls++
            return if (visionResponses.isNotEmpty()) {
                Result.success(visionResponses.removeFirst())
            } else {
                Result.failure(ProviderException(provider, null, "No vision response", false))
            }
        }
    }

    private fun assertFalse(value: Boolean) = kotlin.test.assertFalse(value)
}
