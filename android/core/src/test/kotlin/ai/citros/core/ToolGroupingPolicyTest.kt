package ai.citros.core

import org.junit.After
import org.junit.Before
import org.junit.Test
import java.util.concurrent.Callable
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/**
 * Tests for tool grouping policy resolver, context resolver, and integration.
 * Covers all required test cases from Section 8 of the tool grouping spec.
 */
class ToolGroupingPolicyTest {

    @Before
    fun setUp() {
        FeatureFlags.resetToDefaults()
    }

    @After
    fun tearDown() {
        FeatureFlags.resetToDefaults()
    }

    // ========================================================================
    // Section 8.0: Required invariants
    // ========================================================================

    @Test
    fun `invariant 1 - final categories are subset of policy allow set`() {
        // Disable NAVIGATION via user settings
        val settings = UserToolCategorySettings.builder()
            .setEnabled(ToolCategory.NAVIGATION, false)
            .build()
        val plan = ToolGroupingPolicy.resolve(
            messageText = "open settings",
            modelTier = ModelTier.STANDARD,
            capabilities = ToolGroupingPolicy.Capabilities(),
            userSettings = settings
        )
        assertFalse(ToolCategory.NAVIGATION in plan.activeCategories)
    }

    @Test
    fun `invariant 2 - CORE always present in final categories`() {
        // Try various inputs — CORE must always be present
        listOf(
            "hello",
            "open app",
            "",
            "search the web for news"
        ).forEach { msg ->
            val plan = ToolGroupingPolicy.resolve(
                messageText = msg,
                modelTier = ModelTier.STANDARD,
                capabilities = ToolGroupingPolicy.Capabilities(),
                userSettings = UserToolCategorySettings.allEnabled()
            )
            assertTrue(
                ToolCategory.CORE in plan.activeCategories,
                "CORE must always be present for message: '$msg'"
            )
        }
    }

    @Test
    fun `invariant 3 - fallback cannot reintroduce category blocked by security or user disable`() {
        val settings = UserToolCategorySettings.builder()
            .setEnabled(ToolCategory.NAVIGATION, false)
            .build()
        // "open" triggers action fallback, but NAVIGATION is user-disabled
        val plan = ToolGroupingPolicy.resolve(
            messageText = "open the door",
            modelTier = ModelTier.STANDARD,
            capabilities = ToolGroupingPolicy.Capabilities(),
            userSettings = settings
        )
        assertFalse(ToolCategory.NAVIGATION in plan.activeCategories)
    }

    @Test
    fun `invariant 4 - user enable cannot override security or capability deny`() {
        // User enables RESEARCH, but SMALL tier blocks it
        val settings = UserToolCategorySettings.allEnabled()
        val plan = ToolGroupingPolicy.resolve(
            messageText = "search the web",
            modelTier = ModelTier.SMALL,
            capabilities = ToolGroupingPolicy.Capabilities(),
            userSettings = settings
        )
        assertFalse(ToolCategory.RESEARCH in plan.activeCategories)
    }

    @Test
    fun `invariant 5 - resolver-only categories never bypass policy filters`() {
        // Resolver would suggest RESEARCH, but SMALL blocks it
        val plan = ToolGroupingPolicy.resolve(
            messageText = "search online for news",
            modelTier = ModelTier.SMALL,
            capabilities = ToolGroupingPolicy.Capabilities(),
            userSettings = UserToolCategorySettings.allEnabled()
        )
        assertFalse(ToolCategory.RESEARCH in plan.activeCategories)
        assertTrue(plan.reasonCodes.contains(ReasonCode.tier_small_blocks_research))
    }

    // ========================================================================
    // Section 8.1 item 1: Policy precedence matrix (5 required cases)
    // ========================================================================

    @Test
    fun `precedence 1 - SMALL plus user enables RESEARCH plus resolver requests RESEARCH - RESEARCH absent`() {
        val plan = ToolGroupingPolicy.resolve(
            messageText = "search the web for something",
            modelTier = ModelTier.SMALL,
            capabilities = ToolGroupingPolicy.Capabilities(),
            userSettings = UserToolCategorySettings.allEnabled()
        )
        assertFalse(ToolCategory.RESEARCH in plan.activeCategories)
        assertTrue(ReasonCode.tier_small_blocks_research in plan.reasonCodes)
    }

    @Test
    fun `precedence 2 - user disables NAVIGATION plus resolver requests NAVIGATION - NAVIGATION absent`() {
        val settings = UserToolCategorySettings.builder()
            .setEnabled(ToolCategory.NAVIGATION, false)
            .build()
        val plan = ToolGroupingPolicy.resolve(
            messageText = "open the camera app",
            modelTier = ModelTier.STANDARD,
            capabilities = ToolGroupingPolicy.Capabilities(),
            userSettings = settings
        )
        assertFalse(ToolCategory.NAVIGATION in plan.activeCategories)
        assertTrue(ReasonCode.user_disabled_navigation in plan.reasonCodes)
    }

    @Test
    fun `precedence 3 - accessibility detached plus action prompt - phone control categories absent`() {
        val plan = ToolGroupingPolicy.resolve(
            messageText = "open settings and tap wifi",
            modelTier = ModelTier.STANDARD,
            capabilities = ToolGroupingPolicy.Capabilities(accessibilityAttached = false),
            userSettings = UserToolCategorySettings.allEnabled()
        )
        assertFalse(ToolCategory.NAVIGATION in plan.activeCategories)
        assertFalse(ToolCategory.INTERACTION in plan.activeCategories)
        assertFalse(ToolCategory.OBSERVATION in plan.activeCategories)
        assertTrue(ReasonCode.capability_missing_accessibility_blocks_phone_control in plan.reasonCodes)
    }

    @Test
    fun `precedence 4 - tinyfish key missing plus web request - web browse absent`() {
        val plan = ToolGroupingPolicy.resolve(
            messageText = "browse the web for recipes",
            modelTier = ModelTier.STANDARD,
            capabilities = ToolGroupingPolicy.Capabilities(hasTinyFishKey = false),
            userSettings = UserToolCategorySettings.allEnabled()
        )
        assertFalse("web_browse" in plan.toolNames)
        assertTrue(ReasonCode.capability_missing_tinyfish_blocks_web_browse in plan.reasonCodes)
    }

    @Test
    fun `precedence 5 - user attempts CORE disable - CORE still present`() {
        val settings = UserToolCategorySettings.builder()
            .setEnabled(ToolCategory.CORE, false) // Should be silently ignored
            .build()
        val plan = ToolGroupingPolicy.resolve(
            messageText = "hello",
            modelTier = ModelTier.STANDARD,
            capabilities = ToolGroupingPolicy.Capabilities(),
            userSettings = settings
        )
        assertTrue(ToolCategory.CORE in plan.activeCategories)
    }

    // ========================================================================
    // Section 8.1 item 2: Fallback invariants (4 required cases)
    // ========================================================================

    @Test
    fun `fallback 1 - empty resolver output with trigger false - CORE only`() {
        val plan = ToolGroupingPolicy.resolve(
            messageText = "blorp zizzle quantum",
            modelTier = ModelTier.STANDARD,
            capabilities = ToolGroupingPolicy.Capabilities(),
            userSettings = UserToolCategorySettings.allEnabled()
        )
        assertEquals(listOf(ToolCategory.CORE), plan.activeCategories)
        assertTrue(ReasonCode.fallback_empty_candidate_set in plan.reasonCodes)
    }

    @Test
    fun `fallback 2 - empty resolver with trigger true - add fallback categories if allowed`() {
        // "send" is an action verb that triggers fallback even if resolver returns empty
        // But resolver will actually detect "send" as action keyword... let's use a message
        // that has an imperative verb from the spec list but isn't in resolver keywords
        val plan = ToolGroupingPolicy.resolve(
            messageText = "enable dark mode",
            modelTier = ModelTier.STANDARD,
            capabilities = ToolGroupingPolicy.Capabilities(),
            userSettings = UserToolCategorySettings.allEnabled()
        )
        assertTrue(ToolCategory.NAVIGATION in plan.activeCategories)
        assertTrue(ToolCategory.INTERACTION in plan.activeCategories)
        assertTrue(ToolCategory.OBSERVATION in plan.activeCategories)
        assertTrue(ReasonCode.fallback_action_intent in plan.reasonCodes)
    }

    @Test
    fun `fallback 3 - action prompt with NAVIGATION blocked - fallback adds INTERACTION and OBSERVATION only`() {
        val settings = UserToolCategorySettings.builder()
            .setEnabled(ToolCategory.NAVIGATION, false)
            .build()
        // "open settings" triggers action verbs but NAVIGATION is user-disabled
        val plan = ToolGroupingPolicy.resolve(
            messageText = "open settings",
            modelTier = ModelTier.STANDARD,
            capabilities = ToolGroupingPolicy.Capabilities(),
            userSettings = settings
        )
        assertFalse(ToolCategory.NAVIGATION in plan.activeCategories)
        // Fallback should add INTERACTION and OBSERVATION (in allow_set) but not NAVIGATION
        assertTrue(ToolCategory.INTERACTION in plan.activeCategories)
        assertTrue(ToolCategory.OBSERVATION in plan.activeCategories)
    }

    @Test
    fun `fallback 4 - action prompt with NAVIGATION and INTERACTION blocked - CORE plus OBSERVATION if allowed`() {
        val settings = UserToolCategorySettings.builder()
            .setEnabled(ToolCategory.NAVIGATION, false)
            .setEnabled(ToolCategory.INTERACTION, false)
            .setEnabled(ToolCategory.OBSERVATION, false)
            .build()
        val plan = ToolGroupingPolicy.resolve(
            messageText = "open settings",
            modelTier = ModelTier.STANDARD,
            capabilities = ToolGroupingPolicy.Capabilities(),
            userSettings = settings
        )
        // All fallback categories blocked by user
        assertFalse(ToolCategory.NAVIGATION in plan.activeCategories)
        assertFalse(ToolCategory.INTERACTION in plan.activeCategories)
        assertFalse(ToolCategory.OBSERVATION in plan.activeCategories)
    }

    // ========================================================================
    // Context resolver classification tests
    // ========================================================================

    @Test
    fun `resolver - device action verb activates navigation interaction observation`() {
        val result = ContextCategoryResolver.resolve("open the camera app")
        assertTrue(ToolCategory.NAVIGATION in result.candidates)
        assertTrue(ToolCategory.INTERACTION in result.candidates)
        assertTrue(ToolCategory.OBSERVATION in result.candidates)
        assertTrue(result.actionIntent)
    }

    @Test
    fun `resolver - notification keyword activates NOTIFICATION`() {
        val result = ContextCategoryResolver.resolve("check my notifications")
        assertTrue(ToolCategory.NOTIFICATION in result.candidates)
    }

    @Test
    fun `resolver - clipboard keyword activates CLIPBOARD`() {
        val result = ContextCategoryResolver.resolve("copy this text to clipboard")
        assertTrue(ToolCategory.CLIPBOARD in result.candidates)
    }

    @Test
    fun `resolver - memory keyword activates MEMORY`() {
        val result = ContextCategoryResolver.resolve("remember this for later")
        assertTrue(ToolCategory.MEMORY in result.candidates)
    }

    @Test
    fun `resolver - research keyword activates RESEARCH`() {
        val result = ContextCategoryResolver.resolve("search for news about AI")
        assertTrue(ToolCategory.RESEARCH in result.candidates)
    }

    @Test
    fun `resolver - planning keyword activates PLANNING`() {
        val result = ContextCategoryResolver.resolve("help me plan my trip step by step")
        assertTrue(ToolCategory.PLANNING in result.candidates)
    }

    @Test
    fun `resolver - unrecognized message returns empty set`() {
        val result = ContextCategoryResolver.resolve("blorp zizzle quantum")
        assertTrue(result.candidates.isEmpty())
        assertFalse(result.actionIntent)
    }

    @Test
    fun `resolver - CORE is not auto-included in candidates`() {
        listOf("", "hello", "open app", "search web").forEach { msg ->
            val result = ContextCategoryResolver.resolve(msg)
            assertFalse(ToolCategory.CORE in result.candidates)
        }
    }

    // ========================================================================
    // Determinism test
    // ========================================================================

    @Test
    fun `determinism - identical inputs produce identical output`() {
        val settings = UserToolCategorySettings.builder()
            .setEnabled(ToolCategory.NAVIGATION, false)
            .build()
        val caps = ToolGroupingPolicy.Capabilities(hasTinyFishKey = false)

        val plans = (1..10).map {
            ToolGroupingPolicy.resolve(
                messageText = "search the web for recipes",
                modelTier = ModelTier.STANDARD,
                capabilities = caps,
                userSettings = settings
            )
        }

        plans.forEach { plan ->
            assertEquals(plans[0].activeCategories, plan.activeCategories)
            assertEquals(plans[0].toolNames, plan.toolNames)
            assertEquals(plans[0].reasonCodes, plan.reasonCodes)
            assertEquals(plans[0].estimatedToolCount, plan.estimatedToolCount)
        }
    }

    // ========================================================================
    // Reason-code completeness
    // ========================================================================

    @Test
    fun `reason code completeness - every excluded resolver category has a deny reason`() {
        // SMALL tier blocks RESEARCH; resolver requests it
        val plan = ToolGroupingPolicy.resolve(
            messageText = "search for news online",
            modelTier = ModelTier.SMALL,
            capabilities = ToolGroupingPolicy.Capabilities(),
            userSettings = UserToolCategorySettings.allEnabled()
        )
        // RESEARCH was requested by resolver but excluded
        val resolverResult = ContextCategoryResolver.resolve("search for news online")
        assertTrue(ToolCategory.RESEARCH in resolverResult.candidates)
        assertFalse(ToolCategory.RESEARCH in plan.activeCategories)
        assertTrue(ReasonCode.tier_small_blocks_research in plan.reasonCodes)
    }

    @Test
    fun `reason code completeness - user disabled category has deny reason`() {
        val settings = UserToolCategorySettings.builder()
            .setEnabled(ToolCategory.MEMORY, false)
            .build()
        val plan = ToolGroupingPolicy.resolve(
            messageText = "remember this fact",
            modelTier = ModelTier.STANDARD,
            capabilities = ToolGroupingPolicy.Capabilities(),
            userSettings = settings
        )
        assertFalse(ToolCategory.MEMORY in plan.activeCategories)
        assertTrue(ReasonCode.user_disabled_memory in plan.reasonCodes)
    }

    // ========================================================================
    // Section 7.4: Normative examples (all 5)
    // ========================================================================

    @Test
    fun `example A - SMALL tier blocks RESEARCH with action trigger true`() {
        // The resolver will detect "search" as RESEARCH, but the trigger verb
        // mechanism should also fire. SMALL blocks RESEARCH.
        // Since RESEARCH is blocked and resolver candidates include it,
        // and action trigger is true, fallback adds NAV/INT/OBS.
        val plan = ToolGroupingPolicy.resolve(
            messageText = "search and send the results",
            modelTier = ModelTier.SMALL,
            capabilities = ToolGroupingPolicy.Capabilities(),
            userSettings = UserToolCategorySettings.allEnabled()
        )
        assertTrue(ToolCategory.CORE in plan.activeCategories)
        assertTrue(ToolCategory.NAVIGATION in plan.activeCategories)
        assertTrue(ToolCategory.INTERACTION in plan.activeCategories)
        assertTrue(ToolCategory.OBSERVATION in plan.activeCategories)
        // RESEARCH not present (blocked by tier)
        assertFalse(ToolCategory.RESEARCH in plan.activeCategories)
        assertTrue(ReasonCode.tier_small_blocks_research in plan.reasonCodes)
        assertTrue(ReasonCode.fallback_action_intent in plan.reasonCodes)
    }

    @Test
    fun `example B - user disables NAVIGATION with fallback`() {
        val settings = UserToolCategorySettings.builder()
            .setEnabled(ToolCategory.NAVIGATION, false)
            .build()
        val plan = ToolGroupingPolicy.resolve(
            messageText = "enable dark mode",
            modelTier = ModelTier.STANDARD,
            capabilities = ToolGroupingPolicy.Capabilities(),
            userSettings = settings
        )
        // NAVIGATION absent
        assertFalse(ToolCategory.NAVIGATION in plan.activeCategories)
        // Fallback adds INTERACTION, OBSERVATION (allowed)
        assertTrue(ToolCategory.INTERACTION in plan.activeCategories)
        assertTrue(ToolCategory.OBSERVATION in plan.activeCategories)
        assertTrue(ReasonCode.user_disabled_navigation in plan.reasonCodes)
        assertTrue(ReasonCode.fallback_action_intent in plan.reasonCodes)
    }

    @Test
    fun `example C - action trigger false and empty resolver output`() {
        val plan = ToolGroupingPolicy.resolve(
            messageText = "blorp zizzle quantum",
            modelTier = ModelTier.STANDARD,
            capabilities = ToolGroupingPolicy.Capabilities(),
            userSettings = UserToolCategorySettings.allEnabled()
        )
        assertEquals(listOf(ToolCategory.CORE), plan.activeCategories)
        assertTrue(ReasonCode.fallback_empty_candidate_set in plan.reasonCodes)
    }

    @Test
    fun `example D - tool-level pruning keeps category active`() {
        val plan = ToolGroupingPolicy.resolve(
            messageText = "search the web for recipes",
            modelTier = ModelTier.STANDARD,
            capabilities = ToolGroupingPolicy.Capabilities(hasTinyFishKey = false),
            userSettings = UserToolCategorySettings.allEnabled()
        )
        // RESEARCH category may still be active
        assertTrue(ToolCategory.RESEARCH in plan.activeCategories)
        // But web_browse excluded
        assertFalse("web_browse" in plan.toolNames)
        // web_search and web_fetch still present
        assertTrue("web_search" in plan.toolNames)
        assertTrue("web_fetch" in plan.toolNames)
        assertTrue(ReasonCode.capability_missing_tinyfish_blocks_web_browse in plan.reasonCodes)
    }

    @Test
    fun `example E - canonical ordering and serialization shape`() {
        val plan = ToolGroupingPolicy.resolve(
            messageText = "open the camera and take a photo",
            modelTier = ModelTier.STANDARD,
            capabilities = ToolGroupingPolicy.Capabilities(),
            userSettings = UserToolCategorySettings.allEnabled()
        )
        // Active categories should be in canonical order
        val indices = plan.activeCategories.map { ResolvedToolPlan.CANONICAL_ORDER.indexOf(it) }
        assertEquals(indices.sorted(), indices, "Categories must be in canonical order")
        // No duplicates
        assertEquals(plan.activeCategories.size, plan.activeCategories.toSet().size)
        // CORE first
        assertEquals(ToolCategory.CORE, plan.activeCategories.first())
    }

    // ========================================================================
    // Ambiguous prompt negatives (Section 8.1 item 6)
    // ========================================================================

    @Test
    fun `ambiguous - open weather app includes action categories not RESEARCH`() {
        val result = ContextCategoryResolver.resolve("open weather app")
        assertTrue(ToolCategory.NAVIGATION in result.candidates)
        assertTrue(result.actionIntent)
        // "weather" alone isn't a RESEARCH keyword — "weather forecast" is
        // but "weather app" is about opening an app
    }

    @Test
    fun `ambiguous - find weather online includes RESEARCH`() {
        val result = ContextCategoryResolver.resolve("find weather online")
        assertTrue(ToolCategory.RESEARCH in result.candidates)
    }

    // ========================================================================
    // UserToolCategorySettings tests
    // ========================================================================

    @Test
    fun `settings - CORE cannot be disabled`() {
        val settings = UserToolCategorySettings().withEnabled(ToolCategory.CORE, false)
        assertTrue(settings.isEnabled(ToolCategory.CORE))
        assertTrue(settings.disabledCategories().isEmpty() || ToolCategory.CORE !in settings.disabledCategories())
    }

    @Test
    fun `settings - non-core category can be toggled`() {
        val settings = UserToolCategorySettings()
        assertTrue(settings.isEnabled(ToolCategory.NAVIGATION))
        val disabled = settings.withEnabled(ToolCategory.NAVIGATION, false)
        assertFalse(disabled.isEnabled(ToolCategory.NAVIGATION))
        assertTrue(ToolCategory.NAVIGATION in disabled.disabledCategories())
        val enabledAgain = disabled.withEnabled(ToolCategory.NAVIGATION, true)
        assertTrue(enabledAgain.isEnabled(ToolCategory.NAVIGATION))
    }

    @Test
    fun `settings - snapshot creates independent copy`() {
        val settings = UserToolCategorySettings().withEnabled(ToolCategory.MEMORY, false)
        val snapshot = settings.snapshot()
        val updated = settings.withEnabled(ToolCategory.MEMORY, true)
        // Snapshot should still show disabled
        assertFalse(snapshot.isEnabled(ToolCategory.MEMORY))
        assertTrue(updated.isEnabled(ToolCategory.MEMORY))
    }

    // ========================================================================
    // ResolvedToolPlan invariant tests
    // ========================================================================

    @Test
    fun `plan - canonical order helper works correctly`() {
        val ordered = ResolvedToolPlan.canonicalOrder(
            setOf(ToolCategory.RESEARCH, ToolCategory.CORE, ToolCategory.NAVIGATION)
        )
        assertEquals(
            listOf(ToolCategory.CORE, ToolCategory.NAVIGATION, ToolCategory.RESEARCH),
            ordered
        )
    }

    // ========================================================================
    // Action-oriented trigger tests
    // ========================================================================

    @Test
    fun `action trigger - imperative verbs from spec`() {
        assertTrue(ToolGroupingPolicy.isActionOrientedTrigger("open the app", false))
        assertTrue(ToolGroupingPolicy.isActionOrientedTrigger("tap the button", false))
        assertTrue(ToolGroupingPolicy.isActionOrientedTrigger("type your name", false))
        assertTrue(ToolGroupingPolicy.isActionOrientedTrigger("send a message", false))
        assertTrue(ToolGroupingPolicy.isActionOrientedTrigger("enable wifi", false))
        assertTrue(ToolGroupingPolicy.isActionOrientedTrigger("disable bluetooth", false))
        assertTrue(ToolGroupingPolicy.isActionOrientedTrigger("turn on dark mode", false))
        assertTrue(ToolGroupingPolicy.isActionOrientedTrigger("turn off wifi", false))
        assertTrue(ToolGroupingPolicy.isActionOrientedTrigger("launch chrome", false))
    }

    @Test
    fun `action trigger - resolver action intent overrides`() {
        assertTrue(ToolGroupingPolicy.isActionOrientedTrigger("whatever text", true))
    }

    @Test
    fun `action trigger - non-action messages`() {
        assertFalse(ToolGroupingPolicy.isActionOrientedTrigger("what is the weather?", false))
        assertFalse(ToolGroupingPolicy.isActionOrientedTrigger("hello there", false))
    }

    // ========================================================================
    // Feature flag integration tests
    // ========================================================================

    @Test
    fun `feature flag off - getToolsForModelWithGrouping returns legacy behavior`() {
        FeatureFlags.toolGroupingV1Enabled = false
        val client = TestProviderClient()
        val agent = PhoneAgentApi(chatClient = client, actionClient = client).also {
            it.phoneControlOverride = true
        }

        val (tools, plan) = agent.getToolsForModelWithGrouping(
            messageText = "hello",
            userSettings = UserToolCategorySettings.allEnabled()
        )

        assertEquals(null, plan)
        // Legacy includes all tools
        val allToolNames = (PhoneTools.ALL + PhoneTools.API_TOOLS).map { it.name }.toSet()
        // Without tinyfish key, web_browse excluded
        val expected = allToolNames - "web_browse"
        assertEquals(expected, tools.map { it.name }.toSet())
    }

    @Test
    fun `feature flag on - getToolsForModelWithGrouping returns resolved plan`() {
        FeatureFlags.toolGroupingV1Enabled = true
        val client = TestProviderClient()
        val agent = PhoneAgentApi(chatClient = client, actionClient = client).also {
            it.phoneControlOverride = true
        }

        val (tools, plan) = agent.getToolsForModelWithGrouping(
            messageText = "open the camera",
            userSettings = UserToolCategorySettings.allEnabled()
        )

        assertTrue(plan != null)
        assertTrue(ToolCategory.CORE in plan.activeCategories)
        assertTrue(ToolCategory.NAVIGATION in plan.activeCategories)
        // Tools should be filtered to active categories
        assertTrue(tools.size < (PhoneTools.ALL + PhoneTools.API_TOOLS).size)
    }

    @Test
    fun `feature flag on - CORE tools always present even with minimal categories`() {
        FeatureFlags.toolGroupingV1Enabled = true
        val client = TestProviderClient()
        val agent = PhoneAgentApi(chatClient = client, actionClient = client).also {
            it.phoneControlOverride = true
        }

        val (tools, plan) = agent.getToolsForModelWithGrouping(
            messageText = "blorp zizzle quantum",
            userSettings = UserToolCategorySettings.allEnabled()
        )

        assertTrue(plan != null)
        assertEquals(listOf(ToolCategory.CORE), plan.activeCategories)
        val toolNames = tools.map { it.name }.toSet()
        PhoneTools.CORE_TOOL_NAMES.forEach { coreTool ->
            assertTrue(
                coreTool in toolNames,
                "Core tool '$coreTool' must be present even with CORE-only plan"
            )
        }
    }

    @Test
    fun `feature flag resetToDefaults includes toolGroupingV1Enabled`() {
        FeatureFlags.toolGroupingV1Enabled = true
        FeatureFlags.resetToDefaults()
        assertFalse(FeatureFlags.toolGroupingV1Enabled)
    }

    @Test
    fun `integration - request_tools expansion remains bounded by policy allow set`() {
        val plan = ToolGroupingPolicy.resolve(
            messageText = "search and send results",
            modelTier = ModelTier.SMALL,
            capabilities = ToolGroupingPolicy.Capabilities(),
            userSettings = UserToolCategorySettings.allEnabled()
        )
        assertFalse(ToolCategory.RESEARCH in plan.activeCategories)
        assertFalse("web_search" in plan.toolNames)
        assertFalse("web_fetch" in plan.toolNames)
    }

    @Test
    fun `integration - end to end precedence keeps denied category absent`() {
        val settings = UserToolCategorySettings.builder()
            .setEnabled(ToolCategory.NAVIGATION, false)
            .build()
        val plan = ToolGroupingPolicy.resolve(
            messageText = "enable dark mode and search the web",
            modelTier = ModelTier.SMALL,
            capabilities = ToolGroupingPolicy.Capabilities(),
            userSettings = settings
        )
        assertFalse(ToolCategory.NAVIGATION in plan.activeCategories)
        assertFalse(ToolCategory.RESEARCH in plan.activeCategories)
        assertTrue(ReasonCode.user_disabled_navigation in plan.reasonCodes)
        assertTrue(ReasonCode.tier_small_blocks_research in plan.reasonCodes)
        assertTrue(ReasonCode.fallback_action_intent in plan.reasonCodes)
    }

    @Test
    fun `integration - prompt builder category parity TODO stub`() {
        val plan = ToolGroupingPolicy.resolve(
            messageText = "open settings",
            modelTier = ModelTier.STANDARD,
            capabilities = ToolGroupingPolicy.Capabilities(),
            userSettings = UserToolCategorySettings.allEnabled()
        )
        // TODO(#798): when prompt-builder wiring lands, assert prompt category declarations
        // exactly match plan.activeCategories.
        assertTrue(plan.activeCategories.isNotEmpty())
    }

    @Test
    fun `reason code - core_forced_required emitted when resolver does not return CORE`() {
        val plan = ToolGroupingPolicy.resolve(
            messageText = "blorp zizzle quantum",
            modelTier = ModelTier.STANDARD,
            capabilities = ToolGroupingPolicy.Capabilities(),
            userSettings = UserToolCategorySettings.allEnabled()
        )
        assertTrue(ReasonCode.core_forced_required in plan.reasonCodes)
    }

    @Test
    fun `thread safety - resolve invariants hold under concurrent settings updates`() {
        val settingsRef = AtomicReference(UserToolCategorySettings.allEnabled())
        val allowSet = ToolCategory.entries.toSet()
        val pool = Executors.newFixedThreadPool(6)
        try {
            val resolverTasks = (1..80).map {
                Callable {
                    val snapshot = settingsRef.get().snapshot()
                    val plan = ToolGroupingPolicy.resolve(
                        messageText = if (it % 2 == 0) "enable dark mode" else "search for updates",
                        modelTier = if (it % 3 == 0) ModelTier.SMALL else ModelTier.STANDARD,
                        capabilities = ToolGroupingPolicy.Capabilities(accessibilityAttached = it % 5 != 0),
                        userSettings = snapshot
                    )
                    assertTrue(ToolCategory.CORE in plan.activeCategories)
                    assertTrue(plan.activeCategories.all { category -> category in allowSet })
                }
            }
            val mutatorTask = Callable {
                repeat(80) { index ->
                    settingsRef.updateAndGet { current ->
                        current.withEnabled(ToolCategory.NAVIGATION, index % 2 == 0)
                            .withEnabled(ToolCategory.INTERACTION, index % 3 == 0)
                    }
                }
            }
            pool.invokeAll(resolverTasks + mutatorTask).forEach { it.get() }
        } finally {
            pool.shutdown()
            pool.awaitTermination(5, TimeUnit.SECONDS)
        }
    }

    // ========================================================================
    // Context resolver text normalization
    // ========================================================================

    @Test
    fun `resolver normalizes unicode NFKC and lowercases`() {
        // NFKC normalizes ﬁ ligature to "fi"
        val normalized = ContextCategoryResolver.normalizeText("Find the ﬁle")
        assertEquals("find the file", normalized)
    }

    @Test
    fun `resolver collapses whitespace`() {
        val normalized = ContextCategoryResolver.normalizeText("search   the   web")
        assertEquals("search the web", normalized)
    }

    // Helper test client
    private class TestProviderClient(
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
