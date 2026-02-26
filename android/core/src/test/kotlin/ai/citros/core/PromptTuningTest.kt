package ai.citros.core

import org.junit.After
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test
import java.time.Instant
import java.util.concurrent.CyclicBarrier
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

/**
 * Tests for H2.4 Model-Aware Prompt Tuning.
 *
 * Spec mapping quick-reference:
 * - UT-H24-001..006: unit behaviors (matrix, safety, trimming, runtime, guard, tool groups)
 * - IT-H24-001..003: integration paths (API prompt mode selection, accessibility detached, tier inference)
 * - CT-H24-001..002: concurrency/thread-safety checks
 */
class PromptTuningTest {

    @Before
    fun setUp() {
        FeatureFlags.resetToDefaults()
        FeatureFlags.promptTuningV1Enabled = true
    }

    @After
    fun tearDown() {
        FeatureFlags.resetToDefaults()
    }

    private val testTimestamp = Instant.parse("2026-02-22T18:04:27Z")

    // ── UT-H24-001: Full matrix coverage (mode × tier × capability) ────

    @Test
    fun `UT-H24-001 FULL FLAGSHIP with phone control includes all sections`() {
        val result = PhoneAgentPrompts.buildTunedSystemPrompt(
            mode = PromptMode.FULL,
            modelTier = ModelTier.FLAGSHIP,
            phoneControlAvailable = true,
            modelName = "claude-opus-4-6",
            timestamp = testTimestamp
        )
        assertContains(result.finalPrompt, "You are Citros")
        assertContains(result.finalPrompt, "## Your Tools")
        assertContains(result.finalPrompt, "## Strategy")
        assertContains(result.finalPrompt, "## Security Rules")
        assertContains(result.finalPrompt, "Canonical Safety Clauses")
        assertContains(result.finalPrompt, "runtime|ts=")
    }

    @Test
    fun `UT-H24-001 FULL STANDARD with phone control includes all sections`() {
        val result = PhoneAgentPrompts.buildTunedSystemPrompt(
            mode = PromptMode.FULL,
            modelTier = ModelTier.STANDARD,
            phoneControlAvailable = true,
            modelName = "gpt-4o",
            timestamp = testTimestamp
        )
        assertContains(result.finalPrompt, "## Your Tools")
        assertContains(result.finalPrompt, "## Strategy")
        assertContains(result.finalPrompt, "## Security Rules")
    }

    @Test
    fun `UT-H24-001 FULL SMALL with phone control has reduced verbosity`() {
        val result = PhoneAgentPrompts.buildTunedSystemPrompt(
            mode = PromptMode.FULL,
            modelTier = ModelTier.SMALL,
            phoneControlAvailable = true,
            modelName = "gpt-4o-mini",
            timestamp = testTimestamp
        )
        // SMALL gets compact tools and strategy
        assertContains(result.finalPrompt, "## Your Tools")
        assertContains(result.finalPrompt, "## Security Rules")
        // Should be shorter than STANDARD
        val standardResult = PhoneAgentPrompts.buildTunedSystemPrompt(
            mode = PromptMode.FULL,
            modelTier = ModelTier.STANDARD,
            phoneControlAvailable = true,
            modelName = "gpt-4o",
            timestamp = testTimestamp
        )
        assertTrue(
            "SMALL (${result.charCount}) should be <= STANDARD (${standardResult.charCount})",
            result.charCount <= standardResult.charCount
        )
    }

    @Test
    fun `UT-H24-001 MINIMAL modes include security and reminders but not tools`() {
        for (tier in ModelTier.entries) {
            val result = PhoneAgentPrompts.buildTunedSystemPrompt(
                mode = PromptMode.MINIMAL,
                modelTier = tier,
                phoneControlAvailable = true,
                modelName = "test-model",
                timestamp = testTimestamp
            )
            assertContains(result.finalPrompt, "## Security Rules")
            assertContains(result.finalPrompt, "## Key Reminders")
            assertNotContains(result.finalPrompt, "## Your Tools")
        }
    }

    @Test
    fun `UT-H24-001 NONE mode is identity only for all tiers`() {
        for (tier in ModelTier.entries) {
            val result = PhoneAgentPrompts.buildTunedSystemPrompt(
                mode = PromptMode.NONE,
                modelTier = tier,
                phoneControlAvailable = false,
                modelName = "test-model",
                timestamp = testTimestamp
            )
            assertContains(result.finalPrompt, "You are Citros")
            assertNotContains(result.finalPrompt, "## Security Rules")
            assertContains(result.finalPrompt, "runtime|ts=") // runtime line always present
        }
    }

    @Test
    fun `UT-H24-001 accessibility detached includes warning and strips tool guidance`() {
        val result = PhoneAgentPrompts.buildTunedSystemPrompt(
            mode = PromptMode.FULL,
            modelTier = ModelTier.STANDARD,
            phoneControlAvailable = false,
            modelName = "gpt-4o",
            timestamp = testTimestamp
        )
        assertContains(result.finalPrompt, "Accessibility service is NOT attached")
        assertNotContains(result.finalPrompt, "## Your Tools")
        assertContains(result.finalPrompt, "accessibility=detached")
    }

    // ── UT-H24-002: Canonical safety clauses present + equivalent ───────

    @Test
    fun `UT-H24-002 FULL mode contains all canonical safety clauses`() {
        for (tier in ModelTier.entries) {
            val result = PhoneAgentPrompts.buildTunedSystemPrompt(
                mode = PromptMode.FULL,
                modelTier = tier,
                phoneControlAvailable = true,
                modelName = "test-model",
                timestamp = testTimestamp
            )
            val missing = PromptSafetyContract.findMissingClauses(result.finalPrompt)
            assertTrue("FULL/$tier missing clauses: $missing", missing.isEmpty())
        }
    }

    @Test
    fun `UT-H24-002 MINIMAL mode contains all canonical safety clauses`() {
        for (tier in ModelTier.entries) {
            val result = PhoneAgentPrompts.buildTunedSystemPrompt(
                mode = PromptMode.MINIMAL,
                modelTier = tier,
                phoneControlAvailable = true,
                modelName = "test-model",
                timestamp = testTimestamp
            )
            val missing = PromptSafetyContract.findMissingClauses(result.finalPrompt)
            assertTrue("MINIMAL/$tier missing clauses: $missing", missing.isEmpty())
        }
    }

    @Test
    fun `UT-H24-002 safety equivalence after normalization`() {
        val prompt = PhoneAgentPrompts.buildTunedSystemPrompt(
            mode = PromptMode.FULL,
            modelTier = ModelTier.STANDARD,
            phoneControlAvailable = true,
            modelName = "gpt-4o",
            timestamp = testTimestamp
        ).finalPrompt

        for ((id, canonical) in PromptSafetyContract.ALL_CLAUSES) {
            val normalizedCanonical = PromptSafetyContract.normalize(canonical)
            val normalizedPrompt = PromptSafetyContract.normalize(prompt)
            assertTrue(
                "$id not found after normalization",
                normalizedPrompt.contains(normalizedCanonical)
            )
        }
    }

    // ── UT-H24-003: Over-budget triggers deterministic trimming ─────────

    @Test
    fun `UT-H24-003 over-budget FULL prompt trims in correct order`() {
        val hardChars = PromptBudget.BUDGETS.getValue(PromptMode.FULL).hardBudget * 4
        val sections = listOf(
            PromptBudget.LabeledSection(PromptBudget.SectionId.IDENTITY_BASELINE, "identity"),
            PromptBudget.LabeledSection(PromptBudget.SectionId.SECURITY_BLOCK, "security"),
            PromptBudget.LabeledSection(PromptBudget.SectionId.CRITICAL_EXECUTION_RULES, "rules"),
            PromptBudget.LabeledSection(PromptBudget.SectionId.VERBOSE_EXAMPLES, "v".repeat(hardChars)),
            PromptBudget.LabeledSection(PromptBudget.SectionId.COMMUNICATION_STYLE, "c".repeat(hardChars)),
            PromptBudget.LabeledSection(PromptBudget.SectionId.RECOVERY_ELABORATION, "r".repeat(hardChars))
        )

        val result = PromptBudget.enforce(sections, PromptMode.FULL)

        assertTrue("Trimming must occur for oversized trimmable sections", result.trimmed)
        assertTrue(
            "Prompt must end within hard budget after trimming",
            result.tokenEstimate <= PromptBudget.BUDGETS.getValue(PromptMode.FULL).hardBudget
        )
        if (PromptBudget.SectionId.COMMUNICATION_STYLE in result.trimmedSections) {
            assertTrue(result.trimmedSections.contains(PromptBudget.SectionId.VERBOSE_EXAMPLES))
        }
    }

    @Test
    fun `UT-H24-003 trimming preserves never-trim sections`() {
        // Total must exceed hard budget of 2600 tokens (10400 chars)
        val sections = listOf(
            PromptBudget.LabeledSection(PromptBudget.SectionId.IDENTITY_BASELINE, "identity " + "x".repeat(1000)),
            PromptBudget.LabeledSection(PromptBudget.SectionId.SECURITY_BLOCK, "security " + "x".repeat(1000)),
            PromptBudget.LabeledSection(PromptBudget.SectionId.CRITICAL_EXECUTION_RULES, "rules " + "x".repeat(1000)),
            PromptBudget.LabeledSection(PromptBudget.SectionId.VERBOSE_EXAMPLES, "examples " + "x".repeat(3000)),
            PromptBudget.LabeledSection(PromptBudget.SectionId.COMMUNICATION_STYLE, "comms " + "x".repeat(3000)),
            PromptBudget.LabeledSection(PromptBudget.SectionId.RECOVERY_ELABORATION, "recovery " + "x".repeat(3000)),
            PromptBudget.LabeledSection(PromptBudget.SectionId.STRATEGY_DETAIL, "strategy " + "x".repeat(3000)),
        )
        val result = PromptBudget.enforce(sections, PromptMode.FULL)

        // Identity, security, rules must survive
        assertContains(result.finalPrompt, "identity")
        assertContains(result.finalPrompt, "security")
        assertContains(result.finalPrompt, "rules")

        // At least some trimming happened
        assertTrue("Should have trimmed", result.trimmed)
        // Trim order: verbose_examples first
        assertTrue(result.trimmedSections.contains(PromptBudget.SectionId.VERBOSE_EXAMPLES))
    }

    @Test
    fun `UT-H24-003 deterministic trim order is lowest priority first`() {
        // Total: ~15000 chars = ~3750 tokens, well over FULL hard budget of 2600
        val sections = listOf(
            PromptBudget.LabeledSection(PromptBudget.SectionId.IDENTITY_BASELINE, "id " + "x".repeat(1000)),
            PromptBudget.LabeledSection(PromptBudget.SectionId.SECURITY_BLOCK, "sec " + "x".repeat(1000)),
            PromptBudget.LabeledSection(PromptBudget.SectionId.VERBOSE_EXAMPLES, "ve " + "x".repeat(4000)),
            PromptBudget.LabeledSection(PromptBudget.SectionId.COMMUNICATION_STYLE, "cs " + "x".repeat(4000)),
            PromptBudget.LabeledSection(PromptBudget.SectionId.RECOVERY_ELABORATION, "re " + "x".repeat(4000)),
        )
        val result = PromptBudget.enforce(sections, PromptMode.FULL)
        assertTrue(result.trimmed)
        // verbose_examples should be trimmed before communication_style
        val trimmed = result.trimmedSections
        if (trimmed.contains(PromptBudget.SectionId.COMMUNICATION_STYLE)) {
            assertTrue(
                "verbose_examples should be trimmed before communication_style",
                trimmed.contains(PromptBudget.SectionId.VERBOSE_EXAMPLES)
            )
        }
    }

    // ── UT-H24-004: Runtime line parses against schema ──────────────────

    @Test
    fun `UT-H24-004 runtime line matches schema regex`() {
        val result = PhoneAgentPrompts.buildTunedSystemPrompt(
            mode = PromptMode.FULL,
            modelTier = ModelTier.STANDARD,
            phoneControlAvailable = true,
            modelName = "gpt-4o",
            timestamp = testTimestamp
        )
        val runtimeLine = result.finalPrompt.lines().last { it.startsWith("runtime|") }
        assertTrue(
            "Runtime line should match schema: $runtimeLine",
            RuntimeLine.SCHEMA_REGEX.matches(runtimeLine)
        )
    }

    @Test
    fun `UT-H24-004 runtime line has fixed key order`() {
        val line = RuntimeLine.build(
            modelName = "gpt-4o",
            tier = ModelTier.STANDARD,
            mode = PromptMode.FULL,
            accessibility = "attached",
            toolPolicy = "full",
            promptChars = 5000,
            promptTokensEst = 1250,
            trimmed = false,
            timestamp = testTimestamp
        )
        val parsed = RuntimeLine.parse(line)!!
        val keys = parsed.asMap().keys.toList()
        assertEquals(
            listOf("ts", "model", "tier", "mode", "accessibility", "tool_policy", "prompt_chars", "prompt_tokens_est", "trimmed", "trimmed_sections"),
            keys
        )
    }

    @Test
    fun `UT-H24-004 runtime line timestamp is RFC3339 UTC`() {
        val line = RuntimeLine.build(
            modelName = "gpt-4o",
            tier = ModelTier.STANDARD,
            mode = PromptMode.FULL,
            accessibility = "attached",
            toolPolicy = "full",
            promptChars = 100,
            promptTokensEst = 25,
            trimmed = false,
            timestamp = testTimestamp
        )
        assertContains(line, "ts=2026-02-22T18:04:27Z")
    }

    @Test
    fun `UT-H24-004 trimmed sections are sorted lexicographically`() {
        val line = RuntimeLine.build(
            modelName = "test",
            tier = ModelTier.STANDARD,
            mode = PromptMode.FULL,
            accessibility = "attached",
            toolPolicy = "full",
            promptChars = 100,
            promptTokensEst = 25,
            trimmed = true,
            trimmedSections = listOf("strategy_detail", "communication_style", "verbose_examples"),
            timestamp = testTimestamp
        )
        assertContains(line, "trimmed_sections=communication_style,strategy_detail,verbose_examples")
    }

    @Test
    fun `UT-H24-004 no trimming produces trimmed_sections=none`() {
        val line = RuntimeLine.build(
            modelName = "test",
            tier = ModelTier.STANDARD,
            mode = PromptMode.FULL,
            accessibility = "attached",
            toolPolicy = "full",
            promptChars = 100,
            promptTokensEst = 25,
            trimmed = false,
            timestamp = testTimestamp
        )
        assertContains(line, "trimmed=false|trimmed_sections=none")
    }

    // ── UT-H24-005: Tool-enabled paths reject NONE ──────────────────────

    @Test(expected = IllegalArgumentException::class)
    fun `UT-H24-005 guardModeSelection rejects NONE for tool-capable turns`() {
        PhoneAgentPrompts.guardModeSelection(PromptMode.NONE, toolCapable = true)
    }

    @Test
    fun `UT-H24-005 guardModeSelection allows NONE for non-tool turns`() {
        PhoneAgentPrompts.guardModeSelection(PromptMode.NONE, toolCapable = false)
    }

    @Test
    fun `UT-H24-005 guardModeSelection allows FULL and MINIMAL for tool-capable turns`() {
        PhoneAgentPrompts.guardModeSelection(PromptMode.FULL, toolCapable = true)
        PhoneAgentPrompts.guardModeSelection(PromptMode.MINIMAL, toolCapable = true)
    }

    // ── UT-H24-006: Tool groups match allowed tools per tier ────────────

    @Test
    fun `UT-H24-006 SMALL tier excludes research tools`() {
        val allCategories = ToolCategory.entries.toSet()
        val smallTools = PhoneTools.getToolsForCategories(allCategories, ModelTier.SMALL)
        val standardTools = PhoneTools.getToolsForCategories(allCategories, ModelTier.STANDARD)

        val smallNames = smallTools.map { it.name }.toSet()
        val standardNames = standardTools.map { it.name }.toSet()

        // SMALL should have fewer tools (no research)
        assertTrue(
            "SMALL ($smallNames) should be subset of STANDARD ($standardNames)",
            smallNames.size <= standardNames.size
        )

        // Verify research tools are excluded from SMALL
        val researchTools = PhoneTools.getToolsForCategories(setOf(ToolCategory.RESEARCH), ModelTier.STANDARD)
            .map { it.name }
            .filter { it !in PhoneTools.CORE_TOOL_NAMES }
        for (toolName in researchTools) {
            if (toolName !in PhoneTools.CORE_TOOL_NAMES) {
                assertFalse(
                    "Research tool '$toolName' should not be in SMALL tier",
                    smallNames.contains(toolName)
                )
            }
        }
    }

    @Test
    fun `UT-H24-006 FLAGSHIP and STANDARD have same tool set`() {
        val allCategories = ToolCategory.entries.toSet()
        val flagshipTools = PhoneTools.getToolsForCategories(allCategories, ModelTier.FLAGSHIP).map { it.name }.toSet()
        val standardTools = PhoneTools.getToolsForCategories(allCategories, ModelTier.STANDARD).map { it.name }.toSet()
        assertEquals(flagshipTools, standardTools)
    }

    // ── IT-H24-001..003: Integration-path checks ───────────────────────

    @Test
    fun `IT-H24-001 buildSystemPrompt uses FULL mode and buildActionPrompt uses MINIMAL mode`() {
        val fullPrompt = PhoneAgentPrompts.buildSystemPrompt(
            phoneControlAvailable = true,
            modelName = "gpt-4o",
            modelTier = ModelTier.STANDARD
        )
        val actionPrompt = PhoneAgentPrompts.buildActionPrompt(
            phoneControlAvailable = true,
            modelName = "gpt-4o",
            modelTier = ModelTier.STANDARD
        )

        assertContains(fullPrompt, "mode=FULL")
        assertContains(actionPrompt, "mode=MINIMAL")
    }

    @Test
    fun `IT-H24-002 integration path emits detached accessibility mode`() {
        val fullPrompt = PhoneAgentPrompts.buildSystemPrompt(
            phoneControlAvailable = false,
            modelName = "gpt-4o",
            modelTier = ModelTier.STANDARD
        )

        assertContains(fullPrompt, "Accessibility service is NOT attached")
        assertContains(fullPrompt, "accessibility=detached")
        assertNotContains(fullPrompt, "## Your Tools")
    }

    @Test
    fun `IT-H24-003 tier inference falls back from modelName when tier omitted`() {
        val fullPrompt = PhoneAgentPrompts.buildSystemPrompt(
            phoneControlAvailable = true,
            modelName = "gpt-4o-mini",
            modelTier = null
        )

        assertContains(fullPrompt, "tier=SMALL")
        assertContains(fullPrompt, "tool_policy=small_restricted")
    }

    // ── CT-H24-001: Concurrent builds have no cross-request contamination ───

    @Test
    fun `CT-H24-001 concurrent prompt builds produce isolated results`() {
        val threadCount = 8
        val barrier = CyclicBarrier(threadCount)
        val resultPairs = CopyOnWriteArrayList<Pair<Triple<PromptMode, ModelTier, Boolean>, PromptBudget.BudgetResult>>()
        val executor = Executors.newFixedThreadPool(threadCount)

        val configs = listOf(
            Triple(PromptMode.FULL, ModelTier.FLAGSHIP, true),
            Triple(PromptMode.FULL, ModelTier.STANDARD, true),
            Triple(PromptMode.FULL, ModelTier.SMALL, true),
            Triple(PromptMode.MINIMAL, ModelTier.FLAGSHIP, true),
            Triple(PromptMode.MINIMAL, ModelTier.STANDARD, false),
            Triple(PromptMode.MINIMAL, ModelTier.SMALL, true),
            Triple(PromptMode.NONE, ModelTier.STANDARD, false),
            Triple(PromptMode.FULL, ModelTier.STANDARD, false),
        )

        val futures = configs.map { (mode, tier, phoneControl) ->
            executor.submit<PromptBudget.BudgetResult> {
                barrier.await()
                PhoneAgentPrompts.buildTunedSystemPrompt(
                    mode = mode,
                    modelTier = tier,
                    phoneControlAvailable = phoneControl,
                    modelName = "test-$tier",
                    timestamp = testTimestamp
                )
            }
        }

        configs.zip(futures).forEach { (config, future) ->
            val result = future.get(10, TimeUnit.SECONDS)
            resultPairs.add(config to result)
            val (mode, tier, phoneControl) = config

            // Verify each result is self-consistent
            if (mode != PromptMode.NONE) {
                assertContains(result.finalPrompt, "tier=$tier")
                assertContains(result.finalPrompt, "mode=$mode")
                val expectedAccess = if (phoneControl) "attached" else "detached"
                assertContains(result.finalPrompt, "accessibility=$expectedAccess")
            }
        }

        executor.shutdown()

        // No two FULL results for different configs should have identical content
        val fullPairs = resultPairs.filter { it.first.first == PromptMode.FULL }
        for (i in fullPairs.indices) {
            for (j in i + 1 until fullPairs.size) {
                if (fullPairs[i].first != fullPairs[j].first) {
                    assertNotEquals(
                        "Results for different configs should differ",
                        fullPairs[i].second.finalPrompt,
                        fullPairs[j].second.finalPrompt
                    )
                }
            }
        }
    }

    // ── CT-H24-002: Concurrent telemetry emission produces well-formed lines ───

    @Test
    fun `CT-H24-002 concurrent runtime line emission is well-formed`() {
        val threadCount = 16
        val barrier = CyclicBarrier(threadCount)
        val lines = CopyOnWriteArrayList<String>()
        val executor = Executors.newFixedThreadPool(threadCount)

        val futures = (0 until threadCount).map { i ->
            executor.submit {
                barrier.await()
                val tier = ModelTier.entries[i % ModelTier.entries.size]
                val mode = PromptMode.entries[i % PromptMode.entries.size]
                val line = RuntimeLine.build(
                    modelName = "model-$i",
                    tier = tier,
                    mode = mode,
                    accessibility = if (i % 2 == 0) "attached" else "detached",
                    toolPolicy = "full",
                    promptChars = 1000 + i,
                    promptTokensEst = 250 + i,
                    trimmed = i % 3 == 0,
                    trimmedSections = if (i % 3 == 0) listOf("verbose_examples") else emptyList(),
                    timestamp = testTimestamp
                )
                lines.add(line)
            }
        }

        futures.forEach { it.get(10, TimeUnit.SECONDS) }
        executor.shutdown()

        assertEquals(threadCount, lines.size)
        for (line in lines) {
            assertTrue(
                "Runtime line should match schema: $line",
                RuntimeLine.SCHEMA_REGEX.matches(line)
            )
            val parsed = RuntimeLine.parse(line)
            assertNotNull("Should parse: $line", parsed)
            assertEquals(10, parsed!!.asMap().size)
        }
    }

    // ── Budget enforcement unit tests ───────────────────────────────────

    @Test
    fun `token estimation uses ceil of chars div 4`() {
        assertEquals(1, PromptBudget.estimateTokens(1))
        assertEquals(1, PromptBudget.estimateTokens(4))
        assertEquals(2, PromptBudget.estimateTokens(5))
        assertEquals(250, PromptBudget.estimateTokens(1000))
        // Zero-length prompt should estimate to zero tokens (boundary condition).
        assertEquals(0, PromptBudget.estimateTokens(0))
    }

    @Test
    fun `soft budget exceeded flag is set correctly`() {
        // Create prompt that exceeds soft but not hard budget for MINIMAL (900/1100)
        val sections = listOf(
            PromptBudget.LabeledSection(PromptBudget.SectionId.IDENTITY_BASELINE, "x".repeat(3800))
        )
        val result = PromptBudget.enforce(sections, PromptMode.MINIMAL)
        // 3800 chars = 950 tokens > 900 soft, < 1100 hard
        assertTrue("Should exceed soft budget", result.softBudgetExceeded)
        assertFalse("Should not trim (under hard budget)", result.trimmed)
    }

    // ── Feature flag integration ────────────────────────────────────────

    @Test
    fun `feature flag disabled uses legacy path`() {
        FeatureFlags.promptTuningV1Enabled = false
        val prompt = PhoneAgentPrompts.buildSystemPrompt(
            mode = PromptMode.FULL,
            modelName = "gpt-4o",
            modelTier = ModelTier.STANDARD
        )
        // Legacy path uses old runtime format
        assertContains(prompt, "Runtime: model=gpt-4o | tier=STANDARD")
        assertNotContains(prompt, "runtime|ts=")
    }

    @Test
    fun `feature flag enabled uses tuned path`() {
        FeatureFlags.promptTuningV1Enabled = true
        val prompt = PhoneAgentPrompts.buildSystemPrompt(
            mode = PromptMode.FULL,
            modelName = "gpt-4o",
            modelTier = ModelTier.STANDARD
        )
        assertContains(prompt, "runtime|ts=")
        assertContains(prompt, "Canonical Safety Clauses")
    }

    @Test
    fun `feature flag is included in resetToDefaults`() {
        FeatureFlags.promptTuningV1Enabled = true
        FeatureFlags.resetToDefaults()
        assertFalse(FeatureFlags.promptTuningV1Enabled)
    }

    // ── Safety contract unit tests ──────────────────────────────────────

    @Test
    fun `safety normalization collapses whitespace`() {
        assertEquals("a b c", PromptSafetyContract.normalize("a  b   c"))
    }

    @Test
    fun `safety normalization removes safe parentheticals`() {
        assertEquals(
            "Do something carefully.",
            PromptSafetyContract.normalize("Do something (like always) carefully.")
        )
    }

    @Test
    fun `safety normalization preserves modal verb parentheticals`() {
        val text = "Action (must not skip) required."
        val normalized = PromptSafetyContract.normalize(text)
        assertContains(normalized, "must not")
    }

    @Test
    fun `safety normalization converts semicolons to periods`() {
        assertEquals("a. b", PromptSafetyContract.normalize("a; b"))
    }

    // ── Helper ──────────────────────────────────────────────────────────

    private fun assertContains(text: String, substring: String) {
        assertTrue("Expected to find '$substring' in text (length=${text.length})", text.contains(substring))
    }

    private fun assertNotContains(text: String, substring: String) {
        assertFalse("Expected NOT to find '$substring'", text.contains(substring))
    }
}
